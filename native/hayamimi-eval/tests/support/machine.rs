//! RP-83: the judgement of the machine record of a run, `<run>.machine.jsonl`.
//!
//! `check_runs` takes the runs of one measurement and judges every Rust run (a
//! Python baseline is never judged, whether it has a file or not). A Rust run
//! passes when its file stands, has two rows or more, every field of every row
//! is readable, and:
//!
//! - the first row has `load1` below 2.00 and `load15` below 2.50;
//! - the last row has `load5` below 2.50;
//! - no row has a `cargo` or a `rustc`;
//! - no two rows are more than 15 s apart, and time does not go back.
//!
//! The three loads are JSON strings of two decimals and are compared as
//! hundredths. A failure names the run and the value that failed.
//!
//! A Rust run without a file is exempt only when it is a record measured
//! before the gate: its (directory, name, sha256 of `<name>.jsonl`) is one of
//! [`PRE_GATE`]. The directory is read from the repository root that
//! `check_runs` is given, so a test can stand a copy of the repository on a
//! directory of its own. A run that has a file is judged whether it is listed
//! or not.

use std::path::{Path, PathBuf};

use hayamimi_eval::machine::{LOAD1_LIMIT, LOAD5_LIMIT, LOAD15_LIMIT, MAX_GAP_US, hundredths};
use hayamimi_eval::sha::sha256_hex;
use serde_json::{Value, json};

use super::Failure;

/// The runs measured before the gate, which have no machine file:
/// (directory from the repository root, run, sha256 of `<run>.jsonl`).
pub const PRE_GATE: [(&str, &str, &str); 5] = [
    (
        "docs/results/rp32",
        "rs1",
        "79a297c5821ae1b675785813382c349bc70c2d40c5f9255a5e14aa5e8d931931",
    ),
    (
        "docs/results/rp32",
        "rs2",
        "34927c1397293b31d42cae60d74c8fd67cba70c98e4e98370860ae157a39a0c2",
    ),
    (
        "docs/results/rp32",
        "rs3",
        "e7cfbf9c5328f8c177d38479a11492259dcfc08ed90582bcc78bfc14675fa003",
    ),
    (
        "docs/results/e2e/e2e1",
        "e2e1",
        "6064c1a86c81ce9e35b251516c17ca40e8c81e7ff116e619497f26693bdde653",
    ),
    (
        "docs/results/e2e/e2e2",
        "e2e2",
        "fe214b909ee7675efd2b247d7af5a58fd16e189817ff4cb3b0e71d65071e17ae",
    ),
];

/// One run of a measurement.
#[derive(Clone, Debug)]
pub struct RunInput {
    /// The directory `<name>.jsonl` stands in; it decides whether a run without a file is listed.
    pub dir: PathBuf,
    pub name: String,
    /// A Python baseline is not judged.
    pub rust: bool,
    /// The text of `<name>.machine.jsonl`; `None` when the file does not stand.
    pub machine: Option<String>,
}

impl RunInput {
    pub fn rust(dir: &Path, name: &str, machine: Option<String>) -> RunInput {
        RunInput {
            dir: dir.to_path_buf(),
            name: name.to_owned(),
            rust: true,
            machine,
        }
    }

    pub fn python(dir: &Path, name: &str, machine: Option<String>) -> RunInput {
        RunInput {
            rust: false,
            ..RunInput::rust(dir, name, machine)
        }
    }
}

/// What a passing measurement came to: the runs let through as records of
/// before the gate.
#[derive(Debug, PartialEq, Eq)]
pub struct GateReport {
    pub exempt: Vec<String>,
}

impl GateReport {
    /// The line the output of a real run carries.
    pub fn render(&self) -> String {
        if self.exempt.is_empty() {
            return String::from("RP-83: no run was let through as a pre-gate record");
        }
        format!(
            "RP-83: pre-gate records (no machine file, listed by directory, name and sha256): {}",
            self.exempt.join(", ")
        )
    }
}

/// A load of a row: the text as written and its value in hundredths.
struct Load {
    text: String,
    value: u32,
}

struct Row {
    t_us: i64,
    load1: Load,
    load5: Load,
    load15: Load,
    cargo: u64,
    rustc: u64,
}

fn field<'a>(
    map: &'a serde_json::Map<String, Value>,
    name: &str,
    number: usize,
    key: &str,
) -> Result<&'a Value, Failure> {
    map.get(key)
        .ok_or_else(|| format!("{name}: row {number} has no {key}"))
}

fn count_field(
    map: &serde_json::Map<String, Value>,
    name: &str,
    number: usize,
    key: &str,
) -> Result<u64, Failure> {
    let value = field(map, name, number, key)?;
    value.as_u64().ok_or_else(|| {
        format!("{name}: row {number} {key} is not a non-negative integer ({value})")
    })
}

fn load_field(
    map: &serde_json::Map<String, Value>,
    name: &str,
    number: usize,
    key: &str,
) -> Result<Load, Failure> {
    let value = field(map, name, number, key)?;
    let Some(text) = value.as_str() else {
        return Err(format!(
            "{name}: row {number} {key} is not a JSON string ({value})"
        ));
    };
    let Some(value) = hundredths(text) else {
        return Err(format!(
            "{name}: row {number} {key} {text:?} is not a load with two decimals"
        ));
    };
    Ok(Load {
        text: text.to_owned(),
        value,
    })
}

fn parse_row(name: &str, number: usize, line: &str) -> Result<Row, Failure> {
    let value: Value = serde_json::from_str(line)
        .map_err(|why| format!("{name}: row {number} is not readable JSON ({why})"))?;
    let Value::Object(map) = &value else {
        return Err(format!("{name}: row {number} is not a JSON object"));
    };
    let t_us = field(map, name, number, "t_us")?;
    let Some(t_us) = t_us.as_i64() else {
        return Err(format!(
            "{name}: row {number} t_us is not an integer ({t_us})"
        ));
    };
    Ok(Row {
        t_us,
        load1: load_field(map, name, number, "load1")?,
        load5: load_field(map, name, number, "load5")?,
        load15: load_field(map, name, number, "load15")?,
        cargo: count_field(map, name, number, "cargo")?,
        rustc: count_field(map, name, number, "rustc")?,
    })
}

fn parse_rows(name: &str, text: &str) -> Result<Vec<Row>, Failure> {
    text.lines()
        .enumerate()
        .map(|(index, line)| parse_row(name, index + 1, line))
        .collect()
}

/// A limit in hundredths as it is written: 250 is `2.50`.
fn written(limit: u32) -> String {
    format!("{}.{:02}", limit / 100, limit % 100)
}

fn judge_loads(name: &str, first: &Row, last: &Row) -> Vec<String> {
    let mut problems = Vec::new();
    if first.load1.value >= LOAD1_LIMIT {
        problems.push(format!(
            "{name}: load1 {} in the first row is not below {}",
            first.load1.text,
            written(LOAD1_LIMIT)
        ));
    }
    if first.load15.value >= LOAD15_LIMIT {
        problems.push(format!(
            "{name}: load15 {} in the first row is not below {}",
            first.load15.text,
            written(LOAD15_LIMIT)
        ));
    }
    if last.load5.value >= LOAD5_LIMIT {
        problems.push(format!(
            "{name}: load5 {} in the last row is not below {}",
            last.load5.text,
            written(LOAD5_LIMIT)
        ));
    }
    problems
}

fn judge_builds(name: &str, rows: &[Row]) -> Vec<String> {
    let mut problems = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        if row.cargo > 0 {
            problems.push(format!("{name}: cargo {} in row {}", row.cargo, index + 1));
        }
        if row.rustc > 0 {
            problems.push(format!("{name}: rustc {} in row {}", row.rustc, index + 1));
        }
    }
    problems
}

fn judge_gaps(name: &str, rows: &[Row]) -> Vec<String> {
    let mut problems = Vec::new();
    for (index, pair) in rows.windows(2).enumerate() {
        let (before, after) = (pair[0].t_us, pair[1].t_us);
        let gap = after - before;
        if gap < 0 {
            problems.push(format!(
                "{name}: t_us goes back between rows {} and {} ({before} to {after})",
                index + 1,
                index + 2
            ));
        } else if gap > MAX_GAP_US {
            problems.push(format!(
                "{name}: rows {} and {} are {gap} us apart, the limit is {MAX_GAP_US}",
                index + 1,
                index + 2
            ));
        }
    }
    problems
}

/// Every way the rows of a run fail, in the order loads, builds, gaps.
fn judge_rows(name: &str, rows: &[Row]) -> Vec<String> {
    let (Some(first), Some(last)) = (rows.first(), rows.last()) else {
        return vec![format!("{name}: has no rows")];
    };
    if rows.len() < 2 {
        return vec![format!("{name}: has 1 row, fewer than 2 rows")];
    }
    let mut problems = judge_loads(name, first, last);
    problems.extend(judge_builds(name, rows));
    problems.extend(judge_gaps(name, rows));
    problems
}

/// `dir` below the repository root, or `None` when `dir` cannot be resolved or
/// stands outside it.
pub fn below_root(repo_root: &Path, dir: &Path) -> Option<PathBuf> {
    let root = repo_root.canonicalize().ok()?;
    let full = dir.canonicalize().ok()?;
    full.strip_prefix(&root).ok().map(Path::to_path_buf)
}

/// Whether the run is one of [`PRE_GATE`]: its directory, name and the sha256
/// of its samples file all agree. A samples file that cannot be read is not.
fn is_pre_gate(repo_root: &Path, run: &RunInput) -> bool {
    let samples = run.dir.join(format!("{}.jsonl", run.name));
    std::fs::read(samples)
        .ok()
        .zip(below_root(repo_root, &run.dir))
        .is_some_and(|(bytes, dir)| {
            let sha = sha256_hex(&bytes);
            PRE_GATE.iter().any(|(listed, name, hash)| {
                Path::new(listed) == dir && *name == run.name && *hash == sha
            })
        })
}

enum Verdict {
    Passed,
    /// A record of before the gate, let through for want of a file.
    Exempt,
}

fn judge_run(repo_root: &Path, run: &RunInput) -> Result<Verdict, Failure> {
    let Some(text) = &run.machine else {
        if is_pre_gate(repo_root, run) {
            return Ok(Verdict::Exempt);
        }
        return Err(format!(
            "{}: has no machine file ({}.machine.jsonl)",
            run.name, run.name
        ));
    };
    let problems = judge_rows(&run.name, &parse_rows(&run.name, text)?);
    if problems.is_empty() {
        return Ok(Verdict::Passed);
    }
    Err(problems.join("; "))
}

/// RP-83 over the runs of one measurement. The runs that fail are all named, one
/// after the other.
pub fn check_runs(repo_root: &Path, runs: &[RunInput]) -> Result<GateReport, Failure> {
    let mut exempt = Vec::new();
    let mut failures = Vec::new();
    for run in runs.iter().filter(|run| run.rust) {
        match judge_run(repo_root, run) {
            Ok(Verdict::Passed) => {}
            Ok(Verdict::Exempt) => exempt.push(run.name.clone()),
            Err(why) => failures.push(why),
        }
    }
    if failures.is_empty() {
        return Ok(GateReport { exempt });
    }
    Err(failures.join("; "))
}

// ---- F83, the record the tests build by a formula (T 4.75) ----

/// The five rows of F83: `t_us` 0 to 20 s every 5 s, every load `0.50`, no build.
pub fn f83_rows() -> Vec<Value> {
    (0..5_i64)
        .map(|index| {
            json!({
                "t_us": index * 5_000_000,
                "load1": "0.50",
                "load5": "0.50",
                "load15": "0.50",
                "cargo": 0,
                "rustc": 0,
            })
        })
        .collect()
}

/// Rows as the text of a file: one line each, a newline at the end.
pub fn text_of(rows: &[Value]) -> String {
    rows.iter().map(|row| format!("{row}\n")).collect()
}

/// F83 as the text of `rs1.machine.jsonl`.
pub fn f83_text() -> String {
    text_of(&f83_rows())
}

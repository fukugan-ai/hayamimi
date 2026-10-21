//! RP-83: the judgement of the machine record of a run, `<run>.machine.jsonl`.
//!
//! `check_runs` takes the runs of one measurement and judges every Rust run (a
//! Python baseline is never judged, whether it has a file or not). A Rust run
//! passes when its file stands, has two rows or more, every field of every row
//! is readable, and:
//!
//! - the first row has `load1` below 2.00 and `load15` below 2.50;
//! - the last row has `load5` below 2.50 once the run's own share is taken off;
//! - no row has a `cargo` or a `rustc`;
//! - no two rows are more than 15 s apart, and time does not go back.
//!
//! The three loads are JSON strings of two decimals and are compared as
//! hundredths. A failure names the run and the value that failed.
//!
//! A file whose first row has `own` names the run's own processes. Its rows
//! then all have `own_cpu_ticks` (a JSON integer that never falls from one row
//! to the next) and its first row has `clk_tck`. The rate of the ticks between
//! two rows, in cores, is smoothed the way the kernel smooths `load5` (a
//! 300 s exponential average, starting at 0 in the first row), and the last
//! row's `load5` less that average, both in hundredths and the average cut
//! down to a whole hundredth, must be below 2.50. A file without `own` is
//! judged on `load5` as it stands.
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
    /// `own_cpu_ticks`, in a file that names the run's own processes.
    own_ticks: Option<u64>,
    /// `clk_tck` of the first row of such a file.
    clk_tck: Option<u64>,
}

/// The rows of a file, and the ticks a second of its `own_cpu_ticks` when the
/// file names the run's own processes.
struct Record {
    rows: Vec<Row>,
    clk_tck: Option<u64>,
}

/// The window of the kernel's 5 minute load average, in seconds.
const LOAD5_WINDOW_S: f64 = 300.0;

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

/// The two own fields of a row of a file that names the run's own processes:
/// `own_cpu_ticks` always, and `clk_tck` (not zero) in the first row.
fn own_fields(
    map: &serde_json::Map<String, Value>,
    name: &str,
    number: usize,
) -> Result<(Option<u64>, Option<u64>), Failure> {
    let ticks = count_field(map, name, number, "own_cpu_ticks")?;
    if number != 1 {
        return Ok((Some(ticks), None));
    }
    let clk_tck = count_field(map, name, number, "clk_tck")?;
    if clk_tck == 0 {
        return Err(format!("{name}: row {number} clk_tck is 0"));
    }
    Ok((Some(ticks), Some(clk_tck)))
}

/// `own_mode`: the first row of the file has `own`.
fn parse_row(name: &str, number: usize, line: &str, own_mode: bool) -> Result<Row, Failure> {
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
    let (own_ticks, clk_tck) = if own_mode {
        own_fields(map, name, number)?
    } else {
        (None, None)
    };
    Ok(Row {
        t_us,
        load1: load_field(map, name, number, "load1")?,
        load5: load_field(map, name, number, "load5")?,
        load15: load_field(map, name, number, "load15")?,
        cargo: count_field(map, name, number, "cargo")?,
        rustc: count_field(map, name, number, "rustc")?,
        own_ticks,
        clk_tck,
    })
}

/// Whether the first row of the file has `own`. A first row that cannot be
/// read is not one: reading it as a row reports why.
fn names_own_processes(text: &str) -> bool {
    text.lines()
        .next()
        .and_then(|line| serde_json::from_str::<Value>(line).ok())
        .is_some_and(|row| row.get("own").is_some())
}

fn parse_rows(name: &str, text: &str) -> Result<Record, Failure> {
    let own_mode = names_own_processes(text);
    let rows = text
        .lines()
        .enumerate()
        .map(|(index, line)| parse_row(name, index + 1, line, own_mode))
        .collect::<Result<Vec<Row>, Failure>>()?;
    let clk_tck = rows.first().and_then(|row| row.clk_tck);
    Ok(Record { rows, clk_tck })
}

/// The run's own share of `load5` at the last row, in cores: the rate of
/// `own_cpu_ticks` between two rows, smoothed the way the kernel smooths
/// `load5` (`share = share * e^(-dt/300) + rate * (1 - e^(-dt/300))`, starting
/// at 0 in the first row). Rows at one time, or going back, add nothing.
fn own_share(rows: &[Row], clk_tck: u64) -> f64 {
    let mut share = 0.0_f64;
    for pair in rows.windows(2) {
        let dt = (pair[1].t_us - pair[0].t_us) as f64 / 1_000_000.0;
        let (Some(before), Some(after)) = (pair[0].own_ticks, pair[1].own_ticks) else {
            continue;
        };
        if dt <= 0.0 {
            continue;
        }
        let rate = after.saturating_sub(before) as f64 / (clk_tck as f64 * dt);
        let keep = (-dt / LOAD5_WINDOW_S).exp();
        share = share * keep + rate * (1.0 - keep);
    }
    share
}

/// The share cut down to a whole hundredth. It is cut and not rounded, so that
/// a share of 0.609 takes off 60 and not 61: less is taken off, and the gate
/// refuses more.
fn cut_to_hundredths(share: f64) -> i64 {
    (share * 100.0).floor() as i64
}

/// A limit in hundredths as it is written: 250 is `2.50`.
fn written(limit: u32) -> String {
    format!("{}.{:02}", limit / 100, limit % 100)
}

/// The last row's `load5` against its limit: as it stands, or less the run's
/// own share when the file names the run's own processes.
fn judge_load5(name: &str, rows: &[Row], last: &Row, clk_tck: Option<u64>) -> Option<String> {
    let limit = i64::from(LOAD5_LIMIT);
    let Some(clk_tck) = clk_tck else {
        return (last.load5.value >= LOAD5_LIMIT).then(|| {
            format!(
                "{name}: load5 {} in the last row is not below {}",
                last.load5.text,
                written(LOAD5_LIMIT)
            )
        });
    };
    let share = own_share(rows, clk_tck);
    let judged = i64::from(last.load5.value) - cut_to_hundredths(share);
    (judged >= limit).then(|| {
        format!(
            "{name}: load5 {} in the last row, less {} hundredths of its own share ({share:.6}), \
             leaves {judged} and not below {limit}",
            last.load5.text,
            cut_to_hundredths(share),
        )
    })
}

fn judge_loads(name: &str, rows: &[Row], clk_tck: Option<u64>) -> Vec<String> {
    let (Some(first), Some(last)) = (rows.first(), rows.last()) else {
        return Vec::new();
    };
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
    problems.extend(judge_load5(name, rows, last, clk_tck));
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

/// A row whose `own_cpu_ticks` is below the row before it: a process of the run
/// has ended, or the record is broken.
fn judge_own_ticks(name: &str, rows: &[Row]) -> Vec<String> {
    let mut problems = Vec::new();
    for (index, pair) in rows.windows(2).enumerate() {
        if let (Some(before), Some(after)) = (pair[0].own_ticks, pair[1].own_ticks)
            && after < before
        {
            problems.push(format!(
                "{name}: row {} own_cpu_ticks {after} is below {before} of row {}",
                index + 2,
                index + 1
            ));
        }
    }
    problems
}

/// Every way the rows of a run fail, in the order loads, builds, gaps, own ticks.
fn judge_rows(name: &str, record: &Record) -> Vec<String> {
    let rows = record.rows.as_slice();
    if rows.is_empty() {
        return vec![format!("{name}: has no rows")];
    }
    if rows.len() < 2 {
        return vec![format!("{name}: has 1 row, fewer than 2 rows")];
    }
    let mut problems = judge_loads(name, rows, record.clk_tck);
    problems.extend(judge_builds(name, rows));
    problems.extend(judge_gaps(name, rows));
    problems.extend(judge_own_ticks(name, rows));
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

/// The line a run's judgement reports, passed or not:
/// `<run> load5 <last load5> own5 <share, 6 decimals> (<cut hundredths>) judged <left> limit 250`,
/// or `<run> load5 <last load5> own absent judged <load5 hundredths> limit 250`
/// for a file that does not name the run's own processes.
fn gate_line(name: &str, record: &Record) -> Option<String> {
    let last = record.rows.last()?;
    let limit = LOAD5_LIMIT;
    let load5 = &last.load5;
    Some(match record.clk_tck {
        Some(clk_tck) => {
            let share = own_share(&record.rows, clk_tck);
            let cut = cut_to_hundredths(share);
            let judged = i64::from(load5.value) - cut;
            format!(
                "{name} load5 {} own5 {share:.6} ({cut}) judged {judged} limit {limit}",
                load5.text
            )
        }
        None => format!(
            "{name} load5 {} own absent judged {} limit {limit}",
            load5.text, load5.value
        ),
    })
}

/// The report lines of the Rust runs whose file can be read, in the order given.
pub fn gate_lines(runs: &[RunInput]) -> Vec<String> {
    runs.iter()
        .filter(|run| run.rust)
        .filter_map(|run| {
            let record = parse_rows(&run.name, run.machine.as_deref()?).ok()?;
            gate_line(&run.name, &record)
        })
        .collect()
}

/// RP-83 over the runs of one measurement. The runs that fail are all named, one
/// after the other. A line for each run with a readable file goes to the output
/// first (see [`gate_lines`]).
pub fn check_runs(repo_root: &Path, runs: &[RunInput]) -> Result<GateReport, Failure> {
    for line in gate_lines(runs) {
        println!("{line}");
    }
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

// ---- O83, a record that names the run's own processes (T 4.75) ----

/// The three processes O83 and the two-row file name.
fn o83_own() -> Value {
    json!([
        {"pid": 4242, "comm": "hayamimi"},
        {"pid": 4243, "comm": "hayamimi-relay"},
        {"pid": 4244, "comm": "llama-server"},
    ])
}

/// A row of a file that names the run's own processes: no build, loads `0.50`,
/// `own_cpu_ticks` as given, `cpu_some_total` unreadable (`null`).
fn own_row(t_us: u64, ticks: u64) -> Value {
    json!({
        "t_us": t_us,
        "load1": "0.50",
        "load5": "0.50",
        "load15": "0.50",
        "cargo": 0,
        "rustc": 0,
        "own_cpu_ticks": ticks,
        "cpu_some_total": null,
    })
}

/// The first row also names the processes and the ticks a second.
fn name_the_processes(row: &mut Value) {
    row["own"] = o83_own();
    row["clk_tck"] = json!(100);
}

/// O83: 106 rows 5 s apart; `own_cpu_ticks` rises by `per_row` a row; the first
/// row has `load1` 1.72 and `load15` 2.13 and the last row has `load5` 3.10.
pub fn o83_rows(per_row: u64) -> Vec<Value> {
    let mut rows: Vec<Value> = (0..106_u64)
        .map(|index| own_row(index * 5_000_000, index * per_row))
        .collect();
    name_the_processes(&mut rows[0]);
    rows[0]["load1"] = json!("1.72");
    rows[0]["load15"] = json!("2.13");
    rows[105]["load5"] = json!("3.10");
    rows
}

/// The two-row file of T-RP-83-059 and 060: 15 s apart, 15000 ticks in between
/// (10 cores), the second row's `load5` as given.
pub fn two_o83_rows(last_load5: &str) -> Vec<Value> {
    let mut rows = vec![own_row(0, 0), own_row(15_000_000, 15_000)];
    name_the_processes(&mut rows[0]);
    rows[0]["load1"] = json!("1.00");
    rows[0]["load15"] = json!("1.00");
    rows[1]["load5"] = json!(last_load5);
    rows
}

/// The rows without the four fields a run that names its processes adds.
pub fn without_own_fields(mut rows: Vec<Value>) -> Vec<Value> {
    for row in &mut rows {
        if let Value::Object(map) = row {
            for key in ["own", "clk_tck", "own_cpu_ticks", "cpu_some_total"] {
                map.remove(key);
            }
        }
    }
    rows
}

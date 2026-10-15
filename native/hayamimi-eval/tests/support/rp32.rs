//! RP-32: the judgement of Python and Rust runs measured one after the other
//! under the same load.
//!
//! `check_alternated_runs` takes the runs in the order they were measured.
//! It checks the order (Python, Rust, Python, Rust, ...), the number of
//! pairs, the load before each run, and then judges the samples of the three
//! pairs pooled: the `L` p50 and p95 of Rust are no greater than Python's, and
//! enough utterances of the comparison set are as fast. The validity of a
//! sample, the percentiles and the paired threshold come from
//! `hayamimi_eval::compare`, the functions RP-3 uses.
//!
//! The runs stand in `docs/results/rp32/`: `py<N>.jsonl` and `rs<N>.jsonl`
//! (the samples `stream --out` wrote) and `load<N>.txt` for each pair N. A line
//! of `load<N>.txt` is `<run> before|after <time> <a line of /proc/loadavg>`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use hayamimi_eval::compare::{check_paired, check_percentiles, is_valid_l_asr, read_samples};
use hayamimi_eval::latency::Sample;
use hayamimi_eval::percentile::nearest_rank;

use super::e2e::{Columns, MIN_SILENCE_MS, columns_of};
use super::{Failure, RESULTS, read};

/// A run starts only under a 1 minute load below this, in hundredths (2.00).
const LOAD_LIMIT_HUNDREDTHS: i64 = 200;
/// Pairs a complete measurement has; two are accepted when the count is recorded.
const PAIRS: usize = 3;
const FEWEST_PAIRS: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Implementation {
    Python,
    Rust,
}

/// One run: who ran, the `/proc/loadavg` lines before and after, the samples.
#[derive(Clone, Debug)]
pub struct AltRun {
    pub implementation: Implementation,
    pub load_before: Option<String>,
    pub load_after: Option<String>,
    pub samples: Vec<Sample>,
}

/// What the three pairs pooled came to.
#[derive(Debug)]
pub struct AltReport {
    pub pairs: usize,
    pub python: Columns,
    pub rust: Columns,
    pub comparison_set: Vec<u32>,
    pub not_slower: usize,
    /// The 1 minute load after each run, in hundredths. Recorded, not judged.
    pub loads_after: Vec<i64>,
}

/// The 1 minute load of a `/proc/loadavg` line in hundredths: `1.99` is 199.
pub fn load_hundredths(line: &str) -> Result<i64, Failure> {
    let Some(first) = line.split_whitespace().next() else {
        return Err(String::from("the load line is empty"));
    };
    let Some((whole, fraction)) = first.split_once('.') else {
        return Err(format!("{first:?} is not a load with two decimals"));
    };
    if fraction.len() != 2 {
        return Err(format!("{first:?} is not a load with two decimals"));
    }
    let number = |text: &str| {
        text.parse::<i64>()
            .map_err(|_| format!("{first:?} is not a load"))
    };
    Ok(number(whole)? * 100 + number(fraction)?)
}

fn check_order(runs: &[AltRun]) -> Result<(), Failure> {
    for (index, run) in runs.iter().enumerate() {
        let expected = if index % 2 == 0 {
            Implementation::Python
        } else {
            Implementation::Rust
        };
        if run.implementation != expected {
            return Err(format!(
                "run {} is {:?}, the order is Python, Rust, Python, Rust, ...",
                index + 1,
                run.implementation
            ));
        }
    }
    if runs.len() % 2 == 1 {
        return Err(format!(
            "run {} has no Rust run after it: the last pair is unfinished",
            runs.len()
        ));
    }
    Ok(())
}

fn check_pair_count(pairs: usize, recorded: Option<usize>) -> Result<(), Failure> {
    if pairs >= PAIRS || (pairs == FEWEST_PAIRS && recorded == Some(pairs)) {
        return Ok(());
    }
    if pairs == FEWEST_PAIRS {
        return Err(format!(
            "{pairs} pairs are accepted only with the count recorded; no count of {pairs} is recorded"
        ));
    }
    Err(format!(
        "{pairs} pairs: at least {FEWEST_PAIRS} pairs (with the count recorded) are needed"
    ))
}

/// The load of the run `number` (1-based) in hundredths, from its recorded line.
fn recorded_load(line: &Option<String>, number: usize, when: &str) -> Result<i64, Failure> {
    let Some(line) = line else {
        return Err(format!("run {number} has no load {when} record"));
    };
    load_hundredths(line).map_err(|why| format!("run {number}, load {when}: {why}"))
}

/// Every run was started under a load below 2.00 and both records are readable;
/// the loads after each run, in order.
fn check_loads(runs: &[AltRun]) -> Result<Vec<i64>, Failure> {
    let mut after = Vec::new();
    for (index, run) in runs.iter().enumerate() {
        let number = index + 1;
        let before = recorded_load(&run.load_before, number, "before")?;
        if before >= LOAD_LIMIT_HUNDREDTHS {
            return Err(format!(
                "run {number} started under a load of {}.{:02}, the limit is below 2.00",
                before / 100,
                before % 100
            ));
        }
        after.push(recorded_load(&run.load_after, number, "after")?);
    }
    Ok(after)
}

fn pooled(runs: &[AltRun], who: Implementation) -> Vec<Sample> {
    runs.iter()
        .filter(|run| run.implementation == who)
        .flat_map(|run| run.samples.iter().cloned())
        .collect()
}

/// `L` in microseconds of the valid samples with a translation, by utterance.
fn l_by_utterance(samples: &[Sample]) -> BTreeMap<u32, Vec<i64>> {
    let mut found: BTreeMap<u32, Vec<i64>> = BTreeMap::new();
    let valid = samples
        .iter()
        .filter(|sample| is_valid_l_asr(sample, MIN_SILENCE_MS));
    for sample in valid {
        if let Some(l) = sample.l_us() {
            found.entry(sample.utt).or_default().push(l);
        }
    }
    found
}

/// The utterances that have a valid sample with a translation in every Python
/// run. Python's runs alone fix it; Rust's samples cannot move it.
pub fn l_comparison_set(python_runs: &[&[Sample]]) -> Vec<u32> {
    let mut counts: BTreeMap<u32, usize> = BTreeMap::new();
    for run in python_runs {
        for utt in l_by_utterance(run).keys() {
            *counts.entry(*utt).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .filter(|(_, runs_with_it)| *runs_with_it == python_runs.len())
        .map(|(utt, _)| utt)
        .collect()
}

fn medians(samples: &[Sample]) -> Result<BTreeMap<u32, i64>, Failure> {
    l_by_utterance(samples)
        .into_iter()
        .map(|(utt, values)| {
            let median = nearest_rank(&values, 50).map_err(|why| why.to_string())?;
            Ok((utt, median))
        })
        .collect()
}

fn not_slower(set: &[u32], python: &BTreeMap<u32, i64>, rust: &BTreeMap<u32, i64>) -> usize {
    set.iter()
        .filter(|utt| matches!((python.get(*utt), rust.get(*utt)), (Some(p), Some(r)) if r <= p))
        .count()
}

/// The failures of the pooled judgement: the percentiles, then the paired comparison.
fn judge_pooled(report: &AltReport) -> Vec<String> {
    let mut failures = Vec::new();
    match (report.python.l, report.rust.l) {
        (Some(python), Some(rust)) => {
            failures.extend(
                check_percentiles("L", &python, &rust)
                    .err()
                    .map(|why| why.to_string()),
            );
        }
        _ => failures.push(String::from(
            "L: a side has no valid sample with a translation",
        )),
    }
    failures.extend(
        check_paired(report.comparison_set.len(), report.not_slower)
            .err()
            .map(|why| why.to_string()),
    );
    failures
}

/// RP-32: `runs` in the order they were measured; `recorded_pairs` is the
/// pair count written in the result when fewer than three fitted.
pub fn check_alternated_runs(
    runs: &[AltRun],
    recorded_pairs: Option<usize>,
) -> Result<AltReport, Failure> {
    check_order(runs)?;
    check_pair_count(runs.len() / 2, recorded_pairs)?;
    let loads_after = check_loads(runs)?;
    let (python, rust) = (
        pooled(runs, Implementation::Python),
        pooled(runs, Implementation::Rust),
    );
    let python_runs: Vec<&[Sample]> = runs
        .iter()
        .filter(|run| run.implementation == Implementation::Python)
        .map(|run| run.samples.as_slice())
        .collect();
    let comparison_set = l_comparison_set(&python_runs);
    let not_slower = not_slower(&comparison_set, &medians(&python)?, &medians(&rust)?);
    let report = AltReport {
        pairs: runs.len() / 2,
        python: columns_of(&python, MIN_SILENCE_MS)?,
        rust: columns_of(&rust, MIN_SILENCE_MS)?,
        comparison_set,
        not_slower,
        loads_after,
    };
    let failures = judge_pooled(&report);
    if failures.is_empty() {
        return Ok(report);
    }
    Err(failures.join("; "))
}

/// The columns of a side, one per line. The "translation stage" (from the
/// `final` to `t_tr`) is the same quantity as `L_tr` (3.1) and has its own line.
pub fn render_columns(columns: &Columns) -> String {
    let line = |name: &str, stat: Option<hayamimi_eval::latency::Stat>| match stat {
        Some(stat) => format!(
            "{name} p50 {} p95 {} ms (n {})",
            stat.p50_ms, stat.p95_ms, stat.n
        ),
        None => format!("{name} no sample"),
    };
    [
        line("L", columns.l),
        line("L_asr", columns.l_asr),
        line("L_tr", columns.l_tr),
        line("translation stage", columns.l_tr),
    ]
    .join("\n")
}

/// The output names every column the design lists.
pub fn check_columns_written(text: &str) -> Result<(), Failure> {
    for name in ["L", "L_asr", "L_tr", "translation stage"] {
        let prefix = format!("{name} p50");
        if !text.lines().any(|line| line.starts_with(&prefix)) {
            return Err(format!("the column {name} is missing from the output"));
        }
    }
    Ok(())
}

/// Where the runs of RP-32 stand.
pub fn rp32_dir() -> PathBuf {
    Path::new(RESULTS).join("rp32")
}

/// How many pairs a directory is read as, and the count its `pairs.txt` records.
#[derive(Debug, PartialEq, Eq)]
pub struct PairCount {
    pub pairs: usize,
    pub recorded: Option<usize>,
}

/// The pair count `pairs.txt` writes: one decimal number, alone on its line.
/// `1` is refused (two pairs are the fewest), and so is anything unreadable.
pub fn parse_pairs_text(text: &str) -> Result<usize, Failure> {
    let word = text.trim();
    if word.is_empty() || !word.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!(
            "pairs.txt holds {word:?}, not a pair count such as 2"
        ));
    }
    let pairs: usize = word
        .parse()
        .map_err(|_| format!("pairs.txt holds {word:?}, not a pair count such as 2"))?;
    if pairs < FEWEST_PAIRS {
        return Err(format!(
            "pairs.txt holds {pairs}: at least {FEWEST_PAIRS} pairs are needed"
        ));
    }
    Ok(pairs)
}

/// The highest pair number of which any file stands in `dir`; `None` when no pair's file does.
fn pairs_present(dir: &Path) -> Option<usize> {
    let stands = |n: usize| {
        [
            format!("py{n}.jsonl"),
            format!("rs{n}.jsonl"),
            format!("load{n}.txt"),
        ]
        .iter()
        .any(|name| dir.join(name).is_file())
    };
    (1..=9).filter(|n| stands(*n)).max()
}

/// How many pairs `dir` is read as. No `pairs.txt` means three pairs; a
/// `pairs.txt` means the number it records, which must be the number of pairs
/// whose files stand there. The count is never taken from the file names alone:
/// a directory that lost its third pair would look like one that stopped at two.
pub fn pairs_to_read(dir: &Path) -> Result<PairCount, Failure> {
    let path = dir.join("pairs.txt");
    if !path.is_file() {
        return Ok(PairCount {
            pairs: PAIRS,
            recorded: None,
        });
    }
    let recorded = parse_pairs_text(&read(&path)?)?;
    let present = pairs_present(dir);
    if present != Some(recorded) {
        let stand = present.map_or_else(|| String::from("no"), |n| n.to_string());
        return Err(format!(
            "pairs.txt records {recorded} pairs but {stand} pairs stand in {}",
            dir.display()
        ));
    }
    Ok(PairCount {
        pairs: recorded,
        recorded: Some(recorded),
    })
}

/// RP-32 over a directory of runs: the pair count from `pairs.txt` (three when
/// it is absent), the runs of that many pairs, and the judgement.
pub fn check_dir(dir: &Path) -> Result<AltReport, Failure> {
    let count = pairs_to_read(dir)?;
    let runs = load_alternated(dir, count.pairs)?;
    check_alternated_runs(&runs, count.recorded)
}

/// The run names of `pairs` pairs in the order they were measured.
fn run_names(pairs: usize) -> Vec<(String, Implementation)> {
    (1..=pairs)
        .flat_map(|n| {
            [
                (format!("py{n}"), Implementation::Python),
                (format!("rs{n}"), Implementation::Rust),
            ]
        })
        .collect()
}

/// The files a measurement of `pairs` pairs needs that `dir` does not hold.
pub fn missing_files(dir: &Path, pairs: usize) -> Vec<String> {
    let mut missing = Vec::new();
    for n in 1..=pairs {
        for name in [
            format!("py{n}.jsonl"),
            format!("rs{n}.jsonl"),
            format!("load{n}.txt"),
        ] {
            if !dir.join(&name).is_file() {
                missing.push(name);
            }
        }
    }
    missing
}

/// The load lines of a `load<N>.txt`, keyed by run name and `before` or `after`.
pub fn parse_load_file(text: &str) -> BTreeMap<(String, String), String> {
    let mut found = BTreeMap::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if let [run, when, _time, loadavg @ ..] = fields.as_slice() {
            found.insert((run.to_string(), when.to_string()), loadavg.join(" "));
        }
    }
    found
}

/// The runs of `pairs` pairs in `dir`, in measurement order. A missing file is
/// named in the error; a missing load line is left to the judgement.
pub fn load_alternated(dir: &Path, pairs: usize) -> Result<Vec<AltRun>, Failure> {
    let missing = missing_files(dir, pairs);
    if !missing.is_empty() {
        return Err(format!(
            "{} lacks {}: the E2E seat copies the alternated runs there",
            dir.display(),
            missing.join(", ")
        ));
    }
    let mut runs = Vec::new();
    for (name, implementation) in run_names(pairs) {
        let number = &name[2..];
        let loads = parse_load_file(&read(&dir.join(format!("load{number}.txt")))?);
        runs.push(AltRun {
            implementation,
            load_before: loads.get(&(name.clone(), String::from("before"))).cloned(),
            load_after: loads.get(&(name.clone(), String::from("after"))).cloned(),
            samples: read_samples(&dir.join(format!("{name}.jsonl")))
                .map_err(|why| why.to_string())?,
        });
    }
    Ok(runs)
}

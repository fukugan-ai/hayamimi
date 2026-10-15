//! RP-32: the judgement of Python and Rust runs measured one after the other
//! under the same load.
//!
//! `check_alternated_runs` takes the runs in the order they were measured.
//! It checks the order (Python, Rust, Python, Rust, ...), the number of
//! pairs, the load before each run, and then judges the samples of the three
//! pairs pooled: at least 20 / 23 of the valid Rust samples got a translation
//! (the three Rust runs counted together, not run by run), the `L` p50 and p95
//! of Rust are no greater than Python's, and enough utterances of the
//! comparison set are as fast. The share of Python is written beside Rust's and
//! not judged. The validity of a sample, the share, the percentiles and the
//! paired threshold come from `hayamimi_eval::compare`, the functions RP-3 uses.
//!
//! A Python sample is valid against `min_silence`. A Rust sample is valid
//! against the line of the rule that closed it (`rp32_rules`): the early
//! detector's silence for `early`, `min_silence` for the others. The report
//! also counts every Rust sample against `min_silence`, recorded and not judged.
//!
//! The runs stand in `docs/results/rp32/`: `py<N>.jsonl` and `rs<N>.jsonl`
//! (the samples `stream --out` wrote) and `load<N>.txt` for each pair N. A line
//! of `load<N>.txt` is `<run> before|after <time> <a line of /proc/loadavg>`.
//! Each Rust run also stands with `rs<N>.events.jsonl` (the events) and
//! `rs<N>.hayamimi.stdout.log` (the server's stdout, whose
//! `asr.endpoint.decided` lines give the rule of each `final`).
//! `check_dir` reads the events for RP-72 too: a `fallback` event with the
//! reason `in_use` means the server moved off the port the tool kept sending
//! to, and a run without its events file cannot be checked, so either refuses
//! the directory by the run's name.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use hayamimi_asr::endpoint::Rule;
use hayamimi_eval::compare::{
    TranslatedShare, check_paired, check_percentiles, check_share, comparison_threshold,
    is_valid_l_asr, read_samples, share_of_valid,
};
use hayamimi_eval::latency::Sample;
use hayamimi_eval::percentile::nearest_rank;
use serde_json::Value;

use super::e2e::{Columns, MIN_SILENCE_MS, columns_over};
use super::rp32_rules::{RULES, closing_rules, silence_line_ms};
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
    /// The rule that closed each sample that has a `final`, by `(repeat, utt)`.
    /// `None`: the run records none, and every sample is judged against
    /// `min_silence` (Python, and the runs the tests build by a formula).
    /// `Some`: each sample is judged against the line of its rule, and a sample
    /// with a `final` and no entry here is refused.
    pub rules: Option<BTreeMap<(u32, u32), Rule>>,
}

/// The pooled Rust counts with every sample judged against `min_silence`.
/// Recorded, not judged.
#[derive(Debug)]
pub struct FlatCounts {
    pub not_slower: usize,
    pub rust_share: TranslatedShare,
}

/// Numbers beside the speed judgement: how a side's utterances were cut and
/// recognized. Counted over every sample of the side, valid or not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Quality {
    pub samples: usize,
    /// Samples with two or more `final`s.
    pub split: usize,
    /// Samples with no closing `final`.
    pub no_final: usize,
    pub cer_dist: usize,
    pub cer_denom: usize,
}

/// What the three pairs pooled came to. The judged fields count each Rust
/// sample against the line of its rule.
#[derive(Debug)]
pub struct AltReport {
    pub pairs: usize,
    pub python: Columns,
    pub rust: Columns,
    pub comparison_set: Vec<u32>,
    pub not_slower: usize,
    /// The valid samples of the three Rust runs pooled and those with a translation. Judged.
    pub rust_share: TranslatedShare,
    /// The same count for the Python runs. Recorded, not judged.
    pub python_share: TranslatedShare,
    /// The 1 minute load after each run, in hundredths. Recorded, not judged.
    pub loads_after: Vec<i64>,
    pub flat: FlatCounts,
    /// Rust samples valid against their own line and not against
    /// `min_silence`, by the name of the rule that closed them (every rule is
    /// a key, with 0 when none). Recorded, not judged.
    pub gained: BTreeMap<&'static str, usize>,
    pub python_quality: Quality,
    pub rust_quality: Quality,
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

/// Which silence line a sample is judged against.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Lines {
    /// `min_silence` for every sample.
    Flat,
    /// The line of the rule that closed the sample, `min_silence` when the run records none.
    PerRow,
}

/// A sample and the rule that closed it, when its run records one.
struct Closed<'a> {
    sample: &'a Sample,
    rule: Option<Rule>,
}

impl Closed<'_> {
    fn line_ms(&self, lines: Lines) -> i64 {
        match (lines, self.rule) {
            (Lines::PerRow, Some(rule)) => silence_line_ms(rule, MIN_SILENCE_MS),
            (Lines::PerRow, None) | (Lines::Flat, _) => MIN_SILENCE_MS,
        }
    }

    fn is_valid(&self, lines: Lines) -> bool {
        is_valid_l_asr(self.sample, self.line_ms(lines))
    }
}

/// The rule of `sample` in `run` (the `number`-th, 1-based). A sample with no
/// `final` is invalid against any line and needs none.
fn rule_of(run: &AltRun, sample: &Sample, number: usize) -> Result<Option<Rule>, Failure> {
    let Some(rules) = &run.rules else {
        return Ok(None);
    };
    if sample.t_final_us.is_none() {
        return Ok(None);
    }
    match rules.get(&(sample.repeat, sample.utt)) {
        Some(rule) => Ok(Some(*rule)),
        None => Err(format!(
            "run {number}: repeat {} utt {} has a final and no recorded rule",
            sample.repeat, sample.utt
        )),
    }
}

/// The samples of the runs of `who` pooled, each with its rule.
fn pooled(runs: &[AltRun], who: Implementation) -> Result<Vec<Closed<'_>>, Failure> {
    let mut closed = Vec::new();
    for (index, run) in runs.iter().enumerate() {
        if run.implementation != who {
            continue;
        }
        for sample in &run.samples {
            closed.push(Closed {
                sample,
                rule: rule_of(run, sample, index + 1)?,
            });
        }
    }
    Ok(closed)
}

/// The samples of `closed` that are valid against `lines`.
fn valid<'a>(closed: &[Closed<'a>], lines: Lines) -> Vec<&'a Sample> {
    closed
        .iter()
        .filter(|one| one.is_valid(lines))
        .map(|one| one.sample)
        .collect()
}

/// `L` in microseconds of the valid samples with a translation, by utterance.
fn l_by_utterance(valid: &[&Sample]) -> BTreeMap<u32, Vec<i64>> {
    let mut found: BTreeMap<u32, Vec<i64>> = BTreeMap::new();
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
        let valid: Vec<&Sample> = run
            .iter()
            .filter(|sample| is_valid_l_asr(sample, MIN_SILENCE_MS))
            .collect();
        for utt in l_by_utterance(&valid).keys() {
            *counts.entry(*utt).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .filter(|(_, runs_with_it)| *runs_with_it == python_runs.len())
        .map(|(utt, _)| utt)
        .collect()
}

fn medians(valid: &[&Sample]) -> Result<BTreeMap<u32, i64>, Failure> {
    l_by_utterance(valid)
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

/// The failures of the pooled judgement: the translated share of Rust (`share`,
/// already judged), the percentiles, then the paired comparison.
fn judge_pooled(report: &AltReport, share: Option<String>) -> Vec<String> {
    let mut failures: Vec<String> = share.into_iter().collect();
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

/// What the runs came to, judged on nothing but their form: the order, the
/// pair count and the loads are checked, the values are only counted. The
/// columns of a run that `check_alternated_runs` refuses stand here.
pub fn alternated_report(
    runs: &[AltRun],
    recorded_pairs: Option<usize>,
) -> Result<AltReport, Failure> {
    check_order(runs)?;
    check_pair_count(runs.len() / 2, recorded_pairs)?;
    let loads_after = check_loads(runs)?;
    let (python, rust) = (
        pooled(runs, Implementation::Python)?,
        pooled(runs, Implementation::Rust)?,
    );
    let python_runs: Vec<&[Sample]> = runs
        .iter()
        .filter(|run| run.implementation == Implementation::Python)
        .map(|run| run.samples.as_slice())
        .collect();
    let comparison_set = l_comparison_set(&python_runs);
    let python_valid = valid(&python, Lines::PerRow);
    let rust_valid = valid(&rust, Lines::PerRow);
    let rust_flat = valid(&rust, Lines::Flat);
    let python_medians = medians(&python_valid)?;
    Ok(AltReport {
        pairs: runs.len() / 2,
        python: columns_over(python_valid.iter().copied())?,
        rust: columns_over(rust_valid.iter().copied())?,
        not_slower: not_slower(&comparison_set, &python_medians, &medians(&rust_valid)?),
        rust_share: share_of_valid(&rust_valid),
        python_share: share_of_valid(&python_valid),
        loads_after,
        flat: FlatCounts {
            not_slower: not_slower(&comparison_set, &python_medians, &medians(&rust_flat)?),
            rust_share: share_of_valid(&rust_flat),
        },
        gained: gained_by_rule(&rust),
        python_quality: quality_of(&python),
        rust_quality: quality_of(&rust),
        comparison_set,
    })
}

/// How many Rust samples are valid against their own line and not against
/// `min_silence`, by rule name. Every rule is a key.
fn gained_by_rule(rust: &[Closed<'_>]) -> BTreeMap<&'static str, usize> {
    let mut gained: BTreeMap<&'static str, usize> =
        RULES.iter().map(|rule| (rule.name(), 0)).collect();
    for (one, rule) in rust.iter().filter_map(|one| Some((one, one.rule?))) {
        if one.is_valid(Lines::PerRow) && !one.is_valid(Lines::Flat) {
            *gained.entry(rule.name()).or_default() += 1;
        }
    }
    gained
}

/// The quality numbers over every sample of a side.
fn quality_of(side: &[Closed<'_>]) -> Quality {
    let mut quality = Quality {
        samples: side.len(),
        split: 0,
        no_final: 0,
        cer_dist: 0,
        cer_denom: 0,
    };
    for one in side {
        quality.split += usize::from(one.sample.finals >= 2);
        quality.no_final += usize::from(one.sample.t_final_us.is_none());
        quality.cer_dist += one.sample.cer_dist;
        quality.cer_denom += one.sample.cer_denom;
    }
    quality
}

/// RP-32: `runs` in the order they were measured; `recorded_pairs` is the
/// pair count written in the result when fewer than three fitted.
pub fn check_alternated_runs(
    runs: &[AltRun],
    recorded_pairs: Option<usize>,
) -> Result<AltReport, Failure> {
    let report = alternated_report(runs, recorded_pairs)?;
    let share = check_share(report.rust_share)
        .err()
        .map(|why| why.to_string());
    let failures = judge_pooled(&report, share);
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

/// The headings of the two translated-share columns. The render and the check
/// both read these.
pub const SHARE_COLUMN_RUST: &str = "translated share rust";
pub const SHARE_COLUMN_PYTHON: &str = "translated share python";

/// The translated-share columns, one per side: `<heading> <translated> / <valid>`.
pub fn render_share_columns(report: &AltReport) -> String {
    let line = |heading: &str, share: &TranslatedShare| {
        format!("{heading} {} / {}", share.translated, share.valid)
    };
    [
        line(SHARE_COLUMN_RUST, &report.rust_share),
        line(SHARE_COLUMN_PYTHON, &report.python_share),
    ]
    .join("\n")
}

/// `n / d` and its percentage with two decimals; `n / 0` for an empty side.
fn ratio(n: usize, d: usize) -> String {
    if d == 0 {
        return format!("{n} / {d}");
    }
    format!("{n} / {d} ({:.2} %)", 100.0 * n as f64 / d as f64)
}

/// The margin of `not_slower` over the threshold of the comparison set, in words.
fn margin_text(set_size: usize, not_slower: usize) -> String {
    let Ok(needed) = comparison_threshold(set_size) else {
        return format!("no threshold: the comparison set is empty ({not_slower} not slower)");
    };
    let margin = not_slower.abs_diff(needed);
    if not_slower < needed {
        return format!("{needed} needed, short by {margin}");
    }
    if margin == 0 {
        return format!("{needed} needed, margin 0: one utterance swapped fails");
    }
    format!("{needed} needed, margin {margin}")
}

fn quality_line(side: &str, quality: &Quality) -> String {
    format!(
        "quality {side}: two or more finals {}; CER {}; no final {}",
        ratio(quality.split, quality.samples),
        ratio(quality.cer_dist, quality.cer_denom),
        quality.no_final
    )
}

/// The speed counts at both sets of lines, what moved between them, and the
/// quality numbers, one per line. The judgement reads the per-row counts only.
pub fn render_line_counts(report: &AltReport) -> String {
    let set = report.comparison_set.len();
    let moved: Vec<String> = report
        .gained
        .iter()
        .map(|(rule, count)| format!("{rule} {count}"))
        .collect();
    let total: usize = report.gained.values().sum();
    [
        format!(
            "not slower, {MIN_SILENCE_MS} ms line: {} / {set}",
            report.flat.not_slower
        ),
        format!(
            "not slower, per-row line: {} / {set} ({})",
            report.not_slower,
            margin_text(set, report.not_slower)
        ),
        format!(
            "translated share rust, {MIN_SILENCE_MS} ms line: {}",
            ratio(report.flat.rust_share.translated, report.flat.rust_share.valid)
        ),
        format!(
            "translated share rust, per-row line: {}",
            ratio(report.rust_share.translated, report.rust_share.valid)
        ),
        format!(
            "rust samples valid on their own line and not on the {MIN_SILENCE_MS} ms line: {total} ({})",
            moved.join(", ")
        ),
        quality_line("python", &report.python_quality),
        quality_line("rust", &report.rust_quality),
    ]
    .join("\n")
}

/// The output names both translated-share columns.
pub fn check_share_columns_written(text: &str) -> Result<(), Failure> {
    for heading in [SHARE_COLUMN_RUST, SHARE_COLUMN_PYTHON] {
        if !text.lines().any(|line| line.starts_with(heading)) {
            return Err(format!("the column {heading} is missing from the output"));
        }
    }
    Ok(())
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
/// it is absent), the runs of that many pairs, the port each Rust run held
/// (RP-72), and the judgement.
pub fn check_dir(dir: &Path) -> Result<AltReport, Failure> {
    let count = pairs_to_read(dir)?;
    let runs = load_alternated(dir, count.pairs)?;
    check_ports_held(dir, count.pairs)?;
    check_alternated_runs(&runs, count.recorded)
}

/// RP-72: the events file of the Rust run `number` (`rs<N>.events.jsonl`).
fn events_file_name(number: usize) -> String {
    format!("rs{number}.events.jsonl")
}

/// Whether an event row is the server telling that it moved off its port:
/// `type` is `fallback` and `reason` is `in_use`, whatever the `stage`.
fn is_port_move(row: &Value) -> bool {
    let field = |key: &str| {
        row.get("event")
            .and_then(|event| event.get(key))
            .and_then(Value::as_str)
    };
    field("type") == Some("fallback") && field("reason") == Some("in_use")
}

/// The refusal of one events file, or `None` when it shows no port move. A
/// line that is not JSON cannot be checked and is refused.
fn port_move_in(dir: &Path, number: usize) -> Result<Option<String>, Failure> {
    let name = events_file_name(number);
    let path = dir.join(&name);
    if !path.is_file() {
        return Ok(Some(format!(
            "{name} is missing: a run whose port cannot be checked is not judged"
        )));
    }
    for (index, line) in read(&path)?.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let row: Value = serde_json::from_str(line)
            .map_err(|why| format!("{name} line {}: not JSON: {why}", index + 1))?;
        if is_port_move(&row) {
            return Ok(Some(format!(
                "rs{number} is invalid: the server moved to another port \
                 (a fallback with the reason in_use stands in {name} line {})",
                index + 1
            )));
        }
    }
    Ok(None)
}

/// RP-72: every Rust run of `pairs` pairs held its port. All the runs that did
/// not are named, not the first only; Python's events are not read.
pub fn check_ports_held(dir: &Path, pairs: usize) -> Result<(), Failure> {
    let mut refused = Vec::new();
    for number in 1..=pairs {
        refused.extend(port_move_in(dir, number)?);
    }
    if refused.is_empty() {
        return Ok(());
    }
    Err(refused.join("; "))
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

/// The files the rule of each Rust `final` is read from that `dir` does not
/// hold: `rs<N>.events.jsonl` and `rs<N>.hayamimi.stdout.log` of `pairs` pairs.
pub fn missing_rule_files(dir: &Path, pairs: usize) -> Vec<String> {
    (1..=pairs)
        .flat_map(|n| [events_file_name(n), format!("rs{n}.hayamimi.stdout.log")])
        .filter(|name| !dir.join(name).is_file())
        .collect()
}

/// The runs of `pairs` pairs in `dir`, in measurement order, with no rule
/// recorded: every sample is judged against `min_silence`. A missing file is
/// named in the error; a missing load line is left to the judgement.
pub fn load_runs(dir: &Path, pairs: usize) -> Result<Vec<AltRun>, Failure> {
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
            rules: None,
        });
    }
    Ok(runs)
}

/// The rule of every sample of the Rust run `name` of `dir`.
fn rules_of_run(
    dir: &Path,
    name: &str,
    samples: &[Sample],
) -> Result<BTreeMap<(u32, u32), Rule>, Failure> {
    let events = read(&dir.join(format!("{name}.events.jsonl")))?;
    let log = read(&dir.join(format!("{name}.hayamimi.stdout.log")))?;
    closing_rules(name, samples, &events, &log)
}

/// The runs of `pairs` pairs in `dir` as [`load_runs`] reads them, and for
/// each Rust run the rule that closed each sample. A missing file, a `final`
/// with no decided line and a sample no `final` can be tied to are refused
/// with the run named; no rule is filled in.
pub fn load_alternated(dir: &Path, pairs: usize) -> Result<Vec<AltRun>, Failure> {
    let mut runs = load_runs(dir, pairs)?;
    let missing = missing_rule_files(dir, pairs);
    if !missing.is_empty() {
        return Err(format!(
            "{} lacks {}: the E2E seat copies the Rust runs' events and hayamimi stdout there",
            dir.display(),
            missing.join(", ")
        ));
    }
    let names = run_names(pairs);
    for ((name, implementation), run) in names.iter().zip(runs.iter_mut()) {
        if *implementation == Implementation::Rust {
            run.rules = Some(rules_of_run(dir, name, &run.samples)?);
        }
    }
    Ok(runs)
}

// ---- F32, the runs the tests build by a formula (T 4.42) ----

/// The 1 minute load of `/proc/loadavg` the F32 runs start and end under.
pub const LOAD_OK: &str = "0.50 0.50 0.50 1/300 1000";

/// The moment (microseconds) the F32 `final` of utterance `utt` is received.
fn f32_final_us(utt: u32) -> i64 {
    i64::from(700 + utt) * 1000
}

/// F32: `t_end` 0, `L_asr` 700 + u ms, `L` 1000 + 10u + k ms (utterance u, pair k).
/// The silence gap is 600 + u ms, over the 300 ms limit, so every sample is valid.
pub fn f32_sample(utt: u32, k: u32) -> Sample {
    Sample {
        repeat: 1,
        utt,
        t_start_us: 0,
        t_end_us: 0,
        t_partial_us: None,
        t_partial_tr_us: None,
        t_final_us: Some(f32_final_us(utt)),
        t_tr_us: Some(i64::from(1000 + 10 * utt + k) * 1000),
        finals: 1,
        text: String::new(),
        translation: None,
        final_latency_ms: Some(100.0),
        cer_dist: 0,
        cer_denom: 0,
    }
}

/// An empty directory `target/<group>/<name>` of this crate's build tree.
/// `name` keeps the tests that run side by side apart.
pub fn scratch_dir(group: &str, name: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../target")
        .join(group)
        .join(name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// The events of an F32 Rust run: the recognizer is ready, then the `final`
/// `mic-<u>` of each utterance `u`, received when the F32 sample of `u` closed.
/// No `fallback`.
fn f32_events() -> String {
    let mut text = String::from("{\"event\":{\"sr\":16000,\"type\":\"ready\"},\"t_us\":1}\n");
    for utt in 1..=19_u32 {
        let t_us = f32_final_us(utt);
        text.push_str(&format!(
            "{{\"event\":{{\"id\":\"mic-{utt}\",\"text\":\"x\",\"type\":\"final\"}},\"t_us\":{t_us}}}\n"
        ));
    }
    text
}

/// The stdout of an F32 Rust server: a line that is not a decided line, then
/// the decided line of each utterance, every one ended by the silence rule.
fn f32_stdout() -> String {
    let mut text = String::from("{\"event\":\"asr.ready\",\"level\":\"info\"}\n");
    for utt in 1..=19_u32 {
        text.push_str(&format!(
            "{{\"event\":\"asr.endpoint.decided\",\"id\":{utt},\"rule\":\"silence\",\"silence_ms\":300}}\n"
        ));
    }
    text
}

/// Writes the F32 runs of `pairs` pairs into `dir`: `py<N>.jsonl`, `rs<N>.jsonl`
/// (Rust equals Python), `load<N>.txt` and the clean `rs<N>.events.jsonl` and
/// `rs<N>.hayamimi.stdout.log` per pair, and `pairs_txt` as `pairs.txt` when given.
pub fn write_f32_dir(
    dir: &Path,
    pairs: u32,
    pairs_txt: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    for k in 1..=pairs {
        let mut load = String::new();
        for run in [format!("py{k}"), format!("rs{k}")] {
            let samples: Vec<String> = (1..=19)
                .map(|utt| serde_json::to_string(&f32_sample(utt, k)))
                .collect::<Result<_, _>>()?;
            std::fs::write(dir.join(format!("{run}.jsonl")), samples.join("\n") + "\n")?;
            for when in ["before", "after"] {
                load.push_str(&format!("{run} {when} 00:00:00 {LOAD_OK}\n"));
            }
        }
        std::fs::write(dir.join(format!("load{k}.txt")), load)?;
        std::fs::write(dir.join(format!("rs{k}.events.jsonl")), f32_events())?;
        std::fs::write(dir.join(format!("rs{k}.hayamimi.stdout.log")), f32_stdout())?;
    }
    if let Some(text) = pairs_txt {
        std::fs::write(dir.join("pairs.txt"), text)?;
    }
    Ok(())
}

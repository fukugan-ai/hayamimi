//! Judgement of the end-to-end acceptance (E2E-1, E2E-2) over the records of
//! a real run.
//!
//! The tests start no process. They read the samples file that `hayamimi-eval
//! stream --out` wrote (and, for E2E-2, the events file beside it) and judge
//! it with the functions RP-3 and RP-32 share: the validity of a sample, the
//! translated share and the percentile comparison all come from
//! `hayamimi_eval::compare`.
//!
//! A sample is valid against a line (3.1). A Python sample is judged against
//! `min_silence`. A Rust sample is judged against the line of the rule that
//! closed it (`rp32_rules`): the early detector's silence for `early`,
//! `min_silence` for the others. The rule is read from the `hayamimi` stdout
//! beside the samples file (`<run>.hayamimi.stdout.log`); a Rust run without it
//! is refused, unless it is one of [`NO_EARLY_RULE`], the records of a build
//! that has no early rule, which are counted at `min_silence` and say so. A run
//! counted per sample is also counted with every sample at `min_silence`, and
//! both are written.
//!
//! The records stand at fixed places in the repository, as RP-32 reads
//! `docs/results/r8h_verify/`: `docs/results/e2e/e2e1/` and
//! `docs/results/e2e/e2e2/`. A run is `<name>.jsonl` (the Rust samples) with
//! `<name>.events.jsonl` beside it; the Python runs of E2E-1 are `py<N>.jsonl`
//! in the same directory. The raw remains of a run (stdout, stderr, pid, gpu
//! csv) stay outside git.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use hayamimi_asr::endpoint::Rule;
use hayamimi_eval::compare::{
    TranslatedShare, check_percentiles, check_share, is_valid_l_asr, read_samples, share_of_valid,
};
use hayamimi_eval::latency::{Sample, Stat, stat_of};
use hayamimi_eval::sha::sha256_hex;
use serde_json::Value;

use super::machine::below_root;
use super::rp32::scratch_dir;
use super::rp32_rules::{closing_rules, decided, final_line, silence_line_ms};
use super::{Failure, RESULTS, read};

/// The `--min-silence` both sides of the comparison were started with
/// (milliseconds): the Rust default and the comparison value. A Python sample
/// and a Rust sample of a run with no rule on record are judged against it, and
/// so is every sample of RP-33. RP-32, E2E-1 and E2E-2 judge a Rust sample
/// against it unless the early rule closed that sample
/// (`rp32_rules::silence_line_ms`).
pub const MIN_SILENCE_MS: i64 = 300;
/// E2E-2: the p95 of `L` must not exceed this (RP-3, en to ja).
pub const E2E2_P95_BUDGET_MS: i64 = 2000;

/// The records of a build that has no early rule, kept without a `hayamimi`
/// stdout: (directory from the repository root, run, sha256 of `<run>.jsonl`).
/// The sha256 keeps a run whose content was replaced from passing as one of them.
pub const NO_EARLY_RULE: [(&str, &str, &str); 4] = [
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
    (
        "docs/results/r8h_verify",
        "rs1",
        "b3097191747065e71dcc99045752445b25d5d343a90a5fe969349312b486ce7c",
    ),
    (
        "docs/results/r8h_verify",
        "e2e2c",
        "804e398e19dece8b5e5990df53c9258432ad2808a7b8d0dc3c84a99015690b36",
    ),
];

/// What the output says about a listed record counted at `min_silence`.
const LISTED_NOTE: &str = "no hayamimi stdout (a listed record of a build without the early rule): every sample counted at min_silence";

/// The files of one acceptance run.
#[derive(Debug, PartialEq, Eq)]
pub struct RecordList {
    pub rust: PathBuf,
    pub python: Vec<PathBuf>,
}

/// Where the records of `name` (`e2e1`, `e2e2`) stand.
pub fn run_dir(name: &str) -> PathBuf {
    Path::new(RESULTS).join("e2e").join(name)
}

/// Whether a file name is a Python run: `py<N>.jsonl` (`py1.events.jsonl` is not).
fn is_python_run(file_name: &str) -> bool {
    file_name
        .strip_prefix("py")
        .and_then(|rest| rest.strip_suffix(".jsonl"))
        .is_some_and(|number| !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()))
}

/// The files of the run `<stem>.jsonl` in `dir`, and the Python runs there in name order.
pub fn list_run(dir: &Path, stem: &str) -> Result<RecordList, Failure> {
    let rust = dir.join(format!("{stem}.jsonl"));
    if !rust.is_file() {
        return Err(format!(
            "no {}: the E2E seat copies the run's samples and events jsonl into {}",
            rust.display(),
            dir.display()
        ));
    }
    let io_error = |why: std::io::Error| format!("{}: {why}", dir.display());
    let mut python = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        if is_python_run(&entry.file_name().to_string_lossy()) {
            python.push(entry.path());
        }
    }
    python.sort();
    Ok(RecordList { rust, python })
}

/// The run `name` in its fixed directory.
pub fn load_run(name: &str) -> Result<RecordList, Failure> {
    list_run(&run_dir(name), name)
}

/// The samples of every file, one after the other.
pub fn load_pooled(paths: &[PathBuf]) -> Result<Vec<Sample>, Failure> {
    let mut pooled = Vec::new();
    for path in paths {
        pooled.extend(read_samples(path).map_err(|why| why.to_string())?);
    }
    Ok(pooled)
}

/// The events file beside a samples file.
pub fn events_path(samples: &Path) -> PathBuf {
    samples.with_extension("events.jsonl")
}

/// The `hayamimi` stdout beside a samples file.
fn stdout_path(samples: &Path) -> PathBuf {
    samples.with_extension("hayamimi.stdout.log")
}

/// The name of a run: its samples file without `.jsonl`.
fn run_name(samples: &Path) -> Result<String, Failure> {
    let stem = samples
        .file_stem()
        .ok_or_else(|| format!("{} has no file name", samples.display()))?;
    Ok(stem.to_string_lossy().into_owned())
}

/// A sample and the silence line (milliseconds) it is judged against.
#[derive(Clone, Debug)]
struct Lined {
    sample: Sample,
    line_ms: i64,
}

/// How the lines of a side were chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Counting {
    /// `min_silence` for every sample: Python, and runs the tests build by a formula.
    Flat,
    /// A record of [`NO_EARLY_RULE`] without a stdout: `min_silence` for every
    /// sample, and the output says so.
    Listed,
    /// The line of the rule that closed each sample.
    PerSample,
}

/// The samples of one side of a comparison, each with its line.
#[derive(Clone, Debug)]
pub struct Side {
    rows: Vec<Lined>,
    counting: Counting,
}

impl Side {
    fn at_min_silence(samples: Vec<Sample>, counting: Counting) -> Side {
        let rows = samples
            .into_iter()
            .map(|sample| Lined {
                sample,
                line_ms: MIN_SILENCE_MS,
            })
            .collect();
        Side { rows, counting }
    }

    /// Every sample against `min_silence`.
    pub fn flat(samples: Vec<Sample>) -> Side {
        Side::at_min_silence(samples, Counting::Flat)
    }

    /// A record of [`NO_EARLY_RULE`] that has no stdout.
    fn listed(samples: Vec<Sample>) -> Side {
        Side::at_min_silence(samples, Counting::Listed)
    }

    /// Each sample against the line of the rule that closed it. `events` and
    /// `log` are `<run>.events.jsonl` and `<run>.hayamimi.stdout.log`; a
    /// `final` with no decided line, a decided id that stands twice and two
    /// `final`s of different rules at one moment are refused with the run and
    /// the ids named.
    pub fn per_sample(
        run: &str,
        samples: Vec<Sample>,
        events: &str,
        log: &str,
    ) -> Result<Side, Failure> {
        let rules = closing_rules(run, &samples, events, log)?;
        let rows = samples
            .into_iter()
            .map(|sample| lined(run, &rules, sample))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Side {
            rows,
            counting: Counting::PerSample,
        })
    }

    /// The samples valid against their own line.
    fn valid(&self) -> Vec<&Sample> {
        self.rows
            .iter()
            .filter(|row| is_valid_l_asr(&row.sample, row.line_ms))
            .map(|row| &row.sample)
            .collect()
    }

    /// The samples valid against `min_silence`, whatever their own line.
    fn valid_at_min_silence(&self) -> Vec<&Sample> {
        self.rows
            .iter()
            .map(|row| &row.sample)
            .filter(|sample| is_valid_l_asr(sample, MIN_SILENCE_MS))
            .collect()
    }

    /// The samples with a `final` and a gap under their own line.
    fn bad_end(&self) -> usize {
        self.rows
            .iter()
            .filter(|row| {
                row.sample.t_final_us.is_some() && !is_valid_l_asr(&row.sample, row.line_ms)
            })
            .count()
    }
}

/// A sample with the line of the rule that closed it. A sample with no
/// `final` has no gap, so it is invalid against any line and takes `min_silence`.
fn lined(run: &str, rules: &BTreeMap<(u32, u32), Rule>, sample: Sample) -> Result<Lined, Failure> {
    if sample.t_final_us.is_none() {
        return Ok(Lined {
            sample,
            line_ms: MIN_SILENCE_MS,
        });
    }
    let Some(rule) = rules.get(&(sample.repeat, sample.utt)) else {
        return Err(format!(
            "{run}: repeat {} utt {} has a final and no recorded rule",
            sample.repeat, sample.utt
        ));
    };
    Ok(Lined {
        line_ms: silence_line_ms(*rule, MIN_SILENCE_MS),
        sample,
    })
}

/// Whether the run `name` at `samples` is one of [`NO_EARLY_RULE`]: its
/// directory below `repo_root`, its name and the sha256 of its samples file agree.
fn is_listed(repo_root: &Path, samples: &Path, name: &str) -> bool {
    let dir = samples.parent().and_then(|dir| below_root(repo_root, dir));
    let bytes = std::fs::read(samples).ok();
    dir.zip(bytes).is_some_and(|(dir, bytes)| {
        let sha = sha256_hex(&bytes);
        NO_EARLY_RULE
            .iter()
            .any(|(listed, run, hash)| Path::new(listed) == dir && *run == name && *hash == sha)
    })
}

/// The Rust run `samples_path` as a side. With its `hayamimi` stdout beside it,
/// each sample takes the line of its rule; without it only a record of
/// [`NO_EARLY_RULE`] under `repo_root` is read, at `min_silence`.
pub fn load_rust(repo_root: &Path, samples_path: &Path) -> Result<Side, Failure> {
    let name = run_name(samples_path)?;
    let samples = read_samples(samples_path).map_err(|why| why.to_string())?;
    let log_path = stdout_path(samples_path);
    if log_path.is_file() {
        let events = read(&events_path(samples_path))?;
        return Side::per_sample(&name, samples, &events, &read(&log_path)?);
    }
    if is_listed(repo_root, samples_path, &name) {
        return Ok(Side::listed(samples));
    }
    Err(format!(
        "{name}: no {name}.hayamimi.stdout.log beside {}: the rule that closed each sample is read from it, and only the listed records of a build without the early rule go without",
        samples_path.display()
    ))
}

/// `L`, `L_asr` and `L_tr` of a run (3.1): `L_asr` over the valid samples,
/// `L` and `L_tr` over the valid samples that got a translation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Columns {
    pub l: Option<Stat>,
    pub l_asr: Option<Stat>,
    pub l_tr: Option<Stat>,
}

fn stat_column(values_us: &[i64]) -> Result<Option<Stat>, Failure> {
    stat_of(values_us).map_err(|why| why.to_string())
}

/// The three columns over the valid samples of `samples`, every sample judged
/// against the same line.
pub fn columns_of(samples: &[Sample], min_silence_ms: i64) -> Result<Columns, Failure> {
    columns_over(
        samples
            .iter()
            .filter(|sample| is_valid_l_asr(sample, min_silence_ms)),
    )
}

/// The three columns over samples already judged valid, each against its own line.
pub fn columns_over<'a>(valid: impl IntoIterator<Item = &'a Sample>) -> Result<Columns, Failure> {
    let (mut l, mut l_asr, mut l_tr) = (Vec::new(), Vec::new(), Vec::new());
    for sample in valid {
        l_asr.extend(sample.l_asr_us());
        l.extend(sample.l_us());
        l_tr.extend(sample.l_tr_us());
    }
    Ok(Columns {
        l: stat_column(&l)?,
        l_asr: stat_column(&l_asr)?,
        l_tr: stat_column(&l_tr)?,
    })
}

/// The share and the columns of the samples one count judged valid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Counted {
    pub share: TranslatedShare,
    pub columns: Columns,
}

fn counted(valid: &[&Sample]) -> Result<Counted, Failure> {
    Ok(Counted {
        share: share_of_valid(valid),
        columns: columns_over(valid.iter().copied())?,
    })
}

/// What a run counted: the valid samples, those translated, and the columns,
/// each sample against its own line. A run counted per sample also holds the
/// same samples against `min_silence`, recorded and not judged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    pub share: TranslatedShare,
    /// Samples with a `final` and a gap under their line.
    pub bad_end: usize,
    pub columns: Columns,
    pub counting: Counting,
    pub at_min_silence: Option<Counted>,
}

pub fn report_of(side: &Side) -> Result<Report, Failure> {
    let own = counted(&side.valid())?;
    let at_min_silence = (side.counting == Counting::PerSample)
        .then(|| counted(&side.valid_at_min_silence()))
        .transpose()?;
    Ok(Report {
        share: own.share,
        bad_end: side.bad_end(),
        columns: own.columns,
        counting: side.counting,
        at_min_silence,
    })
}

/// The refusal of a translated share below 20 / 23, or of a run with no valid sample.
fn share_failure(report: &Report) -> Option<String> {
    check_share(report.share).err().map(|why| why.to_string())
}

/// E2E-1: Rust's `L` p50 and p95 are no greater than Python's.
fn python_failure(rust: &Columns, python: &Columns) -> Option<String> {
    match (python.l, rust.l) {
        (Some(python), Some(rust)) => check_percentiles("L", &python, &rust)
            .err()
            .map(|why| why.to_string()),
        (None, _) => Some(String::from("python: no valid sample has a translation")),
        (_, None) => Some(String::from("rust: no valid sample has a translation")),
    }
}

/// E2E-2: the p95 of `L` is within the budget.
pub fn budget_failure(columns: &Columns) -> Option<String> {
    match columns.l {
        Some(l) if l.p95_ms > E2E2_P95_BUDGET_MS => {
            Some(format!("L p95 {} ms > {E2E2_P95_BUDGET_MS} ms", l.p95_ms))
        }
        Some(_) => None,
        None => Some(String::from("rust: no valid sample has a translation")),
    }
}

/// How many events of the wanted languages a run's events file holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventCounts {
    pub en_finals: usize,
    pub ja_translations: usize,
}

fn type_and_lang_are(event: &Value, kind: &str, lang: &str) -> bool {
    event.get("type").and_then(Value::as_str) == Some(kind)
        && event.get("lang").and_then(Value::as_str) == Some(lang)
}

/// A translation the subtitle shows: `status` `ok` or none, `text` not empty,
/// and not the translation of a partial.
fn is_shown_translation(event: &Value, lang: &str) -> bool {
    type_and_lang_are(event, "translation", lang)
        && event.get("of").and_then(Value::as_str) != Some("partial")
        && event
            .get("status")
            .and_then(Value::as_str)
            .is_none_or(|status| status == "ok")
        && event
            .get("text")
            .and_then(Value::as_str)
            .is_some_and(|text| !text.is_empty())
}

/// Counts the `final` events in `en` and the shown translations in `ja`.
pub fn count_events(text: &str) -> Result<EventCounts, Failure> {
    let mut counts = EventCounts {
        en_finals: 0,
        ja_translations: 0,
    };
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let row: Value = serde_json::from_str(line)
            .map_err(|why| format!("an events line is not JSON: {why}"))?;
        let Some(event) = row.get("event") else {
            return Err(format!("an events line has no `event`: {line}"));
        };
        counts.en_finals += usize::from(type_and_lang_are(event, "final", "en"));
        counts.ja_translations += usize::from(is_shown_translation(event, "ja"));
    }
    Ok(counts)
}

fn event_failures(counts: &EventCounts) -> Vec<String> {
    let mut failures = Vec::new();
    if counts.en_finals == 0 {
        failures.push(String::from("the events hold no `final` in en"));
    }
    if counts.ja_translations == 0 {
        failures.push(String::from("the events hold no shown translation in ja"));
    }
    failures
}

/// The numbers of one judged run and the reasons it fails, if it does.
#[derive(Debug)]
pub struct Judged {
    pub rust: Report,
    pub python: Option<Report>,
    pub events: Option<EventCounts>,
    pub failures: Vec<String>,
}

/// E2E-1: share of translated valid samples, and `L` against Python's pooled runs.
pub fn judge_e2e1(rust: &Side, python: &Side) -> Result<Judged, Failure> {
    let (rust_report, python_report) = (report_of(rust)?, report_of(python)?);
    let mut failures = Vec::new();
    failures.extend(share_failure(&rust_report));
    failures.extend(python_failure(&rust_report.columns, &python_report.columns));
    Ok(Judged {
        rust: rust_report,
        python: Some(python_report),
        events: None,
        failures,
    })
}

/// E2E-2: share of translated valid samples, the `L` p95 budget, and the languages of the events.
pub fn judge_e2e2(rust: &Side, events_text: &str) -> Result<Judged, Failure> {
    let report = report_of(rust)?;
    let events = count_events(events_text)?;
    let mut failures = Vec::new();
    failures.extend(share_failure(&report));
    failures.extend(budget_failure(&report.columns));
    failures.extend(event_failures(&events));
    Ok(Judged {
        rust: report,
        python: None,
        events: Some(events),
        failures,
    })
}

/// E2E-1 over the files of a record list. `repo_root` is the root the listed
/// records of [`NO_EARLY_RULE`] are looked for under.
pub fn judge_e2e1_files(repo_root: &Path, list: &RecordList) -> Result<Judged, Failure> {
    if list.python.is_empty() {
        return Err(String::from(
            "E2E-1 compares with Python: the directory needs a `py<N>.jsonl` run",
        ));
    }
    let rust = load_rust(repo_root, &list.rust)?;
    judge_e2e1(&rust, &Side::flat(load_pooled(&list.python)?))
}

/// E2E-2 over the files of a record list.
pub fn judge_e2e2_files(repo_root: &Path, list: &RecordList) -> Result<Judged, Failure> {
    let rust = load_rust(repo_root, &list.rust)?;
    judge_e2e2(&rust, &read(&events_path(&list.rust))?)
}

/// A run of `valid` valid samples (the silence gap is 1200 ms, over the
/// 300 ms limit) of which the first `translated` got a translation 100 ms
/// after the `final`, so every translated `L` is 1400 ms. A fixture for the
/// edge of the translated share.
pub fn share_run(valid: u32, translated: u32) -> Vec<Sample> {
    (1..=valid)
        .map(|utt| Sample {
            repeat: 1,
            utt,
            t_start_us: 0,
            t_end_us: 1_000_000,
            t_partial_us: None,
            t_partial_tr_us: None,
            t_final_us: Some(2_300_000),
            t_tr_us: (utt <= translated).then_some(2_400_000),
            finals: 1,
            text: String::new(),
            translation: None,
            final_latency_ms: Some(100.0),
            cer_dist: 0,
            cer_denom: 0,
        })
        .collect()
}

/// A copy of a run in which every sample that has a `final` gets its
/// translation `delay_us` after it: a fixture for the verdict's two ends.
pub fn translated_after(samples: &[Sample], delay_us: i64) -> Vec<Sample> {
    samples
        .iter()
        .cloned()
        .map(|sample| Sample {
            t_tr_us: sample.t_final_us.map(|t| t + delay_us),
            ..sample
        })
        .collect()
}

// ---- F88, the run the tests build by a formula (T 4.63) ----

/// The moment (microseconds) the `final` of F88 is received.
const F88_FINAL_US: i64 = 1_300_000;

/// F88: one sample whose speech ends at 1.0 s and whose `final` comes at 1.3 s
/// with a decoding time of 100 ms, so `L_asr - L_dec` is 200 ms.
pub fn f88_sample() -> Sample {
    Sample {
        repeat: 1,
        utt: 1,
        t_start_us: 0,
        t_end_us: 1_000_000,
        t_partial_us: None,
        t_partial_tr_us: None,
        t_final_us: Some(F88_FINAL_US),
        t_tr_us: Some(1_500_000),
        finals: 1,
        text: String::new(),
        translation: None,
        final_latency_ms: Some(100.0),
        cer_dist: 0,
        cer_denom: 0,
    }
}

/// Writes F88 as the run `f88` into the scratch directory `test` and returns
/// its samples file. Its events hold a `final` at the moment of the sample's
/// `final` for each id of `ids`. Its stdout holds the `(id, rule)` lines given;
/// `None` writes no stdout.
pub fn write_f88(
    test: &str,
    ids: &[&str],
    decided_rows: Option<&[(u64, &str)]>,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let dir = scratch_dir("e2e-f88", test)?;
    let samples = dir.join("f88.jsonl");
    std::fs::write(&samples, serde_json::to_string(&f88_sample())? + "\n")?;
    let events: String = ids.iter().map(|id| final_line(id, F88_FINAL_US)).collect();
    std::fs::write(events_path(&samples), events)?;
    if let Some(rows) = decided_rows {
        let stdout: String = rows.iter().map(|(id, rule)| decided(*id, rule)).collect();
        std::fs::write(stdout_path(&samples), stdout)?;
    }
    Ok(samples)
}

/// Copies the record `docs/results/<rel>` into `<root>/docs/results/<rel>`,
/// a directory standing in for the repository root, and returns the copy.
pub fn copy_record(root: &Path, rel: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let to = root.join("docs/results").join(rel);
    let dir = to.parent().ok_or("a record has a directory")?;
    std::fs::create_dir_all(dir)?;
    std::fs::copy(Path::new(RESULTS).join(rel), &to)?;
    Ok(to)
}

/// Adds one newline to the end of a file, so its sha256 no longer agrees with a listed one.
pub fn append_newline(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new().append(true).open(path)?;
    file.write_all(b"\n")?;
    Ok(())
}

fn show_stat(stat: Option<Stat>) -> String {
    stat.map_or_else(
        || String::from("no sample"),
        |stat| format!("p50 {} p95 {} ms (n {})", stat.p50_ms, stat.p95_ms, stat.n),
    )
}

fn show_share(share: &TranslatedShare) -> String {
    if share.valid == 0 {
        return String::from("0 of 0 valid samples translated");
    }
    let ratio = share.translated as f64 / share.valid as f64;
    format!(
        "{} of {} valid samples translated ({ratio:.4}; 20 / 23 = 0.8696 is needed)",
        share.translated, share.valid
    )
}

fn show_report(label: &str, report: &Report) -> Vec<String> {
    let mut lines = vec![
        format!("{label}: {}", show_share(&report.share)),
        format!("{label}: bad end {}", report.bad_end),
        format!("{label}: L {}", show_stat(report.columns.l)),
        format!("{label}: L_asr {}", show_stat(report.columns.l_asr)),
        format!("{label}: L_tr {}", show_stat(report.columns.l_tr)),
    ];
    if report.counting == Counting::Listed {
        lines.push(format!("{label}: {LISTED_NOTE}"));
    }
    if let Some(flat) = &report.at_min_silence {
        lines.push(format!(
            "{label}: every sample at min_silence: {}; L {}",
            show_share(&flat.share),
            show_stat(flat.columns.l)
        ));
    }
    lines
}

impl Judged {
    /// The numbers and the verdict, one per line.
    pub fn render(&self, name: &str) -> String {
        let mut lines = show_report(&format!("{name} rust"), &self.rust);
        if let Some(python) = &self.python {
            lines.extend(show_report(&format!("{name} python"), python));
        }
        if let Some(events) = &self.events {
            lines.push(format!(
                "{name} events: {} final in en, {} shown translation in ja",
                events.en_finals, events.ja_translations
            ));
        }
        lines.push(if self.failures.is_empty() {
            format!("{name}: PASS")
        } else {
            format!("{name}: FAIL: {}", self.failures.join("; "))
        });
        lines.join("\n")
    }
}

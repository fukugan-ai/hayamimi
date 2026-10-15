//! Judgement of the end-to-end acceptance (E2E-1, E2E-2) over the records of
//! a real run.
//!
//! The tests start no process. They read the samples file that `hayamimi-eval
//! stream --out` wrote (and, for E2E-2, the events file beside it) and judge
//! it with the functions RP-3 and RP-32 share: the validity of a sample, the
//! translated share and the percentile comparison all come from
//! `hayamimi_eval::compare`.
//!
//! The records stand at fixed places in the repository, as RP-32 reads
//! `docs/results/r8h_verify/`: `docs/results/e2e/e2e1/` and
//! `docs/results/e2e/e2e2/`. A run is `<name>.jsonl` (the Rust samples) with
//! `<name>.events.jsonl` beside it; the Python runs of E2E-1 are `py<N>.jsonl`
//! in the same directory. The raw remains of a run (stdout, stderr, pid, gpu
//! csv) stay outside git.

use std::path::{Path, PathBuf};

use hayamimi_eval::compare::{
    TranslatedShare, check_percentiles, check_translated_share, is_valid_l_asr, read_samples,
    translated_share,
};
use hayamimi_eval::latency::{Sample, Stat, stat_of};
use serde_json::Value;

use super::{Failure, RESULTS, read};

/// The `--min-silence` both sides of the comparison were started with
/// (milliseconds): the Rust default and the comparison value. E2E-1, E2E-2 and
/// RP-33 judge every sample against it; RP-32 judges a Rust sample against it
/// unless the early rule closed that sample (`rp32_rules::silence_line_ms`).
pub const MIN_SILENCE_MS: i64 = 300;
/// E2E-2: the p95 of `L` must not exceed this (RP-3, en to ja).
pub const E2E2_P95_BUDGET_MS: i64 = 2000;

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

/// What a run counted: the valid samples, those translated, and the columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    pub share: TranslatedShare,
    pub columns: Columns,
}

pub fn report_of(samples: &[Sample]) -> Result<Report, Failure> {
    Ok(Report {
        share: translated_share(samples, MIN_SILENCE_MS),
        columns: columns_of(samples, MIN_SILENCE_MS)?,
    })
}

/// The refusal of a translated share below 20 / 23, or of a run with no valid sample.
fn share_failure(samples: &[Sample]) -> Option<String> {
    check_translated_share(samples, MIN_SILENCE_MS)
        .err()
        .map(|why| why.to_string())
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
pub fn judge_e2e1(rust: &[Sample], python: &[Sample]) -> Result<Judged, Failure> {
    let (rust_report, python_report) = (report_of(rust)?, report_of(python)?);
    let mut failures = Vec::new();
    failures.extend(share_failure(rust));
    failures.extend(python_failure(&rust_report.columns, &python_report.columns));
    Ok(Judged {
        rust: rust_report,
        python: Some(python_report),
        events: None,
        failures,
    })
}

/// E2E-2: share of translated valid samples, the `L` p95 budget, and the languages of the events.
pub fn judge_e2e2(rust: &[Sample], events_text: &str) -> Result<Judged, Failure> {
    let report = report_of(rust)?;
    let events = count_events(events_text)?;
    let mut failures = Vec::new();
    failures.extend(share_failure(rust));
    failures.extend(budget_failure(&report.columns));
    failures.extend(event_failures(&events));
    Ok(Judged {
        rust: report,
        python: None,
        events: Some(events),
        failures,
    })
}

/// E2E-1 over the files of a record list.
pub fn judge_e2e1_files(list: &RecordList) -> Result<Judged, Failure> {
    if list.python.is_empty() {
        return Err(String::from(
            "E2E-1 compares with Python: the directory needs a `py<N>.jsonl` run",
        ));
    }
    let rust = load_pooled(std::slice::from_ref(&list.rust))?;
    judge_e2e1(&rust, &load_pooled(&list.python)?)
}

/// E2E-2 over the files of a record list.
pub fn judge_e2e2_files(list: &RecordList) -> Result<Judged, Failure> {
    let rust = load_pooled(std::slice::from_ref(&list.rust))?;
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
    vec![
        format!("{label}: {}", show_share(&report.share)),
        format!("{label}: L {}", show_stat(report.columns.l)),
        format!("{label}: L_asr {}", show_stat(report.columns.l_asr)),
        format!("{label}: L_tr {}", show_stat(report.columns.l_tr)),
    ]
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

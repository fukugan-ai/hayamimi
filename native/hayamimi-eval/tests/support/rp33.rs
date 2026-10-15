//! RP-33: the first translation of a line whose draft T failed V.
//!
//! A line is one sample of a run with whether the translation T made of its
//! closing `final` passed V. The relay emits that early translation (`stage`
//! `t`, `of` `final`, `status` `ok`) only when it passed, so a line whose
//! closing `final` has no such event among the received events is a line whose
//! draft failed. The events are the rows `{"t_us": N, "event": {...}}` of
//! `<run>.events.jsonl`, beside the samples file `<run>.jsonl`.
//!
//! The translation stage is `t_tr - t_final` of the valid samples that got a
//! translation (`Sample::l_tr_us`), counted the way RP-31 fixes. Validity and
//! the percentile comparison come from `hayamimi_eval::compare`, the functions
//! RP-3 and RP-32 use.

use std::path::Path;

use hayamimi_eval::compare::{check_percentiles, is_valid_l_asr, read_samples};
use hayamimi_eval::latency::{Event, Sample, Stat, Window, build_samples, parse_event, stat_of};
use serde_json::Value;

use super::e2e::{MIN_SILENCE_MS, events_path};
use super::rp32::pairs_to_read;
use super::{Failure, read};

/// More references than any run has utterances; the texts are not what RP-33 reads.
const REFERENCES: usize = 64;

pub const ALL_ROWS: &str = "translation stage (all rows)";
pub const FAILED_DRAFT: &str = "translation stage (failed draft)";
pub const PASSED_DRAFT: &str = "translation stage (passed draft)";

/// One sample, and whether T's translation of its closing `final` passed V.
#[derive(Clone, Debug)]
pub struct Line {
    pub sample: Sample,
    pub draft_passed: bool,
}

/// The translation stage of all lines, of the lines whose draft failed V, and of those whose draft passed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StageColumns {
    pub all: Option<Stat>,
    pub failed: Option<Stat>,
    pub passed: Option<Stat>,
}

fn stat_column(values_us: &[i64]) -> Result<Option<Stat>, Failure> {
    stat_of(values_us).map_err(|why| why.to_string())
}

/// The translation stage of the valid samples among `samples` that got a translation.
fn stage_stat<'a>(samples: impl Iterator<Item = &'a Sample>) -> Result<Option<Stat>, Failure> {
    let values: Vec<i64> = samples
        .filter(|sample| is_valid_l_asr(sample, MIN_SILENCE_MS))
        .filter_map(Sample::l_tr_us)
        .collect();
    stat_column(&values)
}

/// The translation stage of a run that has no V to fail, such as Python's.
pub fn samples_stage(samples: &[Sample]) -> Result<Option<Stat>, Failure> {
    stage_stat(samples.iter())
}

fn column(lines: &[Line], keep: impl Fn(&Line) -> bool) -> Result<Option<Stat>, Failure> {
    stage_stat(
        lines
            .iter()
            .filter(|line| keep(line))
            .map(|line| &line.sample),
    )
}

/// The three columns over `lines`. A line without a shown translation is in none of them.
pub fn stage_columns(lines: &[Line]) -> Result<StageColumns, Failure> {
    Ok(StageColumns {
        all: column(lines, |_| true)?,
        failed: column(lines, |line| !line.draft_passed)?,
        passed: column(lines, |line| line.draft_passed)?,
    })
}

/// RP-33: the translation stage of Rust, over all lines, is no greater than Python's at p50 and p95.
/// The columns come back when it is.
pub fn check_first_translation(rust: &[Line], python: &[Sample]) -> Result<StageColumns, Failure> {
    let columns = stage_columns(rust)?;
    let Some(baseline) = samples_stage(python)? else {
        return Err(String::from("python: no valid sample has a translation"));
    };
    let Some(candidate) = columns.all else {
        return Err(String::from("rust: no valid sample has a translation"));
    };
    check_percentiles("translation stage", &baseline, &candidate).map_err(|why| why.to_string())?;
    Ok(columns)
}

fn column_line(name: &str, stat: Option<Stat>) -> String {
    match stat {
        Some(stat) => format!(
            "{name} p50 {} p95 {} ms (n {})",
            stat.p50_ms, stat.p95_ms, stat.n
        ),
        None => format!("{name} no sample"),
    }
}

/// The columns, one per line.
pub fn render_stage_columns(columns: &StageColumns) -> String {
    [
        column_line(ALL_ROWS, columns.all),
        column_line(FAILED_DRAFT, columns.failed),
        column_line(PASSED_DRAFT, columns.passed),
    ]
    .join("\n")
}

/// The output has a line for every column; the first missing one is named.
pub fn check_columns_written(text: &str) -> Result<(), Failure> {
    for name in [ALL_ROWS, FAILED_DRAFT, PASSED_DRAFT] {
        let prefix = format!("{name} ");
        if !text.lines().any(|line| line.starts_with(&prefix)) {
            return Err(format!("the column {name} is missing from the output"));
        }
    }
    Ok(())
}

fn rows_of(stat: Option<Stat>) -> usize {
    stat.iter().map(|stat| stat.n).sum()
}

/// Every counted line is in exactly one of the failed and passed columns.
pub fn check_rows_add_up(columns: &StageColumns) -> Result<(), Failure> {
    let (all, failed, passed) = (
        rows_of(columns.all),
        rows_of(columns.failed),
        rows_of(columns.passed),
    );
    if failed + passed == all {
        return Ok(());
    }
    Err(format!(
        "{passed} lines with a passed draft and {failed} with a failed draft are not the {all} lines counted"
    ))
}

/// `L` (`t_end` to `t_tr`) of the valid lines whose draft passed.
fn passed_draft_l(lines: &[Line]) -> Result<Option<Stat>, Failure> {
    let values: Vec<i64> = lines
        .iter()
        .filter(|line| line.draft_passed && is_valid_l_asr(&line.sample, MIN_SILENCE_MS))
        .filter_map(|line| line.sample.l_us())
        .collect();
    stat_column(&values)
}

/// The rule added when the early translation is built for every line: the `L` of
/// the lines whose draft passed is no greater than in the run before, at p50 and p95.
pub fn check_passed_draft_l(before: &[Line], after: &[Line]) -> Result<(), Failure> {
    let (Some(baseline), Some(candidate)) = (passed_draft_l(before)?, passed_draft_l(after)?)
    else {
        return Err(String::from("a run has no valid line with a passed draft"));
    };
    check_percentiles("L of passed drafts", &baseline, &candidate).map_err(|why| why.to_string())
}

/// The rows of a JSON-lines text.
pub fn jsonl_rows(text: &str) -> Result<Vec<Value>, Failure> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).map_err(|why| format!("a line is not JSON: {why}")))
        .collect()
}

fn str_of<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

/// The ids of the `final` events received at `t_us`.
fn final_ids_at(rows: &[Value], t_us: i64) -> Vec<&str> {
    rows.iter()
        .filter(|row| row.get("t_us").and_then(Value::as_i64) == Some(t_us))
        .filter_map(|row| row.get("event"))
        .filter(|event| str_of(event, "type") == Some("final"))
        .filter_map(|event| str_of(event, "id"))
        .collect()
}

/// A translation of the `final` `id` that T made and V let through.
fn is_passed_draft_of(event: &Value, id: &str) -> bool {
    str_of(event, "type") == Some("translation")
        && str_of(event, "id") == Some(id)
        && str_of(event, "of") == Some("final")
        && str_of(event, "stage") == Some("t")
        && str_of(event, "status") == Some("ok")
}

/// Whether the closing `final` of `sample` has a translation T made and V let through.
/// A sample without a closing `final` has none.
pub fn draft_passed(sample: &Sample, rows: &[Value]) -> bool {
    sample.t_final_us.is_some_and(|t_final_us| {
        let ids = final_ids_at(rows, t_final_us);
        rows.iter()
            .filter_map(|row| row.get("event"))
            .any(|event| ids.iter().any(|id| is_passed_draft_of(event, id)))
    })
}

/// The samples with whether their draft passed, judged on the received events `rows`.
pub fn classify(samples: Vec<Sample>, rows: &[Value]) -> Vec<Line> {
    samples
        .into_iter()
        .map(|sample| {
            let passed = draft_passed(&sample, rows);
            Line {
                sample,
                draft_passed: passed,
            }
        })
        .collect()
}

fn int_of(row: &Value, key: &str) -> Result<i64, Failure> {
    row.get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("a row has no integer `{key}`: {row}"))
}

fn window_of(row: &Value) -> Result<Window, Failure> {
    let narrow =
        |key: &str| u32::try_from(int_of(row, key)?).map_err(|why| format!("{key}: {why}"));
    Ok(Window {
        repeat: narrow("repeat")?,
        utt: narrow("utt")?,
        t_start_us: int_of(row, "t_start_us")?,
        t_end_us: int_of(row, "t_end_us")?,
    })
}

fn event_of(row: &Value) -> Result<Event, Failure> {
    let Some(event) = row.get("event") else {
        return Err(format!("a row has no `event`: {row}"));
    };
    parse_event(int_of(row, "t_us")?, &event.to_string()).map_err(|why| why.to_string())
}

/// The lines of a run counted again, RP-31's way, from its window rows (a row
/// holds `repeat`, `utt`, `t_start_us`, `t_end_us`) and the events it received.
pub fn recount(window_rows: &[Value], event_rows: &[Value]) -> Result<Vec<Line>, Failure> {
    let windows = window_rows
        .iter()
        .map(window_of)
        .collect::<Result<Vec<_>, _>>()?;
    let events = event_rows
        .iter()
        .map(event_of)
        .collect::<Result<Vec<_>, _>>()?;
    let references = vec![String::new(); REFERENCES];
    let samples = build_samples(&windows, &references, &events).map_err(|why| why.to_string())?;
    Ok(classify(samples, event_rows))
}

/// [`recount`] over `<stem>.jsonl` and the `<stem>.events.jsonl` beside it. A missing file is named by its path.
pub fn recount_run(samples_file: &Path) -> Result<Vec<Line>, Failure> {
    let windows = jsonl_rows(&read(samples_file)?)?;
    let events = jsonl_rows(&read(&events_path(samples_file))?)?;
    recount(&windows, &events)
}

/// The lines of a run as `stream --out` wrote its samples, with the events beside the file.
pub fn load_run(samples_file: &Path) -> Result<Vec<Line>, Failure> {
    let samples = read_samples(samples_file).map_err(|why| why.to_string())?;
    let events = jsonl_rows(&read(&events_path(samples_file))?)?;
    Ok(classify(samples, &events))
}

/// The real runs of RP-32 in `dir`: the lines of every Rust run (`rs<N>.jsonl` with `rs<N>.events.jsonl`)
/// and the samples of every Python run (`py<N>.jsonl`), for as many pairs as `pairs.txt` records.
pub fn load_real_runs(dir: &Path) -> Result<(Vec<Line>, Vec<Sample>), Failure> {
    let count = pairs_to_read(dir)?;
    let (mut rust, mut python) = (Vec::new(), Vec::new());
    for n in 1..=count.pairs {
        rust.extend(load_run(&dir.join(format!("rs{n}.jsonl")))?);
        python.extend(
            read_samples(&dir.join(format!("py{n}.jsonl"))).map_err(|why| why.to_string())?,
        );
    }
    Ok((rust, python))
}

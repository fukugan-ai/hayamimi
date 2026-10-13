//! RB-5: the Python server's delays `L` and `L_asr`, measured by playing
//! the same recording through `/ingest` more than once.
//!
//! The inputs are, for each run, the samples file `lat_ja_0.35_<n>.jsonl`,
//! the WebSocket capture `lat_ja_0.35_<n>.events.jsonl` and the server's
//! standard output `server_0.35_<n>.out.log`, and the RB-5 section of
//! `docs/results/baseline_python.md`.
//!
//! The frame of a run (utterances times repeats) splits into valid samples,
//! samples without a `final`, and samples whose end is wrong.

mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::time::Instant;

use hayamimi_eval::error::EvalError;
use hayamimi_eval::latency::{Event, parse_event, positional_translations};
use hayamimi_eval::{percentile, stream};
use serde_json::Value;
use support::{
    BASE, FIXTURES, Failure, RESULTS, Row, TESTDATA, TestResult, load_rows, nearest_rank,
    parse_rows, read, read_json, run_files, run_files_in, section, table_rows,
};

const SILENCE: &str = "0.35";
/// The `--min-silence` of the runs, in milliseconds (`SILENCE` in seconds).
const MIN_SILENCE_MS: i64 = 350;
/// How many times `hayamimi-eval stream --repeat` plays the recording.
const REPEATS: usize = 5;
/// Two means of the server's `latency_ms` agree when they differ by at most
/// 0.5 ms, in units of 10^-6 ms. The log rounds each value to whole
/// milliseconds, which moves a mean by at most half a millisecond.
const MEAN_TOLERANCE_MICROS: i64 = 500_000;
/// The utterances the frame fixtures are built for.
const FIXTURE_UTTERANCES: usize = 23;

/// Where a sample stands against the frame of its run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// A `final` arrived and the silence the server waited for fits in `L_asr`.
    Valid,
    /// No `final` arrived in the sample's span.
    NoFinal,
    /// A `final` arrived, but `t_end` lies after the real end of the speech.
    BadEnd,
}

#[derive(Debug, PartialEq, Eq)]
struct Split {
    valid: usize,
    no_final: usize,
    bad_end: usize,
}

fn ms_to_micros(ms: f64) -> i64 {
    (ms * 1_000_000.0).round() as i64
}

/// `L_asr - L_dec` in units of 10^-6 ms, from `L_asr` in microseconds and
/// the server's own `latency_ms`.
fn silence_gap_micros(l_asr_us: i64, l_dec_ms: f64) -> i64 {
    l_asr_us * 1000 - ms_to_micros(l_dec_ms)
}

fn classify(row: &Row, min_silence_ms: i64) -> Result<Kind, Failure> {
    let Some(t_final) = row.t_final else {
        return Ok(Kind::NoFinal);
    };
    let Some(l_dec) = row.final_latency_ms else {
        return Err(format!(
            "utterance {} repeat {}: a final without latency_ms",
            row.utt, row.repeat
        ));
    };
    let gap = silence_gap_micros(t_final - row.t_end, l_dec);
    Ok(if gap >= min_silence_ms * 1_000_000 {
        Kind::Valid
    } else {
        Kind::BadEnd
    })
}

fn classify_all(rows: &[Row], min_silence_ms: i64) -> Result<Vec<Kind>, Failure> {
    rows.iter()
        .map(|row| classify(row, min_silence_ms))
        .collect()
}

fn split(rows: &[Row], min_silence_ms: i64) -> Result<Split, Failure> {
    let mut counts = Split {
        valid: 0,
        no_final: 0,
        bad_end: 0,
    };
    for kind in classify_all(rows, min_silence_ms)? {
        match kind {
            Kind::Valid => counts.valid += 1,
            Kind::NoFinal => counts.no_final += 1,
            Kind::BadEnd => counts.bad_end += 1,
        }
    }
    Ok(counts)
}

fn valid_rows(rows: &[Row], min_silence_ms: i64) -> Result<Vec<&Row>, Failure> {
    let kinds = classify_all(rows, min_silence_ms)?;
    Ok(rows
        .iter()
        .zip(kinds)
        .filter(|(_, kind)| *kind == Kind::Valid)
        .map(|(row, _)| row)
        .collect())
}

/// The utterances of the recording, counted in its manifest.
fn utterance_count() -> Result<usize, Failure> {
    let path = Path::new(TESTDATA).join("manifests").join("lat-ja.json");
    let manifest = read_json(&path)?;
    match manifest.as_array() {
        Some(items) if !items.is_empty() => Ok(items.len()),
        _ => Err(format!("{} is not a non-empty array", path.display())),
    }
}

/// The samples are exactly one per (utterance, repeat) pair of the frame.
fn check_frame(rows: &[Row], utterances: usize) -> Result<(), Failure> {
    let frame = utterances * REPEATS;
    if rows.len() != frame {
        return Err(format!(
            "{} samples, the frame is {frame} ({utterances} utterances x {REPEATS} repeats)",
            rows.len()
        ));
    }
    let mut seen = BTreeSet::new();
    for row in rows {
        let inside = usize::try_from(row.utt).is_ok_and(|utt| (1..=utterances).contains(&utt))
            && usize::try_from(row.repeat).is_ok_and(|repeat| (1..=REPEATS).contains(&repeat));
        if !inside {
            return Err(format!(
                "utterance {} repeat {} is outside the frame",
                row.utt, row.repeat
            ));
        }
        if !seen.insert((row.utt, row.repeat)) {
            return Err(format!(
                "utterance {} repeat {} appears twice",
                row.utt, row.repeat
            ));
        }
    }
    Ok(())
}

fn summary_section() -> Result<String, Failure> {
    let summary = read(&Path::new(RESULTS).join("baseline_python.md"))?;
    Ok(section(&summary, "RB-5")?.to_owned())
}

fn runs() -> Result<Vec<(u32, Vec<Row>)>, Failure> {
    let loaded = run_files(SILENCE)
        .into_iter()
        .map(|(n, path)| Ok((n, load_rows(&path)?)))
        .collect::<Result<Vec<_>, Failure>>()?;
    if loaded.is_empty() {
        return Err(format!("no samples file for --min-silence {SILENCE}"));
    }
    Ok(loaded)
}

fn run_file(n: u32, suffix: &str) -> PathBuf {
    Path::new(BASE).join(format!("lat_ja_{SILENCE}_{n}.{suffix}"))
}

fn server_log(n: u32) -> PathBuf {
    Path::new(BASE).join(format!("server_{SILENCE}_{n}.out.log"))
}

fn check_run_count(files: &[(u32, PathBuf)]) -> Result<(), Failure> {
    if files.len() >= 2 {
        Ok(())
    } else {
        Err(format!("{} runs, want at least 2", files.len()))
    }
}

/// A run that never reached `/ingest` has no `final` at all.
fn check_has_finals(rows: &[Row]) -> Result<(), Failure> {
    if rows.iter().any(|row| row.t_final.is_some()) {
        Ok(())
    } else {
        Err(String::from("t_final is empty in all samples: 0 finals"))
    }
}

/// The `final` events of a WebSocket capture as `(t_us, latency_ms)`.
fn ws_finals(text: &str) -> Result<Vec<(i64, f64)>, Failure> {
    let mut found = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let value: Value = serde_json::from_str(line)
            .map_err(|error| format!("an event line is not JSON: {error}"))?;
        if value.pointer("/event/type").and_then(Value::as_str) != Some("final") {
            continue;
        }
        let Some(t_us) = value.get("t_us").and_then(Value::as_i64) else {
            return Err(String::from("a final event has no t_us"));
        };
        let Some(latency) = value.pointer("/event/latency_ms").and_then(Value::as_f64) else {
            return Err(String::from("a final event has no latency_ms"));
        };
        found.push((t_us, latency));
    }
    Ok(found)
}

/// Every `latency=<value>ms` the server printed, in milliseconds.
fn log_latencies(text: &str) -> Result<Vec<f64>, Failure> {
    let mut found = Vec::new();
    for line in text.lines() {
        let Some((_, tail)) = line.rsplit_once("latency=") else {
            continue;
        };
        let Some((number, _)) = tail.split_once("ms") else {
            return Err(format!("the latency in {line:?} has no unit"));
        };
        let Ok(value) = number.parse::<f64>() else {
            return Err(format!("{number:?} is not a latency"));
        };
        found.push(value);
    }
    Ok(found)
}

fn mean_micros(values: &[i64]) -> Result<i64, Failure> {
    let Ok(count) = i64::try_from(values.len()) else {
        return Err(String::from("too many values"));
    };
    match values.iter().sum::<i64>().checked_div(count) {
        Some(mean) => Ok(mean),
        None => Err(String::from("the mean of no values")),
    }
}

/// The WebSocket capture and the server's log hold the same `final`
/// events: the same number of them, and means that differ by at most 0.5 ms.
/// The values are in units of 10^-6 ms.
fn check_final_latency_agreement(ws: &[i64], log: &[i64]) -> Result<(), Failure> {
    if ws.is_empty() && log.is_empty() {
        return Err(String::from("0 finals in the capture and 0 in the log"));
    }
    if ws.len() != log.len() {
        return Err(format!(
            "{} finals in the capture, {} in the log",
            ws.len(),
            log.len()
        ));
    }
    let gap = (mean_micros(ws)? - mean_micros(log)?).abs();
    if gap > MEAN_TOLERANCE_MICROS {
        return Err(format!(
            "the means differ by {gap} (10^-6 ms), the limit is {MEAN_TOLERANCE_MICROS}"
        ));
    }
    Ok(())
}

fn final_latency_agreement_of_run(n: u32) -> Result<(), Failure> {
    let ws: Vec<i64> = ws_finals(&read(&run_file(n, "events.jsonl"))?)?
        .iter()
        .map(|(_, latency)| ms_to_micros(*latency))
        .collect();
    let log: Vec<i64> = log_latencies(&read(&server_log(n))?)?
        .into_iter()
        .map(ms_to_micros)
        .collect();
    check_final_latency_agreement(&ws, &log).map_err(|error| format!("run {n}: {error}"))
}

/// Each valid sample's `final` is found in the capture at the same instant,
/// carries the same `latency_ms`, and leaves at least `min_silence` of
/// `L_asr` after that latency is taken off.
fn check_valid_against_capture(rows: &[Row], capture: &str) -> Result<(), Failure> {
    let finals = ws_finals(capture)?;
    for (row, kind) in rows.iter().zip(classify_all(rows, MIN_SILENCE_MS)?) {
        let (Kind::Valid, Some(t_final)) = (kind, row.t_final) else {
            continue;
        };
        let Some((_, latency)) = finals.iter().find(|(t_us, _)| *t_us == t_final) else {
            return Err(format!(
                "utterance {} repeat {}: no final event at t_us {t_final}",
                row.utt, row.repeat
            ));
        };
        if row.final_latency_ms.map(ms_to_micros) != Some(ms_to_micros(*latency)) {
            return Err(format!(
                "utterance {} repeat {}: the sample and the capture disagree on latency_ms",
                row.utt, row.repeat
            ));
        }
        if silence_gap_micros(t_final - row.t_end, *latency) < MIN_SILENCE_MS * 1_000_000 {
            return Err(format!(
                "utterance {} repeat {}: L_asr - L_dec is below {MIN_SILENCE_MS} ms",
                row.utt, row.repeat
            ));
        }
    }
    Ok(())
}

fn p50_p95(values: &[i64]) -> Result<(i64, i64), Failure> {
    Ok((nearest_rank(values, 50)?, nearest_rank(values, 95)?))
}

fn l_values(rows: &[&Row]) -> Vec<i64> {
    rows.iter().filter_map(|row| row.l_ms()).collect()
}

fn l_asr_values(rows: &[&Row]) -> Vec<i64> {
    rows.iter().filter_map(|row| row.l_asr_ms()).collect()
}

fn l_tr_values(rows: &[&Row]) -> Vec<i64> {
    rows.iter().filter_map(|row| row.l_tr_ms()).collect()
}

fn negative_l_tr(rows: &[&Row]) -> usize {
    l_tr_values(rows).iter().filter(|v| **v < 0).count()
}

fn untranslated(rows: &[&Row]) -> usize {
    rows.iter().filter(|row| row.t_tr.is_none()).count()
}

/// The cells a run's row of the summary table must carry after its number:
/// `L_asr` is taken over the valid samples, `L` and `L_tr` over the valid
/// samples that have a translation.
fn expected_cells(rows: &[Row]) -> Result<Vec<String>, Failure> {
    let valid = valid_rows(rows, MIN_SILENCE_MS)?;
    let (l50, l95) = p50_p95(&l_values(&valid))?;
    let (a50, a95) = p50_p95(&l_asr_values(&valid))?;
    let (t50, t95) = p50_p95(&l_tr_values(&valid))?;
    Ok([l50, l95, a50, a95, t50, t95]
        .iter()
        .map(i64::to_string)
        .chain([
            negative_l_tr(&valid).to_string(),
            untranslated(&valid).to_string(),
        ])
        .collect())
}

/// Each run has a row whose cells are the recomputed percentiles and counts.
fn check_summary_rows(runs: &[(u32, Vec<Row>)], section_text: &str) -> Result<(), Failure> {
    let table = table_rows(section_text);
    let Some(head) = table.iter().position(|cells| cells.first() == Some(&"run")) else {
        return Err(String::from("no per-run table"));
    };
    let width = table[head].len();
    let body: Vec<&Vec<&str>> = table[head + 1..]
        .iter()
        .take_while(|cells| cells.len() == width)
        .collect();
    for (n, rows) in runs {
        let want = expected_cells(rows)?;
        let found = body
            .iter()
            .find(|cells| cells.first().is_some_and(|first| *first == n.to_string()));
        match found {
            Some(cells) if cells[1..] == want[..] => {}
            Some(cells) => {
                return Err(format!(
                    "run {n}: summary {:?}, recomputed {want:?}",
                    &cells[1..]
                ));
            }
            None => return Err(format!("run {n} has no row in the summary")),
        }
    }
    Ok(())
}

/// The median over the valid translated samples of each utterance, by
/// nearest rank; an utterance with none maps to `None`.
fn utterance_medians(rows: &[Row]) -> Result<BTreeMap<i64, Option<i64>>, Failure> {
    let mut by_utterance: BTreeMap<i64, Vec<i64>> = BTreeMap::new();
    for (row, kind) in rows.iter().zip(classify_all(rows, MIN_SILENCE_MS)?) {
        let values = by_utterance.entry(row.utt).or_default();
        if let (Kind::Valid, Some(l)) = (kind, row.l_ms()) {
            values.push(l);
        }
    }
    by_utterance
        .into_iter()
        .map(|(utt, values)| {
            if values.is_empty() {
                Ok((utt, None))
            } else {
                Ok((utt, Some(nearest_rank(&values, 50)?)))
            }
        })
        .collect()
}

/// The summary lists every utterance with its median `L` in each run; the
/// result is the number of utterances listed.
fn check_summary_medians(runs: &[(u32, Vec<Row>)], section_text: &str) -> Result<usize, Failure> {
    let table = table_rows(section_text);
    let Some(head) = table.iter().position(|cells| cells.first() == Some(&"utt")) else {
        return Err(String::from("no per-utterance table"));
    };
    let body: Vec<&Vec<&str>> = table[head + 1..]
        .iter()
        .take_while(|cells| cells.first().is_some_and(|c| c.parse::<i64>().is_ok()))
        .collect();
    let per_run: Vec<BTreeMap<i64, Option<i64>>> = runs
        .iter()
        .map(|(_, rows)| utterance_medians(rows))
        .collect::<Result<_, _>>()?;
    let Some(first) = per_run.first() else {
        return Err(String::from("no runs"));
    };
    if body.len() != first.len() {
        return Err(format!(
            "{} rows for {} utterances",
            body.len(),
            first.len()
        ));
    }
    let listed = body.len();
    for cells in body {
        let Ok(utt) = cells[0].parse::<i64>() else {
            return Err(format!("{:?} is not an utterance number", cells[0]));
        };
        for (k, medians) in per_run.iter().enumerate() {
            let want = match medians.get(&utt) {
                Some(Some(value)) => value.to_string(),
                Some(None) => String::from("-"),
                None => return Err(format!("utterance {utt} is not in run {}", k + 1)),
            };
            if cells.get(k + 1) != Some(&want.as_str()) {
                return Err(format!("utterance {utt}, run {}: want {want}", k + 1));
            }
        }
    }
    Ok(listed)
}

/// The recorded difference of the `L` p50 between run 1 and run 2.
fn check_p50_difference(runs: &[(u32, Vec<Row>)], section_text: &str) -> Result<(), Failure> {
    let [(_, first), (_, second), ..] = runs else {
        return Err(String::from("fewer than two runs"));
    };
    let first_l = l_values(&valid_rows(first, MIN_SILENCE_MS)?);
    let second_l = l_values(&valid_rows(second, MIN_SILENCE_MS)?);
    let want = (nearest_rank(&first_l, 50)? - nearest_rank(&second_l, 50)?).abs();
    let marker = "run 1 と run 2 の L の p50 の差：";
    let Some(line) = section_text.lines().find(|line| line.starts_with(marker)) else {
        return Err(String::from("no line with the p50 difference"));
    };
    let said = line[marker.len()..].trim().trim_end_matches("ms").trim();
    if said == want.to_string() {
        Ok(())
    } else {
        Err(format!(
            "the summary says {said:?} ms, recomputed {want} ms"
        ))
    }
}

fn events_of(text: &str) -> Result<Vec<Event>, Failure> {
    let mut events = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let Ok(at) = i64::try_from(index) else {
            return Err(String::from("too many events"));
        };
        events.push(parse_event(at * 1000, line).map_err(|e| e.to_string())?);
    }
    Ok(events)
}

#[test]
fn rb05_python_latency_has_p50_p95_per_run_at_least_two_runs_exist() {
    assert!(check_run_count(&run_files(SILENCE)).is_ok());
}

#[test]
fn rb05_python_latency_has_p50_p95_per_run_single_run_is_rejected() {
    let one = run_files_in(&Path::new(FIXTURES).join("rb05_one_run"), SILENCE);
    assert_eq!(one.len(), 1);
    assert!(check_run_count(&one).is_err());
}

#[test]
fn rb05_python_latency_has_p50_p95_per_run_each_run_splits_its_frame_into_valid_no_final_and_bad_end()
-> TestResult {
    let utterances = utterance_count()?;
    for (n, rows) in runs()? {
        check_frame(&rows, utterances).map_err(|e| format!("run {n}: {e}"))?;
        let counts = split(&rows, MIN_SILENCE_MS)?;
        assert_eq!(
            counts.valid + counts.no_final + counts.bad_end,
            utterances * REPEATS,
            "run {n}: {counts:?}"
        );
        assert!(counts.valid > 0, "run {n}: {counts:?}");
        assert!(counts.no_final > 0, "run {n}: {counts:?}");
        assert!(counts.bad_end > 0, "run {n}: {counts:?}");
    }
    Ok(())
}

#[test]
fn rb05_python_latency_has_p50_p95_per_run_translated_plus_untranslated_valid_samples_equal_valid_samples()
-> TestResult {
    for (n, rows) in runs()? {
        let valid = valid_rows(&rows, MIN_SILENCE_MS)?;
        let translated = l_values(&valid).len();
        assert_eq!(translated + untranslated(&valid), valid.len(), "run {n}");
    }
    Ok(())
}

#[test]
fn rb05_python_latency_has_p50_p95_per_run_valid_samples_satisfy_asr_minus_dec_at_least_min_silence()
-> TestResult {
    for (n, rows) in runs()? {
        let capture = read(&run_file(n, "events.jsonl"))?;
        check_valid_against_capture(&rows, &capture).map_err(|e| format!("run {n}: {e}"))?;
    }
    Ok(())
}

#[test]
fn rb05_python_latency_has_p50_p95_per_run_l_asr_is_positive_for_every_sample() -> TestResult {
    for (n, rows) in runs()? {
        for delay in rows
            .iter()
            .filter_map(|row| row.t_final.map(|f| f - row.t_end))
        {
            assert!(delay > 0, "run {n}: t_final - t_end = {delay}");
        }
    }
    Ok(())
}

#[test]
fn rb05_python_latency_has_p50_p95_per_run_percentiles_of_115_valid_samples_are_58_and_110_and_skip_the_others()
-> TestResult {
    let rows = parse_rows(include_str!("fixtures/rb05_known_valid.jsonl"))?;
    assert_eq!(rows.len(), 120);
    let valid = valid_rows(&rows, 0)?;
    assert_eq!(valid.len(), 115);
    let values = l_asr_values(&valid);
    assert_eq!(p50_p95(&values)?, (58, 110));
    assert_eq!(percentile::nearest_rank(&values, 50)?, 58);
    assert_eq!(percentile::nearest_rank(&values, 95)?, 110);
    // The control: the two samples with a bad end, counted in, move both.
    let counted: Vec<i64> = rows.iter().filter_map(Row::l_asr_ms).collect();
    assert_eq!(counted.len(), 117);
    assert_eq!(p50_p95(&counted)?, (59, 112));
    Ok(())
}

#[test]
fn rb05_python_latency_has_p50_p95_per_run_p95_of_20_samples_is_the_19th() -> TestResult {
    let values: Vec<i64> = (1..=20).collect();
    assert_eq!(nearest_rank(&values, 95)?, 19);
    assert_eq!(percentile::nearest_rank(&values, 95)?, 19);
    Ok(())
}

#[test]
fn rb05_python_latency_has_p50_p95_per_run_p95_of_21_samples_is_the_20th() -> TestResult {
    let values: Vec<i64> = (1..=21).collect();
    assert_eq!(nearest_rank(&values, 95)?, 20);
    assert_eq!(percentile::nearest_rank(&values, 95)?, 20);
    Ok(())
}

#[test]
fn rb05_python_latency_has_p50_p95_per_run_single_sample_percentiles_are_that_sample() -> TestResult
{
    assert_eq!(p50_p95(&[7])?, (7, 7));
    assert_eq!(percentile::nearest_rank(&[7], 50)?, 7);
    assert_eq!(percentile::nearest_rank(&[7], 95)?, 7);
    Ok(())
}

#[test]
fn rb05_python_latency_has_p50_p95_per_run_percentile_of_no_samples_is_an_error() {
    assert!(nearest_rank(&[], 50).is_err());
    assert!(percentile::nearest_rank(&[], 50).is_err());
}

#[test]
fn rb05_python_latency_has_p50_p95_per_run_baseline_summary_matches_recomputed_percentiles()
-> TestResult {
    check_summary_rows(&runs()?, &summary_section()?)?;
    Ok(())
}

#[test]
fn rb05_python_latency_has_p50_p95_per_run_median_of_five_runs_is_the_third_sorted_value()
-> TestResult {
    assert_eq!(nearest_rank(&[10, 30, 20, 50, 40], 50)?, 30);
    assert_eq!(percentile::nearest_rank(&[10, 30, 20, 50, 40], 50)?, 30);
    Ok(())
}

#[test]
fn rb05_python_latency_has_p50_p95_per_run_baseline_summary_lists_23_per_utterance_medians()
-> TestResult {
    let listed = check_summary_medians(&runs()?, &summary_section()?)?;
    assert_eq!(listed, utterance_count()?);
    Ok(())
}

#[test]
fn rb05_python_latency_has_p50_p95_per_run_negative_l_tr_samples_are_counted() -> TestResult {
    let rows = parse_rows(include_str!("fixtures/rb05_negative_l_tr.jsonl"))?;
    let all: Vec<&Row> = rows.iter().collect();
    assert_eq!(negative_l_tr(&all), 1);
    Ok(())
}

#[test]
fn rb05_python_latency_has_p50_p95_per_run_translation_pairs_with_the_preceding_final_until_the_next_final()
-> TestResult {
    let events = events_of(include_str!("fixtures/rb05_pairing_basic.jsonl"))?;
    assert_eq!(
        positional_translations(&events),
        vec![(0, Some(1)), (2, None), (3, Some(4))]
    );
    Ok(())
}

#[test]
fn rb05_python_latency_has_p50_p95_per_run_first_translation_after_a_final_wins() -> TestResult {
    let events = events_of(include_str!("fixtures/rb05_pairing_two_translations.jsonl"))?;
    assert_eq!(
        positional_translations(&events),
        vec![(0, Some(1)), (3, None)]
    );
    Ok(())
}

#[test]
fn rb05_python_latency_has_p50_p95_per_run_translation_before_any_final_is_not_paired() -> TestResult
{
    let events = events_of(include_str!("fixtures/rb05_translation_first.jsonl"))?;
    assert_eq!(positional_translations(&events), vec![(1, None)]);
    Ok(())
}

#[test]
fn rb05_python_latency_has_p50_p95_per_run_p50_difference_between_runs_is_recorded() -> TestResult {
    check_p50_difference(&runs()?, &summary_section()?)?;
    Ok(())
}

#[test]
fn rb05_python_latency_has_p50_p95_per_run_ws_and_log_final_counts_must_be_equal() {
    let said = check_final_latency_agreement(&[266_800_000; 122], &[266_800_000; 123]);
    assert!(matches!(&said, Err(text) if text.contains("122") && text.contains("123")));
}

#[test]
fn rb05_python_latency_has_p50_p95_per_run_final_latency_mean_difference_edge_is_0_5_ms() {
    let ws = [266_800_000; 123];
    assert!(check_final_latency_agreement(&ws, &[267_300_000; 123]).is_ok());
    assert!(check_final_latency_agreement(&ws, &[267_300_001; 123]).is_err());
}

#[test]
fn rb05_python_latency_has_p50_p95_per_run_ws_and_log_final_latency_means_agree_within_0_5_ms()
-> TestResult {
    for (n, _) in runs()? {
        final_latency_agreement_of_run(n)?;
    }
    Ok(())
}

#[test]
fn rb05_python_latency_has_p50_p95_per_run_run_without_final_events_is_an_error() -> TestResult {
    let rows = parse_rows(include_str!("fixtures/rb05_no_final.jsonl"))?;
    assert_eq!(rows.len(), 3);
    let said = check_has_finals(&rows);
    assert!(matches!(&said, Err(text) if text.contains("t_final") && text.contains("0 finals")));
    Ok(())
}

#[test]
fn rb05_python_latency_has_p50_p95_per_run_stream_against_closed_ingest_port_is_a_connect_error()
-> TestResult {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    let opened = stream::open_ingest("127.0.0.1", port, Instant::now());
    assert!(matches!(opened, Err(EvalError::Connect { .. })));
    Ok(())
}

#[test]
fn rb05_python_latency_has_p50_p95_per_run_missing_latency_run_names_its_path() {
    let absent = Path::new(FIXTURES)
        .join("absent")
        .join("lat_ja_0.35_1.jsonl");
    assert!(matches!(load_rows(&absent), Err(path) if path.ends_with("lat_ja_0.35_1.jsonl")));
}

#[test]
fn rb05_python_latency_has_p50_p95_per_run_asr_minus_dec_of_exactly_min_silence_is_valid_and_one_microsecond_less_is_bad_end()
-> TestResult {
    let rows = parse_rows(include_str!("fixtures/rb05_split_boundary.jsonl"))?;
    assert_eq!(
        classify_all(&rows, MIN_SILENCE_MS)?,
        vec![Kind::Valid, Kind::BadEnd, Kind::NoFinal]
    );
    assert_eq!(
        split(&rows, MIN_SILENCE_MS)?,
        Split {
            valid: 1,
            no_final: 1,
            bad_end: 1
        }
    );
    Ok(())
}

#[test]
fn rb05_python_latency_has_p50_p95_per_run_zero_finals_on_both_sides_is_an_error_not_an_agreement()
{
    let said = check_final_latency_agreement(&[], &[]);
    assert!(matches!(&said, Err(text) if text.contains('0')));
}

#[test]
fn rb05_python_latency_has_p50_p95_per_run_run_with_a_missing_sample_is_rejected_by_the_frame_check()
-> TestResult {
    let rows = parse_rows(include_str!("fixtures/rb05_frame_114.jsonl"))?;
    assert_eq!(rows.len(), 114);
    let said = check_frame(&rows, FIXTURE_UTTERANCES);
    assert!(matches!(&said, Err(text) if text.contains("114") && text.contains("23")));
    Ok(())
}

#[test]
fn rb05_python_latency_has_p50_p95_per_run_run_with_a_duplicated_pair_is_rejected_by_the_frame_check()
-> TestResult {
    let rows = parse_rows(include_str!("fixtures/rb05_frame_duplicate.jsonl"))?;
    assert_eq!(rows.len(), FIXTURE_UTTERANCES * REPEATS);
    let said = check_frame(&rows, FIXTURE_UTTERANCES);
    assert!(matches!(&said, Err(text) if text.contains("utterance 1 repeat 1")));
    Ok(())
}

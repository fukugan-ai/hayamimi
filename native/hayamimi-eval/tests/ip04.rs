//! IP-4: what the choice of the language adds to the wait for the closing
//! `final` (`L_asr`). This is only recorded: a delta over the line is written
//! down as "線の外" and does not fail the test.
//!
//! The delta of a clip is the median over its repeats of `L_asr` in the run
//! whose language is chosen by `--mode pair`, minus the same median in the runs
//! that were told the language (`--lang`). Clips are tied by the sha256 of
//! their wav, taken from the manifest of each run, never by position.
//!
//! The real row (T-IP-04-001) reads the records of three runs taken one after
//! the other on one machine, in `docs/results/in-person/`:
//! `ip02_pair.jsonl` and `ip02_pair.log` (IP-2's run of `lat-dialog`),
//! `ip04_lang_ja.jsonl` (`lat-ja` with `--lang ja`) and `ip04_lang_en.jsonl`
//! (`lat-en` with `--lang en`), each with the `.log` that `hayamimi` printed. It
//! is `#[ignore]`d and runs with `-- --ignored`. Every other row feeds the tools
//! below samples made by hand.

mod support;

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use hayamimi_eval::compare::read_samples;
use hayamimi_eval::latency::{Sample, Stat, stat_of};
use hayamimi_eval::manifest::read_rows;
use hayamimi_eval::percentile::{micros_to_ms, nearest_rank};
use serde_json::Value;
use support::{Failure, TestResult, find_up, read};

/// The provisional lines of the delta, in microseconds.
const P50_LINE_US: i64 = 150_000;
const P95_LINE_US: i64 = 300_000;
const RECORDS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/results/in-person");

fn to_failure(error: impl std::fmt::Display) -> Failure {
    error.to_string()
}

/// For each clip (by sha256), the median over its repeats of `L_asr` in
/// microseconds. A repeat with no closing final has no `L_asr` and is left out;
/// `shas[utt - 1]` is the clip of utterance `utt`.
fn medians_by_clip(samples: &[Sample], shas: &[String]) -> Result<BTreeMap<String, i64>, Failure> {
    let mut waits: BTreeMap<&str, Vec<i64>> = BTreeMap::new();
    for sample in samples {
        let Some(sha) = usize::try_from(sample.utt)
            .ok()
            .and_then(|utt| utt.checked_sub(1))
            .and_then(|at| shas.get(at))
        else {
            return Err(format!("utt {} has no row in the manifest", sample.utt));
        };
        if let Some(wait) = sample.l_asr_us() {
            waits.entry(sha).or_default().push(wait);
        }
    }
    waits
        .into_iter()
        .map(|(sha, values)| {
            let median = nearest_rank(&values, 50).map_err(to_failure)?;
            Ok((sha.to_owned(), median))
        })
        .collect()
}

/// The deltas of the clips both runs read, and the clips of the first run the
/// second did not read.
#[derive(Debug, PartialEq, Eq)]
struct Deltas {
    values_us: Vec<i64>,
    unjoined: Vec<String>,
}

/// Ties the clips of two runs by sha256. Nothing in common is an error: a
/// percentile of no clip would read as a delta of 0.
fn deltas(pair: &BTreeMap<String, i64>, forced: &BTreeMap<String, i64>) -> Result<Deltas, Failure> {
    let mut found = Deltas {
        values_us: Vec::new(),
        unjoined: Vec::new(),
    };
    for (sha, wait) in pair {
        match forced.get(sha) {
            Some(told) => found.values_us.push(wait - told),
            None => found.unjoined.push(sha.clone()),
        }
    }
    if found.values_us.is_empty() {
        return Err(String::from("the two runs have no clip in common"));
    }
    Ok(found)
}

/// Where the deltas of a run stand against the two lines.
#[derive(Debug, PartialEq, Eq)]
struct DeltaVerdict {
    joined: usize,
    p50_us: i64,
    p95_us: i64,
}

impl DeltaVerdict {
    fn p50_inside(&self) -> bool {
        self.p50_us <= P50_LINE_US
    }

    fn p95_inside(&self) -> bool {
        self.p95_us <= P95_LINE_US
    }
}

fn judge_delta(values_us: &[i64]) -> Result<DeltaVerdict, Failure> {
    Ok(DeltaVerdict {
        joined: values_us.len(),
        p50_us: nearest_rank(values_us, 50).map_err(to_failure)?,
        p95_us: nearest_rank(values_us, 95).map_err(to_failure)?,
    })
}

/// `Ok` when both percentiles are on their lines; otherwise an error that says
/// which percentile is over its line, by how it reads in milliseconds.
fn check_delta(values_us: &[i64]) -> Result<DeltaVerdict, Failure> {
    let verdict = judge_delta(values_us)?;
    let mut over = Vec::new();
    if !verdict.p50_inside() {
        over.push(format!(
            "p50 is {} ms, over the line of {} ms",
            micros_to_ms(verdict.p50_us),
            micros_to_ms(P50_LINE_US)
        ));
    }
    if !verdict.p95_inside() {
        over.push(format!(
            "p95 is {} ms, over the line of {} ms",
            micros_to_ms(verdict.p95_us),
            micros_to_ms(P95_LINE_US)
        ));
    }
    if over.is_empty() {
        Ok(verdict)
    } else {
        Err(over.join("; "))
    }
}

fn side(inside: bool) -> &'static str {
    if inside { "線の内" } else { "線の外" }
}

/// Appends the deltas of a run to the record file, each percentile with its
/// line and the side it stands. A delta over a line is written down, not failed.
fn record_delta(path: &Path, values_us: &[i64]) -> Result<DeltaVerdict, Failure> {
    let verdict = judge_delta(values_us)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| format!("cannot open {}: {error}", path.display()))?;
    let lines = [
        format!("joined clips: {}", verdict.joined),
        format!(
            "delta p50: {} ms (線 {} ms) {}",
            micros_to_ms(verdict.p50_us),
            micros_to_ms(P50_LINE_US),
            side(verdict.p50_inside())
        ),
        format!(
            "delta p95: {} ms (線 {} ms) {}",
            micros_to_ms(verdict.p95_us),
            micros_to_ms(P95_LINE_US),
            side(verdict.p95_inside())
        ),
    ];
    for line in lines {
        writeln!(file, "{line}").map_err(|error| format!("cannot write the record: {error}"))?;
    }
    Ok(verdict)
}

/// The percentiles of `lid_ms` over the `asr.utterance.timed` lines of a log.
/// A log with no such line is an error, so that no line is not read as 0 ms.
fn lid_stat(log_text: &str) -> Result<Stat, Failure> {
    let mut values_us = Vec::new();
    for line in log_text.lines().filter(|line| !line.trim().is_empty()) {
        let value: Value = serde_json::from_str(line).map_err(to_failure)?;
        if value.get("event").and_then(Value::as_str) != Some("asr.utterance.timed") {
            continue;
        }
        let lid_ms = value
            .get("lid_ms")
            .and_then(Value::as_f64)
            .ok_or("an asr.utterance.timed line has no number `lid_ms`")?;
        values_us.push((lid_ms * 1000.0).round() as i64);
    }
    stat_of(&values_us)
        .map_err(to_failure)?
        .ok_or_else(|| String::from("the log has no asr.utterance.timed line"))
}

/// `L_asr` of the first repeat, of the repeats after it, and of all of them.
struct ByRepeat {
    first: Stat,
    warm: Stat,
    all: Stat,
}

fn by_repeat(samples: &[Sample]) -> Result<ByRepeat, Failure> {
    let waits = |wanted: &dyn Fn(u32) -> bool| -> Result<Stat, Failure> {
        let values: Vec<i64> = samples
            .iter()
            .filter(|sample| wanted(sample.repeat))
            .filter_map(Sample::l_asr_us)
            .collect();
        stat_of(&values)
            .map_err(to_failure)?
            .ok_or_else(|| String::from("no repeat of this kind has an L_asr"))
    };
    Ok(ByRepeat {
        first: waits(&|repeat| repeat == 1)?,
        warm: waits(&|repeat| repeat > 1)?,
        all: waits(&|_| true)?,
    })
}

fn sha_of_each_row(manifest: &Path) -> Result<Vec<String>, Failure> {
    read_rows(manifest)
        .map_err(to_failure)?
        .iter()
        .enumerate()
        .map(|(at, row)| {
            row.get("sha256")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| format!("row {at} of {} has no `sha256`", manifest.display()))
        })
        .collect()
}

/// A run's medians by clip: its samples file, tied to the manifest of its recording.
fn clips_of_run(samples: &Path, manifest: &str) -> Result<BTreeMap<String, i64>, Failure> {
    let shas = sha_of_each_row(&find_up(manifest)?)?;
    medians_by_clip(&read_samples(samples).map_err(to_failure)?, &shas)
}

/// A sample whose closing `final` came `wait_ms` after the speech ended.
fn sample(repeat: u32, utt: u32, wait_ms: i64) -> Sample {
    Sample {
        repeat,
        utt,
        t_start_us: 0,
        t_end_us: 0,
        t_partial_us: None,
        t_partial_tr_us: None,
        t_final_us: Some(wait_ms * 1000),
        t_tr_us: None,
        finals: 1,
        text: String::new(),
        translation: None,
        final_latency_ms: None,
        cer_dist: 0,
        cer_denom: 0,
    }
}

/// One clip `sha`, utterance 1, with the waits (ms) of its repeats in order.
fn run_of_one_clip(waits_ms: [i64; 3]) -> Vec<Sample> {
    (1..=3)
        .zip(waits_ms)
        .map(|(repeat, wait)| sample(repeat, 1, wait))
        .collect()
}

fn one_sha() -> Vec<String> {
    vec![String::from("a")]
}

fn ms(values: &[i64]) -> Vec<i64> {
    values.iter().map(|value| value * 1000).collect()
}

fn scratch(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name)
}

/// T-IP-04-001
#[test]
#[ignore = "reads testdata or run records, which are not in git"]
fn ip04_language_choice_adds_little_to_the_wait() -> TestResult {
    let records = PathBuf::from(RECORDS);
    let record = records.join("ip04.md");
    std::fs::write(&record, "# IP-4 record\n")?;
    let pair = clips_of_run(
        &records.join("ip02_pair.jsonl"),
        "testdata/manifests/lat-dialog.json",
    )?;
    let mut forced = clips_of_run(
        &records.join("ip04_lang_ja.jsonl"),
        "testdata/manifests/lat-ja.json",
    )?;
    forced.extend(clips_of_run(
        &records.join("ip04_lang_en.jsonl"),
        "testdata/manifests/lat-en.json",
    )?);
    let found = deltas(&pair, &forced)?;
    record_delta(&record, &found.values_us)?;
    let notes = format!(
        "expected joined clips: 30\nclips of the pair run not found in the --lang runs: {:?}\n",
        found.unjoined
    );
    std::fs::OpenOptions::new()
        .append(true)
        .open(&record)?
        .write_all(notes.as_bytes())?;
    let log = read(&records.join("ip02_pair.log"))?;
    let lid = lid_stat(&log)?;
    let samples = read_samples(&records.join("ip02_pair.jsonl")).map_err(to_failure)?;
    let split = by_repeat(&samples)?;
    let stats = [
        ("lid_ms", Some(lid)),
        ("L_asr first repeat", Some(split.first)),
        ("L_asr repeats after the first", Some(split.warm)),
        ("L_asr all repeats", Some(split.all)),
        (
            "L",
            stat_of(&samples.iter().filter_map(Sample::l_us).collect::<Vec<_>>())
                .map_err(to_failure)?,
        ),
        (
            "L_tr",
            stat_of(
                &samples
                    .iter()
                    .filter_map(Sample::l_tr_us)
                    .collect::<Vec<_>>(),
            )
            .map_err(to_failure)?,
        ),
    ];
    let mut file = std::fs::OpenOptions::new().append(true).open(&record)?;
    for (name, stat) in stats {
        match stat {
            Some(stat) => writeln!(
                file,
                "{name}: p50 {} ms, p95 {} ms, n {}",
                stat.p50_ms, stat.p95_ms, stat.n
            )?,
            None => writeln!(file, "{name}: no value")?,
        }
    }
    Ok(())
}

/// T-IP-04-002
#[test]
fn the_delta_of_a_clip_is_the_median_of_its_repeats_in_one_run_minus_the_other() -> TestResult {
    let pair = medians_by_clip(&run_of_one_clip([1000, 1400, 1200]), &one_sha())?;
    let forced = medians_by_clip(&run_of_one_clip([1000, 1000, 1100]), &one_sha())?;
    assert_eq!(deltas(&pair, &forced)?.values_us, ms(&[200]));
    Ok(())
}

/// T-IP-04-003
#[test]
fn a_median_of_three_is_not_a_mean() -> TestResult {
    let pair = medians_by_clip(&run_of_one_clip([1000, 1000, 2000]), &one_sha())?;
    let forced = medians_by_clip(&run_of_one_clip([1000, 1000, 1000]), &one_sha())?;
    assert_eq!(deltas(&pair, &forced)?.values_us, [0]);
    Ok(())
}

/// T-IP-04-004
#[test]
fn a_delta_p50_of_150_ms_is_on_the_line() -> TestResult {
    let verdict = check_delta(&ms(&[149, 150, 151]))?;
    assert_eq!(micros_to_ms(verdict.p50_us), 150);
    assert!(verdict.p50_inside());
    Ok(())
}

/// T-IP-04-005
#[test]
fn a_delta_p50_of_151_ms_is_over_the_line() {
    let said = check_delta(&ms(&[150, 151, 152]));
    assert!(
        matches!(&said, Err(message) if message.contains("p50 is 151 ms") && message.contains("150 ms") && !message.contains("p95")),
        "{said:?}"
    );
}

/// Twenty deltas: eighteen of 0 ms, then `second_largest_ms` and 5000 ms.
fn twenty_with(second_largest_ms: i64) -> Vec<i64> {
    let mut values = vec![0; 18];
    values.extend([second_largest_ms, 5000]);
    ms(&values)
}

/// T-IP-04-006
#[test]
fn a_delta_p95_of_300_ms_is_on_the_line_and_the_largest_clip_does_not_decide_it() -> TestResult {
    let verdict = check_delta(&twenty_with(300))?;
    assert_eq!(micros_to_ms(verdict.p95_us), 300);
    assert!(verdict.p95_inside());
    Ok(())
}

/// T-IP-04-007
#[test]
fn a_delta_p95_of_301_ms_is_over_the_line() {
    let said = check_delta(&twenty_with(301));
    assert!(
        matches!(&said, Err(message) if message.contains("p95 is 301 ms") && message.contains("300 ms") && !message.contains("p50")),
        "{said:?}"
    );
}

/// T-IP-04-008
#[test]
fn a_synthetic_delta_of_400_ms_is_over_both_lines() -> TestResult {
    let all = ms(&[400; 30]);
    let said = check_delta(&all);
    assert!(
        matches!(&said, Err(message) if message.contains("p50") && message.contains("p95")),
        "{said:?}"
    );
    let path = scratch("ip04_over.md");
    std::fs::write(&path, "")?;
    record_delta(&path, &all)?;
    let written = read(&path)?;
    assert_eq!(written.matches("線の外").count(), 2, "{written}");
    assert!(check_delta(&ms(&[149, 150, 151])).is_ok());
    assert!(check_delta(&twenty_with(300)).is_ok());
    Ok(())
}

/// T-IP-04-009
#[test]
fn clips_are_joined_by_sha256_and_not_by_position() -> TestResult {
    let shas: Vec<String> = ["a", "b", "c"].map(String::from).to_vec();
    let backwards: Vec<String> = ["c", "b", "a"].map(String::from).to_vec();
    let samples_of = |waits_ms: [i64; 3]| -> Vec<Sample> {
        (1..=3)
            .zip(waits_ms)
            .map(|(utt, wait)| sample(1, utt, wait))
            .collect()
    };
    let pair = medians_by_clip(&samples_of([1100, 1200, 1300]), &shas)?;
    let forced = medians_by_clip(&samples_of([900, 1000, 1100]), &shas)?;
    let in_order = deltas(&pair, &forced)?.values_us;
    assert_eq!(in_order, ms(&[200, 200, 200]));
    // The same clips played in the reverse order: the forced run's utterance 1 is clip `c`.
    let forced_backwards = medians_by_clip(&samples_of([1100, 1000, 900]), &backwards)?;
    assert_eq!(forced_backwards, forced);
    assert_eq!(deltas(&pair, &forced_backwards)?.values_us, in_order);
    Ok(())
}

/// T-IP-04-010
#[test]
fn a_clip_found_in_one_run_only_is_named_and_a_join_of_nothing_is_an_error() -> TestResult {
    let two: Vec<String> = ["a", "b"].map(String::from).to_vec();
    let three: Vec<String> = ["a", "b", "c"].map(String::from).to_vec();
    let samples =
        |count: u32| -> Vec<Sample> { (1..=count).map(|utt| sample(1, utt, 1000)).collect() };
    let forced = medians_by_clip(&samples(2), &two)?;
    let whole = medians_by_clip(&samples(2), &two)?;
    assert_eq!(deltas(&whole, &forced)?.values_us.len(), 2);
    let pair = medians_by_clip(&samples(3), &three)?;
    let found = deltas(&pair, &forced)?;
    assert_eq!(
        (found.values_us.len(), found.unjoined),
        (2, vec![String::from("c")])
    );
    let strangers = medians_by_clip(&samples(1), &[String::from("z")])?;
    assert!(deltas(&strangers, &forced).is_err());
    Ok(())
}

fn timed_line(lid_ms: f64) -> String {
    serde_json::json!({ "event": "asr.utterance.timed", "id": 1, "lid_ms": lid_ms }).to_string()
}

/// T-IP-04-011
#[test]
fn lid_ms_is_counted_in_a_pair_run_and_is_zero_in_a_forced_language_run() -> TestResult {
    let pair = [90.0, 100.0, 110.0].map(timed_line).join("\n");
    let stat = lid_stat(&pair)?;
    assert_eq!((stat.n, stat.p50_ms, stat.p95_ms), (3, 100, 110));
    let forced = [0.0, 0.0, 0.0].map(timed_line).join("\n");
    let stat = lid_stat(&forced)?;
    assert_eq!((stat.n, stat.p50_ms, stat.p95_ms), (3, 0, 0));
    assert!(lid_stat("").is_err());
    Ok(())
}

/// T-IP-04-012
#[test]
fn the_first_repeat_is_reported_apart_from_the_warm_repeats() -> TestResult {
    let samples: Vec<Sample> = (1..=3)
        .flat_map(|repeat| {
            (1..=10).map(move |utt| sample(repeat, utt, if repeat == 1 { 2000 } else { 1000 }))
        })
        .collect();
    let split = by_repeat(&samples)?;
    assert_eq!(split.first.p50_ms, 2000);
    assert_eq!(split.warm.p50_ms, 1000);
    assert_eq!(split.all.p50_ms, 1000);
    assert_ne!(split.first.p50_ms, split.all.p50_ms);
    Ok(())
}

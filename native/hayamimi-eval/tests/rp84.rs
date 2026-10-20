//! RP-84: the first decode of the recognizer is paid at start-up, so the first
//! utterance of a run decodes as fast as the rest.
//!
//! A run of the X set is judged on two records: the samples file that
//! `hayamimi-eval stream` wrote, and what `hayamimi` printed on stdout.
//!
//! - The first utterance (utt 1, repeat 1) took no longer than the p95 of the
//!   other valid samples of the same run. A sample is valid when its final
//!   came at least 120 ms after the decode ended (`d`). The first utterance is
//!   compared whenever it has a final, whatever its `d` is: what is looked at
//!   is its decode, not its `d`.
//! - Stdout holds exactly one `asr.warmup.decoded`, on a line before the first
//!   `asr.endpoint.decided`, and no `asr.warmup.failed`. The fields of the
//!   `decoded` line are not judged.
//!
//! Final latencies are read as integers of 10^-6 ms (nanoseconds), so that a
//! value one step over the limit is over it. Percentiles are nearest-rank.
//!
//! The fixture cases need no process and no models. The run over the records
//! of the window E3' is `#[ignore]`: it reads `docs/results/r8x/e3p<N>.e2e1.jsonl`
//! and `e3p<N>.e2e1.hayamimi.stdout.log`; a run of any other name is not read.
//!
//! The recorded fixtures under `fixtures/rp84_recorded/` are three runs of a
//! recording made before the start-up decode existed. The samples keep five
//! columns of the original files:
//! `jq -c '{utt, repeat, t_end_us, t_final_us, final_latency_ms}' <run>.jsonl`.
//! The stdout file is a copy of the first run's stdout.

mod support;

use std::path::Path;

use serde_json::{Value, json};
use support::{
    Failure, KEY_FINAL_LATENCY, KEY_REPEAT, KEY_T_END, KEY_T_FINAL, KEY_UTT, RESULTS, TestResult,
    micros, nearest_rank, read,
};

const DECODED: &str = "asr.warmup.decoded";
const FAILED: &str = "asr.warmup.failed";
const DECIDED: &str = "asr.endpoint.decided";
const LOADED: &str = "asr.model.loaded";

const RS1: &str = include_str!("fixtures/rp84_recorded/rs1.samples.jsonl");
const RS2: &str = include_str!("fixtures/rp84_recorded/rs2.samples.jsonl");
const RS3: &str = include_str!("fixtures/rp84_recorded/rs3.samples.jsonl");
const RS1_STDOUT: &str = include_str!("fixtures/rp84_recorded/rs1.hayamimi.stdout.log");

const NS_PER_MS: i64 = 1_000_000;
const NS_PER_US: i64 = 1_000;
/// A sample is valid when `d` is at least this long.
const VALID_D_NS: i64 = 120 * NS_PER_MS;
/// The utterance and the repeat of the sample the start-up decode is for.
const FIRST: (i64, i64) = (1, 1);

/// One line of a samples file.
#[derive(Clone, Debug)]
struct Sample {
    utt: i64,
    repeat: i64,
    t_end_us: i64,
    t_final_us: Option<i64>,
    final_latency_ns: Option<i64>,
}

/// One run of a window: its name, its samples and what `hayamimi` printed.
struct Run {
    name: String,
    samples: Vec<Sample>,
    stdout: String,
}

fn ms_to_ns(ms: f64) -> i64 {
    (ms * 1e6).round() as i64
}

fn parse_sample(line: &str) -> Result<Sample, Failure> {
    let value: Value =
        serde_json::from_str(line).map_err(|why| format!("a sample line is not JSON: {why}"))?;
    let whole = |key: &str| value.get(key).and_then(Value::as_i64);
    let (Some(utt), Some(repeat), Some(t_end_us)) =
        (whole(KEY_UTT), whole(KEY_REPEAT), whole(KEY_T_END))
    else {
        return Err(format!("a sample lacks utt, repeat or t_end_us: {line}"));
    };
    Ok(Sample {
        utt,
        repeat,
        t_end_us,
        t_final_us: whole(KEY_T_FINAL),
        final_latency_ns: value
            .get(KEY_FINAL_LATENCY)
            .and_then(Value::as_f64)
            .map(ms_to_ns),
    })
}

fn parse_samples(text: &str) -> Result<Vec<Sample>, Failure> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(parse_sample)
        .collect()
}

fn is_first(sample: &Sample) -> bool {
    (sample.utt, sample.repeat) == FIRST
}

/// `d`: the gap between the end of the voice and the final, less the decode.
fn is_valid(sample: &Sample) -> bool {
    sample
        .t_final_us
        .zip(sample.final_latency_ns)
        .is_some_and(|(t_final, latency)| {
            (t_final - sample.t_end_us) * NS_PER_US - latency >= VALID_D_NS
        })
}

/// The final latencies the first utterance is compared with: the valid
/// samples of the run, the first utterance left out.
fn compared_with(samples: &[Sample]) -> Vec<i64> {
    samples
        .iter()
        .filter(|sample| !is_first(sample) && is_valid(sample))
        .filter_map(|sample| sample.final_latency_ns)
        .collect()
}

/// A time in nanoseconds as milliseconds, without trailing zeros.
fn show_ms(ns: i64) -> String {
    let (whole, part) = (ns / NS_PER_MS, ns % NS_PER_MS);
    if part == 0 {
        return whole.to_string();
    }
    format!("{whole}.{}", format!("{part:06}").trim_end_matches('0'))
}

fn first_utterance_check(run: &Run) -> Result<(), String> {
    let name = &run.name;
    let Some(first) = run.samples.iter().find(|sample| is_first(sample)) else {
        return Err(format!("{name}: no first utterance (utt 1, repeat 1)"));
    };
    if first.t_final_us.is_none() {
        return Err(format!("{name}: the first utterance has no final"));
    }
    let Some(took) = first.final_latency_ns else {
        return Err(format!(
            "{name}: the first utterance has no final_latency_ms"
        ));
    };
    let others = compared_with(&run.samples);
    if others.is_empty() {
        return Err(format!(
            "{name}: no valid sample to compare the first utterance with"
        ));
    }
    let p95 = nearest_rank(&others, 95)?;
    if took > p95 {
        return Err(format!(
            "{name}: the first utterance took {} ms, over the p95 {} ms of the {} others",
            show_ms(took),
            show_ms(p95),
            others.len()
        ));
    }
    Ok(())
}

/// What stdout holds of the three events RP-84 counts.
#[derive(Default)]
struct Counts {
    decoded: usize,
    failed: usize,
    decided: usize,
    decoded_at: Option<usize>,
    first_decided_at: Option<usize>,
}

fn count_events(stdout: &str) -> Result<Counts, Failure> {
    let mut counts = Counts::default();
    for (at, line) in stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .enumerate()
    {
        let event: Value = serde_json::from_str(line)
            .map_err(|why| format!("a stdout line is not JSON: {why}"))?;
        match event.get("event").and_then(Value::as_str) {
            Some(DECODED) => {
                counts.decoded += 1;
                counts.decoded_at = counts.decoded_at.or(Some(at));
            }
            Some(FAILED) => counts.failed += 1,
            Some(DECIDED) => {
                counts.decided += 1;
                counts.first_decided_at = counts.first_decided_at.or(Some(at));
            }
            _ => {}
        }
    }
    Ok(counts)
}

fn order_failure(name: &str, counts: &Counts) -> Option<String> {
    match (counts.decoded_at, counts.first_decided_at) {
        (Some(decoded), Some(decided)) if decoded > decided => Some(format!(
            "{name}: {DECODED} stands after the first {DECIDED}"
        )),
        _ => None,
    }
}

fn stdout_failures(name: &str, stdout: &str) -> Vec<String> {
    let counts = match count_events(stdout) {
        Ok(counts) => counts,
        Err(why) => return std::iter::once(format!("{name}: {why}")).collect(),
    };
    let mut failures = Vec::new();
    if counts.decided == 0 {
        failures.push(format!(
            "{name}: {DECIDED} is 0 lines, so {DECODED} has no first decision to stand before"
        ));
    }
    if counts.decoded != 1 {
        failures.push(format!(
            "{name}: {DECODED} is {} lines, not 1 ({DECIDED} is {} lines)",
            counts.decoded, counts.decided
        ));
    }
    failures.extend(order_failure(name, &counts));
    if counts.failed > 0 {
        failures.push(format!(
            "{name}: {FAILED} is {} lines, not 0",
            counts.failed
        ));
    }
    failures
}

fn run_failures(run: &Run) -> Vec<String> {
    let mut failures: Vec<String> = first_utterance_check(run).err().into_iter().collect();
    failures.extend(stdout_failures(&run.name, &run.stdout));
    failures
}

/// `e3p` and digits: a run of the window E3'. Runs of other names are not read.
fn is_x_run(name: &str) -> bool {
    name.strip_prefix("e3p")
        .is_some_and(|number| !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()))
}

/// Judges every X run in `runs`, and names each reason of each run that fails.
fn judge_first_utterance(runs: &[Run]) -> Result<(), Failure> {
    let x_runs: Vec<&Run> = runs.iter().filter(|run| is_x_run(&run.name)).collect();
    if x_runs.is_empty() {
        return Err(String::from("no X run (e3p<N>) to judge"));
    }
    let failures: Vec<String> = x_runs.into_iter().flat_map(run_failures).collect();
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n"))
    }
}

/// The reasons the judgement gives for `runs`; a judgement that passes them is an error.
fn rejection(runs: &[Run]) -> Result<String, Failure> {
    match judge_first_utterance(runs) {
        Err(said) => Ok(said),
        Ok(()) => Err(String::from(
            "the runs were passed; they should be rejected",
        )),
    }
}

/// The runs of the window E3' under `dir`: `e3p<N>.e2e1.jsonl` with the stdout beside it.
fn load_x_runs(dir: &Path) -> Result<Vec<Run>, Failure> {
    let entries = std::fs::read_dir(dir).map_err(|why| format!("{}: {why}", dir.display()))?;
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter_map(|file| file.strip_suffix(".e2e1.jsonl").map(str::to_owned))
        .filter(|name| is_x_run(name))
        .collect();
    names.sort();
    names
        .into_iter()
        .map(|name| {
            let samples = parse_samples(&read(&dir.join(format!("{name}.e2e1.jsonl")))?)?;
            let stdout = read(&dir.join(format!("{name}.e2e1.hayamimi.stdout.log")))?;
            Ok(Run {
                name,
                samples,
                stdout,
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Fixtures built by formula.
// ---------------------------------------------------------------------------

const UTTERANCES: i64 = 23;
const REPEATS: i64 = 5;

/// A sample whose decode took `latency_ns` and whose final came `d_ms` after
/// the decode ended.
fn sample(utt: i64, repeat: i64, latency_ns: i64, d_ms: i64) -> Sample {
    let t_end_us = 1_000_000 * (utt + 100 * repeat);
    Sample {
        utt,
        repeat,
        t_end_us,
        t_final_us: Some(t_end_us + (latency_ns + d_ms * NS_PER_MS) / NS_PER_US),
        final_latency_ns: Some(latency_ns),
    }
}

/// The first utterance, which took `latency` ms (a decimal) and has `d_ms` of `d`.
fn first(latency: &str, d_ms: i64) -> Result<Sample, Failure> {
    Ok(sample(FIRST.0, FIRST.1, micros(latency)?, d_ms))
}

/// The 114 keys of a window of 23 utterances in 5 repeats, the first one left out.
fn other_keys() -> Vec<(i64, i64)> {
    (1..=REPEATS)
        .flat_map(|repeat| (1..=UTTERANCES).map(move |utt| (utt, repeat)))
        .filter(|key| *key != FIRST)
        .collect()
}

/// F84: 115 samples, all valid (`d` 500 ms). The 114 samples other than
/// `first` took 101, 102, ... 214 ms: the p50 is 157 ms and the p95 209 ms.
fn f84(first: Sample) -> Vec<Sample> {
    let mut samples = vec![first];
    samples.extend(
        other_keys()
            .into_iter()
            .zip(101..)
            .map(|((utt, repeat), ms)| sample(utt, repeat, ms * NS_PER_MS, 500)),
    );
    samples
}

/// F84b: six of the 114 other samples have `d` 100 ms and took 5000 ms; the
/// valid 108 took 101 ... 208 ms, so their p95 is 203 ms.
fn f84b(first: Sample) -> Vec<Sample> {
    let others = other_keys();
    let mut samples = vec![first];
    samples.extend(
        others
            .iter()
            .take(6)
            .map(|(utt, repeat)| sample(*utt, *repeat, 5000 * NS_PER_MS, 100)),
    );
    samples.extend(
        others
            .iter()
            .skip(6)
            .zip(101..)
            .map(|((utt, repeat), ms)| sample(*utt, *repeat, ms * NS_PER_MS, 500)),
    );
    samples
}

fn loaded() -> Value {
    json!({"event": LOADED, "model": "rz", "load_ms": 1000.0})
}

fn decoded() -> Value {
    json!({"event": DECODED, "model": "rz", "decode_ms": 950.5, "audio_s": 10})
}

fn failed() -> Value {
    json!({"event": FAILED, "error_code": "HAYAMIMI_ASR_WARMUP_FAILED"})
}

fn decided(id: u32) -> Value {
    json!({"event": DECIDED, "id": id, "rule": "early", "silence_ms": 120})
}

fn waited() -> Value {
    json!({"event": "asr.feed.waited", "wait_ms": 1.0})
}

/// L84: the model, the start-up decode, then five decisions.
fn l84() -> Vec<Value> {
    let mut lines = vec![loaded(), decoded()];
    lines.extend((1..=5).map(decided));
    lines
}

fn stdout_of(events: &[Value]) -> String {
    events
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

fn run_of(name: &str, samples: Vec<Sample>, events: &[Value]) -> Run {
    Run {
        name: name.to_owned(),
        samples,
        stdout: stdout_of(events),
    }
}

/// A run that passes: the first utterance at the p50 of the rest, L84 on stdout.
fn good_run(name: &str) -> Result<Run, Failure> {
    Ok(run_of(name, f84(first("157.0", 500)?), &l84()))
}

fn first_mut(samples: &mut [Sample]) -> Result<&mut Sample, Failure> {
    samples
        .iter_mut()
        .find(|sample| is_first(sample))
        .ok_or_else(|| String::from("the fixture has no first utterance"))
}

// ---------------------------------------------------------------------------
// The first utterance against the rest of its run.
// ---------------------------------------------------------------------------

#[test]
fn a_first_utterance_at_the_p50_of_the_rest_passes() -> TestResult {
    judge_first_utterance(&[good_run("e3p1")?])?;
    Ok(())
}

#[test]
fn a_first_utterance_exactly_at_the_p95_passes() -> TestResult {
    let run = run_of("e3p1", f84(first("209.0", 500)?), &l84());
    judge_first_utterance(&[run])?;
    Ok(())
}

#[test]
fn a_first_utterance_one_step_over_the_p95_is_rejected() -> TestResult {
    let run = run_of("e3p1", f84(first("209.000001", 500)?), &l84());
    let said = rejection(&[run])?;
    assert!(
        said.contains("e3p1: the first utterance took 209.000001 ms"),
        "{said}"
    );
    assert!(said.contains("over the p95 209 ms"), "{said}");
    Ok(())
}

#[test]
fn the_first_utterance_is_not_part_of_the_set_it_is_compared_with() -> TestResult {
    let run = run_of("e3p1", f84(first("209.5", 500)?), &l84());
    let said = rejection(&[run])?;
    assert!(said.contains("took 209.5 ms"), "{said}");
    assert!(
        said.contains("over the p95 209 ms of the 114 others"),
        "{said}"
    );
    Ok(())
}

#[test]
fn invalid_samples_are_left_out_of_the_set_a_first_utterance_is_compared_with() -> TestResult {
    let samples = f84b(first("203.0", 500)?);
    assert_eq!(compared_with(&samples).len(), 108);
    judge_first_utterance(&[run_of("e3p1", samples, &l84())])?;
    Ok(())
}

#[test]
fn invalid_samples_do_not_raise_the_limit_a_first_utterance_is_compared_with() -> TestResult {
    let run = run_of("e3p1", f84b(first("210.0", 500)?), &l84());
    let said = rejection(&[run])?;
    assert!(said.contains("took 210 ms"), "{said}");
    assert!(
        said.contains("over the p95 203 ms of the 108 others"),
        "{said}"
    );
    Ok(())
}

#[test]
fn a_first_utterance_without_a_final_is_rejected() -> TestResult {
    let mut samples = f84(first("157.0", 500)?);
    first_mut(&mut samples)?.t_final_us = None;
    let said = rejection(&[run_of("e3p1", samples, &l84())])?;
    assert!(
        said.contains("e3p1: the first utterance has no final"),
        "{said}"
    );
    Ok(())
}

#[test]
fn a_run_with_nothing_to_compare_with_is_rejected() -> TestResult {
    let mut samples = f84(first("157.0", 500)?);
    for other in samples.iter_mut().filter(|sample| !is_first(sample)) {
        other.t_final_us = None;
    }
    let said = rejection(&[run_of("e3p1", samples, &l84())])?;
    assert!(said.contains("e3p1: no valid sample to compare"), "{said}");
    Ok(())
}

#[test]
fn a_first_utterance_below_the_validity_line_is_still_compared() -> TestResult {
    let kept = run_of("e3p1", f84(first("157.0", 100)?), &l84());
    assert!(
        !kept
            .samples
            .iter()
            .any(|sample| is_first(sample) && is_valid(sample))
    );
    judge_first_utterance(&[kept])?;
    let late = run_of("e3p1", f84(first("210.0", 100)?), &l84());
    let said = rejection(&[late])?;
    assert!(
        said.contains("took 210 ms, over the p95 209 ms of the 114 others"),
        "{said}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// The records of a run before the start-up decode existed.
// ---------------------------------------------------------------------------

#[test]
fn the_recorded_rs1_first_utterance_is_over_the_p95() -> TestResult {
    let samples = parse_samples(RS1)?;
    assert_eq!(compared_with(&samples).len(), 97);
    let said = rejection(&[run_of("e3p1", samples, &l84())])?;
    assert!(
        said.contains("e3p1: the first utterance took 988.338217 ms"),
        "{said}"
    );
    assert!(
        said.contains("over the p95 213.100368 ms of the 97 others"),
        "{said}"
    );
    assert!(!said.contains("asr.warmup"), "{said}");
    Ok(())
}

#[test]
fn the_recorded_rs2_and_rs3_first_utterances_are_over_the_p95() -> TestResult {
    let runs = [
        run_of("e3p2", parse_samples(RS2)?, &l84()),
        run_of("e3p3", parse_samples(RS3)?, &l84()),
    ];
    assert!(
        runs.iter()
            .all(|run| compared_with(&run.samples).len() == 97)
    );
    let said = rejection(&runs)?;
    assert!(
        said.contains("e3p2: the first utterance took 682.495191 ms"),
        "{said}"
    );
    assert!(said.contains("over the p95 220.126125 ms"), "{said}");
    assert!(
        said.contains("e3p3: the first utterance took 807.670646 ms"),
        "{said}"
    );
    assert!(said.contains("over the p95 204.686185 ms"), "{said}");
    Ok(())
}

#[test]
fn the_recorded_runs_pass_when_the_first_utterance_is_at_their_p50() -> TestResult {
    let cases = [
        ("e3p1", RS1, "117.679425"),
        ("e3p2", RS2, "124.455251"),
        ("e3p3", RS3, "126.553233"),
    ];
    let mut runs = Vec::new();
    for (name, text, p50) in cases {
        let mut samples = parse_samples(text)?;
        let p50 = micros(p50)?;
        assert_eq!(nearest_rank(&compared_with(&samples), 50)?, p50, "{name}");
        first_mut(&mut samples)?.final_latency_ns = Some(p50);
        runs.push(run_of(name, samples, &l84()));
    }
    judge_first_utterance(&runs)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// What stdout says about the start-up decode.
// ---------------------------------------------------------------------------

/// A run with the samples that pass and `events` on stdout.
fn stdout_run(events: &[Value]) -> Result<Run, Failure> {
    Ok(run_of("e3p1", f84(first("157.0", 500)?), events))
}

#[test]
fn a_warmup_decoded_before_the_first_endpoint_decision_passes() -> TestResult {
    judge_first_utterance(&[stdout_run(&l84())?])?;
    let mut apart = vec![loaded(), decoded(), waited(), waited(), waited()];
    apart.extend((1..=5).map(decided));
    judge_first_utterance(&[stdout_run(&apart)?])?;
    Ok(())
}

#[test]
fn a_warmup_decoded_after_the_first_endpoint_decision_is_rejected() -> TestResult {
    let mut swapped = vec![loaded(), decided(1), decoded()];
    swapped.extend((2..=5).map(decided));
    let said = rejection(&[stdout_run(&swapped)?])?;
    assert!(
        said.contains("e3p1: asr.warmup.decoded stands after the first asr.endpoint.decided"),
        "{said}"
    );
    Ok(())
}

#[test]
fn a_run_without_a_warmup_decoded_line_is_rejected() -> TestResult {
    let mut events = vec![loaded()];
    events.extend((1..=5).map(decided));
    let said = rejection(&[stdout_run(&events)?])?;
    assert!(
        said.contains("e3p1: asr.warmup.decoded is 0 lines, not 1"),
        "{said}"
    );
    Ok(())
}

#[test]
fn two_warmup_decoded_lines_are_rejected() -> TestResult {
    let mut events = vec![loaded(), decoded(), decoded()];
    events.extend((1..=5).map(decided));
    let said = rejection(&[stdout_run(&events)?])?;
    assert!(
        said.contains("e3p1: asr.warmup.decoded is 2 lines, not 1"),
        "{said}"
    );
    Ok(())
}

#[test]
fn a_warmup_failed_line_is_rejected_on_its_own() -> TestResult {
    let mut events = vec![loaded(), decoded(), failed()];
    events.extend((1..=5).map(decided));
    let said = rejection(&[stdout_run(&events)?])?;
    assert!(
        said.contains("e3p1: asr.warmup.failed is 1 lines, not 0"),
        "{said}"
    );
    assert!(!said.contains(DECODED), "{said}");
    Ok(())
}

#[test]
fn the_recorded_stdout_has_decisions_and_no_warmup_line() -> TestResult {
    let count = |word: &str| {
        RS1_STDOUT
            .lines()
            .filter(|line| line.contains(word))
            .count()
    };
    assert_eq!(count(DECIDED), 141);
    assert_eq!(count("asr.warmup"), 0);
    let run = Run {
        stdout: RS1_STDOUT.to_owned(),
        ..good_run("e3p1")?
    };
    let said = rejection(&[run])?;
    assert!(
        said.contains(
            "e3p1: asr.warmup.decoded is 0 lines, not 1 (asr.endpoint.decided is 141 lines)"
        ),
        "{said}"
    );
    Ok(())
}

#[test]
fn a_stdout_without_any_endpoint_decision_is_rejected() -> TestResult {
    let said = rejection(&[stdout_run(&[loaded(), decoded()])?])?;
    assert!(
        said.contains("e3p1: asr.endpoint.decided is 0 lines"),
        "{said}"
    );
    assert!(!said.contains("not 1"), "{said}");
    Ok(())
}

#[test]
fn warmup_fields_other_than_the_event_name_are_not_judged() -> TestResult {
    let bare = json!({"event": DECODED, "model": "rz", "audio_s": 1});
    let mut events = vec![loaded(), bare];
    events.extend((1..=5).map(decided));
    judge_first_utterance(&[stdout_run(&events)?])?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Which runs are read, and how a failing run is named.
// ---------------------------------------------------------------------------

#[test]
fn only_the_failing_run_is_named_for_the_first_utterance() -> TestResult {
    let late = run_of("e3p2", f84b(first("210.0", 500)?), &l84());
    let said = rejection(&[good_run("e3p1")?, late])?;
    assert!(
        said.contains("e3p2: the first utterance took 210 ms"),
        "{said}"
    );
    assert!(!said.contains("e3p1"), "{said}");
    Ok(())
}

#[test]
fn each_failing_run_is_named_with_its_own_reason() -> TestResult {
    let late = run_of("e3p1", f84(first("210.0", 500)?), &l84());
    let mut swapped = vec![loaded(), decided(1), decoded()];
    swapped.extend((2..=5).map(decided));
    let misplaced = stdout_run(&swapped).map(|run| Run {
        name: String::from("e3p2"),
        ..run
    })?;
    let said = rejection(&[late, misplaced])?;
    assert!(
        said.contains("e3p1: the first utterance took 210 ms"),
        "{said}"
    );
    assert!(
        said.contains("e3p2: asr.warmup.decoded stands after the first asr.endpoint.decided"),
        "{said}"
    );
    Ok(())
}

#[test]
fn a_set_with_no_x_run_is_rejected() -> TestResult {
    let said = rejection(&[good_run("w1")?, good_run("w2")?])?;
    assert!(said.contains("no X run"), "{said}");
    Ok(())
}

#[test]
fn runs_not_named_e3p_are_not_read_for_the_first_utterance() -> TestResult {
    let slow = run_of("w1", f84(first("5000.0", 500)?), &l84());
    judge_first_utterance(&[slow, good_run("e3p1")?])?;
    Ok(())
}

// ---------------------------------------------------------------------------
// The records of the window E3.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "reads the records of the window E3 under docs/results/r8x/"]
fn rp84_the_first_utterance_decodes_as_fast_as_the_rest() -> TestResult {
    let runs = load_x_runs(&Path::new(RESULTS).join("r8x"))?;
    judge_first_utterance(&runs)?;
    Ok(())
}

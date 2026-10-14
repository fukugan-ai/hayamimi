//! RP-4: the delay from the end of the speech to the closing `final`
//! (`L_asr`) of the Rust server is no greater than the Python server's.
//!
//! The comparison rules are `hayamimi_eval::compare`, which RP-3 uses for
//! `L` too. The real run (T-RP-4-008) reads the samples file a Rust server
//! left beside the Python baseline and is run with `--ignored`.

mod support;

use std::collections::BTreeSet;
use std::path::PathBuf;

use hayamimi_eval::compare::{
    LAsrRecord, capture_final_latencies, check_final_latency_agreement, check_l_asr, check_paired,
    check_percentiles, l_asr_comparison_set, l_asr_record, log_final_latencies, read_samples,
    write_l_asr_record,
};
use hayamimi_eval::error::EvalError;
use hayamimi_eval::latency::{Sample, Stat};
use support::{TestResult, base, read, run_files};

/// The `--min-silence` of the Python runs and of the Rust run, in milliseconds.
const MIN_SILENCE_MS: i64 = 350;
/// The suffix of the Python server's WebSocket capture beside its samples file.
const EVENTS: &str = "events.jsonl";

fn stat(p50_ms: i64, p95_ms: i64) -> Stat {
    Stat {
        n: 20,
        p50_ms,
        p95_ms,
    }
}

/// A sample whose `final` closed `l_asr_ms` after the speech ended, `l_dec_ms` of it decoding.
fn sample(utt: u32, l_asr_ms: i64, l_dec_ms: f64) -> Sample {
    Sample {
        repeat: 1,
        utt,
        t_start_us: 0,
        t_end_us: 1_000_000,
        t_partial_us: None,
        t_partial_tr_us: None,
        t_final_us: Some(1_000_000 + l_asr_ms * 1000),
        t_tr_us: None,
        finals: 1,
        text: String::new(),
        translation: None,
        final_latency_ms: Some(l_dec_ms),
        cer_dist: 0,
        cer_denom: 0,
    }
}

/// 20 utterances, five repeats each. Python takes 800 ms except utterances 19 and 20, which take
/// 5000 ms and put Python's p95 far above the few slower utterances of Rust. The first `slower`
/// utterances take 801 ms in Rust, the others 800 ms.
fn five_repeats(slower: u32) -> (Vec<Sample>, Vec<Sample>) {
    let make = |at: &dyn Fn(u32) -> i64| -> Vec<Sample> {
        (1..=20)
            .flat_map(|utt| std::iter::repeat_n(utt, 5))
            .map(|utt| sample(utt, at(utt), 300.0))
            .collect()
    };
    let python = make(&|utt| if utt >= 19 { 5000 } else { 800 });
    let rust = make(&|utt| if utt <= slower { 801 } else { 800 });
    (python, rust)
}

/// The two Python runs of `--min-silence 0.35`, as samples.
fn python_runs() -> Result<Vec<Vec<Sample>>, Box<dyn std::error::Error>> {
    let mut runs = Vec::new();
    for (_, path) in run_files("0.35")? {
        runs.push(read_samples(&path)?);
    }
    Ok(runs)
}

fn as_slices(runs: &[Vec<Sample>]) -> Vec<&[Sample]> {
    runs.iter().map(Vec::as_slice).collect()
}

/// T-RP-4-001
#[test]
fn equal_l_asr_percentiles_pass() -> TestResult {
    check_percentiles("L_asr", &stat(800, 1500), &stat(800, 1500))?;
    Ok(())
}

/// T-RP-4-002
#[test]
fn l_asr_p50_one_millisecond_slower_is_rejected() {
    let said = check_percentiles("L_asr", &stat(800, 1500), &stat(801, 1500));
    assert!(
        matches!(&said, Err(EvalError::Invalid(m)) if m.contains("p50") && !m.contains("p95")),
        "{said:?}"
    );
}

/// T-RP-4-003
#[test]
fn l_asr_p95_one_millisecond_slower_is_rejected() {
    let said = check_percentiles("L_asr", &stat(800, 1500), &stat(800, 1501));
    assert!(
        matches!(&said, Err(EvalError::Invalid(m)) if m.contains("p95") && !m.contains("p50")),
        "{said:?}"
    );
}

/// T-RP-4-004
#[test]
fn eighteen_of_20_comparison_utterances_passes_and_seventeen_is_rejected() -> TestResult {
    // 18 utterances equal Python, two are 1 ms slower: the percentiles hold, the pairing is at its edge.
    let (python, rust) = five_repeats(2);
    let report = check_l_asr(&[&python], &rust, MIN_SILENCE_MS)?;
    assert_eq!(
        (report.set_size, report.not_slower, report.threshold),
        (20, 18, 18)
    );
    check_paired(20, 18)?;
    // Three slower utterances leave 17: still under the percentile limits, short of the pairing.
    let (python, rust) = five_repeats(3);
    let said = check_l_asr(&[&python], &rust, MIN_SILENCE_MS);
    assert!(
        matches!(&said, Err(EvalError::Invalid(m)) if m.contains("17 of 20")),
        "{said:?}"
    );
    assert!(check_paired(20, 17).is_err());
    Ok(())
}

/// T-RP-4-005
#[test]
fn l_dec_is_recorded_but_does_not_decide() -> TestResult {
    let python: Vec<Sample> = (1..=20).map(|utt| sample(utt, 800, 300.0)).collect();
    // The Rust server decodes in twice the time and waited less silence: L_asr is still lower.
    let rust: Vec<Sample> = (1..=20)
        .map(|utt| sample(utt, 790, 600.0 - 160.0))
        .collect();
    let report = check_l_asr(&[&python], &rust, MIN_SILENCE_MS)?;
    assert_eq!(report.python_l_dec_p50_ms, Some(300));
    assert_eq!(report.rust_l_dec_p50_ms, Some(440));
    assert!(report.rust_l_dec_p50_ms > report.python_l_dec_p50_ms);
    Ok(())
}

/// T-RP-4-006
#[test]
fn python_final_latency_from_ws_and_log_agree_in_count_and_within_0_5_ms() -> TestResult {
    let files = run_files("0.35")?;
    assert!(!files.is_empty(), "no Python run of --min-silence 0.35");
    let directory = base()?;
    for (n, _) in files {
        let capture = read(&directory.join(format!("lat_ja_0.35_{n}.{EVENTS}")))?;
        let log = read(&directory.join(format!("server_0.35_{n}.out.log")))?;
        check_final_latency_agreement(
            &capture_final_latencies(&capture)?,
            &log_final_latencies(&log)?,
        )?;
    }
    Ok(())
}

/// T-RP-4-007
#[test]
fn cpu_provider_run_is_recorded_not_judged() -> TestResult {
    // Far slower than Python: a judged run would fail; a record is made all the same.
    let slow: Vec<Sample> = (1..=20).map(|utt| sample(utt, 5000, 300.0)).collect();
    let python: Vec<Sample> = (1..=20).map(|utt| sample(utt, 800, 300.0)).collect();
    assert!(check_l_asr(&[&python], &slow, MIN_SILENCE_MS).is_err());
    let record = l_asr_record("cpu", &slow, MIN_SILENCE_MS)?;
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("rp04_cpu_record.json");
    write_l_asr_record(&path, &record)?;
    let written: LAsrRecord = serde_json::from_str(&read(&path)?)?;
    assert_eq!(written.provider, "cpu");
    assert_eq!((written.p50_ms, written.p95_ms), (5000, 5000));
    Ok(())
}

/// T-RP-4-008: the Rust server's samples file, `rust_lat_ja_0.35_1.jsonl` beside the Python
/// baseline, written by `hayamimi-eval stream --repeat 5` against `hayamimi --min-silence 0.35`.
#[test]
#[ignore = "needs the samples file of a real Rust run"]
fn rust_l_asr_beats_python_on_lat_ja() -> TestResult {
    let python = python_runs()?;
    let rust = read_samples(&base()?.join("rust_lat_ja_0.35_1.jsonl"))?;
    let report = check_l_asr(&as_slices(&python), &rust, MIN_SILENCE_MS)?;
    eprintln!("{report:?}");
    Ok(())
}

/// Utterances with a closing `final` in every run, whether or not the silence fits in `L_asr`.
/// `None` for no runs at all.
fn utterances_with_a_final_in_every_run(runs: &[Vec<Sample>]) -> Option<BTreeSet<u32>> {
    runs.iter()
        .map(|run| {
            run.iter()
                .filter(|s| s.t_final_us.is_some())
                .map(|s| s.utt)
                .collect::<BTreeSet<u32>>()
        })
        .reduce(|both, next| both.intersection(&next).copied().collect())
}

/// T-RP-4-009
#[test]
fn l_asr_comparison_set_of_the_python_runs_has_20_utterances_with_5_and_without_10_18_19()
-> TestResult {
    let runs = python_runs()?;
    assert_eq!(runs.len(), 2, "the baseline holds two runs of 0.35");
    let set = l_asr_comparison_set(&as_slices(&runs), MIN_SILENCE_MS);
    let expected: Vec<u32> = (1..=9).chain(11..=17).chain(20..=23).collect();
    assert_eq!(set, expected);
    assert_eq!(set.len(), 20);
    assert!(set.contains(&5));
    // Without the validity test the count is 21: utterance 18 has a `final` that is not the recognizer's.
    let Some(counted) = utterances_with_a_final_in_every_run(&runs) else {
        return Err("the baseline holds no run".into());
    };
    assert_eq!(counted.len(), 21);
    assert!(counted.contains(&18) && !set.contains(&18));
    Ok(())
}

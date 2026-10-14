//! RP-5: the text in progress and its translation reach the reader early. The
//! first `partial` of the Rust run is no later than Python's at the 95th
//! percentile, and a translation of the text in progress comes strictly before
//! the translation of the `final` of the same utterance.

mod support;

use hayamimi_eval::latency::Sample;
use hayamimi_eval::partial::{
    Fault, check_first_partial, check_partial_translation_first, count_partial_translations,
    first_partial_delays_us,
};
use support::{TestResult, read, run_files};

fn with_first_partial(delay_us: i64) -> Sample {
    Sample {
        repeat: 1,
        utt: 1,
        t_start_us: 1_000,
        t_end_us: 2_000_000,
        t_partial_us: Some(1_000 + delay_us),
        t_partial_tr_us: None,
        t_final_us: Some(2_500_000),
        t_tr_us: None,
        finals: 1,
        text: String::from("x"),
        translation: None,
        final_latency_ms: None,
        cer_dist: 0,
        cer_denom: 1,
    }
}

fn with_translations(partial_tr_us: Option<i64>, t_tr_us: Option<i64>) -> Sample {
    Sample {
        t_partial_tr_us: partial_tr_us,
        t_tr_us,
        ..with_first_partial(300_000)
    }
}

#[test]
fn equal_first_partial_p95_passes() {
    let python = [with_first_partial(450_000)];
    let rust = [with_first_partial(450_000)];
    assert_eq!(check_first_partial(&python, &rust), Ok(()));
}

#[test]
fn first_partial_p95_one_millisecond_slower_is_rejected() {
    let python = [with_first_partial(450_000)];
    let rust = [with_first_partial(451_000)];
    assert_eq!(
        check_first_partial(&python, &rust),
        Err(Fault::FirstPartialLater {
            rust_ms: 451,
            python_ms: 450
        })
    );
}

#[test]
fn partial_translation_before_t_tr_passes() {
    let run = [with_translations(Some(400), Some(500))];
    assert_eq!(check_partial_translation_first(&run), Ok(()));
}

#[test]
fn partial_translation_at_the_same_time_as_t_tr_is_rejected() {
    let run = [with_translations(Some(500), Some(500))];
    assert_eq!(
        check_partial_translation_first(&run),
        Err(Fault::NotBeforeTranslation { repeat: 1, utt: 1 })
    );
}

#[test]
fn partial_translation_after_t_tr_is_rejected() {
    let run = [with_translations(Some(501), Some(500))];
    assert_eq!(
        check_partial_translation_first(&run),
        Err(Fault::NotBeforeTranslation { repeat: 1, utt: 1 })
    );
}

#[test]
fn utterance_without_partial_translation_is_not_judged() {
    let run = [with_translations(None, Some(500))];
    assert_eq!(check_partial_translation_first(&run), Ok(()));
}

/// The samples of the recorded Python run (`--min-silence 0.35`, first file).
fn recorded_python_run() -> Result<Vec<Sample>, Box<dyn std::error::Error>> {
    let files = run_files("0.35")?;
    let Some((_, path)) = files.first() else {
        return Err("no recorded run for --min-silence 0.35".into());
    };
    let mut samples = Vec::new();
    for line in read(path)?.lines().filter(|line| !line.trim().is_empty()) {
        samples.push(serde_json::from_str::<Sample>(line)?);
    }
    Ok(samples)
}

/// Every utterance of the recorded Python run that has a `final` has a first
/// `partial`, and the delays it gives are positive. An utterance with neither
/// (the recognizer heard nothing in it) has no first `partial` to read.
#[test]
fn python_run_yields_first_partial_times() -> TestResult {
    let samples = recorded_python_run()?;
    let answered = samples.iter().filter(|s| s.t_final_us.is_some()).count();
    let without = samples
        .iter()
        .filter(|s| s.t_final_us.is_some() && s.t_partial_us.is_none())
        .count();
    // The control: the run does have utterances that answered, and some that
    // did not, so the count of those without a first partial is not vacuous.
    assert!(answered > 0);
    assert!(answered < samples.len());
    assert_eq!(
        without, 0,
        "{without} of {answered} answered have no partial"
    );
    let delays = first_partial_delays_us(&samples);
    assert!(delays.len() >= answered);
    assert!(delays.iter().all(|delay| *delay > 0));
    Ok(())
}

#[test]
fn partial_translation_counter_reads_zero_when_disabled() {
    let disabled = [with_translations(None, Some(500)), with_first_partial(1)];
    let enabled = [
        with_translations(Some(400), Some(500)),
        with_translations(None, Some(500)),
    ];
    assert_eq!(count_partial_translations(&disabled), 0);
    assert!(count_partial_translations(&enabled) >= 1);
}

/// The run of `<ev> stream` against the Rust relay is made by the final slice
/// that wires the real models (R8e); until then no recorded run exists.
#[test]
#[ignore = "needs the RP-3 run of the Rust relay (R8e)"]
fn rust_partials_arrive_no_later_than_python() -> TestResult {
    Err("the run of the Rust relay is not recorded yet".into())
}

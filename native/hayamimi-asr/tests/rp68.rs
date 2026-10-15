//! RP-68: a `flush` inside a stream aborts the detector, and neither the early
//! end of speech nor `Engine::finish` may leave the detector in that state.
//!
//! The detector's buffer aborts the whole process, so each case runs in a child
//! process: the parent starts this test binary again (`std::env::current_exe`)
//! with `--ignored --exact <child>` and reads only how the child ended, its
//! exit status and its stderr. A child is named after its parent with `_child`
//! added. Ended abnormally means the status is not 0 (a signal has no status
//! and counts) or stderr carries `Invalid n`.
//!
//! Audio: the first utterance of `testdata/lat-ja.wav` (V68), found the way
//! `vad_threshold_probe.rs` finds it, then silence of whole 512-sample windows:
//! S4 (4 windows), S6 (6 windows) and L12 (12 windows, longer than the
//! detector's 0.30 s of silence).
//!
//!     cargo test --release -p hayamimi-asr --test rp68

mod support;

use std::path::Path;
use std::process::Command;

use hayamimi_asr::audio::{WINDOW, read_wav};
use hayamimi_asr::endpoint::{Mode, Rule};
use hayamimi_asr::vad::{DEFAULT_THRESHOLD, VadSettings, silero_config_with, take_segments};
use hayamimi_asr::{Engine, Final, Settings};
use hayamimi_gpu::session_lock::lock_for_session;
use sherpa_onnx::VoiceActivityDetector;
use support::{Failure, TestResult, models_dir, read_json, testdata_dir};

const SHORT_GAP: usize = 4;
const LONG_GAP: usize = 6;
const LONG_SILENCE: usize = 12;
/// 0.5 s at 16 kHz: a point inside the voice of the first utterance.
const HALF_SECOND: usize = 8000;
const BUFFER_SECONDS: f32 = 30.0;

/// How a child ended.
#[derive(Debug)]
struct Outcome {
    code: Option<i32>,
    stderr: String,
}

impl Outcome {
    fn is_abnormal(&self) -> bool {
        self.code != Some(0) || self.stderr.contains("Invalid n")
    }
}

fn child_of(parent: &str) -> String {
    format!("{parent}_child")
}

/// Starts this binary again for one ignored test and reads how it ended. An
/// `--exact` name that matches no test ends with status 0, so a run that did
/// not run exactly one test is an error.
fn run_child(name: &str) -> Result<Outcome, Failure> {
    let exe = std::env::current_exe().map_err(|why| format!("current_exe: {why}"))?;
    let output = Command::new(exe)
        .args([
            "--ignored",
            "--exact",
            name,
            "--nocapture",
            "--test-threads=1",
        ])
        .output()
        .map_err(|why| format!("cannot start the child {name}: {why}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.contains("running 1 test\n") {
        return Err(format!(
            "the child {name} did not run exactly one test:\n{stdout}"
        ));
    }
    Ok(Outcome {
        code: output.status.code(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// Runs the child that reproduces the defect; it must end abnormally.
fn reproduce(name: &str) -> Result<Outcome, Failure> {
    let outcome = run_child(name)?;
    if !outcome.is_abnormal() {
        return Err(format!(
            "the reproduction child {name} ends cleanly: the premise of the reproduction changed"
        ));
    }
    Ok(outcome)
}

fn expect_abnormal(name: &str) -> TestResult {
    let outcome = run_child(name)?;
    assert!(outcome.is_abnormal(), "{name} ended normally: {outcome:?}");
    Ok(())
}

fn expect_normal(name: &str) -> TestResult {
    let outcome = run_child(name)?;
    assert!(
        !outcome.is_abnormal(),
        "{name} ended abnormally: {outcome:?}"
    );
    Ok(())
}

fn silence(windows: usize) -> Vec<f32> {
    vec![0.0; windows * WINDOW]
}

/// V68: the voice of the first utterance of `lat-ja.wav`.
fn voice() -> Result<Vec<f32>, Failure> {
    let testdata = testdata_dir()?;
    let audio = read_wav(&testdata.join("lat-ja.wav")).map_err(|why| why.to_string())?;
    let manifest = read_json(&testdata.join("manifests").join("lat-ja.json"))?;
    let first = manifest
        .get(0)
        .ok_or_else(|| String::from("the manifest of lat-ja has no utterance"))?;
    let sample = |key: &str| {
        let number = first
            .get(key)
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| format!("the first utterance has no integer `{key}`"))?;
        usize::try_from(number).map_err(|why| format!("{key}: {why}"))
    };
    let (start, end) = (sample("start_sample")?, sample("end_sample")?);
    audio
        .get(start..end)
        .map(<[f32]>::to_vec)
        .ok_or_else(|| format!("the utterance {start}..{end} is outside the wav"))
}

fn open_detector(models: &Path) -> Result<VoiceActivityDetector, Failure> {
    let config = silero_config_with(models, VadSettings::default(), DEFAULT_THRESHOLD)
        .map_err(|why| why.to_string())?;
    let _building = lock_for_session();
    VoiceActivityDetector::create(&config, BUFFER_SECONDS)
        .ok_or_else(|| String::from("the VAD could not be created"))
}

fn feed_detector(detector: &VoiceActivityDetector, samples: &[f32]) -> Result<(), Failure> {
    let (windows, _tail) = samples.as_chunks::<WINDOW>();
    for window in windows {
        detector.accept_waveform(window.as_slice());
        take_segments(detector).map_err(|why| why.to_string())?;
    }
    Ok(())
}

/// V68, a gap, then optionally `flush`, then the long silence, straight on the detector.
fn detector_run(gap: usize, flush: bool) -> TestResult {
    let detector = open_detector(&models_dir()?)?;
    feed_detector(&detector, &voice()?)?;
    feed_detector(&detector, &silence(gap))?;
    if flush {
        detector.flush();
    }
    feed_detector(&detector, &silence(LONG_SILENCE))?;
    take_segments(&detector).map_err(|why| why.to_string())?;
    Ok(())
}

fn open_engine() -> Result<Engine, Failure> {
    let mut settings = Settings::new(models_dir()?);
    settings.stages.speakers = false;
    Engine::open(settings).map_err(|why| why.to_string())
}

fn feed_engine(engine: &mut Engine, samples: &[f32]) -> Result<(), Failure> {
    let (windows, _tail) = samples.as_chunks::<WINDOW>();
    for window in windows {
        engine
            .feed(window.as_slice())
            .map_err(|why| why.to_string())?;
    }
    Ok(())
}

/// `voice`, a gap, `Engine::finish`, then the long silence; optionally the voice once more.
fn engine_run(voice: &[f32], gap: usize, again: bool) -> TestResult {
    let mut engine = open_engine()?;
    feed_engine(&mut engine, voice)?;
    feed_engine(&mut engine, &silence(gap))?;
    engine.finish().map_err(|why| why.to_string())?;
    feed_engine(&mut engine, &silence(LONG_SILENCE))?;
    if again {
        feed_engine(&mut engine, voice)?;
    }
    Ok(())
}

// T-RP-68-001, 002, 003: a `flush` inside the stream, and the control without it.

#[test]
fn a_flush_inside_speech_then_a_long_silence_is_abnormal_after_a_short_gap() -> TestResult {
    expect_abnormal(&child_of(
        "a_flush_inside_speech_then_a_long_silence_is_abnormal_after_a_short_gap",
    ))
}

#[test]
#[ignore = "child process of the RP-68 test of the same name"]
fn a_flush_inside_speech_then_a_long_silence_is_abnormal_after_a_short_gap_child() -> TestResult {
    detector_run(SHORT_GAP, true)
}

#[test]
fn a_flush_inside_speech_then_a_long_silence_is_abnormal_after_a_long_gap() -> TestResult {
    expect_abnormal(&child_of(
        "a_flush_inside_speech_then_a_long_silence_is_abnormal_after_a_long_gap",
    ))
}

#[test]
#[ignore = "child process of the RP-68 test of the same name"]
fn a_flush_inside_speech_then_a_long_silence_is_abnormal_after_a_long_gap_child() -> TestResult {
    detector_run(LONG_GAP, true)
}

#[test]
fn without_the_flush_the_same_audio_ends_cleanly() -> TestResult {
    expect_normal(&child_of("without_the_flush_the_same_audio_ends_cleanly"))
}

#[test]
#[ignore = "child process of the RP-68 test of the same name"]
fn without_the_flush_the_same_audio_ends_cleanly_child() -> TestResult {
    detector_run(LONG_GAP, false)
}

// T-RP-68-006 to 009: `Engine::finish` after the voice.

#[test]
fn finish_after_a_short_silence_then_a_long_silence_ends_cleanly() -> TestResult {
    expect_normal(&child_of(
        "finish_after_a_short_silence_then_a_long_silence_ends_cleanly",
    ))
}

#[test]
#[ignore = "child process of the RP-68 test of the same name"]
fn finish_after_a_short_silence_then_a_long_silence_ends_cleanly_child() -> TestResult {
    engine_run(&voice()?, SHORT_GAP, false)
}

#[test]
fn finish_after_a_long_gap_then_a_long_silence_ends_cleanly() -> TestResult {
    expect_normal(&child_of(
        "finish_after_a_long_gap_then_a_long_silence_ends_cleanly",
    ))
}

#[test]
#[ignore = "child process of the RP-68 test of the same name"]
fn finish_after_a_long_gap_then_a_long_silence_ends_cleanly_child() -> TestResult {
    engine_run(&voice()?, LONG_GAP, false)
}

#[test]
fn finish_inside_speech_does_not_expose_the_defect() -> TestResult {
    expect_normal(&child_of("finish_inside_speech_does_not_expose_the_defect"))
}

#[test]
#[ignore = "child process of the RP-68 test of the same name"]
fn finish_inside_speech_does_not_expose_the_defect_child() -> TestResult {
    let voice = voice()?;
    let inside = voice
        .get(..HALF_SECOND)
        .ok_or_else(|| String::from("the first utterance is shorter than 0.5 s"))?;
    let mut engine = open_engine()?;
    feed_engine(&mut engine, inside)?;
    engine.finish().map_err(|why| why.to_string())?;
    feed_engine(&mut engine, &silence(LONG_SILENCE))?;
    Ok(())
}

#[test]
fn a_second_utterance_after_finish_ends_cleanly() -> TestResult {
    expect_normal(&child_of("a_second_utterance_after_finish_ends_cleanly"))
}

#[test]
#[ignore = "child process of the RP-68 test of the same name"]
fn a_second_utterance_after_finish_ends_cleanly_child() -> TestResult {
    engine_run(&voice()?, SHORT_GAP, true)
}

// T-RP-68-010 to 014: the harness reads how a child ended.

#[test]
fn the_harness_reads_a_nonzero_and_a_signalled_child_as_abnormal() -> TestResult {
    let parent = "the_harness_reads_a_nonzero_and_a_signalled_child_as_abnormal";
    let exited = run_child(&child_of(parent))?;
    assert_eq!(exited.code, Some(3), "{exited:?}");
    assert!(exited.is_abnormal(), "{exited:?}");
    let signalled = run_child(&format!("{parent}_signalled_child"))?;
    assert_eq!(signalled.code, None, "{signalled:?}");
    assert!(signalled.is_abnormal(), "{signalled:?}");
    Ok(())
}

#[test]
#[ignore = "child process of the RP-68 test of the same name"]
fn the_harness_reads_a_nonzero_and_a_signalled_child_as_abnormal_child() -> TestResult {
    std::process::exit(3)
}

#[test]
#[ignore = "child process of the RP-68 test of the same name"]
fn the_harness_reads_a_nonzero_and_a_signalled_child_as_abnormal_signalled_child() -> TestResult {
    std::process::abort()
}

#[test]
fn the_harness_reads_invalid_n_on_stderr_alone_as_abnormal() -> TestResult {
    let outcome = run_child(&child_of(
        "the_harness_reads_invalid_n_on_stderr_alone_as_abnormal",
    ))?;
    assert_eq!(outcome.code, Some(0), "{outcome:?}");
    assert!(outcome.is_abnormal(), "{outcome:?}");
    Ok(())
}

#[test]
#[ignore = "child process of the RP-68 test of the same name"]
fn the_harness_reads_invalid_n_on_stderr_alone_as_abnormal_child() -> TestResult {
    eprintln!("circular-buffer.cc:Get:142 Invalid n: -1. size: 8");
    Ok(())
}

#[test]
fn the_harness_reads_a_silent_clean_child_as_normal() -> TestResult {
    let outcome = run_child(&child_of(
        "the_harness_reads_a_silent_clean_child_as_normal",
    ))?;
    assert_eq!(outcome.code, Some(0), "{outcome:?}");
    assert!(!outcome.is_abnormal(), "{outcome:?}");
    Ok(())
}

#[test]
#[ignore = "child process of the RP-68 test of the same name"]
fn the_harness_reads_a_silent_clean_child_as_normal_child() -> TestResult {
    Ok(())
}

#[test]
fn a_child_name_that_runs_no_test_is_rejected() {
    let rejected = run_child("a_test_that_does_not_exist");
    assert!(
        matches!(&rejected, Err(why) if why.contains("did not run exactly one test")),
        "{rejected:?}"
    );
}

#[test]
fn a_clean_reproduction_child_is_reported_as_a_changed_premise() {
    let clean = child_of("the_harness_reads_a_silent_clean_child_as_normal");
    let rejected = reproduce(&clean);
    assert!(
        matches!(&rejected, Err(why) if why.contains("the premise of the reproduction changed")),
        "{rejected:?}"
    );
}

// T-RP-68-004: the early end of speech. V68, a gap and a long silence go
// through the engine with the early detector on; the main detector is never
// flushed, so the process ends cleanly and one final is made, at a notice of
// the early detector and not at the main detector's silence.

#[test]
fn the_early_end_never_flushes_the_detector() -> TestResult {
    expect_normal(&child_of("the_early_end_never_flushes_the_detector"))
}

fn feed_collect(engine: &mut Engine, samples: &[f32]) -> Result<Vec<Final>, Failure> {
    let mut finals = Vec::new();
    let (windows, _tail) = samples.as_chunks::<WINDOW>();
    for window in windows {
        let made = engine
            .feed(window.as_slice())
            .map_err(|why| why.to_string())?;
        finals.extend(made);
    }
    Ok(finals)
}

#[test]
#[ignore = "child process of the RP-68 test of the same name"]
fn the_early_end_never_flushes_the_detector_child() -> TestResult {
    let mut settings = Settings::new(models_dir()?);
    settings.stages.speakers = false;
    settings.endpoint = Mode::Early;
    let mut engine = Engine::open(settings).map_err(|why| why.to_string())?;
    let voice = voice()?;
    let mut finals = feed_collect(&mut engine, &voice)?;
    finals.extend(feed_collect(&mut engine, &silence(LONG_GAP))?);
    finals.extend(feed_collect(&mut engine, &silence(LONG_SILENCE))?);
    assert_eq!(finals.len(), 1, "{finals:?}");
    let made = &finals[0];
    assert_eq!(made.endpoint.rule, Rule::Early, "{made:?}");
    assert_eq!(made.endpoint.silence_ms, 120, "{made:?}");
    // The line is the end of a window, and it lies in the silence after the voice.
    let line = (made.t1_s * 16_000.0).round() as usize;
    assert_eq!(line % WINDOW, 0, "{line}");
    assert!(line >= voice.len(), "{line} is inside the voice");
    // A second utterance after the first is read as well: the detector was not left in a bad state.
    let mut again = feed_collect(&mut engine, &voice)?;
    again.extend(feed_collect(&mut engine, &silence(LONG_SILENCE))?);
    assert_eq!(again.len(), 1, "{again:?}");
    Ok(())
}

// T-RP-68-015: the acceptance, every child in turn.

#[test]
fn rp68_flush_inside_speech_aborts_and_the_early_end_never_flushes() -> TestResult {
    let reproductions = [
        "a_flush_inside_speech_then_a_long_silence_is_abnormal_after_a_short_gap",
        "a_flush_inside_speech_then_a_long_silence_is_abnormal_after_a_long_gap",
    ];
    let guards = [
        "without_the_flush_the_same_audio_ends_cleanly",
        "finish_after_a_short_silence_then_a_long_silence_ends_cleanly",
        "finish_after_a_long_gap_then_a_long_silence_ends_cleanly",
        "a_second_utterance_after_finish_ends_cleanly",
        "the_early_end_never_flushes_the_detector",
    ];
    for parent in reproductions {
        reproduce(&child_of(parent))?;
    }
    for parent in guards {
        let outcome = run_child(&child_of(parent))?;
        assert!(
            !outcome.is_abnormal(),
            "{parent} ended abnormally: {outcome:?}"
        );
    }
    Ok(())
}

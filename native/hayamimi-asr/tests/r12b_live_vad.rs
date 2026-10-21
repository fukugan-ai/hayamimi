//! R12b: the live voice detector is rebuilt with new settings only when that
//! loses no audio, and segment positions stay on the timeline of the whole
//! input across a rebuild. The detector is the real Silero model on the CPU
//! and the speech is a golden clip; the model is not in git, so a worktree
//! reads it from the main checkout.
//!
//! The caller of `Vad::rebuild` asks `Vad::is_idle` first, so the tests that
//! stand for "a rebuild is deferred" read `is_idle` at the moment the caller
//! would.

mod support;

use std::error::Error;
use std::path::Path;

use hayamimi_asr::AsrError;
use hayamimi_asr::audio::{WINDOW, read_wav, windows};
use hayamimi_asr::vad::{Segment, Sensitivity, VAD_FILE, Vad, VadSettings};
use support::{GOLDEN, TestResult, models_dir};

/// One second of silence on each side of the clip, so that every utterance closes.
fn padded_clip() -> Result<Vec<f32>, Box<dyn Error>> {
    let speech = read_wav(&Path::new(GOLDEN).join("ja_014.wav"))?;
    let silence = vec![0.0_f32; 16_000];
    Ok([silence.as_slice(), speech.as_slice(), silence.as_slice()].concat())
}

fn open() -> Result<Vad, Box<dyn Error>> {
    Ok(Vad::open(&models_dir()?, VadSettings::default())?)
}

/// Feeds whole windows of `audio`, taking the finished segments after each.
/// Returns the number of samples fed and the segments; the clip must end
/// outside speech.
fn feed(vad: &mut Vad, audio: &[f32]) -> Result<(usize, Vec<Segment>), Box<dyn Error>> {
    let mut fed = 0;
    let mut found = Vec::new();
    for window in windows(audio) {
        vad.accept(&window);
        fed += window.len();
        found.extend(vad.take_segments()?);
    }
    if vad.detected() {
        return Err("the clip ended inside speech".into());
    }
    Ok((fed, found))
}

fn with_threshold(vad: &Vad, threshold: f32) -> Sensitivity {
    Sensitivity {
        threshold,
        ..vad.sensitivity()
    }
}

#[test]
fn live_vad_applies_pending_change_at_a_safe_boundary() -> TestResult {
    let mut vad = open()?;
    vad.accept(&[0.0; WINDOW]);
    assert!(vad.is_idle(), "silence is a safe boundary");
    let wanted = with_threshold(&vad, 0.7);
    vad.rebuild(wanted)?;
    assert_eq!(vad.sensitivity(), wanted);
    Ok(())
}

#[test]
fn live_vad_defers_rebuild_while_speech_is_active() -> TestResult {
    let mut vad = open()?;
    let before = vad.sensitivity();
    let mut inside = false;
    for window in windows(&padded_clip()?) {
        vad.accept(&window);
        if vad.detected() && !inside {
            inside = true;
            assert!(
                !vad.is_idle(),
                "a rebuild must wait while speech is in progress"
            );
            assert_eq!(vad.sensitivity(), before, "the old settings stay in effect");
        }
        vad.take_segments()?;
    }
    assert!(inside, "the clip must contain speech");
    // The segment closed and was taken: the next boundary is safe.
    assert!(!vad.detected());
    assert!(vad.is_idle());
    let wanted = with_threshold(&vad, 0.9);
    vad.rebuild(wanted)?;
    assert_eq!(vad.sensitivity(), wanted);
    Ok(())
}

#[test]
fn live_vad_waits_for_completed_segment_to_be_drained() -> TestResult {
    let mut vad = open()?;
    for window in windows(&padded_clip()?) {
        vad.accept(&window);
    }
    // The speech has ended, but its segment has not been taken.
    assert!(!vad.detected());
    assert!(!vad.is_idle(), "a finished segment is waiting");
    let taken = vad.take_segments()?;
    assert!(!taken.is_empty());
    assert!(vad.is_idle());
    Ok(())
}

#[test]
fn live_vad_rebuild_preserves_absolute_segment_positions() -> TestResult {
    let audio = padded_clip()?;
    let mut vad = open()?;
    let (length, first) = feed(&mut vad, &audio)?;
    assert!(
        !first.is_empty(),
        "the real clip must produce speech segments"
    );
    // A rebuild with the settings already in effect makes the same segments
    // from the same audio, so each one lands exactly one clip later on the
    // timeline, and the offsets of successive rebuilds add up.
    for round in 1..=2 {
        vad.rebuild(vad.sensitivity())?;
        let (_, again) = feed(&mut vad, &audio)?;
        assert_eq!(again.len(), first.len(), "round {round}");
        for (found, original) in again.iter().zip(&first) {
            assert_eq!(
                found.start,
                original.start + round * length,
                "round {round}"
            );
        }
    }
    Ok(())
}

#[test]
fn live_vad_real_speech_matches_fresh_detector_after_rebuild() -> TestResult {
    let audio = padded_clip()?;
    let mut live = open()?;
    let (offset, first) = feed(&mut live, &audio)?;
    let (_, reference_first) = feed(&mut open()?, &audio)?;

    let mut fresh = open()?;
    fresh.rebuild(with_threshold(&fresh, 0.7))?;
    live.rebuild(with_threshold(&live, 0.7))?;
    let (_, second) = feed(&mut live, &audio)?;
    let (_, reference_second) = feed(&mut fresh, &audio)?;

    for (actual, reference, shift) in [
        (first, reference_first, 0),
        (second, reference_second, offset),
    ] {
        assert!(
            !actual.is_empty(),
            "the real clip must produce speech segments"
        );
        assert_eq!(actual.len(), reference.len());
        for (found, wanted) in actual.iter().zip(&reference) {
            assert_eq!(found.start, wanted.start + shift);
            assert_eq!(found.samples, wanted.samples);
        }
    }
    assert_eq!(live.sensitivity().threshold, 0.7);
    Ok(())
}

/// A missing model is an error the caller can read, not the end of the process:
/// the C library exits on a model path that is not there, so reaching it would
/// end this test run instead of failing it.
#[test]
fn build_vad_raises_not_exits_when_model_missing() -> TestResult {
    let missing = models_dir()?.join("no-such-models");
    let said = Vad::open(&missing, VadSettings::default()).err();
    assert!(
        matches!(&said, Some(AsrError::MissingModel(path)) if *path == missing.join(VAD_FILE)),
        "{said:?}"
    );
    Ok(())
}

//! R12b: the live voice detector is rebuilt with new settings only when that
//! loses no audio, and segment positions stay on the timeline of the whole
//! input across a rebuild. The detector is the real Silero model on the CPU
//! and the speech is a golden clip; the model is not in git, so a worktree
//! reads it from the main checkout.

mod support;

use std::path::Path;

use hayamimi_asr::audio::{WINDOW, read_wav};
use hayamimi_asr::vad::{Segment, Sensitivity, Vad, VadSettings};
use support::{GOLDEN, TestResult, models_dir};

/// One second of silence on each side of the clip, so that every utterance closes.
fn padded_clip() -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    let speech = read_wav(&Path::new(GOLDEN).join("ja_014.wav"))?;
    let silence = vec![0.0_f32; 16_000];
    Ok([silence.as_slice(), speech.as_slice(), silence.as_slice()].concat())
}

fn open() -> Result<Vad, Box<dyn std::error::Error>> {
    Ok(Vad::open(&models_dir()?, VadSettings::default())?)
}

/// Feeds whole windows of `audio`, taking the finished segments after each.
/// Returns the number of samples fed and the segments.
fn feed(vad: &mut Vad, audio: &[f32]) -> Result<(usize, Vec<Segment>), Box<dyn std::error::Error>> {
    let mut fed = 0;
    let mut found = Vec::new();
    for window in audio.chunks_exact(WINDOW) {
        vad.accept(window);
        fed += window.len();
        found.extend(vad.take_segments()?);
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
    for window in padded_clip()?.chunks_exact(WINDOW) {
        vad.accept(window);
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
    for window in padded_clip()?.chunks_exact(WINDOW) {
        vad.accept(window);
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
    let (offset, first) = feed(&mut vad, &audio)?;
    assert!(!first.is_empty());
    assert!(first.iter().all(|segment| segment.start < offset));
    let wanted = with_threshold(&vad, 0.7);
    vad.rebuild(wanted)?;
    let (_, second) = feed(&mut vad, &audio)?;
    assert!(!second.is_empty());
    assert!(
        second.iter().all(|segment| segment.start >= offset),
        "positions after the rebuild continue the timeline: {second:?}"
    );
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

//! BR-8: do the short voices and the shouts become segments? A record only: it
//! runs the production VAD over the clips of the evaluation sets with the
//! shortest speech it keeps (`MIN_SPEECH_SECONDS`) at 0.25, 0.15 and 0.10 s and
//! the cutoff at 0.5 and 0.4, and counts, for each of the six pairs, the
//! segments, the utterances that got no segment and the segments that swallowed
//! a neighbour (design 10.13.13, 19). Nothing here changes a product value: the
//! shortest speech is set on the detector's configuration inside the test, and
//! `vad.rs` keeps 0.25 s and 0.5 (T-BR-08-007).
//!
//! The real rows (T-BR-08-001 and 002) read `testdata/` and `models/`, which are
//! not in git, and run with `-- --ignored`. The row that writes the record is
//! the first of them: `docs/results/in-person/br08_vad.md`.
//!
//! How this can break, and what stops it:
//!   first run   the VAD model is read from `models/silero_vad.onnx`; a machine
//!               without it, or without `testdata/`, fails with the path it looked for.
//!   contention  the detector is built under the session lock the other GPU probes use.
//!   start       the record is written after all six pairs have run; a pair that
//!               cannot run fails the row and no half record is written.
//!   waiting     none: no prompt, no stdin.

mod support;

use std::path::Path;

use hayamimi_asr::audio::{WINDOW, read_wav};
use hayamimi_asr::vad::{Segment, VadSettings, silero_config_with, take_segments};
use hayamimi_gpu::session_lock::lock_for_session;
use serde_json::Value;
use sherpa_onnx::VoiceActivityDetector;
use support::{Failure, REPO, TestResult, models_dir, read, read_json, testdata_dir};

/// The shortest speech the detector keeps, in seconds, and the cutoff of its
/// speech probability, in the order the record lists them.
const PAIRS: [(f32, f32); 6] = [
    (0.25, 0.5),
    (0.25, 0.4),
    (0.15, 0.5),
    (0.15, 0.4),
    (0.10, 0.5),
    (0.10, 0.4),
];

/// Seconds of audio the detector's ring buffer holds (the production value).
const BUFFER_SECONDS: f32 = 30.0;

/// A stretch of samples, `[start, end)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Span {
    start: usize,
    end: usize,
}

/// Whether two stretches share a sample. Half-open, so stretches that only touch
/// do not (design 10.13.13, 19).
fn overlaps(a: Span, b: Span) -> bool {
    a.start < b.end && b.start < a.end
}

/// What the segments of one pair made of a set of utterances.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Counts {
    segments: usize,
    /// The 1-based numbers of the utterances no segment overlaps.
    silent: Vec<usize>,
    /// The segments that overlap two utterances or more.
    swallowing: usize,
}

fn count(utterances: &[Span], segments: &[Span]) -> Counts {
    let silent = utterances
        .iter()
        .enumerate()
        .filter(|(_, utterance)| !segments.iter().any(|s| overlaps(*s, **utterance)))
        .map(|(at, _)| at + 1)
        .collect();
    let swallowing = segments
        .iter()
        .filter(|segment| {
            utterances
                .iter()
                .filter(|u| overlaps(**segment, **u))
                .count()
                >= 2
        })
        .count();
    Counts {
        segments: segments.len(),
        silent,
        swallowing,
    }
}

fn span(start: usize, end: usize) -> Span {
    Span { start, end }
}

// ---- the detector ---------------------------------------------------------------

fn queue_segments(detector: &VoiceActivityDetector, spans: &mut Vec<Span>) -> Result<(), Failure> {
    let found = take_segments(detector).map_err(|why| why.to_string())?;
    spans.extend(found.iter().map(|Segment { start, samples }| Span {
        start: *start,
        end: start + samples.len(),
    }));
    Ok(())
}

/// The segments the production detector makes of `samples` with the shortest
/// speech and the cutoff given.
fn detect(
    models: &Path,
    samples: &[f32],
    (min_speech, threshold): (f32, f32),
) -> Result<Vec<Span>, Failure> {
    let mut config = silero_config_with(models, VadSettings::default(), threshold)
        .map_err(|why| why.to_string())?;
    config.silero_vad.min_speech_duration = min_speech;
    let detector = {
        let _building = lock_for_session();
        VoiceActivityDetector::create(&config, BUFFER_SECONDS)
    };
    let Some(detector) = detector else {
        return Err(String::from("the VAD could not be created"));
    };
    let mut spans = Vec::new();
    let (windows, _tail) = samples.as_chunks::<WINDOW>();
    for window in windows {
        detector.accept_waveform(window.as_slice());
        queue_segments(&detector, &mut spans)?;
    }
    detector.flush();
    queue_segments(&detector, &mut spans)?;
    Ok(spans)
}

// ---- the sets -------------------------------------------------------------------

fn sample_field(row: &Value, key: &str) -> Result<usize, Failure> {
    let number = row
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("a manifest row has no integer `{key}`"))?;
    usize::try_from(number).map_err(|why| format!("{key}: {why}"))
}

/// The utterance windows of a manifest of one long recording, in order.
fn manifest_spans(path: &Path) -> Result<Vec<Span>, Failure> {
    let Some(rows) = read_json(path)?.as_array().cloned() else {
        return Err(format!("{} is not an array", path.display()));
    };
    rows.iter()
        .map(|row| {
            Ok(span(
                sample_field(row, "start_sample")?,
                sample_field(row, "end_sample")?,
            ))
        })
        .collect()
}

/// The counts of one long recording (`lat-dialog`, `lat-short`) for one pair.
fn long_recording(
    models: &Path,
    wav: &Path,
    manifest: &Path,
    pair: (f32, f32),
) -> Result<Counts, Failure> {
    let samples = read_wav(wav).map_err(|why| why.to_string())?;
    let segments = detect(models, &samples, pair)?;
    Ok(count(&manifest_spans(manifest)?, &segments))
}

/// The clips of `eval_real`, each a recording of its own with one utterance
/// that spans all of it: the counts of the whole set for one pair.
fn separate_clips(models: &Path, directory: &Path, pair: (f32, f32)) -> Result<Counts, Failure> {
    let manifest = read_json(&directory.join("manifest.json"))?;
    let Some(list) = manifest.as_array() else {
        return Err(String::from("eval_real/manifest.json is not an array"));
    };
    let mut total = Counts {
        segments: 0,
        silent: Vec::new(),
        swallowing: 0,
    };
    for (at, clip) in list.iter().enumerate() {
        let name = clip
            .get("wav")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("clip {} of eval_real has no `wav`", at + 1))?;
        let samples = read_wav(&directory.join(name)).map_err(|why| why.to_string())?;
        let segments = detect(models, &samples, pair)?;
        let found = count(&[span(0, samples.len())], &segments);
        total.segments += found.segments;
        total.swallowing += found.swallowing;
        if !found.silent.is_empty() {
            total.silent.push(at + 1);
        }
    }
    Ok(total)
}

/// What one pair made of the three sets.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PairReport {
    pair_at: usize,
    eval_real: Counts,
    dialog: Counts,
    short: Counts,
}

fn report_of(pair_at: usize) -> Result<PairReport, Failure> {
    let models = models_dir()?;
    let testdata = testdata_dir()?;
    let manifests = testdata.join("manifests");
    let pair = PAIRS[pair_at];
    Ok(PairReport {
        pair_at,
        eval_real: separate_clips(&models, &testdata.join("eval_real"), pair)?,
        dialog: long_recording(
            &models,
            &testdata.join("lat-dialog.wav"),
            &manifests.join("lat-dialog.json"),
            pair,
        )?,
        short: long_recording(
            &models,
            &testdata.join("lat-short.wav"),
            &manifests.join("lat-short.json"),
            pair,
        )?,
    })
}

fn all_reports() -> Result<Vec<PairReport>, Failure> {
    (0..PAIRS.len()).map(report_of).collect()
}

fn counts_text(label: &str, unit: &str, counts: &Counts) -> String {
    format!(
        "{label}: セグメント {}、区間の無い{unit} {:?}、隣の発話を飲んだ区間 {}\n",
        counts.segments, counts.silent, counts.swallowing
    )
}

fn record_text(reports: &[PairReport]) -> String {
    let mut text = String::from(
        "# BR-8: 短い声と叫びが区間になるか\n\n\
         `MIN_SPEECH_SECONDS`（秒）と閾値の組ごとに、本番の VAD（無音 0.30 秒、最長 12 秒）へ \
         `eval_real` の 30 clip（1 本ずつ）、`lat-dialog`（30 発話）、`lat-short`（20 発話）を通した値。\n\
         eval_real の「区間の無い clip」は manifest の順の番号。製品の値は動かしていない。\n\n",
    );
    for report in reports {
        let (min_speech, threshold) = PAIRS[report.pair_at];
        text.push_str(&format!("## ({min_speech}, {threshold})\n\n"));
        text.push_str(&counts_text("eval_real", "clip", &report.eval_real));
        text.push_str(&counts_text("lat-dialog", "発話", &report.dialog));
        text.push_str(&counts_text("lat-short", "発話", &report.short));
        text.push('\n');
    }
    text
}

// ---- the real rows --------------------------------------------------------------

/// T-BR-08-001
#[test]
#[ignore = "reads testdata/ and the VAD model, and writes docs/results/in-person/br08_vad.md"]
fn br08_short_and_shouted_voices_are_counted_with_the_neighbours_they_swallow() -> TestResult {
    let reports = all_reports()?;
    let order: Vec<(f32, f32)> = reports.iter().map(|report| PAIRS[report.pair_at]).collect();
    assert_eq!(order, PAIRS);
    assert!(
        reports
            .iter()
            .all(|report| report.dialog.segments > 0 && report.short.segments > 0)
    );
    let record = Path::new(REPO).join("docs/results/in-person/br08_vad.md");
    std::fs::write(record, record_text(&reports))?;
    Ok(())
}

/// T-BR-08-002
#[test]
#[ignore = "reads testdata/ and the VAD model"]
fn the_current_setting_leaves_the_known_shouted_and_short_voices_without_a_segment() -> TestResult {
    let report = report_of(0)?;
    assert_eq!(PAIRS[0], (0.25, 0.5));
    assert!(
        report.dialog.silent.contains(&3),
        "{:?}",
        report.dialog.silent
    );
    for row in [1, 5, 7, 14] {
        assert!(
            report.short.silent.contains(&row),
            "{row} in {:?}",
            report.short.silent
        );
    }
    Ok(())
}

// ---- the tool on speech made by hand --------------------------------------------

/// T-BR-08-003
#[test]
fn one_utterance_split_into_two_segments_swallows_nothing() {
    let found = count(&[span(0, 48_000)], &[span(0, 20_000), span(28_000, 48_000)]);
    assert_eq!(found.segments, 2);
    assert_eq!(found.swallowing, 0);
    assert!(found.silent.is_empty());
}

/// T-BR-08-004
#[test]
fn a_segment_over_two_utterances_counts_as_swallowing_a_neighbour() {
    let utterances = [span(0, 16_000), span(20_000, 36_000)];
    let found = count(&utterances, &[span(8_000, 30_000)]);
    assert_eq!(found.swallowing, 1);
    assert!(found.silent.is_empty());
}

/// T-BR-08-005
#[test]
fn a_segment_that_only_touches_an_utterance_does_not_cover_it() {
    let found = count(&[span(100, 200)], &[span(0, 100)]);
    assert_eq!(found.silent, vec![1]);
}

/// T-BR-08-006
#[test]
fn a_segment_that_overlaps_an_utterance_by_one_sample_covers_it() {
    let found = count(&[span(100, 200)], &[span(0, 101)]);
    assert!(found.silent.is_empty());
}

/// T-BR-08-007
#[test]
fn the_vad_line_is_not_moved_by_the_record_tool() -> TestResult {
    let vad = read(&Path::new(REPO).join("native/hayamimi-asr/src/vad.rs"))?;
    let lines = |pattern: &str| vad.lines().filter(|line| line.contains(pattern)).count();
    // The two lines of the product: the shortest speech and the cutoff.
    assert_eq!(
        lines("MIN_SPEECH_SECONDS: f32 = 0.25") + lines("const THRESHOLD: f32 = 0.5"),
        2
    );
    // The control: the pattern does find the constant when it is there.
    assert!(lines("MIN_SPEECH_SECONDS") >= 1);
    Ok(())
}

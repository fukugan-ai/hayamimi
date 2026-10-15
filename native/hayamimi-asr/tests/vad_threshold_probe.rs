//! Offline probe of Silero's speech-probability cutoff (0.5, 0.6, 0.7).
//!
//! It cuts `lat-ja.wav` and `lat-en.wav` into segments at each cutoff with the
//! production detector settings and compares every segment with the utterance
//! windows of the manifest. Nothing here changes a product default. For each
//! utterance it reports:
//!
//! - the end of the last segment minus the manifest end (ms),
//! - the number of splits (segments inside the utterance minus one),
//! - the start of the first segment minus the manifest start (ms).
//!
//! Per cutoff and language it prints the end difference p50 and p95, the sum
//! of splits and the largest start shift, and a verdict against 0.5: a cutoff
//! is rejected when the splits grow or when a start moves later than 100 ms
//! compared with the same utterance at 0.5.
//!
//!     cargo test --release -p hayamimi-asr --test vad_threshold_probe -- --ignored --nocapture

mod support;

use std::path::Path;

use hayamimi_asr::audio::{WINDOW, read_wav};
use hayamimi_asr::vad::{Segment, VadSettings, silero_config_with, take_segments};
use hayamimi_eval::percentile::nearest_rank;
use hayamimi_gpu::session_lock::lock_for_session;
use serde_json::Value;
use sherpa_onnx::VoiceActivityDetector;
use support::{Failure, TestResult, models_dir, read_json, testdata_dir};

const THRESHOLDS: [f32; 3] = [0.5, 0.6, 0.7];
/// Samples per millisecond at 16 kHz.
const SAMPLES_PER_MS: i64 = 16;
/// A start later than this, against the same utterance at 0.5, rejects a cutoff.
const START_LIMIT_MS: i64 = 100;
/// Seconds of audio the detector's ring buffer holds (the production value).
const BUFFER_SECONDS: f32 = 30.0;

/// A stretch of the input in samples: a manifest utterance or a detected segment.
#[derive(Clone, Copy, Debug)]
struct Span {
    start: usize,
    end: usize,
}

/// What one utterance got at one cutoff.
#[derive(Clone, Copy, Debug)]
struct Row {
    segments: usize,
    end_diff_ms: Option<i64>,
    start_diff_ms: Option<i64>,
}

fn ms_between(later: usize, earlier: usize) -> i64 {
    (later as i64 - earlier as i64) / SAMPLES_PER_MS
}

fn sample_field(entry: &Value, key: &str) -> Result<usize, Failure> {
    let number = entry
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("a manifest entry has no integer `{key}`"))?;
    usize::try_from(number).map_err(|why| format!("{key}: {why}"))
}

/// The utterance windows of a manifest, in order.
fn manifest_spans(path: &Path) -> Result<Vec<Span>, Failure> {
    let Some(entries) = read_json(path)?.as_array().cloned() else {
        return Err(format!("{} is not an array", path.display()));
    };
    entries
        .iter()
        .map(|entry| {
            Ok(Span {
                start: sample_field(entry, "start_sample")?,
                end: sample_field(entry, "end_sample")?,
            })
        })
        .collect()
}

fn queue_segments(detector: &VoiceActivityDetector, spans: &mut Vec<Span>) -> Result<(), Failure> {
    let found = take_segments(detector).map_err(|why| why.to_string())?;
    spans.extend(found.iter().map(|Segment { start, samples }| Span {
        start: *start,
        end: start + samples.len(),
    }));
    Ok(())
}

/// The segments the production detector settings cut `samples` into at `threshold`.
fn detect(models: &Path, samples: &[f32], threshold: f32) -> Result<Vec<Span>, Failure> {
    let config = silero_config_with(models, VadSettings::default(), threshold)
        .map_err(|why| why.to_string())?;
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

/// The row of one utterance: the segments that overlap its window.
fn row_of(utterance: &Span, segments: &[Span]) -> Row {
    let hits: Vec<&Span> = segments
        .iter()
        .filter(|s| s.start < utterance.end && s.end > utterance.start)
        .collect();
    Row {
        segments: hits.len(),
        end_diff_ms: hits.last().map(|s| ms_between(s.end, utterance.end)),
        start_diff_ms: hits.first().map(|s| ms_between(s.start, utterance.start)),
    }
}

fn rows_of(utterances: &[Span], segments: &[Span]) -> Vec<Row> {
    utterances.iter().map(|u| row_of(u, segments)).collect()
}

fn splits_of(rows: &[Row]) -> usize {
    rows.iter().map(|r| r.segments.saturating_sub(1)).sum()
}

fn missed_of(rows: &[Row]) -> usize {
    rows.iter().filter(|r| r.segments == 0).count()
}

fn percentile_text(values: &[i64], p: usize) -> Result<String, Failure> {
    if values.is_empty() {
        return Ok(String::from("n/a"));
    }
    nearest_rank(values, p)
        .map(|value| value.to_string())
        .map_err(|why| why.to_string())
}

fn end_diffs(rows: &[Row]) -> Vec<i64> {
    rows.iter().filter_map(|r| r.end_diff_ms).collect()
}

/// The largest start shift against the same utterance at the baseline cutoff.
fn worst_start_delay(rows: &[Row], baseline: &[Row]) -> Option<i64> {
    rows.iter()
        .zip(baseline)
        .filter_map(|(row, base)| Some(row.start_diff_ms? - base.start_diff_ms?))
        .max()
}

fn utterances_split_more(rows: &[Row], baseline: &[Row]) -> usize {
    rows.iter()
        .zip(baseline)
        .filter(|(row, base)| row.segments > base.segments)
        .count()
}

fn per_utterance_lines(lang: &str, threshold: f32, rows: &[Row]) -> Vec<String> {
    rows.iter()
        .enumerate()
        .map(|(index, row)| {
            format!(
                "th={threshold} {lang} utt={} segments={} end_diff_ms={:?} splits={} start_diff_ms={:?}",
                index + 1,
                row.segments,
                row.end_diff_ms,
                row.segments.saturating_sub(1),
                row.start_diff_ms
            )
        })
        .collect()
}

/// Why a cutoff is rejected against the baseline, or none.
fn rejections(rows: &[Row], baseline: &[Row]) -> Vec<String> {
    let mut why = Vec::new();
    if splits_of(rows) > splits_of(baseline) {
        why.push(format!(
            "splits {} > {}",
            splits_of(rows),
            splits_of(baseline)
        ));
    }
    if let Some(delay) = worst_start_delay(rows, baseline).filter(|d| *d > START_LIMIT_MS) {
        why.push(format!("a start is {delay} ms later than at 0.5"));
    }
    why
}

fn summary_line(
    lang: &str,
    threshold: f32,
    rows: &[Row],
    baseline: &[Row],
) -> Result<String, Failure> {
    let ends = end_diffs(rows);
    let worst = worst_start_delay(rows, baseline);
    let bad = rejections(rows, baseline);
    let verdict = if bad.is_empty() {
        String::from("NOT REJECTED")
    } else {
        format!("REJECTED ({})", bad.join("; "))
    };
    Ok(format!(
        "SUMMARY th={threshold} {lang} utterances={} end_diff_ms p50={} p95={} splits_total={} \
         utterances_split_more_than_0.5={} missed={} start_diff_max_ms={:?} \
         start_delay_vs_0.5_max_ms={worst:?} -> {verdict}",
        rows.len(),
        percentile_text(&ends, 50)?,
        percentile_text(&ends, 95)?,
        splits_of(rows),
        utterances_split_more(rows, baseline),
        missed_of(rows),
        rows.iter().filter_map(|r| r.start_diff_ms).max(),
    ))
}

/// The lines for one language: every utterance and every cutoff, then the summaries.
fn language_report(
    models: &Path,
    lang: &str,
    wav: &Path,
    manifest: &Path,
) -> Result<Vec<String>, Failure> {
    let samples = read_wav(wav).map_err(|why| why.to_string())?;
    let utterances = manifest_spans(manifest)?;
    let mut per_threshold = Vec::new();
    for threshold in THRESHOLDS {
        let segments = detect(models, &samples, threshold)?;
        per_threshold.push((threshold, rows_of(&utterances, &segments), segments.len()));
    }
    let baseline = per_threshold[0].1.clone();
    let mut lines = Vec::new();
    for (threshold, rows, segments) in &per_threshold {
        lines.push(format!(
            "th={threshold} {lang} segments_in_total={segments}"
        ));
        lines.extend(per_utterance_lines(lang, *threshold, rows));
    }
    for (threshold, rows, _) in &per_threshold {
        lines.push(summary_line(lang, *threshold, rows, &baseline)?);
    }
    Ok(lines)
}

#[test]
#[ignore = "an offline probe: reads testdata/ and the VAD model"]
fn vad_threshold_probe_0_5_0_6_0_7() -> TestResult {
    let models = models_dir()?;
    let testdata = testdata_dir()?;
    for lang in ["ja", "en"] {
        let wav = testdata.join(format!("lat-{lang}.wav"));
        let manifest = testdata.join("manifests").join(format!("lat-{lang}.json"));
        for line in language_report(&models, lang, &wav, &manifest)? {
            println!("{line}");
        }
    }
    Ok(())
}

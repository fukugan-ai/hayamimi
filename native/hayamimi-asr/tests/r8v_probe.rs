//! R8v step 1: an offline probe of the two-stage end-of-speech decision (3.17).
//!
//! The current detector (threshold 0.5, min_silence 0.30 s) and an early one
//! (threshold 0.65, min_silence 0.12 s, the same 0.25 s shortest speech) are
//! fed the same windows of `lat-ja.wav` and `lat-en.wav` in the same order.
//! A notice of the early detector is a candidate only while the current
//! detector is inside speech. A candidate is solved from the voice start of
//! the current detector (the pre-roll included) to the candidate's sample
//! through the engine, so the text has been through recognition, ITN and
//! punctuation; the completeness table below is then applied to it.
//!
//! Nothing here changes a product default. The tables are constants of this
//! file in the shape `endpoint.rs` will take them.
//!
//!     cargo test --release -p hayamimi-asr --test r8v_probe -- --ignored --nocapture

mod support;

use std::path::Path;
use std::time::Instant;

use hayamimi_asr::audio::{PREROLL_SECONDS, SAMPLE_RATE, WINDOW, read_wav};
use hayamimi_asr::route::Policy;
use hayamimi_asr::vad::{
    DEFAULT_THRESHOLD, Segment, VadSettings, silero_config_with, take_segments,
};
use hayamimi_asr::{Engine, Settings};
use hayamimi_eval::percentile::nearest_rank;
use hayamimi_gpu::session_lock::lock_for_session;
use serde_json::Value;
use sherpa_onnx::VoiceActivityDetector;
use support::{Failure, TestResult, models_dir, read_json, testdata_dir};

/// The early detector: threshold 0.65 puts its end line at 0.50.
const EARLY_THRESHOLD: f32 = 0.65;
const EARLY_SETTINGS: VadSettings = VadSettings {
    min_silence: 0.12,
    max_speech: 12.0,
};
const NOW_SETTINGS: VadSettings = VadSettings {
    min_silence: 0.30,
    max_speech: 12.0,
};
const BUFFER_SECONDS: f32 = 30.0;
/// A candidate this close to the manifest end, or earlier, has speech still
/// to come: 300 ms at 16 kHz.
const STILL_SPEAKING: usize = 4800;
/// The manifest end of ja moved by at least this many ms is a pass (R8v step 1).
const REQUIRED_GAIN_MS: i64 = 150;
/// The limit of the p95 of the detectors' time per window (C3), in microseconds.
const VAD_P95_LIMIT_US: i64 = 2000;

/// Sentence endings that can close a Japanese sentence (an initial table; the
/// rows are fixed by the test-designer in step 2).
const JA_FINAL: [&str; 17] = [
    "です",
    "ます",
    "ました",
    "でした",
    "ません",
    "ない",
    "なかった",
    "ている",
    "ていた",
    "た",
    "だ",
    "ね",
    "よ",
    "か",
    "う",
    "る",
    "い",
];
/// Connective forms: a sentence that ends in one of these goes on.
const JA_CONNECTIVE: [&str; 14] = [
    "が",
    "けど",
    "けれど",
    "けれども",
    "て",
    "で",
    "し",
    "から",
    "ので",
    "のに",
    "ば",
    "たら",
    "と",
    "ながら",
];
/// Words an English sentence does not end on.
const EN_CONTINUATION: [&str; 22] = [
    "and", "but", "so", "because", "the", "a", "an", "to", "of", "or", "that", "with", "for", "in",
    "on", "at", "by", "if", "when", "as", "than", "then",
];

/// A stretch of the input in samples: a manifest utterance or a detected segment.
#[derive(Clone, Copy, Debug)]
struct Span {
    start: usize,
    end: usize,
}

/// A segment a detector handed over, the sample at which it did so, and
/// whether the current detector was inside speech then.
#[derive(Clone, Copy, Debug)]
struct Notice {
    span: Span,
    seen: usize,
    in_voice: bool,
}

/// What the two detectors did over one recording.
struct Pass {
    now: Vec<Notice>,
    early: Vec<Notice>,
    now_us: Vec<i64>,
    both_us: Vec<i64>,
}

/// A candidate with its solved text and verdicts.
struct Candidate {
    notice: Notice,
    text: String,
    complete: bool,
    inside_window: bool,
}

fn is_terminal(ch: char) -> bool {
    matches!(ch, '。' | '？' | '！' | '.' | '?' | '!')
}

/// A Japanese sentence is complete when it ends in 。？！, the form before that
/// is a sentence ending, and it is not a connective.
fn ja_complete(text: &str) -> bool {
    let trimmed = text.trim_end();
    if !trimmed.chars().last().is_some_and(is_terminal) {
        return false;
    }
    let body = trimmed.trim_end_matches(is_terminal);
    !JA_CONNECTIVE.iter().any(|tail| body.ends_with(tail))
        && JA_FINAL.iter().any(|tail| body.ends_with(tail))
}

/// An English sentence is complete when it ends in . ? ! and its last word is
/// not one the table lists.
fn en_complete(text: &str) -> bool {
    let trimmed = text.trim_end();
    if !trimmed.chars().last().is_some_and(is_terminal) {
        return false;
    }
    let last = trimmed
        .trim_end_matches(is_terminal)
        .split_whitespace()
        .last()
        .map(str::to_lowercase);
    last.is_some_and(|word| !EN_CONTINUATION.contains(&word.as_str()))
}

fn is_complete(lang: &str, text: &str) -> bool {
    match lang {
        "ja" => ja_complete(text),
        "en" => en_complete(text),
        _ => false,
    }
}

fn manifest_spans(path: &Path) -> Result<Vec<Span>, Failure> {
    let Some(entries) = read_json(path)?.as_array().cloned() else {
        return Err(format!("{} is not an array", path.display()));
    };
    let sample = |entry: &Value, key: &str| {
        let number = entry
            .get(key)
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("a manifest entry has no integer `{key}`"))?;
        usize::try_from(number).map_err(|why| format!("{key}: {why}"))
    };
    entries
        .iter()
        .map(|entry| {
            Ok(Span {
                start: sample(entry, "start_sample")?,
                end: sample(entry, "end_sample")?,
            })
        })
        .collect()
}

fn detector(
    models: &Path,
    settings: VadSettings,
    threshold: f32,
) -> Result<VoiceActivityDetector, Failure> {
    let config = silero_config_with(models, settings, threshold).map_err(|why| why.to_string())?;
    let _building = lock_for_session();
    VoiceActivityDetector::create(&config, BUFFER_SECONDS)
        .ok_or_else(|| String::from("the VAD could not be created"))
}

fn collect(
    detector: &VoiceActivityDetector,
    seen: usize,
    in_voice: bool,
    out: &mut Vec<Notice>,
) -> Result<(), Failure> {
    let found = take_segments(detector).map_err(|why| why.to_string())?;
    out.extend(found.iter().map(|Segment { start, samples }| Notice {
        span: Span {
            start: *start,
            end: start + samples.len(),
        },
        seen,
        in_voice,
    }));
    Ok(())
}

fn micros_since(started: Instant) -> i64 {
    started.elapsed().as_micros() as i64
}

/// Feeds both detectors the windows in order, timing each detector's work on a window.
fn run_pass(models: &Path, audio: &[f32]) -> Result<Pass, Failure> {
    let now = detector(models, NOW_SETTINGS, DEFAULT_THRESHOLD)?;
    let early = detector(models, EARLY_SETTINGS, EARLY_THRESHOLD)?;
    let mut pass = Pass {
        now: Vec::new(),
        early: Vec::new(),
        now_us: Vec::new(),
        both_us: Vec::new(),
    };
    let (windows, _tail) = audio.as_chunks::<WINDOW>();
    for (index, window) in windows.iter().enumerate() {
        let seen = (index + 1) * WINDOW;
        let started = Instant::now();
        now.accept_waveform(window.as_slice());
        collect(&now, seen, false, &mut pass.now)?;
        let now_us = micros_since(started);
        let started = Instant::now();
        early.accept_waveform(window.as_slice());
        let in_voice = now.detected();
        collect(&early, seen, in_voice, &mut pass.early)?;
        pass.now_us.push(now_us);
        pass.both_us.push(now_us + micros_since(started));
    }
    now.flush();
    collect(&now, audio.len(), false, &mut pass.now)?;
    early.flush();
    collect(&early, audio.len(), false, &mut pass.early)?;
    Ok(pass)
}

/// The segment of the current detector a candidate falls in: it started
/// before the candidate and was handed over after it.
fn enclosing(now: &[Notice], seen: usize) -> Option<Span> {
    now.iter()
        .rev()
        .find(|n| n.span.start <= seen && seen <= n.seen)
        .map(|n| n.span)
}

/// The text from the voice start (the pre-roll included) to `seen`, through the engine.
fn solve(
    models: &Path,
    audio: &[f32],
    voice: Span,
    seen: usize,
    lang: &str,
) -> Result<String, Failure> {
    let preroll = (PREROLL_SECONDS * f64::from(SAMPLE_RATE)) as usize;
    let from = voice.start.saturating_sub(preroll);
    let to = seen.min(audio.len());
    // A fresh engine per candidate: a reused one carries its detector's
    // timeline over and aborted inside the detector's buffer on the 7th slice.
    let mut engine = engine_for(models, lang)?;
    let finals = engine
        .transcribe(&audio[from..to])
        .map_err(|why| why.to_string())?;
    let texts: Vec<&str> = finals.iter().map(|done| done.text.as_str()).collect();
    Ok(texts.join(if lang == "en" { " " } else { "" }))
}

fn falls_in_window(seen: usize, utterances: &[Span]) -> bool {
    utterances
        .iter()
        .any(|u| u.start <= seen && seen + STILL_SPEAKING <= u.end)
}

/// Solves every candidate: a notice of the early detector while the current one is in speech.
fn candidates(
    models: &Path,
    lang: &str,
    audio: &[f32],
    pass: &Pass,
    utterances: &[Span],
) -> Result<Vec<Candidate>, Failure> {
    let mut found = Vec::new();
    for notice in pass.early.iter().filter(|n| n.in_voice) {
        let Some(voice) = enclosing(&pass.now, notice.seen) else {
            continue;
        };
        let text = solve(models, audio, voice, notice.seen, lang)?;
        found.push(Candidate {
            notice: *notice,
            complete: is_complete(lang, &text),
            inside_window: falls_in_window(notice.seen, utterances),
            text,
        });
    }
    Ok(found)
}

fn overlaps(a: Span, b: Span) -> bool {
    a.start < b.end && a.end > b.start
}

fn ms(samples: i64) -> i64 {
    samples / 16
}

fn signed(later: usize, earlier: usize) -> i64 {
    ms(later as i64 - earlier as i64)
}

/// For each utterance the last notice that overlaps it: end minus manifest end, and seen minus manifest end.
fn last_notice_diffs(notices: &[Notice], utterances: &[Span]) -> (Vec<i64>, Vec<i64>) {
    let (mut ends, mut seens) = (Vec::new(), Vec::new());
    for utterance in utterances {
        if let Some(last) = notices.iter().rfind(|n| overlaps(n.span, *utterance)) {
            ends.push(signed(last.span.end, utterance.end));
            seens.push(signed(last.seen, utterance.end));
        }
    }
    (ends, seens)
}

/// For each utterance the first complete candidate that overlaps it: seen minus manifest end.
fn first_complete_delays(found: &[Candidate], utterances: &[Span]) -> Vec<i64> {
    utterances
        .iter()
        .filter_map(|u| {
            found
                .iter()
                .find(|c| c.complete && overlaps(c.notice.span, *u))
                .map(|c| signed(c.notice.seen, u.end))
        })
        .collect()
}

fn percentile(values: &[i64], p: usize) -> Result<Option<i64>, Failure> {
    if values.is_empty() {
        return Ok(None);
    }
    nearest_rank(values, p)
        .map(Some)
        .map_err(|why| why.to_string())
}

fn p50_p95(values: &[i64]) -> Result<(Option<i64>, Option<i64>), Failure> {
    Ok((percentile(values, 50)?, percentile(values, 95)?))
}

/// What one language came to.
struct LangReport {
    lang: &'static str,
    utterances: usize,
    now_end: Vec<i64>,
    early_end: Vec<i64>,
    early_seen: Vec<i64>,
    first_complete: Vec<i64>,
    raw_notices: usize,
    candidates: usize,
    inside_raw: usize,
    complete: usize,
    complete_inside: usize,
    listing: Vec<String>,
}

fn language_report(
    models: &Path,
    lang: &'static str,
    audio: &[f32],
    pass: &Pass,
    utterances: &[Span],
) -> Result<LangReport, Failure> {
    let found = candidates(models, lang, audio, pass, utterances)?;
    let (now_end, _) = last_notice_diffs(&pass.now, utterances);
    let (early_end, early_seen) = last_notice_diffs(&pass.early, utterances);
    let listing = found
        .iter()
        .filter(|c| c.inside_window || c.complete)
        .map(|c| {
            format!(
                "CAND {lang} seen={} inside_window={} complete={} text={:?}",
                c.notice.seen, c.inside_window, c.complete, c.text
            )
        })
        .collect();
    Ok(LangReport {
        lang,
        utterances: utterances.len(),
        now_end,
        early_end,
        early_seen,
        first_complete: first_complete_delays(&found, utterances),
        raw_notices: pass.early.len(),
        candidates: found.len(),
        inside_raw: found.iter().filter(|c| c.inside_window).count(),
        complete: found.iter().filter(|c| c.complete).count(),
        complete_inside: found
            .iter()
            .filter(|c| c.complete && c.inside_window)
            .count(),
        listing,
    })
}

fn show(pair: (Option<i64>, Option<i64>)) -> String {
    let one = |value: Option<i64>| value.map_or_else(|| String::from("n/a"), |v| v.to_string());
    format!("p50 {} p95 {}", one(pair.0), one(pair.1))
}

fn report_lines(report: &LangReport) -> Result<Vec<String>, Failure> {
    let lang = report.lang;
    Ok(vec![
        format!(
            "R8V {lang} utterances={} early_notices={} candidates(in current speech)={} \
             inside_window_raw={} complete={} complete_inside_window={}",
            report.utterances,
            report.raw_notices,
            report.candidates,
            report.inside_raw,
            report.complete,
            report.complete_inside
        ),
        format!(
            "R8V {lang} end_diff_ms current {}",
            show(p50_p95(&report.now_end)?)
        ),
        format!(
            "R8V {lang} end_diff_ms early {}",
            show(p50_p95(&report.early_end)?)
        ),
        format!(
            "R8V {lang} detection_minus_end_ms early (last notice) {}",
            show(p50_p95(&report.early_seen)?)
        ),
        format!(
            "R8V {lang} detection_minus_end_ms first complete candidate (n {}) {}",
            report.first_complete.len(),
            show(p50_p95(&report.first_complete)?)
        ),
    ])
}

fn vad_line(name: &str, micros: &[i64]) -> Result<String, Failure> {
    let (p50, p95) = p50_p95(micros)?;
    let ms = |value: Option<i64>| {
        value.map_or_else(
            || String::from("n/a"),
            |v| format!("{:.3}", v as f64 / 1000.0),
        )
    };
    Ok(format!(
        "R8V vad_ms_per_window {name}: windows={} p50 {} p95 {} (limit p95 {:.1})",
        micros.len(),
        ms(p50),
        ms(p95),
        VAD_P95_LIMIT_US as f64 / 1000.0
    ))
}

/// The three stop conditions of step 1, evaluated on the pooled numbers.
fn verdict_lines(
    ja: &LangReport,
    en: &LangReport,
    both_us: &[i64],
) -> Result<Vec<String>, Failure> {
    let ja_now = percentile(&ja.now_end, 50)?;
    let ja_early = percentile(&ja.early_end, 50)?;
    let gain = ja_now.zip(ja_early).map(|(now, early)| now - early);
    let p95 = percentile(both_us, 95)?;
    Ok(vec![
        format!(
            "R8V STOP1 ja end diff p50 gain {gain:?} ms (need >= {REQUIRED_GAIN_MS}): {}",
            gain.is_some_and(|g| g >= REQUIRED_GAIN_MS)
        ),
        format!(
            "R8V STOP2 complete candidates inside an utterance window: ja {} en {} (need 0)",
            ja.complete_inside, en.complete_inside
        ),
        format!(
            "R8V STOP3 vad p95 per window (both detectors) {p95:?} us (limit {VAD_P95_LIMIT_US}): {}",
            p95.is_some_and(|us| us <= VAD_P95_LIMIT_US)
        ),
        format!(
            "R8V STOP4 early detector had candidates: ja {} en {} (need > 0)",
            ja.candidates, en.candidates
        ),
    ])
}

fn engine_for(models: &Path, lang: &str) -> Result<Engine, Failure> {
    let mut settings = Settings::new(models.to_path_buf());
    settings.policy = Policy::single(lang);
    settings.stages.speakers = false;
    Engine::open(settings).map_err(|why| why.to_string())
}

/// One language: the pass, the candidates solved with that language's engine.
fn one_language(
    models: &Path,
    testdata: &Path,
    lang: &'static str,
) -> Result<(LangReport, Pass), Failure> {
    let audio =
        read_wav(&testdata.join(format!("lat-{lang}.wav"))).map_err(|why| why.to_string())?;
    let manifest = testdata.join("manifests").join(format!("lat-{lang}.json"));
    let utterances = manifest_spans(&manifest)?;
    let pass = run_pass(models, &audio)?;
    let report = language_report(models, lang, &audio, &pass, &utterances)?;
    Ok((report, pass))
}

#[test]
#[ignore = "an offline probe: reads testdata/ and the models"]
fn r8v_early_end_probe() -> TestResult {
    let models = models_dir()?;
    let testdata = testdata_dir()?;
    let (ja, ja_pass) = one_language(&models, &testdata, "ja")?;
    let (en, en_pass) = one_language(&models, &testdata, "en")?;
    let now_us: Vec<i64> = [ja_pass.now_us.as_slice(), en_pass.now_us.as_slice()].concat();
    let both_us: Vec<i64> = [ja_pass.both_us.as_slice(), en_pass.both_us.as_slice()].concat();
    let mut lines = Vec::new();
    for report in [&ja, &en] {
        lines.extend(report_lines(report)?);
    }
    lines.push(vad_line("current detector alone", &now_us)?);
    lines.push(vad_line("current + early", &both_us)?);
    lines.extend(verdict_lines(&ja, &en, &both_us)?);
    lines.extend(ja.listing.iter().cloned());
    lines.extend(en.listing.iter().cloned());
    for line in lines {
        println!("{line}");
    }
    Ok(())
}

#[test]
fn a_japanese_sentence_is_complete_only_with_a_sentence_ending_and_no_connective() {
    assert!(ja_complete("今日は雨です。"));
    assert!(ja_complete("本当ですか？"));
    assert!(!ja_complete("雨が降っていますが。"));
    assert!(!ja_complete("今日は雨です"));
    assert!(!ja_complete("そして、"));
}

#[test]
fn an_english_sentence_is_complete_only_when_its_last_word_does_not_continue() {
    assert!(en_complete("It is raining."));
    assert!(en_complete("Is it raining?"));
    assert!(!en_complete("It is raining and."));
    assert!(!en_complete("It is raining"));
}

#[test]
fn a_language_outside_ja_and_en_is_never_complete() {
    assert!(!is_complete("zh", "今天下雨了。"));
}

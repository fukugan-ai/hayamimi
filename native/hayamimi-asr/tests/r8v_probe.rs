//! R8v step 1: an offline probe of the two-stage end-of-speech decision (3.17).
//!
//! The current detector A (threshold 0.5, min_silence 0.30 s) and an early one
//! B (threshold 0.65, min_silence 0.12 s, the same 0.25 s shortest speech) are
//! fed the same windows in the same order. A notice of B is a candidate only
//! while A is inside speech. The early path is replayed per segment of A:
//! each candidate is solved from the line (the voice start of A with its
//! pre-roll, then the last confirmed candidate) to the candidate sample, through
//! the engine, so the text has been through recognition, ITN and punctuation.
//! A complete text confirms the candidate and moves the line. What is left of
//! the segment after the last line is solved once more and counts as a final
//! when it holds text.
//!
//! Three runs, each its own test: the latency set (`lat-ja`, `lat-en`), and
//! the held-out sets (FLEURS ja joined with 1.5 s gaps, AMI turns). The
//! thresholds, the tables and the pass line below are fixed before the
//! held-out sets are run.
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
use support::{Failure, TestResult, models_dir, read, read_json, testdata_dir};
use unicode_normalization::UnicodeNormalization;

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
/// The limit of the p95 of the detectors' time per window, in microseconds.
const VAD_P95_LIMIT_US: i64 = 2000;
/// What is left of a segment after the last line is solved only when it is
/// at least this long (0.5 s at 16 kHz).
const REMAINDER_MIN: usize = 8000;
/// The p95 of `d` of the current main for ja, in ms (3.17, count 1).
const JA_D_P95_MAIN_MS: i64 = 1341;
/// The AMI turns taken per meeting, from the start of the meeting.
const AMI_TURNS_PER_MEETING: usize = 12;
/// An AMI turn is an utterance when it lasts this long (seconds)...
const AMI_MIN_TURN_S: f64 = 1.0;
/// ...and no other speaker's turn comes this close before or after it.
const AMI_MARGIN_S: f64 = 0.3;

/// Sentence endings that can close a Japanese sentence.
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

/// A stretch of the input in samples: an utterance or a detected segment.
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

/// A solved candidate.
struct Candidate {
    text: String,
    complete: bool,
    continues: bool,
}

/// The early path replayed over one segment of A.
struct Plan {
    seg: Notice,
    candidates: Vec<Candidate>,
    /// The sample of each confirmed (complete) candidate, in order.
    confirmed: Vec<usize>,
    /// The text left after the last line (empty when nothing is left).
    remainder_text: String,
}

impl Plan {
    fn has_remainder(&self) -> bool {
        !self.remainder_text.trim().is_empty()
    }

    /// The finals the early path makes out of this segment.
    fn finals(&self) -> usize {
        if self.confirmed.is_empty() {
            1
        } else {
            self.confirmed.len() + usize::from(self.has_remainder())
        }
    }

    /// The sample at which the last final of the segment is made.
    fn last_final_at(&self) -> usize {
        match self.confirmed.last() {
            Some(last) if !self.has_remainder() => *last,
            _ => self.seg.seen,
        }
    }

    /// The texts of the finals, in order: each confirmed candidate, then the
    /// remainder. Empty when the early path makes no split.
    fn final_texts(&self) -> Vec<&str> {
        if self.finals() < 2 {
            return Vec::new();
        }
        let mut texts: Vec<&str> = self
            .candidates
            .iter()
            .filter(|c| c.complete)
            .map(|c| c.text.as_str())
            .collect();
        if self.has_remainder() {
            texts.push(self.remainder_text.as_str());
        }
        texts
    }
}

/// What the plans say about one utterance.
struct UttStat {
    a_segments: Vec<Span>,
    finals_early: usize,
    first_complete_at: Option<usize>,
    a_seen: usize,
    last_final_at: usize,
}

impl UttStat {
    /// The first complete candidate, or the detection of A when none came.
    fn first_detection(&self) -> usize {
        match self.first_complete_at {
            Some(at) => at,
            None => self.a_seen,
        }
    }
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

fn overlaps(a: Span, b: Span) -> bool {
    a.start < b.end && a.end > b.start
}

/// Whether voice goes on after `candidate` inside the segment `seg` of A: B
/// reads the start of another stretch of voice after the candidate's end and
/// before the segment's end.
fn voice_continues(early: &[Notice], candidate: &Notice, seg: Span) -> bool {
    early
        .iter()
        .any(|n| n.span.start >= candidate.span.end && n.span.start < seg.end)
}

/// Rounds up to a multiple of 10.
fn ceil_ten(value: i64) -> i64 {
    (value + 9).div_euclid(10) * 10
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

/// The utterances of one AMI meeting: single-speaker turns of at least a
/// second with no other speaker's turn within the margin, the first
/// `per_meeting` of them.
fn ami_utterances(rttm: &str, per_meeting: usize) -> Result<Vec<Span>, Failure> {
    let mut turns: Vec<(f64, f64, String)> = Vec::new();
    for line in rttm.lines().filter(|l| l.starts_with("SPEAKER")) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let [_, _, _, start, duration, _, _, speaker, ..] = fields.as_slice() else {
            return Err(format!("an RTTM line has too few fields: {line}"));
        };
        let start: f64 = start.parse().map_err(|why| format!("{line}: {why}"))?;
        let duration: f64 = duration.parse().map_err(|why| format!("{line}: {why}"))?;
        turns.push((start, start + duration, (*speaker).to_owned()));
    }
    turns.sort_by(|a, b| a.0.total_cmp(&b.0));
    let rate = f64::from(SAMPLE_RATE);
    let alone = |turn: &(f64, f64, String)| {
        turns.iter().all(|other| {
            other.2 == turn.2
                || other.1 <= turn.0 - AMI_MARGIN_S
                || other.0 >= turn.1 + AMI_MARGIN_S
        })
    };
    Ok(turns
        .iter()
        .filter(|turn| turn.1 - turn.0 >= AMI_MIN_TURN_S && alone(turn))
        .take(per_meeting)
        .map(|turn| Span {
            start: (turn.0 * rate) as usize,
            end: (turn.1 * rate) as usize,
        })
        .collect())
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
fn run_pass(models: &Path, audio: &[f32], early_settings: VadSettings) -> Result<Pass, Failure> {
    let now = detector(models, NOW_SETTINGS, DEFAULT_THRESHOLD)?;
    let early = detector(models, early_settings, EARLY_THRESHOLD)?;
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

fn engine_for(models: &Path, lang: &str) -> Result<Engine, Failure> {
    let mut settings = Settings::new(models.to_path_buf());
    settings.policy = Policy::single(lang);
    settings.stages.speakers = false;
    Engine::open(settings).map_err(|why| why.to_string())
}

/// The text of `audio[from..to]` through a fresh engine (a reused engine's
/// detector is not used again after `finish`). A slice that yields several
/// finals is joined.
fn solve(
    models: &Path,
    audio: &[f32],
    from: usize,
    to: usize,
    lang: &str,
) -> Result<String, Failure> {
    let mut engine = engine_for(models, lang)?;
    let finals = engine
        .transcribe(&audio[from..to.min(audio.len())])
        .map_err(|why| why.to_string())?;
    let texts: Vec<&str> = finals.iter().map(|done| done.text.as_str()).collect();
    Ok(texts.join(if lang == "en" { " " } else { "" }))
}

fn preroll_samples() -> usize {
    (PREROLL_SECONDS * f64::from(SAMPLE_RATE)) as usize
}

/// The B notices that fall inside the segment `seg` of A while A was in voice.
fn candidates_in<'a>(pass: &'a Pass, seg: &Notice) -> Vec<&'a Notice> {
    pass.early
        .iter()
        .filter(|n| n.in_voice && seg.span.start <= n.seen && n.seen <= seg.seen)
        .collect()
}

/// Replays the early path over one segment of A.
fn plan_segment(
    models: &Path,
    lang: &str,
    audio: &[f32],
    pass: &Pass,
    seg: &Notice,
) -> Result<Plan, Failure> {
    let mut line: Option<usize> = None;
    let mut candidates = Vec::new();
    let mut confirmed = Vec::new();
    for cand in candidates_in(pass, seg) {
        let from = match line {
            Some(at) => at,
            None => seg.span.start.saturating_sub(preroll_samples()),
        };
        if cand.seen <= from {
            continue;
        }
        let text = solve(models, audio, from, cand.seen, lang)?;
        let complete = is_complete(lang, &text);
        candidates.push(Candidate {
            continues: voice_continues(&pass.early, cand, seg.span),
            complete,
            text,
        });
        if complete {
            line = Some(cand.seen);
            confirmed.push(cand.seen);
        }
    }
    let remainder_text = match line {
        Some(at) if seg.span.end >= at + REMAINDER_MIN => {
            solve(models, audio, at, seg.span.end, lang)?
        }
        _ => String::new(),
    };
    Ok(Plan {
        seg: *seg,
        candidates,
        confirmed,
        remainder_text,
    })
}

/// What the plans say about `utt`; `None` when no segment of A touches it.
fn utterance_stat(utt: Span, plans: &[Plan]) -> Option<UttStat> {
    let touching: Vec<&Plan> = plans
        .iter()
        .filter(|plan| overlaps(plan.seg.span, utt))
        .collect();
    let last = touching.last()?;
    Some(UttStat {
        a_segments: touching.iter().map(|plan| plan.seg.span).collect(),
        finals_early: touching.iter().map(|plan| plan.finals()).sum(),
        first_complete_at: touching
            .iter()
            .flat_map(|plan| plan.confirmed.iter().copied())
            .min(),
        a_seen: last.seg.seen,
        last_final_at: last.last_final_at(),
    })
}

fn ms(samples: i64) -> i64 {
    samples / 16
}

fn signed(later: usize, earlier: usize) -> i64 {
    ms(later as i64 - earlier as i64)
}

fn percentile(values: &[i64], p: usize) -> Result<Option<i64>, Failure> {
    if values.is_empty() {
        return Ok(None);
    }
    nearest_rank(values, p)
        .map(Some)
        .map_err(|why| why.to_string())
}

fn show(value: Option<i64>) -> String {
    value.map_or_else(|| String::from("n/a"), |v| v.to_string())
}

fn p50_p95(label: &str, values: &[i64]) -> Result<String, Failure> {
    Ok(format!(
        "{label} (n {}) p50 {} p95 {}",
        values.len(),
        show(percentile(values, 50)?),
        show(percentile(values, 95)?)
    ))
}

/// What one set came to.
struct SetReport {
    name: String,
    lang: String,
    utterances: usize,
    missed: usize,
    a_segments: usize,
    plans: Vec<Plan>,
    stats: Vec<(usize, UttStat)>,
    utt_spans: Vec<Span>,
    now_us: Vec<i64>,
    both_us: Vec<i64>,
    splits: Vec<SplitRecord>,
}

/// Where a split by B alone falls against the sentences of the reference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// The final before the line ends where a reference sentence ends.
    Boundary,
    /// The final after the line carries nothing of the rest of the reference.
    Tail,
    /// Neither.
    Mid,
}

/// The units of a text (characters for ja, lower-case words for en) and the
/// places a sentence ends in them, counted in units before the end mark.
struct Marked {
    units: Vec<String>,
    ends: Vec<usize>,
}

fn push_end(units: &[String], ends: &mut Vec<usize>) {
    if !units.is_empty() && ends.last() != Some(&units.len()) {
        ends.push(units.len());
    }
}

/// ja: NFKC, then letters and digits; 。 ? ! end a sentence. en: words
/// without punctuation, lower-case; a word ending in . ? ! ends a sentence.
fn marked(text: &str, lang: &str) -> Marked {
    let mut found = Marked {
        units: Vec::new(),
        ends: Vec::new(),
    };
    if lang == "ja" {
        for ch in text.nfkc() {
            if matches!(ch, '。' | '?' | '!') {
                push_end(&found.units, &mut found.ends);
            } else if ch.is_alphanumeric() {
                found.units.push(ch.to_string());
            }
        }
        return found;
    }
    for token in text.split_whitespace() {
        let word: String = token.chars().filter(|c| c.is_alphanumeric()).collect();
        if !word.is_empty() {
            found.units.push(word.to_lowercase());
        }
        if token.ends_with(['.', '?', '!']) {
            push_end(&found.units, &mut found.ends);
        }
    }
    found
}

/// How far into `reference` the units of `prefix` reach: the length of the
/// reference prefix with the smallest edit distance to `prefix` (the first
/// such length when several tie). `None` cannot happen: the row always has
/// the empty reference prefix in it.
fn align_end(prefix: &[String], reference: &[String]) -> Option<usize> {
    let width = reference.len() + 1;
    let mut previous: Vec<usize> = (0..width).collect();
    for (i, unit) in prefix.iter().enumerate() {
        let mut row = vec![i + 1; width];
        for j in 1..width {
            let swap = usize::from(*unit != reference[j - 1]);
            row[j] = (previous[j - 1] + swap)
                .min(previous[j] + 1)
                .min(row[j - 1] + 1);
        }
        previous = row;
    }
    previous
        .iter()
        .enumerate()
        .min_by_key(|(_, distance)| **distance)
        .map(|(reach, _)| reach)
}

/// Sorts one split. `prefix` is every final up to the line, `next` the final
/// after it. A tail wins over a boundary when both hold.
fn classify(prefix: &Marked, next: &Marked, reference: &Marked) -> Option<Kind> {
    let reach = align_end(&prefix.units, &reference.units)?;
    let rest = &reference.units[reach..];
    if next.units.is_empty() || !next.units.iter().any(|u| rest.contains(u)) {
        Some(Kind::Tail)
    } else if reference.ends.iter().any(|end| end.abs_diff(reach) <= 1) {
        Some(Kind::Boundary)
    } else {
        Some(Kind::Mid)
    }
}

/// One split by B alone with its two sortings.
struct SplitRecord {
    prefix: String,
    next: String,
    reference: Option<String>,
    a_final: String,
    by_reference: Option<Kind>,
    by_a: Kind,
}

impl SplitRecord {
    /// The sorting that counts: by the reference for ja, by the final of A for en.
    fn kind(&self, lang: &str) -> Kind {
        match (lang, self.by_reference) {
            ("ja", Some(kind)) => kind,
            _ => self.by_a,
        }
    }
}

/// The splits of one plan, sorted against the reference and the final of A.
fn plan_splits(
    models: &Path,
    lang: &str,
    audio: &[f32],
    plan: &Plan,
    reference: Option<&str>,
) -> Result<Vec<SplitRecord>, Failure> {
    let texts = plan.final_texts();
    if texts.len() < 2 {
        return Ok(Vec::new());
    }
    let from = plan.seg.span.start.saturating_sub(preroll_samples());
    let a_final = solve(models, audio, from, plan.seg.span.end, lang)?;
    let a_marked = marked(&a_final, lang);
    let joiner = if lang == "en" { " " } else { "" };
    let mut records = Vec::new();
    for line in 1..texts.len() {
        let prefix = texts[..line].join(joiner);
        let (prefix_m, next_m) = (marked(&prefix, lang), marked(texts[line], lang));
        let by_reference = match reference {
            Some(text) => Some(
                classify(&prefix_m, &next_m, &marked(text, lang))
                    .ok_or_else(|| String::from("an empty alignment"))?,
            ),
            None => None,
        };
        records.push(SplitRecord {
            by_reference,
            by_a: classify(&prefix_m, &next_m, &a_marked)
                .ok_or_else(|| String::from("an empty alignment"))?,
            reference: reference.map(str::to_owned),
            a_final: a_final.clone(),
            next: texts[line].to_owned(),
            prefix,
        });
    }
    Ok(records)
}

/// The sorted splits of every plan; the reference of a plan is that of the
/// first utterance it touches.
fn all_splits(
    models: &Path,
    lang: &str,
    audio: &[f32],
    plans: &[Plan],
    utterances: &[Span],
    references: &[Option<String>],
) -> Result<Vec<SplitRecord>, Failure> {
    let mut records = Vec::new();
    for plan in plans {
        let owner = utterances
            .iter()
            .position(|u| overlaps(plan.seg.span, *u))
            .and_then(|index| references.get(index))
            .and_then(Option::as_deref);
        records.extend(plan_splits(models, lang, audio, plan, owner)?);
    }
    Ok(records)
}

/// The segments of A that touch an utterance, replayed one by one.
fn plan_all(
    models: &Path,
    lang: &str,
    audio: &[f32],
    pass: &Pass,
    utterances: &[Span],
) -> Result<Vec<Plan>, Failure> {
    pass.now
        .iter()
        .filter(|seg| utterances.iter().any(|u| overlaps(seg.span, *u)))
        .map(|seg| plan_segment(models, lang, audio, pass, seg))
        .collect()
}

/// What one run is given: the set, its utterances with their reference
/// sentences (none for AMI), and the settings of B.
struct Input<'a> {
    name: &'a str,
    lang: &'a str,
    audio: &'a [f32],
    utterances: &'a [Span],
    references: &'a [Option<String>],
    early: VadSettings,
}

fn analyse(models: &Path, input: &Input) -> Result<SetReport, Failure> {
    let Input {
        name,
        lang,
        audio,
        utterances,
        references,
        early,
    } = *input;
    let pass = run_pass(models, audio, early)?;
    let plans = plan_all(models, lang, audio, &pass, utterances)?;
    let splits = all_splits(models, lang, audio, &plans, utterances, references)?;
    let stats: Vec<(usize, UttStat)> = utterances
        .iter()
        .enumerate()
        .filter_map(|(index, utt)| utterance_stat(*utt, &plans).map(|stat| (index, stat)))
        .collect();
    Ok(SetReport {
        name: name.to_owned(),
        lang: lang.to_owned(),
        utterances: utterances.len(),
        missed: utterances.len() - stats.len(),
        a_segments: plans.len(),
        plans,
        stats,
        utt_spans: utterances.to_vec(),
        now_us: pass.now_us,
        both_us: pass.both_us,
        splits,
    })
}

fn candidate_counts(report: &SetReport) -> [usize; 4] {
    let all = report.plans.iter().flat_map(|plan| plan.candidates.iter());
    let total = all.clone().count();
    let continuing = all.clone().filter(|c| c.continues).count();
    let complete = all.clone().filter(|c| c.complete).count();
    let complete_continuing = all.filter(|c| c.complete && c.continues).count();
    [total, continuing, complete, complete_continuing]
}

/// Splits as the pass line counts them: those A makes, and those B adds.
fn split_counts(report: &SetReport) -> (usize, usize) {
    let a_only: usize = report
        .stats
        .iter()
        .map(|(_, s)| s.a_segments.len() - 1)
        .sum();
    let b_added: usize = report.plans.iter().map(|plan| plan.finals() - 1).sum();
    (a_only, b_added)
}

/// The delays per utterance, in ms: first complete candidate (or A when none),
/// the last final, and A alone.
fn delays(report: &SetReport) -> (Vec<i64>, Vec<i64>, Vec<i64>) {
    let (mut first, mut last, mut now) = (Vec::new(), Vec::new(), Vec::new());
    for (index, stat) in &report.stats {
        let end = report.utt_spans[*index].end;
        first.push(signed(stat.first_detection(), end));
        last.push(signed(stat.last_final_at, end));
        now.push(signed(stat.a_seen, end));
    }
    (first, last, now)
}

fn vad_line(name: &str, micros: &[i64]) -> Result<String, Failure> {
    let value = |p| percentile(micros, p).map(|v| v.map(|us| us as f64 / 1000.0));
    let (p50, p95) = (value(50)?, value(95)?);
    let text = |v: Option<f64>| v.map_or_else(|| String::from("n/a"), |x| format!("{x:.3}"));
    Ok(format!(
        "vad_ms_per_window {name}: windows {} p50 {} p95 {}",
        micros.len(),
        text(p50),
        text(p95)
    ))
}

fn utterance_lines(report: &SetReport) -> Vec<String> {
    let rate = f64::from(SAMPLE_RATE);
    report
        .stats
        .iter()
        .filter(|(_, s)| s.a_segments.len() > 1 || s.finals_early > 1)
        .map(|(index, stat)| {
            let segments: Vec<String> = stat
                .a_segments
                .iter()
                .map(|s| format!("{:.2}-{:.2}", s.start as f64 / rate, s.end as f64 / rate))
                .collect();
            format!(
                "UTT {index} A_segments {} [{}] finals_early {}",
                stat.a_segments.len(),
                segments.join(" "),
                stat.finals_early
            )
        })
        .collect()
}

fn candidate_listing(report: &SetReport) -> Vec<String> {
    report
        .plans
        .iter()
        .flat_map(|plan| {
            plan.candidates.iter().filter(|c| c.complete).map(move |c| {
                format!(
                    "CAND A_seg_at {} continues {} text {:?}",
                    plan.seg.span.start, c.continues, c.text
                )
            })
        })
        .collect()
}

fn report_lines(report: &SetReport) -> Result<Vec<String>, Failure> {
    let [total, continuing, complete, complete_continuing] = candidate_counts(report);
    let (a_only, b_added) = split_counts(report);
    let (first, last, now) = delays(report);
    let tag = format!("R8V {}", report.name);
    let mut lines = vec![
        format!(
            "{tag} lang {} utterances {} without_A_segment {} A_segments {} candidates {total} \
             candidates_voice_continues {continuing} complete {complete} \
             complete_voice_continues {complete_continuing}",
            report.lang, report.utterances, report.missed, report.a_segments
        ),
        format!(
            "{tag} splits A_only {a_only} B_only {b_added} total_with_B {}",
            a_only + b_added
        ),
        format!("{tag} delay_ms {}", p50_p95("first complete or A", &first)?),
        format!("{tag} delay_ms {}", p50_p95("last final", &last)?),
        format!("{tag} delay_ms {}", p50_p95("A alone", &now)?),
        format!("{tag} {}", vad_line("A alone", &report.now_us)?),
        format!("{tag} {}", vad_line("A plus B", &report.both_us)?),
    ];
    lines.extend(
        utterance_lines(report)
            .into_iter()
            .map(|l| format!("{tag} {l}")),
    );
    lines.extend(
        candidate_listing(report)
            .into_iter()
            .map(|l| format!("{tag} {l}")),
    );
    Ok(lines)
}

/// The pass line: B adds no split of its own, and with B the splits are no
/// more than A alone makes.
fn pass_line(report: &SetReport) -> (bool, usize, usize) {
    let (a_only, b_added) = split_counts(report);
    (b_added == 0 && a_only + b_added <= a_only, a_only, b_added)
}

/// The ja limit of RP-66: P_B + (1341 - P_A), rounded up to 10 ms.
fn ja_limit(report: &SetReport) -> Result<Option<i64>, Failure> {
    let (first, _, now) = delays(report);
    let (Some(p_b), Some(p_a)) = (percentile(&first, 95)?, percentile(&now, 95)?) else {
        return Ok(None);
    };
    Ok(Some(ceil_ten(p_b + JA_D_P95_MAIN_MS - p_a)))
}

/// The counts of the three kinds and, for ja, how often the two sortings agree.
fn kind_lines(report: &SetReport) -> Vec<String> {
    let count = |kind: Kind| {
        report
            .splits
            .iter()
            .filter(|s| s.kind(&report.lang) == kind)
            .count()
    };
    let total = report.splits.len();
    let mut lines = vec![format!(
        "R8V {} KINDS splits {total} boundary {} tail {} mid {} (mid per split {}/{total}, mid per utterance {}/{})",
        report.name,
        count(Kind::Boundary),
        count(Kind::Tail),
        count(Kind::Mid),
        count(Kind::Mid),
        count(Kind::Mid),
        report.utterances
    )];
    let both: Vec<&SplitRecord> = report
        .splits
        .iter()
        .filter(|s| s.by_reference.is_some())
        .collect();
    if !both.is_empty() {
        let agree = both
            .iter()
            .filter(|s| s.by_reference == Some(s.by_a))
            .count();
        lines.push(format!(
            "R8V {} KINDS_AGREE by reference vs by A: agree {agree} differ {} of {}",
            report.name,
            both.len() - agree,
            both.len()
        ));
    }
    for s in &report.splits {
        lines.push(format!(
            "R8V {} SPLIT kind {:?} by_reference {:?} by_A {:?} prefix {:?} next {:?} reference {:?} a_final {:?}",
            report.name,
            s.kind(&report.lang),
            s.by_reference,
            s.by_a,
            s.prefix,
            s.next,
            s.reference,
            s.a_final
        ));
    }
    lines
}

fn print_set(report: &SetReport) -> TestResult {
    for line in report_lines(report)? {
        println!("{line}");
    }
    for line in kind_lines(report) {
        println!("{line}");
    }
    let (passed, a_only, b_added) = pass_line(report);
    println!(
        "R8V {} PASS_LINE {passed} (A_only {a_only}, B_only {b_added}); vad p95 limit {} us: {}",
        report.name,
        VAD_P95_LIMIT_US,
        percentile(&report.both_us, 95)?.is_some_and(|us| us <= VAD_P95_LIMIT_US)
    );
    if report.lang == "ja" {
        println!(
            "R8V {} JA_LIMIT_MS {} (stop when >= {JA_D_P95_MAIN_MS})",
            report.name,
            show(ja_limit(report)?)
        );
    }
    Ok(())
}

/// The `ref` of each manifest entry.
fn manifest_references(path: &Path) -> Result<Vec<Option<String>>, Failure> {
    let Some(entries) = read_json(path)?.as_array().cloned() else {
        return Err(format!("{} is not an array", path.display()));
    };
    entries
        .iter()
        .map(|entry| {
            let text = entry
                .get("ref")
                .and_then(Value::as_str)
                .ok_or_else(|| String::from("a manifest entry has no `ref`"))?;
            Ok(Some(text.to_owned()))
        })
        .collect()
}

/// A latency set under the name `name`, with B set to `early`.
fn lat_set(
    models: &Path,
    testdata: &Path,
    lang: &str,
    name: &str,
    early: VadSettings,
) -> Result<SetReport, Failure> {
    let audio =
        read_wav(&testdata.join(format!("lat-{lang}.wav"))).map_err(|why| why.to_string())?;
    let manifest = testdata.join("manifests").join(format!("lat-{lang}.json"));
    let utterances = manifest_spans(&manifest)?;
    let references = manifest_references(&manifest)?;
    analyse(
        models,
        &Input {
            name,
            lang,
            audio: &audio,
            utterances: &utterances,
            references: &references,
            early,
        },
    )
}

#[test]
#[ignore = "an offline probe: reads testdata/ and the models"]
fn r8v_latency_set_probe() -> TestResult {
    let (models, testdata) = (models_dir()?, testdata_dir()?);
    for lang in ["ja", "en"] {
        let name = format!("lat-{lang}");
        print_set(&lat_set(&models, &testdata, lang, &name, EARLY_SETTINGS)?)?;
    }
    Ok(())
}

/// B with `min_silence` 0.20 s, on the latency sets only; a record, not a judgement.
#[test]
#[ignore = "an offline probe: reads testdata/ and the models"]
fn r8v_latency_set_probe_min_silence_020() -> TestResult {
    let (models, testdata) = (models_dir()?, testdata_dir()?);
    let early = VadSettings {
        min_silence: 0.20,
        ..EARLY_SETTINGS
    };
    for lang in ["ja", "en"] {
        let name = format!("lat-{lang}-b020");
        print_set(&lat_set(&models, &testdata, lang, &name, early)?)?;
    }
    Ok(())
}

#[test]
#[ignore = "an offline probe: reads testdata/r8v_heldout/ and the models"]
fn r8v_heldout_ja_probe() -> TestResult {
    let (models, testdata) = (models_dir()?, testdata_dir()?);
    let dir = testdata.join("r8v_heldout");
    let audio = read_wav(&dir.join("ja.wav")).map_err(|why| why.to_string())?;
    let utterances = manifest_spans(&dir.join("ja.json"))?;
    let references = manifest_references(&dir.join("ja.json"))?;
    let input = Input {
        name: "heldout-ja",
        lang: "ja",
        audio: &audio,
        utterances: &utterances,
        references: &references,
        early: EARLY_SETTINGS,
    };
    print_set(&analyse(&models, &input)?)
}

#[test]
#[ignore = "an offline probe: reads testdata/eval_diar/ and the models"]
fn r8v_heldout_en_probe() -> TestResult {
    let (models, testdata) = (models_dir()?, testdata_dir()?);
    let dir = testdata.join("eval_diar");
    let meetings = read_json(&dir.join("manifest.json"))?;
    let Some(meetings) = meetings.as_array() else {
        return Err("eval_diar/manifest.json is not an array".into());
    };
    for meeting in meetings {
        let id = meeting
            .get("meeting")
            .and_then(Value::as_str)
            .ok_or("a meeting has no name")?;
        let audio = read_wav(&dir.join(format!("{id}.wav"))).map_err(|why| why.to_string())?;
        let rttm = read(&dir.join(format!("{id}.rttm")))?;
        let utterances = ami_utterances(&rttm, AMI_TURNS_PER_MEETING)?;
        let name = format!("heldout-en-{id}");
        let references = vec![None; utterances.len()];
        let input = Input {
            name: &name,
            lang: "en",
            audio: &audio,
            utterances: &utterances,
            references: &references,
            early: EARLY_SETTINGS,
        };
        print_set(&analyse(&models, &input)?)?;
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

#[test]
fn a_limit_is_rounded_up_to_ten_milliseconds() {
    assert_eq!(ceil_ten(1031), 1040);
    assert_eq!(ceil_ten(1040), 1040);
    assert_eq!(ceil_ten(1041), 1050);
}

#[test]
fn voice_continues_only_when_another_stretch_starts_inside_the_segment() {
    let notice = |start, end| Notice {
        span: Span { start, end },
        seen: end,
        in_voice: true,
    };
    let early = [notice(0, 1000), notice(2000, 3000)];
    let seg = Span {
        start: 0,
        end: 5000,
    };
    assert!(voice_continues(&early, &early[0], seg));
    assert!(!voice_continues(&early, &early[1], seg));
    let short = Span {
        start: 0,
        end: 1500,
    };
    assert!(!voice_continues(&early, &early[0], short));
}

#[test]
fn an_ami_turn_needs_a_second_and_a_quiet_neighbourhood() -> TestResult {
    let rttm = "SPEAKER m 1 1.000 2.000 <NA> <NA> A <NA> <NA>\n\
                SPEAKER m 1 3.100 2.000 <NA> <NA> B <NA> <NA>\n\
                SPEAKER m 1 10.000 0.500 <NA> <NA> A <NA> <NA>\n\
                SPEAKER m 1 20.000 3.000 <NA> <NA> A <NA> <NA>\n";
    let turns = ami_utterances(rttm, 10)?;
    let seconds: Vec<usize> = turns.iter().map(|t| t.start / 16000).collect();
    // The first two turns are closer than 0.3 s to each other, the third is short.
    assert_eq!(seconds, vec![20]);
    Ok(())
}

fn plan_of(confirmed: Vec<usize>, remainder: bool) -> Plan {
    Plan {
        seg: Notice {
            span: Span { start: 0, end: 100 },
            seen: 500,
            in_voice: false,
        },
        candidates: Vec::new(),
        confirmed,
        remainder_text: String::from(if remainder { "あ" } else { "" }),
    }
}

fn units(text: &str) -> Vec<String> {
    marked(text, "ja").units
}

#[test]
fn a_ja_text_is_split_into_characters_with_the_sentence_ends_between_them() {
    let m = marked("今日は雨です。明日は？", "ja");
    assert_eq!(m.units.len(), 9);
    assert_eq!(m.ends, vec![6, 9]);
}

#[test]
fn an_en_text_is_split_into_lower_case_words_with_the_sentence_ends_between_them() {
    let m = marked("It rains. Does it, today?", "en");
    assert_eq!(m.units, vec!["it", "rains", "does", "it", "today"]);
    assert_eq!(m.ends, vec![2, 5]);
}

#[test]
fn the_alignment_reaches_the_end_of_the_matching_reference_prefix() {
    let reference = units("今日は雨です明日は晴れです");
    assert_eq!(align_end(&units("今日は雨です"), &reference), Some(6));
    assert_eq!(align_end(&[], &reference), Some(0));
}

fn kind_of(prefix: &str, next: &str, reference: &str) -> Option<Kind> {
    classify(
        &marked(prefix, "ja"),
        &marked(next, "ja"),
        &marked(reference, "ja"),
    )
}

#[test]
fn a_cut_where_the_reference_sentence_ends_is_a_boundary() {
    let reference = "今日は雨です。明日は晴れです。";
    assert_eq!(
        kind_of("今日は雨です。", "明日は晴れです。", reference),
        Some(Kind::Boundary)
    );
}

#[test]
fn a_cut_inside_a_reference_sentence_is_mid() {
    let reference = "今日は雨ですが明日は晴れです。";
    assert_eq!(
        kind_of("今日は雨です", "が明日は晴れです。", reference),
        Some(Kind::Mid)
    );
}

#[test]
fn a_next_final_with_nothing_of_the_reference_is_a_tail_even_at_a_boundary() {
    let reference = "今日は雨です。";
    assert_eq!(
        kind_of("今日は雨です。", "ABC", reference),
        Some(Kind::Tail)
    );
    assert_eq!(kind_of("今日は雨です。", "", reference), Some(Kind::Tail));
}

#[test]
fn only_the_final_texts_of_a_split_plan_are_listed() {
    let mut plan = plan_of(vec![200], true);
    plan.candidates.push(Candidate {
        text: String::from("前。"),
        complete: true,
        continues: true,
    });
    assert_eq!(plan.final_texts(), vec!["前。", "あ"]);
    assert!(plan_of(vec![200], false).final_texts().is_empty());
}

#[test]
fn a_segment_without_a_complete_candidate_makes_one_final_at_the_detection() {
    let plan = plan_of(Vec::new(), false);
    assert_eq!(plan.finals(), 1);
    assert_eq!(plan.last_final_at(), 500);
}

#[test]
fn a_confirmed_candidate_with_text_left_makes_two_finals() {
    let open = plan_of(vec![200], true);
    assert_eq!(open.finals(), 2);
    assert_eq!(open.last_final_at(), 500);
    let closed = plan_of(vec![200], false);
    assert_eq!(closed.finals(), 1);
    assert_eq!(closed.last_final_at(), 200);
}

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
//! Two more runs judge the same candidates by the shadow rule of the endpoint
//! module (`shadow_early_hold`) instead of the live one: `lat-ja` with `lat-en`,
//! and the ja held-out set. Each prints the live-rule figures first (the
//! positive control, the same code path as the runs above), then the shadow
//! figures, then one `SHADOW_GATE` line that compares the two numbers the gate
//! passes on: the complete candidates with voice going on, and the splits by B
//! alone. A candidate the shadow rule adds is listed with its utterance (the
//! index in the manifest) and its place in the input.
//!
//! Two more probes replay the live rule with two more early detectors (RP-86).
//! C (threshold 0.80, end line 0.65, silence 0.12 s) notices the end of a
//! stretch of voice before B does. D (threshold 0.50, end line 0.35, silence
//! 0.12 s) finalises the sentence a notice kept, when that sentence does not end
//! on a connective or a particle and B has read no voice since. Each probe prints
//! the arm W (C and D off: the positive control, which must give the live
//! figures) and the arm X (C and D on), and writes one JSON row per set and arm
//! to `docs/results/r8v/`. The gate passes when X is within W on both figures.
//!
//!     cargo test --release -p hayamimi-asr --test r8v_probe -- --ignored --nocapture

mod support;

use std::path::{Path, PathBuf};
use std::time::Instant;

use hayamimi_asr::audio::{PREROLL_SECONDS, SAMPLE_RATE, WINDOW, read_wav};
use hayamimi_asr::endpoint::{Hold, Tail, is_complete, shadow_early_hold, tail_of};
use hayamimi_asr::route::Policy;
use hayamimi_asr::vad::{
    DEFAULT_THRESHOLD, Segment, VadSettings, silero_config_with, take_segments,
};
use hayamimi_asr::{Engine, Settings};
use hayamimi_eval::percentile::nearest_rank;
use hayamimi_gpu::session_lock::lock_for_session;
use serde_json::Value;
use sherpa_onnx::VoiceActivityDetector;
use support::{Failure, REPO, TARGET, TestResult, models_dir, read, read_json, testdata_dir};
use unicode_normalization::UnicodeNormalization;

/// The early detector: threshold 0.65 puts its end line at 0.50.
const EARLY_THRESHOLD: f32 = 0.65;
/// The detector C of RP-86: threshold 0.80 puts its end line at 0.65.
const C_THRESHOLD: f32 = 0.80;
/// The detector D of RP-86: the main threshold, so its end line is at 0.35.
const D_THRESHOLD: f32 = DEFAULT_THRESHOLD;
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
/// The limit of mid-sentence cuts in ja, in percent of the utterances (8 of 80 pass, 9 fail).
const MID_LIMIT_JA_PERCENT: usize = 10;
/// The limit of splits on which the two sortings differ, in percent...
const DIFFER_LIMIT_PERCENT: usize = 5;
/// ...and under this many splits no difference is allowed.
const SMALL_SPLIT_COUNT: usize = 20;
/// The AMI turns taken per meeting, from the start of the meeting.
const AMI_TURNS_PER_MEETING: usize = 12;
/// An AMI turn is an utterance when it lasts this long (seconds)...
const AMI_MIN_TURN_S: f64 = 1.0;
/// ...and no other speaker's turn comes this close before or after it.
const AMI_MARGIN_S: f64 = 0.3;

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

/// What the detectors did over one recording. `c` and `d` stay empty unless
/// the pass was made with the two extra detectors of RP-86.
struct Pass {
    now: Vec<Notice>,
    early: Vec<Notice>,
    c: Vec<Notice>,
    d: Vec<Notice>,
    now_us: Vec<i64>,
    both_us: Vec<i64>,
}

/// The rule that judges a solved candidate: `(lang, text)` is complete or not.
type Judge = fn(&str, &str) -> bool;

/// The shadow rule: complete unless `shadow_early_hold` keeps the sentence as
/// incomplete. The speech length is the largest, so the shortest-speech hold
/// never applies (the probe does not apply it under the live rule either).
fn shadow_complete(lang: &str, text: &str) -> bool {
    !matches!(
        shadow_early_hold(lang, text, usize::MAX),
        Some(Hold::Incomplete)
    )
}

/// A solved candidate.
struct Candidate {
    /// The sample the candidate was solved up to.
    at: usize,
    text: String,
    complete: bool,
    continues: bool,
    /// The window at which D finalised this candidate after it was held;
    /// `None` for every candidate of the arm W.
    d_at: Option<usize>,
}

/// What D answered at one of its notices while a candidate was held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DOutcome {
    /// The held sentence was finalised.
    Confirmed,
    /// The held sentence ends on a connective or a particle.
    EndsOnOpenForm,
    /// B read a voice start after the end of the held range.
    VoiceAfter,
}

/// One notice of D that found a candidate held.
struct DRecord {
    /// The window of D's notice.
    at: usize,
    /// The index of the held candidate in the plan.
    held: usize,
    outcome: DOutcome,
}

/// The early path replayed over one segment of A.
struct Plan {
    seg: Notice,
    candidates: Vec<Candidate>,
    /// The sample of each confirmed candidate (the line it moved), in order.
    confirmed: Vec<usize>,
    /// The window at which each confirmed candidate became a final, in order:
    /// the candidate's own notice, or D's notice for a held one.
    emitted: Vec<usize>,
    /// The text left after the last line (empty when nothing is left).
    remainder_text: String,
    /// What D answered while a candidate was held (empty in the arm W).
    d_records: Vec<DRecord>,
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
        match self.emitted.last() {
            Some(last) if !self.has_remainder() => *last,
            _ => self.seg.seen,
        }
    }

    /// The texts of the finals, in order: each confirmed candidate (a complete
    /// one, or a held one that D finalised), then the remainder. Empty when the
    /// early path makes no split.
    fn final_texts(&self) -> Vec<&str> {
        if self.finals() < 2 {
            return Vec::new();
        }
        let mut texts: Vec<&str> = self
            .candidates
            .iter()
            .filter(|c| c.complete || c.d_at.is_some())
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
fn ami_utterances(rttm: &str, skip: usize, take: usize) -> Result<Vec<Span>, Failure> {
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
        .skip(skip)
        .take(take)
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

/// The two detectors of RP-86 beyond A and B: C and D, fed the same windows.
struct Extra {
    c: VoiceActivityDetector,
    d: VoiceActivityDetector,
}

impl Extra {
    fn open(models: &Path, early_settings: VadSettings) -> Result<Extra, Failure> {
        Ok(Extra {
            c: detector(models, early_settings, C_THRESHOLD)?,
            d: detector(models, early_settings, D_THRESHOLD)?,
        })
    }

    fn feed(
        &self,
        window: &[f32],
        seen: usize,
        in_voice: bool,
        pass: &mut Pass,
    ) -> Result<(), Failure> {
        self.c.accept_waveform(window);
        collect(&self.c, seen, in_voice, &mut pass.c)?;
        self.d.accept_waveform(window);
        collect(&self.d, seen, in_voice, &mut pass.d)
    }

    fn flush(&self, seen: usize, pass: &mut Pass) -> Result<(), Failure> {
        self.c.flush();
        collect(&self.c, seen, false, &mut pass.c)?;
        self.d.flush();
        collect(&self.d, seen, false, &mut pass.d)
    }
}

/// Feeds the detectors the windows in order, timing the work of A and B on a
/// window. C and D are fed as well when `with_extra` is set.
fn run_pass(
    models: &Path,
    audio: &[f32],
    early_settings: VadSettings,
    with_extra: bool,
) -> Result<Pass, Failure> {
    let now = detector(models, NOW_SETTINGS, DEFAULT_THRESHOLD)?;
    let early = detector(models, early_settings, EARLY_THRESHOLD)?;
    let extra = if with_extra {
        Some(Extra::open(models, early_settings)?)
    } else {
        None
    };
    let mut pass = Pass {
        now: Vec::new(),
        early: Vec::new(),
        c: Vec::new(),
        d: Vec::new(),
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
        if let Some(extra) = &extra {
            extra.feed(window.as_slice(), seen, in_voice, &mut pass)?;
        }
    }
    now.flush();
    collect(&now, audio.len(), false, &mut pass.now)?;
    early.flush();
    collect(&early, audio.len(), false, &mut pass.early)?;
    if let Some(extra) = &extra {
        extra.flush(audio.len(), &mut pass)?;
    }
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

/// The notices among `notices` that fall inside the segment `seg` of A while A was in voice.
fn inside<'a>(notices: &'a [Notice], seg: &Notice) -> Vec<&'a Notice> {
    notices
        .iter()
        .filter(|n| n.in_voice && seg.span.start <= n.seen && n.seen <= seg.seen)
        .collect()
}

/// The B notices that fall inside the segment `seg` of A while A was in voice.
fn candidates_in<'a>(pass: &'a Pass, seg: &Notice) -> Vec<&'a Notice> {
    inside(&pass.early, seg)
}

/// Replays the early path over one segment of A.
fn plan_segment(
    models: &Path,
    lang: &str,
    audio: &[f32],
    pass: &Pass,
    seg: &Notice,
    judge: Judge,
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
        let complete = judge(lang, &text);
        candidates.push(Candidate {
            at: cand.seen,
            continues: voice_continues(&pass.early, cand, seg.span),
            complete,
            text,
            d_at: None,
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
        emitted: confirmed.clone(),
        confirmed,
        remainder_text,
        d_records: Vec::new(),
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
            .flat_map(|plan| plan.emitted.iter().copied())
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
    judge: Judge,
) -> Result<Vec<Plan>, Failure> {
    pass.now
        .iter()
        .filter(|seg| utterances.iter().any(|u| overlaps(seg.span, *u)))
        .map(|seg| plan_segment(models, lang, audio, pass, seg, judge))
        .collect()
}

/// What one run is given: the set, its utterances with their reference
/// sentences (none for AMI), the settings of B and the rule that judges a
/// candidate complete.
struct Input<'a> {
    name: &'a str,
    lang: &'a str,
    audio: &'a [f32],
    utterances: &'a [Span],
    references: &'a [Option<String>],
    early: VadSettings,
    judge: Judge,
}

fn analyse(models: &Path, input: &Input) -> Result<SetReport, Failure> {
    let Input {
        name,
        lang,
        audio,
        utterances,
        early,
        judge,
        ..
    } = *input;
    let pass = run_pass(models, audio, early, false)?;
    let plans = plan_all(models, lang, audio, &pass, utterances, judge)?;
    build_report(models, input, name, &pass, plans)
}

/// The report of `plans` replayed over `pass`, under the name `name`.
fn build_report(
    models: &Path,
    input: &Input,
    name: &str,
    pass: &Pass,
    plans: Vec<Plan>,
) -> Result<SetReport, Failure> {
    let Input {
        lang,
        audio,
        utterances,
        references,
        ..
    } = *input;
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
        now_us: pass.now_us.clone(),
        both_us: pass.both_us.clone(),
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

/// The ja limit of RP-66: P_B + (1341 - P_A), rounded up to 10 ms. P_B is the
/// p95 of the delay at the last final (3.17, count 10), P_A that of A alone.
fn ja_limit(report: &SetReport) -> Result<Option<i64>, Failure> {
    let (_, last, now) = delays(report);
    let (Some(p_b), Some(p_a)) = (percentile(&last, 95)?, percentile(&now, 95)?) else {
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

/// The limits the council fixed for step 1c, applied to a ja set: the mid-sentence
/// cuts (sorted by the reference) over the utterances, and the splits on which
/// the two sortings differ. For en the figures are recorded and judge nothing.
fn limit_lines(report: &SetReport) -> Vec<String> {
    let mid = report
        .splits
        .iter()
        .filter(|s| s.by_reference == Some(Kind::Mid))
        .count();
    let rated: Vec<&SplitRecord> = report
        .splits
        .iter()
        .filter(|s| s.by_reference.is_some())
        .collect();
    let differ = rated
        .iter()
        .filter(|s| s.by_reference != Some(s.by_a))
        .count();
    let mid_ok = mid * 100 <= MID_LIMIT_JA_PERCENT * report.utterances;
    let differ_ok = if rated.len() < SMALL_SPLIT_COUNT {
        differ == 0
    } else {
        differ * 100 <= DIFFER_LIMIT_PERCENT * rated.len()
    };
    vec![format!(
        "R8V {} LIMITS mid {mid} of {} utterances ({:.1} %, limit {MID_LIMIT_JA_PERCENT} %): {mid_ok}; \
         sortings differ {differ} of {} (limit {DIFFER_LIMIT_PERCENT} %, 0 under {SMALL_SPLIT_COUNT} splits): {differ_ok}",
        report.name,
        report.utterances,
        100.0 * mid as f64 / report.utterances as f64,
        rated.len()
    )]
}

fn print_set(report: &SetReport) -> TestResult {
    for line in report_lines(report)? {
        println!("{line}");
    }
    for line in kind_lines(report) {
        println!("{line}");
    }
    if report.lang == "ja" {
        for line in limit_lines(report) {
            println!("{line}");
        }
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

/// The two figures the offline gate compares: the complete candidates with
/// voice going on, and the splits by B alone.
fn gate_figures(report: &SetReport) -> (usize, usize) {
    let [_, _, _, complete_continuing] = candidate_counts(report);
    (complete_continuing, split_counts(report).1)
}

/// The complete candidates with voice going on that the live rule would not
/// judge complete: what the shadow rule adds inside an utterance. Each line
/// names the utterance (its index in the manifest), the place in the input and
/// the distance to the end of the utterance.
fn added_listing(report: &SetReport) -> Vec<String> {
    let rate = f64::from(SAMPLE_RATE);
    let mut lines = Vec::new();
    for plan in &report.plans {
        let Some(index) = report
            .utt_spans
            .iter()
            .position(|utt| overlaps(plan.seg.span, *utt))
        else {
            continue;
        };
        let end = report.utt_spans[index].end;
        let added = plan
            .candidates
            .iter()
            .filter(|c| c.complete && c.continues && !is_complete(&report.lang, &c.text));
        for cand in added {
            lines.push(format!(
                "utterance {index} at_s {:.2} ms_before_utterance_end {} text {:?}",
                cand.at as f64 / rate,
                signed(end, cand.at),
                cand.text
            ));
        }
    }
    lines
}

/// The `SHADOW_GATE` line, and the candidates the shadow rule adds. The gate
/// passes when both figures of the shadow rule are within those of the live rule.
fn print_gate(live: &SetReport, shadow: &SetReport) {
    let (live_voice, live_splits) = gate_figures(live);
    let (shadow_voice, shadow_splits) = gate_figures(shadow);
    let within = shadow_voice <= live_voice && shadow_splits <= live_splits;
    println!(
        "R8V {} SHADOW_GATE complete_voice_continues live {live_voice} shadow {shadow_voice}; \
         splits_by_B_alone live {live_splits} shadow {shadow_splits}; within the live figures: {within}",
        shadow.name
    );
    for line in added_listing(shadow) {
        println!("R8V {} ADDED {line}", shadow.name);
    }
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

/// A latency set under the name `name`, with B set to `early`, judged by `judge`.
fn lat_set(
    models: &Path,
    testdata: &Path,
    lang: &str,
    name: &str,
    early: VadSettings,
    judge: Judge,
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
            judge,
        },
    )
}

#[test]
#[ignore = "an offline probe: reads testdata/ and the models"]
fn r8v_latency_set_probe() -> TestResult {
    let (models, testdata) = (models_dir()?, testdata_dir()?);
    for lang in ["ja", "en"] {
        let name = format!("lat-{lang}");
        let report = lat_set(&models, &testdata, lang, &name, EARLY_SETTINGS, is_complete)?;
        print_set(&report)?;
    }
    Ok(())
}

/// The latency set of `lang` judged by the live rule, then by the shadow rule.
fn print_lat_both_rules(models: &Path, testdata: &Path, lang: &str) -> TestResult {
    let live_name = format!("lat-{lang}");
    let live = lat_set(
        models,
        testdata,
        lang,
        &live_name,
        EARLY_SETTINGS,
        is_complete,
    )?;
    print_set(&live)?;
    let shadow_name = format!("{live_name}-shadow");
    let shadow = lat_set(
        models,
        testdata,
        lang,
        &shadow_name,
        EARLY_SETTINGS,
        shadow_complete,
    )?;
    print_set(&shadow)?;
    print_gate(&live, &shadow);
    Ok(())
}

/// The latency sets under the live rule and under the shadow rule.
#[test]
#[ignore = "an offline probe: reads testdata/ and the models"]
fn r8v_latency_set_probe_shadow_rule() -> TestResult {
    let (models, testdata) = (models_dir()?, testdata_dir()?);
    for lang in ["ja", "en"] {
        print_lat_both_rules(&models, &testdata, lang)?;
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
        print_set(&lat_set(
            &models,
            &testdata,
            lang,
            &name,
            early,
            is_complete,
        )?)?;
    }
    Ok(())
}

/// The ja held-out set (`testdata/r8v_heldout/`) under the name `name`, judged by `judge`.
fn heldout_ja_set(
    models: &Path,
    testdata: &Path,
    name: &str,
    judge: Judge,
) -> Result<SetReport, Failure> {
    let dir = testdata.join("r8v_heldout");
    let audio = read_wav(&dir.join("ja.wav")).map_err(|why| why.to_string())?;
    let utterances = manifest_spans(&dir.join("ja.json"))?;
    let references = manifest_references(&dir.join("ja.json"))?;
    let input = Input {
        name,
        lang: "ja",
        audio: &audio,
        utterances: &utterances,
        references: &references,
        early: EARLY_SETTINGS,
        judge,
    };
    analyse(models, &input)
}

#[test]
#[ignore = "an offline probe: reads testdata/r8v_heldout/ and the models"]
fn r8v_heldout_ja_probe() -> TestResult {
    let (models, testdata) = (models_dir()?, testdata_dir()?);
    print_set(&heldout_ja_set(
        &models,
        &testdata,
        "heldout-ja",
        is_complete,
    )?)
}

/// The ja held-out set under the live rule and under the shadow rule.
#[test]
#[ignore = "an offline probe: reads testdata/r8v_heldout/ and the models"]
fn r8v_heldout_ja_probe_shadow_rule() -> TestResult {
    let (models, testdata) = (models_dir()?, testdata_dir()?);
    let live = heldout_ja_set(&models, &testdata, "heldout-ja", is_complete)?;
    print_set(&live)?;
    let shadow = heldout_ja_set(&models, &testdata, "heldout-ja-shadow", shadow_complete)?;
    print_set(&shadow)?;
    print_gate(&live, &shadow);
    Ok(())
}

/// ReazonSpeech test rows (80 of them, joined with 1.5 s gaps), the second held-out set.
#[test]
#[ignore = "an offline probe: reads testdata/r8v_heldout2/ and the models"]
fn r8v_heldout2_ja_probe() -> TestResult {
    let (models, testdata) = (models_dir()?, testdata_dir()?);
    let dir = testdata.join("r8v_heldout2");
    let audio = read_wav(&dir.join("ja.wav")).map_err(|why| why.to_string())?;
    let utterances = manifest_spans(&dir.join("ja.json"))?;
    let references = manifest_references(&dir.join("ja.json"))?;
    let inside = references
        .iter()
        .flatten()
        .filter(|text| text.trim_end_matches('。').contains('。'))
        .count();
    println!(
        "R8V heldout2-ja references: {} utterances, {inside} with 。 inside the sentence",
        references.len()
    );
    let input = Input {
        name: "heldout2-ja",
        lang: "ja",
        audio: &audio,
        utterances: &utterances,
        references: &references,
        early: EARLY_SETTINGS,
        judge: is_complete,
    };
    print_set(&analyse(&models, &input)?)
}

#[test]
#[ignore = "an offline probe: reads testdata/eval_diar/ and the models"]
fn r8v_heldout_en_probe() -> TestResult {
    ami_sets("heldout-en", 0, AMI_TURNS_PER_MEETING)
}

/// The AMI turns that step 1 did not use: every turn after the first 12 of each meeting.
#[test]
#[ignore = "an offline probe: reads testdata/eval_diar/ and the models"]
fn r8v_heldout2_en_probe() -> TestResult {
    ami_sets("heldout2-en", AMI_TURNS_PER_MEETING, usize::MAX)
}

/// Every meeting of `eval_diar` as its own set: the turns after `skip`, `take` of them.
fn ami_sets(prefix: &str, skip: usize, take: usize) -> TestResult {
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
        let utterances = ami_utterances(&rttm, skip, take)?;
        let name = format!("{prefix}-{id}");
        let references = vec![None; utterances.len()];
        let input = Input {
            name: &name,
            lang: "en",
            audio: &audio,
            utterances: &utterances,
            references: &references,
            early: EARLY_SETTINGS,
            judge: is_complete,
        };
        print_set(&analyse(&models, &input)?)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// RP-86: the early detectors C and D
// ---------------------------------------------------------------------------

/// The log of the latency sets, and of the ja held-out set, under `docs/results/r8v/`.
const LAT_LOG: &str = "extra-lat.log";
const HELDOUT_LOG: &str = "extra-heldout-ja.log";

/// The connective forms and particles of `endpoint.rs` (`JA_CONNECTIVE` and
/// `JA_PARTICLE`), copied because that module keeps them private: D does not
/// finalise a held sentence that ends on one of them.
const JA_OPEN_FORMS: [&str; 24] = [
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
    "を",
    "も",
    "は",
    "に",
    "の",
    "へ",
    "や",
    "など",
    "まで",
    "より",
];

/// Whether a held sentence ends on a connective form or a particle, judged on
/// what stands before its closing mark.
fn ends_on_open_form(text: &str) -> bool {
    let body = text
        .trim_end()
        .trim_end_matches(['。', '？', '！', '.', '?', '!'])
        .trim_end();
    JA_OPEN_FORMS.iter().any(|tail| body.ends_with(tail))
}

/// One thing the early path reads inside a segment of A.
#[derive(Clone, Copy)]
enum Event<'a> {
    /// A notice of B or C: solve from the line, judge, then confirm or hold.
    Early(&'a Notice),
    /// A notice of D: finalise the held sentence when the rule allows it.
    HeldSilence(&'a Notice),
}

impl Event<'_> {
    /// The window of the event, then 0 for an early notice and 1 for D's: in
    /// one window the early notices are read first, so a voice start that B
    /// reads in D's own window counts against D.
    fn order(&self) -> (usize, u8) {
        match self {
            Event::Early(notice) => (notice.seen, 0),
            Event::HeldSilence(notice) => (notice.seen, 1),
        }
    }
}

/// The notices of B and C inside one segment of A in the order the early path
/// reads them. When both detectors notice in one window, B's notice is read
/// alone (the notice is then `b`); a notice of C in a window of its own is read
/// as it is (the notice is `c`).
fn early_order<'a>(b: &[&'a Notice], c: &[&'a Notice]) -> Vec<&'a Notice> {
    let mut all: Vec<&Notice> = b.to_vec();
    all.extend(
        c.iter()
            .copied()
            .filter(|notice| b.iter().all(|other| other.seen != notice.seen)),
    );
    all.sort_by_key(|notice| notice.seen);
    all
}

/// The events of one segment of A in the order of the windows.
fn events<'a>(early: &[&'a Notice], held_silence: &[&'a Notice]) -> Vec<Event<'a>> {
    let mut all: Vec<Event> = early
        .iter()
        .map(|notice| Event::Early(notice))
        .chain(held_silence.iter().map(|notice| Event::HeldSilence(notice)))
        .collect();
    all.sort_by_key(Event::order);
    all
}

/// Whether B read a stretch of voice that starts at or after `range_end`, in a
/// window up to `d_seen`. A start at the end of the range itself counts: it
/// would otherwise finalise a sentence that voice went on after.
fn voice_read_since(early: &[Notice], range_end: usize, d_seen: usize) -> bool {
    early
        .iter()
        .any(|n| n.span.start >= range_end && n.seen <= d_seen)
}

/// What D does with the held candidate at its notice in the window `d_seen`.
fn d_outcome(held: &Candidate, early: &[Notice], d_seen: usize) -> DOutcome {
    if ends_on_open_form(&held.text) {
        DOutcome::EndsOnOpenForm
    } else if voice_read_since(early, held.at, d_seen) {
        DOutcome::VoiceAfter
    } else {
        DOutcome::Confirmed
    }
}

/// What a replay of one segment reads from: the models, the audio, the pass and the segment.
#[derive(Clone, Copy)]
struct Ctx<'a> {
    models: &'a Path,
    lang: &'a str,
    audio: &'a [f32],
    pass: &'a Pass,
    seg: &'a Notice,
    judge: Judge,
}

/// The early path of the arm X while it replays one segment of A.
#[derive(Default)]
struct Replay {
    /// The sample the segment is finalised up to.
    line: Option<usize>,
    /// The index of the candidate the early path keeps, when it keeps one.
    held: Option<usize>,
    candidates: Vec<Candidate>,
    confirmed: Vec<usize>,
    emitted: Vec<usize>,
    d_records: Vec<DRecord>,
}

impl Replay {
    /// A notice of B or C: solve from the line to the notice. A complete
    /// sentence is confirmed and moves the line; any other with text is kept.
    fn read_early(&mut self, ctx: Ctx<'_>, notice: &Notice) -> Result<(), Failure> {
        let from = match self.line {
            Some(at) => at,
            None => ctx.seg.span.start.saturating_sub(preroll_samples()),
        };
        if notice.seen <= from {
            return Ok(());
        }
        let text = solve(ctx.models, ctx.audio, from, notice.seen, ctx.lang)?;
        let complete = (ctx.judge)(ctx.lang, &text);
        let spoken = !text.trim().is_empty();
        self.candidates.push(Candidate {
            at: notice.seen,
            continues: voice_continues(&ctx.pass.early, notice, ctx.seg.span),
            complete,
            text,
            d_at: None,
        });
        if complete {
            self.line = Some(notice.seen);
            self.held = None;
            self.confirmed.push(notice.seen);
            self.emitted.push(notice.seen);
        } else if spoken {
            self.held = Some(self.candidates.len() - 1);
        }
        Ok(())
    }

    /// A notice of D: with a candidate held, finalise it when the rule allows
    /// it. The line moves to the end of the held range.
    fn read_held_silence(&mut self, ctx: Ctx<'_>, notice: &Notice) {
        let Some(index) = self.held else {
            return;
        };
        let Some(held) = self.candidates.get(index) else {
            return;
        };
        let outcome = d_outcome(held, &ctx.pass.early, notice.seen);
        let range_end = held.at;
        self.d_records.push(DRecord {
            at: notice.seen,
            held: index,
            outcome,
        });
        if outcome != DOutcome::Confirmed {
            return;
        }
        if let Some(done) = self.candidates.get_mut(index) {
            done.d_at = Some(notice.seen);
        }
        self.line = Some(range_end);
        self.held = None;
        self.confirmed.push(range_end);
        self.emitted.push(notice.seen);
    }

    /// The plan: what is left of the segment after the last line is solved once more.
    fn into_plan(self, ctx: Ctx<'_>) -> Result<Plan, Failure> {
        let remainder_text = match self.line {
            Some(at) if ctx.seg.span.end >= at + REMAINDER_MIN => {
                solve(ctx.models, ctx.audio, at, ctx.seg.span.end, ctx.lang)?
            }
            _ => String::new(),
        };
        Ok(Plan {
            seg: *ctx.seg,
            candidates: self.candidates,
            confirmed: self.confirmed,
            emitted: self.emitted,
            remainder_text,
            d_records: self.d_records,
        })
    }
}

/// Replays the early path of the arm X over one segment of A: the notices of
/// B and C, and, in ja, the notices of D.
fn plan_segment_extra(ctx: Ctx<'_>) -> Result<Plan, Failure> {
    let early = early_order(
        &candidates_in(ctx.pass, ctx.seg),
        &inside(&ctx.pass.c, ctx.seg),
    );
    let held_silence = if ctx.lang == "ja" {
        inside(&ctx.pass.d, ctx.seg)
    } else {
        Vec::new()
    };
    let mut replay = Replay::default();
    for event in events(&early, &held_silence) {
        match event {
            Event::Early(notice) => replay.read_early(ctx, notice)?,
            Event::HeldSilence(notice) => replay.read_held_silence(ctx, notice),
        }
    }
    replay.into_plan(ctx)
}

/// [`plan_all`] for the arm X.
fn plan_all_extra(
    models: &Path,
    lang: &str,
    audio: &[f32],
    pass: &Pass,
    utterances: &[Span],
    judge: Judge,
) -> Result<Vec<Plan>, Failure> {
    pass.now
        .iter()
        .filter(|seg| utterances.iter().any(|u| overlaps(seg.span, *u)))
        .map(|seg| {
            plan_segment_extra(Ctx {
                models,
                lang,
                audio,
                pass,
                seg,
                judge,
            })
        })
        .collect()
}

/// The arms W and X of one set, replayed over the same pass.
fn analyse_extra(models: &Path, input: &Input) -> Result<(SetReport, SetReport), Failure> {
    let Input {
        name,
        lang,
        audio,
        utterances,
        early,
        judge,
        ..
    } = *input;
    let pass = run_pass(models, audio, early, true)?;
    let w_plans = plan_all(models, lang, audio, &pass, utterances, judge)?;
    let x_plans = plan_all_extra(models, lang, audio, &pass, utterances, judge)?;
    let w = build_report(models, input, &format!("{name}-w"), &pass, w_plans)?;
    let x = build_report(models, input, &format!("{name}-x"), &pass, x_plans)?;
    Ok((w, x))
}

/// The replay of the arm X over a pass without C and D has to be the arm W:
/// the same figures and the same candidates.
fn machinery_matches_w(models: &Path, input: &Input, w: &SetReport) -> Result<bool, Failure> {
    let Input {
        lang,
        audio,
        utterances,
        early,
        judge,
        ..
    } = *input;
    let pass = run_pass(models, audio, early, false)?;
    let plans = plan_all_extra(models, lang, audio, &pass, utterances, judge)?;
    let off = build_report(models, input, "x-without-c-and-d", &pass, plans)?;
    Ok(arm_figures(&off) == arm_figures(w) && candidate_counts(&off) == candidate_counts(w))
}

/// The two figures the offline gate compares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Figures {
    /// The candidates finalised with voice going on after them: the complete
    /// ones (whichever notice finalised them) and the held ones D finalised.
    continuing_confirm: usize,
    /// The finals beyond the first that the early path makes out of one segment of A.
    b_only_split: usize,
}

/// The figures of the live rule: lat-ja and the ja held-out set, from `shadow.md`.
const LIVE_LAT_JA: Figures = Figures {
    continuing_confirm: 3,
    b_only_split: 4,
};
const LIVE_HELDOUT_JA: Figures = Figures {
    continuing_confirm: 9,
    b_only_split: 9,
};

/// The held candidates D finalised that voice went on after: the term D adds
/// to the first figure. A held candidate D finalised with no voice after it adds nothing.
fn held_confirmed_continuing(report: &SetReport) -> usize {
    report
        .plans
        .iter()
        .flat_map(|plan| plan.candidates.iter())
        .filter(|c| c.d_at.is_some() && c.continues)
        .count()
}

/// The figures of either arm: [`gate_figures`], and the term of D (0 in the arm W).
fn arm_figures(report: &SetReport) -> Figures {
    let (voice, splits) = gate_figures(report);
    Figures {
        continuing_confirm: voice + held_confirmed_continuing(report),
        b_only_split: splits,
    }
}

/// The row of one set and arm in a log.
fn log_row(set: &str, arm: &str, figures: Figures) -> String {
    format!(
        r#"{{"set":"{set}","arm":"{arm}","continuing_confirm":{},"b_only_split":{}}}"#,
        figures.continuing_confirm, figures.b_only_split
    )
}

/// A row of a log, read back.
struct Row {
    set: String,
    arm: String,
    figures: Figures,
}

fn row_text<'a>(value: &'a Value, key: &str, number: usize) -> Result<&'a str, Failure> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("line {number}: `{key}` is missing or not a string"))
}

fn row_count(value: &Value, key: &str, which: &str) -> Result<usize, Failure> {
    let number = value
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("{which}: `{key}` is missing or not an integer"))?;
    usize::try_from(number).map_err(|why| format!("{which}: `{key}`: {why}"))
}

fn parse_row(number: usize, line: &str) -> Result<Row, Failure> {
    let value: Value =
        serde_json::from_str(line).map_err(|why| format!("line {number} is not JSON: {why}"))?;
    let (set, arm) = (
        row_text(&value, "set", number)?,
        row_text(&value, "arm", number)?,
    );
    let which = format!("line {number} ({set} {arm})");
    Ok(Row {
        set: set.to_owned(),
        arm: arm.to_owned(),
        figures: Figures {
            continuing_confirm: row_count(&value, "continuing_confirm", &which)?,
            b_only_split: row_count(&value, "b_only_split", &which)?,
        },
    })
}

/// Every row of a log. A row that cannot be read, or that lacks a figure, is
/// an error: a missing figure is not read as 0.
fn parse_rows(log: &str) -> Result<Vec<Row>, Failure> {
    log.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| parse_row(index + 1, line))
        .collect()
}

fn row_of<'a>(rows: &'a [Row], set: &str, arm: &str) -> Result<&'a Row, Failure> {
    let mut found = rows.iter().filter(|r| r.set == set && r.arm == arm);
    let (Some(row), None) = (found.next(), found.next()) else {
        return Err(format!("the log has not exactly one row for {set} {arm}"));
    };
    Ok(row)
}

/// Why the offline gate does not pass.
#[derive(Debug, PartialEq, Eq)]
enum GateFault {
    /// A row is missing or unreadable, or a log cannot be read.
    Log(Failure),
    /// The arm W is not at the live figures, so X is not compared with it.
    Control(Failure),
    /// A figure of X is above that of W.
    Above(Failure),
}

impl GateFault {
    fn message(&self) -> &str {
        match self {
            GateFault::Log(why) | GateFault::Control(why) | GateFault::Above(why) => why,
        }
    }
}

/// X within W on both figures; the first figure above W is named.
fn within(set: &str, x: Figures, w: Figures) -> Result<(), GateFault> {
    let pairs = [
        (
            "continuing_confirm",
            x.continuing_confirm,
            w.continuing_confirm,
        ),
        ("b_only_split", x.b_only_split, w.b_only_split),
    ];
    for (label, x_value, w_value) in pairs {
        if x_value > w_value {
            return Err(GateFault::Above(format!(
                "{set}: {label} of X is {x_value}, above {w_value} of W"
            )));
        }
    }
    Ok(())
}

/// The positive control, then the comparison: W has to give the live figures
/// of the set before X is looked at.
fn check_set(rows: &[Row], set: &str, live: Figures) -> Result<(), GateFault> {
    let w = row_of(rows, set, "W").map_err(GateFault::Log)?.figures;
    let x = row_of(rows, set, "X").map_err(GateFault::Log)?.figures;
    if w != live {
        return Err(GateFault::Control(format!(
            "{set}: the arm W gives {w:?}, the live figures are {live:?}; X is not compared"
        )));
    }
    within(set, x, w)
}

fn check_log(log: &str, set: &str, live: Figures) -> Result<(), GateFault> {
    let rows = parse_rows(log).map_err(GateFault::Log)?;
    check_set(&rows, set, live)
}

/// The gate over the two logs: lat-ja first, then the ja held-out set.
fn gate_verdict(lat_log: &str, heldout_log: &str) -> Result<(), GateFault> {
    check_log(lat_log, "lat-ja", LIVE_LAT_JA)?;
    check_log(heldout_log, "heldout-ja", LIVE_HELDOUT_JA)
}

/// [`gate_verdict`] over the two logs of a directory. A log that is not there
/// stops the verdict before anything else in the directory is read.
fn gate_verdict_of_dir(dir: &Path) -> Result<(), GateFault> {
    let lat = read(&dir.join(LAT_LOG)).map_err(GateFault::Log)?;
    let heldout = read(&dir.join(HELDOUT_LOG)).map_err(GateFault::Log)?;
    gate_verdict(&lat, &heldout)
}

fn describe(outcome: &Result<(), GateFault>) -> String {
    match outcome {
        Ok(()) => String::from("passes: true"),
        Err(fault) => format!("passes: false ({})", fault.message()),
    }
}

/// `docs/results/r8v/` of the checkout the test was built from.
fn results_dir() -> PathBuf {
    Path::new(REPO).join("docs").join("results").join("r8v")
}

fn write_log(dir: &Path, name: &str, rows: &[String]) -> Result<(), Failure> {
    let path = dir.join(name);
    std::fs::write(&path, format!("{}\n", rows.join("\n")))
        .map_err(|why| format!("cannot write {}: {why}", path.display()))
}

/// Writes the rows of a set, reads them back, and judges the set. A log that
/// cannot be read back, or an arm W off the live figures, fails the probe; a
/// figure of X above W is printed as `passes: false` and the probe goes on.
fn finish_log(name: &str, set: &str, live: Figures, rows: &[String]) -> TestResult {
    let dir = results_dir();
    write_log(&dir, name, rows)?;
    let outcome = check_log(&read(&dir.join(name))?, set, live);
    println!("R8X {set} GATE {}", describe(&outcome));
    match outcome {
        Err(GateFault::Log(why) | GateFault::Control(why)) => Err(why.into()),
        Ok(()) | Err(GateFault::Above(_)) => Ok(()),
    }
}

/// The utterance (index in the manifest) that the segment of `plan` touches first.
fn utterance_of(report: &SetReport, plan: &Plan) -> Option<usize> {
    report
        .utt_spans
        .iter()
        .position(|utt| overlaps(plan.seg.span, *utt))
}

/// What D answered at each notice that found a candidate held: the utterance
/// (counted from 1, as the end-to-end records count), the outcome, and the
/// windows of the held range, of D and of A, in ms after the end of the utterance.
fn d_record_lines(set: &str, report: &SetReport) -> Vec<String> {
    let rate = f64::from(SAMPLE_RATE);
    let mut lines = Vec::new();
    for plan in &report.plans {
        let Some(index) = utterance_of(report, plan) else {
            continue;
        };
        let Some(utt) = report.utt_spans.get(index) else {
            continue;
        };
        for record in &plan.d_records {
            let Some(held) = plan.candidates.get(record.held) else {
                continue;
            };
            lines.push(format!(
                "R8X {set} D_RECORD utt {} index {index} outcome {:?} held_at_s {:.2} d_at_s {:.2} \
                 d_ms_after_utterance_end {} a_ms_after_utterance_end {} continues {} text {:?}",
                index + 1,
                record.outcome,
                held.at as f64 / rate,
                record.at as f64 / rate,
                signed(record.at, utt.end),
                signed(plan.seg.seen, utt.end),
                held.continues,
                held.text
            ));
        }
    }
    lines
}

/// Every candidate of the arm X, complete or not, with the window D finalised
/// it at when it did.
fn candidate_all_lines(set: &str, report: &SetReport) -> Vec<String> {
    let rate = f64::from(SAMPLE_RATE);
    let mut lines = Vec::new();
    for plan in &report.plans {
        let Some(index) = utterance_of(report, plan) else {
            continue;
        };
        for cand in &plan.candidates {
            lines.push(format!(
                "R8X {set} CAND_ALL utt {} at_s {:.2} complete {} continues {} d_at_s {:?} text {:?}",
                index + 1,
                cand.at as f64 / rate,
                cand.complete,
                cand.continues,
                cand.d_at.map(|at| at as f64 / rate),
                cand.text
            ));
        }
    }
    lines
}

/// Per utterance: the delay of A alone and of the last final in both arms, in
/// ms after the end of the utterance, and the finals of the early path.
fn utterance_rows(set: &str, w: &SetReport, x: &SetReport) -> Vec<String> {
    w.stats
        .iter()
        .filter_map(|(index, w_stat)| {
            let (_, x_stat) = x.stats.iter().find(|(other, _)| other == index)?;
            let end = w.utt_spans.get(*index)?.end;
            Some(format!(
                "R8X {set} UTT utt {} index {index} a_ms {} w_last_final_ms {} x_last_final_ms {} \
                 w_finals {} x_finals {}",
                index + 1,
                signed(w_stat.a_seen, end),
                signed(w_stat.last_final_at, end),
                signed(x_stat.last_final_at, end),
                w_stat.finals_early,
                x_stat.finals_early
            ))
        })
        .collect()
}

fn figures_line(set: &str, arm: &str, report: &SetReport) -> String {
    let figures = arm_figures(report);
    format!(
        "R8X {set} FIGURES arm {arm} continuing_confirm {} b_only_split {} \
         held_confirmed_continuing {} candidates {}",
        figures.continuing_confirm,
        figures.b_only_split,
        held_confirmed_continuing(report),
        report
            .plans
            .iter()
            .map(|p| p.candidates.len())
            .sum::<usize>()
    )
}

/// Everything the probe prints about the two arms of a set.
fn print_arms(set: &str, w: &SetReport, x: &SetReport) -> TestResult {
    print_set(w)?;
    print_set(x)?;
    println!("{}", figures_line(set, "W", w));
    println!("{}", figures_line(set, "X", x));
    let lines = d_record_lines(set, x)
        .into_iter()
        .chain(utterance_rows(set, w, x))
        .chain(candidate_all_lines(set, x));
    for line in lines {
        println!("{line}");
    }
    Ok(())
}

/// The two rows of a set in a log.
fn arm_rows(set: &str, w: &SetReport, x: &SetReport) -> Vec<String> {
    vec![
        log_row(set, "W", arm_figures(w)),
        log_row(set, "X", arm_figures(x)),
    ]
}

/// A set's audio, utterances and references, owned so that an [`Input`] can borrow them.
struct Loaded {
    audio: Vec<f32>,
    utterances: Vec<Span>,
    references: Vec<Option<String>>,
}

impl Loaded {
    /// The input of the live rule over this set.
    fn input<'a>(&'a self, name: &'a str, lang: &'a str) -> Input<'a> {
        Input {
            name,
            lang,
            audio: &self.audio,
            utterances: &self.utterances,
            references: &self.references,
            early: EARLY_SETTINGS,
            judge: is_complete,
        }
    }
}

fn load_lat(testdata: &Path, lang: &str) -> Result<Loaded, Failure> {
    let audio =
        read_wav(&testdata.join(format!("lat-{lang}.wav"))).map_err(|why| why.to_string())?;
    let manifest = testdata.join("manifests").join(format!("lat-{lang}.json"));
    Ok(Loaded {
        audio,
        utterances: manifest_spans(&manifest)?,
        references: manifest_references(&manifest)?,
    })
}

fn load_heldout_ja(testdata: &Path) -> Result<Loaded, Failure> {
    let dir = testdata.join("r8v_heldout");
    let audio = read_wav(&dir.join("ja.wav")).map_err(|why| why.to_string())?;
    Ok(Loaded {
        audio,
        utterances: manifest_spans(&dir.join("ja.json"))?,
        references: manifest_references(&dir.join("ja.json"))?,
    })
}

/// The replay of X with C and D off has to be W, or X is not measured by the code that measured W.
fn require_machinery_control(models: &Path, input: &Input, w: &SetReport) -> TestResult {
    let same = machinery_matches_w(models, input, w)?;
    println!("R8X {} MACHINERY_OFF_EQUALS_W {same}", input.name);
    if same {
        Ok(())
    } else {
        Err(String::from("the replay of X without C and D is not the arm W").into())
    }
}

/// The latency sets with C and D added (RP-86). The rows of lat-ja judge the
/// gate; the rows of lat-en are a record.
#[test]
#[ignore = "an offline probe: reads testdata/ and the models"]
fn r8v_latency_set_probe_extra_notices() -> TestResult {
    let (models, testdata) = (models_dir()?, testdata_dir()?);
    let mut rows = Vec::new();
    for lang in ["ja", "en"] {
        let set = format!("lat-{lang}");
        let loaded = load_lat(&testdata, lang)?;
        let input = loaded.input(&set, lang);
        let (w, x) = analyse_extra(&models, &input)?;
        print_arms(&set, &w, &x)?;
        rows.extend(arm_rows(&set, &w, &x));
        if lang == "ja" {
            require_machinery_control(&models, &input, &w)?;
        }
    }
    finish_log(LAT_LOG, "lat-ja", LIVE_LAT_JA, &rows)
}

/// The ja held-out set (`testdata/r8v_heldout/`) with C and D added (RP-86).
#[test]
#[ignore = "an offline probe: reads testdata/r8v_heldout/ and the models"]
fn r8v_heldout_ja_probe_extra_notices() -> TestResult {
    let (models, testdata) = (models_dir()?, testdata_dir()?);
    let loaded = load_heldout_ja(&testdata)?;
    let input = loaded.input("heldout-ja", "ja");
    let (w, x) = analyse_extra(&models, &input)?;
    print_arms("heldout-ja", &w, &x)?;
    finish_log(
        HELDOUT_LOG,
        "heldout-ja",
        LIVE_HELDOUT_JA,
        &arm_rows("heldout-ja", &w, &x),
    )
}

#[test]
fn a_japanese_sentence_is_complete_only_with_a_sentence_ending_and_no_connective() {
    assert!(is_complete("ja", "今日は雨です。"));
    assert!(is_complete("ja", "本当ですか？"));
    assert!(!is_complete("ja", "雨が降っていますが。"));
    assert!(!is_complete("ja", "今日は雨です"));
    assert!(!is_complete("ja", "そして、"));
}

#[test]
fn an_english_sentence_is_complete_only_when_its_last_word_does_not_continue() {
    assert!(is_complete("en", "It is raining."));
    assert!(is_complete("en", "Is it raining?"));
    assert!(!is_complete("en", "It is raining and."));
    assert!(!is_complete("en", "It is raining"));
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
    let turns = ami_utterances(rttm, 0, 10)?;
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
        emitted: confirmed.clone(),
        confirmed,
        remainder_text: String::from(if remainder { "あ" } else { "" }),
        d_records: Vec::new(),
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
        at: 200,
        text: String::from("前。"),
        complete: true,
        continues: true,
        d_at: None,
    });
    assert_eq!(plan.final_texts(), vec!["前。", "あ"]);
    assert!(plan_of(vec![200], false).final_texts().is_empty());
}

#[test]
fn the_shadow_rule_adds_a_noun_or_an_honorific_ending_under_a_closing_mark() {
    for text in ["アピヤはサモアの首都。", "近藤さん。"] {
        assert!(!is_complete("ja", text), "{text}");
        assert!(shadow_complete("ja", text), "{text}");
    }
}

#[test]
fn the_shadow_rule_keeps_every_sentence_the_live_rule_completes() {
    for text in ["今日は雨です。", "本当ですか？"] {
        assert!(is_complete("ja", text), "{text}");
        assert!(shadow_complete("ja", text), "{text}");
    }
}

#[test]
fn the_shadow_rule_still_holds_a_text_without_a_closing_mark_or_with_a_connective() {
    for text in [
        "アピヤはサモアの首都",
        "雨が降っていますが。",
        "そして、",
        "",
    ] {
        assert!(!shadow_complete("ja", text), "{text:?}");
    }
}

#[test]
fn the_shadow_rule_is_the_live_rule_outside_japanese() {
    for text in ["It is raining.", "It is raining and.", "It is raining", ""] {
        assert_eq!(
            shadow_complete("en", text),
            is_complete("en", text),
            "{text:?}"
        );
    }
    assert!(!shadow_complete("zh", "今天下雨了。"));
}

/// A report holding one segment of A over the utterance 0..48000 with the
/// given candidates (sample, text, voice goes on); every one is complete.
fn report_of(
    candidates: &[(usize, &str, bool)],
    confirmed: Vec<usize>,
    remainder: bool,
) -> SetReport {
    let span = Span {
        start: 0,
        end: 48000,
    };
    let mut plan = plan_of(confirmed, remainder);
    plan.seg.span = span;
    plan.candidates = candidates
        .iter()
        .map(|(at, text, continues)| Candidate {
            at: *at,
            text: (*text).to_owned(),
            complete: true,
            continues: *continues,
            d_at: None,
        })
        .collect();
    SetReport {
        name: String::from("t"),
        lang: String::from("ja"),
        utterances: 1,
        missed: 0,
        a_segments: 1,
        plans: vec![plan],
        stats: Vec::new(),
        utt_spans: vec![span],
        now_us: Vec::new(),
        both_us: Vec::new(),
        splits: Vec::new(),
    }
}

#[test]
fn only_a_candidate_the_live_rule_rejects_is_listed_as_added() {
    let report = report_of(
        &[
            (16000, "言ったろう。", true),
            (32000, "首都。", true),
            (40000, "近藤さん。", false),
        ],
        Vec::new(),
        false,
    );
    let lines = added_listing(&report);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines.first().is_some_and(|line| {
        line.contains("utterance 0 at_s 2.00 ms_before_utterance_end 1000")
            && line.contains("首都。")
    }));
}

#[test]
fn the_gate_figures_are_the_complete_candidates_with_voice_and_the_splits_by_b_alone() {
    let report = report_of(
        &[(16000, "言ったろう。", true), (32000, "首都。", false)],
        vec![16000],
        true,
    );
    assert_eq!(gate_figures(&report), (1, 1));
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

// ---------------------------------------------------------------------------
// RP-86: the offline gate over the logs, and the arm X
// ---------------------------------------------------------------------------

fn figures(continuing_confirm: usize, b_only_split: usize) -> Figures {
    Figures {
        continuing_confirm,
        b_only_split,
    }
}

/// The two rows of a set.
fn log_of(set: &str, w: Figures, x: Figures) -> String {
    [log_row(set, "W", w), log_row(set, "X", x)].join("\n")
}

/// The logs of the design's positive case: W at the live figures, X equal to W.
fn design_logs() -> (String, String) {
    (
        log_of("lat-ja", LIVE_LAT_JA, LIVE_LAT_JA),
        log_of("heldout-ja", LIVE_HELDOUT_JA, LIVE_HELDOUT_JA),
    )
}

/// The logs of the positive case with the X row of one set replaced.
fn logs_with_x(set: &str, x: Figures) -> (String, String) {
    match set {
        "lat-ja" => (
            log_of(set, LIVE_LAT_JA, x),
            log_of("heldout-ja", LIVE_HELDOUT_JA, LIVE_HELDOUT_JA),
        ),
        _ => (
            log_of("lat-ja", LIVE_LAT_JA, LIVE_LAT_JA),
            log_of(set, LIVE_HELDOUT_JA, x),
        ),
    }
}

/// The message of a gate that failed because X is above W.
fn above_message(outcome: Result<(), GateFault>) -> Result<String, Failure> {
    match outcome {
        Err(GateFault::Above(why)) => Ok(why),
        other => Err(format!("expected a figure of X above W, got {other:?}")),
    }
}

/// T-RP-86-001, the part that reads the offline logs: W at the live figures
/// and X equal to W pass. The checks of the end-to-end records, (1) to (6),
/// are those of `hayamimi-eval/tests/rp86.rs`.
#[test]
fn a_run_set_shaped_like_the_design_passes_every_check() {
    let (lat, heldout) = design_logs();
    assert_eq!(gate_verdict(&lat, &heldout), Ok(()));
}

/// T-RP-86-002.
#[test]
fn x_counts_below_w_in_the_offline_log_pass() {
    let lat = log_of("lat-ja", LIVE_LAT_JA, figures(2, 3));
    let heldout = log_of("heldout-ja", LIVE_HELDOUT_JA, figures(8, 8));
    assert_eq!(gate_verdict(&lat, &heldout), Ok(()));
}

/// T-RP-86-003.
#[test]
fn one_more_continuing_confirm_in_lat_ja_is_rejected() -> TestResult {
    let (lat, heldout) = logs_with_x("lat-ja", figures(4, 4));
    let said = above_message(gate_verdict(&lat, &heldout))?;
    assert!(
        said.contains("lat-ja") && said.contains("continuing_confirm"),
        "{said}"
    );
    assert!(
        said.contains('4') && said.contains('3') && !said.contains("b_only_split"),
        "{said}"
    );
    Ok(())
}

/// T-RP-86-004: lat-ja inside W does not save a ja held-out set above it.
#[test]
fn one_more_continuing_confirm_in_the_heldout_is_rejected() -> TestResult {
    let (lat, heldout) = logs_with_x("heldout-ja", figures(10, 9));
    let said = above_message(gate_verdict(&lat, &heldout))?;
    assert!(
        said.contains("heldout-ja") && said.contains("continuing_confirm"),
        "{said}"
    );
    assert!(said.contains("10") && said.contains('9'), "{said}");
    Ok(())
}

/// T-RP-86-005.
#[test]
fn one_more_b_only_split_in_lat_ja_is_rejected() -> TestResult {
    let (lat, heldout) = logs_with_x("lat-ja", figures(3, 5));
    let said = above_message(gate_verdict(&lat, &heldout))?;
    assert!(
        said.contains("lat-ja") && said.contains("b_only_split"),
        "{said}"
    );
    assert!(
        said.contains('5') && said.contains('4') && !said.contains("continuing_confirm"),
        "{said}"
    );
    Ok(())
}

/// T-RP-86-006.
#[test]
fn one_more_b_only_split_in_the_heldout_is_rejected() -> TestResult {
    let (lat, heldout) = logs_with_x("heldout-ja", figures(9, 10));
    let said = above_message(gate_verdict(&lat, &heldout))?;
    assert!(
        said.contains("heldout-ja") && said.contains("b_only_split"),
        "{said}"
    );
    assert!(said.contains("10") && said.contains('9'), "{said}");
    Ok(())
}

/// T-RP-86-007: the comparison of the two arms alone, with a W of 0 (the
/// positive control would not let such a W through a log).
#[test]
fn a_zero_baseline_allows_zero_and_rejects_one() -> TestResult {
    assert_eq!(within("lat-ja", figures(0, 0), figures(0, 0)), Ok(()));
    let said = above_message(within("lat-ja", figures(0, 1), figures(0, 0)))?;
    assert!(
        said.contains("b_only_split") && said.contains('1') && said.contains('0'),
        "{said}"
    );
    Ok(())
}

/// A directory under the workspace `target`, emptied for one test.
fn fixture_dir(name: &str) -> Result<PathBuf, Failure> {
    let dir = Path::new(TARGET).join(format!("r8x-5-{name}"));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|why| format!("{}: {why}", dir.display()))?;
    }
    std::fs::create_dir_all(&dir).map_err(|why| format!("{}: {why}", dir.display()))?;
    Ok(dir)
}

/// T-RP-86-008, the part that reads the offline logs: a log that is not there
/// fails by its own path, before a file of the end-to-end records is read.
#[test]
fn a_missing_offline_log_fails_before_any_e2e_file_is_read() -> TestResult {
    let dir = fixture_dir("missing-log")?;
    let (lat, heldout) = design_logs();
    std::fs::write(dir.join(LAT_LOG), lat)?;
    std::fs::write(dir.join(HELDOUT_LOG), heldout)?;
    std::fs::write(dir.join("x1.e2e1.jsonl"), "this is not JSON")?;
    assert_eq!(gate_verdict_of_dir(&dir), Ok(()));
    std::fs::remove_file(dir.join(LAT_LOG))?;
    let outcome = gate_verdict_of_dir(&dir);
    std::fs::remove_dir_all(&dir)?;
    match outcome {
        Err(GateFault::Log(said)) => {
            assert!(said.contains(LAT_LOG), "{said}");
            assert!(!said.contains("x1.e2e1"), "{said}");
            Ok(())
        }
        other => Err(format!("expected a missing log, got {other:?}").into()),
    }
}

/// T-RP-86-009: a figure that is not in a row is an error, not a 0 that passes.
#[test]
fn a_missing_counter_in_the_offline_log_is_rejected_not_read_as_zero() -> TestResult {
    let (lat, _) = design_logs();
    let heldout = format!(
        "{}\n{}",
        log_row("heldout-ja", "W", LIVE_HELDOUT_JA),
        r#"{"set":"heldout-ja","arm":"X","continuing_confirm":9}"#
    );
    match gate_verdict(&lat, &heldout) {
        Err(GateFault::Log(said)) => {
            assert!(
                said.contains("heldout-ja X") && said.contains("b_only_split"),
                "{said}"
            );
            Ok(())
        }
        other => Err(format!("expected an unreadable row, got {other:?}").into()),
    }
}

#[test]
fn a_row_that_is_not_json_names_its_line() {
    let first = parse_rows("not json").err();
    assert!(first.is_some_and(|text| text.contains("line 1 is not JSON")));
    let valid = log_row("lat-ja", "W", LIVE_LAT_JA);
    let second = parse_rows(&format!("{valid}\nnot json")).err();
    assert!(second.is_some_and(|text| text.contains("line 2 is not JSON")));
}

#[test]
fn a_row_is_written_in_the_form_the_design_shows() {
    assert_eq!(
        log_row("lat-ja", "W", figures(2, 3)),
        r#"{"set":"lat-ja","arm":"W","continuing_confirm":2,"b_only_split":3}"#
    );
}

#[test]
fn a_log_with_two_rows_for_one_arm_is_unreadable_as_a_verdict() {
    let log = format!(
        "{}\n{}",
        log_of("lat-ja", LIVE_LAT_JA, LIVE_LAT_JA),
        log_row("lat-ja", "X", LIVE_LAT_JA)
    );
    assert!(matches!(
        check_log(&log, "lat-ja", LIVE_LAT_JA),
        Err(GateFault::Log(_))
    ));
}

/// The rows of lat-en are a record: they judge nothing.
#[test]
fn rows_of_other_sets_do_not_judge_the_gate() {
    let (lat, heldout) = design_logs();
    let with_en = format!("{lat}\n{}", log_of("lat-en", figures(2, 2), figures(9, 9)));
    assert_eq!(gate_verdict(&with_en, &heldout), Ok(()));
}

/// The positive control: W at the live figures of its set.
#[test]
fn the_arm_w_at_the_live_figures_is_the_positive_control() {
    for (set, live) in [("lat-ja", LIVE_LAT_JA), ("heldout-ja", LIVE_HELDOUT_JA)] {
        assert_eq!(
            check_log(&log_of(set, live, live), set, live),
            Ok(()),
            "{set}"
        );
    }
}

/// The negative control of the positive control: a W off the live figures
/// fails before X is compared. In the first log X is within the odd W (it
/// would pass if compared); in the second X is above it (it would be named
/// `Above` if compared). Both are named for the control.
#[test]
fn an_arm_w_off_the_live_figures_fails_before_x_is_compared() -> TestResult {
    let within_odd_w = log_of("lat-ja", figures(3, 5), figures(0, 0));
    let above_odd_w = log_of("heldout-ja", figures(9, 10), figures(99, 99));
    for (log, set, live) in [
        (within_odd_w, "lat-ja", LIVE_LAT_JA),
        (above_odd_w, "heldout-ja", LIVE_HELDOUT_JA),
    ] {
        match check_log(&log, set, live) {
            Err(GateFault::Control(said)) => {
                assert!(
                    said.contains(set) && said.contains("X is not compared"),
                    "{said}"
                );
            }
            other => {
                return Err(format!("{set}: expected the control to fail, got {other:?}").into());
            }
        }
    }
    Ok(())
}

fn complete_candidate(at: usize, text: &str, continues: bool) -> Candidate {
    Candidate {
        at,
        text: text.to_owned(),
        complete: true,
        continues,
        d_at: None,
    }
}

/// A candidate the early path kept: not complete, and finalised by D at `d_at` when it was.
fn held_candidate(at: usize, text: &str, continues: bool, d_at: Option<usize>) -> Candidate {
    Candidate {
        at,
        text: text.to_owned(),
        complete: false,
        continues,
        d_at,
    }
}

/// A report with one segment of A holding `candidates`; the finals are the
/// confirmed ones, in the order given.
fn report_from_candidates(
    candidates: Vec<Candidate>,
    confirmed: Vec<usize>,
    remainder: bool,
) -> SetReport {
    let mut report = report_of(&[], confirmed, remainder);
    if let Some(plan) = report.plans.first_mut() {
        plan.candidates = candidates;
    }
    report
}

/// The term of D: a held candidate D finalised with voice going on after it
/// adds one to the first figure, one with no voice after it adds none, and a
/// held candidate D did not finalise adds none.
#[test]
fn a_held_candidate_d_finalises_adds_one_with_voice_going_on_and_none_without() {
    let complete = || complete_candidate(16000, "言ったろう。", true);
    let base = report_from_candidates(vec![complete()], vec![16000], false);
    let base_count = arm_figures(&base).continuing_confirm;
    assert_eq!(base_count, 1);
    let cases = [
        (true, Some(34000), 1),
        (false, Some(34000), 0),
        (true, None, 0),
    ];
    for (continues, d_at, added) in cases {
        let held = held_candidate(32000, "近藤さん。", continues, d_at);
        let confirmed = if d_at.is_some() {
            vec![16000, 32000]
        } else {
            vec![16000]
        };
        let report = report_from_candidates(vec![complete(), held], confirmed, false);
        assert_eq!(
            arm_figures(&report).continuing_confirm,
            base_count + added,
            "continues {continues}, finalised by D {d_at:?}"
        );
    }
}

#[test]
fn the_arm_w_has_no_term_of_d() {
    let report = report_from_candidates(
        vec![
            complete_candidate(16000, "言ったろう。", true),
            held_candidate(32000, "近藤さん。", true, None),
        ],
        vec![16000],
        false,
    );
    assert_eq!(held_confirmed_continuing(&report), 0);
    assert_eq!(arm_figures(&report), figures(1, 0));
}

fn notice_at(start: usize, end: usize, seen: usize) -> Notice {
    Notice {
        span: Span { start, end },
        seen,
        in_voice: true,
    }
}

/// The notices of C that come in a window of B's are not read; the others are.
#[test]
fn a_notice_of_c_in_the_window_of_a_notice_of_b_is_not_read_twice() {
    let (b1, b2) = (notice_at(1, 250, 300), notice_at(2, 550, 600));
    let (c1, c2) = (notice_at(10, 200, 300), notice_at(11, 400, 450));
    let order = early_order(&[&b1, &b2], &[&c1, &c2]);
    let starts: Vec<usize> = order.iter().map(|n| n.span.start).collect();
    assert_eq!(starts, vec![1, 11, 2]);
}

/// In one window the early notices come before D's; D's earlier window comes first.
#[test]
fn in_one_window_the_early_notice_is_read_before_the_notice_of_d() {
    let (early, same, before) = (
        notice_at(1, 250, 300),
        notice_at(5, 280, 300),
        notice_at(6, 100, 200),
    );
    let order = events(&[&early], &[&same, &before]);
    let seen: Vec<(usize, bool)> = order
        .iter()
        .map(|event| (event.order().0, matches!(event, Event::HeldSilence(_))))
        .collect();
    assert_eq!(seen, vec![(200, true), (300, false), (300, true)]);
}

#[test]
fn d_finalises_a_held_sentence_that_does_not_end_on_an_open_form_with_no_voice_since() {
    let held = held_candidate(1000, "近藤さん。", true, None);
    assert_eq!(d_outcome(&held, &[], 2000), DOutcome::Confirmed);
}

#[test]
fn d_keeps_a_held_sentence_that_ends_on_a_connective_or_a_particle() {
    for text in [
        "雨が降っていますが。",
        "今日は雨ですから",
        "本音とは。",
        "夏の",
    ] {
        let held = held_candidate(1000, text, false, None);
        assert_eq!(
            d_outcome(&held, &[], 2000),
            DOutcome::EndsOnOpenForm,
            "{text}"
        );
    }
}

/// The boundary of "B read a voice start": the end of the held range itself counts.
#[test]
fn a_voice_start_of_b_at_the_end_of_the_held_range_counts_against_d() {
    let held = held_candidate(1000, "近藤さん。", true, None);
    let at_end = [notice_at(1000, 1600, 1700)];
    assert_eq!(d_outcome(&held, &at_end, 2000), DOutcome::VoiceAfter);
    let before = [notice_at(999, 1600, 1700)];
    assert_eq!(d_outcome(&held, &before, 2000), DOutcome::Confirmed);
}

/// A voice start B reads in D's own window counts; one B reads after it does not.
#[test]
fn only_a_voice_start_read_up_to_the_window_of_d_counts() {
    let held = held_candidate(1000, "近藤さん。", true, None);
    let same_window = [notice_at(1500, 1900, 2000)];
    assert_eq!(d_outcome(&held, &same_window, 2000), DOutcome::VoiceAfter);
    let later = [notice_at(1500, 1900, 2001)];
    assert_eq!(d_outcome(&held, &later, 2000), DOutcome::Confirmed);
}

/// The copy of the two tables is the tables of `endpoint.rs`: each form is a
/// particle or connective there, and the endings that are not are not here.
#[test]
fn the_copied_open_forms_are_those_the_endpoint_module_calls_a_particle() {
    for tail in JA_OPEN_FORMS {
        let text = format!("雨{tail}。");
        assert!(ends_on_open_form(&text), "{text}");
        assert_eq!(tail_of("ja", &text), Tail::Particle, "{text}");
    }
    for text in [
        "雨です。",
        "近藤さん。",
        "本当ですか？",
        "独占配信。",
        "言ったろう。",
    ] {
        assert!(!ends_on_open_form(text), "{text}");
        assert_ne!(tail_of("ja", text), Tail::Particle, "{text}");
    }
}

#[test]
fn a_held_candidate_d_finalised_is_one_of_the_final_texts() {
    let mut plan = plan_of(vec![200, 400], true);
    plan.candidates = vec![
        complete_candidate(200, "前。", true),
        held_candidate(400, "近藤さん。", false, Some(450)),
        held_candidate(500, "keep", false, None),
    ];
    assert_eq!(plan.final_texts(), vec!["前。", "近藤さん。", "あ"]);
}

/// The final of a held candidate is made at D's window, not at the end of its range.
#[test]
fn the_final_of_a_held_candidate_is_made_at_the_window_of_d() {
    let mut plan = plan_of(vec![200], false);
    plan.emitted = vec![350];
    assert_eq!(plan.last_final_at(), 350);
}

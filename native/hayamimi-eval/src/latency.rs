//! Per-utterance delays from the events a run received.
//!
//! Four moments are read off the tool's own clock for each utterance: the
//! last audio sample sent (`t_end`), the `final` event that closes the
//! utterance (`t_final`), the translation of that final (`t_tr`), and the
//! first `partial` (`t_partial`). `L = t_tr - t_end`, `L_asr = t_final -
//! t_end`, `L_tr = t_tr - t_final`. All times are microseconds since the run
//! started; a percentile is reported in whole milliseconds, halves up.
//!
//! Only a translation the subtitle would show counts as `t_tr`: its `status`
//! is `ok` (or absent, as in the Python server's events) and its `text` is not
//! empty. A `final` that carries an `id` and `t1_s` (the Rust server) is tied
//! to its utterance by `t1_s` and to its translation by `id` and the sha256 of
//! its text; a `final` without them is tied by arrival time and position.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cer::{Cer, cer};
use crate::error::EvalError;
use crate::percentile::{micros_to_ms, nearest_rank};
use crate::sha::sha256_hex;

/// What an event was, as far as the delay measurement cares.
#[derive(Clone, Debug, PartialEq)]
pub enum EventKind {
    Partial,
    Final {
        text: String,
        latency_ms: Option<f64>,
        /// The line id the Rust server gives; the Python server has none.
        id: Option<String>,
        /// `t1_s` (the utterance end on the stream) in whole microseconds.
        t1_us: Option<i64>,
    },
    Translation {
        text: String,
        id: Option<String>,
        status: Option<String>,
        /// sha256 of the source text the translation was made from.
        src_sha256: Option<String>,
    },
    /// A translation of the text of an utterance in progress (`of` is
    /// `partial`). It never closes an utterance, so it is not paired with a
    /// `final`.
    PartialTranslation,
    /// The reply of `/ingest` to the config. `stream_us` is where the server's
    /// stream clock stood when it replied (`stream_s`, in whole microseconds);
    /// a server that does not say has none.
    Ready {
        stream_us: Option<i64>,
    },
    /// The server's stream clock began (`session_start`).
    SessionStart,
    Other,
}

/// An event received from the server, stamped by the tool's clock.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub t_us: i64,
    pub kind: EventKind,
}

/// The text field of an event, which must be a string.
fn text_of(value: &Value) -> Result<String, EvalError> {
    match value.get("text") {
        Some(Value::String(text)) => Ok(text.clone()),
        _ => Err(EvalError::Protocol(String::from(
            "an event of this type has no string `text`",
        ))),
    }
}

/// A string field of an event, when it has one.
fn string_field(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

/// Whole microseconds of a seconds figure, rounded to the nearest; `None`
/// for a figure that is not finite or does not fit.
fn seconds_to_us(seconds: f64) -> Option<i64> {
    let us = (seconds * 1_000_000.0).round();
    (us.abs() < 9.0e18).then_some(us as i64)
}

/// Reads one event from the JSON text the server sent.
pub fn parse_event(t_us: i64, json_text: &str) -> Result<Event, EvalError> {
    let value: Value = serde_json::from_str(json_text)
        .map_err(|e| EvalError::Protocol(format!("an event is not JSON: {e}")))?;
    let kind = match value.get("type").and_then(Value::as_str) {
        Some("partial") => EventKind::Partial,
        Some("final") => EventKind::Final {
            text: text_of(&value)?,
            latency_ms: value.get("latency_ms").and_then(Value::as_f64),
            id: string_field(&value, "id"),
            t1_us: value
                .get("t1_s")
                .and_then(Value::as_f64)
                .and_then(seconds_to_us),
        },
        Some("translation") if value.get("of").and_then(Value::as_str) == Some("partial") => {
            EventKind::PartialTranslation
        }
        Some("translation") => EventKind::Translation {
            text: text_of(&value)?,
            id: string_field(&value, "id"),
            status: string_field(&value, "status"),
            src_sha256: string_field(&value, "src_sha256"),
        },
        Some("ready") => EventKind::Ready {
            stream_us: value
                .get("stream_s")
                .and_then(Value::as_f64)
                .and_then(seconds_to_us),
        },
        Some("session_start") => EventKind::SessionStart,
        _ => EventKind::Other,
    };
    Ok(Event { t_us, kind })
}

impl EventKind {
    /// Whether this is a translation the subtitle shows: `status` `ok` (or
    /// none, as in the Python server's events) and a `text` that is not empty.
    fn is_shown_translation(&self) -> bool {
        matches!(
            self,
            EventKind::Translation { text, status, .. }
                if !text.is_empty() && status.as_deref().is_none_or(|s| s == "ok")
        )
    }
}

/// The span of one utterance in one repetition, on the tool's clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    pub repeat: u32,
    /// 1-based position in the evaluation set.
    pub utt: u32,
    pub t_start_us: i64,
    pub t_end_us: i64,
}

/// One line of a run's result file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    pub repeat: u32,
    pub utt: u32,
    pub t_start_us: i64,
    pub t_end_us: i64,
    pub t_partial_us: Option<i64>,
    /// The first translation of the text in progress in this utterance. Files
    /// written before the relay made such translations do not have it.
    pub t_partial_tr_us: Option<i64>,
    pub t_final_us: Option<i64>,
    pub t_tr_us: Option<i64>,
    /// How many `final` events fell in this utterance.
    pub finals: u32,
    /// Those finals joined in order.
    pub text: String,
    pub translation: Option<String>,
    pub final_latency_ms: Option<f64>,
    pub cer_dist: usize,
    pub cer_denom: usize,
}

impl Sample {
    /// Speech end to translation.
    pub fn l_us(&self) -> Option<i64> {
        self.t_tr_us.map(|t| t - self.t_end_us)
    }

    /// Speech end to the closing final.
    pub fn l_asr_us(&self) -> Option<i64> {
        self.t_final_us.map(|t| t - self.t_end_us)
    }

    /// Final to translation. Negative when a partial's translation matched.
    pub fn l_tr_us(&self) -> Option<i64> {
        match (self.t_final_us, self.t_tr_us) {
            (Some(f), Some(t)) => Some(t - f),
            _ => None,
        }
    }
}

/// For each `final` in `events` (by index), the translation it is paired
/// with: the first shown `translation` (see [`EventKind::is_shown_translation`])
/// after it and before the next `final`.
///
/// The Python server's translation event carries no id, so position is the
/// only link. A translation that arrives before any final, or after another
/// final has already been received, belongs to no one.
pub fn positional_translations(events: &[Event]) -> Vec<(usize, Option<usize>)> {
    let mut pairs: Vec<(usize, Option<usize>)> = Vec::new();
    for (index, event) in events.iter().enumerate() {
        if matches!(event.kind, EventKind::Final { .. }) {
            pairs.push((index, None));
        } else if event.kind.is_shown_translation()
            && let Some(last) = pairs.last_mut()
            && last.1.is_none()
        {
            last.1 = Some(index);
        }
    }
    pairs
}

/// The first shown translation made from the text of the `final` at
/// `closing`: same `id`, `src_sha256` equal to the sha256 of the final's text.
fn translation_by_id(events: &[Event], closing: usize) -> Option<usize> {
    let EventKind::Final {
        id: Some(id), text, ..
    } = &events[closing].kind
    else {
        return None;
    };
    let source = sha256_hex(text.as_bytes());
    events
        .iter()
        .enumerate()
        .filter(|(_, event)| match &event.kind {
            EventKind::Translation {
                id: Some(of),
                src_sha256: Some(sha),
                ..
            } => of == id && *sha == source && event.kind.is_shown_translation(),
            _ => false,
        })
        .min_by_key(|(_, event)| event.t_us)
        .map(|(index, _)| index)
}

/// The translation that closes the utterance whose closing `final` is at
/// `closing`: by `id` when the final has one, else by position.
fn translation_of(
    events: &[Event],
    pairs: &[(usize, Option<usize>)],
    closing: usize,
) -> Option<usize> {
    if matches!(&events[closing].kind, EventKind::Final { id: Some(_), .. }) {
        return translation_by_id(events, closing);
    }
    pairs
        .iter()
        .find(|(final_index, _)| *final_index == closing)
        .and_then(|(_, translation)| *translation)
}

/// The index of the last window that started at or before `t_us`.
fn window_at(windows: &[Window], t_us: i64) -> Option<usize> {
    windows
        .partition_point(|w| w.t_start_us <= t_us)
        .checked_sub(1)
}

/// The moment a `final` is placed by: the utterance end on the stream when it
/// carries one, else the time it arrived.
fn placement_us(event: &Event) -> i64 {
    match event.kind {
        EventKind::Final {
            t1_us: Some(t1), ..
        } => t1,
        _ => event.t_us,
    }
}

/// For each window, the indexes of the `final` events placed in it, in
/// placement order.
fn finals_by_window(windows: &[Window], events: &[Event]) -> Vec<Vec<usize>> {
    let mut by_window: Vec<Vec<usize>> = vec![Vec::new(); windows.len()];
    for (index, event) in events.iter().enumerate() {
        if matches!(event.kind, EventKind::Final { .. })
            && let Some(k) = window_at(windows, placement_us(event))
        {
            by_window[k].push(index);
        }
    }
    for finals in &mut by_window {
        finals.sort_by_key(|&i| placement_us(&events[i]));
    }
    by_window
}

/// Builds one sample per window from the run's events.
///
/// A `final` belongs to the latest utterance that had started at its
/// placement moment (see [`placement_us`]), so the early finals of an
/// utterance the server cut at its maximum length still count toward that
/// utterance's text, and a `final` that arrived late still belongs to the
/// utterance it ended. The last of an utterance's finals closes it; if that
/// one arrived before the utterance's last sample was sent, the utterance has
/// no closing final.
pub fn build_samples(
    windows: &[Window],
    references: &[String],
    events: &[Event],
) -> Result<Vec<Sample>, EvalError> {
    if windows
        .windows(2)
        .any(|w| w[0].t_start_us > w[1].t_start_us)
    {
        return Err(EvalError::Invalid(String::from(
            "utterance windows are not in time order",
        )));
    }
    let pairs = positional_translations(events);
    let placed = finals_by_window(windows, events);
    let mut samples = Vec::with_capacity(windows.len());
    for (k, window) in windows.iter().enumerate() {
        let Some(reference) = usize::try_from(window.utt)
            .ok()
            .and_then(|u| u.checked_sub(1))
            .and_then(|u| references.get(u))
        else {
            return Err(EvalError::Invalid(format!(
                "utterance {} has no reference text",
                window.utt
            )));
        };
        let finals = &placed[k];
        let text: String = finals
            .iter()
            .filter_map(|&i| match &events[i].kind {
                EventKind::Final { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        let closing = finals
            .last()
            .copied()
            .filter(|&i| events[i].t_us >= window.t_end_us);
        let t_final_us = closing.map(|i| events[i].t_us);
        let final_latency_ms = closing.and_then(|i| match &events[i].kind {
            EventKind::Final { latency_ms, .. } => *latency_ms,
            _ => None,
        });
        let paired = closing.and_then(|i| translation_of(events, &pairs, i));
        let (t_tr_us, translation) = match paired.map(|i| &events[i]) {
            Some(Event {
                t_us,
                kind: EventKind::Translation { text, .. },
            }) => (Some(*t_us), Some(text.clone())),
            _ => (None, None),
        };
        let upper = t_final_us.or_else(|| windows.get(k + 1).map(|next| next.t_start_us));
        let first_in_window = |wanted: fn(&EventKind) -> bool| {
            events
                .iter()
                .filter(|e| wanted(&e.kind) && e.t_us >= window.t_start_us)
                .map(|e| e.t_us)
                .next()
                .filter(|t| upper.is_none_or(|u| *t < u))
        };
        let t_partial_us = first_in_window(|kind| matches!(kind, EventKind::Partial));
        let t_partial_tr_us = first_in_window(|kind| matches!(kind, EventKind::PartialTranslation));
        let Cer { dist, denom } = cer(reference, &text);
        samples.push(Sample {
            repeat: window.repeat,
            utt: window.utt,
            t_start_us: window.t_start_us,
            t_end_us: window.t_end_us,
            t_partial_us,
            t_partial_tr_us,
            t_final_us,
            t_tr_us,
            finals: u32::try_from(finals.len())
                .map_err(|_| EvalError::Invalid(String::from("too many finals")))?,
            text,
            translation,
            final_latency_ms,
            cer_dist: dist,
            cer_denom: denom,
        });
    }
    Ok(samples)
}

/// A percentile pair over some samples, in whole milliseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Stat {
    pub n: usize,
    pub p50_ms: i64,
    pub p95_ms: i64,
}

/// The p50 and p95 of microsecond values; `None` for no values.
pub fn stat_of(values_us: &[i64]) -> Result<Option<Stat>, EvalError> {
    if values_us.is_empty() {
        return Ok(None);
    }
    Ok(Some(Stat {
        n: values_us.len(),
        p50_ms: micros_to_ms(nearest_rank(values_us, 50)?),
        p95_ms: micros_to_ms(nearest_rank(values_us, 95)?),
    }))
}

/// The numbers a run is reported by.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Summary {
    pub samples: usize,
    pub with_final: usize,
    pub with_translation: usize,
    /// Samples with no translation: those that got none and those with no
    /// closing final. `samples = with_translation + untranslated`.
    pub untranslated: usize,
    pub finals_total: usize,
    /// Utterances with no closing final whose next utterance holds two or more
    /// finals: the signature of a final that arrived after its window closed.
    /// Counted only for finals placed by arrival time (the Python server's);
    /// `None` for a run whose finals carry `t1_s`, which is not counted at all.
    pub late_final_suspects: Option<usize>,
    pub l: Option<Stat>,
    pub l_asr: Option<Stat>,
    pub l_tr: Option<Stat>,
    pub l_tr_negative: usize,
    pub final_latency_mean_ms: Option<f64>,
    /// Median over the repetitions of each utterance, by utterance number.
    pub per_utt_median_l_ms: Vec<Option<i64>>,
    pub per_utt_median_l_asr_ms: Vec<Option<i64>>,
    pub cer_dist: usize,
    pub cer_denom: usize,
    pub cer_micros: u64,
}

/// The median of each utterance's values, utterance numbers 1..=`count`.
fn per_utterance_medians(
    samples: &[Sample],
    count: u32,
    pick: fn(&Sample) -> Option<i64>,
) -> Result<Vec<Option<i64>>, EvalError> {
    (1..=count)
        .map(|utt| {
            let values: Vec<i64> = samples
                .iter()
                .filter(|s| s.utt == utt)
                .filter_map(pick)
                .collect();
            if values.is_empty() {
                Ok(None)
            } else {
                Ok(Some(micros_to_ms(nearest_rank(&values, 50)?)))
            }
        })
        .collect()
}

/// Summarises a run's samples.
pub fn summarize(samples: &[Sample]) -> Result<Summary, EvalError> {
    let l: Vec<i64> = samples.iter().filter_map(Sample::l_us).collect();
    let l_asr: Vec<i64> = samples.iter().filter_map(Sample::l_asr_us).collect();
    let l_tr: Vec<i64> = samples.iter().filter_map(Sample::l_tr_us).collect();
    let latencies: Vec<f64> = samples.iter().filter_map(|s| s.final_latency_ms).collect();
    let final_latency_mean_ms = if latencies.is_empty() {
        None
    } else {
        Some(latencies.iter().sum::<f64>() / latencies.len() as f64)
    };
    let total = samples
        .iter()
        .map(|s| Cer {
            dist: s.cer_dist,
            denom: s.cer_denom,
        })
        .fold(Cer::default(), Cer::plus);
    // The highest utterance number; no samples means no utterances.
    let count = samples.iter().map(|s| s.utt).fold(0, u32::max);
    Ok(Summary {
        samples: samples.len(),
        with_final: l_asr.len(),
        with_translation: l.len(),
        untranslated: samples.len() - l.len(),
        finals_total: samples.iter().map(|s| s.finals as usize).sum(),
        late_final_suspects: None,
        l: stat_of(&l)?,
        l_asr: stat_of(&l_asr)?,
        l_tr: stat_of(&l_tr)?,
        l_tr_negative: l_tr.iter().filter(|v| **v < 0).count(),
        final_latency_mean_ms,
        per_utt_median_l_ms: per_utterance_medians(samples, count, Sample::l_us)?,
        per_utt_median_l_asr_ms: per_utterance_medians(samples, count, Sample::l_asr_us)?,
        cer_dist: total.dist,
        cer_denom: total.denom,
        cer_micros: total.micros(),
    })
}

/// How many utterances had no closing final while the next one holds two or
/// more: a `final` placed by arrival time that came after its window.
pub fn late_final_suspects(samples: &[Sample]) -> usize {
    samples
        .windows(2)
        .filter(|pair| pair[0].finals == 0 && pair[1].finals >= 2)
        .count()
}

/// Whether any `final` in the run was placed by arrival time because it
/// carries no `t1_s`.
fn has_finals_placed_by_arrival(events: &[Event]) -> bool {
    events
        .iter()
        .any(|event| matches!(event.kind, EventKind::Final { t1_us: None, .. }))
}

/// Summarises a run: the samples' numbers, and the late-final suspects when
/// the run's finals were placed by arrival time.
pub fn summarize_run(samples: &[Sample], events: &[Event]) -> Result<Summary, EvalError> {
    let mut summary = summarize(samples)?;
    if has_finals_placed_by_arrival(events) {
        summary.late_final_suspects = Some(late_final_suspects(samples));
    }
    Ok(summary)
}

/// How far the tool's clock is ahead of the server's stream clock, in
/// microseconds: a `final` carries `t1_s` on the stream clock, a window is on
/// the tool's clock, and the one is put on the other by adding this.
///
/// `ready` says where the stream clock stood when the connection was made
/// (`stream_s`), so the offset is the arrival of that frame minus the position;
/// the stream clock keeps going from one client to the next, and only the
/// position tells a second connection from the first. A server that does not
/// say is read by its `session_start`, the moment the clock began. The first
/// `final` is never the origin: which utterance it belongs to is what the
/// offset is needed to decide.
pub fn stream_offset_us(events: &[Event]) -> Option<i64> {
    let by_ready = events.iter().find_map(|event| match event.kind {
        EventKind::Ready {
            stream_us: Some(position),
        } => Some(event.t_us - position),
        _ => None,
    });
    by_ready.or_else(|| {
        events
            .iter()
            .find(|event| event.kind == EventKind::SessionStart)
            .map(|event| event.t_us)
    })
}

/// Puts the `t1_s` of every `final` on the tool's clock (see
/// [`stream_offset_us`]). A run with no frame about the stream clock has the
/// two clocks equal and is left as it is.
pub fn place_on_tool_clock(events: &mut [Event]) {
    let Some(offset) = stream_offset_us(events) else {
        return;
    };
    for event in events {
        if let EventKind::Final {
            t1_us: Some(t1), ..
        } = &mut event.kind
        {
            *t1 += offset;
        }
    }
}

/// The windows and the events of a run from the two files it left: the
/// samples file (`<run>.jsonl`, one [`Sample`] a line) and the events file
/// (`<run>.events.jsonl`, `{"t_us", "event"}` a line).
pub fn read_record(
    samples_text: &str,
    events_text: &str,
) -> Result<(Vec<Window>, Vec<Event>), EvalError> {
    Ok((windows_of_text(samples_text)?, events_of_text(events_text)?))
}

/// The lines of a file that are not blank, with their 1-based numbers.
fn non_blank(text: &str) -> impl Iterator<Item = (usize, &str)> {
    text.lines()
        .enumerate()
        .map(|(at, line)| (at + 1, line))
        .filter(|(_, line)| !line.trim().is_empty())
}

/// The span of each utterance of a samples file.
fn windows_of_text(text: &str) -> Result<Vec<Window>, EvalError> {
    non_blank(text)
        .map(|(number, line)| {
            let sample: Sample = serde_json::from_str(line)
                .map_err(|error| EvalError::Invalid(format!("samples line {number}: {error}")))?;
            Ok(Window {
                repeat: sample.repeat,
                utt: sample.utt,
                t_start_us: sample.t_start_us,
                t_end_us: sample.t_end_us,
            })
        })
        .collect()
}

/// An event as the events file holds it: when it arrived and its JSON.
type RawEvent = (i64, Value);

/// The lines of an events file, unread.
fn raw_events(text: &str) -> Result<Vec<RawEvent>, EvalError> {
    non_blank(text)
        .map(|(number, line)| {
            let bad = |why: &str| EvalError::Invalid(format!("events line {number}: {why}"));
            let value: Value =
                serde_json::from_str(line).map_err(|error| bad(&error.to_string()))?;
            let t_us = value
                .get("t_us")
                .and_then(Value::as_i64)
                .ok_or_else(|| bad("no integer `t_us`"))?;
            let event = value.get("event").ok_or_else(|| bad("no `event`"))?;
            Ok((t_us, event.clone()))
        })
        .collect()
}

/// The events of an events file, read the way the run reads what it hears.
pub fn events_of_text(text: &str) -> Result<Vec<Event>, EvalError> {
    parse_raw(&raw_events(text)?)
}

fn parse_raw(raw: &[RawEvent]) -> Result<Vec<Event>, EvalError> {
    raw.iter()
        .map(|(t_us, body)| parse_event(*t_us, &body.to_string()))
        .collect()
}

/// The samples of a run built again from the two files it left, the way
/// `stream` builds them: the finals are put on the tool's clock first.
/// `references` has one text for each utterance number.
pub fn samples_of_record(
    samples_text: &str,
    events_text: &str,
    references: &[String],
) -> Result<Vec<Sample>, EvalError> {
    let (windows, mut events) = read_record(samples_text, events_text)?;
    place_on_tool_clock(&mut events);
    build_samples(&windows, references, &events)
}

/// The stages of the wait from the end of speech to the translation. Each is
/// the difference of two moments on the tool's clock (design 10.13.3):
/// `d = t_close - t_end` with `t_close = t_final - L_dec`, `L_dec` the
/// `latency_ms` of the closing `final`, `L_tr = t_tr - t_final` and
/// `L_e = t_settled - t_tr`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// The wait for the end: `d`.
    Wait,
    /// The decode: `L_dec`.
    Decode,
    /// The draft translation: `L_tr`.
    Draft,
    /// The settled translation: `L_e`. Recorded, with no line.
    Settle,
}

impl Stage {
    /// The name the record gives the stage.
    pub fn name(self) -> &'static str {
        match self {
            Stage::Wait => "d",
            Stage::Decode => "L_dec",
            Stage::Draft => "L_tr",
            Stage::Settle => "L_e",
        }
    }

    /// The line of the stage in milliseconds, `(p50, p95)`; none for a stage that has none.
    pub fn budget_ms(self) -> Option<(i64, i64)> {
        match self {
            Stage::Wait => Some((300, 600)),
            Stage::Decode => Some((270, 480)),
            Stage::Draft => Some((100, 300)),
            Stage::Settle => None,
        }
    }
}

/// The stages of one utterance that has a closing `final`, in microseconds.
/// A stage the record cannot give is none: it is never filled with 0.
#[derive(Clone, Debug, PartialEq)]
pub struct StageRow {
    pub repeat: u32,
    pub utt: u32,
    /// The `lang` of the closing `final`.
    pub lang: Option<String>,
    /// The `id` of the closing `final`: what a page record is joined by.
    pub final_id: Option<String>,
    pub d_us: Option<i64>,
    pub dec_us: Option<i64>,
    pub tr_us: Option<i64>,
    pub settle_us: Option<i64>,
    /// The wait of 3.1, `t_tr - t_end`.
    pub l_us: Option<i64>,
    /// `t_settled - t_end`.
    pub settled_wait_us: Option<i64>,
}

/// Microseconds as whole milliseconds, rounded toward negative infinity.
pub fn floor_ms(micros: i64) -> i64 {
    micros.div_euclid(1000)
}

impl StageRow {
    pub fn d_ms(&self) -> Option<i64> {
        self.d_us.map(floor_ms)
    }

    pub fn dec_ms(&self) -> Option<i64> {
        self.dec_us.map(floor_ms)
    }

    pub fn tr_ms(&self) -> Option<i64> {
        self.tr_us.map(floor_ms)
    }

    pub fn settle_ms(&self) -> Option<i64> {
        self.settle_us.map(floor_ms)
    }

    fn of(&self, stage: Stage) -> Option<i64> {
        match stage {
            Stage::Wait => self.d_us,
            Stage::Decode => self.dec_us,
            Stage::Draft => self.tr_us,
            Stage::Settle => self.settle_us,
        }
    }
}

/// The utterances of a run with a closing `final`, apart: the first of each
/// language in the run is cold (the recognizer of that language had not read
/// anything yet) and all the others are warm.
#[derive(Clone, Debug, PartialEq)]
pub struct StageReport {
    pub cold: Vec<StageRow>,
    pub warm: Vec<StageRow>,
}

/// A percentile pair over the warm utterances, floored to whole milliseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StageStat {
    pub n: usize,
    pub p50_ms: i64,
    pub p95_ms: i64,
}

/// Whether the percentiles of a stage stand on or inside the line of the stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Verdict {
    pub stage: Stage,
    pub p50_inside: bool,
    pub p95_inside: bool,
}

/// How the record words a side of a line.
pub fn side_of(inside: bool) -> &'static str {
    if inside { "線の内" } else { "線の外" }
}

/// The `latency_ms` of a `final` in whole microseconds, floored. The server
/// writes three decimals, so the value is first taken to nanoseconds to keep
/// the noise of a binary fraction from moving the floor.
fn latency_us(latency_ms: f64) -> Option<i64> {
    let nanos = (latency_ms * 1_000_000.0).round();
    (nanos.is_finite() && nanos.abs() < 9.0e18).then(|| (nanos as i64).div_euclid(1000))
}

fn text_of_body<'a>(body: &'a Value, key: &str) -> Option<&'a str> {
    body.get(key).and_then(Value::as_str)
}

/// The closing `final` of a sample: the event that arrived when the sample says.
fn closing_final(raw: &[RawEvent], arrived_us: i64) -> Option<&Value> {
    raw.iter()
        .find(|(t_us, body)| *t_us == arrived_us && text_of_body(body, "type") == Some("final"))
        .map(|(_, body)| body)
}

/// When the settled translation of the `final` `id` arrived: the first with
/// `of: final`, `settled: true` and `status: ok`.
fn settled_at(raw: &[RawEvent], id: &str) -> Option<i64> {
    raw.iter()
        .filter(|(_, body)| {
            text_of_body(body, "type") == Some("translation")
                && text_of_body(body, "id") == Some(id)
                && text_of_body(body, "of") == Some("final")
                && body.get("settled").and_then(Value::as_bool) == Some(true)
                && text_of_body(body, "status") == Some("ok")
        })
        .map(|(t_us, _)| *t_us)
        .min()
}

/// The stages of a sample, or none for one with no closing `final`.
fn stage_row(sample: &Sample, raw: &[RawEvent]) -> Option<StageRow> {
    let t_final = sample.t_final_us?;
    let closing = closing_final(raw, t_final)?;
    let dec_us = sample.final_latency_ms.and_then(latency_us);
    let settled = text_of_body(closing, "id").and_then(|id| settled_at(raw, id));
    Some(StageRow {
        repeat: sample.repeat,
        utt: sample.utt,
        lang: text_of_body(closing, "lang").map(str::to_owned),
        final_id: text_of_body(closing, "id").map(str::to_owned),
        d_us: dec_us.map(|dec| t_final - dec - sample.t_end_us),
        dec_us,
        tr_us: sample.l_tr_us(),
        settle_us: settled
            .zip(sample.t_tr_us)
            .map(|(done, draft)| done - draft),
        l_us: sample.l_us(),
        settled_wait_us: settled.map(|done| done - sample.t_end_us),
    })
}

/// Splits the rows, in the order of the run, into the first of each language and the rest.
fn split_cold(rows: Vec<StageRow>) -> StageReport {
    let mut seen: Vec<Option<String>> = Vec::new();
    let (mut cold, mut warm) = (Vec::new(), Vec::new());
    for row in rows {
        if seen.contains(&row.lang) {
            warm.push(row);
        } else {
            seen.push(row.lang.clone());
            cold.push(row);
        }
    }
    StageReport { cold, warm }
}

/// The stages of every utterance of a run, from the two files it left (the
/// samples file and the events file). The finals are placed on the windows
/// again, by [`samples_of_record`], so the stages do not depend on how the
/// samples file was written. An utterance with no closing `final` has no
/// stage and is not counted.
pub fn stage_report(samples_text: &str, events_text: &str) -> Result<StageReport, EvalError> {
    let windows = windows_of_text(samples_text)?;
    let Some(longest) = windows.iter().map(|window| window.utt).max() else {
        return Err(EvalError::Invalid(String::from(
            "the record has no sample line",
        )));
    };
    let raw = raw_events(events_text)?;
    let mut events = parse_raw(&raw)?;
    place_on_tool_clock(&mut events);
    let references = vec![String::new(); longest as usize];
    let samples = build_samples(&windows, &references, &events)?;
    Ok(split_cold(
        samples
            .iter()
            .filter_map(|sample| stage_row(sample, &raw))
            .collect(),
    ))
}

impl StageReport {
    /// The percentiles of a stage over the warm utterances that have it; none
    /// when no warm utterance does.
    pub fn stat(&self, stage: Stage) -> Result<Option<StageStat>, EvalError> {
        let values: Vec<i64> = self.warm.iter().filter_map(|row| row.of(stage)).collect();
        if values.is_empty() {
            return Ok(None);
        }
        Ok(Some(StageStat {
            n: values.len(),
            p50_ms: floor_ms(nearest_rank(&values, 50)?),
            p95_ms: floor_ms(nearest_rank(&values, 95)?),
        }))
    }

    /// Where the percentiles of each stage that has a line stand against it.
    /// With no warm utterance, or a stage that has no value, nothing was
    /// measured and the answer is an error and not a side of the line.
    pub fn verdicts(&self) -> Result<Vec<Verdict>, EvalError> {
        if self.warm.is_empty() {
            return Err(EvalError::Invalid(String::from(
                "温まった標本 0: the stages cannot be judged against their lines",
            )));
        }
        [Stage::Wait, Stage::Decode, Stage::Draft]
            .into_iter()
            .map(|stage| self.verdict(stage))
            .collect()
    }

    fn verdict(&self, stage: Stage) -> Result<Verdict, EvalError> {
        let (Some(stat), Some((line50, line95))) = (self.stat(stage)?, stage.budget_ms()) else {
            return Err(EvalError::Invalid(format!(
                "no warm utterance has the stage {}",
                stage.name()
            )));
        };
        Ok(Verdict {
            stage,
            p50_inside: stat.p50_ms <= line50,
            p95_inside: stat.p95_ms <= line95,
        })
    }

    /// The record of the run, one line a stage.
    pub fn render(&self) -> String {
        let mut text = format!(
            "冷えた標本 {}、温まった標本 {}\n",
            self.cold.len(),
            self.warm.len()
        );
        if self.warm.is_empty() {
            text.push_str("温まった標本 0: 段の p50 と p95 は出していない\n");
        }
        for stage in [Stage::Wait, Stage::Decode, Stage::Draft, Stage::Settle] {
            if !self.warm.is_empty() {
                text.push_str(&self.stage_line(stage));
            }
        }
        for row in &self.cold {
            text.push_str(&cold_line(row));
        }
        text
    }

    fn stage_line(&self, stage: Stage) -> String {
        let name = stage.name();
        let Ok(Some(stat)) = self.stat(stage) else {
            return format!("{name}: 値なし\n");
        };
        let line = |value: i64, budget: Option<i64>| match budget {
            Some(limit) => format!("{value} ms (線 {limit} ms) {}", side_of(value <= limit)),
            None => format!("{value} ms"),
        };
        let (line50, line95) = match stage.budget_ms() {
            Some((p50, p95)) => (Some(p50), Some(p95)),
            None => (None, None),
        };
        format!(
            "{name}: n {}, p50 {}, p95 {}\n",
            stat.n,
            line(stat.p50_ms, line50),
            line(stat.p95_ms, line95)
        )
    }
}

fn cold_line(row: &StageRow) -> String {
    let show = |value: Option<i64>| value.map_or_else(|| String::from("なし"), |v| v.to_string());
    format!(
        "冷えた標本 repeat {} utt {} ({}): d {} ms, L_dec {} ms, L_tr {} ms\n",
        row.repeat,
        row.utt,
        row.lang.as_deref().map_or("lang なし", |lang| lang),
        show(row.d_ms()),
        show(row.dec_ms()),
        show(row.tr_ms())
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(t_us: i64, kind: EventKind) -> Event {
        Event { t_us, kind }
    }

    fn final_event(t_us: i64, text: &str) -> Event {
        event(
            t_us,
            EventKind::Final {
                text: text.to_owned(),
                latency_ms: Some(100.0),
                id: None,
                t1_us: None,
            },
        )
    }

    fn translation_event(t_us: i64, text: &str) -> Event {
        event(
            t_us,
            EventKind::Translation {
                text: text.to_owned(),
                id: None,
                status: None,
                src_sha256: None,
            },
        )
    }

    fn window(utt: u32, t_start_us: i64, t_end_us: i64) -> Window {
        Window {
            repeat: 1,
            utt,
            t_start_us,
            t_end_us,
        }
    }

    #[test]
    fn a_final_event_is_parsed_with_its_text_and_latency() -> Result<(), EvalError> {
        let parsed = parse_event(
            5,
            r#"{"type":"final","text":"こんにちは","latency_ms":99.5}"#,
        )?;
        assert_eq!(
            parsed,
            event(
                5,
                EventKind::Final {
                    text: String::from("こんにちは"),
                    latency_ms: Some(99.5),
                    id: None,
                    t1_us: None,
                }
            )
        );
        Ok(())
    }

    #[test]
    fn a_final_without_a_latency_has_none() -> Result<(), EvalError> {
        let parsed = parse_event(5, r#"{"type":"final","text":"a","latency_ms":null}"#)?;
        assert!(matches!(
            parsed.kind,
            EventKind::Final {
                latency_ms: None,
                ..
            }
        ));
        Ok(())
    }

    #[test]
    fn other_types_are_other() -> Result<(), EvalError> {
        assert_eq!(
            parse_event(1, r#"{"type":"warning","message":"w"}"#)?.kind,
            EventKind::Other
        );
        assert_eq!(
            parse_event(1, r#"{"type":"partial","text":"a"}"#)?.kind,
            EventKind::Partial
        );
        Ok(())
    }

    #[test]
    fn a_ready_is_read_with_the_stream_position_it_names_and_without_one_it_does_not()
    -> Result<(), EvalError> {
        assert_eq!(
            parse_event(1, r#"{"type":"ready","sr":16000,"stream_s":30.904}"#)?.kind,
            EventKind::Ready {
                stream_us: Some(30_904_000)
            }
        );
        assert_eq!(
            parse_event(1, r#"{"type":"ready","sr":16000}"#)?.kind,
            EventKind::Ready { stream_us: None }
        );
        assert_eq!(
            parse_event(1, r#"{"type":"session_start"}"#)?.kind,
            EventKind::SessionStart
        );
        Ok(())
    }

    #[test]
    fn text_that_is_not_json_is_refused() {
        assert!(matches!(parse_event(1, "{"), Err(EvalError::Protocol(_))));
    }

    #[test]
    fn a_final_without_text_is_refused() {
        assert!(matches!(
            parse_event(1, r#"{"type":"final"}"#),
            Err(EvalError::Protocol(_))
        ));
    }

    #[test]
    fn a_translation_pairs_with_the_final_before_it_until_the_next_final() {
        let events = [
            final_event(10, "u1"),
            translation_event(20, "a"),
            final_event(30, "u2"),
            final_event(40, "u3"),
            translation_event(50, "b"),
        ];
        assert_eq!(
            positional_translations(&events),
            vec![(0, Some(1)), (2, None), (3, Some(4))]
        );
    }

    #[test]
    fn only_the_first_translation_after_a_final_counts() {
        let events = [
            final_event(10, "u1"),
            translation_event(20, "a"),
            translation_event(25, "a2"),
            final_event(30, "u2"),
        ];
        assert_eq!(
            positional_translations(&events),
            vec![(0, Some(1)), (3, None)]
        );
    }

    #[test]
    fn a_translation_before_any_final_is_paired_with_nothing() {
        let events = [translation_event(5, "x"), final_event(10, "u1")];
        assert_eq!(positional_translations(&events), vec![(1, None)]);
    }

    #[test]
    fn a_sample_takes_its_times_from_the_closing_final_and_its_translation() -> Result<(), EvalError>
    {
        let windows = [window(1, 0, 1_000), window(2, 3_000, 4_000)];
        let events = [
            final_event(1_500, "あい"),
            translation_event(1_800, "ai"),
            final_event(4_600, "うえ"),
        ];
        let refs = [String::from("あい"), String::from("うえお")];
        let samples = build_samples(&windows, &refs, &events)?;
        assert_eq!(samples[0].l_asr_us(), Some(500));
        assert_eq!(samples[0].l_us(), Some(800));
        assert_eq!(samples[0].l_tr_us(), Some(300));
        assert_eq!(samples[0].cer_dist, 0);
        assert_eq!(samples[1].l_asr_us(), Some(600));
        assert_eq!(samples[1].l_us(), None);
        assert_eq!(samples[1].cer_dist, 1);
        Ok(())
    }

    #[test]
    fn a_final_before_the_utterance_ended_does_not_close_it() -> Result<(), EvalError> {
        let windows = [window(1, 0, 1_000)];
        let events = [final_event(700, "early")];
        let samples = build_samples(&windows, &[String::from("x")], &events)?;
        assert_eq!(samples[0].t_final_us, None);
        assert_eq!(samples[0].finals, 1);
        assert_eq!(samples[0].text, "early");
        Ok(())
    }

    #[test]
    fn the_early_final_of_a_split_utterance_joins_its_text() -> Result<(), EvalError> {
        let windows = [window(1, 0, 1_000)];
        let events = [final_event(600, "あい"), final_event(1_400, "うえ")];
        let samples = build_samples(&windows, &[String::from("あいうえ")], &events)?;
        assert_eq!(samples[0].text, "あいうえ");
        assert_eq!(samples[0].t_final_us, Some(1_400));
        assert_eq!(samples[0].cer_dist, 0);
        Ok(())
    }

    #[test]
    fn the_first_partial_after_the_start_is_kept() -> Result<(), EvalError> {
        let windows = [window(1, 100, 1_000)];
        let events = [
            event(50, EventKind::Partial),
            event(300, EventKind::Partial),
            final_event(1_500, "a"),
        ];
        let samples = build_samples(&windows, &[String::from("a")], &events)?;
        assert_eq!(samples[0].t_partial_us, Some(300));
        Ok(())
    }

    #[test]
    fn a_partial_after_the_closing_final_is_not_this_utterances() -> Result<(), EvalError> {
        let windows = [window(1, 100, 1_000)];
        let events = [final_event(1_500, "a"), event(1_600, EventKind::Partial)];
        let samples = build_samples(&windows, &[String::from("a")], &events)?;
        assert_eq!(samples[0].t_partial_us, None);
        Ok(())
    }

    #[test]
    fn windows_out_of_order_are_refused() {
        let windows = [window(1, 500, 600), window(2, 100, 200)];
        let refs = [String::from("a"), String::from("b")];
        assert!(matches!(
            build_samples(&windows, &refs, &[]),
            Err(EvalError::Invalid(_))
        ));
    }

    #[test]
    fn an_utterance_with_no_reference_is_refused() {
        let windows = [window(2, 0, 100)];
        assert!(matches!(
            build_samples(&windows, &[String::from("a")], &[]),
            Err(EvalError::Invalid(_))
        ));
    }

    #[test]
    fn no_events_give_samples_with_nothing_measured() -> Result<(), EvalError> {
        let samples = build_samples(&[window(1, 0, 100)], &[String::from("あ")], &[])?;
        assert_eq!(samples[0].t_final_us, None);
        assert_eq!(samples[0].cer_dist, 1);
        Ok(())
    }

    fn sample(utt: u32, t_end: i64, t_final: Option<i64>, t_tr: Option<i64>) -> Sample {
        Sample {
            repeat: 1,
            utt,
            t_start_us: 0,
            t_end_us: t_end,
            t_partial_us: None,
            t_partial_tr_us: None,
            t_final_us: t_final,
            t_tr_us: t_tr,
            finals: 1,
            text: String::new(),
            translation: None,
            final_latency_ms: Some(100.0),
            cer_dist: 1,
            cer_denom: 4,
        }
    }

    #[test]
    fn the_summary_counts_translated_and_untranslated_samples() -> Result<(), EvalError> {
        let samples = [
            sample(1, 0, Some(500_000), Some(900_000)),
            sample(2, 0, Some(600_000), None),
            sample(3, 0, None, None),
        ];
        let summary = summarize(&samples)?;
        assert_eq!(summary.with_final, 2);
        assert_eq!(summary.with_translation, 1);
        assert_eq!(summary.untranslated, 2);
        assert_eq!(
            summary.l,
            Some(Stat {
                n: 1,
                p50_ms: 900,
                p95_ms: 900
            })
        );
        assert_eq!(summary.cer_micros, 250_000);
        Ok(())
    }

    #[test]
    fn a_negative_final_to_translation_gap_is_counted() -> Result<(), EvalError> {
        let samples = [sample(1, 0, Some(500_000), Some(400_000))];
        assert_eq!(summarize(&samples)?.l_tr_negative, 1);
        Ok(())
    }

    #[test]
    fn the_per_utterance_median_spans_the_repetitions() -> Result<(), EvalError> {
        let mut samples = Vec::new();
        for l_asr in [10_000, 30_000, 20_000, 50_000, 40_000] {
            samples.push(sample(1, 0, Some(l_asr), None));
        }
        let summary = summarize(&samples)?;
        assert_eq!(summary.per_utt_median_l_asr_ms, vec![Some(30)]);
        assert_eq!(summary.per_utt_median_l_ms, vec![None]);
        Ok(())
    }

    #[test]
    fn the_mean_of_the_final_latency_is_over_the_samples_that_have_one() -> Result<(), EvalError> {
        let mut without = sample(2, 0, Some(1), None);
        without.final_latency_ms = None;
        let summary = summarize(&[sample(1, 0, Some(1), None), without])?;
        assert_eq!(summary.final_latency_mean_ms, Some(100.0));
        Ok(())
    }

    #[test]
    fn an_empty_run_summarises_to_nothing_measured() -> Result<(), EvalError> {
        let summary = summarize(&[])?;
        assert_eq!(summary.samples, 0);
        assert_eq!(summary.l, None);
        assert_eq!(summary.final_latency_mean_ms, None);
        Ok(())
    }

    #[test]
    fn stat_of_nothing_is_none() -> Result<(), EvalError> {
        assert_eq!(stat_of(&[])?, None);
        Ok(())
    }

    #[test]
    fn a_sample_survives_a_json_line() -> Result<(), serde_json::Error> {
        let original = sample(3, 10, Some(20), Some(30));
        let line = serde_json::to_string(&original)?;
        assert!(!line.contains('\n'));
        let back: Sample = serde_json::from_str(&line)?;
        assert_eq!(back, original);
        Ok(())
    }
}

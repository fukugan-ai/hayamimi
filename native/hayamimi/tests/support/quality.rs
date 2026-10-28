//! The tools of IP-S11 that name what a run got wrong (BR-9 and BR-10).
//!
//! `dialog.rs` counts: how many repeats got no final, how many utterances were
//! mixed up. These functions say which ones, and why, from the text a run left
//! (its samples file, its events file and the `hayamimi` log) and from nothing
//! else. They judge the text they are given and open no file. A count that does
//! not add up is an error: it is never rounded into a record.

use hayamimi_eval::latency::Sample;
use hayamimi_eval::manifest::Row;
use serde_json::Value;

use super::Failure;
use super::dialog::{
    EventLine, FallbackRow, Given, events_of, fallback_rows, final_number, given_of, langs_of,
    samples_of, session_start_us, span_of, stream_us, text_field, unread_rows,
};

/// A stretch of the tool's clock in microseconds, `[start, end)`.
type Span = (i64, i64);

/// Samples in a millisecond at 16 kHz.
const SAMPLES_PER_MS: u64 = 16;

/// Whether two stretches share a point. They are half-open, so stretches that
/// only touch do not (design 10.13.13, 19).
fn overlaps(a: Span, b: Span) -> bool {
    a.0 < b.1 && b.0 < a.1
}

/// What moves the server's stream clock to the tool's: the arrival time of
/// `session_start`, or 0 for a record that has none.
fn shift_of(events: &[EventLine]) -> i64 {
    session_start_us(events).iter().sum()
}

fn tool_span(start_s: f64, end_s: f64, shift_us: i64) -> Span {
    (stream_us(start_s) + shift_us, stream_us(end_s) + shift_us)
}

fn seconds_field(body: &Value, key: &str) -> Option<f64> {
    body.get(key)
        .and_then(Value::as_f64)
        .filter(|seconds| seconds.is_finite())
}

// ---- BR-9: the repeats that got no final ------------------------------------

/// Why a repeat of an utterance got no final (design 10.13.13, 21), in the order
/// the reasons are tried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoFinal {
    /// A final overlaps the voice of the utterance, and `build_samples` gave it
    /// to another utterance.
    Elsewhere,
    /// A segment of the detector overlaps the voice and read as nothing.
    ReadEmpty,
    /// Nothing overlaps the voice: the detector made no segment, or the
    /// recognizer was not given one.
    NoSegment,
}

impl NoFinal {
    pub const ALL: [NoFinal; 3] = [NoFinal::Elsewhere, NoFinal::ReadEmpty, NoFinal::NoSegment];

    /// The words of the record file.
    pub fn word(self) -> &'static str {
        match self {
            NoFinal::Elsewhere => "窓の外に割り当たった",
            NoFinal::ReadEmpty => "空に読んだ",
            NoFinal::NoSegment => "区間が無い",
        }
    }
}

/// One repeat of one utterance that got no final, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoFinalRepeat {
    pub repeat: u32,
    pub utt: u32,
    pub why: NoFinal,
}

/// The stretches of the stream the `final` events cover, on the tool's clock. A
/// final that names no `t0_s` and `t1_s` covers nothing.
fn final_spans(events: &[EventLine], shift_us: i64) -> Vec<Span> {
    events
        .iter()
        .filter(|line| text_field(&line.body, "type") == Some("final"))
        .filter_map(|line| {
            let start = seconds_field(&line.body, "t0_s")?;
            let end = seconds_field(&line.body, "t1_s")?;
            Some(tool_span(start, end, shift_us))
        })
        .collect()
}

/// The stretches the `asr.segment.unread` lines cover, on the tool's clock. A
/// line without a number `start_s` is an error: its stretch is not known.
fn unread_spans(log_text: &str, shift_us: i64) -> Result<Vec<Span>, Failure> {
    unread_rows(log_text)?
        .iter()
        .map(|row| {
            let start_s = row.start_s.ok_or_else(|| {
                String::from("an asr.segment.unread line has no number `start_s`")
            })?;
            Ok(tool_span(start_s, row.end_s, shift_us))
        })
        .collect()
}

/// The first reason that fits a voice that got no final (design 10.13.13, 21).
fn why_no_final(voice: Span, finals: &[Span], unread: &[Span]) -> NoFinal {
    if finals.iter().any(|stretch| overlaps(voice, *stretch)) {
        NoFinal::Elsewhere
    } else if unread.iter().any(|stretch| overlaps(voice, *stretch)) {
        NoFinal::ReadEmpty
    } else {
        NoFinal::NoSegment
    }
}

fn sample_of<'a>(samples: &'a [Sample], given: &Given) -> Result<&'a Sample, Failure> {
    samples
        .iter()
        .find(|sample| sample.repeat == given.repeat && sample.utt == given.utt)
        .ok_or_else(|| format!("repeat {} utt {} has no sample", given.repeat, given.utt))
}

/// The repeats of a run that were given no final, each with its reason.
pub fn repeats_without_final(
    samples_text: &str,
    events_text: &str,
    log_text: &str,
) -> Result<Vec<NoFinalRepeat>, Failure> {
    let samples = samples_of(samples_text)?;
    let events = events_of(events_text)?;
    let shift_us = shift_of(&events);
    let finals = final_spans(&events, shift_us);
    let unread = unread_spans(log_text, shift_us)?;
    let mut repeats = Vec::new();
    for given in given_of(samples_text, events_text)? {
        if !given.finals.is_empty() {
            continue;
        }
        let sample = sample_of(&samples, &given)?;
        let voice = (sample.t_start_us, sample.t_end_us);
        repeats.push(NoFinalRepeat {
            repeat: given.repeat,
            utt: given.utt,
            why: why_no_final(voice, &finals, &unread),
        });
    }
    Ok(repeats)
}

/// How many of the repeats have the reason.
pub fn count_of(repeats: &[NoFinalRepeat], why: NoFinal) -> usize {
    repeats.iter().filter(|repeat| repeat.why == why).count()
}

/// The repeats without a final, told apart. An error when the three counts do
/// not add up to `reported`, the number the record of the run says (design
/// 10.13.13, 21): the way of telling them apart is then wrong.
pub fn split_no_final(
    samples_text: &str,
    events_text: &str,
    log_text: &str,
    reported: usize,
) -> Result<Vec<NoFinalRepeat>, Failure> {
    let repeats = repeats_without_final(samples_text, events_text, log_text)?;
    let sum: usize = NoFinal::ALL
        .iter()
        .map(|why| count_of(&repeats, *why))
        .sum();
    if sum != reported {
        return Err(format!(
            "the three reasons add up to {sum}, the record says {reported} repeats with no final"
        ));
    }
    Ok(repeats)
}

/// The integer after `label:` at the start of each line of a record file, in
/// order (`mixed-up utterances: 1 (線 6) 線の内` gives 1 for the label
/// `mixed-up utterances`).
pub fn numbers_after(record_text: &str, label: &str) -> Vec<usize> {
    let prefix = format!("{label}:");
    record_text
        .lines()
        .filter_map(|line| line.strip_prefix(prefix.as_str()))
        .filter_map(|rest| {
            let digits: String = rest
                .trim_start()
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            digits.parse().ok()
        })
        .collect()
}

/// The one number after `label:` in a record file. A label that is not there,
/// or is there twice, is an error: no line is read as zero.
pub fn the_number_after(record_text: &str, label: &str) -> Result<usize, Failure> {
    match numbers_after(record_text, label).as_slice() {
        [number] => Ok(*number),
        [] => Err(format!("the record has no line `{label}:`")),
        more => Err(format!("the record has {} lines `{label}:`", more.len())),
    }
}

/// The record lines of the repeats without a final: one line for each, then the
/// count of each reason and their sum.
pub fn no_final_text(repeats: &[NoFinalRepeat]) -> String {
    let mut text = String::new();
    for repeat in repeats {
        text.push_str(&format!(
            "repeat {} utt {}: {}\n",
            repeat.repeat,
            repeat.utt,
            repeat.why.word()
        ));
    }
    for why in NoFinal::ALL {
        text.push_str(&format!("{}: {}\n", why.word(), count_of(repeats, why)));
    }
    text.push_str(&format!("和: {}\n", repeats.len()));
    text
}

// ---- BR-9: the utterances the SenseVoice tag decided ---------------------------

/// One `asr.lang.fallback` line whose step is `sv_tag`, joined to the final it
/// belongs to and the utterance that final was given to.
#[derive(Debug, Clone, PartialEq)]
pub struct SvTagRow {
    pub repeat: u32,
    pub utt: u32,
    pub id: String,
    /// The `latency_ms` of the final: the speech end to the final, as the
    /// server measured it (`L_asr`).
    pub l_asr_ms: f64,
    /// The answer of the language identifier, which named neither language.
    pub lid: Option<String>,
}

/// The `latency_ms` of the one final event whose id ends in `number`. A final
/// that is not there, is there twice, or has no number `latency_ms` is an error.
fn latency_of_final(events: &[EventLine], number: u64) -> Result<(String, f64), Failure> {
    let mut found = events
        .iter()
        .filter(|line| text_field(&line.body, "type") == Some("final"))
        .filter_map(|line| {
            let id = text_field(&line.body, "id")?;
            (final_number(id) == Some(number)).then_some((id, &line.body))
        });
    let (id, body) = found
        .next()
        .ok_or_else(|| format!("the fallback of id {number} has no final"))?;
    if found.next().is_some() {
        return Err(format!("the id {number} belongs to two finals"));
    }
    let latency = seconds_field(body, "latency_ms")
        .ok_or_else(|| format!("{id} has no number `latency_ms`"))?;
    Ok((id.to_owned(), latency))
}

fn sv_tag_row(
    row: &FallbackRow,
    events: &[EventLine],
    given: &[Given],
) -> Result<SvTagRow, Failure> {
    let (id, l_asr_ms) = latency_of_final(events, row.id)?;
    let place = given
        .iter()
        .find(|place| {
            place
                .finals
                .iter()
                .any(|placed| placed.id.as_deref() == Some(id.as_str()))
        })
        .ok_or_else(|| format!("{id} was given to no utterance"))?;
    Ok(SvTagRow {
        repeat: place.repeat,
        utt: place.utt,
        id,
        l_asr_ms,
        lid: row.lid.clone(),
    })
}

/// The `sv_tag` fallback lines of a run, in the order of the log, each with the
/// `L_asr` of its final and the answer of the identifier.
pub fn sv_tag_rows(
    samples_text: &str,
    events_text: &str,
    log_text: &str,
) -> Result<Vec<SvTagRow>, Failure> {
    let events = events_of(events_text)?;
    let given = given_of(samples_text, events_text)?;
    fallback_rows(log_text)?
        .iter()
        .filter(|row| row.step == "sv_tag")
        .map(|row| sv_tag_row(row, &events, &given))
        .collect()
}

/// The record lines of the `sv_tag` rows. A run with none says so.
pub fn sv_tag_text(rows: &[SvTagRow]) -> String {
    if rows.is_empty() {
        return String::from("sv_tag なし\n");
    }
    let mut text = String::new();
    for row in rows {
        text.push_str(&format!(
            "repeat {} utt {} {}: L_asr {:.1} ms, lid {}\n",
            row.repeat,
            row.utt,
            row.id,
            row.l_asr_ms,
            // DEFAULT: an `sv_tag` fallback line whose `lid` is null: the identifier answered nothing, failed or was not there; the record writes "なし".
            row.lid.as_deref().unwrap_or("なし")
        ));
    }
    text.push_str(&format!("sv_tag: {}\n", rows.len()));
    text
}

// ---- BR-10: the utterances that were mixed up ----------------------------------

/// One utterance that was read in the wrong language in two repeats or more,
/// with what the two identifiers said (design 10.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MixedUp {
    pub utt: u32,
    /// The language the manifest says the utterance is in.
    pub manifest_lang: String,
    /// The language of the first wrong final.
    pub read_as: String,
    /// The answer of whisper-tiny, when the `asr.lang.fallback` line of the
    /// first wrong final names one.
    pub lid: Option<String>,
    /// The language the SenseVoice tag chose, when the step of the line is
    /// `sv_tag`: the tag itself is not in the log, the language it chose is
    /// the language the final was read in.
    pub sv_mark: Option<String>,
    /// The step of the `asr.lang.fallback` line of the first wrong final.
    pub step: Option<String>,
    /// The voice of the utterance in whole milliseconds (the manifest's span,
    /// rounded down).
    pub voice_ms: i64,
}

/// The repeats of one utterance that have a final in a language other than `want`.
fn wrong_repeats<'a>(given: &'a [Given], utt: u32, want: &str) -> Vec<&'a Given> {
    given
        .iter()
        .filter(|place| place.utt == utt)
        .filter(|place| place.finals.iter().any(|placed| placed.lang != want))
        .collect()
}

fn mixed_up_row(
    utt: u32,
    want: &str,
    wrong: &[&Given],
    fallbacks: &[FallbackRow],
    voice_ms: i64,
) -> Option<MixedUp> {
    let first = wrong
        .first()?
        .finals
        .iter()
        .find(|placed| placed.lang != want)?;
    let line = first
        .id
        .as_deref()
        .and_then(final_number)
        .and_then(|number| fallbacks.iter().find(|line| line.id == number));
    let step = line.map(|line| line.step.clone());
    let sv_mark = (step.as_deref() == Some("sv_tag")).then(|| first.lang.clone());
    Some(MixedUp {
        utt,
        manifest_lang: want.to_owned(),
        read_as: first.lang.clone(),
        lid: line.and_then(|line| line.lid.clone()),
        sv_mark,
        step,
        voice_ms,
    })
}

/// The utterances read in the wrong language in two repeats or more (the rule
/// of `dialog::tally`), each with its manifest language, what the identifiers
/// said, the step of the language route and the length of its voice.
pub fn mixed_up_utterances(
    samples_text: &str,
    events_text: &str,
    log_text: &str,
    manifest: &[Row],
) -> Result<Vec<MixedUp>, Failure> {
    let given = given_of(samples_text, events_text)?;
    let langs = langs_of(manifest)?;
    let fallbacks = fallback_rows(log_text)?;
    let mut rows = Vec::new();
    for (at, want) in langs.iter().enumerate() {
        let utt = u32::try_from(at + 1).map_err(|error| error.to_string())?;
        let wrong = wrong_repeats(&given, utt, want);
        if wrong.len() < 2 {
            continue;
        }
        let samples = span_of(&manifest[at], at)?;
        let voice_ms = i64::try_from(samples / SAMPLES_PER_MS).map_err(|e| e.to_string())?;
        rows.extend(mixed_up_row(utt, want, &wrong, &fallbacks, voice_ms));
    }
    Ok(rows)
}

/// The record lines of the mixed-up utterances. A run with none says so.
pub fn mixed_up_text(rows: &[MixedUp]) -> String {
    if rows.is_empty() {
        return String::from("取り違えなし\n");
    }
    let mut text = String::new();
    for row in rows {
        text.push_str(&format!(
            "utt {}: manifest {}, read as {}, whisper-tiny {}, SenseVoice {}, step {}, voice {} ms\n",
            row.utt,
            row.manifest_lang,
            row.read_as,
            // DEFAULT: the first wrong final has no `asr.lang.fallback` line (it did not fall back, or the run's mode writes none, as the fast run of IP-2) or the line's `lid` is null, so no whisper-tiny answer; the record writes "なし" (H88).
            row.lid.as_deref().unwrap_or("なし"),
            // DEFAULT: the SenseVoice mark is taken only from an `sv_tag` fallback line; without one the record writes "なし" and does not say it was not applied.
            row.sv_mark.as_deref().unwrap_or("なし"),
            // DEFAULT: without a fallback line there is no step; the record writes "なし".
            row.step.as_deref().unwrap_or("なし"),
            row.voice_ms
        ));
    }
    text
}

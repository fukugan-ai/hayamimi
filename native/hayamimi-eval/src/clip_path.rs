//! The path each clip took through the language route (IP-S11, BR-12).
//!
//! IP-4 measures what `--mode pair` adds to the wait for the closing `final`
//! against a run told the language: the delta of a clip is the median over its
//! repeats of `L_asr` in the pair run minus the same median in the `--lang`
//! runs. The clips are tied by the sha256 of their wav. This module ranks every
//! joined clip by that delta, largest first, marks the clips over the line of
//! the 95th percentile, and puts on every clip the marks of the path its
//! language took, so that a clip over the line can be set against the clips
//! inside it: the step of `asr.lang.fallback`, whether the text was read again,
//! and `lid_ms`. It judges the text it is given and opens no file. It changes
//! nothing about how IP-4 joins or measures.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::error::EvalError;
use crate::latency::{Event, EventKind, Sample, Window, build_samples, parse_event};
use crate::percentile::nearest_rank;

/// A delta over this many microseconds is over the line of IP-4 (300 ms).
pub const OVER_US: i64 = 300_000;

/// The event a log would carry for a text read again after its script
/// contradicted its language (`correct_by_script`). The live route does not
/// write it yet, so a log without one says nothing about re-reads.
pub const REREAD_EVENT: &str = "asr.lang.reread";

fn invalid(message: impl Into<String>) -> EvalError {
    EvalError::Invalid(message.into())
}

/// Whether a clip's text was read again after the language was chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reread {
    /// A re-read line names a closing final of the clip.
    Seen,
    /// The log has re-read lines, and none names a closing final of the clip.
    NotSeen,
    /// The log has no re-read line at all: it cannot say.
    NoMark,
}

impl Reread {
    /// The words of the record file.
    pub fn word(self) -> &'static str {
        match self {
            Reread::Seen => "あり",
            Reread::NotSeen => "なし",
            Reread::NoMark => "印なし",
        }
    }
}

/// The marks of the path the language of a clip took, over its repeats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marks {
    /// The distinct steps of the `asr.lang.fallback` lines of the closing
    /// finals of the clip, sorted. Empty when no closing final has a line.
    pub steps: Vec<String>,
    pub reread: Reread,
    /// The median over the closing finals of `lid_ms`, in whole milliseconds
    /// rounded down; `None` when no closing final has an `asr.utterance.timed` line.
    pub lid_ms: Option<i64>,
}

/// One joined clip of the pair run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RankedClip {
    /// The first utterance of the pair run that is this clip.
    pub utt: u32,
    pub sha: String,
    /// The median `L_asr` of the pair run minus that of the `--lang` run, in microseconds.
    pub delta_us: i64,
    /// The delta is over [`OVER_US`].
    pub over: bool,
    pub marks: Marks,
}

/// The samples of one run, and the clip (sha256) of each utterance of it:
/// `shas[utt - 1]` is the clip of utterance `utt`.
#[derive(Clone, Copy)]
pub struct RunInput<'a> {
    pub samples: &'a [Sample],
    pub shas: &'a [String],
}

/// For each clip, the median over its repeats of `L_asr` in microseconds. A
/// repeat with no closing final has no `L_asr` and is left out. This is the
/// rule of IP-4 (`tests/ip04.rs`).
fn medians_by_clip(run: RunInput<'_>) -> Result<BTreeMap<String, i64>, EvalError> {
    let mut waits: BTreeMap<&str, Vec<i64>> = BTreeMap::new();
    for sample in run.samples {
        let sha = usize::try_from(sample.utt)
            .ok()
            .and_then(|utt| utt.checked_sub(1))
            .and_then(|at| run.shas.get(at))
            .ok_or_else(|| invalid(format!("utt {} has no row in the manifest", sample.utt)))?;
        if let Some(wait) = sample.l_asr_us() {
            waits.entry(sha).or_default().push(wait);
        }
    }
    waits
        .into_iter()
        .map(|(sha, values)| Ok((sha.to_owned(), nearest_rank(&values, 50)?)))
        .collect()
}

/// The utterances of a run that are the clip, in order.
fn utts_of_clip(shas: &[String], sha: &str) -> Vec<u32> {
    shas.iter()
        .enumerate()
        .filter(|(_, own)| own.as_str() == sha)
        .filter_map(|(at, _)| u32::try_from(at + 1).ok())
        .collect()
}

// ---- which finals closed which sample -----------------------------------------

/// One line of an events file: when it arrived and what it said.
fn event_lines(text: &str) -> Result<Vec<(i64, Value)>, EvalError> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .enumerate()
        .map(|(at, line)| {
            let value: Value = serde_json::from_str(line)
                .map_err(|error| invalid(format!("events line {}: {error}", at + 1)))?;
            let t_us = value
                .get("t_us")
                .and_then(Value::as_i64)
                .ok_or_else(|| invalid(format!("events line {} has no integer `t_us`", at + 1)))?;
            let body = value
                .get("event")
                .cloned()
                .ok_or_else(|| invalid(format!("events line {} has no `event`", at + 1)))?;
            Ok((t_us, body))
        })
        .collect()
}

/// The finals of an events file as events whose text is their place in the list
/// and a space, with the id of each: the rule of `build_samples` then gives the
/// final to a sample and puts the places of a sample in placement order. When
/// the stream clock began at a `session_start`, the end of each final moves to
/// the tool's clock by the arrival of that event.
fn finals_by_place(text: &str) -> Result<(Vec<Event>, Vec<String>), EvalError> {
    let lines = event_lines(text)?;
    let shift_us = lines
        .iter()
        .find(|(_, body)| body.get("type").and_then(Value::as_str) == Some("session_start"))
        .map_or(0, |(t_us, _)| *t_us);
    let mut stand_ins = Vec::new();
    let mut ids = Vec::new();
    for (t_us, body) in lines {
        if body.get("type").and_then(Value::as_str) != Some("final") {
            continue;
        }
        let EventKind::Final {
            latency_ms,
            id,
            t1_us,
            ..
        } = parse_event(t_us, &body.to_string())?.kind
        else {
            return Err(invalid(format!("a final at {t_us} us was not read as one")));
        };
        let id = id.ok_or_else(|| invalid(format!("a final at {t_us} us has no `id`")))?;
        stand_ins.push(Event {
            t_us,
            kind: EventKind::Final {
                text: format!("{} ", ids.len()),
                latency_ms,
                id: Some(id.clone()),
                t1_us: t1_us.map(|t1| t1 + shift_us),
            },
        });
        ids.push(id);
    }
    Ok((stand_ins, ids))
}

/// The ids of the finals given to each sample of the pair run, in placement
/// order, by `(repeat, utt)`.
fn ids_by_sample(
    samples: &[Sample],
    events_text: &str,
) -> Result<BTreeMap<(u32, u32), Vec<String>>, EvalError> {
    let (stand_ins, ids) = finals_by_place(events_text)?;
    let windows: Vec<Window> = samples
        .iter()
        .map(|sample| Window {
            repeat: sample.repeat,
            utt: sample.utt,
            t_start_us: sample.t_start_us,
            t_end_us: sample.t_end_us,
        })
        .collect();
    // DEFAULT: a pair run with no sample names no utterance, so 0 sizes an empty reference list (there is nothing to place).
    let longest = samples.iter().map(|sample| sample.utt).max().unwrap_or(0);
    // DEFAULT: a u32 always fits a usize on the targets this tool runs on, so the 0 is never taken there; it only keeps the list empty.
    let references = vec![String::new(); usize::try_from(longest).unwrap_or(0)];
    let built = build_samples(&windows, &references, &stand_ins)?;
    built
        .into_iter()
        .map(|sample| {
            let placed = sample
                .text
                .split_whitespace()
                .map(|place| {
                    place
                        .parse::<usize>()
                        .ok()
                        .and_then(|at| ids.get(at))
                        .cloned()
                        .ok_or_else(|| invalid(format!("a sample names the place {place:?}")))
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(((sample.repeat, sample.utt), placed))
        })
        .collect()
}

// ---- the lines of the log -----------------------------------------------------

/// The number a `final` id ends in: `file-3` gives 3.
fn final_number(id: &str) -> Option<u64> {
    id.rsplit_once('-')?.1.parse().ok()
}

/// What the lines of a log say about the finals, by the number of the final.
#[derive(Default)]
struct LogMarks {
    step: BTreeMap<u64, String>,
    lid_us: BTreeMap<u64, i64>,
    reread: BTreeSet<u64>,
    /// The log has at least one re-read line, so a final without one was not read again.
    has_reread_lines: bool,
}

fn log_marks(text: &str) -> Result<LogMarks, EvalError> {
    let mut marks = LogMarks::default();
    for (at, line) in text
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
    {
        let value: Value = serde_json::from_str(line)
            .map_err(|error| invalid(format!("log line {}: {error}", at + 1)))?;
        let id = value.get("id").and_then(Value::as_u64);
        match (value.get("event").and_then(Value::as_str), id) {
            (Some("asr.lang.fallback"), Some(id)) => {
                let step = value.get("step").and_then(Value::as_str);
                let step =
                    step.ok_or_else(|| invalid(format!("log line {} has no `step`", at + 1)))?;
                marks.step.insert(id, step.to_owned());
            }
            (Some("asr.utterance.timed"), Some(id)) => {
                if let Some(lid_ms) = value.get("lid_ms").and_then(Value::as_f64) {
                    marks.lid_us.insert(id, (lid_ms * 1000.0).round() as i64);
                }
            }
            (Some(REREAD_EVENT), Some(id)) => {
                marks.reread.insert(id);
                marks.has_reread_lines = true;
            }
            (Some(REREAD_EVENT), None) => marks.has_reread_lines = true,
            _ => {}
        }
    }
    Ok(marks)
}

/// The marks of the closing finals of a clip, over the repeats it was played in.
fn marks_of_clip(closing: &[u64], log: &LogMarks) -> Result<Marks, EvalError> {
    let steps: BTreeSet<&String> = closing.iter().filter_map(|n| log.step.get(n)).collect();
    let lid_us: Vec<i64> = closing
        .iter()
        .filter_map(|n| log.lid_us.get(n))
        .copied()
        .collect();
    let lid_ms = if lid_us.is_empty() {
        None
    } else {
        Some(nearest_rank(&lid_us, 50)?.div_euclid(1000))
    };
    let reread = if closing.iter().any(|n| log.reread.contains(n)) {
        Reread::Seen
    } else if log.has_reread_lines {
        Reread::NotSeen
    } else {
        Reread::NoMark
    };
    Ok(Marks {
        steps: steps.into_iter().cloned().collect(),
        reread,
        lid_ms,
    })
}

/// The numbers of the closing finals (the last final given to the sample) of
/// the samples of the utterances of a clip.
fn closing_numbers(
    pair: RunInput<'_>,
    sha: &str,
    ids: &BTreeMap<(u32, u32), Vec<String>>,
) -> Vec<u64> {
    let utts = utts_of_clip(pair.shas, sha);
    pair.samples
        .iter()
        .filter(|sample| utts.contains(&sample.utt) && sample.t_final_us.is_some())
        .filter_map(|sample| ids.get(&(sample.repeat, sample.utt))?.last())
        .filter_map(|id| final_number(id))
        .collect()
}

/// Every clip the pair run and the `--lang` runs both read, largest delta first
/// (clips of an equal delta in the order of their utterance), each with the
/// marks of its path. Nothing in common is an error: it would read as a delta of 0.
pub fn rank_clips(
    pair: RunInput<'_>,
    forced: &[RunInput<'_>],
    events_text: &str,
    log_text: &str,
) -> Result<Vec<RankedClip>, EvalError> {
    let own = medians_by_clip(pair)?;
    let mut told = BTreeMap::new();
    for run in forced {
        told.extend(medians_by_clip(*run)?);
    }
    let ids = ids_by_sample(pair.samples, events_text)?;
    let log = log_marks(log_text)?;
    let mut ranked = Vec::new();
    for (sha, wait) in &own {
        let Some(forced_wait) = told.get(sha) else {
            continue;
        };
        let delta_us = wait - forced_wait;
        // DEFAULT: `sha` is a key of the pair run's own medians, which come from `pair.shas`, so the clip has an utterance and the 0 is not taken; it only keeps the code total.
        let utt = utts_of_clip(pair.shas, sha).first().copied().unwrap_or(0);
        let closing = closing_numbers(pair, sha, &ids);
        ranked.push(RankedClip {
            utt,
            sha: sha.clone(),
            delta_us,
            over: delta_us > OVER_US,
            marks: marks_of_clip(&closing, &log)?,
        });
    }
    if ranked.is_empty() {
        return Err(invalid(
            "the pair run and the --lang runs have no clip in common",
        ));
    }
    ranked.sort_by(|a, b| b.delta_us.cmp(&a.delta_us).then(a.utt.cmp(&b.utt)));
    Ok(ranked)
}

/// The 95th percentile of the deltas of the ranked clips, in microseconds (the
/// rule of IP-4: nearest rank).
pub fn delta_p95_us(ranked: &[RankedClip]) -> Result<i64, EvalError> {
    let deltas: Vec<i64> = ranked.iter().map(|clip| clip.delta_us).collect();
    nearest_rank(&deltas, 95)
}

/// The record line of one clip. A clip whose path left no mark at all says so.
pub fn clip_text(clip: &RankedClip) -> String {
    let over = if clip.over { " over" } else { "" };
    let lid = clip
        .marks
        .lid_ms
        .map_or_else(|| String::from("lid_ms なし"), |ms| format!("lid_ms {ms}"));
    let path = if clip.marks.steps.is_empty() && clip.marks.reread == Reread::NoMark {
        String::from("道 印なし")
    } else {
        let steps = if clip.marks.steps.is_empty() {
            String::from("なし")
        } else {
            clip.marks.steps.join("、")
        };
        format!("step {steps}、読み直し {}", clip.marks.reread.word())
    };
    format!(
        "utt {}: 差 {} ms{over}、{path}、{lid}",
        clip.utt,
        clip.delta_us.div_euclid(1000)
    )
}

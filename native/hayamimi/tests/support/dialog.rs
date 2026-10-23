//! The tools that count what an in-person run recorded (IP-2 and IP-7).
//!
//! A run leaves `<run>.jsonl` (one sample per utterance and repeat), `<run>.events.jsonl`
//! (every event received, with its arrival time) and the `hayamimi` log. These
//! functions read that text and answer the questions of design 10.9.2: is the
//! record complete, which utterances were read in the wrong language, and how
//! often the language route fell back. They judge the text they are given and
//! open no file except where a function says so.

use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

use hayamimi_eval::latency::{Event, EventKind, Sample, Window, build_samples, parse_event};
use hayamimi_eval::manifest::{Row, read_rows};
use serde_json::Value;

use super::Failure;

/// The steps `asr.lang.fallback` names, in the order of the design.
pub const STEPS: [&str; 4] = ["sv_tag", "script", "default", "sv_failed"];

/// The lines of a text that are not blank, with their 1-based numbers.
fn lines_of(text: &str) -> impl Iterator<Item = (usize, &str)> {
    text.lines()
        .enumerate()
        .map(|(at, line)| (at + 1, line))
        .filter(|(_, line)| !line.trim().is_empty())
}

/// The samples file of a run.
pub fn samples_of(text: &str) -> Result<Vec<Sample>, Failure> {
    lines_of(text)
        .map(|(number, line)| {
            serde_json::from_str::<Sample>(line)
                .map_err(|error| format!("samples line {number}: {error}"))
        })
        .collect()
}

/// A record is complete when every repeat of every utterance appears once. The
/// count of samples is returned. A record with no line is not complete.
pub fn check_complete(text: &str, repeats: u32, utterances: u32) -> Result<usize, Failure> {
    let samples = samples_of(text)?;
    if samples.is_empty() {
        return Err(String::from("the record has no line"));
    }
    let mut seen: BTreeMap<(u32, u32), usize> = BTreeMap::new();
    for sample in &samples {
        *seen.entry((sample.repeat, sample.utt)).or_default() += 1;
    }
    let mut missing = Vec::new();
    for repeat in 1..=repeats {
        for utt in 1..=utterances {
            match seen.get(&(repeat, utt)) {
                None => missing.push(format!("repeat {repeat} utt {utt}")),
                Some(1) => {}
                Some(times) => {
                    return Err(format!("repeat {repeat} utt {utt} appears {times} times"));
                }
            }
        }
    }
    if !missing.is_empty() {
        return Err(format!("the record lacks {}", missing.join(", ")));
    }
    let expected = usize::try_from(repeats * utterances)
        .map_err(|error| format!("{repeats} x {utterances} does not fit: {error}"))?;
    if samples.len() != expected {
        return Err(format!(
            "the record has {} lines, expected {expected}",
            samples.len()
        ));
    }
    Ok(samples.len())
}

/// One event of an events file: when it arrived and what it said.
pub struct EventLine {
    pub t_us: i64,
    pub body: Value,
}

/// The events file of a run: one `{"t_us", "event"}` object per line.
pub fn events_of(text: &str) -> Result<Vec<EventLine>, Failure> {
    lines_of(text)
        .map(|(number, line)| {
            let value: Value = serde_json::from_str(line)
                .map_err(|error| format!("events line {number}: {error}"))?;
            let t_us = value
                .get("t_us")
                .and_then(Value::as_i64)
                .ok_or_else(|| format!("events line {number} has no integer `t_us`"))?;
            let body = value
                .get("event")
                .cloned()
                .ok_or_else(|| format!("events line {number} has no `event`"))?;
            Ok(EventLine { t_us, body })
        })
        .collect()
}

fn text_field<'a>(body: &'a Value, key: &str) -> Option<&'a str> {
    body.get(key).and_then(Value::as_str)
}

/// The languages of the finals one utterance of one repeat was given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    pub repeat: u32,
    pub utt: u32,
    pub langs: Vec<String>,
}

/// A `final` as the run placed it: its language and the id the server gave it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    pub lang: String,
    pub id: Option<String>,
}

/// The finals one utterance of one repeat was given, in the order the run
/// placed them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Given {
    pub repeat: u32,
    pub utt: u32,
    pub finals: Vec<Placed>,
}

/// The finals of a run, and the same finals as events whose text is their
/// place in that list and a space: the run's own rule (`build_samples`) then
/// gives each final to its utterance and puts the places of an utterance in
/// the order the run placed them.
fn finals_as_events(
    events: &[EventLine],
    shift_us: i64,
) -> Result<(Vec<Event>, Vec<Placed>), Failure> {
    let mut stand_ins = Vec::new();
    let mut placed = Vec::new();
    for line in events {
        if text_field(&line.body, "type") != Some("final") {
            continue;
        }
        let lang = text_field(&line.body, "lang")
            .ok_or_else(|| format!("a final at {} us has no `lang`", line.t_us))?;
        let read = parse_event(line.t_us, &line.body.to_string()).map_err(|e| e.to_string())?;
        let EventKind::Final {
            latency_ms,
            id,
            t1_us,
            ..
        } = read.kind
        else {
            return Err(format!("a final at {} us was not read as one", line.t_us));
        };
        stand_ins.push(Event {
            t_us: line.t_us,
            kind: EventKind::Final {
                text: format!("{} ", placed.len()),
                latency_ms,
                id: id.clone(),
                t1_us: t1_us.map(|t1| t1 + shift_us),
            },
        });
        placed.push(Placed {
            lang: lang.to_owned(),
            id,
        });
    }
    Ok((stand_ins, placed))
}

/// When the server's stream clock began, on the tool's clock: the arrival time
/// of the first `session_start` event, which the server publishes as it starts
/// to take the audio of the run (`hayamimi/src/ingest.rs`, `session_started`).
/// A `final`'s `t1_s` counts from then, and a sample's `t_start_us` from the
/// start of the tool, which waited for the server to be ready.
pub fn session_start_us(events: &[EventLine]) -> Option<i64> {
    events
        .iter()
        .find(|line| text_field(&line.body, "type") == Some("session_start"))
        .map(|line| line.t_us)
}

/// The span of each sample of a run, for `build_samples`.
fn windows_of(samples: &[Sample]) -> Vec<Window> {
    samples
        .iter()
        .map(|sample| Window {
            repeat: sample.repeat,
            utt: sample.utt,
            t_start_us: sample.t_start_us,
            t_end_us: sample.t_end_us,
        })
        .collect()
}

/// The finals that stand at `places`, the places `build_samples` joined into
/// the text of one sample.
fn finals_at(places: &str, placed: &[Placed]) -> Result<Vec<Placed>, Failure> {
    places
        .split_whitespace()
        .map(|place| {
            place
                .parse::<usize>()
                .ok()
                .and_then(|at| placed.get(at))
                .cloned()
                .ok_or_else(|| format!("a sample names the place {place:?}, which is no final"))
        })
        .collect()
}

/// For each sample of the run, the finals that were given to it.
pub fn given_of(samples_text: &str, events_text: &str) -> Result<Vec<Given>, Failure> {
    let samples = samples_of(samples_text)?;
    let Some(longest) = samples.iter().map(|sample| sample.utt).max() else {
        return Err(String::from("the record has no line"));
    };
    let references = vec![String::new(); usize::try_from(longest).map_err(|e| e.to_string())?];
    let lines = events_of(events_text)?;
    // A record with no `session_start` (a hand-made one) has its server clock
    // and the tool's clock equal: the sum over no event is 0.
    let shift_us: i64 = session_start_us(&lines).iter().sum();
    let (stand_ins, placed) = finals_as_events(&lines, shift_us)?;
    let built =
        build_samples(&windows_of(&samples), &references, &stand_ins).map_err(|e| e.to_string())?;
    built
        .into_iter()
        .map(|sample| {
            Ok(Given {
                repeat: sample.repeat,
                utt: sample.utt,
                finals: finals_at(&sample.text, &placed)?,
            })
        })
        .collect()
}

/// For each sample of the run, the languages of the finals that were given to it.
pub fn cells_of(samples_text: &str, events_text: &str) -> Result<Vec<Cell>, Failure> {
    Ok(given_of(samples_text, events_text)?
        .into_iter()
        .map(|given| Cell {
            repeat: given.repeat,
            utt: given.utt,
            langs: given.finals.into_iter().map(|done| done.lang).collect(),
        })
        .collect())
}

/// What a run got wrong, counted by utterance (design 10.9.2).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Tally {
    pub utterances: usize,
    /// Utterances read in the wrong language in two repeats or more.
    pub mixed_up: usize,
    pub ja_as_en: usize,
    pub en_as_ja: usize,
    /// Mixed-up utterances whose wrong language is neither `ja` nor `en`.
    pub other: usize,
    /// Finals in a language that is neither `ja` nor `en`.
    pub other_finals: usize,
    /// Utterances that were given two finals or more in some repeat.
    pub split: usize,
    /// Repeats of an utterance that were given no final.
    pub no_final_repeats: usize,
    /// Utterances that were given no final in any repeat.
    pub undecided: Vec<u32>,
}

/// The language a final of a mixed-up utterance had, in the first wrong repeat.
fn first_wrong<'a>(cells: &[&'a Cell], want: &str) -> Option<&'a str> {
    cells
        .iter()
        .flat_map(|cell| cell.langs.iter())
        .map(String::as_str)
        .find(|lang| *lang != want)
}

fn count_direction(tally: &mut Tally, want: &str, got: &str) {
    match (want, got) {
        ("ja", "en") => tally.ja_as_en += 1,
        ("en", "ja") => tally.en_as_ja += 1,
        _ => tally.other += 1,
    }
}

fn is_wrong(cell: &Cell, want: &str) -> bool {
    cell.langs.iter().any(|lang| lang != want)
}

/// Counts what the cells of a run got wrong; `manifest[utt - 1]` is the
/// language of utterance `utt`.
pub fn tally(cells: &[Cell], manifest: &[String]) -> Result<Tally, Failure> {
    let mut tally = Tally {
        utterances: manifest.len(),
        ..Tally::default()
    };
    let in_manifest =
        |utt: u32| usize::try_from(utt).is_ok_and(|at| (1..=manifest.len()).contains(&at));
    if let Some(cell) = cells.iter().find(|cell| !in_manifest(cell.utt)) {
        return Err(format!(
            "utt {} has no row in the manifest of {} rows",
            cell.utt,
            manifest.len()
        ));
    }
    tally.other_finals = cells
        .iter()
        .flat_map(|cell| cell.langs.iter())
        .filter(|lang| !matches!(lang.as_str(), "ja" | "en"))
        .count();
    for (at, want) in manifest.iter().enumerate() {
        let utt = u32::try_from(at + 1).map_err(|error| error.to_string())?;
        let own: Vec<&Cell> = cells.iter().filter(|cell| cell.utt == utt).collect();
        tally.no_final_repeats += own.iter().filter(|cell| cell.langs.is_empty()).count();
        if !own.is_empty() && own.iter().all(|cell| cell.langs.is_empty()) {
            tally.undecided.push(utt);
        }
        tally.split += usize::from(own.iter().any(|cell| cell.langs.len() >= 2));
        let wrong: Vec<&Cell> = own
            .iter()
            .copied()
            .filter(|cell| is_wrong(cell, want))
            .collect();
        if wrong.len() < 2 {
            continue;
        }
        tally.mixed_up += 1;
        if let Some(got) = first_wrong(&wrong, want) {
            count_direction(&mut tally, want, got);
        }
    }
    Ok(tally)
}

/// How many translations in an events file were made into the language they were made from.
pub fn same_language_translations(events_text: &str) -> Result<usize, Failure> {
    Ok(events_of(events_text)?
        .iter()
        .filter(|line| text_field(&line.body, "type") == Some("translation"))
        .filter(|line| {
            let from = text_field(&line.body, "src_lang");
            from.is_some() && from == text_field(&line.body, "lang")
        })
        .count())
}

/// One `asr.lang.fallback` line of a log: the `final` it belongs to and the step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FallbackRow {
    pub id: u64,
    pub step: String,
}

/// The `asr.lang.fallback` lines of a log. A line that is not JSON, or a step
/// outside the four the design names, is an error.
pub fn fallback_rows(log_text: &str) -> Result<Vec<FallbackRow>, Failure> {
    let mut rows = Vec::new();
    for (number, line) in lines_of(log_text) {
        let value: Value = serde_json::from_str(line)
            .map_err(|error| format!("log line {number} is not JSON: {error}"))?;
        if text_field(&value, "event") != Some("asr.lang.fallback") {
            continue;
        }
        let step =
            text_field(&value, "step").ok_or_else(|| format!("log line {number} has no `step`"))?;
        if !STEPS.contains(&step) {
            return Err(format!("log line {number} names the step {step:?}"));
        }
        let id = value
            .get("id")
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("log line {number} has no integer `id`"))?;
        rows.push(FallbackRow {
            id,
            step: step.to_owned(),
        });
    }
    Ok(rows)
}

/// How many fallback lines name each of the four steps, in the order of [`STEPS`].
pub fn count_by_step(log_text: &str) -> Result<[usize; 4], Failure> {
    let rows = fallback_rows(log_text)?;
    Ok(STEPS.map(|step| rows.iter().filter(|row| row.step == step).count()))
}

/// [`count_by_step`] over the log file of a run; a file that is not there is an
/// error that names it, and is not read as no fallback.
pub fn count_by_step_in(path: &Path) -> Result<[usize; 4], Failure> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read the log {}: {error}", path.display()))?;
    count_by_step(&text)
}

/// The number a `final` id ends in: `file-3` gives 3.
fn final_number(id: &str) -> Option<u64> {
    id.rsplit_once('-')?.1.parse().ok()
}

/// Ties each fallback line of a log to the `final` of the same run that has its
/// number, by that number alone (repeats of one process continue the numbers).
/// A final with two fallback lines, a fallback line with no final, and a number
/// that two finals share (the events of two runs mixed) are errors. The count of
/// tied lines is returned.
pub fn match_fallbacks(log_text: &str, events_text: &str) -> Result<usize, Failure> {
    let rows = fallback_rows(log_text)?;
    let mut finals: BTreeMap<u64, Vec<String>> = BTreeMap::new();
    for line in events_of(events_text)? {
        if text_field(&line.body, "type") != Some("final") {
            continue;
        }
        let Some(id) = text_field(&line.body, "id") else {
            continue;
        };
        if let Some(number) = final_number(id) {
            finals.entry(number).or_default().push(id.to_owned());
        }
    }
    for row in &rows {
        let times = rows.iter().filter(|other| other.id == row.id).count();
        if times > 1 {
            return Err(format!(
                "id {} has {times} fallback events for one final",
                row.id
            ));
        }
        match finals.get(&row.id) {
            None => return Err(format!("id {} has no final", row.id)),
            Some(ids) if ids.len() > 1 => {
                return Err(format!("{} has {} finals", ids[0], ids.len()));
            }
            Some(_) => {}
        }
    }
    Ok(rows.len())
}

/// The reasons `asr.segment.unread` names, in the order of the design.
pub const REASONS: [&str; 2] = ["empty_decode", "empty_text"];

/// One `asr.segment.unread` line of a log: why the segment made no `final`,
/// and where on the stream it ended.
#[derive(Debug, Clone, PartialEq)]
pub struct UnreadRow {
    pub end_s: f64,
    pub reason: String,
}

/// The `asr.segment.unread` lines of a log. A line that is not JSON, a reason
/// outside the two the design names, and a span that is not a number are errors.
pub fn unread_rows(log_text: &str) -> Result<Vec<UnreadRow>, Failure> {
    let mut rows = Vec::new();
    for (number, line) in lines_of(log_text) {
        let value: Value = serde_json::from_str(line)
            .map_err(|error| format!("log line {number} is not JSON: {error}"))?;
        if text_field(&value, "event") != Some("asr.segment.unread") {
            continue;
        }
        let reason = text_field(&value, "reason")
            .ok_or_else(|| format!("log line {number} has no `reason`"))?;
        if !REASONS.contains(&reason) {
            return Err(format!("log line {number} names the reason {reason:?}"));
        }
        let end_s = value
            .get("end_s")
            .and_then(Value::as_f64)
            .filter(|seconds| seconds.is_finite())
            .ok_or_else(|| format!("log line {number} has no number `end_s`"))?;
        rows.push(UnreadRow {
            end_s,
            reason: reason.to_owned(),
        });
    }
    Ok(rows)
}

/// How many unread lines name each of the two reasons, in the order of [`REASONS`].
pub fn count_by_reason(rows: &[UnreadRow]) -> [usize; 2] {
    REASONS.map(|reason| rows.iter().filter(|row| row.reason == reason).count())
}

/// The utterances that were given no final in any repeat, sorted by whether
/// the detector handed the recognizer a segment in their window.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Silent {
    /// No segment that read as nothing ended in any repeat's window: the
    /// detector made no segment, or the recognizer was not given one.
    pub no_segment: Vec<u32>,
    /// A segment that read as nothing ended in the window of some repeat.
    pub read_empty: Vec<u32>,
}

/// A time on the stream in whole microseconds, the unit of the samples file.
fn stream_us(seconds: f64) -> i64 {
    (seconds * 1_000_000.0).round() as i64
}

/// Sorts the utterances in `undecided` (those with no final in any repeat,
/// `Tally::undecided`) by their window, which runs from the start of the
/// sample to the start of the next one (design 10.9.2, 6). A segment belongs
/// to the window its end falls in, as a `final` does by its `t1_s`.
pub fn split_undecided(
    samples_text: &str,
    undecided: &[u32],
    unread: &[UnreadRow],
) -> Result<Silent, Failure> {
    let samples = samples_of(samples_text)?;
    let marks: Vec<i64> = unread.iter().map(|row| stream_us(row.end_s)).collect();
    let ends = samples
        .iter()
        .skip(1)
        .map(|next| next.t_start_us)
        .chain(std::iter::once(i64::MAX));
    let windows: Vec<(u32, i64, i64)> = samples
        .iter()
        .zip(ends)
        .map(|(sample, end)| (sample.utt, sample.t_start_us, end))
        .collect();
    let mut split = Silent::default();
    for utt in undecided {
        let read = windows
            .iter()
            .filter(|(own, _, _)| own == utt)
            .any(|(_, start, end)| marks.iter().any(|mark| (*start..*end).contains(mark)));
        if read {
            split.read_empty.push(*utt);
        } else {
            split.no_segment.push(*utt);
        }
    }
    Ok(split)
}

/// Which side of a line a count stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Inside,
    Outside,
}

impl Verdict {
    /// The words the record file uses.
    pub fn word(self) -> &'static str {
        match self {
            Verdict::Inside => "線の内",
            Verdict::Outside => "線の外",
        }
    }
}

/// A count at or below the line is inside it.
pub fn judge_line(count: usize, line: usize) -> Verdict {
    if count <= line {
        Verdict::Inside
    } else {
        Verdict::Outside
    }
}

/// Appends the verdict to the record file with the line it was judged by. A
/// count over the line is written down and is not a failure.
pub fn record_verdict(
    path: &Path,
    label: &str,
    count: usize,
    line: usize,
) -> Result<Verdict, Failure> {
    let verdict = judge_line(count, line);
    append_line(
        path,
        &format!("{label}: {count} (線 {line}) {}", verdict.word()),
    )?;
    Ok(verdict)
}

/// Appends one line to a file, making it when it is not there.
pub fn append_line(path: &Path, line: &str) -> Result<(), Failure> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| format!("cannot open {}: {error}", path.display()))?;
    writeln!(file, "{line}").map_err(|error| format!("cannot write {}: {error}", path.display()))
}

/// How many times the language changes from one row of a manifest to the next.
pub fn switches(langs: &[String]) -> usize {
    langs.windows(2).filter(|pair| pair[0] != pair[1]).count()
}

/// The rows of a manifest.
pub fn manifest_rows(path: &Path) -> Result<Vec<Row>, Failure> {
    read_rows(path).map_err(|error| error.to_string())
}

/// The `lang` of each row of a manifest, in order.
pub fn langs_of(rows: &[Row]) -> Result<Vec<String>, Failure> {
    rows.iter()
        .enumerate()
        .map(|(at, row)| {
            row.get("lang")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| format!("row {at} has no string `lang`"))
        })
        .collect()
}

/// The samples one row of a manifest spans.
pub fn span_of(row: &Row, at: usize) -> Result<u64, Failure> {
    let number = |key: &str| {
        row.get(key)
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("row {at} has no integer `{key}`"))
    };
    number("end_sample")?
        .checked_sub(number("start_sample")?)
        .ok_or_else(|| format!("row {at} ends before it starts"))
}

/// Writes the counts of a run to the record file. The count of mixed-up
/// utterances is judged against `line` and written down either way.
pub fn record_tally(path: &Path, tally: &Tally, line: usize) -> Result<Verdict, Failure> {
    let verdict = record_verdict(path, "mixed-up utterances", tally.mixed_up, line)?;
    let facts = [
        ("utterances", tally.utterances),
        ("ja read as en", tally.ja_as_en),
        ("en read as ja", tally.en_as_ja),
        ("mixed up toward a third language", tally.other),
        ("finals in a third language", tally.other_finals),
        ("utterances split into two finals or more", tally.split),
        ("repeats with no final", tally.no_final_repeats),
    ];
    for (name, count) in facts {
        append_line(path, &format!("{name}: {count}"))?;
    }
    append_line(
        path,
        &format!("utterances with no final: {:?}", tally.undecided),
    )?;
    Ok(verdict)
}

/// Writes what the language route did in a run to the record file: how often
/// each step chose the language, and whether each fallback line is tied to one
/// final. A log that cannot be read or tied is written down as it is said, and
/// is not a failure here (design 10.9, IP-2: it is looked into as a fall of IP-3).
fn record_fallbacks(path: &Path, log: &Path, events_text: &str) -> Result<(), Failure> {
    match count_by_step_in(log) {
        Ok(counts) => {
            for (step, count) in STEPS.iter().zip(counts) {
                append_line(path, &format!("asr.lang.fallback {step}: {count}"))?;
            }
        }
        Err(error) => append_line(path, &format!("asr.lang.fallback: {error}"))?,
    }
    let tied = std::fs::read_to_string(log)
        .map_err(|error| error.to_string())
        .and_then(|text| match_fallbacks(&text, events_text));
    match tied {
        Ok(count) => append_line(path, &format!("fallback lines tied to a final: {count}")),
        Err(error) => append_line(path, &format!("fallback lines not tied: {error}")),
    }
}

/// Writes what the detector made of the speech that got no final: how often
/// each reason ended a segment, and the utterances with no final by kind
/// (design 10.9.2, 6). A log that cannot be read is written down as it is said,
/// and is not a failure here.
fn record_unread(
    path: &Path,
    log: &Path,
    samples_text: &str,
    undecided: &[u32],
) -> Result<(), Failure> {
    let read = std::fs::read_to_string(log)
        .map_err(|error| format!("cannot read the log {}: {error}", log.display()))
        .and_then(|text| unread_rows(&text));
    let rows = match read {
        Ok(rows) => rows,
        Err(error) => return append_line(path, &format!("asr.segment.unread: {error}")),
    };
    for (reason, count) in REASONS.iter().zip(count_by_reason(&rows)) {
        append_line(path, &format!("asr.segment.unread {reason}: {count}"))?;
    }
    let split = split_undecided(samples_text, undecided, &rows)?;
    append_line(
        path,
        &format!("utterances with no segment: {:?}", split.no_segment),
    )?;
    append_line(
        path,
        &format!("utterances read as nothing: {:?}", split.read_empty),
    )
}

/// Reads the files a run left (`out`, its `.events.jsonl` and its `.log`) and
/// writes the counts of design 10.9 to the record file under `label`.
pub fn record_dialog_run(
    path: &Path,
    label: &str,
    out: &Path,
    langs: &[String],
    line: usize,
) -> Result<Verdict, Failure> {
    let samples = super::read(out)?;
    let events_text = super::read(&out.with_extension("events.jsonl"))?;
    let counted = tally(&cells_of(&samples, &events_text)?, langs)?;
    append_line(path, &format!("## {label}"))?;
    let verdict = record_tally(path, &counted, line)?;
    let same = same_language_translations(&events_text)?;
    append_line(
        path,
        &format!("translations into their own language: {same}"),
    )?;
    let log = out.with_extension("log");
    record_fallbacks(path, &log, &events_text)?;
    record_unread(path, &log, &samples, &counted.undecided)?;
    Ok(verdict)
}

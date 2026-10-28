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

use hayamimi_eval::latency::{
    Event, EventKind, Sample, Stage, StageReport, StageRow, Window, build_samples, floor_ms,
    parse_event,
};
use hayamimi_eval::manifest::{Row, read_rows};
use hayamimi_eval::percentile::nearest_rank;
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

/// How many samples of a run have a closing final, and how many have a
/// translation. An `Err` when no sample has a final, or when fewer than half as
/// many samples have a translation as have a final: the relay stopped
/// translating while the run went on.
pub fn check_translated(text: &str) -> Result<(usize, usize), Failure> {
    let samples = samples_of(text)?;
    let finals = samples
        .iter()
        .filter(|sample| sample.t_final_us.is_some())
        .count();
    let translated = samples
        .iter()
        .filter(|sample| sample.t_tr_us.is_some())
        .count();
    if finals == 0 {
        return Err(String::from("no sample has a final"));
    }
    if translated * 2 < finals {
        return Err(format!(
            "only {translated} of the {finals} samples with a final have a translation"
        ));
    }
    Ok((finals, translated))
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

pub fn text_field<'a>(body: &'a Value, key: &str) -> Option<&'a str> {
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

/// One `asr.lang.fallback` line of a log: the `final` it belongs to, the step,
/// and the answer of the language identifier (`None` when the line says null).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FallbackRow {
    pub id: u64,
    pub step: String,
    pub lid: Option<String>,
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
            lid: text_field(&value, "lid").map(str::to_owned),
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
pub fn final_number(id: &str) -> Option<u64> {
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
/// and where on the stream it started (`None` when the line has no number)
/// and ended.
#[derive(Debug, Clone, PartialEq)]
pub struct UnreadRow {
    pub start_s: Option<f64>,
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
            start_s: value
                .get("start_s")
                .and_then(Value::as_f64)
                .filter(|seconds| seconds.is_finite()),
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
pub fn stream_us(seconds: f64) -> i64 {
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

// ---- the page stage and the budget (BR-2, BR-3) ----------------------------

/// One translation as the page's own clock saw it, in milliseconds of
/// `performance.now()`.
#[derive(Clone, Debug, PartialEq)]
pub struct PageStamp {
    /// The `id` of the line the translation is of.
    pub id: String,
    pub p_arrive: Option<f64>,
    pub p_draw: Option<f64>,
    /// When the tool received the same translation, on the tool's clock. It only
    /// joins a stamp to a sample; the page stage never reads it.
    pub tool_us: Option<i64>,
}

/// The line of the page stage in milliseconds, `(p50, p95)`.
const PAGE_STAGE_LINE: (f64, f64) = (50.0, 100.0);

/// `L_pg = p_draw - p_arrive` of one stamp. A stamp that lacks a time is
/// named, and so is a draw that is before its arrival on the same clock.
fn page_wait(stamp: &PageStamp) -> Result<f64, Failure> {
    let id = &stamp.id;
    let arrive = stamp
        .p_arrive
        .ok_or_else(|| format!("{id}: the page recorded no p_arrive"))?;
    let draw = stamp
        .p_draw
        .ok_or_else(|| format!("{id}: the page recorded no p_draw"))?;
    let wait = draw - arrive;
    if wait < 0.0 {
        return Err(format!(
            "{id}: p_draw {draw:.3} is before p_arrive {arrive:.3}: L_pg {wait:.3} ms"
        ));
    }
    Ok(wait)
}

/// `L_pg` in milliseconds of every translation the page recorded. A page that
/// drew none of them, or none at all, has no stage and is an error.
pub fn page_waits(stamps: &[PageStamp]) -> Result<Vec<f64>, Failure> {
    if stamps.is_empty() {
        return Err(String::from("the page recorded no translation"));
    }
    if stamps.iter().all(|stamp| stamp.p_draw.is_none()) {
        return Err(format!(
            "the page drew none of {} translations: no p_draw",
            stamps.len()
        ));
    }
    stamps.iter().map(page_wait).collect()
}

/// Milliseconds as whole microseconds, to the nearest.
fn micros_of(ms: f64) -> i64 {
    (ms * 1000.0).round() as i64
}

/// `(p50, p95)` of waits in milliseconds, nearest-rank over microseconds.
pub fn page_percentiles(waits: &[f64]) -> Result<(f64, f64), Failure> {
    let micros: Vec<i64> = waits.iter().map(|wait| micros_of(*wait)).collect();
    let rank = |p: usize| -> Result<f64, Failure> {
        let found = nearest_rank(&micros, p).map_err(|error| error.to_string())?;
        Ok(found as f64 / 1000.0)
    };
    Ok((rank(50)?, rank(95)?))
}

fn side_word(inside: bool) -> &'static str {
    if inside {
        Verdict::Inside.word()
    } else {
        Verdict::Outside.word()
    }
}

/// The record line of the page stage: its count and percentiles against its line.
pub fn page_stage_text(waits: &[f64]) -> Result<String, Failure> {
    let (p50, p95) = page_percentiles(waits)?;
    Ok(format!(
        "L_pg: n {}, p50 {p50:.3} ms (線 {} ms) {}, p95 {p95:.3} ms (線 {} ms) {}",
        waits.len(),
        PAGE_STAGE_LINE.0,
        side_word(p50 <= PAGE_STAGE_LINE.0),
        PAGE_STAGE_LINE.1,
        side_word(p95 <= PAGE_STAGE_LINE.1)
    ))
}

/// The stages of one warm utterance, and the two waits to the page, in whole
/// milliseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wait {
    pub d_ms: i64,
    pub dec_ms: i64,
    pub tr_ms: i64,
    /// `L_page = L + L_pg`.
    pub l_page_ms: i64,
    /// `L_settled_page = (t_settled - t_end) + L_pg`.
    pub l_settled_page_ms: i64,
}

/// The lines of the whole wait, `(p50, p95)` in milliseconds.
const L_PAGE_LINE: (i64, i64) = (750, 1300);
const L_SETTLED_PAGE_LINE: (i64, i64) = (1100, 1700);
/// The line of the largest cold `L_page`.
const COLD_L_PAGE_LINE: i64 = 2500;

fn percentiles_ms(values: &[i64]) -> Result<(i64, i64), Failure> {
    let rank = |p: usize| nearest_rank(values, p).map_err(|error| error.to_string());
    Ok((rank(50)?, rank(95)?))
}

/// One record line of a stage or a wait, and the sides of a line it is outside of.
fn line_of(
    name: &str,
    values: &[i64],
    line: Option<(i64, i64)>,
    outside: &mut Vec<String>,
) -> Result<String, Failure> {
    let (p50, p95) = percentiles_ms(values)?;
    let Some((line50, line95)) = line else {
        return Ok(format!(
            "{name}: n {}, p50 {p50} ms, p95 {p95} ms\n",
            values.len()
        ));
    };
    for (percent, value, limit) in [(50, p50, line50), (95, p95, line95)] {
        if value > limit {
            outside.push(format!(
                "`{name}` の p{percent} が {value} ms で線 {limit} ms の外"
            ));
        }
    }
    Ok(format!(
        "{name}: n {}, p50 {p50} ms (線 {line50} ms) {}, p95 {p95} ms (線 {line95} ms) {}\n",
        values.len(),
        side_word(p50 <= line50),
        side_word(p95 <= line95)
    ))
}

/// The record of the warm stages (a stage outside its line is only written),
/// and the sides of the whole-wait lines the waits are outside of.
fn budget_record(warm: &[Wait], cold_l_page_ms: &[i64]) -> Result<(String, Vec<String>), Failure> {
    if warm.is_empty() {
        return Err(String::from(
            "温まった標本 0: the budget cannot be judged without a warm sample",
        ));
    }
    let column = |pick: fn(&Wait) -> i64| -> Vec<i64> { warm.iter().map(pick).collect() };
    let (mut ignored, mut outside) = (Vec::new(), Vec::new());
    let mut record = String::new();
    for (stage, values) in [
        (Stage::Wait, column(|w| w.d_ms)),
        (Stage::Decode, column(|w| w.dec_ms)),
        (Stage::Draft, column(|w| w.tr_ms)),
    ] {
        record += &line_of(stage.name(), &values, stage.budget_ms(), &mut ignored)?;
    }
    for (name, values, line) in [
        ("L_page", column(|w| w.l_page_ms), L_PAGE_LINE),
        (
            "L_settled_page",
            column(|w| w.l_settled_page_ms),
            L_SETTLED_PAGE_LINE,
        ),
    ] {
        record += &line_of(name, &values, Some(line), &mut outside)?;
    }
    record += &cold_line_of(cold_l_page_ms, &mut outside);
    Ok((record, outside))
}

/// The record line of the cold samples, with their largest `L_page` against its line.
fn cold_line_of(cold_l_page_ms: &[i64], outside: &mut Vec<String>) -> String {
    let Some(largest) = cold_l_page_ms.iter().max() else {
        return String::from("冷えた標本 0\n");
    };
    if *largest > COLD_L_PAGE_LINE {
        outside.push(format!(
            "冷えた標本の `L_page` の最大が {largest} ms で線 {COLD_L_PAGE_LINE} ms の外"
        ));
    }
    format!(
        "冷えた標本: n {}, `L_page` の最大 {largest} ms (線 {COLD_L_PAGE_LINE} ms) {}\n",
        cold_l_page_ms.len(),
        side_word(*largest <= COLD_L_PAGE_LINE)
    )
}

/// Judges the budget of design 10.13.3: the warm `L_page` and `L_settled_page`
/// against their lines, and the largest cold `L_page` against its line. The
/// record is returned when all are inside; a stage outside its own line is
/// written in it and does not fail the run. With no warm sample nothing was
/// measured and the answer is an error.
pub fn judge_budget(warm: &[Wait], cold_l_page_ms: &[i64]) -> Result<String, Failure> {
    let (record, outside) = budget_record(warm, cold_l_page_ms)?;
    if outside.is_empty() {
        Ok(record)
    } else {
        Err(outside.join("; "))
    }
}

/// The waits of a run put together from its stages (BR-1) and its page stamps (BR-2).
#[derive(Debug)]
pub struct Joined {
    pub warm: Vec<Wait>,
    pub cold_l_page_ms: Vec<i64>,
    /// `L_pg` in milliseconds of the utterances that were joined.
    pub page_waits: Vec<f64>,
    /// The utterances left out, each with the reason.
    pub lacking: Vec<String>,
}

/// The wait of one utterance, or the reason it has none.
fn wait_of(row: &StageRow, stamps: &[PageStamp]) -> Result<(Wait, f64), Failure> {
    let id = row.final_id.as_deref().ok_or("its final has no id")?;
    let stamp = stamps
        .iter()
        .find(|stamp| stamp.id == id)
        .ok_or_else(|| format!("{id}: the page recorded no translation"))?;
    let pg = page_wait(stamp)?;
    let pg_us = micros_of(pg);
    let need = |value: Option<i64>, what: &str| value.ok_or_else(|| format!("{id}: no {what}"));
    Ok((
        Wait {
            d_ms: floor_ms(need(row.d_us, "d")?),
            dec_ms: floor_ms(need(row.dec_us, "L_dec")?),
            tr_ms: floor_ms(need(row.tr_us, "L_tr")?),
            l_page_ms: floor_ms(need(row.l_us, "L")? + pg_us),
            l_settled_page_ms: floor_ms(need(row.settled_wait_us, "t_settled")? + pg_us),
        },
        pg,
    ))
}

/// Puts the page stamps on the utterances of a report by the `id` of the
/// closing `final`. An utterance the page has no usable stamp for is listed in
/// `lacking` with the reason and is in no percentile.
pub fn join_page(report: &StageReport, stamps: &[PageStamp]) -> Result<Joined, Failure> {
    let mut joined = Joined {
        warm: Vec::new(),
        cold_l_page_ms: Vec::new(),
        page_waits: Vec::new(),
        lacking: Vec::new(),
    };
    for (row, is_cold) in report
        .warm
        .iter()
        .map(|row| (row, false))
        .chain(report.cold.iter().map(|row| (row, true)))
    {
        match wait_of(row, stamps) {
            Ok((wait, pg)) if is_cold => {
                joined.cold_l_page_ms.push(wait.l_page_ms);
                joined.page_waits.push(pg);
            }
            Ok((wait, pg)) => {
                joined.warm.push(wait);
                joined.page_waits.push(pg);
            }
            Err(reason) => joined
                .lacking
                .push(format!("repeat {} utt {}: {reason}", row.repeat, row.utt)),
        }
    }
    Ok(joined)
}

/// The warm utterances of the report that were spoken in `lang`, as a report of
/// their own.
pub fn spoken_in(report: &StageReport, lang: &str) -> StageReport {
    StageReport {
        cold: Vec::new(),
        warm: report
            .warm
            .iter()
            .filter(|row| row.lang.as_deref() == Some(lang))
            .cloned()
            .collect(),
    }
}

/// `L_tr` of the warm utterances of each language of the pair (BR-4): how many
/// there are and their percentiles, or the words that say there is none.
pub fn draft_by_language_text(report: &StageReport) -> Result<String, Failure> {
    let mut text = String::new();
    for lang in ["ja", "en"] {
        let found = spoken_in(report, lang)
            .stat(Stage::Draft)
            .map_err(|error| error.to_string())?;
        text += &match found {
            Some(stat) => format!(
                "L_tr {lang}: n {}, p50 {} ms, p95 {} ms\n",
                stat.n, stat.p50_ms, stat.p95_ms
            ),
            None => format!("L_tr {lang}: 温まった標本が 0 件で、値は無い\n"),
        };
    }
    Ok(text)
}

/// The record of a real run: what the page was told, the stages of BR-1, the
/// page stage and the whole waits, each against its line, and what was left
/// out. It judges nothing. `title` names the step the values were taken at.
pub fn render_baseline(
    title: &str,
    report: &StageReport,
    joined: &Joined,
) -> Result<String, Failure> {
    let mut text = format!(
        "# {title}\n\n設計 `docs/design/conversation_subtitles.ja.md` の 10.13.3（遅れの予算と測り方）と 10.13.7（受入 ID BR-3、T-BR-03-001）の線に対する値。合否は付けない。\n\n",
    );
    text += "## 段（BR-1。温まった標本は閉じる final を持つ発話の全部）\n\n";
    text += &report.render();
    text += "\n## 速報の訳 L_tr を言語で分けた値（BR-4。温まった標本）\n\n";
    text += &draft_by_language_text(report)?;
    text += "\n## 頁の段と頁までの遅れ（BR-2 の頁の時計と BR-1 を結んだ発話だけ）\n\n";
    text += &format!(
        "結べた発話 {}、結べなかった発話 {}\n\n",
        joined.page_waits.len(),
        joined.lacking.len()
    );
    if !joined.page_waits.is_empty() {
        text += &page_stage_text(&joined.page_waits)?;
        text.push('\n');
    }
    match budget_record(&joined.warm, &joined.cold_l_page_ms) {
        Ok((record, _)) => text += &record,
        Err(reason) => text += &format!("{reason}\n"),
    }
    for reason in &joined.lacking {
        text += &format!("結べなかった: {reason}\n");
    }
    Ok(text)
}

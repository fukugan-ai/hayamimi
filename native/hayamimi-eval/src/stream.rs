//! `stream`: plays a long recording into `/ingest` at real-time pace,
//! several times over, and measures how late each utterance's `final` and
//! translation arrive.
//!
//! Writes three files next to `--out`: the per-utterance samples (`.jsonl`,
//! one line per repetition and utterance), every event received with its
//! arrival time (`.events.jsonl`), and the summary (`.summary.json`, also
//! printed on stdout).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::error::EvalError;
use crate::ingest::{self, Plan, sent_at};
use crate::latency::{Event, Sample, Summary, Window, build_samples, parse_event, summarize};
use crate::log;
use crate::manifest::{read_rows, write_text};
use crate::relay_events::{Watch, in_time_order};
use crate::wav::{SAMPLE_RATE, read_wav};

pub struct Options {
    pub host: String,
    pub port: u16,
    /// The relay's `/events` port, when its translations are read too.
    pub relay_port: Option<u16>,
    pub wav: PathBuf,
    pub manifest: PathBuf,
    pub repeat: u32,
    pub out: PathBuf,
    /// Silence after each copy of the recording, in milliseconds.
    pub gap_ms: u32,
    /// How long to keep listening after the last sample is sent.
    pub drain_s: u32,
}

/// What a run produced.
#[derive(Debug)]
pub struct Report {
    pub samples: Vec<Sample>,
    pub summary: Summary,
    pub events_received: usize,
    /// Events that were not JSON the tool understood.
    pub events_unparsed: usize,
}

/// A recording's utterances: sample spans and reference texts.
#[derive(Debug, PartialEq, Eq)]
pub struct Utterances {
    pub spans: Vec<(usize, usize)>,
    pub references: Vec<String>,
}

type JsonRow = serde_json::Map<String, Value>;

fn sample_position(row: &JsonRow, key: &str, index: usize) -> Result<usize, EvalError> {
    row.get(key)
        .and_then(Value::as_u64)
        .and_then(|v| usize::try_from(v).ok())
        .ok_or_else(|| EvalError::Manifest(format!("row {index} has no sample position `{key}`")))
}

/// Reads the utterance spans and references from a joined recording's
/// manifest and checks them against the recording's length: every span is
/// non-empty, inside the recording, and after the one before it.
pub fn utterances_of(rows: &[JsonRow], audio_len: usize) -> Result<Utterances, EvalError> {
    let mut spans = Vec::with_capacity(rows.len());
    let mut references = Vec::with_capacity(rows.len());
    let mut previous_end = 0;
    for (index, row) in rows.iter().enumerate() {
        let start = sample_position(row, "start_sample", index)?;
        let end = sample_position(row, "end_sample", index)?;
        if start >= end || end > audio_len || start < previous_end {
            return Err(EvalError::Manifest(format!(
                "row {index}: span {start}..{end} is empty, overlaps the one before, or passes {audio_len}"
            )));
        }
        let Some(reference) = row.get("ref").and_then(Value::as_str) else {
            return Err(EvalError::Manifest(format!(
                "row {index} has no string `ref`"
            )));
        };
        spans.push((start, end));
        references.push(reference.to_owned());
        previous_end = end;
    }
    if spans.is_empty() {
        return Err(EvalError::Manifest(String::from(
            "the manifest has no rows",
        )));
    }
    Ok(Utterances { spans, references })
}

/// The window of each utterance in each copy, from the chunk send times.
pub fn windows_for(
    plan: &Plan,
    sent_us: &[i64],
    spans: &[(usize, usize)],
    repeat: u32,
) -> Result<Vec<Window>, EvalError> {
    let mut windows = Vec::with_capacity(spans.len() * repeat as usize);
    for copy in 0..repeat {
        let base = copy as usize * plan.period;
        for (index, (start, end)) in spans.iter().enumerate() {
            let (Some(t_start_us), Some(t_end_us)) = (
                sent_at(plan, sent_us, base + start, false),
                sent_at(plan, sent_us, base + end, true),
            ) else {
                return Err(EvalError::Invalid(format!(
                    "no chunk edge at the utterance {} of copy {}",
                    index + 1,
                    copy + 1
                )));
            };
            windows.push(Window {
                repeat: copy + 1,
                utt: index as u32 + 1,
                t_start_us,
                t_end_us,
            });
        }
    }
    Ok(windows)
}

/// `path` with `suffix` in place of its extension: `run_1.jsonl` and
/// `events.jsonl` give `run_1.events.jsonl`.
pub fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    path.with_extension(suffix)
}

fn write_lines(
    lines: impl Iterator<Item = Result<String, serde_json::Error>>,
    path: &Path,
) -> Result<(), EvalError> {
    let mut text = String::new();
    for line in lines {
        text.push_str(&line.map_err(|e| EvalError::json(path, e))?);
        text.push('\n');
    }
    write_text(path, &text)
}

/// One received event as a result-file line: its arrival time and its JSON,
/// or its raw text when it was not JSON.
fn raw_event_line(t_us: i64, text: &str) -> Result<String, serde_json::Error> {
    let event = match serde_json::from_str::<Value>(text) {
        Ok(parsed) => parsed,
        // DEFAULT: the file records what arrived; text that is not JSON is kept verbatim as a string, nothing is invented.
        Err(_) => Value::String(text.to_owned()),
    };
    serde_json::to_string(&serde_json::json!({ "t_us": t_us, "event": event }))
}

/// Opens the ingest connection of a run. A refused connection is a
/// `Connect` error before any file is read or written.
pub fn open_ingest(host: &str, port: u16, origin: Instant) -> Result<ingest::Ingest, EvalError> {
    ingest::open(host, port, origin)
}

/// Plays the recording and measures it.
pub fn run(options: &Options) -> Result<Report, EvalError> {
    let audio = read_wav(&options.wav)?;
    if audio.sample_rate != SAMPLE_RATE {
        return Err(EvalError::Wav {
            path: options.wav.display().to_string(),
            message: format!("sample rate {} is not {SAMPLE_RATE}", audio.sample_rate),
        });
    }
    let rows = read_rows(&options.manifest)?;
    let utterances = utterances_of(&rows, audio.samples.len())?;
    let marks: Vec<usize> = utterances
        .spans
        .iter()
        .flat_map(|(s, e)| [*s, *e])
        .collect();
    let gap = options.gap_ms as usize * (SAMPLE_RATE / 1000) as usize;
    let plan = ingest::plan(audio.samples.len(), &marks, options.repeat, gap);

    let origin = Instant::now();
    let mut connection = open_ingest(&options.host, options.port, origin)?;
    let relay = match options.relay_port {
        Some(port) => Some(Watch::open(&options.host, port, origin)?),
        None => None,
    };
    connection.start()?;
    log::info(
        "eval.stream.started",
        "worker",
        "playing the recording",
        &[
            ("repeat", options.repeat.into()),
            ("utterances", utterances.spans.len().into()),
            ("chunks", plan.chunks.len().into()),
        ],
    );
    let sent_us = connection.play(&plan, &audio.samples)?;
    let received = connection.finish(Duration::from_secs(u64::from(options.drain_s)))?;
    let heard = match &relay {
        Some(watch) => in_time_order(received.events, watch.events()),
        None => received.events,
    };

    let mut events: Vec<Event> = Vec::with_capacity(heard.len());
    let mut unparsed = 0;
    for (t_us, text) in &heard {
        match parse_event(*t_us, text) {
            Ok(event) => events.push(event),
            Err(error) => {
                unparsed += 1;
                log::warn(
                    "eval.stream.event_unparsed",
                    "worker",
                    &error.to_string(),
                    &[],
                );
            }
        }
    }
    let windows = windows_for(&plan, &sent_us, &utterances.spans, options.repeat)?;
    let samples = build_samples(&windows, &utterances.references, &events)?;
    let summary = summarize(&samples)?;

    write_lines(samples.iter().map(serde_json::to_string), &options.out)?;
    let raw_path = sidecar(&options.out, "events.jsonl");
    write_lines(
        heard.iter().map(|(t_us, text)| raw_event_line(*t_us, text)),
        &raw_path,
    )?;
    let summary_path = sidecar(&options.out, "summary.json");
    let summary_text =
        serde_json::to_string_pretty(&summary).map_err(|e| EvalError::json(&summary_path, e))?;
    write_text(&summary_path, &format!("{summary_text}\n"))?;
    println!("{summary_text}");
    log::info(
        "eval.stream.finished",
        "worker",
        "stream finished",
        &[
            ("samples", samples.len().into()),
            ("with_final", summary.with_final.into()),
            ("with_translation", summary.with_translation.into()),
            ("events", heard.len().into()),
            ("events_unparsed", unparsed.into()),
            ("closed_by_server", received.closed_by_server.into()),
        ],
    );
    Ok(Report {
        samples,
        summary,
        events_received: heard.len(),
        events_unparsed: unparsed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::plan as make_plan;
    use serde_json::json;

    fn row(start: u64, end: u64) -> JsonRow {
        match json!({ "start_sample": start, "end_sample": end, "ref": "あ" }) {
            Value::Object(map) => map,
            _ => JsonRow::new(),
        }
    }

    #[test]
    fn spans_and_references_are_read_in_order() -> Result<(), EvalError> {
        let found = utterances_of(&[row(0, 100), row(200, 300)], 300)?;
        assert_eq!(found.spans, vec![(0, 100), (200, 300)]);
        assert_eq!(found.references, vec!["あ", "あ"]);
        Ok(())
    }

    #[test]
    fn an_empty_span_is_refused() {
        assert!(matches!(
            utterances_of(&[row(5, 5)], 100),
            Err(EvalError::Manifest(_))
        ));
    }

    #[test]
    fn a_span_past_the_recording_is_refused() {
        assert!(matches!(
            utterances_of(&[row(0, 101)], 100),
            Err(EvalError::Manifest(_))
        ));
        assert!(utterances_of(&[row(0, 100)], 100).is_ok());
    }

    #[test]
    fn overlapping_spans_are_refused() {
        assert!(matches!(
            utterances_of(&[row(0, 100), row(99, 150)], 200),
            Err(EvalError::Manifest(_))
        ));
        assert!(utterances_of(&[row(0, 100), row(100, 150)], 200).is_ok());
    }

    #[test]
    fn a_row_without_positions_is_refused() {
        assert!(matches!(
            utterances_of(&[JsonRow::new()], 100),
            Err(EvalError::Manifest(_))
        ));
    }

    #[test]
    fn no_rows_are_refused() {
        assert!(matches!(
            utterances_of(&[], 100),
            Err(EvalError::Manifest(_))
        ));
    }

    #[test]
    fn windows_follow_the_send_times_of_each_copy() -> Result<(), EvalError> {
        let plan = make_plan(1_000, &[100, 900], 2, 200);
        let sent: Vec<i64> = plan.chunks.iter().map(|c| c.end as i64 * 10).collect();
        let windows = windows_for(&plan, &sent, &[(100, 900)], 2)?;
        assert_eq!(windows.len(), 2);
        assert_eq!((windows[0].repeat, windows[0].utt), (1, 1));
        assert_eq!(windows[0].t_end_us, 9_000);
        assert_eq!(windows[1].t_end_us, 1_200 * 10 + 9_000);
        assert!(windows[0].t_start_us < windows[0].t_end_us);
        Ok(())
    }

    #[test]
    fn a_span_with_no_chunk_edge_is_refused() {
        let plan = make_plan(1_000, &[], 1, 0);
        assert!(matches!(
            windows_for(&plan, &[0], &[(10, 20)], 1),
            Err(EvalError::Invalid(_))
        ));
    }

    #[test]
    fn a_sidecar_replaces_the_extension() {
        assert_eq!(
            sidecar(Path::new("base/lat_ja_0.35_1.jsonl"), "events.jsonl"),
            PathBuf::from("base/lat_ja_0.35_1.events.jsonl")
        );
    }

    #[test]
    fn a_raw_event_line_keeps_json_and_wraps_other_text() -> Result<(), serde_json::Error> {
        let json_line: Value = serde_json::from_str(&raw_event_line(5, r#"{"type":"ready"}"#)?)?;
        assert_eq!(json_line["event"]["type"], "ready");
        let text_line: Value = serde_json::from_str(&raw_event_line(6, "plain")?)?;
        assert_eq!(text_line["event"], "plain");
        Ok(())
    }

    #[test]
    fn a_closed_port_is_a_connect_error_before_anything_is_written() -> Result<(), std::io::Error> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        drop(listener);
        assert!(matches!(
            open_ingest("127.0.0.1", port, Instant::now()),
            Err(EvalError::Connect { .. })
        ));
        Ok(())
    }
}

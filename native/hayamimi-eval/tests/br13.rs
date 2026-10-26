//! BR-13: the `final`s of a second run into one session are placed on the
//! windows of that run.
//!
//! A `final` carries `t1_s` on the server's stream clock, which keeps going
//! when a second client connects; the windows of a run are on the tool's
//! clock, which starts with the run. The `ready` of a connection says where the
//! stream clock stood (`stream_s`), so the tool puts `t1_s` on its own clock by
//! the arrival of `ready` minus that position (design 10.13.13, 10).
//!
//! Record C is the second run: no `session_start`, `ready` at 50 ms with
//! `stream_s` 30.904. Record D is the first-run form: `session_start` at
//! 50 ms and no `stream_s`.

mod support;

use hayamimi_eval::latency::{
    Sample, build_samples, read_record, samples_of_record, summarize_run,
};
use serde_json::{Value, json};
use support::{TestResult, event_line, samples_text, window_sample};

const WINDOWS: [(i64, i64); 4] = [
    (1_000_000, 3_000_000),
    (6_000_000, 8_000_000),
    (11_000_000, 13_000_000),
    (16_000_000, 18_000_000),
];
const LANGS: [&str; 4] = ["ja", "en", "ja", "en"];
const TEXTS: [&str; 4] = ["t1", "t2", "t3", "t4"];
const FIRST_RUN_SECONDS: f64 = 30.854;
const STREAM_S: f64 = 30.904;
const READY_AT_US: i64 = 50_000;

/// What the two files of a record hold, built by hand.
struct Record {
    /// `stream_s` of the `ready` frame, when it says.
    stream_s: Option<f64>,
    /// Whether a `session_start` stands at the arrival of `ready`.
    session_start: bool,
    /// Seconds added to the `t1_s` of every `final`.
    t1_shift_s: f64,
    /// Whether the `final` of the first utterance is left out.
    without_first_final: bool,
    /// Whether any frame about the stream clock is written at all.
    clock_frames: bool,
}

impl Record {
    /// Record C.
    fn second_run() -> Record {
        Record {
            stream_s: Some(STREAM_S),
            session_start: false,
            t1_shift_s: 0.0,
            without_first_final: false,
            clock_frames: true,
        }
    }

    /// Record D.
    fn first_run() -> Record {
        Record {
            stream_s: None,
            session_start: true,
            t1_shift_s: -STREAM_S,
            without_first_final: false,
            clock_frames: true,
        }
    }

    fn samples(&self) -> Result<String, String> {
        let windows: Vec<Sample> = (1..=4_u32)
            .zip(WINDOWS)
            .map(|(utt, (start, end))| window_sample(1, utt, start, end))
            .collect();
        samples_text(&windows)
    }

    fn ready(&self) -> Value {
        match self.stream_s {
            Some(position) => json!({"type": "ready", "sr": 16000, "stream_s": position}),
            None => json!({"type": "ready", "sr": 16000}),
        }
    }

    fn final_of(&self, at: usize, end: i64) -> Result<String, String> {
        let (Some(lang), Some(text)) = (LANGS.get(at), TEXTS.get(at)) else {
            return Err(format!("no utterance {at}"));
        };
        let id = format!("file-{}", at + 1);
        let t1_s = end as f64 / 1e6 + FIRST_RUN_SECONDS + self.t1_shift_s;
        Ok(event_line(
            end + 400_000,
            json!({"type": "final", "id": id, "lang": lang, "text": text,
                   "latency_ms": 300.0, "t1_s": t1_s}),
        ))
    }

    fn events(&self) -> Result<String, String> {
        let mut lines = Vec::new();
        if self.clock_frames {
            lines.push(event_line(READY_AT_US, self.ready()));
            if self.session_start {
                lines.push(event_line(READY_AT_US, json!({"type": "session_start"})));
            }
        }
        for (at, (_, end)) in WINDOWS.iter().enumerate() {
            if at == 0 && self.without_first_final {
                continue;
            }
            lines.push(self.final_of(at, *end)?);
        }
        Ok(lines.join("\n") + "\n")
    }
}

fn references() -> Vec<String> {
    TEXTS.iter().map(|text| (*text).to_owned()).collect()
}

fn blank_references() -> Vec<String> {
    vec![String::new(); 4]
}

/// How many `final`s each window was given, and in how many windows one
/// closed the utterance.
fn counts(record: &Record, references: &[String]) -> Result<(Vec<u32>, usize), String> {
    let (samples_file, events_file) = (record.samples()?, record.events()?);
    let samples = samples_of_record(&samples_file, &events_file, references)
        .map_err(|error| error.to_string())?;
    let (_, events) =
        read_record(&samples_file, &events_file).map_err(|error| error.to_string())?;
    let summary = summarize_run(&samples, &events).map_err(|error| error.to_string())?;
    Ok((
        samples.iter().map(|sample| sample.finals).collect(),
        summary.with_final,
    ))
}

/// T-BR-13-001
#[test]
fn br13_a_second_stream_into_one_session_places_its_finals_on_its_own_windows() -> TestResult {
    let (finals, with_final) = counts(&Record::second_run(), &blank_references())?;
    assert_eq!(finals, [1, 1, 1, 1]);
    assert_eq!(with_final, 4);
    Ok(())
}

/// T-BR-13-002
#[test]
fn a_first_stream_with_a_session_start_is_still_placed_as_before() -> TestResult {
    let (finals, with_final) = counts(&Record::first_run(), &blank_references())?;
    assert_eq!(finals, [1, 1, 1, 1]);
    assert_eq!(with_final, 4);
    Ok(())
}

/// T-BR-13-003
#[test]
fn without_a_new_origin_the_four_finals_fall_on_the_last_window() -> TestResult {
    let record = Record::second_run();
    let (windows, events) = read_record(&record.samples()?, &record.events()?)?;
    let samples = build_samples(&windows, &blank_references(), &events)?;
    let finals: Vec<u32> = samples.iter().map(|sample| sample.finals).collect();
    assert_eq!(finals, [0, 0, 0, 4]);
    assert_eq!(summarize_run(&samples, &events)?.with_final, 1);
    Ok(())
}

/// T-BR-13-004
#[test]
fn a_third_stream_into_one_session_places_its_finals_on_its_own_windows() -> TestResult {
    let record = Record {
        stream_s: Some(STREAM_S + FIRST_RUN_SECONDS),
        t1_shift_s: FIRST_RUN_SECONDS,
        ..Record::second_run()
    };
    let (finals, _) = counts(&record, &blank_references())?;
    assert_eq!(finals, [1, 1, 1, 1]);
    Ok(())
}

/// T-BR-13-005
#[test]
fn the_summary_of_a_second_stream_has_no_character_errors_when_the_texts_agree() -> TestResult {
    let record = Record::second_run();
    let samples = samples_of_record(&record.samples()?, &record.events()?, &references())?;
    assert_eq!(
        samples.iter().map(|sample| sample.cer_dist).sum::<usize>(),
        0
    );
    assert!(samples.iter().all(|sample| sample.cer_denom > 0));
    Ok(())
}

/// T-BR-13-006
#[test]
fn a_ready_without_stream_s_is_placed_by_the_session_start_as_before() -> TestResult {
    let by_session_start = Record {
        stream_s: None,
        session_start: true,
        t1_shift_s: -STREAM_S,
        ..Record::second_run()
    };
    assert_eq!(
        counts(&by_session_start, &blank_references())?.0,
        [1, 1, 1, 1]
    );
    let by_neither = Record {
        stream_s: None,
        session_start: false,
        clock_frames: false,
        ..Record::second_run()
    };
    assert_eq!(counts(&by_neither, &blank_references())?.0, [0, 0, 0, 4]);
    Ok(())
}

/// T-BR-13-007
#[test]
fn the_origin_does_not_depend_on_the_first_final_of_the_connection() -> TestResult {
    let record = Record {
        without_first_final: true,
        ..Record::second_run()
    };
    assert_eq!(counts(&record, &blank_references())?.0, [0, 1, 1, 1]);
    Ok(())
}

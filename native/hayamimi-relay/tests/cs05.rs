//! CS-5: a line whose translation fails verification is withheld. Its event
//! says so and carries no text, nothing is sent to the outside, and the
//! subtitle file holds the source text under the mark. A line that passes is
//! sent once.
//!
//! The translator is fake and changes the amount in the first line, in both
//! of its answers (T and E), so that V has nothing to keep.

mod support;

use std::sync::Arc;
use std::time::Duration;

use hayamimi_relay::deliver::Caption;
use hayamimi_relay::lang::Lang;
use hayamimi_relay::llm::{Completion, Llm, LlmError};
use hayamimi_relay::logging::LogSink;
use hayamimi_relay::pipeline::{Pipeline, PipelineConfig};
use hayamimi_relay::relay::{Lanes, MakePipeline};
use hayamimi_relay::strings::Strings;
use serde_json::{Value, json};
use support::TestResult;
use support::events::{
    Buffer, FakeRecognizer, Recording, drafts_of, outputs, run_relay_with, translations_of,
};

const SOON: Duration = Duration::from_secs(10);
/// An amount of five million yen, spoken.
const BUDGET: &str = "予算は五百万円を想定しています。";
const WRONG: &str = "The budget is assumed to be 1,500,000 yen.";
const GREETING: &str = "こんにちは。";

/// Answers every request about the budget with the wrong amount, and every
/// other request with a greeting.
struct ChangesTheAmount;

impl Llm for ChangesTheAmount {
    fn complete(&self, request: &Completion<'_>) -> Result<String, LlmError> {
        let answer = if request.prompt.contains("予算") {
            WRONG
        } else {
            "Hello."
        };
        Ok(answer.to_owned())
    }
}

fn final_event(seq: usize, text: &str) -> String {
    json!({
        "type": "final", "id": format!("mic-{seq}"), "source": "mic", "lang": "ja",
        "text": text, "t0_s": 0.5, "t1_s": 2.0, "end_wall_ms": 1_790_000_000_000_u64,
    })
    .to_string()
}

/// What a check of the captions sent outside refuses.
#[derive(Debug, PartialEq, Eq)]
enum Fault {
    SentForAHeldLine(String),
    MissingFor(String),
}

/// No caption may be sent for a held line, and one must be sent for each line
/// that passed.
fn check_sent(sent: &[Caption], held: &[&str], passed: &[&str]) -> Result<(), Fault> {
    if let Some(caption) = sent
        .iter()
        .find(|caption| held.contains(&caption.id.as_str()))
    {
        return Err(Fault::SentForAHeldLine(caption.id.clone()));
    }
    for id in passed {
        if !sent.iter().any(|caption| caption.id == *id) {
            return Err(Fault::MissingFor((*id).to_owned()));
        }
    }
    Ok(())
}

struct Run {
    arrived: Vec<(std::time::Instant, Value)>,
    sent: Vec<Caption>,
    srt: String,
}

fn run() -> Result<Run, Box<dyn std::error::Error>> {
    let recognizer = FakeRecognizer::start()?;
    let make: MakePipeline<ChangesTheAmount> = Arc::new(|sink: &mut dyn LogSink| {
        Pipeline::open(ChangesTheAmount, &PipelineConfig::default(), sink)
    });
    let (recording, file) = (Recording::new(true), Buffer::default());
    let destinations =
        vec![Box::new(recording.clone()) as Box<dyn hayamimi_relay::deliver::Destination>];
    let outputs = outputs(destinations, Some(Box::new(file.clone())))?;
    let running = run_relay_with(vec![recognizer.url()], Lanes::PerSource, make, outputs)?;
    recognizer.send(&final_event(1, BUDGET));
    recognizer.send(&final_event(2, GREETING));
    let arrived = running.translations.wait_for(2, SOON);
    // The courier works behind the stream; wait for the second cue.
    let deadline = std::time::Instant::now() + SOON;
    while file.text().matches("-->").count() < 2 && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    Ok(Run {
        arrived,
        sent: recording.sent(),
        srt: file.text(),
    })
}

fn settled<'a>(run: &'a Run, id: &str) -> Result<&'a Value, String> {
    translations_of(&run.arrived)
        .into_iter()
        .map(|(_, value)| value)
        .find(|value| value["id"] == id)
        .ok_or_else(|| format!("{id} was never settled"))
}

#[test]
fn cs05_held_line_shows_marked_source_and_sends_nothing() -> TestResult {
    let run = run()?;
    // The held line: an event with the reason and no text, so that a screen
    // can only show the source under its mark.
    let held = settled(&run, "mic-1")?;
    assert_eq!(held["status"], "held");
    assert_eq!(held["hold_reason"], "number");
    assert_eq!(held["text"], "");
    assert_eq!(held["annotations"], json!([]));
    // Neither translation passed, so no draft was shown for it.
    assert!(
        drafts_of(&run.arrived)
            .iter()
            .all(|(_, d)| d["id"] != "mic-1")
    );
    // Nothing was sent for it; the line that passed was sent once.
    assert_eq!(check_sent(&run.sent, &["mic-1"], &["mic-2"]), Ok(()));
    assert_eq!(run.sent.len(), 1);
    assert_eq!(settled(&run, "mic-2")?["status"], "ok");
    // The subtitle file holds the source under the mark, then the translation.
    let mark = Strings::embedded(Lang::En)?.withheld_mark;
    let expected = format!(
        "1\n00:00:00,500 --> 00:00:02,000\n{mark} {BUDGET}\n\n2\n00:00:00,500 --> 00:00:02,000\nHello.\n\n"
    );
    assert_eq!(run.srt, expected);
    Ok(())
}

/// The negative controls: a build that sends a held line, or drops a line
/// that passed.
#[test]
fn cs05_sending_a_held_line_or_dropping_a_good_one_is_refused() {
    let caption = |id: &str| Caption {
        id: id.to_owned(),
        text: String::from("x"),
    };
    assert_eq!(
        check_sent(
            &[caption("mic-1"), caption("mic-2")],
            &["mic-1"],
            &["mic-2"]
        ),
        Err(Fault::SentForAHeldLine(String::from("mic-1")))
    );
    assert_eq!(
        check_sent(&[], &["mic-1"], &["mic-2"]),
        Err(Fault::MissingFor(String::from("mic-2")))
    );
}

//! RP-29, the sink scene, run against the relay: the destination outside is
//! unreachable, so the translations stay in the subtitle file and on the
//! stream, the fall is said once (log event and stream event), and when the
//! destination comes back that is said once too.

mod support;

use std::sync::Arc;
use std::time::Duration;

use hayamimi_relay::deliver::Destination;
use hayamimi_relay::llm::{Completion, Llm, LlmError};
use hayamimi_relay::logging::LogSink;
use hayamimi_relay::pipeline::{Pipeline, PipelineConfig};
use hayamimi_relay::relay::{Lanes, MakePipeline};
use serde_json::{Value, json};
use support::TestResult;
use support::events::{
    Buffer, FakeRecognizer, Recording, Running, outputs, run_relay_with, translations_of,
};

const SOON: Duration = Duration::from_secs(10);

struct Greets;

impl Llm for Greets {
    fn complete(&self, _: &Completion<'_>) -> Result<String, LlmError> {
        Ok(String::from("Hello."))
    }
}

fn final_event(seq: usize) -> String {
    json!({
        "type": "final", "id": format!("mic-{seq}"), "source": "mic", "lang": "ja",
        "text": "こんにちは。", "t0_s": 0.5, "t1_s": 2.0, "end_wall_ms": 1_790_000_000_000_u64,
    })
    .to_string()
}

fn fallbacks(running: &Running) -> Vec<Value> {
    running
        .translations
        .snapshot()
        .into_iter()
        .map(|(_, value)| value)
        .filter(|value| value["type"] == "fallback" && value["stage"] == "sink")
        .collect()
}

fn start(
    recording: &Recording,
    file: &Buffer,
) -> Result<(FakeRecognizer, Running), Box<dyn std::error::Error>> {
    let recognizer = FakeRecognizer::start()?;
    let make: MakePipeline<Greets> =
        Arc::new(|sink: &mut dyn LogSink| Pipeline::open(Greets, &PipelineConfig::default(), sink));
    let destinations = vec![Box::new(recording.clone()) as Box<dyn Destination>];
    let outputs = outputs(destinations, Some(Box::new(file.clone())))?;
    let running = run_relay_with(vec![recognizer.url()], Lanes::PerSource, make, outputs)?;
    Ok((recognizer, running))
}

fn wait_cues(file: &Buffer, count: usize) {
    let deadline = std::time::Instant::now() + SOON;
    while file.text().matches("-->").count() < count && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn rp29_unreachable_sink_keeps_translations_locally_and_says_so_once() -> TestResult {
    let (recording, file) = (Recording::new(false), Buffer::default());
    let (recognizer, running) = start(&recording, &file)?;
    for seq in 1..=3 {
        recognizer.send(&final_event(seq));
    }
    running.translations.wait_for(3, SOON);
    wait_cues(&file, 3);
    // The translations are kept: three cues in the file, three settled lines
    // on the stream, nothing reached the destination.
    assert_eq!(file.text().matches("Hello.").count(), 3);
    assert_eq!(translations_of(&running.translations.snapshot()).len(), 3);
    assert!(recording.sent().is_empty());
    // The fall is said once on each channel, however many lines failed.
    let deadline = std::time::Instant::now() + SOON;
    while running.logs.named("translate.send.failed").len() < 3
        && std::time::Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(running.logs.named("sink.fallback.local").len(), 1);
    let said = fallbacks(&running);
    assert_eq!(said.len(), 1, "{said:?}");
    assert_eq!(said[0]["to"], "local");
    assert_eq!(said[0]["reason"], "unreachable");
    assert!(running.logs.named("sink.fallback.cleared").is_empty());
    Ok(())
}

#[test]
fn rp29_recovered_sink_says_cleared_once_and_gets_the_next_line() -> TestResult {
    let (recording, file) = (Recording::new(false), Buffer::default());
    let (recognizer, running) = start(&recording, &file)?;
    recognizer.send(&final_event(1));
    running.translations.wait_for(1, SOON);
    let deadline = std::time::Instant::now() + SOON;
    while running.logs.named("sink.fallback.local").is_empty()
        && std::time::Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(5));
    }
    recording.set_up(true);
    recognizer.send(&final_event(2));
    let deadline = std::time::Instant::now() + SOON;
    while recording.sent().is_empty() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(recording.sent().len(), 1);
    assert_eq!(running.logs.named("sink.fallback.cleared").len(), 1);
    let said = fallbacks(&running);
    assert_eq!(said.len(), 2, "{said:?}");
    assert_eq!(said[1]["to"], Value::Null);
    Ok(())
}

//! CS-25 and RP-25 through the relay: the two paths that decide which
//! translation is settled and which one leaves the machine.
//!
//! - E fails V: the translation of T, which passed, is the settled one, and
//!   it is what the outside destination gets.
//! - A draft handed to the delivery is never sent outside, only the settled
//!   translation is.

mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use hayamimi_relay::deliver::{Delivery, Destination};
use hayamimi_relay::hub::Hub;
use hayamimi_relay::lang::Lang;
use hayamimi_relay::line::parse_line;
use hayamimi_relay::llm::{Completion, Llm, LlmError};
use hayamimi_relay::logging::{LogSink, NullSink, SharedSink};
use hayamimi_relay::pipeline::{NoObserver, Pass, Pipeline, PipelineConfig, Status};
use hayamimi_relay::relay::{Lanes, MakePipeline};
use serde_json::{Value, json};
use support::TestResult;
use support::events::{
    FakeRecognizer, Recording, drafts_of, outputs, run_relay_with, translations_of,
};

const SOON: Duration = Duration::from_secs(10);
/// Five million yen, spoken.
const BUDGET: &str = "予算は五百万円を想定しています。";
const T_ANSWER: &str = "The budget is assumed to be 5,000,000 yen.";
const E_ANSWER: &str = "The budget is assumed to be 5,000,000 yen in total.";
const E_WRONG: &str = "The budget is assumed to be 1,500,000 yen.";

/// Answers T with `t` and E (its system message carries the draft) with `e`.
struct Scripted {
    t: &'static str,
    e: &'static str,
}

impl Llm for Scripted {
    fn complete(&self, request: &Completion<'_>) -> Result<String, LlmError> {
        let answer = if request.system.is_some() {
            self.e
        } else {
            self.t
        };
        Ok(answer.to_owned())
    }
}

fn final_event() -> String {
    json!({
        "type": "final", "id": "mic-1", "source": "mic", "lang": "ja",
        "text": BUDGET, "t0_s": 0.5, "t1_s": 2.0, "end_wall_ms": 1_790_000_000_000_u64,
    })
    .to_string()
}

fn wait_for(done: impl Fn() -> bool) {
    let deadline = Instant::now() + SOON;
    while !done() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn text_of(value: &Value) -> Result<&str, String> {
    value["text"]
        .as_str()
        .ok_or_else(|| format!("no text in {value}"))
}

#[test]
fn cs25_a_held_edit_keeps_the_draft_as_the_settled_translation_and_sends_it() -> TestResult {
    let recognizer = FakeRecognizer::start()?;
    let make: MakePipeline<Scripted> = Arc::new(|sink: &mut dyn LogSink| {
        Pipeline::open(
            Scripted {
                t: T_ANSWER,
                e: E_WRONG,
            },
            &PipelineConfig::default(),
            sink,
        )
    });
    let recording = Recording::new(true);
    let destinations = vec![Box::new(recording.clone()) as Box<dyn Destination>];
    let running = run_relay_with(
        vec![recognizer.url()],
        Lanes::PerSource,
        make,
        outputs(destinations, None)?,
    )?;
    recognizer.send(&final_event());
    let arrived = running.translations.wait_for(1, SOON);
    wait_for(|| !recording.sent().is_empty());
    // The draft was shown, then the same text was settled as T's.
    let drafts = drafts_of(&arrived);
    assert_eq!(drafts.len(), 1);
    assert_eq!(text_of(&drafts[0].1)?, T_ANSWER);
    let settled = translations_of(&arrived);
    assert_eq!(settled.len(), 1);
    let line = &settled[0].1;
    assert_eq!(
        (line["stage"].as_str(), line["status"].as_str()),
        (Some("t"), Some("ok"))
    );
    assert_eq!(text_of(line)?, T_ANSWER);
    // Outside, the same text went once.
    let sent = recording.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].text, T_ANSWER);
    Ok(())
}

#[test]
fn rp25_a_draft_handed_to_the_delivery_is_not_sent_outside() -> TestResult {
    let mut sink = hayamimi_relay::logging::MemorySink::default();
    let mut pipeline = Pipeline::open(
        Scripted {
            t: T_ANSWER,
            e: E_ANSWER,
        },
        &PipelineConfig::default(),
        &mut sink,
    )?;
    let Some(line) = parse_line(&final_event()).map_err(|skip| skip.as_str())? else {
        return Err("the fixture is not a line".into());
    };
    let request = line.request(Lang::Ja, 1_790_000_000);
    let mut drafts = Vec::new();
    let settled = pipeline.run_staged(
        &request,
        &mut NoObserver,
        &mut |draft| drafts.push(draft),
        &mut sink,
    );
    let said: Vec<_> = sink.events.iter().map(|e| (&e.event, &e.fields)).collect();
    assert_eq!(
        (drafts.len(), settled.stage, settled.status),
        (1, Pass::E, Status::Ok),
        "{said:?}"
    );
    assert!(!drafts[0].settled);

    let recording = Recording::new(true);
    let destinations = vec![Box::new(recording.clone()) as Box<dyn Destination>];
    let delivery = Delivery::start(
        outputs(destinations, None)?,
        Hub::new(),
        SharedSink::new(NullSink),
    )?;
    // The courier is in order: a draft that was delivered would be sent first.
    delivery.submit(&line, &drafts[0]);
    delivery.submit(&line, &settled);
    wait_for(|| !recording.sent().is_empty());
    let sent = recording.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].text, E_ANSWER);
    Ok(())
}

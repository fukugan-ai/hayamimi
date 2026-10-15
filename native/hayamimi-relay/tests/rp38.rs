//! RP-38: `eval` records a hedge disagreement once per line, for the
//! translation it keeps, whichever passes of T and E the line went through.
//!
//! A fake llama-server answers every request (T and E alike) with one text.

mod support;

use std::time::Duration;

use hayamimi_relay::eval::{self, EvalOptions};
use hayamimi_relay::llm::HttpLlm;
use hayamimi_relay::logging::MemorySink;
use hayamimi_relay::verify::HEDGE_EVENT;
use support::{FakeServer, Reply, TempDir, TestResult};

const HEDGED: &str = "明日は雨かもしれません。";
/// Drops the hedge of `HEDGED`.
const DROPPED: &str = "明天会下雨。";
/// Keeps it.
const KEPT: &str = "明天可能会下雨。";

/// The hedge events of an `eval` run of one zh sentence that the server
/// answers with `said` to every request.
fn hedge_events_for(said: &str) -> Result<usize, Box<dyn std::error::Error>> {
    let body = support::chat_reply(said);
    let server = FakeServer::start(move |_, _| Reply::json(&body))?;
    let pairs = TempDir::new("rp38-pairs")?;
    std::fs::write(pairs.join("zh.ja.txt"), format!("{HEDGED}\n"))?;
    let out = TempDir::new("rp38-out")?;
    let llm = HttpLlm::with_timeout(&server.base_url(), Duration::from_secs(5));
    let mut sink = MemorySink::default();
    eval::run(
        &llm,
        &llm,
        &EvalOptions {
            pairs: pairs.path(),
            out: out.path(),
        },
        &mut sink,
    )?;
    Ok(sink
        .events
        .iter()
        .filter(|event| event.event == HEDGE_EVENT)
        .count())
}

/// T-RP-38-001: one event for the line.
#[test]
fn rp38_eval_logs_one_hedge_event_per_line() -> TestResult {
    assert_eq!(hedge_events_for(DROPPED)?, 1);
    Ok(())
}

/// T-RP-38-002, the control: a translation whose hedge agrees logs nothing.
#[test]
fn rp38_eval_logs_nothing_when_the_hedge_agrees() -> TestResult {
    assert_eq!(hedge_events_for(KEPT)?, 0);
    Ok(())
}

/// T-RP-38-007: into English, where T and E both pass V, the dropped hedge is
/// recorded once for the settled translation and nothing is held.
#[test]
fn rp38_the_ja_to_en_route_logs_a_dropped_hedge_once_and_does_not_hold() -> TestResult {
    let said = "It will rain tomorrow.";
    let body = support::chat_reply(said);
    let server = FakeServer::start(move |_, _| Reply::json(&body))?;
    let pairs = TempDir::new("rp38-en-pairs")?;
    std::fs::write(pairs.join("en.ja.txt"), format!("{HEDGED}\n"))?;
    let out = TempDir::new("rp38-en-out")?;
    let llm = HttpLlm::with_timeout(&server.base_url(), Duration::from_secs(5));
    let mut sink = MemorySink::default();
    eval::run(
        &llm,
        &llm,
        &EvalOptions {
            pairs: pairs.path(),
            out: out.path(),
        },
        &mut sink,
    )?;
    let count = |name: &str| sink.events.iter().filter(|e| e.event == name).count();
    assert_eq!(count(HEDGE_EVENT), 1, "hedge events");
    assert_eq!(count(eval::HELD_EVENT), 0, "held events");
    let hyp = std::fs::read_to_string(out.join("en.hyp.txt"))?;
    assert_eq!(hyp.lines().next(), Some(said));
    let lang = sink
        .events
        .iter()
        .find(|e| e.event == HEDGE_EVENT)
        .and_then(|e| e.fields.get("lang"));
    assert_eq!(lang, Some(&serde_json::json!("en")));
    Ok(())
}

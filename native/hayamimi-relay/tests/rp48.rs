//! RP-48: into English a hedge that only one side of a line carries does not
//! hold the line; V records it in `translate.hedge.logged` once, for the
//! translation that is settled. Into Japanese the same disagreement still
//! holds for `uncertainty`, and a lost negation or a changed number holds in
//! both directions.
//!
//! A fake llama-server answers every request (T and E alike) with one text, so
//! the text goes through V exactly as the model's answer would.

mod support;

use hayamimi_relay::lang::Lang;
use hayamimi_relay::llm::HttpLlm;
use hayamimi_relay::logging::{Level, LogEvent, MemorySink};
use hayamimi_relay::pipeline::{
    LineKind, LineRequest, NoObserver, Pipeline, PipelineConfig, Status, Translation,
};
use hayamimi_relay::verify::{HEDGE_EVENT, HoldReason};
use serde_json::json;
use support::{FakeServer, Reply, TestResult};

const HEDGED: &str = "明日は雨かもしれません。";
const PLAIN: &str = "明日は雨です。";
const NEGATED_SOURCE: &str = "そこにいませんでした。";

/// What one line came to, every event the run logged and how many requests
/// the server took.
struct Outcome {
    line: Translation,
    events: Vec<LogEvent>,
    requests: usize,
}

/// A line of the live pair against a server that answers `said` to T and E.
fn in_pair(src_lang: Lang, src: &str, said: &str) -> Result<Outcome, Box<dyn std::error::Error>> {
    let body = support::chat_reply(said);
    let server = FakeServer::start(move |_, _| Reply::json(&body))?;
    let mut sink = MemorySink::default();
    let mut pipeline = Pipeline::open(
        HttpLlm::new(&server.base_url()),
        &PipelineConfig::default(),
        &mut sink,
    )?;
    let request = LineRequest {
        id: "rp48-1",
        of: LineKind::Final,
        source: "file",
        src_lang,
        text: src,
        now_epoch: support::epoch(support::DEFAULT_NOW)?,
    };
    let line = pipeline.run(&request, &mut NoObserver, &mut sink);
    Ok(Outcome {
        line,
        events: sink.events,
        requests: server.requests().len(),
    })
}

fn hedge_events(outcome: &Outcome) -> Vec<&LogEvent> {
    outcome
        .events
        .iter()
        .filter(|event| event.event == HEDGE_EVENT)
        .collect()
}

/// The one hedge event of `outcome`: `lang` is `en` and the line's `id`.
fn the_hedge_event(outcome: &Outcome) -> Result<&LogEvent, String> {
    let found = hedge_events(outcome);
    let [event] = found.as_slice() else {
        return Err(format!("{} hedge events, not 1", found.len()));
    };
    if event.fields.get("lang") != Some(&json!("en")) {
        return Err(format!("lang is {:?}", event.fields.get("lang")));
    }
    if event.fields.get("id") != Some(&json!("rp48-1")) {
        return Err(format!("id is {:?}", event.fields.get("id")));
    }
    Ok(event)
}

fn assert_held(outcome: &Outcome, reason: HoldReason, label: &str) {
    assert_eq!(outcome.line.status, Status::Held, "{label}: status");
    assert_eq!(outcome.line.hold_reason, Some(reason), "{label}: reason");
    assert!(hedge_events(outcome).is_empty(), "{label}: events");
}

fn assert_out(outcome: &Outcome, said: &str, label: &str) {
    assert_eq!(
        outcome.line.status,
        Status::Ok,
        "{label}: held for {:?}",
        outcome.line.hold_reason
    );
    assert_eq!(outcome.line.text, said, "{label}: text");
}

/// T-RP-48-001: the four directions of a one-sided hedge.
#[test]
fn rp48_hedge_mismatch_into_english_is_logged_and_into_japanese_held() -> TestResult {
    let said = "It will rain tomorrow.";
    let dropped = in_pair(Lang::Ja, HEDGED, said)?;
    assert_out(&dropped, said, "a");
    let event = the_hedge_event(&dropped)?;
    assert_eq!(event.fields.get("src_hedge"), Some(&json!("かもしれ")));
    assert!(event.fields.get("dst_hedge").is_some_and(|v| v.is_null()));

    let said = "It might rain tomorrow.";
    let added = in_pair(Lang::Ja, PLAIN, said)?;
    assert_out(&added, said, "b");
    let event = the_hedge_event(&added)?;
    assert!(event.fields.get("src_hedge").is_some_and(|v| v.is_null()));
    assert_eq!(event.fields.get("dst_hedge"), Some(&json!("might")));

    let lost = in_pair(Lang::En, "It might rain tomorrow.", "明日は雨が降ります。")?;
    assert_held(&lost, HoldReason::Uncertainty, "c");
    // A hedge only the Japanese translation carries goes out.
    let gained = in_pair(Lang::En, "It will rain tomorrow.", HEDGED)?;
    assert_eq!(gained.line.status, Status::Ok, "d: status");
    Ok(())
}

/// T-RP-48-002: T and E both pass V and the line is recorded once.
#[test]
fn rp48_the_pair_route_logs_once_for_the_settled_translation() -> TestResult {
    let outcome = in_pair(Lang::Ja, HEDGED, "It will rain tomorrow.")?;
    assert_eq!(outcome.requests, 2, "requests");
    assert_eq!(hedge_events(&outcome).len(), 1, "events");
    Ok(())
}

/// T-RP-48-003: a lost negation into English is held.
#[test]
fn rp48_a_lost_negation_into_english_is_still_held() -> TestResult {
    let outcome = in_pair(Lang::Ja, NEGATED_SOURCE, "I was there.")?;
    assert_held(&outcome, HoldReason::Negation, "dropped");
    let kept = in_pair(Lang::Ja, NEGATED_SOURCE, "I was not there.")?;
    assert_out(&kept, "I was not there.", "kept");
    Ok(())
}

/// T-RP-48-004: a changed number into English is held.
#[test]
fn rp48_a_changed_number_into_english_is_still_held() -> TestResult {
    let src = "予算は五百万円です。";
    let wrong = in_pair(Lang::Ja, src, "The budget is seven million yen.")?;
    assert_held(&wrong, HoldReason::Number, "changed");
    let said = "The budget is five million yen.";
    assert_out(&in_pair(Lang::Ja, src, said)?, said, "kept");
    Ok(())
}

/// T-RP-48-005: a negation gap wins over a hedge gap and nothing is logged.
#[test]
fn rp48_a_held_line_into_english_logs_no_hedge_event() -> TestResult {
    let outcome = in_pair(Lang::Ja, "そこにいないかもしれません。", "He is there.")?;
    assert_held(&outcome, HoldReason::Negation, "negation first");
    Ok(())
}

/// T-RP-48-006: agreeing hedges go out and log nothing.
#[test]
fn rp48_agreeing_hedges_into_english_log_nothing() -> TestResult {
    let kept = "It might rain tomorrow.";
    let hedged = in_pair(Lang::Ja, HEDGED, kept)?;
    assert_out(&hedged, kept, "hedged");
    assert!(hedge_events(&hedged).is_empty(), "hedged: events");
    let plain = "It will rain tomorrow.";
    let flat = in_pair(Lang::Ja, PLAIN, plain)?;
    assert_out(&flat, plain, "plain");
    assert!(hedge_events(&flat).is_empty(), "plain: events");
    Ok(())
}

/// T-RP-48-007: several hedge words on one side make one event.
#[test]
fn rp48_several_hedge_words_into_english_log_one_event() -> TestResult {
    let outcome = in_pair(
        Lang::Ja,
        "たぶん明日は雨かもしれません。",
        "It will rain tomorrow.",
    )?;
    assert_out(&outcome, "It will rain tomorrow.", "several");
    let event = the_hedge_event(&outcome)?;
    let word = event.fields.get("src_hedge");
    assert!(
        word == Some(&json!("たぶん")) || word == Some(&json!("かもしれ")),
        "{word:?}"
    );
    Ok(())
}

/// T-RP-48-008: the event is info with four fields and no line text.
#[test]
fn rp48_the_english_hedge_event_carries_only_the_four_fields() -> TestResult {
    let said = "It will rain tomorrow.";
    let outcome = in_pair(Lang::Ja, HEDGED, said)?;
    let event = the_hedge_event(&outcome)?;
    assert_eq!(event.level, Level::Info);
    let keys: Vec<&str> = event.fields.keys().map(String::as_str).collect();
    assert_eq!(keys, ["dst_hedge", "id", "lang", "src_hedge"]);
    let shown = format!("{} {:?}", event.message, event.fields);
    assert!(!shown.contains(HEDGED) && !shown.contains(said), "{shown}");
    Ok(())
}

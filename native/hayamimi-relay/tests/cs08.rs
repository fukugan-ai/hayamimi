//! CS-8: the end-of-sequence marker never reaches a subtitle.
//!
//! A fake llama-server (a TCP server on the loopback address) records every
//! request body and answers with the scripted text.

mod support;

use hayamimi_relay::lang::Lang;
use hayamimi_relay::llm::HttpLlm;
use hayamimi_relay::logging::MemorySink;
use hayamimi_relay::pipeline::{
    LineKind, LineRequest, NoObserver, Pipeline, PipelineConfig, Status, Translation,
};
use hayamimi_relay::verify::HoldReason;
use serde_json::Value;
use support::{FakeServer, Reply, TestResult, request_body};

const REQUEST_FIXTURE: &str = include_str!("fixtures/cs08_request_without_stop.json");
const SOURCE: &str = "こんにちは。";
const EOS: &str = "</s>";

/// What a request check refuses.
#[derive(Debug)]
enum RequestFault {
    NotJson,
    NoStop,
}

fn check_request_has_eos_stop(body: &[u8]) -> Result<(), RequestFault> {
    let value: Value = serde_json::from_slice(body).map_err(|_| RequestFault::NotJson)?;
    if value.get("stop") == Some(&serde_json::json!([EOS])) {
        Ok(())
    } else {
        Err(RequestFault::NoStop)
    }
}

/// A text that must not carry the marker.
fn check_no_eos(text: &str) -> Result<(), String> {
    if text.contains(EOS) {
        Err(format!(
            "the marker is in a subtitle of {} chars",
            text.chars().count()
        ))
    } else {
        Ok(())
    }
}

/// The line the pipeline produced and every request body the server saw.
type Outcome = (Translation, Vec<Vec<u8>>);

/// One line through the pipeline against a server that answers the n-th
/// request with the n-th reply, or with a 500 when the script is spent.
fn run(replies: &[&str]) -> Result<Outcome, Box<dyn std::error::Error>> {
    let script: Vec<String> = replies
        .iter()
        .map(|content| serde_json::json!({ "content": content }).to_string())
        .collect();
    let server = FakeServer::start(move |index, _| match script.get(index) {
        Some(body) => Reply::json(body),
        None => Reply::status(500),
    })?;
    let mut sink = MemorySink::default();
    let mut pipeline = Pipeline::open(
        HttpLlm::new(&server.base_url()),
        &PipelineConfig::default(),
        &mut sink,
    )?;
    let request = LineRequest {
        id: "cs08",
        of: LineKind::Final,
        source: "file",
        src_lang: Lang::Ja,
        text: SOURCE,
        now_epoch: support::epoch(support::DEFAULT_NOW)?,
    };
    let line = pipeline.run(&request, &mut NoObserver, &mut sink);
    Ok((line, server.requests()))
}

fn expect_held_for_form(line: &Translation, id: &str) {
    assert_eq!(line.status, Status::Held, "{id}: status");
    assert_eq!(
        line.hold_reason,
        Some(HoldReason::Form),
        "{id}: hold_reason"
    );
}

/// T-CS-8-001
#[test]
fn cs08_eos_marker_never_reaches_a_subtitle_t_request_carries_stop_eos() -> TestResult {
    let (_, requests) = run(&["Hello.", "Hello."])?;
    let first = requests.first().ok_or("no request reached the server")?;
    assert!(
        check_request_has_eos_stop(request_body(first)).is_ok(),
        "T-CS-8-001"
    );
    Ok(())
}

/// T-CS-8-002
#[test]
fn cs08_eos_marker_never_reaches_a_subtitle_e_request_carries_stop_eos() -> TestResult {
    let (_, requests) = run(&["Hello.", "Hello."])?;
    let second = requests.get(1).ok_or("the second request is missing")?;
    assert!(
        check_request_has_eos_stop(request_body(second)).is_ok(),
        "T-CS-8-002"
    );
    Ok(())
}

/// T-CS-8-003
#[test]
fn cs08_eos_marker_never_reaches_a_subtitle_eos_ending_reply_is_held_for_form() -> TestResult {
    let (line, _) = run(&["Hello.", "Hello.</s>"])?;
    expect_held_for_form(&line, "T-CS-8-003");
    Ok(())
}

/// T-CS-8-004
#[test]
fn cs08_eos_marker_never_reaches_a_subtitle_bare_eos_reply_is_held_for_form() -> TestResult {
    let (line, _) = run(&["Hello.", "</s>"])?;
    expect_held_for_form(&line, "T-CS-8-004");
    Ok(())
}

/// T-CS-8-005
#[test]
fn cs08_eos_marker_never_reaches_a_subtitle_clean_reply_passes() -> TestResult {
    let (line, _) = run(&["Hello.", "Hello."])?;
    assert_eq!(line.status, Status::Ok, "T-CS-8-005: status");
    assert_eq!(line.text, "Hello.", "T-CS-8-005: text");
    Ok(())
}

/// T-CS-8-006
#[test]
fn cs08_eos_marker_never_reaches_a_subtitle_request_without_stop_is_rejected_by_the_check()
-> TestResult {
    let verdict = check_request_has_eos_stop(REQUEST_FIXTURE.as_bytes());
    assert!(
        matches!(verdict, Err(RequestFault::NoStop)),
        "T-CS-8-006: {verdict:?}"
    );
    let (_, requests) = run(&["Hello.", "Hello."])?;
    let first = requests.first().ok_or("no request reached the server")?;
    assert!(
        check_request_has_eos_stop(request_body(first)).is_ok(),
        "T-CS-8-006: real request"
    );
    Ok(())
}

/// T-CS-8-007
#[test]
fn cs08_eos_marker_never_reaches_a_subtitle_no_ok_subtitle_contains_the_marker() -> TestResult {
    let mut ok_texts = Vec::new();
    for edited in ["Hello.</s>", "</s>", "Hello."] {
        let (line, _) = run(&["Hello.", edited])?;
        if line.status == Status::Ok {
            ok_texts.push(line.text);
        }
    }
    assert_eq!(ok_texts.len(), 1, "T-CS-8-007: ok count");
    for text in &ok_texts {
        assert_eq!(text.matches(EOS).count(), 0, "T-CS-8-007: marker count");
        assert!(check_no_eos(text).is_ok(), "T-CS-8-007");
    }
    assert!(
        check_no_eos("Hello.</s>").is_err(),
        "T-CS-8-007: negative control"
    );
    Ok(())
}

/// T-CS-8-008
#[test]
fn cs08_eos_marker_never_reaches_a_subtitle_t_reply_with_eos_is_held_without_asking_e() -> TestResult
{
    let (line, requests) = run(&["Hello.</s>"])?;
    expect_held_for_form(&line, "T-CS-8-008");
    assert_eq!(requests.len(), 1, "T-CS-8-008: requests");
    Ok(())
}

/// T-CS-8-009
#[test]
fn cs08_eos_marker_never_reaches_a_subtitle_t_reply_with_eos_in_the_middle_is_held_without_asking_e()
-> TestResult {
    let (line, requests) = run(&["Hel</s>lo."])?;
    expect_held_for_form(&line, "T-CS-8-009");
    assert_eq!(requests.len(), 1, "T-CS-8-009: requests");
    Ok(())
}

/// T-CS-8-010
#[test]
fn cs08_eos_marker_never_reaches_a_subtitle_e_reply_with_eos_in_the_middle_is_held_for_form()
-> TestResult {
    let (line, requests) = run(&["Hello.", "Hel</s>lo."])?;
    expect_held_for_form(&line, "T-CS-8-010");
    assert_eq!(requests.len(), 2, "T-CS-8-010: requests");
    Ok(())
}

/// T-CS-8-011
#[test]
fn cs08_eos_marker_never_reaches_a_subtitle_e_reply_with_eos_and_trailing_newline_is_held_for_form()
-> TestResult {
    let (line, _) = run(&["Hello.", "Hello.</s>\n"])?;
    expect_held_for_form(&line, "T-CS-8-011");
    Ok(())
}

/// T-CS-8-012
#[test]
fn cs08_eos_marker_never_reaches_a_subtitle_e_reply_with_spaced_eos_is_held_for_form() -> TestResult
{
    let (line, _) = run(&["Hello.", "Hello. </s> "])?;
    expect_held_for_form(&line, "T-CS-8-012");
    Ok(())
}

/// T-CS-8-013
#[test]
fn cs08_eos_marker_never_reaches_a_subtitle_clean_t_reply_lets_e_run() -> TestResult {
    let (line, requests) = run(&["Hello.", "Hello."])?;
    assert_eq!(requests.len(), 2, "T-CS-8-013: requests");
    assert_eq!(line.status, Status::Ok, "T-CS-8-013: status");
    assert_eq!(line.text, "Hello.", "T-CS-8-013: text");
    Ok(())
}

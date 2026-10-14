//! RP-37: into Chinese, Korean and Spanish a hedge that only one side of a
//! line carries does not hold the line; V records it in `translate.hedge.logged`
//! and the translation goes out. Into Japanese and English the same
//! disagreement still holds for `uncertainty`, and a lost negation or a changed
//! number holds into every language.
//!
//! A fake llama-server answers every request with one scripted text, so the
//! text goes through V exactly as the model's answer would.

mod support;

use hayamimi_relay::lang::Lang;
use hayamimi_relay::llm::HttpLlm;
use hayamimi_relay::logging::{LogEvent, MemorySink};
use hayamimi_relay::pipeline::{
    LineKind, LineRequest, NoObserver, Pipeline, PipelineConfig, Status, Translation,
};
use hayamimi_relay::target::Target;
use hayamimi_relay::verify::{HEDGE_EVENT, HoldReason};
use serde_json::json;
use support::{FakeServer, Reply, TestResult};

/// A Japanese source that hedges.
const HEDGED: &str = "明日は雨かもしれません。";
/// A Japanese source that does not.
const PLAIN: &str = "明日は雨です。";

/// Per language: a translation that keeps the hedge and one that drops it.
const KEPT_AND_DROPPED: [(&str, &str, &str); 3] = [
    ("zh", "明天可能会下雨。", "明天会下雨。"),
    ("ko", "내일은 비가 올 수도 있어요.", "내일은 비가 와요."),
    ("es", "Quizás llueva mañana.", "Lloverá mañana."),
];

/// Per language: a translation that adds a hedge the plain source lacks.
const ADDED: [(&str, &str); 3] = [
    ("zh", "明天可能会下雨。"),
    ("ko", "내일은 비가 올 수도 있어요."),
    ("es", "Quizás llueva mañana."),
];

/// Per language: the negated source, a translation that keeps the negation
/// and one that drops it.
const NEGATION: [(&str, &str, &str); 3] = [
    ("zh", "我没有在那里。", "我在那里。"),
    ("ko", "저는 거기에 없었어요.", "저는 거기에 있었어요."),
    ("es", "No estaba allí.", "Estaba allí."),
];
const NEGATED_SOURCE: &str = "そこにいませんでした。";

/// A server that answers every chat request with `said`.
fn server_saying(said: &str) -> std::io::Result<FakeServer> {
    let body = support::chat_reply(said);
    FakeServer::start(move |_, _| Reply::json(&body))
}

fn request<'a>(src_lang: Lang, text: &'a str) -> Result<LineRequest<'a>, String> {
    Ok(LineRequest {
        id: "rp37-1",
        of: LineKind::Final,
        source: "file",
        src_lang,
        text,
        now_epoch: support::epoch(support::DEFAULT_NOW)?,
    })
}

/// What one line came to and every event the run logged.
type Outcome = (Translation, Vec<LogEvent>);

/// A Japanese line into `code`, a language outside the live pair, against a
/// server that answers `said`.
fn into_other(code: &str, src: &str, said: &str) -> Result<Outcome, Box<dyn std::error::Error>> {
    let Some(target) = Target::from_code(code) else {
        return Err(format!("no target {code}").into());
    };
    let server = server_saying(said)?;
    let mut sink = MemorySink::default();
    let mut pipeline = Pipeline::open(
        HttpLlm::new(&server.base_url()),
        &PipelineConfig::default(),
        &mut sink,
    )?;
    let line = pipeline.run_into(&request(Lang::Ja, src)?, &target, &mut |_| {}, &mut sink);
    Ok((line, sink.events))
}

/// A line of the live pair (Japanese to English or English to Japanese)
/// against a server that answers `said` to T and to E.
fn in_pair(src_lang: Lang, src: &str, said: &str) -> Result<Outcome, Box<dyn std::error::Error>> {
    let server = server_saying(said)?;
    let mut sink = MemorySink::default();
    let mut pipeline = Pipeline::open(
        HttpLlm::new(&server.base_url()),
        &PipelineConfig::default(),
        &mut sink,
    )?;
    let line = pipeline.run(&request(src_lang, src)?, &mut NoObserver, &mut sink);
    Ok((line, sink.events))
}

fn hedge_events(events: &[LogEvent]) -> Vec<&LogEvent> {
    events
        .iter()
        .filter(|event| event.event == HEDGE_EVENT)
        .collect()
}

/// The one hedge event of `events`, with its `id` and `lang` checked.
fn the_hedge_event<'a>(events: &'a [LogEvent], lang: &str) -> Result<&'a LogEvent, String> {
    let found = hedge_events(events);
    let [event] = found.as_slice() else {
        return Err(format!("{lang}: {} hedge events, not 1", found.len()));
    };
    if event.fields.get("id") != Some(&json!("rp37-1")) {
        return Err(format!("{lang}: id is {:?}", event.fields.get("id")));
    }
    if event.fields.get("lang") != Some(&json!(lang)) {
        return Err(format!("{lang}: lang is {:?}", event.fields.get("lang")));
    }
    Ok(event)
}

fn field_is_text(event: &LogEvent, key: &str) -> bool {
    event.fields.get(key).is_some_and(|value| value.is_string())
}

fn field_is_null(event: &LogEvent, key: &str) -> bool {
    event.fields.get(key).is_some_and(|value| value.is_null())
}

/// T-RP-37-001 .. 004 in one: the acceptance test of the row.
#[test]
fn rp37_hedge_mismatch_is_logged_in_zh_ko_es_and_held_in_ja_en() -> TestResult {
    for (code, _, dropped) in KEPT_AND_DROPPED {
        let (line, events) = into_other(code, HEDGED, dropped)?;
        assert_eq!(line.status, Status::Ok, "{code}: status");
        assert_eq!(line.text, dropped, "{code}: text");
        let event = the_hedge_event(&events, code)?;
        assert!(field_is_text(event, "src_hedge"), "{code}: src_hedge");
        assert!(field_is_null(event, "dst_hedge"), "{code}: dst_hedge");
    }
    for (code, hedged) in ADDED {
        let (line, events) = into_other(code, PLAIN, hedged)?;
        assert_eq!(line.status, Status::Ok, "{code}: added, status");
        let event = the_hedge_event(&events, code)?;
        assert!(
            field_is_null(event, "src_hedge"),
            "{code}: added, src_hedge"
        );
        assert!(
            field_is_text(event, "dst_hedge"),
            "{code}: added, dst_hedge"
        );
    }
    // The same disagreement into the live pair still holds, and logs nothing.
    let (to_en, events) = in_pair(Lang::Ja, HEDGED, "It will rain tomorrow.")?;
    assert_eq!(to_en.status, Status::Held, "en: status");
    assert_eq!(
        to_en.hold_reason,
        Some(HoldReason::Uncertainty),
        "en: reason"
    );
    assert!(hedge_events(&events).is_empty(), "en: events");
    let (to_ja, events) = in_pair(Lang::En, "It might rain tomorrow.", "明日は雨が降ります。")?;
    assert_eq!(to_ja.status, Status::Held, "ja: status");
    assert_eq!(
        to_ja.hold_reason,
        Some(HoldReason::Uncertainty),
        "ja: reason"
    );
    assert!(hedge_events(&events).is_empty(), "ja: events");
    Ok(())
}

/// T-RP-37-005, the negative control: a translation whose hedge agrees with
/// its source goes out and logs nothing.
#[test]
fn rp37_agreeing_hedges_log_nothing() -> TestResult {
    for (code, kept, _) in KEPT_AND_DROPPED {
        let (line, events) = into_other(code, HEDGED, kept)?;
        assert_eq!(line.status, Status::Ok, "{code}: hedged");
        assert!(hedge_events(&events).is_empty(), "{code}: hedged events");
    }
    for (code, _, dropped) in KEPT_AND_DROPPED {
        let (line, events) = into_other(code, PLAIN, dropped)?;
        assert_eq!(line.status, Status::Ok, "{code}: plain");
        assert!(hedge_events(&events).is_empty(), "{code}: plain events");
    }
    Ok(())
}

/// T-RP-37-006: a lost negation holds as `negation` into zh, ko and es, with
/// no hedge event, and the same sentence that keeps it goes out.
#[test]
fn rp37_a_lost_negation_still_holds_in_zh_ko_es() -> TestResult {
    for (code, kept, dropped) in NEGATION {
        let (line, events) = into_other(code, NEGATED_SOURCE, dropped)?;
        assert_eq!(line.status, Status::Held, "{code}: status");
        assert_eq!(
            line.hold_reason,
            Some(HoldReason::Negation),
            "{code}: reason"
        );
        assert!(hedge_events(&events).is_empty(), "{code}: events");
        let (line, _) = into_other(code, NEGATED_SOURCE, kept)?;
        assert_eq!(line.status, Status::Ok, "{code}: kept");
    }
    Ok(())
}

/// T-RP-37-007: a changed number holds as `number` into zh, ko and es even
/// when the hedge also differs.
#[test]
fn rp37_a_changed_number_still_holds_in_zh_ko_es() -> TestResult {
    let wrong = [
        ("zh", "预算大约是7百万日元。"),
        ("ko", "예산은 7백만 엔 정도예요."),
        ("es", "El presupuesto es de 7 millones de yenes."),
    ];
    for (code, said) in wrong {
        let (line, events) = into_other(code, "予算は五百万円です。", said)?;
        assert_eq!(line.status, Status::Held, "{code}: status");
        assert_eq!(line.hold_reason, Some(HoldReason::Number), "{code}: reason");
        assert!(hedge_events(&events).is_empty(), "{code}: events");
    }
    Ok(())
}

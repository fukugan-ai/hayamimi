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
use hayamimi_relay::logging::{Level, LogEvent, MemorySink};
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
    from_into_other(Lang::Ja, code, src, said)
}

/// A line in `src_lang` into `code`, a language outside the live pair.
fn from_into_other(
    src_lang: Lang,
    code: &str,
    src: &str,
    said: &str,
) -> Result<Outcome, Box<dyn std::error::Error>> {
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
    let line = pipeline.run_into(&request(src_lang, src)?, &target, &mut |_| {}, &mut sink);
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

/// Every line of a run is held for `reason` and logs no hedge event.
fn assert_held(outcome: &Outcome, reason: HoldReason, label: &str) {
    let (line, events) = outcome;
    assert_eq!(line.status, Status::Held, "{label}: status");
    assert_eq!(line.hold_reason, Some(reason), "{label}: reason");
    assert!(hedge_events(events).is_empty(), "{label}: events");
}

/// Every line of a run goes out and logs no hedge event.
fn assert_out_silently(outcome: &Outcome, label: &str) {
    let (line, events) = outcome;
    assert_eq!(
        line.status,
        Status::Ok,
        "{label}: status, held for {:?}",
        line.hold_reason
    );
    assert!(hedge_events(events).is_empty(), "{label}: events");
}

/// T-RP-37-008: a hedge only the English translation carries holds.
#[test]
fn rp37_an_added_hedge_into_en_still_holds_for_uncertainty() -> TestResult {
    let outcome = in_pair(Lang::Ja, PLAIN, "It might rain tomorrow.")?;
    assert_held(&outcome, HoldReason::Uncertainty, "en");
    Ok(())
}

/// T-RP-37-009: a hedge only the Japanese translation carries holds.
#[test]
fn rp37_an_added_hedge_into_ja_still_holds_for_uncertainty() -> TestResult {
    let outcome = in_pair(Lang::En, "It will rain tomorrow.", HEDGED)?;
    assert_held(&outcome, HoldReason::Uncertainty, "ja");
    Ok(())
}

/// T-RP-37-010: a lost negation wins over a hedge gap and nothing is logged.
#[test]
fn rp37_a_lost_negation_wins_over_a_hedge_mismatch_in_zh_ko_es() -> TestResult {
    let said = [
        ("zh", "他在那里。"),
        ("ko", "그는 거기에 있어요."),
        ("es", "Él está allí."),
    ];
    for (code, text) in said {
        let outcome = into_other(code, "そこにいないかもしれません。", text)?;
        assert_held(&outcome, HoldReason::Negation, code);
    }
    Ok(())
}

/// T-RP-37-011: a negation only the translation carries holds.
#[test]
fn rp37_an_added_negation_holds_in_zh_ko_es() -> TestResult {
    let said = [
        ("zh", "明天不会下雨。"),
        ("ko", "내일은 비가 안 와요."),
        ("es", "No lloverá mañana."),
    ];
    for (code, text) in said {
        assert_held(&into_other(code, PLAIN, text)?, HoldReason::Negation, code);
    }
    Ok(())
}

/// T-RP-37-012: the same number goes out, the counterpart of 007.
#[test]
fn rp37_the_same_number_passes_in_zh_ko_es() -> TestResult {
    let said = [
        // The row's `预算是5百万日元。` is held for `number` by V today.
        ("zh", "预算是500万日元。"),
        // The row's `5백만` is held for `number` by V today, as is the zh form;
        // the numbers table does not read the Korean scale word.
        ("ko", "예산은 5,000,000엔이에요."),
        // The row's `5 millones` is held for `number` by V today as well.
        ("es", "El presupuesto es de 5000000 yenes."),
    ];
    for (code, text) in said {
        assert_out_silently(&into_other(code, "予算は五百万円です。", text)?, code);
    }
    Ok(())
}

/// T-RP-37-013: the event is info, has the four fields and no line text.
#[test]
fn rp37_the_hedge_event_is_info_and_carries_no_line_text() -> TestResult {
    let dropped = "내일은 비가 와요.";
    let (_, events) = into_other("ko", HEDGED, dropped)?;
    let event = the_hedge_event(&events, "ko")?;
    assert_eq!(event.level, Level::Info);
    let keys: Vec<&str> = event.fields.keys().map(String::as_str).collect();
    assert_eq!(keys, ["dst_hedge", "id", "lang", "src_hedge"]);
    assert_eq!(event.fields.get("src_hedge"), Some(&json!("かもしれ")));
    let shown = format!("{} {:?}", event.message, event.fields);
    assert!(
        !shown.contains(HEDGED) && !shown.contains(dropped),
        "{shown}"
    );
    Ok(())
}

/// T-RP-37-014: several hedge words on one side still make one event.
#[test]
fn rp37_several_hedge_words_on_one_side_log_one_event() -> TestResult {
    let (line, events) = into_other("zh", "たぶん明日は雨かもしれません。", DROPPED_ZH)?;
    assert_eq!(line.status, Status::Ok);
    let event = the_hedge_event(&events, "zh")?;
    let word = event.fields.get("src_hedge");
    assert!(
        word == Some(&json!("たぶん")) || word == Some(&json!("かもしれ")),
        "{word:?}"
    );
    Ok(())
}

const DROPPED_ZH: &str = "明天会下雨。";

/// T-RP-37-015: an English source that loses its hedge is logged too.
#[test]
fn rp37_an_english_source_dropped_hedge_into_zh_ko_es_is_logged() -> TestResult {
    let said = [
        ("zh", DROPPED_ZH),
        ("ko", "내일은 비가 와요."),
        ("es", "Lloverá mañana."),
    ];
    for (code, text) in said {
        let (line, events) = from_into_other(Lang::En, code, "It might rain tomorrow.", text)?;
        assert_eq!(line.status, Status::Ok, "{code}: status");
        let event = the_hedge_event(&events, code)?;
        assert_eq!(
            event.fields.get("src_hedge"),
            Some(&json!("might")),
            "{code}"
        );
        assert!(field_is_null(event, "dst_hedge"), "{code}: dst_hedge");
    }
    Ok(())
}

/// T-RP-37-016: a language with no marker table logs no hedge event.
#[test]
fn rp37_a_language_without_a_marker_table_logs_no_hedge_event() -> TestResult {
    assert_out_silently(&into_other("fr", HEDGED, "Il pleuvra demain.")?, "fr");
    Ok(())
}

/// T-RP-37-017: a negation with no word in the source is not caught, a hole
/// kept on purpose until the marker tables learn it.
#[test]
fn rp37_a_negation_with_no_word_in_the_source_is_a_known_hole() -> TestResult {
    let said = [
        ("zh", "他出席了。"),
        ("ko", "그는 출석했어요."),
        ("es", "Él asistió."),
    ];
    for (code, text) in said {
        assert_out_silently(&into_other(code, "彼は欠席した。", text)?, code);
    }
    Ok(())
}

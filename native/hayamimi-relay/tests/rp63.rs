//! RP-63: from English into Japanese a hedge holds the line only when the
//! source carries it and the translation loses it; a hedge only the
//! translation carries goes out and is recorded in `translate.hedge.logged`.
//! The Japanese obligation forms `なければならなかった` and kin are not a
//! negation, and a real negation still holds.
//!
//! The hedge rows run a line through the pipeline against a fake
//! llama-server; the negation rows call V with the built-in tables.

mod support;

use hayamimi_relay::lang::Lang;
use hayamimi_relay::llm::HttpLlm;
use hayamimi_relay::logging::MemorySink;
use hayamimi_relay::pipeline::{
    LineKind, LineRequest, NoObserver, Pipeline, PipelineConfig, Status,
};
use hayamimi_relay::verify::{HEDGE_EVENT, HoldReason, Verdict, Verifier};
use support::{FakeServer, Reply, TestResult};

/// What one line came to: its status and reason, and the hedge events logged.
struct Outcome {
    status: Status,
    reason: Option<HoldReason>,
    hedge_events: usize,
}

/// An English line against a server that answers `said` to T and to E.
fn en_to_ja(src: &str, said: &str) -> Result<Outcome, Box<dyn std::error::Error>> {
    let body = support::chat_reply(said);
    let server = FakeServer::start(move |_, _| Reply::json(&body))?;
    let mut sink = MemorySink::default();
    let mut pipeline = Pipeline::open(
        HttpLlm::new(&server.base_url()),
        &PipelineConfig::default(),
        &mut sink,
    )?;
    let request = LineRequest {
        id: "rp63-1",
        of: LineKind::Final,
        source: "file",
        src_lang: Lang::En,
        text: src,
        now_epoch: support::epoch(support::DEFAULT_NOW)?,
    };
    let line = pipeline.run(&request, &mut NoObserver, &mut sink);
    let hedge_events = sink
        .events
        .iter()
        .filter(|event| event.event == HEDGE_EVENT)
        .count();
    Ok(Outcome {
        status: line.status,
        reason: line.hold_reason,
        hedge_events,
    })
}

fn verdict(src_lang: Lang, src: &str, translation: &str) -> Result<Verdict, String> {
    let mut sink = MemorySink::default();
    let mut verifier = Verifier::open(None, &mut sink)?;
    Ok(verifier.verify(src, translation, src_lang, &mut sink))
}

const NEGATION: Verdict = Verdict::Held(HoldReason::Negation);

/// The eight obligation sources, each with the English translation that has no
/// negation.
const OBLIGATIONS: [(&str, &str); 8] = [
    ("宿題をしなければならなかった。", "I had to do my homework."),
    ("宿題をしないといけなかった。", "I had to do my homework."),
    ("宿題をしなくてはならない。", "I must do my homework."),
    ("宿題をしなくてはなりません。", "I must do my homework."),
    ("宿題をしなくてはならなかった。", "I had to do my homework."),
    ("宿題をしなくてはいけない。", "I must do my homework."),
    ("宿題をしなくてはいけません。", "I must do my homework."),
    ("宿題をしなくてはいけなかった。", "I had to do my homework."),
];

/// T-RP-63-001 to 003: the acceptance test of the row.
#[test]
fn rp63_into_japanese_a_dropped_hedge_is_held_and_an_added_hedge_is_logged() -> TestResult {
    // 001: the source hedges and the translation does not.
    for (src, said) in [
        ("I might go.", "行きます。"),
        ("He might look for it at once.", "彼はすぐにそれを探す。"),
        ("I think it is late.", "もう遅いです。"),
    ] {
        let one = en_to_ja(src, said)?;
        assert_eq!(one.status, Status::Held, "{src}: status");
        assert_eq!(one.reason, Some(HoldReason::Uncertainty), "{src}: reason");
        assert_eq!(one.hedge_events, 0, "{src}: events");
    }
    // 002: only the translation hedges.
    for (src, said) in [
        ("It will rain.", "雨が降るでしょう。"),
        ("I could go.", "行けるかもしれません。"),
        (
            "He would look for it at once.",
            "彼はすぐにそれを探すだろう。",
        ),
    ] {
        let one = en_to_ja(src, said)?;
        assert_eq!(one.status, Status::Ok, "{src}: held for {:?}", one.reason);
        assert_eq!(one.hedge_events, 1, "{src}: events");
    }
    // 003: both hedge, or neither does.
    for (src, said) in [
        ("I might go.", "行くかもしれません。"),
        ("It will rain.", "雨が降ります。"),
    ] {
        let one = en_to_ja(src, said)?;
        assert_eq!(one.status, Status::Ok, "{src}: status");
        assert_eq!(one.hedge_events, 0, "{src}: events");
    }
    Ok(())
}

/// T-RP-63-004: the eight obligation forms with a translation that has no
/// negation go out.
#[test]
fn rp63_a_past_obligation_is_not_a_negation() -> Result<(), String> {
    for (src, said) in OBLIGATIONS {
        assert_eq!(verdict(Lang::Ja, src, said)?, Verdict::Ok, "{src}");
    }
    Ok(())
}

/// T-RP-63-005: the same sources with a negated translation are held.
#[test]
fn rp63_an_obligation_source_with_a_negated_translation_is_held() -> Result<(), String> {
    for (src, _) in OBLIGATIONS {
        let said = "I did not have to do my homework.";
        assert_eq!(verdict(Lang::Ja, src, said)?, NEGATION, "{src}");
    }
    Ok(())
}

/// T-RP-63-006: a real negation holds in both directions.
#[test]
fn rp63_a_real_negation_is_still_held() -> Result<(), String> {
    let ja = verdict(Lang::Ja, "宿題をしなかった。", "I did my homework.")?;
    assert_eq!(ja, NEGATION, "ja to en");
    let en = verdict(Lang::En, "He did not come.", "彼は来た。")?;
    assert_eq!(en, NEGATION, "en to ja");
    Ok(())
}

/// T-RP-63-007: a short `ならなかった` is not hidden from the table.
#[test]
fn rp63_a_short_past_of_naru_is_not_hidden_from_the_negation_table() -> Result<(), String> {
    assert_eq!(
        verdict(Lang::Ja, "雨にならなかった。", "It rained.")?,
        NEGATION,
        "a"
    );
    assert_eq!(
        verdict(Lang::Ja, "それは美味しくない。", "It is not delicious.")?,
        Verdict::Ok,
        "b"
    );
    assert_eq!(
        verdict(Lang::Ja, "それは美味しくない。", "It is delicious.")?,
        NEGATION,
        "c"
    );
    Ok(())
}

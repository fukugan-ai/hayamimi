//! R8t Q1 (names provisional): into Japanese a hedge that only the translation
//! carries (`would` becoming `だろう` or `でしょう`) goes out and is recorded
//! in `translate.hedge.logged`; a hedge the source carries and the translation
//! loses still holds, and negation and numbers hold as before.

use hayamimi_relay::lang::Lang;
use hayamimi_relay::logging::{LogEvent, MemorySink};
use hayamimi_relay::verify::{HEDGE_EVENT, HoldReason, Verdict, Verifier};

const WOULD_SEARCH: (&str, &str) = (
    "He would look for it at once.",
    "彼はすぐにそれを探すだろう。",
);
const WOULD_TROUBLE: (&str, &str) = (
    "Or there would be trouble.",
    "さもなければ、問題が生じるでしょう。",
);

/// V on an English line and its Japanese translation, then the hedge record of
/// the settled line when V let it through.
fn verdict_and_events(src: &str, translation: &str) -> Result<(Verdict, Vec<LogEvent>), String> {
    let mut sink = MemorySink::default();
    let mut verifier = Verifier::open(None, &mut sink)?;
    let verdict = verifier.verify(src, translation, Lang::En, &mut sink);
    if verdict == Verdict::Ok {
        verifier.log_pair_hedge("r8t-1", src, translation, Lang::En, &mut sink);
    }
    Ok((verdict, sink.events))
}

fn hedge_events(events: &[LogEvent]) -> usize {
    events
        .iter()
        .filter(|event| event.event == HEDGE_EVENT)
        .count()
}

/// The positive control: today these are held for `uncertainty`.
#[test]
fn r8t_a_hedge_added_by_the_japanese_translation_goes_out_and_is_logged() -> Result<(), String> {
    for (src, translation) in [WOULD_SEARCH, WOULD_TROUBLE] {
        let (verdict, events) = verdict_and_events(src, translation)?;
        assert_eq!(verdict, Verdict::Ok, "{src}");
        assert_eq!(hedge_events(&events), 1, "{src}: events");
    }
    Ok(())
}

/// A hedge the source carries and the translation loses is held, whether the
/// source says `might` or `think`, and nothing is logged.
#[test]
fn r8t_a_hedge_lost_by_the_japanese_translation_is_still_held() -> Result<(), String> {
    for (src, translation) in [
        ("He might look for it at once.", "彼はすぐにそれを探す。"),
        ("I think it is late.", "もう遅いです。"),
    ] {
        let (verdict, events) = verdict_and_events(src, translation)?;
        assert_eq!(verdict, Verdict::Held(HoldReason::Uncertainty), "{src}");
        assert_eq!(hedge_events(&events), 0, "{src}: events");
    }
    Ok(())
}

/// A hedge both sides carry goes out and logs nothing.
#[test]
fn r8t_agreeing_hedges_into_japanese_log_nothing() -> Result<(), String> {
    let (verdict, events) =
        verdict_and_events("He might look for it.", "彼はそれを探すかもしれません。")?;
    assert_eq!(verdict, Verdict::Ok);
    assert_eq!(hedge_events(&events), 0);
    Ok(())
}

/// Negation and numbers still hold into Japanese.
#[test]
fn r8t_negation_and_numbers_still_hold_into_japanese() -> Result<(), String> {
    let (lost, _) = verdict_and_events("He did not come.", "彼は来た。")?;
    assert_eq!(lost, Verdict::Held(HoldReason::Negation));
    let (changed, _) = verdict_and_events("Three people came.", "四人が来ました。")?;
    assert_eq!(changed, Verdict::Held(HoldReason::Number));
    Ok(())
}

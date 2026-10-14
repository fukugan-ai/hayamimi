//! The draft of an utterance still in progress.

use crate::route::{Policy, RouteError, Tier};

use super::{FixedLid, SAMPLES, Script, router};

#[test]
fn partial_forced_lang_routes_directly_without_lid_or_sv_probe() -> Result<(), RouteError> {
    // The forced language wins over the identifier, the session's own language
    // and SenseVoice's probe, none of which is consulted.
    let script = Script::answering(&[(Tier::Sv, "raw text", "")]);
    let mut routed = router(script, Some(FixedLid::naming("en")), Policy::single("ko"));
    routed.session.last_lang = Some("en".to_owned());
    assert_eq!(routed.partial(&SAMPLES, None)?, "raw text");
    assert_eq!(routed.decoder.calls, [Tier::Sv]);
    assert!(matches!(
        routed.lid.as_ref().map(|lid| lid.asked.get()),
        Some(0)
    ));
    Ok(())
}

#[test]
fn a_language_detected_earlier_in_the_utterance_routes_the_draft() -> Result<(), RouteError> {
    let script = Script::answering(&[(Tier::V3, "bonjour", "")]);
    let mut routed = router(script, None, Policy::balanced());
    assert_eq!(routed.partial(&SAMPLES, Some("fr"))?, "bonjour");
    assert_eq!(routed.decoder.calls, [Tier::V3]);
    Ok(())
}

#[test]
fn a_european_session_language_routes_the_draft_without_a_probe() -> Result<(), RouteError> {
    let script = Script::answering(&[(Tier::V3, "hola", "")]);
    let mut routed = router(script, None, Policy::balanced());
    routed.session.last_lang = Some("es".to_owned());
    assert_eq!(routed.partial(&SAMPLES, None)?, "hola");
    assert_eq!(routed.decoder.calls, [Tier::V3]);
    Ok(())
}

#[test]
fn an_english_session_still_follows_the_probe() -> Result<(), RouteError> {
    // English is the language a model romanises Korean into, so a draft in an
    // English session is not trusted to be English.
    let script = Script::answering(&[
        (Tier::Sv, "안녕하세요", "<|ko|>"),
        (Tier::V3, "annyeong", ""),
    ]);
    let mut routed = router(script, None, Policy::balanced());
    routed.session.last_lang = Some("en".to_owned());
    assert_eq!(routed.partial(&SAMPLES, None)?, "안녕하세요");
    assert_eq!(routed.decoder.calls, [Tier::Sv]);
    Ok(())
}

#[test]
fn a_japanese_probe_is_drafted_by_the_fast_japanese_model() -> Result<(), RouteError> {
    let script = Script::answering(&[
        (Tier::Sv, "こんにちわ", "<|ja|>"),
        (Tier::Rz, "こんにちは", ""),
    ]);
    let mut routed = router(script, None, Policy::balanced());
    assert_eq!(routed.partial(&SAMPLES, None)?, "こんにちは");
    Ok(())
}

#[test]
fn without_the_probe_model_the_draft_is_japanese() -> Result<(), RouteError> {
    let mut routed = router(
        Script::answering(&[(Tier::Rz, "はい", "")]),
        None,
        Policy::balanced(),
    );
    assert_eq!(routed.partial(&SAMPLES, None)?, "はい");
    Ok(())
}

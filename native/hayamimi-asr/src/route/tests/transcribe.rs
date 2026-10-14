//! One segment through the router: which language it is read in, which
//! recognizers are asked, and what the session remembers afterwards.

use crate::route::{Policy, Request, RouteError, Tier};

use super::{FixedLid, SAMPLES, Script, router, single};

fn live_in(lang: &str, speech_s: f64) -> Request<'_> {
    Request {
        known_lang: Some(lang),
        speech_s: Some(speech_s),
        ..Request::live(&SAMPLES)
    }
}

#[test]
fn transcribe_bootstrap_too_short_decodes_but_does_not_seed_last_lang() -> Result<(), RouteError> {
    // A segment shorter than the probe length opens the session: it is read as
    // well as it can be, but one blip must not lock the whole session.
    let script = Script::answering(&[(Tier::Sv, "hi", "<|en|>"), (Tier::V3, "hi there", "")]);
    let mut routed = router(script, None, Policy::balanced());
    let got = routed.transcribe(&live_in("en", 0.3))?;
    assert_eq!(got.text, "hi there");
    assert_eq!(got.lang, "en");
    assert_eq!(routed.last_lang(), None);
    Ok(())
}

#[test]
fn transcribe_bootstrap_zh_yue_reuses_single_sv_probe_decode() -> Result<(), RouteError> {
    // The bootstrap check, the switch confirmation and the Korean/Cantonese
    // reuse all want SenseVoice's reading; it must be made once.
    let script = Script::answering(&[(Tier::Sv, "some text", "<|yue|>")]);
    let mut routed = router(script, None, Policy::balanced());
    let got = routed.transcribe(&live_in("zh", 2.0))?;
    assert_eq!(got.text, "some text");
    assert_eq!(got.lang, "yue");
    assert_eq!(got.tier, Tier::Sv);
    assert_eq!(routed.decoder.times_asked(Tier::Sv), 1);
    assert_eq!(routed.last_lang(), Some("yue"));
    Ok(())
}

#[test]
fn a_european_language_needs_two_detections_to_take_the_session() -> Result<(), RouteError> {
    let script = Script::answering(&[
        (Tier::Sv, "", "<|ja|>"),
        (Tier::Rz, "こんにちは", ""),
        (Tier::V3, "bonjour", ""),
    ]);
    let mut routed = router(script, Some(FixedLid::naming("fr")), Policy::balanced());
    // The first segment is read in the language SenseVoice names.
    let first = routed.transcribe(&Request::live(&SAMPLES))?;
    assert_eq!(
        (first.lang.as_str(), first.text.as_str()),
        ("ja", "こんにちは")
    );
    assert_eq!(routed.last_lang(), Some("ja"));
    // The second detection of the same language confirms the switch.
    let second = routed.transcribe(&Request::live(&SAMPLES))?;
    assert_eq!(
        (second.lang.as_str(), second.text.as_str()),
        ("fr", "bonjour")
    );
    assert_eq!(routed.last_lang(), Some("fr"));
    Ok(())
}

#[test]
fn without_an_identifier_the_session_language_is_kept() -> Result<(), RouteError> {
    let script = Script::answering(&[(Tier::Sv, "", "<|ja|>"), (Tier::Rz, "はい", "")]);
    let mut routed = router(script, None, Policy::balanced());
    for _ in 0..2 {
        let got = routed.transcribe(&Request::live(&SAMPLES))?;
        assert_eq!(got.lang, "ja");
    }
    Ok(())
}

#[test]
fn empty_specialist_falls_through_to_omni_when_installed() -> Result<(), RouteError> {
    let script = Script::answering(&[(Tier::Rz, "", ""), (Tier::Omni, "omni text", "")]);
    let mut routed = single(script, "ja");
    let got = routed.transcribe(&Request::live(&SAMPLES))?;
    assert_eq!(routed.decoder.calls, [Tier::Rz, Tier::Omni]);
    assert_eq!(got.text, "omni text");
    assert_eq!(got.tier, Tier::Omni);
    Ok(())
}

#[test]
fn empty_specialist_stays_empty_on_a_minimal_install() -> Result<(), RouteError> {
    // No generalist is installed: the empty text stands, and nothing stops.
    let mut routed = single(Script::present(&[Tier::Rz]), "ja");
    let got = routed.transcribe(&Request::live(&SAMPLES))?;
    assert_eq!(routed.decoder.calls, [Tier::Rz, Tier::Omni]);
    assert_eq!(got.text, "");
    assert_eq!(got.tier, Tier::Rz);
    // The missing generalist is remembered and not asked again.
    routed.transcribe(&Request::live(&SAMPLES))?;
    assert_eq!(routed.decoder.times_asked(Tier::Omni), 1);
    Ok(())
}

#[test]
fn a_suppressed_segment_does_not_go_to_the_generalist() -> Result<(), RouteError> {
    let script = Script::answering(&[
        (Tier::Sv, "", "<|ja|>"),
        (Tier::Rz, "", ""),
        (Tier::Omni, "x", ""),
    ]);
    let mut routed = router(script, None, Policy::balanced());
    let got = routed.transcribe(&live_in("ja", 0.3))?;
    assert_eq!(routed.decoder.times_asked(Tier::Omni), 0);
    assert_eq!(got.text, "");
    Ok(())
}

#[test]
fn split_retry_pins_the_recognizer_and_never_reroutes() -> Result<(), RouteError> {
    // The retry goes through the recognizer that made the text, once, and
    // nothing else is asked on its account.
    let mut routed = single(Script::answering(&[(Tier::Rz, "私たちは植物", "")]), "ja");
    routed.transcribe(&Request::live(&SAMPLES))?;
    assert_eq!(routed.decoder.retries, [Tier::Rz]);
    assert_eq!(routed.decoder.calls, [Tier::Rz]);
    Ok(())
}

#[test]
fn split_retry_follows_the_recognizer_whose_text_stands() -> Result<(), RouteError> {
    // Paraformer came back empty, so SenseVoice's text stands for Chinese and
    // the retry is pinned to SenseVoice, not to the tier asked for.
    let script = Script::answering(&[(Tier::Sv, "你好吗", "<|zh|>"), (Tier::Pz, "", "")]);
    let mut routed = router(script, None, Policy::balanced());
    let got = routed.transcribe(&live_in("zh", 3.0))?;
    assert_eq!(got.text, "你好吗");
    assert_eq!(got.tier, Tier::Pz);
    assert_eq!(routed.decoder.retries, [Tier::Sv]);
    Ok(())
}

#[test]
fn a_script_that_contradicts_the_tag_is_read_again_in_its_language() -> Result<(), RouteError> {
    // English text under a Japanese tag is read again by the English tier.
    let script = Script::answering(&[
        (Tier::Rz, "NICETOMEETYOUEVERYONE", ""),
        (Tier::V3, "Nice to meet you everyone.", ""),
    ]);
    let mut routed = single(script, "ja");
    // A forced language is final: the script is not argued with.
    let forced = routed.transcribe(&Request::live(&SAMPLES))?;
    assert_eq!(forced.lang, "ja");
    assert_eq!(routed.decoder.times_asked(Tier::V3), 0);
    // Without the force, the same text is corrected.
    let script = Script::answering(&[
        (Tier::Rz, "NICETOMEETYOUEVERYONE", ""),
        (Tier::V3, "Nice to meet you everyone.", ""),
    ]);
    let mut routed = router(script, None, Policy::fast());
    let corrected = routed.transcribe(&live_in("ja", 3.0))?;
    assert_eq!(corrected.lang, "en");
    assert_eq!(corrected.text, "Nice to meet you everyone.");
    Ok(())
}

#[test]
fn a_decode_that_failed_is_an_error_and_the_router_goes_on() -> Result<(), RouteError> {
    let script = Script::answering(&[(Tier::Rz, "はい", "")]).failing(Tier::Rz);
    let mut routed = single(script, "ja");
    assert!(matches!(
        routed.transcribe(&Request::live(&SAMPLES)),
        Err(RouteError::Decode(_))
    ));
    // The next segment is asked again; a failed decode did not close anything.
    assert!(!routed.is_closed());
    assert_eq!(routed.decoder.times_asked(Tier::Rz), 1);
    assert!(matches!(
        routed.transcribe(&Request::live(&SAMPLES)),
        Err(RouteError::Decode(_))
    ));
    assert_eq!(routed.decoder.times_asked(Tier::Rz), 2);
    Ok(())
}

#[test]
fn a_segment_that_is_not_live_leaves_the_session_alone() -> Result<(), RouteError> {
    let mut routed = single(Script::answering(&[(Tier::Rz, "はい", "")]), "ja");
    let request = Request {
        live: false,
        ..Request::live(&SAMPLES)
    };
    routed.transcribe(&request)?;
    assert_eq!(routed.last_lang(), None);
    Ok(())
}

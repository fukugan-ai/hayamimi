//! The refine pass: a merged group of segments read again.

use crate::route::{Policy, Refined, RouteError, Tier, resolve_refine_lang};

use super::{FixedLid, Script, router};

/// Three seconds of audio, long enough to be judged again.
fn merged_group() -> Vec<f32> {
    vec![0.0; 48_000]
}

#[test]
fn refine_short_solo_group_never_reconsiders_language() {
    // Below the length gate the re-judgment is skipped even when it disagrees.
    assert_eq!(resolve_refine_lang("ko", "ru", None, 1.9), ("ko", false));
}

#[test]
fn refine_disagreement_without_sv_confirmation_keeps_current_lang() {
    assert_eq!(
        resolve_refine_lang("ko", "ru", Some("ja"), 5.0),
        ("ko", false)
    );
    assert_eq!(resolve_refine_lang("ko", "ru", None, 5.0), ("ko", false));
}

#[test]
fn refine_agreement_above_length_gate_overrides() {
    assert_eq!(
        resolve_refine_lang("ko", "ja", Some("ja"), 5.0),
        ("ja", true)
    );
}

#[test]
fn refine_same_lang_is_a_noop() {
    assert_eq!(
        resolve_refine_lang("ja", "ja", Some("ja"), 5.0),
        ("ja", false)
    );
}

#[test]
fn refine_agreement_below_length_gate_still_does_not_override() {
    assert_eq!(
        resolve_refine_lang("ko", "ja", Some("ja"), 2.4),
        ("ko", false)
    );
    // The gate itself is inclusive.
    assert_eq!(
        resolve_refine_lang("ko", "ja", Some("ja"), 2.5),
        ("ja", true)
    );
}

#[test]
fn refine_reuses_sv_probe_text_for_ko_yue_instead_of_redecoding() -> Result<(), RouteError> {
    // The correction is only accepted when SenseVoice's probe agrees, and
    // Korean is routed to SenseVoice, so the probe already read this audio.
    let script = Script::answering(&[(Tier::Sv, "코리안 텍스트 refine 예시입니다", "<|ko|>")]);
    let mut routed = router(script, Some(FixedLid::naming("ko")), Policy::balanced());
    let got = routed.refine(&merged_group(), "en")?;
    assert_eq!(
        got,
        Refined {
            text: "코리안 텍스트 refine 예시입니다".to_owned(),
            lang: "ko".to_owned(),
        }
    );
    assert_eq!(routed.decoder.calls, [Tier::Sv]);
    Ok(())
}

#[test]
fn refine_keeps_the_language_when_the_identifier_agrees() -> Result<(), RouteError> {
    let script = Script::answering(&[(Tier::V3, "hello everyone", "")]);
    let mut routed = router(script, Some(FixedLid::naming("en")), Policy::balanced());
    let got = routed.refine(&merged_group(), "en")?;
    assert_eq!(
        (got.text.as_str(), got.lang.as_str()),
        ("hello everyone", "en")
    );
    // No probe was made: the identifier agreed with the fast path.
    assert_eq!(routed.decoder.calls, [Tier::V3]);
    Ok(())
}

#[test]
fn refine_does_not_change_the_live_session_language() -> Result<(), RouteError> {
    let script = Script::answering(&[(Tier::V3, "hello", "")]);
    let mut routed = router(script, Some(FixedLid::naming("en")), Policy::balanced());
    routed.refine(&merged_group(), "en")?;
    assert_eq!(routed.last_lang(), None);
    Ok(())
}

#[test]
fn refine_keeps_the_fast_path_language_when_the_new_reading_is_empty() -> Result<(), RouteError> {
    // The identifier and SenseVoice both say ja, but Japanese comes back empty:
    // the fast path's English stands.
    let script = Script::answering(&[
        (Tier::Sv, "", "<|ja|>"),
        (Tier::Rz, "", ""),
        (Tier::V3, "hello", ""),
    ]);
    let mut routed = router(script, Some(FixedLid::naming("ja")), Policy::balanced());
    let got = routed.refine(&merged_group(), "en")?;
    assert_eq!((got.text.as_str(), got.lang.as_str()), ("hello", "en"));
    Ok(())
}

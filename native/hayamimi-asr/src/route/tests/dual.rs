//! The switch among the five languages both identifiers cover.

use crate::route::resolve_dual_confirm;

#[test]
fn dual_confirm_same_lang_is_a_noop() {
    assert_eq!(
        resolve_dual_confirm("ja", Some("ja"), Some(3.0), Some("ja")),
        ("ja", false)
    );
}

#[test]
fn dual_confirm_bootstrap_trusts_probe_over_whisper_misfire() {
    // The first identifier says zh, SenseVoice's own says ja: the first segment
    // is read as ja, and the two disagreeing is not a confirmed agreement.
    assert_eq!(
        resolve_dual_confirm("zh", None, Some(3.0), Some("ja")),
        ("ja", false)
    );
}

#[test]
fn dual_confirm_bootstrap_agreement_marks_switched() {
    assert_eq!(
        resolve_dual_confirm("en", None, Some(3.0), Some("en")),
        ("en", true)
    );
}

#[test]
fn dual_confirm_holds_current_lang_on_disagreement() {
    assert_eq!(
        resolve_dual_confirm("en", Some("ja"), Some(1.5), Some("ja")),
        ("ja", false)
    );
}

#[test]
fn dual_confirm_switches_immediately_on_agreement() {
    assert_eq!(
        resolve_dual_confirm("en", Some("ja"), Some(1.0), Some("en")),
        ("en", true)
    );
}

#[test]
fn dual_confirm_ignores_sub_probe_length_even_on_agreement() {
    assert_eq!(
        resolve_dual_confirm("en", Some("ja"), Some(0.3), Some("en")),
        ("ja", false)
    );
}

#[test]
fn dual_confirm_ignores_sub_probe_length_at_bootstrap() {
    assert_eq!(
        resolve_dual_confirm("en", None, Some(0.3), Some("en")),
        ("en", false)
    );
}

#[test]
fn dual_confirm_zh_whisper_yue_sensevoice_switches_to_yue() {
    // The first identifier folds Cantonese into Chinese, so zh against a yue
    // probe is the only way it can agree with one.
    assert_eq!(
        resolve_dual_confirm("zh", Some("en"), Some(3.0), Some("yue")),
        ("yue", true)
    );
}

#[test]
fn dual_confirm_session_switches_from_each_stuck_lang_to_yue() {
    for stuck in ["en", "ja", "ko"] {
        let (lang, switched) = resolve_dual_confirm("zh", Some(stuck), Some(3.0), Some("yue"));
        assert_eq!((lang, switched), ("yue", true), "{stuck}");
        // And the session holds on yue afterwards rather than bouncing back.
        let held = resolve_dual_confirm("yue", Some(lang), Some(3.0), Some("yue"));
        assert_eq!(held, ("yue", false), "{stuck}");
    }
}

#[test]
fn dual_confirm_mismatch_at_bootstrap_falls_back_to_whisper_guess() {
    // No probe tag at all (the SenseVoice model is not installed).
    assert_eq!(
        resolve_dual_confirm("zh", None, Some(3.0), None),
        ("zh", false)
    );
}

#[test]
fn dual_confirm_with_an_unknown_length_counts_the_segment() {
    assert_eq!(
        resolve_dual_confirm("en", Some("ja"), None, Some("en")),
        ("en", true)
    );
}

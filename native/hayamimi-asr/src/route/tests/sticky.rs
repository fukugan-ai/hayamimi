//! The hysteresis that holds a language until a switch is confirmed.

use crate::route::{Detection, Hysteresis, Pending, Sticky, resolve_sticky};

const BALANCED: Hysteresis = Hysteresis {
    min_switch_s: 2.0,
    confirm: 2,
};

fn seen<'a>(lang: &'a str, speech_s: f64, probe_lang: Option<&'a str>) -> Detection<'a> {
    Detection {
        lang,
        speech_s: Some(speech_s),
        probe_lang,
    }
}

fn candidate(lang: &str, count: u32) -> Option<Pending> {
    Some(Pending {
        lang: lang.to_owned(),
        count,
    })
}

fn decided(lang: &str, suppress_fallback: bool, pending: Option<Pending>) -> Sticky<'_> {
    Sticky {
        lang,
        suppress_fallback,
        pending,
    }
}

#[test]
fn sticky_first_utterance_has_no_last_lang_yet() {
    // Even a first detection must collect its confirmations: one misfire on the
    // first segment must not seed the whole session.
    let got = resolve_sticky(&seen("en", 3.0, None), None, BALANCED, None);
    assert_eq!(got, decided("en", false, candidate("en", 1)));
}

#[test]
fn sticky_bootstrap_prefers_probe_language_over_unarbitrable_guess() {
    // Too short to count, so it is held, and read in the probe's language.
    let got = resolve_sticky(&seen("ru", 1.9, Some("ja")), None, BALANCED, None);
    assert_eq!(got, decided("ja", true, None));
}

#[test]
fn sticky_bootstrap_prefers_probe_language_for_long_unarbitrable_guess() {
    let got = resolve_sticky(&seen("ru", 2.5, Some("ja")), None, BALANCED, None);
    assert_eq!(got, decided("ja", false, candidate("ru", 1)));
}

#[test]
fn sticky_bootstrap_non_sv_lang_confirms_after_repeats() {
    let first = resolve_sticky(&seen("fr", 5.0, Some("ja")), None, BALANCED, None);
    assert_eq!(first, decided("ja", false, candidate("fr", 1)));
    let second = resolve_sticky(&seen("fr", 5.0, Some("ja")), None, BALANCED, first.pending);
    assert_eq!(second, decided("fr", false, None));
}

#[test]
fn sticky_same_lang_resets_pending() {
    let got = resolve_sticky(
        &seen("ja", 3.0, None),
        Some("ja"),
        BALANCED,
        candidate("en", 1),
    );
    assert_eq!(got, decided("ja", false, None));
}

#[test]
fn sticky_single_misfire_is_held_not_switched() {
    // Long enough to be speech: the generalist stays available.
    let got = resolve_sticky(&seen("en", 3.0, None), Some("ja"), BALANCED, None);
    assert_eq!(got, decided("ja", false, candidate("en", 1)));
}

#[test]
fn sticky_short_misfire_suppresses_fallback() {
    let got = resolve_sticky(&seen("en", 1.0, None), Some("ja"), BALANCED, None);
    assert_eq!(got.lang, "ja");
    assert!(got.suppress_fallback);
}

#[test]
fn sticky_confirmed_switch_after_two_consecutive_detections() {
    let first = resolve_sticky(&seen("en", 3.0, None), Some("ja"), BALANCED, None);
    assert_eq!(first, decided("ja", false, candidate("en", 1)));
    let second = resolve_sticky(&seen("en", 3.0, None), Some("ja"), BALANCED, first.pending);
    assert_eq!(second, decided("en", false, None));
}

#[test]
fn sticky_alternating_misfires_never_accumulate() {
    let first = resolve_sticky(&seen("en", 3.0, None), Some("ja"), BALANCED, None);
    assert_eq!(first.pending, candidate("en", 1));
    let second = resolve_sticky(&seen("zh", 3.0, None), Some("ja"), BALANCED, first.pending);
    // Still held at the session language, with the candidate replaced.
    assert_eq!(second, decided("ja", false, candidate("zh", 1)));
}

#[test]
fn sticky_switch_confirm_one_disables_hysteresis() {
    let one = Hysteresis {
        confirm: 1,
        ..BALANCED
    };
    let got = resolve_sticky(&seen("en", 3.0, None), Some("ja"), one, None);
    assert_eq!(got, decided("en", false, None));
}

#[test]
fn switch_confirm_one_and_guard_zero_fully_disable_the_lock() {
    let open = Hysteresis {
        min_switch_s: 0.0,
        confirm: 1,
    };
    // A misfire of 0.3 s switches at once, and the next detection switches back.
    let away = resolve_sticky(&seen("zh", 0.3, None), Some("ja"), open, None);
    assert_eq!(away, decided("zh", false, None));
    let back = resolve_sticky(&seen("ja", 0.3, None), Some("zh"), open, away.pending);
    assert_eq!(back, decided("ja", false, None));
}

#[test]
fn sticky_short_detection_never_advances_pending_count() {
    let large_guard = Hysteresis {
        min_switch_s: 10.0,
        confirm: 2,
    };
    let got = resolve_sticky(&seen("zh", 1.9, None), Some("ja"), large_guard, None);
    assert_eq!(got, decided("ja", true, None));
}

#[test]
fn sticky_short_detections_never_confirm_a_switch() {
    let large_guard = Hysteresis {
        min_switch_s: 10.0,
        confirm: 2,
    };
    let mut pending = None;
    for speech_s in [9.1, 1.9, 1.5, 1.5] {
        let got = resolve_sticky(
            &seen("zh", speech_s, None),
            Some("ja"),
            large_guard,
            pending,
        );
        assert_eq!(got, decided("ja", true, None), "{speech_s}");
        pending = got.pending;
    }
}

#[test]
fn sticky_short_detection_does_not_reset_a_real_candidate() {
    let first = resolve_sticky(&seen("en", 3.0, None), Some("ja"), BALANCED, None);
    assert_eq!(first.pending, candidate("en", 1));
    // A short unrelated blip leaves the candidate as it was.
    let blip = resolve_sticky(&seen("zh", 0.5, None), Some("ja"), BALANCED, first.pending);
    assert_eq!(blip, decided("ja", true, candidate("en", 1)));
    // The candidate still confirms on its next long detection.
    let last = resolve_sticky(&seen("en", 3.0, None), Some("ja"), BALANCED, blip.pending);
    assert_eq!(last, decided("en", false, None));
}

#[test]
fn sticky_long_detections_still_confirm_a_switch_under_large_guard() {
    let large_guard = Hysteresis {
        min_switch_s: 10.0,
        confirm: 2,
    };
    let first = resolve_sticky(&seen("zh", 10.0, None), Some("ja"), large_guard, None);
    assert_eq!(first.pending, candidate("zh", 1));
    let second = resolve_sticky(
        &seen("zh", 10.0, None),
        Some("ja"),
        large_guard,
        first.pending,
    );
    assert_eq!(second, decided("zh", false, None));
}

#[test]
fn a_detection_with_no_known_length_is_not_taken_for_short() {
    let unknown = Detection {
        lang: "fr",
        speech_s: None,
        probe_lang: None,
    };
    let got = resolve_sticky(&unknown, Some("ja"), BALANCED, None);
    assert_eq!(got, decided("ja", false, candidate("fr", 1)));
}

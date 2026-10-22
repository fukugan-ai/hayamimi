//! What `--mode pair` does to the state of a session. The cases that need to
//! read `session.pending` are here because that field is private to the route;
//! the choice of the language itself is tested in `tests/ip03.rs`.

use crate::route::{FallbackStep, Pending, Policy, Request, RouteError, Tier};

use super::{FixedLid, SAMPLES, Script, TestRouter, router};

fn japanese_and_english() -> Script {
    Script::answering(&[
        (Tier::Rz, "はい", ""),
        (Tier::V3, "yes", ""),
        (Tier::Sv, "はい", "<|ja|>"),
    ])
}

fn not_live() -> Request<'static> {
    Request {
        live: false,
        ..Request::live(&SAMPLES)
    }
}

fn speech(seconds: f64) -> Request<'static> {
    Request {
        speech_s: Some(seconds),
        ..Request::live(&SAMPLES)
    }
}

/// A router that has read one live segment the identifier named `ja`, and then
/// hears `next`.
fn after_a_japanese_segment(next: &'static str) -> Result<TestRouter, RouteError> {
    let mut routed = router(
        japanese_and_english(),
        Some(FixedLid::naming("ja")),
        Policy::pair(),
    );
    routed.transcribe(&Request::live(&SAMPLES))?;
    routed.lid = Some(FixedLid::naming(next));
    Ok(routed)
}

#[test]
fn the_steps_are_named_as_the_log_writes_them() {
    let steps = [
        FallbackStep::SvTag,
        FallbackStep::Script,
        FallbackStep::Default,
        FallbackStep::SvFailed,
    ];
    assert_eq!(
        steps.map(FallbackStep::name),
        ["sv_tag", "script", "default", "sv_failed"]
    );
}

/// T-IP-03-017
#[test]
fn a_decode_that_is_not_live_leaves_the_session_language_alone_in_pair_mode()
-> Result<(), RouteError> {
    let mut routed = after_a_japanese_segment("en")?;
    let read = routed.transcribe(&not_live())?;
    assert_eq!(read.lang, "en");
    assert_eq!(routed.last_lang(), Some("ja"));
    assert_eq!(routed.session.pending, None);
    Ok(())
}

/// T-IP-03-018
#[test]
fn a_live_decode_in_pair_mode_sets_the_session_language() -> Result<(), RouteError> {
    let mut routed = after_a_japanese_segment("en")?;
    assert_eq!(routed.last_lang(), Some("ja"));
    routed.transcribe(&Request::live(&SAMPLES))?;
    assert_eq!(routed.last_lang(), Some("en"));
    Ok(())
}

/// T-IP-03-034
#[test]
fn pair_mode_neither_reads_nor_writes_the_pending_language_candidate() -> Result<(), RouteError> {
    let mut pair = after_a_japanese_segment("fr")?;
    assert_eq!(pair.session.pending, None);
    pair.transcribe(&speech(3.0))?;
    assert_eq!(pair.session.pending, None);

    let mut balanced = router(
        japanese_and_english(),
        Some(FixedLid::naming("ja")),
        Policy::balanced(),
    );
    balanced.transcribe(&speech(3.0))?;
    balanced.lid = Some(FixedLid::naming("fr"));
    balanced.transcribe(&speech(3.0))?;
    let waiting = Pending {
        lang: String::from("fr"),
        count: 1,
    };
    assert_eq!(balanced.session.pending, Some(waiting.clone()));

    let mut seeded = after_a_japanese_segment("fr")?;
    seeded.session.pending = Some(waiting.clone());
    seeded.transcribe(&speech(3.0))?;
    assert_eq!(seeded.session.pending, Some(waiting));
    Ok(())
}

//! A session put back to a mark: the readings between the mark and the
//! restore are forgotten, which is what a reading taken ahead and thrown away
//! needs. The cases read `session.pending`, a field private to the route.

use crate::route::{Pending, Policy, Request, RouteError, Tier};

use super::{FixedLid, SAMPLES, Script, TestRouter, router};

fn japanese_and_english() -> Script {
    Script::answering(&[
        (Tier::Rz, "はい", ""),
        (Tier::V3, "yes", ""),
        (Tier::Sv, "はい", "<|ja|>"),
    ])
}

fn speech(seconds: f64) -> Request<'static> {
    Request {
        speech_s: Some(seconds),
        ..Request::live(&SAMPLES)
    }
}

/// A balanced router that has settled on Japanese.
fn in_japanese() -> Result<TestRouter, RouteError> {
    let mut routed = router(
        japanese_and_english(),
        Some(FixedLid::naming("ja")),
        Policy::balanced(),
    );
    routed.transcribe(&speech(3.0))?;
    assert_eq!(routed.last_lang(), Some("ja"));
    Ok(routed)
}

fn french_waiting(count: u32) -> Option<Pending> {
    Some(Pending {
        lang: String::from("fr"),
        count,
    })
}

/// T-BR-43-011
#[test]
fn a_session_put_back_forgets_the_detection_a_reading_counted() -> Result<(), RouteError> {
    let mut routed = in_japanese()?;
    let mark = routed.mark_session();
    routed.lid = Some(FixedLid::naming("fr"));
    routed.transcribe(&speech(3.0))?;
    assert_eq!(routed.session.pending, french_waiting(1));

    routed.restore_session(mark);
    assert_eq!(routed.session.pending, None);
    assert_eq!(routed.last_lang(), Some("ja"));
    Ok(())
}

/// T-BR-43-012
#[test]
fn the_same_speech_read_again_after_a_restore_counts_one_detection_not_two()
-> Result<(), RouteError> {
    let mut routed = in_japanese()?;
    let mark = routed.mark_session();
    routed.lid = Some(FixedLid::naming("fr"));
    routed.transcribe(&speech(3.0))?;
    routed.restore_session(mark);
    routed.transcribe(&speech(3.0))?;
    assert_eq!(routed.session.pending, french_waiting(1));
    assert_eq!(routed.last_lang(), Some("ja"));
    Ok(())
}

/// T-BR-43-013: the control of the two above. Without the restore the second
/// reading is the second detection in a row and the session switches, which is
/// the double count that the mark exists to prevent.
#[test]
fn without_a_restore_the_same_speech_read_twice_switches_the_session() -> Result<(), RouteError> {
    let mut routed = in_japanese()?;
    routed.lid = Some(FixedLid::naming("fr"));
    routed.transcribe(&speech(3.0))?;
    routed.transcribe(&speech(3.0))?;
    assert_eq!(routed.session.pending, None);
    assert_eq!(routed.last_lang(), Some("fr"));
    Ok(())
}

/// T-BR-43-014
#[test]
fn a_mark_is_a_copy_so_later_readings_do_not_change_it() -> Result<(), RouteError> {
    let mut routed = in_japanese()?;
    let mark = routed.mark_session();
    routed.lid = Some(FixedLid::naming("fr"));
    routed.transcribe(&speech(3.0))?;
    routed.transcribe(&speech(3.0))?;
    assert_eq!(routed.last_lang(), Some("fr"));
    routed.restore_session(mark);
    assert_eq!(routed.last_lang(), Some("ja"));
    assert_eq!(routed.session.pending, None);
    Ok(())
}

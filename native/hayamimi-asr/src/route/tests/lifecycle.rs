//! The end of the router's life, and the router with no identifier at all.

use std::path::Path;
use std::rc::Rc;

use crate::fallback::{Fallback, Gpu, Reason, Stage, To};
use crate::lid::{self, Lid};
use crate::route::{Policy, Request, RouteError, Router, Tier};

use super::{FixedLid, SAMPLES, Script, TestRouter, router};

fn open_router() -> TestRouter {
    let script = Script::answering(&[(Tier::Rz, "はい", ""), (Tier::Sv, "", "<|ja|>")]);
    router(script, Some(FixedLid::naming("ja")), Policy::balanced())
}

#[test]
fn engine_close_joins_the_preload_thread() {
    // The recognizers own the background loading; closing the router closes them, once.
    let mut routed = open_router();
    routed.prefetch(Tier::Pz);
    routed.close();
    assert_eq!(routed.decoder.closed, 1);
}

#[test]
fn engine_close_drops_every_model_reference() -> Result<(), RouteError> {
    let mut routed = open_router();
    routed.transcribe(&Request::live(&SAMPLES))?;
    assert_eq!(routed.last_lang(), Some("ja"));
    routed.close();
    assert!(routed.lid.is_none());
    assert_eq!(routed.last_lang(), None);
    Ok(())
}

#[test]
fn engine_close_is_idempotent() {
    let mut routed = open_router();
    routed.close();
    routed.close();
    assert!(routed.is_closed());
    assert_eq!(routed.decoder.closed, 1);
}

#[test]
fn engine_refuses_work_after_close() {
    let mut routed = open_router();
    routed.close();
    let live = Request::live(&SAMPLES);
    assert!(matches!(routed.transcribe(&live), Err(RouteError::Closed)));
    assert!(matches!(
        routed.partial(&SAMPLES, None),
        Err(RouteError::Closed)
    ));
    assert!(matches!(routed.identify(&SAMPLES), Err(RouteError::Closed)));
    assert!(matches!(
        routed.refine(&SAMPLES, "ja"),
        Err(RouteError::Closed)
    ));
    // Nothing was asked of a recognizer on the way.
    assert!(routed.decoder.calls.is_empty());
}

#[test]
fn closed_engine_starts_no_new_background_work() {
    let mut routed = open_router();
    assert!(routed.prefetch(Tier::Pz));
    routed.close();
    assert!(!routed.prefetch(Tier::V3));
    assert_eq!(routed.decoder.prefetched, [Tier::Pz]);
}

#[test]
fn closed_engine_is_collectable() {
    let script = Script::present(&[Tier::Rz]);
    let alive = Rc::downgrade(&script.life);
    let mut routed = router(script, Some(FixedLid::naming("ja")), Policy::balanced());
    routed.close();
    drop(routed);
    assert!(alive.upgrade().is_none());
}

#[test]
fn identify_starts_loading_the_model_of_the_language_it_names() -> Result<(), RouteError> {
    let mut routed = router(
        Script::present(&[]),
        Some(FixedLid::naming("ko")),
        Policy::balanced(),
    );
    assert_eq!(routed.identify(&SAMPLES)?, "ko");
    assert_eq!(routed.decoder.prefetched, [Tier::Sv]);
    Ok(())
}

#[test]
fn a_pair_run_loads_the_model_of_a_language_it_reads() -> Result<(), RouteError> {
    for (named, tier) in [("ja", Tier::Rz), ("en", Tier::V3)] {
        let mut routed = router(
            Script::present(&[]),
            Some(FixedLid::naming(named)),
            Policy::pair(),
        );
        assert_eq!(routed.identify(&SAMPLES)?, named);
        assert_eq!(routed.decoder.prefetched, [tier], "{named}");
    }
    Ok(())
}

#[test]
fn a_pair_run_loads_nothing_for_a_language_it_never_reads() -> Result<(), RouteError> {
    let mut routed = router(
        Script::present(&[]),
        Some(FixedLid::naming("ko")),
        Policy::pair(),
    );
    assert_eq!(routed.identify(&SAMPLES)?, "ko");
    assert!(routed.decoder.prefetched.is_empty());
    Ok(())
}

#[test]
fn routed_asr_init_raises_not_exits_when_lid_missing() -> Result<(), RouteError> {
    // The C library ends the whole process on a missing model file. Opening the
    // identifier over a directory with no model must come back as a value.
    let absent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/lid-tests/absent");
    let opened = lid::open(&absent, Gpu::Absent(Reason::NoDevice)).map_err(RouteError::Lid)?;
    assert!(opened.lid.is_none());
    assert_eq!(
        opened.fallback,
        Some(Fallback {
            stage: Stage::Lid,
            reason: Reason::Missing,
            to: To::Disabled,
        })
    );
    // The route goes on without it, reading in the session's language.
    let script = Script::answering(&[(Tier::Rz, "はい", ""), (Tier::Sv, "", "<|ja|>")]);
    let mut routed: Router<Script, Lid> = Router::new(script, opened.lid, Policy::balanced());
    let got = routed.transcribe(&Request::live(&SAMPLES))?;
    assert_eq!((got.lang.as_str(), got.text.as_str()), ("ja", "はい"));
    Ok(())
}

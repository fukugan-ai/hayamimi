//! The settings a caller can change while the router runs.

use crate::route::{Pending, Policy, RouteError, Tier};

use super::{FixedLid, Script, TestRouter, router};

fn auto_router() -> TestRouter {
    router(
        Script::present(&[Tier::Rz]),
        Some(FixedLid::naming("ja")),
        Policy::balanced(),
    )
}

fn with_session_state(routed: &mut TestRouter) {
    routed.session.last_lang = Some("ja".to_owned());
    routed.session.pending = Some(Pending {
        lang: "en".to_owned(),
        count: 1,
    });
}

#[test]
fn set_forced_lang_none_reverts_to_auto() -> Result<(), RouteError> {
    let mut routed = router(Script::present(&[]), None, Policy::single("ko"));
    routed.set_forced_lang(None)?;
    assert_eq!(routed.policy().forced_lang, None);
    Ok(())
}

#[test]
fn set_forced_lang_accepts_a_routable_code() -> Result<(), RouteError> {
    let mut routed = auto_router();
    routed.set_forced_lang(Some("ko"))?;
    assert_eq!(routed.policy().forced_lang.as_deref(), Some("ko"));
    Ok(())
}

#[test]
fn set_forced_lang_rejects_an_unroutable_code() {
    let mut routed = auto_router();
    let refused = routed.set_forced_lang(Some("xx"));
    assert!(matches!(refused, Err(RouteError::Invalid(_))));
    // The setting stays as it was.
    assert_eq!(routed.policy().forced_lang, None);
}

#[test]
fn set_forced_lang_resets_sticky_and_pending_lid_state() -> Result<(), RouteError> {
    let mut routed = auto_router();
    with_session_state(&mut routed);
    routed.set_forced_lang(Some("ja"))?;
    assert_eq!(routed.last_lang(), None);
    assert_eq!(routed.session.pending, None);
    Ok(())
}

#[test]
fn a_refused_forced_lang_does_not_reset_the_session() {
    let mut routed = auto_router();
    with_session_state(&mut routed);
    assert!(routed.set_forced_lang(Some("xx")).is_err());
    assert_eq!(routed.last_lang(), Some("ja"));
}

#[test]
fn set_dual_confirm_updates_the_flag() {
    let mut routed = auto_router();
    assert!(routed.policy().dual_confirm);
    routed.set_dual_confirm(false);
    assert!(!routed.policy().dual_confirm);
    routed.set_dual_confirm(true);
    assert!(routed.policy().dual_confirm);
}

#[test]
fn set_lid_switch_confirm_accepts_valid_and_rejects_below_one() -> Result<(), RouteError> {
    let mut routed = auto_router();
    routed.set_lid_switch_confirm(1)?;
    assert_eq!(routed.policy().lid_switch_confirm, 1);
    routed.set_lid_switch_confirm(5)?;
    assert_eq!(routed.policy().lid_switch_confirm, 5);
    assert!(matches!(
        routed.set_lid_switch_confirm(0),
        Err(RouteError::Invalid(_))
    ));
    assert_eq!(routed.policy().lid_switch_confirm, 5);
    Ok(())
}

#[test]
fn set_min_switch_s_accepts_valid_and_rejects_negative() -> Result<(), RouteError> {
    let mut routed = auto_router();
    routed.set_min_switch_s(0.0)?;
    routed.set_min_switch_s(1.5)?;
    assert!((routed.policy().min_switch_s - 1.5).abs() < f64::EPSILON);
    assert!(matches!(
        routed.set_min_switch_s(-0.1),
        Err(RouteError::Invalid(_))
    ));
    assert!(matches!(
        routed.set_min_switch_s(f64::NAN),
        Err(RouteError::Invalid(_))
    ));
    assert!((routed.policy().min_switch_s - 1.5).abs() < f64::EPSILON);
    Ok(())
}

#[test]
fn reset_session_clears_sticky_state() {
    let mut routed = auto_router();
    with_session_state(&mut routed);
    routed.reset_session();
    assert_eq!(routed.last_lang(), None);
    assert_eq!(routed.session.pending, None);
}

#[test]
fn the_three_modes_set_the_documented_knobs() {
    let balanced = Policy::balanced();
    assert!(balanced.dual_confirm);
    assert_eq!(balanced.lid_switch_confirm, 2);
    assert!((balanced.min_switch_s - 2.0).abs() < f64::EPSILON);
    let fast = Policy::fast();
    assert!(!fast.dual_confirm);
    assert_eq!(fast.lid_switch_confirm, 1);
    assert!(fast.min_switch_s.abs() < f64::EPSILON);
    assert_eq!(Policy::single("ko").forced_lang.as_deref(), Some("ko"));
    assert_eq!(Policy::default(), Policy::balanced());
}

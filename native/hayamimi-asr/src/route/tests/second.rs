//! The Japanese second opinion and the gate that adopts it.

use crate::fallback::{Fallback, Reason, Stage, To};
use crate::route::{
    Policy, Request, RouteError, RouteNotice, SECOND_OPINION_THRESHOLD, Tier,
    choose_second_opinion, hyp_agreement_cer,
};

use super::{SAMPLES, Script, single};

/// A refine decode, which is where the policy asks for the second opinion.
fn refine_request() -> Request<'static> {
    Request {
        live: false,
        ..Request::live(&SAMPLES)
    }
}

fn asking_for_second_opinion() -> Policy {
    Policy {
        second_opinion: true,
        ..Policy::single("ja")
    }
}

#[test]
fn agreement_cer_ignores_spaces_and_punct() {
    let cer = hyp_agreement_cer("高校生200人に聞きました。", "高校生 200人に聞きました");
    assert!(cer.abs() < f64::EPSILON, "{cer}");
}

#[test]
fn agreement_cer_scales_with_divergence() {
    // Extra trailing speech, the failure mode of broadcast audio, drives the
    // rate well past any sane threshold.
    let cer = hyp_agreement_cer(
        "高校生200人に聞きました",
        "高校生200人に聞きましたネットの見過ぎを感じ使うのをやめたというもいます",
    );
    assert!(cer > 0.5, "{cer}");
}

#[test]
fn agreement_cer_of_two_empty_texts_is_zero_and_of_one_empty_text_is_one() {
    assert!(hyp_agreement_cer("。 ", "").abs() < f64::EPSILON);
    assert!((hyp_agreement_cer("", "あ") - 1.0).abs() < f64::EPSILON);
}

#[test]
fn choose_adopts_second_when_close() {
    let (text, used) = choose_second_opinion(
        "きょうのニュースをお伝えします",
        "今日のニュースをお伝えします",
        SECOND_OPINION_THRESHOLD,
    );
    assert!(used);
    assert_eq!(text, "今日のニュースをお伝えします");
}

#[test]
fn choose_keeps_primary_when_divergent() {
    let (text, used) = choose_second_opinion(
        "今100mのターンを迎える",
        "ただ本多選手はこの得意のドルフィンキックで",
        SECOND_OPINION_THRESHOLD,
    );
    assert!(!used);
    assert_eq!(text, "今100mのターンを迎える");
}

#[test]
fn choose_keeps_primary_on_empty_second() {
    let (text, used) = choose_second_opinion("なにか", "   ", SECOND_OPINION_THRESHOLD);
    assert!(!used);
    assert_eq!(text, "なにか");
}

#[test]
fn choose_keeps_primary_on_empty_primary() {
    let (text, used) = choose_second_opinion("", "なにか", SECOND_OPINION_THRESHOLD);
    assert!(!used);
    assert_eq!(text, "");
}

#[test]
fn missing_model_warns_once_and_disables() -> Result<(), RouteError> {
    let mut routed = single(Script::answering(&[(Tier::Rz, "元テキスト", "")]), "ja");
    routed.policy.second_opinion = true;
    let first = routed.transcribe(&refine_request())?;
    assert_eq!(first.text, "元テキスト");
    assert!(!routed.policy().second_opinion);
    assert_eq!(
        routed.take_notices(),
        [RouteNotice::Left(Fallback {
            stage: Stage::SecondOpinion,
            reason: Reason::Missing,
            to: To::Disabled,
        })]
    );
    // Asked again by the caller, it is not reported a second time.
    routed.transcribe(&Request {
        second_opinion: Some(true),
        ..refine_request()
    })?;
    assert!(routed.take_notices().is_empty());
    assert_eq!(routed.decoder.times_asked(Tier::Pja), 1);
    Ok(())
}

#[test]
fn second_decode_failure_keeps_primary() -> Result<(), RouteError> {
    let script = Script::answering(&[(Tier::Rz, "元テキスト", "")]).failing(Tier::Pja);
    let mut routed = router_asking(script);
    let got = routed.transcribe(&refine_request())?;
    assert_eq!(got.text, "元テキスト");
    Ok(())
}

#[test]
fn agreeing_second_is_adopted() -> Result<(), RouteError> {
    let script = Script::answering(&[
        (Tier::Rz, "元テキストてす", ""),
        (Tier::Pja, "元テキストです", ""),
    ]);
    let mut routed = router_asking(script);
    let got = routed.transcribe(&refine_request())?;
    assert_eq!(got.text, "元テキストです");
    Ok(())
}

#[test]
fn a_divergent_second_is_not_adopted() -> Result<(), RouteError> {
    let script = Script::answering(&[
        (Tier::Rz, "今日のニュース", ""),
        (Tier::Pja, "全く別の長い話をしています", ""),
    ]);
    let mut routed = router_asking(script);
    let got = routed.transcribe(&refine_request())?;
    assert_eq!(got.text, "今日のニュース");
    Ok(())
}

#[test]
fn a_live_segment_is_not_given_a_second_opinion_by_the_policy() -> Result<(), RouteError> {
    let script = Script::answering(&[(Tier::Rz, "元テキスト", ""), (Tier::Pja, "元テキスト", "")]);
    let mut routed = router_asking(script);
    routed.transcribe(&Request::live(&SAMPLES))?;
    assert_eq!(routed.decoder.times_asked(Tier::Pja), 0);
    Ok(())
}

#[test]
fn the_second_opinion_is_asked_only_of_japanese() -> Result<(), RouteError> {
    let script = Script::answering(&[(Tier::V3, "hello", ""), (Tier::Pja, "hello", "")]);
    let mut routed = router_asking(script);
    routed.set_forced_lang(Some("en"))?;
    routed.transcribe(&refine_request())?;
    assert_eq!(routed.decoder.times_asked(Tier::Pja), 0);
    Ok(())
}

fn router_asking(script: Script) -> super::TestRouter {
    super::router(script, None, asking_for_second_opinion())
}

//! CX-1: the conversion note is added after verification, never before.

mod support;

use hayamimi_relay::lang::Lang;
use hayamimi_relay::logging::NullSink;
use hayamimi_relay::pipeline::{Stage, Status};
use hayamimi_relay::verify::{Verdict, Verifier};
use support::{DEFAULT_NOW, Harness, Recorder, TestResult, dir_with_rates, epoch, rates_a};

const ANNOTATED_FIXTURE: &str = include_str!("fixtures/cx01_verify_input_annotated.txt");
const EDITED_JA: &str = "私は100ドル払いました。";

/// What the check refuses.
#[derive(Debug)]
enum InputFault {
    Annotated,
}

/// The text handed to V must carry none of the note's pieces.
fn check_verify_input_is_unannotated(text: &str) -> Result<(), InputFault> {
    let note_pieces = ["（", "(", "約", "15,900", "10/12"];
    if note_pieces.iter().any(|piece| text.contains(piece)) {
        Err(InputFault::Annotated)
    } else {
        Ok(())
    }
}

/// One line against the rate file of 2026-10-12.
fn run_line(
    src: &str,
    lang: Lang,
    edited: &str,
    recorder: &mut Recorder,
) -> Result<hayamimi_relay::pipeline::Translation, Box<dyn std::error::Error>> {
    let (_dir, path) = dir_with_rates(&rates_a())?;
    let mut harness = Harness::open(Some(path), None)?;
    Ok(harness.line_observed(src, lang, edited, epoch(DEFAULT_NOW)?, recorder))
}

/// T-CX-1-001
#[test]
fn cx01_dollar_amount_is_annotated_after_verification_ja_dollar_annotation_text() -> TestResult {
    let line = run_line(
        "I paid $100.",
        Lang::En,
        EDITED_JA,
        &mut Recorder::default(),
    )?;
    assert_eq!(line.status, Status::Ok, "T-CX-1-001: status");
    assert_eq!(line.annotations.len(), 1, "T-CX-1-001: annotations");
    assert_eq!(
        line.annotations[0].text.trim(),
        "（約 15,900 円、10/12 の値）",
        "T-CX-1-001"
    );
    Ok(())
}

/// T-CX-1-002
#[test]
fn cx01_dollar_amount_is_annotated_after_verification_verify_receives_the_translation_without_annotation()
-> TestResult {
    let mut recorder = Recorder::default();
    run_line("I paid $100.", Lang::En, EDITED_JA, &mut recorder)?;
    assert_eq!(recorder.verify_inputs, [EDITED_JA], "T-CX-1-002: V input");
    assert!(
        check_verify_input_is_unannotated(&recorder.verify_inputs[0]).is_ok(),
        "T-CX-1-002: pieces"
    );
    let verify_at = recorder
        .stages
        .iter()
        .position(|stage| *stage == Stage::Verify);
    let annotate_at = recorder
        .stages
        .iter()
        .position(|stage| *stage == Stage::Annotate);
    assert!(
        matches!((verify_at, annotate_at), (Some(v), Some(a)) if v < a),
        "T-CX-1-002: {:?}",
        recorder.stages
    );
    assert_eq!(
        recorder
            .stages
            .iter()
            .filter(|stage| **stage == Stage::Verify)
            .count(),
        1,
        "T-CX-1-002: V calls"
    );
    Ok(())
}

/// T-CX-1-003
#[test]
fn cx01_dollar_amount_is_annotated_after_verification_annotation_before_verify_is_rejected_by_the_check()
 {
    let annotated = check_verify_input_is_unannotated(ANNOTATED_FIXTURE.trim());
    assert!(
        matches!(annotated, Err(InputFault::Annotated)),
        "T-CX-1-003: {annotated:?}"
    );
    assert!(
        check_verify_input_is_unannotated(EDITED_JA).is_ok(),
        "T-CX-1-003: the plain text"
    );
}

/// T-CX-1-004
#[test]
fn cx01_dollar_amount_is_annotated_after_verification_annotated_text_would_make_verify_hold()
-> TestResult {
    let mut verifier = Verifier::open(None, &mut NullSink)?;
    let verdict = verifier.verify(
        "I paid $100.",
        ANNOTATED_FIXTURE.trim(),
        Lang::En,
        &mut NullSink,
    );
    assert!(
        matches!(verdict, Verdict::Held(_)),
        "T-CX-1-004: {verdict:?}"
    );
    Ok(())
}

/// T-CX-1-005
#[test]
fn cx01_dollar_amount_is_annotated_after_verification_en_annotation_text_and_end() -> TestResult {
    let line = run_line(
        "予算は五百万円です。",
        Lang::Ja,
        "The budget is five million yen.",
        &mut Recorder::default(),
    )?;
    assert_eq!(line.annotations.len(), 1, "T-CX-1-005: annotations");
    assert_eq!(
        line.annotations[0].text.trim(),
        "(about $31,500, rate of Oct 12)",
        "T-CX-1-005: text"
    );
    assert_eq!(line.annotations[0].end, 30, "T-CX-1-005: end");
    Ok(())
}

/// T-CX-1-006
#[test]
fn cx01_dollar_amount_is_annotated_after_verification_held_line_gets_no_annotation() -> TestResult {
    let line = run_line(
        "I paid $100.",
        Lang::En,
        "私は200ドル払いました。",
        &mut Recorder::default(),
    )?;
    assert_eq!(line.status, Status::Held, "T-CX-1-006: status");
    assert!(line.annotations.is_empty(), "T-CX-1-006: annotations");
    Ok(())
}

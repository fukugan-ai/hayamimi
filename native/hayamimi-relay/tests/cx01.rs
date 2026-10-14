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

const DRAFT_PASSING: &str = "私は100ドルを払いました。";
const DRAFT_WRONG_NUMBER: &str = "私は200ドルを払いました。";

/// What the check of V's calls refuses.
#[derive(Debug, PartialEq, Eq)]
enum VFault {
    Calls(usize),
    NoAnnotate,
    NoteBeforeV,
    SameStringTwice,
}

/// V runs once on T's answer and once on E's, the note is added after the
/// second, and the two strings are different.
fn check_v_calls_are_one_per_translation_and_before_annotation(
    stages: &[Stage],
    inputs: &[String],
) -> Result<(), VFault> {
    let is_v = |stage: &Stage| matches!(stage, Stage::VerifyDraft | Stage::Verify);
    let calls = stages.iter().filter(|stage| is_v(stage)).count();
    let drafts = stages
        .iter()
        .filter(|stage| **stage == Stage::VerifyDraft)
        .count();
    if calls != 2 || drafts != 1 {
        return Err(VFault::Calls(calls));
    }
    let Some(annotate_at) = stages.iter().position(|stage| *stage == Stage::Annotate) else {
        return Err(VFault::NoAnnotate);
    };
    let Some(last_v) = stages.iter().rposition(is_v) else {
        return Err(VFault::Calls(0));
    };
    if annotate_at < last_v {
        return Err(VFault::NoteBeforeV);
    }
    match inputs {
        [first, second] if first == second => Err(VFault::SameStringTwice),
        _ => Ok(()),
    }
}

/// Runs the line with the given answer of T and records what V was given.
fn run_pair(
    draft: &str,
    edited: &str,
) -> Result<(Recorder, hayamimi_relay::pipeline::Translation), Box<dyn std::error::Error>> {
    let (_dir, path) = dir_with_rates(&rates_a())?;
    let mut harness = Harness::open(Some(path), None)?;
    let mut recorder = Recorder::default();
    let line = harness.line_with_draft(
        "I paid $100.",
        Lang::En,
        draft,
        edited,
        epoch(DEFAULT_NOW)?,
        &mut recorder,
    );
    Ok((recorder, line))
}

/// T-CX-1-002
#[test]
fn cx01_dollar_amount_is_annotated_after_verification_verify_receives_each_translation_once_without_annotation()
-> TestResult {
    for (draft, label) in [(DRAFT_PASSING, "run A"), (DRAFT_WRONG_NUMBER, "run B")] {
        let (recorder, line) = run_pair(draft, EDITED_JA)?;
        let given: Vec<&str> = recorder
            .draft_inputs
            .iter()
            .chain(&recorder.verify_inputs)
            .map(String::as_str)
            .collect();
        assert_eq!(given, [draft, EDITED_JA], "T-CX-1-002 {label}: V inputs");
        for text in &given {
            assert!(
                check_verify_input_is_unannotated(text).is_ok(),
                "T-CX-1-002 {label}: pieces"
            );
        }
        assert_eq!(
            recorder.stages,
            [
                Stage::Translate,
                Stage::VerifyDraft,
                Stage::Edit,
                Stage::Verify,
                Stage::Annotate
            ],
            "T-CX-1-002 {label}: stages"
        );
        assert_eq!(
            check_v_calls_are_one_per_translation_and_before_annotation(
                &recorder.stages,
                &given
                    .iter()
                    .map(|text| (*text).to_owned())
                    .collect::<Vec<_>>()
            ),
            Ok(()),
            "T-CX-1-002 {label}: check"
        );
        assert_eq!(line.annotations.len(), 1, "T-CX-1-002 {label}: annotations");
        assert_eq!(line.text, EDITED_JA, "T-CX-1-002 {label}: settled text");
    }
    Ok(())
}

/// T-CX-1-002, the negative controls.
#[test]
fn cx01_dollar_amount_is_annotated_after_verification_a_third_v_call_or_a_note_before_v_is_refused_by_the_check()
 {
    use Stage::{Annotate, Edit, Translate, Verify, VerifyDraft};
    let two = [String::from("a"), String::from("b")];
    let ok = [Translate, VerifyDraft, Edit, Verify, Annotate];
    assert_eq!(
        check_v_calls_are_one_per_translation_and_before_annotation(&ok, &two),
        Ok(())
    );
    let refused = [
        (
            vec![Translate, VerifyDraft, Edit, Verify, Verify, Annotate],
            two.to_vec(),
            VFault::Calls(3),
        ),
        (
            vec![Translate, Edit, Verify, Annotate],
            two.to_vec(),
            VFault::Calls(1),
        ),
        (
            vec![Translate, VerifyDraft, Edit, Annotate, Verify],
            two.to_vec(),
            VFault::NoteBeforeV,
        ),
        (
            ok.to_vec(),
            vec![String::from("a"), String::from("a")],
            VFault::SameStringTwice,
        ),
    ];
    for (stages, inputs, fault) in refused {
        assert_eq!(
            check_v_calls_are_one_per_translation_and_before_annotation(&stages, &inputs),
            Err(fault),
            "{stages:?}"
        );
    }
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
    // Neither T's answer nor E's carries the amount of the source.
    let (_recorder, line) = run_pair(DRAFT_WRONG_NUMBER, "私は200ドル払いました。")?;
    assert_eq!(line.status, Status::Held, "T-CX-1-006: status");
    assert!(line.annotations.is_empty(), "T-CX-1-006: annotations");
    Ok(())
}

//! CX-2: ambiguous units are left without a note.

mod support;

use hayamimi_relay::annotate::SkipReason;
use hayamimi_relay::lang::Lang;
use hayamimi_relay::pipeline::{Status, Translation};
use hayamimi_relay::units::Kind;
use serde_json::Value;
use support::{DEFAULT_NOW, Harness, TestResult, dir_with_rates, epoch, rates_a};

const POUND_ANNOTATED: &str = include_str!("fixtures/cx02_pound_annotated.json");

/// What the check refuses.
#[derive(Debug)]
enum NoteFault {
    NotJson,
    Annotated(usize),
}

/// A line that must carry no note.
fn check_unannotated(line: &Value) -> Result<(), NoteFault> {
    let notes = line
        .get("annotations")
        .and_then(Value::as_array)
        .ok_or(NoteFault::NotJson)?;
    if notes.is_empty() {
        Ok(())
    } else {
        Err(NoteFault::Annotated(notes.len()))
    }
}

fn run(src: &str, edited: &str) -> Result<Translation, Box<dyn std::error::Error>> {
    let (_dir, path) = dir_with_rates(&rates_a())?;
    let mut harness = Harness::open(Some(path), None)?;
    let line = harness.line(src, Lang::En, edited, epoch(DEFAULT_NOW)?);
    assert_eq!(
        line.status,
        Status::Ok,
        "the verification must pass for {src:?}"
    );
    Ok(line)
}

fn expect_ambiguous(src: &str, edited: &str, id: &str) -> TestResult {
    let line = run(src, edited)?;
    assert!(
        line.annotations.is_empty(),
        "{id}: annotations {:?}",
        line.annotations
    );
    assert!(
        line.skipped
            .iter()
            .any(|skipped| skipped.reason == SkipReason::Ambiguous),
        "{id}: {:?}",
        line.skipped
    );
    Ok(())
}

fn expect_one_note(
    src: &str,
    edited: &str,
    note: &str,
    id: &str,
) -> Result<Translation, Box<dyn std::error::Error>> {
    let line = run(src, edited)?;
    assert_eq!(
        line.annotations.len(),
        1,
        "{id}: annotations {:?}",
        line.annotations
    );
    assert_eq!(line.annotations[0].text.trim(), note, "{id}");
    Ok(line)
}

fn expect_unplaced(src: &str, edited: &str, id: &str) -> TestResult {
    let line = run(src, edited)?;
    assert!(
        line.annotations.is_empty(),
        "{id}: annotations {:?}",
        line.annotations
    );
    assert!(
        line.skipped
            .iter()
            .any(|skipped| skipped.kind == Kind::Currency && skipped.reason == SkipReason::Unplaced),
        "{id}: {:?}",
        line.skipped
    );
    Ok(())
}

/// T-CX-2-001
#[test]
fn cx02_ambiguous_units_are_left_unannotated_ten_pounds_is_left_unannotated() -> TestResult {
    expect_ambiguous(
        "It weighs ten pounds.",
        "重さは10ポンドです。",
        "T-CX-2-001",
    )
}

/// T-CX-2-002
#[test]
fn cx02_ambiguous_units_are_left_unannotated_two_tons_is_left_unannotated() -> TestResult {
    expect_ambiguous("It weighs two tons.", "重さは2トンです。", "T-CX-2-002")
}

/// T-CX-2-003
#[test]
fn cx02_ambiguous_units_are_left_unannotated_five_gallons_is_left_unannotated() -> TestResult {
    expect_ambiguous("I bought 5 gallons.", "5ガロン買いました。", "T-CX-2-003")
}

/// T-CX-2-004
#[test]
fn cx02_ambiguous_units_are_left_unannotated_at_5_in_the_morning_is_left_unannotated() -> TestResult
{
    expect_ambiguous(
        "Meet me at 5 in the morning.",
        "朝5時に会いましょう。",
        "T-CX-2-004",
    )
}

/// T-CX-2-005
#[test]
fn cx02_ambiguous_units_are_left_unannotated_ten_lb_is_annotated() -> TestResult {
    expect_one_note(
        "It weighs 10 lb.",
        "重さは10 ポンドです。",
        "（約 4.54 kg）",
        "T-CX-2-005",
    )?;
    Ok(())
}

/// T-CX-2-006
#[test]
fn cx02_ambiguous_units_are_left_unannotated_five_inches_is_annotated() -> TestResult {
    expect_one_note(
        "It is 5 inches long.",
        "長さは5インチです。",
        "（約 12.7 cm）",
        "T-CX-2-006",
    )?;
    Ok(())
}

/// T-CX-2-007
#[test]
fn cx02_ambiguous_units_are_left_unannotated_two_us_gallons_is_annotated() -> TestResult {
    expect_one_note(
        "I bought 2 US gallons.",
        "2 米ガロン買いました。",
        "（約 7.57 L）",
        "T-CX-2-007",
    )?;
    Ok(())
}

/// T-CX-2-008
#[test]
fn cx02_ambiguous_units_are_left_unannotated_annotated_pound_is_rejected_by_the_check() -> TestResult
{
    let fake: Value = serde_json::from_str(POUND_ANNOTATED)?;
    let verdict = check_unannotated(&fake);
    assert!(
        matches!(verdict, Err(NoteFault::Annotated(1))),
        "T-CX-2-008: {verdict:?}"
    );
    let real = run("It weighs ten pounds.", "重さは10ポンドです。")?;
    assert!(
        check_unannotated(&serde_json::to_value(&real)?).is_ok(),
        "T-CX-2-008: the real result"
    );
    assert!(matches!(
        check_unannotated(&Value::Null),
        Err(NoteFault::NotJson)
    ));
    Ok(())
}

/// T-CX-2-009
#[test]
fn cx02_ambiguous_units_are_left_unannotated_zero_matches_are_skipped_as_unplaced() -> TestResult {
    expect_unplaced("I paid $100.", "私は100払いました。", "T-CX-2-009")
}

/// T-CX-2-010
#[test]
fn cx02_ambiguous_units_are_left_unannotated_two_matches_are_skipped_as_unplaced() -> TestResult {
    expect_unplaced(
        "I planned $100 and paid $100.",
        "100ドルのつもりが、100ドル払いました。",
        "T-CX-2-010",
    )
}

/// T-CX-2-011
#[test]
fn cx02_ambiguous_units_are_left_unannotated_one_match_by_alias_dollar_sign_is_annotated_with_its_span()
-> TestResult {
    let line = expect_one_note(
        "I paid $100.",
        "私は$100払いました。",
        "（約 15,900 円、10/12 の値）",
        "T-CX-2-011",
    )?;
    assert_eq!(
        (line.annotations[0].start, line.annotations[0].end),
        (2, 6),
        "T-CX-2-011: span"
    );
    assert!(
        line.skipped
            .iter()
            .all(|skipped| skipped.reason != SkipReason::Unplaced),
        "T-CX-2-011: {:?}",
        line.skipped
    );
    Ok(())
}

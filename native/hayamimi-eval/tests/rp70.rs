//! RP-70: a sentence translated without its closing mark is as good.
//!
//! The fixture rows judge numbers and rows built here. The real-run row needs
//! the four runs `a1`, `b1`, `a2`, `b2` that the E2E seat sends to one
//! server, and is `#[ignore]`.

mod support;

use std::path::Path;

use support::rp70::{
    COLUMN_A_CHRF_GAP, COLUMN_A_HELD_GAP, COLUMN_LOST_MARKS, MarkRun, Row, check_closing_mark,
    check_columns_written, check_pair_sets, check_run_order, ending_mark_rows, lost_final_marks,
    read_rows, render_report, without_mark_rows,
};
use support::{Failure, REPO, TestResult};

/// The pooled chrF of F70 in millionths, 40.123456.
const BASE: i64 = 40_123_456;

fn run(chrf_micro: i64, held: usize) -> MarkRun {
    MarkRun {
        chrf_micro,
        held,
        lost_marks: 0,
    }
}

/// F70: both sets at `BASE`, three held lines in `a` (2 and 1) and in `b`.
fn f70() -> ([MarkRun; 2], [MarkRun; 2]) {
    ([run(BASE, 2), run(BASE, 1)], [run(BASE, 2), run(BASE, 1)])
}

/// F70 with `b` at `chrf` in both runs and `held` lines held in total.
fn f70_b(chrf: i64, held: usize) -> ([MarkRun; 2], [MarkRun; 2]) {
    let (a, _) = f70();
    (a, [run(chrf, held), run(chrf, 0)])
}

/// The refusal of a judgement; a pass is an error of the test.
fn refusal(a: &[MarkRun; 2], b: &[MarkRun; 2]) -> Result<Failure, Box<dyn std::error::Error>> {
    match check_closing_mark(a, b) {
        Ok(_) => Err("the runs were not refused".into()),
        Err(why) => Ok(why),
    }
}

/// T-RP-70-001: equal chrF and equal held rows pass.
#[test]
fn equal_chrf_and_equal_held_rows_pass() -> TestResult {
    let (a, b) = f70();
    check_closing_mark(&a, &b)?;
    Ok(())
}

/// T-RP-70-002: `b` 0.01 below `a` is refused, naming both values.
#[test]
fn a_chrf_one_hundredth_below_a_is_rejected() -> TestResult {
    let (a, b) = f70_b(BASE - 10_000, 3);
    let why = refusal(&a, &b)?;
    assert!(why.contains("chrF") && why.contains("40.113456") && why.contains("40.123456"));
    assert!(!why.contains("held rows"), "{why}");
    Ok(())
}

/// T-RP-70-003: one millionth below is refused.
#[test]
fn a_chrf_one_micro_unit_below_a_is_rejected() -> TestResult {
    let (a, b) = f70_b(BASE - 1, 3);
    let why = refusal(&a, &b)?;
    assert!(why.contains("chrF"), "{why}");
    Ok(())
}

/// T-RP-70-004: one millionth above passes.
#[test]
fn a_chrf_one_micro_unit_above_a_passes() -> TestResult {
    let (a, b) = f70_b(BASE + 1, 3);
    check_closing_mark(&a, &b)?;
    Ok(())
}

/// T-RP-70-005: one held line more in `b` is refused, naming 4 and 3.
#[test]
fn one_more_held_row_in_b_is_rejected() -> TestResult {
    let (a, b) = f70_b(BASE, 4);
    let why = refusal(&a, &b)?;
    assert!(why.contains("held rows") && why.contains('4') && why.contains('3'));
    assert!(!why.contains("chrF"), "{why}");
    Ok(())
}

/// T-RP-70-006: fewer held lines in `b` pass.
#[test]
fn fewer_held_rows_in_b_pass() -> TestResult {
    let (a, b) = f70_b(BASE, 2);
    check_closing_mark(&a, &b)?;
    Ok(())
}

/// F70-7: `a` at 40.00 twice, `b` at 39.00 and 42.00 (40.50 pooled).
fn f70_pooled(b2: i64) -> ([MarkRun; 2], [MarkRun; 2]) {
    (
        [run(40_000_000, 0), run(40_000_000, 0)],
        [run(39_000_000, 0), run(b2, 0)],
    )
}

/// T-RP-70-007: the pooled value passes although `b1` is below `a1`.
#[test]
fn the_two_runs_are_pooled_before_the_comparison() -> TestResult {
    let (a, b) = f70_pooled(42_000_000);
    assert!(b[0].chrf_micro < a[0].chrf_micro);
    check_closing_mark(&a, &b)?;
    Ok(())
}

/// T-RP-70-008: a pooled value of 39.75 is refused although `b2` is above `a2`.
#[test]
fn a_pooled_value_below_a_fails_even_if_one_run_is_above() -> TestResult {
    let (a, b) = f70_pooled(40_500_000);
    assert!(b[1].chrf_micro > a[1].chrf_micro);
    let why = refusal(&a, &b)?;
    assert!(why.contains("chrF") && why.contains("39.750000"), "{why}");
    Ok(())
}

/// T-RP-70-009: five lines that lost their end mark are recorded, not judged.
#[test]
fn rows_that_lose_their_final_mark_are_recorded_and_not_judged() -> TestResult {
    let (a, mut b) = f70();
    b[0].lost_marks = 5;
    let report = check_closing_mark(&a, &b)?;
    let written = render_report(&report);
    assert!(
        written.contains(&format!("{COLUMN_LOST_MARKS}: 5\n")),
        "{written}"
    );
    check_columns_written(&written)?;
    let without = check_columns_written("a chrF gap: 0\na held gap: 0\n");
    assert!(matches!(&without, Err(why) if why.contains(COLUMN_LOST_MARKS)));
    Ok(())
}

/// T-RP-70-010: the two `a` runs alone give a chrF gap and a held gap.
#[test]
fn the_difference_between_the_two_a_runs_is_reported() -> TestResult {
    let a = [run(40_000_000, 1), run(40_500_000, 3)];
    let report = check_closing_mark(&a, &a)?;
    let written = render_report(&report);
    assert!(
        written.contains(&format!("{COLUMN_A_CHRF_GAP}: 0.500000\n")),
        "{written}"
    );
    assert!(
        written.contains(&format!("{COLUMN_A_HELD_GAP}: 2\n")),
        "{written}"
    );
    let without = check_columns_written("rows that lost the final mark: 0\n");
    assert!(matches!(&without, Err(why) if why.contains(COLUMN_A_CHRF_GAP)));
    Ok(())
}

fn row(id: &str, ja: &str, reference: &str) -> Row {
    Row {
        id: id.to_owned(),
        ja: ja.to_owned(),
        reference: reference.to_owned(),
    }
}

/// F70-11: three rows of `a`, each ending with `。`.
fn f70_a_rows() -> Vec<Row> {
    vec![
        row(
            "s1-1",
            "鉄は摩耗の原因になりました。",
            "iron wore the wheels",
        ),
        row(
            "s1-2",
            "主な生息地は河口域です。",
            "estuaries are the habitat",
        ),
        row(
            "s1-3",
            "公園は広さ19,500平方キロです。",
            "the park is large",
        ),
    ]
}

/// T-RP-70-011: `b` is `a` without its final `。`, same ids and references.
#[test]
fn a_b_pair_set_is_the_a_set_without_the_final_closing_mark() -> TestResult {
    let a = f70_a_rows();
    let b = without_mark_rows(&a);
    assert_eq!(check_pair_sets("seed 0", &a, &b)?, 3);
    assert!(b.iter().all(|row| !row.ja.ends_with('。')));
    Ok(())
}

/// The refusal of a pair of sets; a pass is an error of the test.
fn set_refusal(seed: &str, a: &[Row], b: &[Row]) -> Result<Failure, Box<dyn std::error::Error>> {
    match check_pair_sets(seed, a, b) {
        Ok(rows) => Err(format!("{seed}: {rows} rows were not refused").into()),
        Err(why) => Ok(why),
    }
}

/// T-RP-70-012: a `b` that differs by more than the mark, an `a` with a row
/// that does not end with it, and an empty set are refused by name.
#[test]
fn a_b_set_that_differs_by_more_than_the_mark_or_an_empty_set_is_rejected() -> TestResult {
    let a = f70_a_rows();
    let mut shorter = without_mark_rows(&a);
    shorter[1].ja.pop();
    assert!(set_refusal("seed 0", &a, &shorter)?.contains("s1-2"));

    let mut mixed = f70_a_rows();
    mixed[2].ja = String::from("公園は広いです");
    let b = without_mark_rows(&f70_a_rows());
    assert!(set_refusal("seed 0", &mixed, &b)?.contains("s1-3"));

    assert!(set_refusal("seed 1", &[], &[])?.contains("seed 1"));
    let none = ending_mark_rows(&[row("x", "終わりの印が無い", "no mark")]);
    assert!(set_refusal("seed 0", &none, &none)?.contains("seed 0"));
    Ok(())
}

/// A row of a different id or reference is not the same line.
#[test]
fn a_b_row_of_another_id_or_reference_is_rejected() -> TestResult {
    let a = f70_a_rows();
    let mut other_id = without_mark_rows(&a);
    other_id[0].id = String::from("s9-9");
    assert!(set_refusal("seed 0", &a, &other_id)?.contains("s9-9"));
    let mut other_reference = without_mark_rows(&a);
    other_reference[2].reference = String::from("changed");
    assert!(set_refusal("seed 0", &a, &other_reference)?.contains("s1-3"));
    let fewer = without_mark_rows(&a[..2]);
    assert!(set_refusal("seed 0", &a, &fewer)?.contains("3 rows"));
    Ok(())
}

/// The lines whose translation lost `.`, `?` or `!` are counted.
#[test]
fn a_translation_that_lost_its_end_mark_is_counted() {
    let with_mark = ["It wore.", "Is it?", "Yes!", "Plain", "Ends."];
    let without_mark = ["It wore", "Is it?", "Yes", "Plain", "Ends. "];
    assert_eq!(lost_final_marks(&with_mark, &without_mark), 2);
    assert_eq!(lost_final_marks(&[], &[]), 0);
}

/// The runs are sent as `a1`, `b1`, `a2`, `b2` and in no other order.
#[test]
fn runs_must_be_sent_in_the_a1_b1_a2_b2_order() {
    let sent = [("b2", 40), ("a1", 10), ("a2", 30), ("b1", 20)];
    assert!(check_run_order(&sent).is_ok());
    let swapped = [("a1", 10), ("a2", 20), ("b1", 30), ("b2", 40)];
    assert!(matches!(check_run_order(&swapped), Err(why) if why.contains("a2")));
    let tied = [("a1", 10), ("b1", 20), ("a2", 20), ("b2", 40)];
    assert!(check_run_order(&tied).is_err());
    assert!(check_run_order(&[]).is_err());
}

/// The sets of one seed and language, built from a pairs directory.
fn check_seed(dir: &Path, seed: &str, lang: &str) -> Result<usize, Failure> {
    let a = ending_mark_rows(&read_rows(dir, lang)?);
    check_pair_sets(&format!("{seed} {lang}"), &a, &without_mark_rows(&a))
}

/// T-RP-70-013: the sets of seed 0 and seed 1 are built from the dev pairs
/// directories, and then the four runs are judged.
#[test]
#[ignore = "needs the results of the runs a1, b1, a2, b2, which the E2E seat sends to one server"]
fn rp70_translating_without_the_closing_mark_keeps_chrf_and_verify() -> TestResult {
    let base = support::base()?;
    for (seed, dir) in [("seed 0", "pairs"), ("seed 1", "pairs_seed1")] {
        for lang in ["en", "es", "ko", "zh"] {
            let rows = check_seed(&base.join(dir), seed, lang)?;
            println!("rp70 {seed} {lang}: {rows} rows end with the mark");
        }
    }
    Err(format!(
        "the results of the four runs are not in the repository ({REPO}); \
         the design names no directory for them or for the relay stdout"
    )
    .into())
}

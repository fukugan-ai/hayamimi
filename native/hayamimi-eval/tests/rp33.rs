//! RP-33: the first translation of a line whose draft T failed V.
//!
//! The translation stage (from the `final` to `t_tr`) of Rust, over all lines,
//! must be no greater than Python's at p50 and p95, and the lines whose draft
//! failed V have a column of their own. The fixture rows build the lines
//! from a formula (F33); the rows on the recorded runs read `tests/rp31/` and
//! `docs/results/r8e/`; the real-run row reads the runs RP-32 measured in
//! `docs/results/rp32/` and is `#[ignore]`.

mod support;

use std::path::Path;

use hayamimi_eval::compare::read_samples;
use hayamimi_eval::latency::{Sample, Stat};
use serde_json::{Value, json};
use support::rp32::rp32_dir;
use support::rp33::{
    FAILED_DRAFT, Line, StageColumns, check_columns_written, check_first_translation,
    check_passed_draft_l, check_rows_add_up, jsonl_rows, load_real_runs, recount, recount_run,
    render_stage_columns, samples_stage, stage_columns,
};
use support::{Failure, RESULTS, TestResult, base, read, rp3_run};

const SLICES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/rp31");
/// The translation stage of every F33 line, in milliseconds.
const STAGE_MS: i64 = 200;

fn stat(n: usize, p50_ms: i64, p95_ms: i64) -> Option<Stat> {
    Some(Stat { n, p50_ms, p95_ms })
}

/// A valid sample: `t_end` 0, the `final` at `t_final_ms`, latency 100 ms, so
/// the silence gap is over the 300 ms limit. `t_tr_ms` is `None` for a line
/// that got no shown translation.
fn sample(utt: u32, t_final_ms: i64, t_tr_ms: Option<i64>) -> Sample {
    Sample {
        repeat: 1,
        utt,
        t_start_us: 0,
        t_end_us: 0,
        t_partial_us: None,
        t_partial_tr_us: None,
        t_final_us: Some(t_final_ms * 1000),
        t_tr_us: t_tr_ms.map(|t| t * 1000),
        finals: 1,
        text: String::new(),
        translation: None,
        final_latency_ms: Some(100.0),
        cer_dist: 0,
        cer_denom: 0,
    }
}

/// F33, Rust: 20 lines, rows 1 to 10 passed V and rows 11 to 20 failed it.
fn f33_rust() -> Vec<Line> {
    (1..=20)
        .map(|utt| {
            let t_final = 700 + i64::from(utt);
            Line {
                sample: sample(utt, t_final, Some(t_final + STAGE_MS)),
                draft_passed: utt <= 10,
            }
        })
        .collect()
}

/// F33, Python: 20 lines, every translation stage 200 ms.
fn f33_python() -> Vec<Sample> {
    (1..=20)
        .map(|utt| {
            let t_final = 700 + i64::from(utt);
            sample(utt, t_final, Some(t_final + STAGE_MS))
        })
        .collect()
}

/// Sets the translation stage of the rows `rows` (1-based) to `stage_ms`.
fn with_stage(
    mut lines: Vec<Line>,
    rows: std::ops::RangeInclusive<u32>,
    stage_ms: i64,
) -> Vec<Line> {
    for line in lines
        .iter_mut()
        .filter(|line| rows.contains(&line.sample.utt))
    {
        line.sample.t_tr_us = line.sample.t_final_us.map(|t| t + stage_ms * 1000);
    }
    lines
}

/// The judgement fails, names every word of `named` and none of `not_named`.
fn rejected(said: &Result<StageColumns, Failure>, named: &[&str], not_named: &[&str]) {
    let Err(why) = said else {
        panic!("the judgement passed: {said:?}");
    };
    for word in named {
        assert!(why.contains(word), "{word} is not named: {why}");
    }
    for word in not_named {
        assert!(!why.contains(word), "{word} is named: {why}");
    }
}

/// T-RP-33-001
#[test]
fn equal_translation_stage_passes() -> TestResult {
    let columns = check_first_translation(&f33_rust(), &f33_python())?;
    assert_eq!(columns.all, stat(20, 200, 200));
    Ok(())
}

/// T-RP-33-002
#[test]
fn p95_one_millisecond_over_at_the_19th_row_is_rejected() {
    let rust = with_stage(f33_rust(), 19..=20, 201);
    let said = check_first_translation(&rust, &f33_python());
    rejected(&said, &["p95 rust 201 ms > python 200 ms"], &["p50"]);
}

/// T-RP-33-003
#[test]
fn p95_is_decided_by_the_19th_row_only() -> TestResult {
    let rust = with_stage(f33_rust(), 20..=20, 201);
    check_first_translation(&rust, &f33_python())?;
    Ok(())
}

/// T-RP-33-004
#[test]
fn ten_slower_rows_leave_the_p50_alone() {
    let rust = with_stage(f33_rust(), 11..=20, 201);
    let said = check_first_translation(&rust, &f33_python());
    rejected(&said, &["p95 rust 201 ms > python 200 ms"], &["p50"]);
}

/// T-RP-33-005
#[test]
fn p50_one_millisecond_over_at_the_10th_row_is_rejected() {
    let rust = with_stage(f33_rust(), 10..=20, 201);
    let said = check_first_translation(&rust, &f33_python());
    rejected(
        &said,
        &[
            "p50 rust 201 ms > python 200 ms",
            "p95 rust 201 ms > python 200 ms",
        ],
        &[],
    );
}

/// T-RP-33-006
#[test]
fn six_hundred_ms_slower_failed_drafts_are_rejected() -> TestResult {
    let rust = with_stage(f33_rust(), 11..=20, STAGE_MS + 600);
    let said = check_first_translation(&rust, &f33_python());
    rejected(&said, &["p95 rust 800 ms > python 200 ms"], &["p50"]);
    let columns = stage_columns(&rust)?;
    assert_eq!(columns.failed, stat(10, 800, 800));
    assert_eq!(columns.passed, stat(10, 200, 200));
    Ok(())
}

/// T-RP-33-007
#[test]
fn failed_and_passed_draft_columns_are_written_apart() -> TestResult {
    let columns = check_first_translation(&f33_rust(), &f33_python())?;
    assert_eq!(columns.failed, stat(10, 200, 200));
    assert_eq!(columns.passed, stat(10, 200, 200));
    let text = render_stage_columns(&columns);
    check_columns_written(&text)?;
    assert!(
        text.contains("(failed draft) p50 200 p95 200 ms (n 10)"),
        "{text}"
    );
    assert!(
        text.contains("(passed draft) p50 200 p95 200 ms (n 10)"),
        "{text}"
    );
    Ok(())
}

/// T-RP-33-008
#[test]
fn a_missing_failed_draft_column_is_rejected() -> TestResult {
    let columns = check_first_translation(&f33_rust(), &f33_python())?;
    let text: Vec<String> = render_stage_columns(&columns)
        .lines()
        .filter(|line| !line.starts_with(FAILED_DRAFT))
        .map(str::to_owned)
        .collect();
    let said = check_columns_written(&text.join("\n"));
    assert!(
        said.as_ref().is_err_and(|why| why.contains("failed draft")),
        "{said:?}"
    );
    Ok(())
}

/// T-RP-33-009
#[test]
fn rows_without_a_shown_translation_are_left_out_of_the_columns() -> TestResult {
    let mut rust = f33_rust();
    for utt in 21..=24 {
        rust.push(Line {
            sample: sample(utt, 700 + i64::from(utt), None),
            draft_passed: false,
        });
    }
    let columns = check_first_translation(&rust, &f33_python())?;
    assert_eq!(columns.all, stat(20, 200, 200));
    assert_eq!(columns.failed, stat(10, 200, 200));
    assert_eq!(columns.passed, stat(10, 200, 200));
    Ok(())
}

fn slice_rows(name: &str) -> Result<(Vec<Value>, Vec<Value>), Failure> {
    let windows = jsonl_rows(&read(
        &Path::new(SLICES).join(format!("{name}.windows.jsonl")),
    )?)?;
    let events = jsonl_rows(&read(
        &Path::new(SLICES).join(format!("{name}.events.jsonl")),
    )?)?;
    Ok((windows, events))
}

fn line_of(lines: &[Line], repeat: u32, utt: u32) -> Result<&Line, Failure> {
    lines
        .iter()
        .find(|line| line.sample.repeat == repeat && line.sample.utt == utt)
        .ok_or_else(|| format!("no line for repeat {repeat} utterance {utt}"))
}

/// T-RP-33-010
#[test]
fn a_row_without_a_t_draft_event_is_a_failed_draft() -> TestResult {
    let (windows, events) = slice_rows("a")?;
    let a = recount(&windows, &events)?;
    let passed = line_of(&a, 1, 2)?;
    assert!(
        passed.draft_passed,
        "utterance 2 of A has a `t` translation of its final"
    );
    assert!(passed.sample.t_tr_us.is_some());
    let held = line_of(&a, 1, 1)?;
    assert!(
        !held.draft_passed,
        "utterance 1 of A has only a held `e` translation"
    );
    assert_eq!(held.sample.t_tr_us, None);
    let (windows, events) = slice_rows("b")?;
    let b = recount(&windows, &events)?;
    let failed = line_of(&b, 2, 13)?;
    assert!(
        !failed.draft_passed,
        "utterance 13 of B has only an `e` translation"
    );
    assert!(failed.sample.t_tr_us.is_some());
    // A translation of the text in progress does not decide it: utterance 13
    // has `t` translations of its partials and still failed.
    assert!(events.iter().any(|row| {
        row["event"]["id"] == "mic-38"
            && row["event"]["of"] == "partial"
            && row["event"]["stage"] == "t"
    }));
    Ok(())
}

/// T-RP-33-011
#[test]
fn a_partial_t_translation_does_not_make_a_passed_draft() -> TestResult {
    let (windows, mut events) = slice_rows("a")?;
    let Some(row) = events.iter_mut().find(|row| {
        row["event"]["id"] == "mic-3"
            && row["event"]["of"] == "final"
            && row["event"]["stage"] == "t"
    }) else {
        return Err("slice A has no `t` translation of the final mic-3".into());
    };
    row["event"]["of"] = json!("partial");
    let lines = recount(&windows, &events)?;
    assert!(!line_of(&lines, 1, 2)?.draft_passed);
    Ok(())
}

fn python_r0() -> Result<Vec<Sample>, Box<dyn std::error::Error>> {
    Ok(read_samples(&base()?.join("lat_ja_0.30_1.jsonl"))?)
}

/// T-RP-33-012
#[test]
fn the_recorded_r8e_failed_drafts_are_slower_than_python() -> TestResult {
    let rust = recount_run(&rp3_run("jsonl"))?;
    let said = check_first_translation(&rust, &python_r0()?);
    rejected(&said, &["p50 rust 313 ms", "p95 rust 1237 ms"], &[]);
    let columns = stage_columns(&rust)?;
    assert_eq!(columns.all, stat(66, 313, 1237));
    assert_eq!(columns.passed, stat(47, 260, 439));
    assert_eq!(columns.failed, stat(19, 701, 1526));
    assert_eq!(samples_stage(&python_r0()?)?, stat(95, 212, 1019));
    // A record that is not there is named by its path.
    let missing = Path::new(RESULTS)
        .join("r8e_no_such_run")
        .join("rp3_rust.jsonl");
    let said = recount_run(&missing);
    assert!(
        said.as_ref()
            .is_err_and(|why| why.contains("r8e_no_such_run")),
        "{said:?}"
    );
    Ok(())
}

/// T-RP-33-013
#[test]
fn passed_and_failed_rows_add_up_to_all_rows() -> TestResult {
    let columns = stage_columns(&recount_run(&rp3_run("jsonl"))?)?;
    check_rows_add_up(&columns)?;
    assert_eq!((47 + 19, columns.all.map(|all| all.n)), (66, Some(66)));
    let broken = StageColumns {
        passed: stat(46, 260, 439),
        ..columns
    };
    assert!(check_rows_add_up(&broken).is_err());
    Ok(())
}

/// `L` of ten lines whose draft passed, 1000 to 1009 ms: the `final` at 800 ms.
fn passed_draft_run(l_ms: [i64; 10]) -> Vec<Line> {
    (1..=10)
        .zip(l_ms)
        .map(|(utt, l)| Line {
            sample: sample(utt, 800, Some(l)),
            draft_passed: true,
        })
        .collect()
}

const L_BEFORE_A: [i64; 10] = [1000, 1001, 1002, 1003, 1004, 1005, 1006, 1007, 1008, 1009];

/// T-RP-33-014
#[test]
fn a_passed_draft_l_equal_to_before_a_passes() -> TestResult {
    check_passed_draft_l(&passed_draft_run(L_BEFORE_A), &passed_draft_run(L_BEFORE_A))?;
    Ok(())
}

fn after_a_with(row: usize, l_ms: i64) -> Result<Vec<Line>, Failure> {
    let mut values = L_BEFORE_A;
    let Some(slot) = values.get_mut(row - 1) else {
        return Err(format!("no row {row}"));
    };
    *slot = l_ms;
    Ok(passed_draft_run(values))
}

/// T-RP-33-015
#[test]
fn a_slower_passed_draft_l_after_a_is_rejected() -> TestResult {
    let before = passed_draft_run(L_BEFORE_A);
    let p50 = check_passed_draft_l(&before, &after_a_with(5, 1005)?);
    assert!(
        p50.as_ref()
            .is_err_and(|why| why.contains("p50") && !why.contains("p95")),
        "{p50:?}"
    );
    let p95 = check_passed_draft_l(&before, &after_a_with(10, 1010)?);
    assert!(
        p95.as_ref()
            .is_err_and(|why| why.contains("p95") && !why.contains("p50")),
        "{p95:?}"
    );
    check_passed_draft_l(&before, &after_a_with(1, 1001)?)?;
    Ok(())
}

/// T-RP-33-016: the real runs of `docs/results/rp32/`, Python, Rust, Python, Rust, Python, Rust.
#[test]
#[ignore = "reads the runs in docs/results/rp32/"]
fn rp33_first_translation_after_a_failed_draft_beats_python() -> TestResult {
    let (rust, python) = load_real_runs(&rp32_dir())?;
    let said = check_first_translation(&rust, &python);
    if let Ok(columns) = &said {
        println!("rp33 rust:\n{}", render_stage_columns(columns));
    }
    assert!(said.is_ok(), "{said:?}");
    Ok(())
}

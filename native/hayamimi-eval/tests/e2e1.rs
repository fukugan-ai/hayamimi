//! E2E-1: a process on the microphone path (ja to en) is no slower than Python.
//!
//! The acceptance test reads the records of a real run and starts nothing:
//! `docs/results/e2e/e2e1/` holds the Rust samples `e2e1.jsonl` and the Python
//! runs `py<N>.jsonl` (see `support/e2e.rs`). It passes when the valid samples (3.1) got a
//! translation at a rate of at least 20 / 23, and the `L` p50 and p95 of the
//! Rust run are no greater than those of the Python runs pooled.
//!
//!     cargo test --release --test e2e1 -- --ignored --nocapture
//!
//! The other tests run on records and fixtures kept in the repository: the
//! aggregation, both ends of the verdict, a recorded run for a positive
//! control, and a run with no translation, which must fail.

mod support;

use std::path::{Path, PathBuf};

use hayamimi_eval::compare::read_samples;
use hayamimi_eval::latency::{Sample, Stat};
use support::e2e::{
    RecordList, events_path, judge_e2e1, judge_e2e1_files, list_run, load_pooled, load_run,
    report_of, run_dir, share_run, translated_after,
};
use support::{FIXTURES, RESULTS, TestResult};

fn fixture(name: &str) -> PathBuf {
    Path::new(FIXTURES).join(name)
}

/// A run kept under `docs/results/r8h_verify/`.
fn verify_run(name: &str) -> PathBuf {
    Path::new(RESULTS).join("r8h_verify").join(name)
}

fn fixture_samples(name: &str) -> Result<Vec<Sample>, Box<dyn std::error::Error>> {
    Ok(read_samples(&fixture(name))?)
}

#[test]
#[ignore = "reads the records in docs/results/e2e/e2e1/"]
fn e2e1_mic_process_ja_to_en_beats_python() -> TestResult {
    let judged = judge_e2e1_files(&load_run("e2e1")?)?;
    println!("{}", judged.render("e2e1"));
    assert!(judged.failures.is_empty(), "{}", judged.render("e2e1"));
    Ok(())
}

/// Of four samples one has a silence gap of 299.5 ms (not valid) and one
/// has no translation: 3 valid, 2 translated.
#[test]
fn the_columns_count_only_valid_samples() -> TestResult {
    let report = report_of(&fixture_samples("e2e_columns.jsonl")?)?;
    assert_eq!((report.share.valid, report.share.translated), (3, 2));
    let stat = |n, p50_ms, p95_ms| Some(Stat { n, p50_ms, p95_ms });
    assert_eq!(report.columns.l, stat(2, 1500, 2000));
    assert_eq!(report.columns.l_asr, stat(3, 1000, 1300));
    assert_eq!(report.columns.l_tr, stat(2, 200, 1000));
    Ok(())
}

#[test]
fn a_run_with_no_sample_has_no_column() -> TestResult {
    let report = report_of(&[])?;
    assert_eq!((report.share.valid, report.share.translated), (0, 0));
    assert_eq!(report.columns.l, None);
    assert_eq!(report.columns.l_asr, None);
    Ok(())
}

#[test]
fn a_run_faster_than_python_and_fully_translated_passes() -> TestResult {
    let python = fixture_samples("e2e_columns.jsonl")?;
    let rust = translated_after(&python, 100_000);
    let judged = judge_e2e1(&rust, &python)?;
    assert!(judged.failures.is_empty(), "{}", judged.render("e2e1"));
    Ok(())
}

#[test]
fn a_run_slower_than_python_fails_on_both_percentiles() -> TestResult {
    let python = fixture_samples("e2e_columns.jsonl")?;
    let rust = translated_after(&python, 1_000_000);
    let judged = judge_e2e1(&rust, &python)?;
    assert_eq!(judged.failures.len(), 1, "{:?}", judged.failures);
    assert!(judged.failures[0].contains("p50 rust 2000 ms > python 1500 ms"));
    assert!(judged.failures[0].contains("p95 rust 2300 ms > python 2000 ms"));
    Ok(())
}

/// The same run on both sides ties on `L`, and a tie is not slower; only the
/// translated share (2 of 3) is below 20 / 23.
#[test]
fn equal_percentiles_are_not_slower() -> TestResult {
    let python = fixture_samples("e2e_columns.jsonl")?;
    let judged = judge_e2e1(&python, &python)?;
    assert_eq!(judged.failures.len(), 1, "{:?}", judged.failures);
    assert!(judged.failures[0].contains("2 of 3 valid samples have a translation"));
    Ok(())
}

/// The negative control: a run that translated nothing fails, however fast it is.
#[test]
fn a_run_with_no_translation_fails() -> TestResult {
    let python = fixture_samples("e2e_columns.jsonl")?;
    let rust = fixture_samples("e2e_no_translation.jsonl")?;
    let judged = judge_e2e1(&rust, &python)?;
    let said = judged.failures.join("; ");
    assert!(
        said.contains("0 of 3 valid samples have a translation"),
        "{said}"
    );
    assert!(
        said.contains("rust: no valid sample has a translation"),
        "{said}"
    );
    Ok(())
}

#[test]
fn an_empty_run_is_not_a_pass() -> TestResult {
    let python = fixture_samples("e2e_columns.jsonl")?;
    let judged = judge_e2e1(&[], &python)?;
    assert!(
        judged
            .failures
            .iter()
            .any(|why| why.contains("no valid sample"))
    );
    Ok(())
}

/// The positive control: the recorded Rust run `rs1` against the recorded
/// Python runs `py1` and `py2` pooled gives numbers. Counted with `jq` on the
/// same files: 40 valid, 34 translated; 80 valid, 76 translated.
#[test]
fn a_recorded_run_gives_its_columns() -> TestResult {
    let rust = load_pooled(&[verify_run("rs1.jsonl")])?;
    let python = load_pooled(&[verify_run("py1.jsonl"), verify_run("py2.jsonl")])?;
    let judged = judge_e2e1(&rust, &python)?;
    assert_eq!(
        (judged.rust.share.valid, judged.rust.share.translated),
        (40, 34)
    );
    let Some(python_report) = judged.python else {
        return Err("the judgement holds no Python report".into());
    };
    assert_eq!(
        (python_report.share.valid, python_report.share.translated),
        (80, 76)
    );
    assert!(judged.rust.columns.l.is_some() && python_report.columns.l.is_some());
    let rendered = judged.render("e2e1");
    assert!(
        rendered.contains("e2e1 rust: 34 of 40 valid samples translated"),
        "{rendered}"
    );
    assert!(
        rendered.contains("e2e1 python: 76 of 80 valid samples translated"),
        "{rendered}"
    );
    Ok(())
}

#[test]
fn a_run_directory_lists_its_samples_and_the_python_runs_in_name_order() -> TestResult {
    let dir = Path::new(RESULTS).join("r8h_verify");
    let list = list_run(&dir, "rs1")?;
    // `py1.events.jsonl` and `py1.summary.json` stand beside `py1.jsonl` and are not runs.
    let expected = RecordList {
        rust: dir.join("rs1.jsonl"),
        python: vec![dir.join("py1.jsonl"), dir.join("py2.jsonl")],
    };
    assert_eq!(list, expected);
    Ok(())
}

#[test]
fn a_run_directory_without_the_samples_says_what_to_copy() {
    let said = list_run(Path::new(RESULTS), "no_such_run");
    assert!(
        said.as_ref()
            .is_err_and(|m| m.contains("the E2E seat copies")),
        "{said:?}"
    );
}

#[test]
fn the_fixed_directory_of_a_run_is_under_docs_results_e2e() {
    assert!(run_dir("e2e1").ends_with("docs/results/e2e/e2e1"));
}

#[test]
fn e2e1_without_a_python_run_is_refused() {
    let list = RecordList {
        rust: fixture("e2e_columns.jsonl"),
        python: Vec::new(),
    };
    let said = judge_e2e1_files(&list);
    assert!(
        said.as_ref().is_err_and(|m| m.contains("py<N>.jsonl")),
        "{said:?}"
    );
}

/// The edge of the translated share, 20 / 23 = 0.8696: 70 of 80 (0.875) is
/// over it, 69 of 80 (0.8625) is under it. Python has the same `L` (1400 ms)
/// on both sides, so only the share can fail.
#[test]
fn the_translated_share_passes_at_70_of_80_and_fails_at_69() -> TestResult {
    let python = share_run(80, 80);
    let at_70 = judge_e2e1(&share_run(80, 70), &python)?;
    assert!(at_70.failures.is_empty(), "{:?}", at_70.failures);
    let at_69 = judge_e2e1(&share_run(80, 69), &python)?;
    assert_eq!(at_69.failures.len(), 1, "{:?}", at_69.failures);
    assert!(at_69.failures[0].contains("69 of 80 valid samples have a translation"));
    Ok(())
}

#[test]
fn the_events_file_stands_beside_the_samples_file() {
    assert_eq!(
        events_path(Path::new("/r/obs2.jsonl")),
        PathBuf::from("/r/obs2.events.jsonl")
    );
}

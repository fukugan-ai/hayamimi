//! E2E-1: a process on the microphone path (ja to en) is no slower than Python.
//!
//! The acceptance test reads the records of a real run and starts nothing:
//! `docs/results/e2e/e2e1/` holds the Rust samples `e2e1.jsonl` and the Python
//! runs `py<N>.jsonl` (see `support/e2e.rs`). It passes when the valid samples (3.1) got a
//! translation at a rate of at least 20 / 23, and the `L` p50 and p95 of the
//! Rust run are no greater than those of the Python runs pooled. A Rust sample
//! is valid against the line of the rule that closed it, read from the
//! `hayamimi` stdout beside the samples; a Python sample against `min_silence`.
//!
//!     cargo test --release --test e2e1 -- --ignored --nocapture
//!
//! The other tests run on records and fixtures kept in the repository: the
//! aggregation, both ends of the verdict, a recorded run for a positive
//! control, a run with no translation, which must fail, and the lines of the
//! rules (F88).

mod support;

use std::path::{Path, PathBuf};

use hayamimi_eval::compare::read_samples;
use hayamimi_eval::latency::{Sample, Stat};
use support::e2e::{
    Counting, Judged, RecordList, Side, append_newline, copy_record, events_path, judge_e2e1,
    judge_e2e1_files, list_run, load_pooled, load_run, load_rust, report_of, run_dir, share_run,
    translated_after, write_f88,
};
use support::rp32::scratch_dir;
use support::{FIXTURES, Failure, REPO, RESULTS, TestResult};

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

/// E2E-1 over the F88 run `samples` against the Python run `e2e_columns.jsonl`.
fn judge_f88(samples: &Path) -> Result<Judged, Failure> {
    let list = RecordList {
        rust: samples.to_path_buf(),
        python: vec![fixture("e2e_columns.jsonl")],
    };
    judge_e2e1_files(Path::new(REPO), &list)
}

/// The refusal of `judged`, which has to name every one of `names`.
fn assert_refused_naming(judged: &Result<Judged, Failure>, names: &[&str]) {
    assert!(
        judged
            .as_ref()
            .err()
            .is_some_and(|why| names.iter().all(|name| why.contains(name))),
        "{judged:?}"
    );
}

#[test]
#[ignore = "reads the records in docs/results/e2e/e2e1/"]
fn e2e1_mic_process_ja_to_en_beats_python() -> TestResult {
    let judged = judge_e2e1_files(Path::new(REPO), &load_run("e2e1")?)?;
    println!("{}", judged.render("e2e1"));
    assert!(judged.failures.is_empty(), "{}", judged.render("e2e1"));
    Ok(())
}

/// Of four samples one has a silence gap of 299.5 ms (not valid) and one
/// has no translation: 3 valid, 2 translated. The rules beside the samples
/// are all `silence`, so every line is 300 ms.
#[test]
fn the_columns_count_only_valid_samples() -> TestResult {
    let rust = load_rust(Path::new(REPO), &fixture("e2e_columns.jsonl"))?;
    let report = report_of(&rust)?;
    assert_eq!((report.share.valid, report.share.translated), (3, 2));
    let stat = |n, p50_ms, p95_ms| Some(Stat { n, p50_ms, p95_ms });
    assert_eq!(report.columns.l, stat(2, 1500, 2000));
    assert_eq!(report.columns.l_asr, stat(3, 1000, 1300));
    assert_eq!(report.columns.l_tr, stat(2, 200, 1000));
    Ok(())
}

#[test]
fn a_run_with_no_sample_has_no_column() -> TestResult {
    let report = report_of(&Side::flat(Vec::new()))?;
    assert_eq!((report.share.valid, report.share.translated), (0, 0));
    assert_eq!(report.columns.l, None);
    assert_eq!(report.columns.l_asr, None);
    Ok(())
}

#[test]
fn a_run_faster_than_python_and_fully_translated_passes() -> TestResult {
    let python = fixture_samples("e2e_columns.jsonl")?;
    let rust = translated_after(&python, 100_000);
    let judged = judge_e2e1(&Side::flat(rust), &Side::flat(python))?;
    assert!(judged.failures.is_empty(), "{}", judged.render("e2e1"));
    Ok(())
}

#[test]
fn a_run_slower_than_python_fails_on_both_percentiles() -> TestResult {
    let python = fixture_samples("e2e_columns.jsonl")?;
    let rust = translated_after(&python, 1_000_000);
    let judged = judge_e2e1(&Side::flat(rust), &Side::flat(python))?;
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
    let judged = judge_e2e1(&Side::flat(python.clone()), &Side::flat(python))?;
    assert_eq!(judged.failures.len(), 1, "{:?}", judged.failures);
    assert!(judged.failures[0].contains("2 of 3 valid samples have a translation"));
    Ok(())
}

/// The negative control: a run that translated nothing fails, however fast it is.
#[test]
fn a_run_with_no_translation_fails() -> TestResult {
    let python = fixture_samples("e2e_columns.jsonl")?;
    let rust = fixture_samples("e2e_no_translation.jsonl")?;
    let judged = judge_e2e1(&Side::flat(rust), &Side::flat(python))?;
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
    let judged = judge_e2e1(&Side::flat(Vec::new()), &Side::flat(python))?;
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
/// same files: 40 valid, 34 translated; 80 valid, 76 translated. `rs1` is a
/// listed record of a build without the early rule and has no stdout, so every
/// sample is counted at `min_silence` and the output says so.
#[test]
fn a_recorded_run_gives_its_columns() -> TestResult {
    let rust = load_rust(Path::new(REPO), &verify_run("rs1.jsonl"))?;
    let python = load_pooled(&[verify_run("py1.jsonl"), verify_run("py2.jsonl")])?;
    let judged = judge_e2e1(&rust, &Side::flat(python))?;
    assert_eq!(
        (judged.rust.share.valid, judged.rust.share.translated),
        (40, 34)
    );
    assert_eq!(judged.rust.counting, Counting::Listed);
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
    assert!(
        rendered.contains("every sample counted at min_silence"),
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
    let said = judge_e2e1_files(Path::new(REPO), &list);
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
    let python = Side::flat(share_run(80, 80));
    let at_70 = judge_e2e1(&Side::flat(share_run(80, 70)), &python)?;
    assert!(at_70.failures.is_empty(), "{:?}", at_70.failures);
    let at_69 = judge_e2e1(&Side::flat(share_run(80, 69)), &python)?;
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

/// T-E2E-1-016: F88 closed by the early rule has a gap of 200 ms over the
/// line of 120 ms, so it is valid. At `min_silence` for every sample it would
/// not be.
#[test]
fn an_early_sample_of_200_ms_is_valid() -> TestResult {
    let run = write_f88("e2e1-t016", &["mic-1"], Some(&[(1, "early")]))?;
    let judged = judge_f88(&run)?;
    assert_eq!((judged.rust.share.valid, judged.rust.bad_end), (1, 0));
    let flat = judged
        .rust
        .at_min_silence
        .map(|counted| counted.share.valid);
    assert_eq!(flat, Some(0));
    Ok(())
}

/// T-E2E-1-017: F88 closed by the silence rule has the same gap of 200 ms
/// under the line of 300 ms, so it is a bad end. The pair of 016.
#[test]
fn a_silence_sample_of_200_ms_is_not_valid() -> TestResult {
    let run = write_f88("e2e1-t017", &["mic-1"], Some(&[(1, "silence")]))?;
    let judged = judge_f88(&run)?;
    assert_eq!((judged.rust.share.valid, judged.rust.bad_end), (0, 1));
    Ok(())
}

/// T-E2E-1-018: a Rust run off the list with no stdout is refused by its name,
/// and so is a listed run whose content differs (one newline added).
#[test]
fn a_rust_run_off_the_list_without_a_stdout_is_rejected() -> TestResult {
    let dir = scratch_dir("e2e1-018", "off-list")?;
    let off_list = dir.join("rs9.jsonl");
    std::fs::copy(fixture("e2e_columns.jsonl"), &off_list)?;
    let list = RecordList {
        rust: off_list,
        python: vec![fixture("e2e_columns.jsonl")],
    };
    assert_refused_naming(
        &judge_e2e1_files(Path::new(REPO), &list),
        &["rs9", "hayamimi.stdout.log"],
    );
    let root = scratch_dir("e2e1-018", "root")?;
    let changed = copy_record(&root, "e2e/e2e1/e2e1.jsonl")?;
    append_newline(&changed)?;
    let python = copy_record(&root, "e2e/e2e1/py1.jsonl")?;
    let list = RecordList {
        rust: changed,
        python: vec![python],
    };
    assert_refused_naming(
        &judge_e2e1_files(&root, &list),
        &["e2e1", "hayamimi.stdout.log"],
    );
    Ok(())
}

/// T-E2E-1-019: the listed record `e2e1` has no stdout, so every sample is
/// counted at `min_silence`: 100 valid, 93 translated, and the output says so.
#[test]
fn the_listed_e2e1_run_is_counted_at_min_silence_without_a_stdout() -> TestResult {
    let root = scratch_dir("e2e1-019", "root")?;
    let rust = copy_record(&root, "e2e/e2e1/e2e1.jsonl")?;
    copy_record(&root, "e2e/e2e1/py1.jsonl")?;
    let dir = rust.parent().ok_or("a record has a directory")?;
    let judged = judge_e2e1_files(&root, &list_run(dir, "e2e1")?)?;
    assert_eq!(
        (judged.rust.share.valid, judged.rust.share.translated),
        (100, 93)
    );
    assert_eq!(judged.rust.counting, Counting::Listed);
    let rendered = judged.render("e2e1");
    assert!(
        rendered.contains("no hayamimi stdout") && rendered.contains("counted at min_silence"),
        "{rendered}"
    );
    Ok(())
}

/// T-E2E-1-020: `rs1` of `r8h_verify` with one newline added is not the listed
/// record. The unchanged copy passes, which is the control.
#[test]
fn a_listed_name_with_another_sha256_is_not_exempt_in_e2e1() -> TestResult {
    let kept = scratch_dir("e2e1-020", "kept")?;
    let unchanged = copy_record(&kept, "r8h_verify/rs1.jsonl")?;
    assert!(load_rust(&kept, &unchanged).is_ok());
    let root = scratch_dir("e2e1-020", "changed")?;
    let changed = copy_record(&root, "r8h_verify/rs1.jsonl")?;
    append_newline(&changed)?;
    let said = load_rust(&root, &changed);
    assert!(
        said.as_ref()
            .err()
            .is_some_and(|why| why.contains("rs1") && why.contains("hayamimi.stdout.log")),
        "{said:?}"
    );
    Ok(())
}

/// T-E2E-1-021: two `final`s at the moment F88 closed. Ended by different
/// rules they are refused with both ids named; ended by the same rule one line
/// is chosen, which is the control.
#[test]
fn two_finals_with_different_rules_at_one_t_us_are_rejected() -> TestResult {
    let ids = ["mic-1", "mic-2"];
    let split = write_f88(
        "e2e1-t021-split",
        &ids,
        Some(&[(1, "early"), (2, "silence")]),
    )?;
    assert_refused_naming(&judge_f88(&split), &["f88", "mic-1", "mic-2"]);
    let same = write_f88("e2e1-t021-same", &ids, Some(&[(1, "early"), (2, "early")]))?;
    let judged = judge_f88(&same)?;
    assert_eq!((judged.rust.share.valid, judged.rust.bad_end), (1, 0));
    Ok(())
}

/// T-E2E-1-022: a `final` that no decided row joins is refused by its id.
#[test]
fn a_final_without_a_decided_row_is_rejected() -> TestResult {
    let run = write_f88("e2e1-t022", &["mic-1"], Some(&[(9, "early")]))?;
    assert_refused_naming(&judge_f88(&run), &["f88", "mic-1"]);
    Ok(())
}

/// T-E2E-1-023: the same id decided twice is refused, even with the same rule.
#[test]
fn two_decided_rows_with_one_id_are_rejected_even_with_the_same_rule() -> TestResult {
    let run = write_f88("e2e1-t023", &["mic-1"], Some(&[(1, "early"), (1, "early")]))?;
    assert_refused_naming(&judge_f88(&run), &["f88", "id 1"]);
    Ok(())
}

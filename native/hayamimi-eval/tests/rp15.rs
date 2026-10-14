//! RP-15: the Rust route labels speakers as well as the Python one does.
//!
//! `hayamimi-eval der --set testdata/eval_diar --collar 0.25` scores the five
//! AMI meetings; its log is `docs/results/r4/der_rust_run<n>.log`. The mean
//! DER of the Rust runs must not be above the mean of at least three Python
//! `--method refine_diarize` runs under the baseline directory. The spread of
//! the Python runs (the largest mean minus the smallest) is written beside the
//! result: a difference smaller than it is read as within the run-to-run
//! spread of Python itself.

mod support;

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use hayamimi_eval::der::{Turn, der};
use support::der_logs::{
    MIN_DER_RUNS, check_der, check_der_run_count, check_refine_diarize, der_run_logs, mean_of_runs,
    numbered_logs, spread_of_runs,
};
use support::{Failure, RESULTS, TestResult, read};

/// How the Rust mean stands against the Python mean.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reading {
    /// The Rust mean is not above the Python mean.
    NotWorse,
    /// Worse, by less than the spread of the Python runs.
    WithinSpread,
    /// Worse, by the spread or more.
    OutsideSpread,
}

/// Both sides in units of 10^-6 points.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Judgement {
    python_runs: Vec<i64>,
    python_mean: i64,
    python_spread: i64,
    rust_runs: Vec<i64>,
    rust_mean: i64,
    reading: Reading,
}

fn run_means(logs: &[&str]) -> Result<Vec<i64>, Failure> {
    logs.iter().map(|log| check_der(log)).collect()
}

fn reading_of(rust_mean: i64, python_mean: i64, spread: i64) -> Reading {
    let worse_by = rust_mean - python_mean;
    if worse_by <= 0 {
        Reading::NotWorse
    } else if worse_by < spread {
        Reading::WithinSpread
    } else {
        Reading::OutsideSpread
    }
}

/// Reads the logs of both sides. Fewer than three Python runs fix no spread.
fn judge(rust_logs: &[&str], python_logs: &[&str]) -> Result<Judgement, Failure> {
    check_der_run_count(python_logs)?;
    let python_runs = run_means(python_logs)?;
    let rust_runs = run_means(rust_logs)?;
    let python_mean = mean_of_runs(&python_runs)?;
    let python_spread = spread_of_runs(&python_runs)?;
    let rust_mean = mean_of_runs(&rust_runs)?;
    Ok(Judgement {
        reading: reading_of(rust_mean, python_mean, python_spread),
        python_runs,
        python_mean,
        python_spread,
        rust_runs,
        rust_mean,
    })
}

/// The pass: the Rust mean minus the Python mean is below the spread of the
/// Python runs (a Rust mean at or under the Python mean always passes).
fn check_not_worse(judgement: &Judgement) -> Result<(), Failure> {
    if judgement.reading != Reading::OutsideSpread {
        return Ok(());
    }
    Err(format!(
        "the Rust mean DER {} is above the Python mean {} by {}, not below the spread {}",
        points(judgement.rust_mean),
        points(judgement.python_mean),
        points(judgement.rust_mean - judgement.python_mean),
        points(judgement.python_spread)
    ))
}

/// Units of 10^-6 points as `13.30`.
fn points(micros: i64) -> String {
    format!(
        "{}.{:02}",
        micros / 1_000_000,
        (micros % 1_000_000) / 10_000
    )
}

fn signed_points(micros: i64) -> String {
    let sign = if micros < 0 { "-" } else { "" };
    format!("{sign}{}", points(micros.abs()))
}

fn listed(runs: &[i64]) -> String {
    let each: Vec<String> = runs.iter().map(|run| points(*run)).collect();
    each.join(", ")
}

fn reading_text(reading: Reading) -> &'static str {
    match reading {
        Reading::NotWorse => "Rust is not worse than the Python mean",
        Reading::WithinSpread => {
            "worse by less than the spread of the Python runs: within the spread"
        }
        Reading::OutsideSpread => {
            "worse by the spread of the Python runs or more: outside the spread"
        }
    }
}

/// The result file: both sides' runs, the means, the spread, the difference and the reading.
fn report(judgement: &Judgement) -> Result<String, std::fmt::Error> {
    let mut text =
        String::from("# RP-15 speaker separation: mean DER, AMI 5 meetings, collar 0.25\n\n");
    writeln!(
        text,
        "Python runs ({MIN_DER_RUNS} or more, `--method refine_diarize`): {}",
        listed(&judgement.python_runs)
    )?;
    writeln!(text, "Python mean: {}", points(judgement.python_mean))?;
    writeln!(text, "Python spread: {}", points(judgement.python_spread))?;
    writeln!(text, "Rust runs: {}", listed(&judgement.rust_runs))?;
    writeln!(text, "Rust mean: {}", points(judgement.rust_mean))?;
    writeln!(
        text,
        "Difference (Rust minus Python): {}",
        signed_points(judgement.rust_mean - judgement.python_mean)
    )?;
    writeln!(text, "Reading: {}", reading_text(judgement.reading))?;
    Ok(text)
}

fn logs_of(directory: &Path, prefix: &str) -> Result<Vec<String>, Failure> {
    numbered_logs(directory, None, prefix)?
        .iter()
        .map(|path| read(path))
        .collect()
}

fn borrowed(logs: &[String]) -> Vec<&str> {
    logs.iter().map(String::as_str).collect()
}

/// A log with one mean DER line, as a run prints it.
fn run_with_mean(mean: &str) -> String {
    format!("=== mean DER over 5 meeting(s): {mean}% (collar=0.25s) ===")
}

fn python_runs(means: &[&str]) -> Vec<String> {
    means.iter().map(|mean| run_with_mean(mean)).collect()
}

fn judge_means(rust: &str, python: &[&str]) -> Result<Judgement, Failure> {
    let rust_log = run_with_mean(rust);
    judge(&[rust_log.as_str()], &borrowed(&python_runs(python)))
}

const PYTHON_13_3: [&str; 3] = ["13.3", "13.1", "13.5"];

/// T-RP-15-001
#[test]
fn equal_mean_der_passes() -> TestResult {
    let judgement = judge_means("13.3", &PYTHON_13_3)?;
    check_not_worse(&judgement)?;
    assert_eq!(judgement.reading, Reading::NotWorse);
    Ok(())
}

/// T-RP-15-002
#[test]
fn mean_der_one_tenth_worse_is_rejected() -> TestResult {
    // Python runs 13.275, 13.3, 13.325: mean 13.30, spread 0.05.
    let judgement = judge_means("13.4", &["13.275", "13.3", "13.325"])?;
    assert_eq!(judgement.python_spread, 50_000);
    assert!(check_not_worse(&judgement).is_err());
    Ok(())
}

/// T-RP-15-003
#[test]
fn four_meetings_are_rejected() {
    let four = "=== mean DER over 4 meeting(s): 13.3% (collar=0.25s) ===";
    let python = python_runs(&PYTHON_13_3);
    let said = judge(&[four], &borrowed(&python)).err();
    assert!(
        said.as_deref()
            .is_some_and(|text| text.contains("4 meetings")),
        "{said:?}"
    );
}

/// T-RP-15-004
#[test]
fn der_of_half_confused_speech_is_0_5() {
    let reference = [Turn::new("A", 0.0, 10.0)];
    let hypothesis = [Turn::new("A", 0.0, 5.0), Turn::new("B", 5.0, 10.0)];
    assert!((der(&reference, &hypothesis, 0.0) - 0.5).abs() < 1e-12);
}

/// T-RP-15-005
#[test]
fn der_of_identical_segments_is_zero() {
    let turns = [Turn::new("A", 0.0, 3.0), Turn::new("B", 3.0, 8.0)];
    assert!(der(&turns, &turns, 0.0).abs() < 1e-12);
}

/// T-RP-15-006
#[test]
fn python_der_runs_yield_mean_and_spread_from_three_or_more_refine_diarize_runs() -> TestResult {
    let logs = der_run_logs()?;
    for log in &logs {
        check_refine_diarize(log)?;
    }
    let borrowed = borrowed(&logs);
    check_der_run_count(&borrowed)?;
    let runs = run_means(&borrowed)?;
    assert!(runs.len() >= MIN_DER_RUNS);
    assert!(mean_of_runs(&runs)? > 0);
    assert!(spread_of_runs(&runs)? >= 0);
    Ok(())
}

/// T-RP-15-007
#[test]
fn rediarization_off_fixture_is_worse_than_python() -> TestResult {
    let off = include_str!("fixtures/rb07_der_default_method.log");
    assert!(check_refine_diarize(off).is_err());
    let python = python_runs(&PYTHON_13_3);
    let judgement = judge(&[off], &borrowed(&python))?;
    assert!(check_not_worse(&judgement).is_err());
    assert_eq!(judgement.reading, Reading::OutsideSpread);
    Ok(())
}

/// T-RP-15-008
#[test]
#[ignore = "reads the logs written by `hayamimi-eval der`"]
fn rust_mean_der_not_worse_than_python() -> TestResult {
    let directory = Path::new(RESULTS).join("r4");
    let rust = logs_of(&directory, "der_rust_run")?;
    for log in &rust {
        check_refine_diarize(log)?;
    }
    let python = der_run_logs()?;
    let judgement = judge(&borrowed(&rust), &borrowed(&python))?;
    fs::write(directory.join("rp15_der.md"), report(&judgement)?)?;
    check_not_worse(&judgement)?;
    Ok(())
}

/// T-RP-15-009
#[test]
fn two_python_der_runs_cannot_fix_a_spread() {
    let python = python_runs(&["13.3", "13.1"]);
    let rust = run_with_mean("13.0");
    let said = judge(&[rust.as_str()], &borrowed(&python)).err();
    assert!(
        said.as_deref()
            .is_some_and(|text| text.contains("2 DER runs")),
        "{said:?}"
    );
}

/// T-RP-15-010
#[test]
fn worse_than_python_mean_by_less_than_the_spread_passes() -> TestResult {
    let judgement = judge_means("13.69", &PYTHON_13_3)?;
    assert_eq!(judgement.python_mean, 13_300_000);
    assert_eq!(judgement.python_spread, 400_000);
    check_not_worse(&judgement)?;
    assert_eq!(judgement.reading, Reading::WithinSpread);
    Ok(())
}

/// T-RP-15-011
#[test]
fn worse_than_python_mean_by_exactly_the_spread_is_rejected() -> TestResult {
    let judgement = judge_means("13.70", &PYTHON_13_3)?;
    assert!(check_not_worse(&judgement).is_err());
    assert_eq!(judgement.reading, Reading::OutsideSpread);
    Ok(())
}

#[test]
fn the_result_file_names_the_means_the_spread_and_the_reading() -> TestResult {
    let judgement = judge_means("13.69", &PYTHON_13_3)?;
    let text = report(&judgement)?;
    for want in [
        "Python mean: 13.30",
        "Python spread: 0.40",
        "Rust mean: 13.69",
        "Difference (Rust minus Python): 0.39",
        "within the spread",
    ] {
        assert!(text.contains(want), "{want:?} is missing from\n{text}");
    }
    Ok(())
}

#[test]
fn points_are_written_with_two_decimals() {
    assert_eq!(points(13_300_000), "13.30");
    assert_eq!(points(130_000), "0.13");
    assert_eq!(signed_points(-130_000), "-0.13");
}

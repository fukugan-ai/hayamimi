//! RP-36: the Korean recognition error of the shipped build stays within the
//! cap recorded for it, compared as an integer in millionths with no rounding.
//! The cap is the entry bar of R11; `check_scorecard` of RP-1 keeps its own
//! three-decimal comparison and is not touched.

mod support;

use std::path::Path;

use hayamimi_eval::scorecard::{Scorecard, read_json};
use support::{FIXTURES, RESULTS, TestResult};

/// The Korean mean error of the shipped Windows build, in millionths.
const KO_CAP_MICROS: u64 = 86_939;

/// Why a record does not pass the cap.
#[derive(Debug, PartialEq, Eq)]
enum CapError {
    NoKoRow,
    OverCap(u64),
}

/// The Korean mean error of `card` in millionths, when it is within the cap.
fn check_ko_cap(card: &Scorecard) -> Result<u64, CapError> {
    let Some(ko) = card.rows.iter().find(|row| row.lang == "ko") else {
        return Err(CapError::NoKoRow);
    };
    if ko.mean_err_micros > KO_CAP_MICROS {
        return Err(CapError::OverCap(ko.mean_err_micros));
    }
    Ok(ko.mean_err_micros)
}

fn recorded(name: &str) -> Result<Scorecard, Box<dyn std::error::Error>> {
    Ok(read_json(&Path::new(RESULTS).join("r3c").join(name))?)
}

/// T-RP-36-001: the scorecard run of the shipped build on Windows.
#[test]
#[ignore = "reads the scorecard of the shipped build"]
fn rp36_ko_error_within_the_shipped_build_cap() -> TestResult {
    let card = recorded("win_routed.json")?;
    let ko = check_ko_cap(&card).map_err(|why| format!("{why:?}"))?;
    println!("ko mean_err_micros = {ko}");
    Ok(())
}

/// T-RP-36-002, the positive control: the run that loads the Python wheel's
/// ONNX Runtime reads 80572 and passes the same check.
#[test]
fn rp36_python_wheel_run_reads_ko_80572() -> TestResult {
    let card = recorded("win_python_libs.json")?;
    assert_eq!(check_ko_cap(&card), Ok(80_572));
    Ok(())
}

/// T-RP-36-003, the negative control: a record whose Korean mean is 86940
/// is over the cap.
#[test]
fn rp36_ko_86940_is_over_the_cap() -> TestResult {
    let card = read_json(&Path::new(FIXTURES).join("rp36_ko_86940.json"))?;
    assert_eq!(check_ko_cap(&card), Err(CapError::OverCap(86_940)));
    Ok(())
}

/// T-RP-36-004: a record with no Korean row is refused, not passed.
#[test]
fn rp36_a_record_without_a_ko_row_is_refused() -> TestResult {
    let mut card = recorded("win_python_libs.json")?;
    card.rows.retain(|row| row.lang != "ko");
    assert_eq!(check_ko_cap(&card), Err(CapError::NoKoRow));
    Ok(())
}

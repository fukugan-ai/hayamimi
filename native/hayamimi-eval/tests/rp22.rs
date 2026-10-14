//! RP-22: the memory in use on the GPU stays within the budget. The cases here
//! read the output of `nvidia-smi` saved as fixtures. The cases that run the
//! binary (T-RP-22-007 to T-RP-22-011) are in the `hayamimi` package's `rp22.rs`.

mod support;

use std::path::Path;

use hayamimi_eval::vram::{BUDGET_MIB, VramError, check_vram, parse_reading, parse_readings};
use support::{FIXTURES, TestResult, read};

/// The readings in a fixture file.
fn readings(name: &str) -> Result<Vec<u64>, Box<dyn std::error::Error>> {
    let text = read(&Path::new(FIXTURES).join(name))?;
    Ok(parse_readings(&text)?)
}

/// T-RP-22-001
#[test]
fn peak_of_14000_mib_passes() -> TestResult {
    let during = readings("rp22_peak_14000.csv")?;
    assert_eq!(check_vram(&[1198], &during, BUDGET_MIB), Ok(14_000));
    Ok(())
}

/// T-RP-22-002
#[test]
fn peak_of_14001_mib_is_rejected() -> TestResult {
    let during = readings("rp22_peak_14001.csv")?;
    assert!(matches!(
        check_vram(&[1198], &during, BUDGET_MIB),
        Err(VramError::OverBudget {
            peak_mib: 14_001,
            ..
        })
    ));
    Ok(())
}

/// T-RP-22-003
#[test]
fn no_samples_is_an_error() -> TestResult {
    let during = readings("rp22_no_readings.csv")?;
    assert!(during.is_empty());
    assert_eq!(
        check_vram(&[1198], &during, BUDGET_MIB),
        Err(VramError::NoReadingDuring)
    );
    Ok(())
}

/// T-RP-22-004
#[test]
fn pre_start_reading_is_available() -> TestResult {
    let before = readings("rp22_before_start.csv")?;
    assert_eq!(before, [1161, 1161, 1162]);
    assert_eq!(check_vram(&before, &[1500], BUDGET_MIB), Ok(1500));
    assert_eq!(
        check_vram(&[], &[1500], BUDGET_MIB),
        Err(VramError::NoReadingBefore)
    );
    Ok(())
}

/// T-RP-22-005
#[test]
fn unit_suffix_is_parsed_and_missing_unit_is_rejected() -> TestResult {
    assert_eq!(parse_reading("1161 MiB"), Ok(1161));
    let text = read(&Path::new(FIXTURES).join("rp22_no_unit.csv"))?;
    assert_eq!(
        parse_readings(&text),
        Err(VramError::BadReading(String::from("1161")))
    );
    Ok(())
}

/// T-RP-22-006
#[test]
fn reading_over_the_budget_is_rejected() -> TestResult {
    let during = readings("rp22_peak_16000.csv")?;
    let said = check_vram(&[1198], &during, BUDGET_MIB)
        .err()
        .map(|error| error.to_string());
    assert!(
        said.as_deref().is_some_and(|text| text.contains("16000")),
        "{said:?}"
    );
    Ok(())
}

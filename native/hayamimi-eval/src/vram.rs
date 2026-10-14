//! RP-22: the memory in use on the whole GPU stays within the budget.
//!
//! `nvidia-smi --query-gpu=memory.used --format=csv` prints a header line and
//! then one line such as `1161 MiB` per call. The unit is required: a figure
//! without it is not a reading. Every process on the GPU is included, so a
//! language model server running beside the program counts against it.

use std::fmt;

/// The most MiB the run may use on the GPU.
pub const BUDGET_MIB: u64 = 14_000;

/// The first word of the header line `nvidia-smi` prints before the readings.
const HEADER: &str = "memory.used";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VramError {
    /// A line is not a figure in MiB.
    BadReading(String),
    /// Nothing was read before the start, so the machine's own use is unknown.
    NoReadingBefore,
    /// Nothing was read while the run was going.
    NoReadingDuring,
    /// A reading reached past the budget.
    OverBudget { peak_mib: u64, budget_mib: u64 },
}

impl fmt::Display for VramError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VramError::BadReading(text) => write!(f, "`{text}` is not a memory figure in MiB"),
            VramError::NoReadingBefore => write!(f, "no memory read before the start"),
            VramError::NoReadingDuring => write!(f, "no memory read while the run was going"),
            VramError::OverBudget {
                peak_mib,
                budget_mib,
            } => write!(
                f,
                "{peak_mib} MiB in use, over the budget of {budget_mib} MiB"
            ),
        }
    }
}

impl std::error::Error for VramError {}

/// One reading: `1161 MiB`.
pub fn parse_reading(line: &str) -> Result<u64, VramError> {
    let bad = || VramError::BadReading(line.trim().to_owned());
    let figure = line.trim().strip_suffix("MiB").ok_or_else(bad)?;
    figure.trim().parse().map_err(|_| bad())
}

/// The readings in the output of one or more `nvidia-smi` calls: header lines
/// and empty lines are skipped, every other line is a reading.
pub fn parse_readings(text: &str) -> Result<Vec<u64>, VramError> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with(HEADER))
        .map(parse_reading)
        .collect()
}

/// The largest reading while the run was going, when no reading reached
/// past `budget_mib`. At least one reading before the start is required.
pub fn check_vram(before: &[u64], during: &[u64], budget_mib: u64) -> Result<u64, VramError> {
    if before.is_empty() {
        return Err(VramError::NoReadingBefore);
    }
    let Some(peak_mib) = during.iter().copied().max() else {
        return Err(VramError::NoReadingDuring);
    };
    if peak_mib > budget_mib {
        return Err(VramError::OverBudget {
            peak_mib,
            budget_mib,
        });
    }
    Ok(peak_mib)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_figure_with_its_unit_is_a_reading() {
        assert_eq!(parse_reading("1161 MiB"), Ok(1161));
        assert_eq!(parse_reading(" 2008 MiB\n"), Ok(2008));
    }

    #[test]
    fn a_figure_without_the_unit_or_with_a_wrong_one_is_refused() {
        assert_eq!(
            parse_reading("1161"),
            Err(VramError::BadReading(String::from("1161")))
        );
        assert!(parse_reading("1161 GiB").is_err());
        assert!(parse_reading("MiB").is_err());
        assert!(parse_reading("").is_err());
    }

    #[test]
    fn the_header_and_blank_lines_are_not_readings() {
        let text = "memory.used [MiB]\n1198 MiB\n\nmemory.used [MiB]\n1957 MiB\n";
        assert_eq!(parse_readings(text), Ok(vec![1198, 1957]));
    }

    #[test]
    fn a_bad_line_among_readings_is_named() {
        let said = parse_readings("1198 MiB\n[N/A]\n");
        assert_eq!(said, Err(VramError::BadReading(String::from("[N/A]"))));
    }

    #[test]
    fn a_peak_equal_to_the_budget_passes_and_one_over_does_not() {
        assert_eq!(check_vram(&[1198], &[900, 14_000], BUDGET_MIB), Ok(14_000));
        assert_eq!(
            check_vram(&[1198], &[14_001], BUDGET_MIB),
            Err(VramError::OverBudget {
                peak_mib: 14_001,
                budget_mib: 14_000
            })
        );
    }

    #[test]
    fn no_reading_during_the_run_is_not_a_pass() {
        assert_eq!(
            check_vram(&[1198], &[], BUDGET_MIB),
            Err(VramError::NoReadingDuring)
        );
    }

    #[test]
    fn no_reading_before_the_start_is_refused() {
        assert_eq!(
            check_vram(&[], &[900], BUDGET_MIB),
            Err(VramError::NoReadingBefore)
        );
    }
}

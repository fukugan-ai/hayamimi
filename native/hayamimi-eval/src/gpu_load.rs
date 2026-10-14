//! The GPU's load read before and after a run, and whether two runs were
//! measured under the same load.
//!
//! One reading is the first line of
//! `nvidia-smi --query-gpu=utilization.gpu,memory.used --format=csv,noheader,nounits`:
//! `<percent>, <MiB>`. A Python run and a Rust run are compared only when the
//! reading taken just before each shows an idle GPU ([`MAX_IDLE_UTILIZATION_PCT`])
//! and the two memory figures are within [`MAX_MEMORY_GAP_MIB`]. Both widths are
//! provisional until the first alternated runs have shown how wide the readings
//! really are.

use std::path::Path;
use std::process::Command;

use serde::Serialize;

use crate::error::EvalError;
use crate::log;
use crate::manifest::write_text;

/// The most `utilization.gpu` a reading may show for a run to count as started on an idle GPU.
pub const MAX_IDLE_UTILIZATION_PCT: u32 = 5;
/// The most the `memory.used` of two runs may differ by and still be the same load.
pub const MAX_MEMORY_GAP_MIB: u64 = 256;

/// One `nvidia-smi` reading.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct GpuReading {
    pub utilization_pct: u32,
    pub memory_mib: u64,
}

/// What a run read before it started and after it ended; a reading that
/// could not be taken is `null`, and the run is not valid evidence of the load.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct GpuRecord {
    pub before: Option<GpuReading>,
    pub after: Option<GpuReading>,
}

/// Reads the first line of `nvidia-smi`'s CSV output: `<percent>, <MiB>`.
pub fn parse_reading(output: &str) -> Result<GpuReading, EvalError> {
    let bad = || EvalError::Invalid(format!("not a GPU reading: `{}`", output.trim()));
    let Some(line) = output.lines().next() else {
        return Err(bad());
    };
    let Some((percent, mib)) = line.split_once(',') else {
        return Err(bad());
    };
    let (Ok(utilization_pct), Ok(memory_mib)) =
        (percent.trim().parse::<u32>(), mib.trim().parse::<u64>())
    else {
        return Err(bad());
    };
    Ok(GpuReading {
        utilization_pct,
        memory_mib,
    })
}

/// Asks `nvidia-smi` now.
pub fn read_now() -> Result<GpuReading, EvalError> {
    let output = Command::new("nvidia-smi")
        .args([
            "--query-gpu=utilization.gpu,memory.used",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .map_err(|e| EvalError::Invalid(format!("nvidia-smi: {e}")))?;
    if !output.status.success() {
        return Err(EvalError::Invalid(format!(
            "nvidia-smi exited with {}",
            output.status
        )));
    }
    parse_reading(&String::from_utf8_lossy(&output.stdout))
}

/// A reading for the record: a failure is logged and recorded as `null`, and
/// the run goes on (a machine with no GPU still runs, with no load record).
pub fn read_for_record(moment: &str) -> Option<GpuReading> {
    match read_now() {
        Ok(reading) => Some(reading),
        Err(error) => {
            log::warn(
                "eval.stream.gpu_unreadable",
                "worker",
                &error.to_string(),
                &[("moment", moment.into())],
            );
            None
        }
    }
}

/// Writes the record to `path`.
pub fn write_record(path: &Path, record: &GpuRecord) -> Result<(), EvalError> {
    let text = serde_json::to_string_pretty(record).map_err(|e| EvalError::json(path, e))?;
    write_text(path, &format!("{text}\n"))
}

/// Whether a Python run and a Rust run started under the same GPU load:
/// neither reading above [`MAX_IDLE_UTILIZATION_PCT`] and the memory figures
/// no further apart than [`MAX_MEMORY_GAP_MIB`].
pub fn pair_is_valid(python: &GpuReading, rust: &GpuReading) -> bool {
    python.utilization_pct <= MAX_IDLE_UTILIZATION_PCT
        && rust.utilization_pct <= MAX_IDLE_UTILIZATION_PCT
        && python.memory_mib.abs_diff(rust.memory_mib) <= MAX_MEMORY_GAP_MIB
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(utilization_pct: u32, memory_mib: u64) -> GpuReading {
        GpuReading {
            utilization_pct,
            memory_mib,
        }
    }

    #[test]
    fn a_reading_is_the_first_line_percent_then_mib() -> Result<(), EvalError> {
        assert_eq!(parse_reading("3, 9803\n")?, reading(3, 9803));
        assert_eq!(parse_reading("0, 0\n12, 99999\n")?, reading(0, 0));
        Ok(())
    }

    #[test]
    fn text_that_is_not_a_reading_is_refused() {
        for text in [
            "", "\n", "7", "x, 5", "5, y", "5, 6, 7", "-1, 5", "N/A, N/A",
        ] {
            assert!(
                matches!(parse_reading(text), Err(EvalError::Invalid(_))),
                "{text:?}"
            );
        }
    }

    #[test]
    fn utilization_at_the_width_is_valid_and_one_over_is_not() {
        let idle = reading(0, 9803);
        assert!(pair_is_valid(&reading(5, 9803), &idle));
        assert!(!pair_is_valid(&reading(6, 9803), &idle));
        assert!(pair_is_valid(&idle, &reading(5, 9803)));
        assert!(!pair_is_valid(&idle, &reading(6, 9803)));
    }

    #[test]
    fn a_memory_gap_at_the_width_is_valid_and_one_over_is_not() {
        let base = reading(0, 9803);
        assert!(pair_is_valid(&base, &reading(0, 9803 + 256)));
        assert!(!pair_is_valid(&base, &reading(0, 9803 + 257)));
        assert!(pair_is_valid(&reading(0, 9803 + 256), &base));
        assert!(!pair_is_valid(&reading(0, 9803 + 257), &base));
    }

    #[test]
    fn identical_readings_are_a_valid_pair() {
        assert!(pair_is_valid(&reading(0, 9803), &reading(0, 9803)));
    }

    #[test]
    fn a_record_with_a_missing_reading_writes_null() -> Result<(), serde_json::Error> {
        let record = GpuRecord {
            before: Some(reading(1, 2)),
            after: None,
        };
        let value = serde_json::to_value(record)?;
        assert_eq!(value["before"]["memory_mib"], 2);
        assert!(value["after"].is_null());
        Ok(())
    }
}

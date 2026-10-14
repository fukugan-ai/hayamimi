//! RP-6: whether a run used the GPU, read from the memory of the whole GPU.
//!
//! `nvidia-smi --query-gpu=memory.used --format=csv,noheader,nounits` prints one
//! integer in MiB per call. Its per-process table is not used: it names no
//! process under WSL and gives no figure under WDDM. The baseline is the median
//! of three reads before the start. A run that used the GPU raises the figure
//! by at least [`RISE_MIB`] while it runs, and the figure is back within
//! [`RETURN_MIB`] of the baseline in some read after it ends. A run with the GPU
//! hidden raises it by no more than [`RETURN_MIB`].

use std::fmt;

/// Reads taken before the start; their median is the baseline.
pub const BEFORE_READS: usize = 3;
/// The least rise of the maximum over the baseline that counts as GPU use.
pub const RISE_MIB: u64 = 256;
/// How far from the baseline a read after the run may be and still count as returned.
pub const RETURN_MIB: u64 = 64;

#[derive(Debug, PartialEq, Eq)]
pub enum MemoryError {
    /// A read is not one integer.
    BadRead(String),
    /// The baseline needs [`BEFORE_READS`] reads.
    WrongBeforeCount(usize),
    /// No read was taken while the run was going.
    NoReadsDuring,
    /// No read was taken after the run ended.
    NoReadsAfter,
    /// The maximum rose by less than the least rise.
    RiseTooSmall { rise: u64, need: u64 },
    /// No read after the run was within the return band; the reads are listed.
    NotReturned { base: u64, after: Vec<u64> },
    /// A run with the GPU hidden still raised the figure.
    RoseWhileHidden { rise: u64, allowed: u64 },
}

impl fmt::Display for MemoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MemoryError::BadRead(text) => write!(f, "`{text}` is not a memory figure in MiB"),
            MemoryError::WrongBeforeCount(count) => {
                write!(f, "{count} reads before the start, want {BEFORE_READS}")
            }
            MemoryError::NoReadsDuring => write!(f, "no memory read while the run was going"),
            MemoryError::NoReadsAfter => write!(f, "no memory read after the run ended"),
            MemoryError::RiseTooSmall { rise, need } => {
                write!(f, "memory rose {rise} MiB, want at least {need} MiB")
            }
            MemoryError::NotReturned { base, after } => write!(
                f,
                "memory did not return to {base} MiB (+/- {RETURN_MIB}) after the run: reads {after:?}"
            ),
            MemoryError::RoseWhileHidden { rise, allowed } => write!(
                f,
                "memory rose {rise} MiB with the GPU hidden, at most {allowed} MiB allowed"
            ),
        }
    }
}

impl std::error::Error for MemoryError {}

/// One read: the output of a single `nvidia-smi` call.
pub fn parse_read(text: &str) -> Result<u64, MemoryError> {
    text.trim()
        .parse::<u64>()
        .map_err(|_| MemoryError::BadRead(text.trim().to_owned()))
}

fn parse_all(texts: &[String]) -> Result<Vec<u64>, MemoryError> {
    texts.iter().map(|text| parse_read(text)).collect()
}

/// The median of the reads before the start.
pub fn baseline(before: &[String]) -> Result<u64, MemoryError> {
    let mut reads = parse_all(before)?;
    if reads.len() != BEFORE_READS {
        return Err(MemoryError::WrongBeforeCount(reads.len()));
    }
    reads.sort_unstable();
    Ok(reads[BEFORE_READS / 2])
}

/// The largest read while the run was going.
fn peak(during: &[String]) -> Result<u64, MemoryError> {
    let reads = parse_all(during)?;
    reads.into_iter().max().ok_or(MemoryError::NoReadsDuring)
}

/// A run that used the GPU: the maximum over the baseline is at least
/// [`RISE_MIB`], and a read after the end is within [`RETURN_MIB`] of the baseline.
/// Returns the rise.
pub fn check_memory_rise(
    before: &[String],
    during: &[String],
    after: &[String],
) -> Result<u64, MemoryError> {
    let base = baseline(before)?;
    let rise = peak(during)?.saturating_sub(base);
    if rise < RISE_MIB {
        return Err(MemoryError::RiseTooSmall {
            rise,
            need: RISE_MIB,
        });
    }
    let after = parse_all(after)?;
    if after.is_empty() {
        return Err(MemoryError::NoReadsAfter);
    }
    if !after.iter().any(|read| read.abs_diff(base) <= RETURN_MIB) {
        return Err(MemoryError::NotReturned { base, after });
    }
    Ok(rise)
}

/// A run with the GPU hidden: the maximum is no more than [`RETURN_MIB`] over the baseline.
pub fn check_memory_flat(before: &[String], during: &[String]) -> Result<(), MemoryError> {
    let base = baseline(before)?;
    let rise = peak(during)?.saturating_sub(base);
    if rise > RETURN_MIB {
        return Err(MemoryError::RoseWhileHidden {
            rise,
            allowed: RETURN_MIB,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reads(figures: &[&str]) -> Vec<String> {
        figures.iter().map(|figure| (*figure).to_owned()).collect()
    }

    #[test]
    fn a_read_is_one_integer_with_surrounding_space_ignored() {
        assert_eq!(parse_read(" 1198\n"), Ok(1198));
    }

    #[test]
    fn a_read_that_is_not_an_integer_is_named() {
        assert_eq!(
            parse_read("[N/A]"),
            Err(MemoryError::BadRead(String::from("[N/A]")))
        );
        assert_eq!(parse_read(""), Err(MemoryError::BadRead(String::new())));
    }

    #[test]
    fn the_baseline_is_the_median_of_three() {
        assert_eq!(baseline(&reads(&["1199", "1198", "1500"])), Ok(1199));
    }

    #[test]
    fn a_baseline_of_two_reads_is_refused() {
        assert_eq!(
            baseline(&reads(&["1198", "1198"])),
            Err(MemoryError::WrongBeforeCount(2))
        );
    }

    #[test]
    fn no_reads_during_the_run_is_refused() {
        let before = reads(&["1198", "1198", "1198"]);
        assert_eq!(
            check_memory_rise(&before, &[], &reads(&["1198"])),
            Err(MemoryError::NoReadsDuring)
        );
    }

    #[test]
    fn no_reads_after_the_run_is_refused() {
        let before = reads(&["1198", "1198", "1198"]);
        assert_eq!(
            check_memory_rise(&before, &reads(&["1957"]), &[]),
            Err(MemoryError::NoReadsAfter)
        );
    }

    #[test]
    fn a_return_of_exactly_64_mib_counts() {
        let before = reads(&["1198", "1198", "1198"]);
        assert_eq!(
            check_memory_rise(&before, &reads(&["1957"]), &reads(&["1262"])),
            Ok(759)
        );
    }

    #[test]
    fn a_hidden_run_may_rise_by_64_mib_and_not_65() {
        let before = reads(&["1198", "1198", "1198"]);
        assert_eq!(check_memory_flat(&before, &reads(&["1262"])), Ok(()));
        assert_eq!(
            check_memory_flat(&before, &reads(&["1263"])),
            Err(MemoryError::RoseWhileHidden {
                rise: 65,
                allowed: 64
            })
        );
    }
}

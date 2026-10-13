//! Nearest-rank percentiles over integer samples.
//!
//! The p-th percentile of n samples is the `(p * n + 99) / 100`-th smallest
//! (1-based, integer division). For n = 115 the 95th is the 110th. A median
//! is the 50th: the middle one of an odd count.

use crate::error::EvalError;

/// The nearest-rank `p`-th percentile (`1..=100`) of `values`.
pub fn nearest_rank(values: &[i64], p: usize) -> Result<i64, EvalError> {
    if !(1..=100).contains(&p) {
        return Err(EvalError::Invalid(format!(
            "percentile {p} is outside 1..=100"
        )));
    }
    if values.is_empty() {
        return Err(EvalError::Invalid(String::from("percentile of no samples")));
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let rank = (p * sorted.len()).div_ceil(100);
    // rank is in 1..=len for p in 1..=100 and a non-empty slice.
    Ok(sorted[rank - 1])
}

/// Microseconds as whole milliseconds, halves rounded up.
pub fn micros_to_ms(micros: i64) -> i64 {
    (micros + 500).div_euclid(1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one_to(n: i64) -> Vec<i64> {
        (1..=n).collect()
    }

    #[test]
    fn p50_and_p95_of_115_samples_are_the_58th_and_110th() -> Result<(), EvalError> {
        let values = one_to(115);
        assert_eq!(nearest_rank(&values, 50)?, 58);
        assert_eq!(nearest_rank(&values, 95)?, 110);
        Ok(())
    }

    #[test]
    fn p95_of_20_samples_is_the_19th() -> Result<(), EvalError> {
        assert_eq!(nearest_rank(&one_to(20), 95)?, 19);
        Ok(())
    }

    #[test]
    fn p95_of_21_samples_is_the_20th() -> Result<(), EvalError> {
        assert_eq!(nearest_rank(&one_to(21), 95)?, 20);
        Ok(())
    }

    #[test]
    fn one_sample_is_every_percentile() -> Result<(), EvalError> {
        assert_eq!(nearest_rank(&[7], 50)?, 7);
        assert_eq!(nearest_rank(&[7], 95)?, 7);
        Ok(())
    }

    #[test]
    fn the_median_of_five_is_the_third_smallest() -> Result<(), EvalError> {
        assert_eq!(nearest_rank(&[10, 30, 20, 50, 40], 50)?, 30);
        Ok(())
    }

    #[test]
    fn no_samples_is_an_error() {
        assert!(matches!(nearest_rank(&[], 50), Err(EvalError::Invalid(_))));
    }

    #[test]
    fn a_percentile_outside_the_range_is_an_error() {
        assert!(matches!(nearest_rank(&[1], 0), Err(EvalError::Invalid(_))));
        assert!(matches!(
            nearest_rank(&[1], 101),
            Err(EvalError::Invalid(_))
        ));
    }

    #[test]
    fn microseconds_round_half_up_to_milliseconds() {
        assert_eq!(micros_to_ms(499), 0);
        assert_eq!(micros_to_ms(500), 1);
        assert_eq!(micros_to_ms(1_499), 1);
        assert_eq!(micros_to_ms(-500), 0);
        assert_eq!(micros_to_ms(-501), -1);
    }
}

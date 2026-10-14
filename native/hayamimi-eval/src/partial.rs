//! RP-5: how early the text in progress, and its translation, reach the reader.
//!
//! The first `partial` of an utterance must not come later than Python's: the
//! 95th percentile of `t_partial - t_start` (the time from the first sample
//! of the utterance being sent to the first `partial`) of the Rust run is at
//! most the Python run's. An utterance whose text in progress was translated
//! must show that translation before the translation of its `final`.

use crate::latency::{Sample, Stat, stat_of};

/// What a check refuses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fault {
    /// The run has no utterance with a first `partial`, so nothing is measured.
    NoFirstPartial,
    /// The 95th percentile of the Rust run is later than the Python run's, in
    /// whole milliseconds.
    FirstPartialLater { rust_ms: i64, python_ms: i64 },
    /// An utterance's first translation of the text in progress came at or
    /// after the translation of its `final`.
    NotBeforeTranslation { repeat: u32, utt: u32 },
    /// The counts of the kinds of utterance do not fit the run's length.
    MiscountedUtterances,
}

/// `t_partial - t_start` of every utterance that had a `final` and a
/// `partial`, in microseconds. An utterance with no `final` is left out
/// whether or not it had a `partial`.
pub fn first_partial_delays_us(samples: &[Sample]) -> Vec<i64> {
    samples
        .iter()
        .filter(|sample| sample.t_final_us.is_some())
        .filter_map(|sample| sample.t_partial_us.map(|t| t - sample.t_start_us))
        .collect()
}

/// How the utterances of a run split by what the recognizer sent for them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tally {
    pub with_final: usize,
    /// A `partial` and no `final`.
    pub partial_only: usize,
    /// Neither a `partial` nor a `final`.
    pub neither: usize,
}

/// The utterances that are in neither of the two counted kinds, or `None`
/// when the two counts exceed the run's length.
fn neither_of(total: usize, with_final: usize, partial_only: usize) -> Option<usize> {
    total.checked_sub(with_final)?.checked_sub(partial_only)
}

/// Counts the utterances of each kind; the three add up to the run's length.
/// Counts that do not fit the length are `Fault::MiscountedUtterances`.
pub fn tally(samples: &[Sample]) -> Result<Tally, Fault> {
    let with_final = samples.iter().filter(|s| s.t_final_us.is_some()).count();
    let partial_only = samples
        .iter()
        .filter(|s| s.t_final_us.is_none() && s.t_partial_us.is_some())
        .count();
    let neither =
        neither_of(samples.len(), with_final, partial_only).ok_or(Fault::MiscountedUtterances)?;
    Ok(Tally {
        with_final,
        partial_only,
        neither,
    })
}

fn stat_of_first_partials(samples: &[Sample]) -> Result<Stat, Fault> {
    stat_of(&first_partial_delays_us(samples))
        .ok()
        .flatten()
        .ok_or(Fault::NoFirstPartial)
}

/// The first `partial` of the Rust run comes no later than Python's, at the
/// 95th percentile.
pub fn check_first_partial(python: &[Sample], rust: &[Sample]) -> Result<(), Fault> {
    let python_ms = stat_of_first_partials(python)?.p95_ms;
    let rust_ms = stat_of_first_partials(rust)?.p95_ms;
    if rust_ms > python_ms {
        return Err(Fault::FirstPartialLater { rust_ms, python_ms });
    }
    Ok(())
}

/// Every utterance whose text in progress was translated shows that
/// translation strictly before the translation of its `final`. An utterance
/// with no such translation, or with no translation of its `final`, is not
/// judged.
pub fn check_partial_translation_first(samples: &[Sample]) -> Result<(), Fault> {
    for sample in samples {
        if let (Some(early), Some(settled)) = (sample.t_partial_tr_us, sample.t_tr_us)
            && early >= settled
        {
            return Err(Fault::NotBeforeTranslation {
                repeat: sample.repeat,
                utt: sample.utt,
            });
        }
    }
    Ok(())
}

/// How many utterances had their text in progress translated.
pub fn count_partial_translations(samples: &[Sample]) -> usize {
    samples
        .iter()
        .filter(|sample| sample.t_partial_tr_us.is_some())
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(utt: u32, start: i64, partial: Option<i64>) -> Sample {
        Sample {
            repeat: 1,
            utt,
            t_start_us: start,
            t_end_us: start + 1_000_000,
            t_partial_us: partial,
            t_partial_tr_us: None,
            t_final_us: Some(start + 2_000_000),
            t_tr_us: None,
            finals: 1,
            text: String::new(),
            translation: None,
            final_latency_ms: None,
            cer_dist: 0,
            cer_denom: 0,
        }
    }

    #[test]
    fn counts_that_exceed_the_length_are_not_wrapped() {
        assert_eq!(neither_of(5, 3, 2), Some(0));
        assert_eq!(neither_of(5, 3, 1), Some(1));
        assert_eq!(neither_of(5, 4, 2), None);
        assert_eq!(neither_of(2, 3, 0), None);
        assert_eq!(
            tally(&[]),
            Ok(Tally {
                with_final: 0,
                partial_only: 0,
                neither: 0
            })
        );
    }

    #[test]
    fn the_delay_is_counted_from_the_first_sample_sent() {
        let samples = [sample(1, 100, Some(600)), sample(2, 0, None)];
        assert_eq!(first_partial_delays_us(&samples), [500]);
    }

    #[test]
    fn a_run_with_no_partial_measures_nothing() {
        let none = [sample(1, 0, None)];
        assert_eq!(
            check_first_partial(&none, &none),
            Err(Fault::NoFirstPartial)
        );
        assert_eq!(check_first_partial(&[], &none), Err(Fault::NoFirstPartial));
    }

    #[test]
    fn a_later_first_partial_at_the_95th_percentile_is_named() {
        let python = [sample(1, 0, Some(400_000))];
        let rust = [sample(1, 0, Some(401_000))];
        assert_eq!(
            check_first_partial(&python, &rust),
            Err(Fault::FirstPartialLater {
                rust_ms: 401,
                python_ms: 400
            })
        );
    }

    #[test]
    fn an_earlier_or_equal_first_partial_passes() {
        let python = [sample(1, 0, Some(400_000))];
        assert_eq!(check_first_partial(&python, &python), Ok(()));
        assert_eq!(
            check_first_partial(&python, &[sample(1, 0, Some(399_000))]),
            Ok(())
        );
    }

    fn with_translations(early: Option<i64>, settled: Option<i64>) -> Sample {
        let mut found = sample(1, 0, Some(1));
        found.t_partial_tr_us = early;
        found.t_tr_us = settled;
        found
    }

    #[test]
    fn a_translation_in_progress_must_come_strictly_before_the_settled_one() {
        let judge = |early, settled| {
            check_partial_translation_first(&[with_translations(Some(early), Some(settled))])
        };
        assert_eq!(judge(400, 500), Ok(()));
        let late = Err(Fault::NotBeforeTranslation { repeat: 1, utt: 1 });
        assert_eq!(judge(500, 500), late);
        assert_eq!(judge(501, 500), late);
    }

    #[test]
    fn an_utterance_with_nothing_to_compare_is_not_judged() {
        let samples = [
            with_translations(None, Some(500)),
            with_translations(Some(900), None),
        ];
        assert_eq!(check_partial_translation_first(&samples), Ok(()));
    }

    #[test]
    fn the_translations_in_progress_are_counted() {
        let samples = [
            with_translations(Some(1), None),
            with_translations(None, None),
            with_translations(Some(2), Some(3)),
        ];
        assert_eq!(count_partial_translations(&samples), 2);
        assert_eq!(count_partial_translations(&[]), 0);
    }
}

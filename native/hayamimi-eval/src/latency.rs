//! Per-utterance delays from the events a run received.
//!
//! Four moments are read off the tool's own clock for each utterance: the
//! last audio sample sent (`t_end`), the `final` event that closes the
//! utterance (`t_final`), the translation of that final (`t_tr`), and the
//! first `partial` (`t_partial`). `L = t_tr - t_end`, `L_asr = t_final -
//! t_end`, `L_tr = t_tr - t_final`. All times are microseconds since the run
//! started; a percentile is reported in whole milliseconds, halves up.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cer::{Cer, cer};
use crate::error::EvalError;
use crate::percentile::{micros_to_ms, nearest_rank};

/// What an event was, as far as the delay measurement cares.
#[derive(Clone, Debug, PartialEq)]
pub enum EventKind {
    Partial,
    Final {
        text: String,
        latency_ms: Option<f64>,
    },
    Translation {
        text: String,
    },
    /// A translation of the text of an utterance in progress (`of` is
    /// `partial`). It never closes an utterance, so it is not paired with a
    /// `final`.
    PartialTranslation,
    Other,
}

/// An event received from the server, stamped by the tool's clock.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub t_us: i64,
    pub kind: EventKind,
}

/// The text field of an event, which must be a string.
fn text_of(value: &Value) -> Result<String, EvalError> {
    match value.get("text") {
        Some(Value::String(text)) => Ok(text.clone()),
        _ => Err(EvalError::Protocol(String::from(
            "an event of this type has no string `text`",
        ))),
    }
}

/// Reads one event from the JSON text the server sent.
pub fn parse_event(t_us: i64, json_text: &str) -> Result<Event, EvalError> {
    let value: Value = serde_json::from_str(json_text)
        .map_err(|e| EvalError::Protocol(format!("an event is not JSON: {e}")))?;
    let kind = match value.get("type").and_then(Value::as_str) {
        Some("partial") => EventKind::Partial,
        Some("final") => EventKind::Final {
            text: text_of(&value)?,
            latency_ms: value.get("latency_ms").and_then(Value::as_f64),
        },
        Some("translation") if value.get("of").and_then(Value::as_str) == Some("partial") => {
            EventKind::PartialTranslation
        }
        Some("translation") => EventKind::Translation {
            text: text_of(&value)?,
        },
        _ => EventKind::Other,
    };
    Ok(Event { t_us, kind })
}

/// The span of one utterance in one repetition, on the tool's clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    pub repeat: u32,
    /// 1-based position in the evaluation set.
    pub utt: u32,
    pub t_start_us: i64,
    pub t_end_us: i64,
}

/// One line of a run's result file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    pub repeat: u32,
    pub utt: u32,
    pub t_start_us: i64,
    pub t_end_us: i64,
    pub t_partial_us: Option<i64>,
    /// The first translation of the text in progress in this utterance. Files
    /// written before the relay made such translations do not have it.
    pub t_partial_tr_us: Option<i64>,
    pub t_final_us: Option<i64>,
    pub t_tr_us: Option<i64>,
    /// How many `final` events fell in this utterance.
    pub finals: u32,
    /// Those finals joined in order.
    pub text: String,
    pub translation: Option<String>,
    pub final_latency_ms: Option<f64>,
    pub cer_dist: usize,
    pub cer_denom: usize,
}

impl Sample {
    /// Speech end to translation.
    pub fn l_us(&self) -> Option<i64> {
        self.t_tr_us.map(|t| t - self.t_end_us)
    }

    /// Speech end to the closing final.
    pub fn l_asr_us(&self) -> Option<i64> {
        self.t_final_us.map(|t| t - self.t_end_us)
    }

    /// Final to translation. Negative when a partial's translation matched.
    pub fn l_tr_us(&self) -> Option<i64> {
        match (self.t_final_us, self.t_tr_us) {
            (Some(f), Some(t)) => Some(t - f),
            _ => None,
        }
    }
}

/// For each `final` in `events` (by index), the translation it is paired
/// with: the first `translation` after it and before the next `final`.
///
/// The server's translation event carries no id, so position is the only
/// link. A translation that arrives before any final, or after another
/// final has already been received, belongs to no one.
pub fn positional_translations(events: &[Event]) -> Vec<(usize, Option<usize>)> {
    let mut pairs: Vec<(usize, Option<usize>)> = Vec::new();
    for (index, event) in events.iter().enumerate() {
        match event.kind {
            EventKind::Final { .. } => pairs.push((index, None)),
            EventKind::Translation { .. } => {
                if let Some(last) = pairs.last_mut()
                    && last.1.is_none()
                {
                    last.1 = Some(index);
                }
            }
            _ => {}
        }
    }
    pairs
}

/// The index of the last window that started at or before `t_us`.
fn window_at(windows: &[Window], t_us: i64) -> Option<usize> {
    windows
        .partition_point(|w| w.t_start_us <= t_us)
        .checked_sub(1)
}

/// Builds one sample per window from the run's events.
///
/// A `final` belongs to the latest utterance that had started when it
/// arrived, so the early finals of an utterance the server cut at its
/// maximum length still count toward that utterance's text. The last of an
/// utterance's finals closes it; if that one arrived before the utterance's
/// last sample was sent, the utterance has no closing final.
pub fn build_samples(
    windows: &[Window],
    references: &[String],
    events: &[Event],
) -> Result<Vec<Sample>, EvalError> {
    if windows
        .windows(2)
        .any(|w| w[0].t_start_us > w[1].t_start_us)
    {
        return Err(EvalError::Invalid(String::from(
            "utterance windows are not in time order",
        )));
    }
    let pairs = positional_translations(events);
    let mut finals_by_window: Vec<Vec<usize>> = vec![Vec::new(); windows.len()];
    for (index, event) in events.iter().enumerate() {
        if matches!(event.kind, EventKind::Final { .. })
            && let Some(k) = window_at(windows, event.t_us)
        {
            finals_by_window[k].push(index);
        }
    }
    let mut samples = Vec::with_capacity(windows.len());
    for (k, window) in windows.iter().enumerate() {
        let Some(reference) = usize::try_from(window.utt)
            .ok()
            .and_then(|u| u.checked_sub(1))
            .and_then(|u| references.get(u))
        else {
            return Err(EvalError::Invalid(format!(
                "utterance {} has no reference text",
                window.utt
            )));
        };
        let finals = &finals_by_window[k];
        let text: String = finals
            .iter()
            .filter_map(|&i| match &events[i].kind {
                EventKind::Final { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        let closing = finals
            .last()
            .copied()
            .filter(|&i| events[i].t_us >= window.t_end_us);
        let t_final_us = closing.map(|i| events[i].t_us);
        let final_latency_ms = closing.and_then(|i| match &events[i].kind {
            EventKind::Final { latency_ms, .. } => *latency_ms,
            _ => None,
        });
        let paired = closing.and_then(|i| {
            pairs
                .iter()
                .find(|(final_index, _)| *final_index == i)
                .and_then(|(_, translation)| *translation)
        });
        let (t_tr_us, translation) = match paired.map(|i| &events[i]) {
            Some(Event {
                t_us,
                kind: EventKind::Translation { text },
            }) => (Some(*t_us), Some(text.clone())),
            _ => (None, None),
        };
        let upper = t_final_us.or_else(|| windows.get(k + 1).map(|next| next.t_start_us));
        let first_in_window = |wanted: fn(&EventKind) -> bool| {
            events
                .iter()
                .filter(|e| wanted(&e.kind) && e.t_us >= window.t_start_us)
                .map(|e| e.t_us)
                .next()
                .filter(|t| upper.is_none_or(|u| *t < u))
        };
        let t_partial_us = first_in_window(|kind| matches!(kind, EventKind::Partial));
        let t_partial_tr_us = first_in_window(|kind| matches!(kind, EventKind::PartialTranslation));
        let Cer { dist, denom } = cer(reference, &text);
        samples.push(Sample {
            repeat: window.repeat,
            utt: window.utt,
            t_start_us: window.t_start_us,
            t_end_us: window.t_end_us,
            t_partial_us,
            t_partial_tr_us,
            t_final_us,
            t_tr_us,
            finals: u32::try_from(finals.len())
                .map_err(|_| EvalError::Invalid(String::from("too many finals")))?,
            text,
            translation,
            final_latency_ms,
            cer_dist: dist,
            cer_denom: denom,
        });
    }
    Ok(samples)
}

/// A percentile pair over some samples, in whole milliseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Stat {
    pub n: usize,
    pub p50_ms: i64,
    pub p95_ms: i64,
}

/// The p50 and p95 of microsecond values; `None` for no values.
pub fn stat_of(values_us: &[i64]) -> Result<Option<Stat>, EvalError> {
    if values_us.is_empty() {
        return Ok(None);
    }
    Ok(Some(Stat {
        n: values_us.len(),
        p50_ms: micros_to_ms(nearest_rank(values_us, 50)?),
        p95_ms: micros_to_ms(nearest_rank(values_us, 95)?),
    }))
}

/// The numbers a run is reported by.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Summary {
    pub samples: usize,
    pub with_final: usize,
    pub with_translation: usize,
    /// Samples with no translation: those that got none and those with no
    /// closing final. `samples = with_translation + untranslated`.
    pub untranslated: usize,
    pub finals_total: usize,
    pub l: Option<Stat>,
    pub l_asr: Option<Stat>,
    pub l_tr: Option<Stat>,
    pub l_tr_negative: usize,
    pub final_latency_mean_ms: Option<f64>,
    /// Median over the repetitions of each utterance, by utterance number.
    pub per_utt_median_l_ms: Vec<Option<i64>>,
    pub per_utt_median_l_asr_ms: Vec<Option<i64>>,
    pub cer_dist: usize,
    pub cer_denom: usize,
    pub cer_micros: u64,
}

/// The median of each utterance's values, utterance numbers 1..=`count`.
fn per_utterance_medians(
    samples: &[Sample],
    count: u32,
    pick: fn(&Sample) -> Option<i64>,
) -> Result<Vec<Option<i64>>, EvalError> {
    (1..=count)
        .map(|utt| {
            let values: Vec<i64> = samples
                .iter()
                .filter(|s| s.utt == utt)
                .filter_map(pick)
                .collect();
            if values.is_empty() {
                Ok(None)
            } else {
                Ok(Some(micros_to_ms(nearest_rank(&values, 50)?)))
            }
        })
        .collect()
}

/// Summarises a run's samples.
pub fn summarize(samples: &[Sample]) -> Result<Summary, EvalError> {
    let l: Vec<i64> = samples.iter().filter_map(Sample::l_us).collect();
    let l_asr: Vec<i64> = samples.iter().filter_map(Sample::l_asr_us).collect();
    let l_tr: Vec<i64> = samples.iter().filter_map(Sample::l_tr_us).collect();
    let latencies: Vec<f64> = samples.iter().filter_map(|s| s.final_latency_ms).collect();
    let final_latency_mean_ms = if latencies.is_empty() {
        None
    } else {
        Some(latencies.iter().sum::<f64>() / latencies.len() as f64)
    };
    let total = samples
        .iter()
        .map(|s| Cer {
            dist: s.cer_dist,
            denom: s.cer_denom,
        })
        .fold(Cer::default(), Cer::plus);
    // The highest utterance number; no samples means no utterances.
    let count = samples.iter().map(|s| s.utt).fold(0, u32::max);
    Ok(Summary {
        samples: samples.len(),
        with_final: l_asr.len(),
        with_translation: l.len(),
        untranslated: samples.len() - l.len(),
        finals_total: samples.iter().map(|s| s.finals as usize).sum(),
        l: stat_of(&l)?,
        l_asr: stat_of(&l_asr)?,
        l_tr: stat_of(&l_tr)?,
        l_tr_negative: l_tr.iter().filter(|v| **v < 0).count(),
        final_latency_mean_ms,
        per_utt_median_l_ms: per_utterance_medians(samples, count, Sample::l_us)?,
        per_utt_median_l_asr_ms: per_utterance_medians(samples, count, Sample::l_asr_us)?,
        cer_dist: total.dist,
        cer_denom: total.denom,
        cer_micros: total.micros(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(t_us: i64, kind: EventKind) -> Event {
        Event { t_us, kind }
    }

    fn final_event(t_us: i64, text: &str) -> Event {
        event(
            t_us,
            EventKind::Final {
                text: text.to_owned(),
                latency_ms: Some(100.0),
            },
        )
    }

    fn translation_event(t_us: i64, text: &str) -> Event {
        event(
            t_us,
            EventKind::Translation {
                text: text.to_owned(),
            },
        )
    }

    fn window(utt: u32, t_start_us: i64, t_end_us: i64) -> Window {
        Window {
            repeat: 1,
            utt,
            t_start_us,
            t_end_us,
        }
    }

    #[test]
    fn a_final_event_is_parsed_with_its_text_and_latency() -> Result<(), EvalError> {
        let parsed = parse_event(
            5,
            r#"{"type":"final","text":"こんにちは","latency_ms":99.5}"#,
        )?;
        assert_eq!(
            parsed,
            event(
                5,
                EventKind::Final {
                    text: String::from("こんにちは"),
                    latency_ms: Some(99.5)
                }
            )
        );
        Ok(())
    }

    #[test]
    fn a_final_without_a_latency_has_none() -> Result<(), EvalError> {
        let parsed = parse_event(5, r#"{"type":"final","text":"a","latency_ms":null}"#)?;
        assert!(matches!(
            parsed.kind,
            EventKind::Final {
                latency_ms: None,
                ..
            }
        ));
        Ok(())
    }

    #[test]
    fn other_types_are_other() -> Result<(), EvalError> {
        assert_eq!(
            parse_event(1, r#"{"type":"ready","sr":16000}"#)?.kind,
            EventKind::Other
        );
        assert_eq!(
            parse_event(1, r#"{"type":"partial","text":"a"}"#)?.kind,
            EventKind::Partial
        );
        Ok(())
    }

    #[test]
    fn text_that_is_not_json_is_refused() {
        assert!(matches!(parse_event(1, "{"), Err(EvalError::Protocol(_))));
    }

    #[test]
    fn a_final_without_text_is_refused() {
        assert!(matches!(
            parse_event(1, r#"{"type":"final"}"#),
            Err(EvalError::Protocol(_))
        ));
    }

    #[test]
    fn a_translation_pairs_with_the_final_before_it_until_the_next_final() {
        let events = [
            final_event(10, "u1"),
            translation_event(20, "a"),
            final_event(30, "u2"),
            final_event(40, "u3"),
            translation_event(50, "b"),
        ];
        assert_eq!(
            positional_translations(&events),
            vec![(0, Some(1)), (2, None), (3, Some(4))]
        );
    }

    #[test]
    fn only_the_first_translation_after_a_final_counts() {
        let events = [
            final_event(10, "u1"),
            translation_event(20, "a"),
            translation_event(25, "a2"),
            final_event(30, "u2"),
        ];
        assert_eq!(
            positional_translations(&events),
            vec![(0, Some(1)), (3, None)]
        );
    }

    #[test]
    fn a_translation_before_any_final_is_paired_with_nothing() {
        let events = [translation_event(5, "x"), final_event(10, "u1")];
        assert_eq!(positional_translations(&events), vec![(1, None)]);
    }

    #[test]
    fn a_sample_takes_its_times_from_the_closing_final_and_its_translation() -> Result<(), EvalError>
    {
        let windows = [window(1, 0, 1_000), window(2, 3_000, 4_000)];
        let events = [
            final_event(1_500, "あい"),
            translation_event(1_800, "ai"),
            final_event(4_600, "うえ"),
        ];
        let refs = [String::from("あい"), String::from("うえお")];
        let samples = build_samples(&windows, &refs, &events)?;
        assert_eq!(samples[0].l_asr_us(), Some(500));
        assert_eq!(samples[0].l_us(), Some(800));
        assert_eq!(samples[0].l_tr_us(), Some(300));
        assert_eq!(samples[0].cer_dist, 0);
        assert_eq!(samples[1].l_asr_us(), Some(600));
        assert_eq!(samples[1].l_us(), None);
        assert_eq!(samples[1].cer_dist, 1);
        Ok(())
    }

    #[test]
    fn a_final_before_the_utterance_ended_does_not_close_it() -> Result<(), EvalError> {
        let windows = [window(1, 0, 1_000)];
        let events = [final_event(700, "early")];
        let samples = build_samples(&windows, &[String::from("x")], &events)?;
        assert_eq!(samples[0].t_final_us, None);
        assert_eq!(samples[0].finals, 1);
        assert_eq!(samples[0].text, "early");
        Ok(())
    }

    #[test]
    fn the_early_final_of_a_split_utterance_joins_its_text() -> Result<(), EvalError> {
        let windows = [window(1, 0, 1_000)];
        let events = [final_event(600, "あい"), final_event(1_400, "うえ")];
        let samples = build_samples(&windows, &[String::from("あいうえ")], &events)?;
        assert_eq!(samples[0].text, "あいうえ");
        assert_eq!(samples[0].t_final_us, Some(1_400));
        assert_eq!(samples[0].cer_dist, 0);
        Ok(())
    }

    #[test]
    fn the_first_partial_after_the_start_is_kept() -> Result<(), EvalError> {
        let windows = [window(1, 100, 1_000)];
        let events = [
            event(50, EventKind::Partial),
            event(300, EventKind::Partial),
            final_event(1_500, "a"),
        ];
        let samples = build_samples(&windows, &[String::from("a")], &events)?;
        assert_eq!(samples[0].t_partial_us, Some(300));
        Ok(())
    }

    #[test]
    fn a_partial_after_the_closing_final_is_not_this_utterances() -> Result<(), EvalError> {
        let windows = [window(1, 100, 1_000)];
        let events = [final_event(1_500, "a"), event(1_600, EventKind::Partial)];
        let samples = build_samples(&windows, &[String::from("a")], &events)?;
        assert_eq!(samples[0].t_partial_us, None);
        Ok(())
    }

    #[test]
    fn windows_out_of_order_are_refused() {
        let windows = [window(1, 500, 600), window(2, 100, 200)];
        let refs = [String::from("a"), String::from("b")];
        assert!(matches!(
            build_samples(&windows, &refs, &[]),
            Err(EvalError::Invalid(_))
        ));
    }

    #[test]
    fn an_utterance_with_no_reference_is_refused() {
        let windows = [window(2, 0, 100)];
        assert!(matches!(
            build_samples(&windows, &[String::from("a")], &[]),
            Err(EvalError::Invalid(_))
        ));
    }

    #[test]
    fn no_events_give_samples_with_nothing_measured() -> Result<(), EvalError> {
        let samples = build_samples(&[window(1, 0, 100)], &[String::from("あ")], &[])?;
        assert_eq!(samples[0].t_final_us, None);
        assert_eq!(samples[0].cer_dist, 1);
        Ok(())
    }

    fn sample(utt: u32, t_end: i64, t_final: Option<i64>, t_tr: Option<i64>) -> Sample {
        Sample {
            repeat: 1,
            utt,
            t_start_us: 0,
            t_end_us: t_end,
            t_partial_us: None,
            t_partial_tr_us: None,
            t_final_us: t_final,
            t_tr_us: t_tr,
            finals: 1,
            text: String::new(),
            translation: None,
            final_latency_ms: Some(100.0),
            cer_dist: 1,
            cer_denom: 4,
        }
    }

    #[test]
    fn the_summary_counts_translated_and_untranslated_samples() -> Result<(), EvalError> {
        let samples = [
            sample(1, 0, Some(500_000), Some(900_000)),
            sample(2, 0, Some(600_000), None),
            sample(3, 0, None, None),
        ];
        let summary = summarize(&samples)?;
        assert_eq!(summary.with_final, 2);
        assert_eq!(summary.with_translation, 1);
        assert_eq!(summary.untranslated, 2);
        assert_eq!(
            summary.l,
            Some(Stat {
                n: 1,
                p50_ms: 900,
                p95_ms: 900
            })
        );
        assert_eq!(summary.cer_micros, 250_000);
        Ok(())
    }

    #[test]
    fn a_negative_final_to_translation_gap_is_counted() -> Result<(), EvalError> {
        let samples = [sample(1, 0, Some(500_000), Some(400_000))];
        assert_eq!(summarize(&samples)?.l_tr_negative, 1);
        Ok(())
    }

    #[test]
    fn the_per_utterance_median_spans_the_repetitions() -> Result<(), EvalError> {
        let mut samples = Vec::new();
        for l_asr in [10_000, 30_000, 20_000, 50_000, 40_000] {
            samples.push(sample(1, 0, Some(l_asr), None));
        }
        let summary = summarize(&samples)?;
        assert_eq!(summary.per_utt_median_l_asr_ms, vec![Some(30)]);
        assert_eq!(summary.per_utt_median_l_ms, vec![None]);
        Ok(())
    }

    #[test]
    fn the_mean_of_the_final_latency_is_over_the_samples_that_have_one() -> Result<(), EvalError> {
        let mut without = sample(2, 0, Some(1), None);
        without.final_latency_ms = None;
        let summary = summarize(&[sample(1, 0, Some(1), None), without])?;
        assert_eq!(summary.final_latency_mean_ms, Some(100.0));
        Ok(())
    }

    #[test]
    fn an_empty_run_summarises_to_nothing_measured() -> Result<(), EvalError> {
        let summary = summarize(&[])?;
        assert_eq!(summary.samples, 0);
        assert_eq!(summary.l, None);
        assert_eq!(summary.final_latency_mean_ms, None);
        Ok(())
    }

    #[test]
    fn stat_of_nothing_is_none() -> Result<(), EvalError> {
        assert_eq!(stat_of(&[])?, None);
        Ok(())
    }

    #[test]
    fn a_sample_survives_a_json_line() -> Result<(), serde_json::Error> {
        let original = sample(3, 10, Some(20), Some(30));
        let line = serde_json::to_string(&original)?;
        assert!(!line.contains('\n'));
        let back: Sample = serde_json::from_str(&line)?;
        assert_eq!(back, original);
        Ok(())
    }
}

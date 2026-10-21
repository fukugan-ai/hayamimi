//! The counting of RP-67, shared by the tests that judge a run by it.
//!
//! A sample is valid when it has a `final` and `L_asr - L_dec` is at least
//! 0.12 s (the early detector's silence), whichever rule closed it. `L` is
//! counted over the valid samples that got a translation; the valid samples
//! without one are counted apart, so a slow translation cannot leave the
//! sample set. The p95 is the nearest rank on microseconds and is compared
//! to the line without rounding. The same run is also counted at 0.30 s.

use hayamimi_eval::compare::is_valid_l_asr;
use hayamimi_eval::latency::Sample;
use hayamimi_eval::percentile::nearest_rank;

use super::Failure;

/// The pass line for the p95 of `L`, in microseconds.
pub const LINE_US: i64 = 1_600_000;
/// The target line, in microseconds. Recorded, never judged.
pub const TARGET_US: i64 = 1_200_000;
/// The silence a sample must have waited for to be valid here, in milliseconds.
pub const VALID_MS: i64 = 120;
/// The earlier line of validity, counted beside for comparison, in milliseconds.
pub const OLD_VALID_MS: i64 = 300;

/// The two runs the acceptance reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Ja,
    En,
}

impl Lang {
    /// The name of the run's files (`<stem>.jsonl`, `<stem>.machine.jsonl`).
    pub fn stem(self) -> &'static str {
        match self {
            Lang::Ja => "e2e1",
            Lang::En => "e2e2",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Lang::Ja => "E2E-1 (ja)",
            Lang::En => "E2E-2 (en)",
        }
    }
}

/// `p50` and `p95` of one quantity over `n` samples, in microseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Column {
    pub n: usize,
    pub p50_us: i64,
    pub p95_us: i64,
}

pub fn column(values_us: &[i64]) -> Result<Option<Column>, Failure> {
    if values_us.is_empty() {
        return Ok(None);
    }
    let pick = |p| nearest_rank(values_us, p).map_err(|why| why.to_string());
    Ok(Some(Column {
        n: values_us.len(),
        p50_us: pick(50)?,
        p95_us: pick(95)?,
    }))
}

/// What one run counted.
#[derive(Debug)]
pub struct Report {
    /// Every sample of the run, the frame of 23 (or 15) utterances times the repeats.
    pub frame: usize,
    pub no_final: usize,
    /// Samples that hold two or more `final` events, counted over the whole frame.
    pub split: usize,
    pub valid: usize,
    pub translated: usize,
    pub l: Option<Column>,
    pub d: Option<Column>,
    pub decode: Option<Column>,
    pub translation: Option<Column>,
    pub over_line: usize,
    pub over_target: usize,
    pub valid_at_old_line: usize,
    pub l_at_old_line: Option<Column>,
    /// `L` of the translated samples that are valid at 0.12 s and not at 0.30 s.
    pub only_at_valid_line_us: Vec<i64>,
}

pub fn valid_at(samples: &[Sample], line_ms: i64) -> Vec<&Sample> {
    samples
        .iter()
        .filter(|sample| is_valid_l_asr(sample, line_ms))
        .collect()
}

/// The recognizer's own latency (`final_latency_ms`) in microseconds.
fn decode_us(sample: &Sample) -> Option<i64> {
    sample
        .final_latency_ms
        .map(|ms| (ms * 1000.0).round() as i64)
}

/// The wait for the end of speech: `L_asr - L_dec`, in microseconds.
fn wait_us(sample: &Sample) -> Option<i64> {
    Some(sample.l_asr_us()? - decode_us(sample)?)
}

fn count_over(values_us: &[i64], line_us: i64) -> usize {
    values_us.iter().filter(|value| **value > line_us).count()
}

/// The values of one quantity over `samples`, as a column.
fn column_of(
    samples: &[&Sample],
    quantity: fn(&Sample) -> Option<i64>,
) -> Result<Option<Column>, Failure> {
    let values: Vec<i64> = samples.iter().filter_map(|s| quantity(s)).collect();
    column(&values)
}

pub fn report_of(samples: &[Sample]) -> Result<Report, Failure> {
    let valid = valid_at(samples, VALID_MS);
    let translated: Vec<&Sample> = valid
        .iter()
        .copied()
        .filter(|sample| sample.t_tr_us.is_some())
        .collect();
    let l_us: Vec<i64> = translated.iter().filter_map(|s| s.l_us()).collect();
    let old = valid_at(samples, OLD_VALID_MS);
    Ok(Report {
        frame: samples.len(),
        no_final: samples.iter().filter(|s| s.t_final_us.is_none()).count(),
        split: samples.iter().filter(|s| s.finals >= 2).count(),
        valid: valid.len(),
        translated: translated.len(),
        l: column(&l_us)?,
        d: column_of(&translated, wait_us)?,
        decode: column_of(&translated, decode_us)?,
        translation: column_of(&translated, Sample::l_tr_us)?,
        over_line: count_over(&l_us, LINE_US),
        over_target: count_over(&l_us, TARGET_US),
        valid_at_old_line: old.len(),
        l_at_old_line: column_of(&old, Sample::l_us)?,
        only_at_valid_line_us: translated
            .iter()
            .filter(|s| !is_valid_l_asr(s, OLD_VALID_MS))
            .filter_map(|s| s.l_us())
            .collect(),
    })
}

/// Microseconds as milliseconds with one decimal.
pub fn ms1(us: i64) -> String {
    format!("{:.1}", us as f64 / 1000.0)
}

pub fn show(column: Option<Column>) -> String {
    column.map_or_else(
        || String::from("no sample"),
        |c| {
            format!(
                "p50 {} ms, p95 {} ms (n {})",
                ms1(c.p50_us),
                ms1(c.p95_us),
                c.n
            )
        },
    )
}

impl Report {
    fn render_columns(&self, label: &str) -> Vec<String> {
        vec![
            format!("{label}: L {}", show(self.l)),
            format!("{label}: d {}", show(self.d)),
            format!("{label}: decode {}", show(self.decode)),
            format!("{label}: translation {}", show(self.translation)),
        ]
    }

    fn render_counts(&self, label: &str) -> Vec<String> {
        let only: Vec<String> = self
            .only_at_valid_line_us
            .iter()
            .map(|us| format!("{} ms", ms1(*us)))
            .collect();
        let only = if only.is_empty() {
            String::from("none")
        } else {
            only.join(", ")
        };
        let met = self.l.is_some_and(|l| l.p95_us <= TARGET_US);
        vec![
            format!(
                "{label}: frame {}, no final {}, two or more finals {}, valid {}, translated {}, valid without a translation {}",
                self.frame,
                self.no_final,
                self.split,
                self.valid,
                self.translated,
                self.valid - self.translated
            ),
            format!(
                "{label}: {} of {} samples over {} ms (pass line: p95 within it)",
                self.over_line,
                self.translated,
                LINE_US / 1000
            ),
            format!(
                "{label}: target {} ms (recorded, not judged): {}, {} of {} samples over",
                TARGET_US / 1000,
                if met { "met" } else { "not met" },
                self.over_target,
                self.translated
            ),
            format!(
                "{label}: counted at 0.30 s (recorded): valid {}, L {}; valid only at 0.12 s: {only}",
                self.valid_at_old_line,
                show(self.l_at_old_line)
            ),
        ]
    }

    pub fn render(&self, label: &str) -> Vec<String> {
        let mut lines = self.render_counts(label);
        lines.extend(self.render_columns(label));
        lines
    }
}

/// The reason a run's `L` fails the line, if it does.
pub fn p95_failure(lang: Lang, report: &Report) -> Option<String> {
    let label = lang.label();
    match report.l {
        None => Some(format!("{label}: no valid sample has a translation")),
        Some(l) if l.p95_us > LINE_US => Some(format!(
            "{label}: L p95 {} ms is over {} ms ({} of {} samples are over)",
            ms1(l.p95_us),
            LINE_US / 1000,
            report.over_line,
            l.n
        )),
        Some(_) => None,
    }
}

/// One sample: the speech ends at 1 s, the `final` comes `wait_ms + decode_ms`
/// later, and the translation `translation_ms` after the `final` (none: no translation).
pub fn sample(utt: u32, wait_ms: i64, decode_ms: i64, translation_ms: Option<i64>) -> Sample {
    let t_end_us = 1_000_000;
    let t_final_us = t_end_us + (wait_ms + decode_ms) * 1000;
    Sample {
        repeat: 1,
        utt,
        t_start_us: 0,
        t_end_us,
        t_partial_us: None,
        t_partial_tr_us: None,
        t_final_us: Some(t_final_us),
        t_tr_us: translation_ms.map(|ms| t_final_us + ms * 1000),
        finals: 1,
        text: String::new(),
        translation: None,
        final_latency_ms: Some(decode_ms as f64),
        cer_dist: 0,
        cer_denom: 0,
    }
}

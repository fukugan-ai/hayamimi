//! The rules the latency acceptance of RP-3 and RP-4 shares: when a Rust run
//! is "not slower than" the Python baseline, and over which utterances the
//! paired comparison is made.
//!
//! A sample counts for `L_asr` only when the server waited its silence
//! (`L_asr - L_dec >= min_silence`): a sample whose `final` came from a
//! clipped span carries a delay that is not the recognizer's. The comparison
//! set is fixed by the Python runs alone, so a Rust run cannot move it.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::EvalError;
use crate::latency::{Sample, Stat, stat_of};
use crate::manifest::write_text;
use crate::percentile::{micros_to_ms, nearest_rank};

/// Of every 23 utterances of the comparison set, this many must be as fast as Python's.
const PAIRED_NUMERATOR: usize = 20;
const PAIRED_DENOMINATOR: usize = 23;
/// Two means of the server's `latency_ms` agree when they differ by at most this (10^-6 ms).
const MEAN_TOLERANCE_MICROS: i64 = 500_000;

fn invalid(message: impl Into<String>) -> EvalError {
    EvalError::Invalid(message.into())
}

/// How many utterances of a comparison set of `set_size` must be at least as
/// fast: `set_size * 20 / 23`, rounded up. An empty set is an error, never a pass.
pub fn comparison_threshold(set_size: usize) -> Result<usize, EvalError> {
    if set_size == 0 {
        return Err(invalid(
            "the comparison set is empty: an empty set is not a pass",
        ));
    }
    Ok((set_size * PAIRED_NUMERATOR).div_ceil(PAIRED_DENOMINATOR))
}

/// Rust's p50 and p95 are no greater than Python's. Every failing percentile is named.
pub fn check_percentiles(metric: &str, python: &Stat, rust: &Stat) -> Result<(), EvalError> {
    let mut slower = Vec::new();
    if rust.p50_ms > python.p50_ms {
        slower.push(format!(
            "p50 rust {} ms > python {} ms",
            rust.p50_ms, python.p50_ms
        ));
    }
    if rust.p95_ms > python.p95_ms {
        slower.push(format!(
            "p95 rust {} ms > python {} ms",
            rust.p95_ms, python.p95_ms
        ));
    }
    if slower.is_empty() {
        return Ok(());
    }
    Err(invalid(format!("{metric}: {}", slower.join("; "))))
}

/// At least `comparison_threshold(set_size)` utterances are not slower than Python's.
pub fn check_paired(set_size: usize, not_slower: usize) -> Result<(), EvalError> {
    let needed = comparison_threshold(set_size)?;
    if not_slower >= needed {
        return Ok(());
    }
    Err(invalid(format!(
        "{not_slower} of {set_size} utterances are not slower than python, {needed} are needed"
    )))
}

/// Milliseconds as 10^-6 ms, the unit the means are compared in.
fn ms_to_micros(ms: f64) -> i64 {
    (ms * 1_000_000.0).round() as i64
}

/// `L_asr - L_dec` of a sample in 10^-6 ms; `None` without a closing `final` or its latency.
fn silence_gap_micros(sample: &Sample) -> Option<i64> {
    let l_asr_us = sample.l_asr_us()?;
    let l_dec_ms = sample.final_latency_ms?;
    Some(l_asr_us * 1000 - ms_to_micros(l_dec_ms))
}

/// Whether the sample's `L_asr` is the recognizer's: the silence the rule that
/// closed it waited for fits inside it. `min_silence_ms` is the line of this
/// sample, so a caller that knows the closing rule of each sample passes the
/// line of that rule (RP-32); a caller that knows none passes the run's
/// `--min-silence` for every sample.
pub fn is_valid_l_asr(sample: &Sample, min_silence_ms: i64) -> bool {
    silence_gap_micros(sample).is_some_and(|gap| gap >= min_silence_ms * 1_000_000)
}

/// Of the valid samples of a run, at least this many in 23 must have a shown translation.
const SHARE_NUMERATOR: usize = 20;
const SHARE_DENOMINATOR: usize = 23;

/// The valid samples of a run (the utterance samples whose `L_asr` is the
/// recognizer's, [`is_valid_l_asr`]) and how many of them got a translation.
/// A valid sample with only a `held` translation, or whose translation never
/// came, counts in `valid` and not in `translated`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TranslatedShare {
    pub valid: usize,
    pub translated: usize,
}

impl TranslatedShare {
    /// The valid samples that got no translation.
    pub fn missing(&self) -> usize {
        self.valid - self.translated
    }

    /// `translated / valid` is at least 20 / 23. A run with no valid sample is not a pass.
    pub fn is_enough(&self) -> bool {
        self.valid > 0 && self.translated * SHARE_DENOMINATOR >= self.valid * SHARE_NUMERATOR
    }
}

/// Counts the samples of `valid` (already judged valid, each against its own
/// line) and those among them with a translation.
pub fn share_of_valid(valid: &[&Sample]) -> TranslatedShare {
    TranslatedShare {
        valid: valid.len(),
        translated: valid
            .iter()
            .filter(|sample| sample.t_tr_us.is_some())
            .count(),
    }
}

/// Counts the valid samples of a run and those among them with a translation.
pub fn translated_share(samples: &[Sample], min_silence_ms: i64) -> TranslatedShare {
    share_of_valid(&valid_samples(samples, min_silence_ms))
}

/// The share of valid samples with a translation reaches 20 / 23. A run
/// below it fails however small its `L` is: a translation the checker stops
/// leaves no `L`, so a fast `L` over the rest proves nothing.
pub fn check_translated_share(
    samples: &[Sample],
    min_silence_ms: i64,
) -> Result<TranslatedShare, EvalError> {
    check_share(translated_share(samples, min_silence_ms))
}

/// [`check_translated_share`] over a share that is already counted.
pub fn check_share(share: TranslatedShare) -> Result<TranslatedShare, EvalError> {
    if share.valid == 0 {
        return Err(invalid(
            "the run has no valid sample: an empty run is not a pass",
        ));
    }
    if !share.is_enough() {
        return Err(invalid(format!(
            "{} of {} valid samples have a translation, {SHARE_NUMERATOR} of {SHARE_DENOMINATOR} are needed",
            share.translated, share.valid
        )));
    }
    Ok(share)
}

fn valid_samples(samples: &[Sample], min_silence_ms: i64) -> Vec<&Sample> {
    samples
        .iter()
        .filter(|sample| is_valid_l_asr(sample, min_silence_ms))
        .collect()
}

/// The utterance numbers that have a valid sample in every run.
pub fn l_asr_comparison_set(runs: &[&[Sample]], min_silence_ms: i64) -> Vec<u32> {
    let mut counts: BTreeMap<u32, usize> = BTreeMap::new();
    for run in runs {
        let mut seen: Vec<u32> = valid_samples(run, min_silence_ms)
            .iter()
            .map(|sample| sample.utt)
            .collect();
        seen.sort_unstable();
        seen.dedup();
        for utt in seen {
            *counts.entry(utt).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .filter(|(_, runs_with_it)| *runs_with_it == runs.len())
        .map(|(utt, _)| utt)
        .collect()
}

/// The median `L_asr` in microseconds of each utterance's valid samples.
fn medians_us(samples: &[&Sample]) -> Result<BTreeMap<u32, i64>, EvalError> {
    let mut by_utt: BTreeMap<u32, Vec<i64>> = BTreeMap::new();
    for sample in samples {
        if let Some(l_asr) = sample.l_asr_us() {
            by_utt.entry(sample.utt).or_default().push(l_asr);
        }
    }
    by_utt
        .into_iter()
        .map(|(utt, values)| Ok((utt, nearest_rank(&values, 50)?)))
        .collect()
}

/// The numbers of one judged comparison.
#[derive(Debug, Clone, PartialEq)]
pub struct LAsrReport {
    pub set_size: usize,
    pub not_slower: usize,
    pub threshold: usize,
    pub python: Stat,
    pub rust: Stat,
    /// The server's own `latency_ms`, recorded and never judged.
    pub python_l_dec_p50_ms: Option<i64>,
    pub rust_l_dec_p50_ms: Option<i64>,
}

fn l_asr_stat(samples: &[&Sample], side: &str) -> Result<Stat, EvalError> {
    let values: Vec<i64> = samples.iter().filter_map(|s| s.l_asr_us()).collect();
    match stat_of(&values)? {
        Some(stat) => Ok(stat),
        None => Err(invalid(format!("{side}: no sample has a valid L_asr"))),
    }
}

fn l_dec_p50_ms(samples: &[&Sample]) -> Result<Option<i64>, EvalError> {
    let micros: Vec<i64> = samples
        .iter()
        .filter_map(|s| s.final_latency_ms)
        .map(|ms| (ms * 1000.0).round() as i64)
        .collect();
    if micros.is_empty() {
        return Ok(None);
    }
    Ok(Some(micros_to_ms(nearest_rank(&micros, 50)?)))
}

/// How many utterances of `set` have a Rust median no greater than Python's.
fn not_slower_utterances(
    set: &[u32],
    python: &BTreeMap<u32, i64>,
    rust: &BTreeMap<u32, i64>,
) -> usize {
    set.iter()
        .filter(|utt| match (python.get(utt), rust.get(utt)) {
            (Some(python), Some(rust)) => rust <= python,
            _ => false,
        })
        .count()
}

/// RP-4: Rust's `L_asr` p50 and p95 are no greater than Python's, and enough
/// utterances of the comparison set are as fast. `python` holds one slice per
/// Python run; `min_silence_ms` is the value both sides were started with.
pub fn check_l_asr(
    python: &[&[Sample]],
    rust: &[Sample],
    min_silence_ms: i64,
) -> Result<LAsrReport, EvalError> {
    let set = l_asr_comparison_set(python, min_silence_ms);
    let python_valid: Vec<&Sample> = python
        .iter()
        .flat_map(|run| valid_samples(run, min_silence_ms))
        .collect();
    let rust_valid = valid_samples(rust, min_silence_ms);
    let python_stat = l_asr_stat(&python_valid, "python")?;
    let rust_stat = l_asr_stat(&rust_valid, "rust")?;
    check_percentiles("L_asr", &python_stat, &rust_stat)?;
    let not_slower =
        not_slower_utterances(&set, &medians_us(&python_valid)?, &medians_us(&rust_valid)?);
    check_paired(set.len(), not_slower)?;
    Ok(LAsrReport {
        set_size: set.len(),
        not_slower,
        threshold: comparison_threshold(set.len())?,
        python: python_stat,
        rust: rust_stat,
        python_l_dec_p50_ms: l_dec_p50_ms(&python_valid)?,
        rust_l_dec_p50_ms: l_dec_p50_ms(&rust_valid)?,
    })
}

/// The `L_asr` of a run on one provider, written down and not judged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LAsrRecord {
    pub provider: String,
    pub samples: usize,
    pub p50_ms: i64,
    pub p95_ms: i64,
}

/// The record of a run: its valid samples' `L_asr` p50 and p95, whatever their size.
pub fn l_asr_record(
    provider: &str,
    rust: &[Sample],
    min_silence_ms: i64,
) -> Result<LAsrRecord, EvalError> {
    let valid = valid_samples(rust, min_silence_ms);
    let stat = l_asr_stat(&valid, provider)?;
    Ok(LAsrRecord {
        provider: provider.to_owned(),
        samples: stat.n,
        p50_ms: stat.p50_ms,
        p95_ms: stat.p95_ms,
    })
}

/// Writes a record as one JSON document.
pub fn write_l_asr_record(path: &Path, record: &LAsrRecord) -> Result<(), EvalError> {
    let text = serde_json::to_string_pretty(record).map_err(|e| EvalError::json(path, e))?;
    write_text(path, &format!("{text}\n"))
}

/// A samples file of `stream`: one JSON sample per line.
pub fn read_samples(path: &Path) -> Result<Vec<Sample>, EvalError> {
    let text = std::fs::read_to_string(path).map_err(|e| EvalError::io(path, e))?;
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).map_err(|e| EvalError::json(path, e)))
        .collect()
}

/// Why one repeat of a clip has no `L_asr`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lack {
    /// No `final` was placed in the utterance.
    NoFinal,
    /// A `final` was placed in it, but the last came before the utterance ended.
    ClosedBeforeEnd,
}

impl Lack {
    /// The name the record gives the reason.
    pub fn name(self) -> &'static str {
        match self {
            Lack::NoFinal => "no_final",
            Lack::ClosedBeforeEnd => "closed_before_end",
        }
    }
}

/// A repeat of a clip: its number, and why it has no `L_asr` (none when it has one).
type Repeat = (u32, Option<Lack>);

fn repeat_of(sample: &Sample) -> Repeat {
    let lack = match (sample.l_asr_us(), sample.finals) {
        (Some(_), _) => None,
        (None, 0) => Some(Lack::NoFinal),
        (None, _) => Some(Lack::ClosedBeforeEnd),
    };
    (sample.repeat, lack)
}

/// Why a clip has no `L_asr` in a run: the same reason in every repeat, or the
/// reason of each repeat when they differ.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SideReason {
    Same(Lack),
    Mixed(Vec<(u32, Lack)>),
}

impl SideReason {
    /// The reason of a clip none of whose repeats has an `L_asr`; none when one has.
    fn of(repeats: &[Repeat]) -> Option<SideReason> {
        let lacks: Vec<(u32, Lack)> = repeats
            .iter()
            .map_while(|(number, lack)| lack.map(|lack| (*number, lack)))
            .collect();
        if lacks.len() != repeats.len() || lacks.is_empty() {
            return None;
        }
        let first = lacks[0].1;
        if lacks.iter().all(|(_, lack)| *lack == first) {
            Some(SideReason::Same(first))
        } else {
            Some(SideReason::Mixed(lacks))
        }
    }

    /// The name of the reason: the one reason of every repeat, or `mixed`.
    pub fn name(&self) -> &'static str {
        match self {
            SideReason::Same(lack) => lack.name(),
            SideReason::Mixed(_) => "mixed",
        }
    }

    /// The reason as the record writes it.
    pub fn text(&self) -> String {
        match self {
            SideReason::Same(lack) => lack.name().to_owned(),
            SideReason::Mixed(each) => {
                let listed: Vec<String> = each
                    .iter()
                    .map(|(number, lack)| format!("{number} {}", lack.name()))
                    .collect();
                format!("mixed: {}", listed.join(", "))
            }
        }
    }
}

/// Why a clip of the pair run is not joined to the `--lang` runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClipReason {
    /// The pair run could not read it, and the `--lang` run could.
    Pair(SideReason),
    /// The pair run read it, and the `--lang` run `run` could not.
    MissingInLangRun { run: String, side: SideReason },
    /// Neither run could read it.
    MissingInBoth {
        pair: SideReason,
        run: String,
        side: SideReason,
    },
}

impl ClipReason {
    /// The name the record gives the reason.
    pub fn name(&self) -> &'static str {
        match self {
            ClipReason::Pair(side) => side.name(),
            ClipReason::MissingInLangRun { .. } => "missing_in_lang_run",
            ClipReason::MissingInBoth { .. } => "missing_in_both",
        }
    }

    /// What the pair run did in the clip, when it could not read it.
    pub fn pair_side(&self) -> Option<&SideReason> {
        match self {
            ClipReason::Pair(side) | ClipReason::MissingInBoth { pair: side, .. } => Some(side),
            ClipReason::MissingInLangRun { .. } => None,
        }
    }

    /// The reason as the record writes it, with the side of each run.
    pub fn text(&self) -> String {
        match self {
            ClipReason::Pair(side) => side.text(),
            ClipReason::MissingInLangRun { run, side } => {
                format!("missing_in_lang_run ({run}): {}", side.text())
            }
            ClipReason::MissingInBoth { pair, run, side } => format!(
                "missing_in_both: pair {}; {run} {}",
                pair.text(),
                side.text()
            ),
        }
    }
}

/// A clip that could not be joined, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NamedClip {
    pub sha: String,
    /// The number of its utterance in the pair run's manifest, from 1.
    pub utt: u32,
    pub reason: ClipReason,
}

/// A `--lang` run: the label the record gives it, its samples and the sha256
/// of the clip of each of its utterances (`shas[utt - 1]`).
pub struct ForcedRun<'a> {
    pub label: &'a str,
    pub samples: &'a [Sample],
    pub shas: &'a [String],
}

/// The clips of the pair run that were joined, and the ones that were named.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipNames {
    pub joined: Vec<String>,
    pub named: Vec<NamedClip>,
}

/// The repeats of each clip of a run, by the sha256 of the clip, in the order of
/// their numbers. `shas[utt - 1]` is the clip of utterance `utt`.
fn repeats_by_clip<'a>(
    samples: &[Sample],
    shas: &'a [String],
) -> Result<BTreeMap<&'a str, Vec<Repeat>>, EvalError> {
    let mut found: BTreeMap<&str, Vec<Repeat>> = BTreeMap::new();
    for sample in samples {
        let Some(sha) = usize::try_from(sample.utt)
            .ok()
            .and_then(|utt| utt.checked_sub(1))
            .and_then(|at| shas.get(at))
        else {
            return Err(invalid(format!(
                "utt {} has no row in the manifest",
                sample.utt
            )));
        };
        found.entry(sha).or_default().push(repeat_of(sample));
    }
    for repeats in found.values_mut() {
        repeats.sort_by_key(|(number, _)| *number);
    }
    Ok(found)
}

/// The side of a clip in the run that read it, with the label of the run.
type Side<'a> = (&'a str, &'a [Repeat]);

/// How a clip stands between the pair run and the `--lang` run that read it.
fn clip_reason(pair: &[Repeat], (label, forced): Side<'_>) -> Option<ClipReason> {
    match (SideReason::of(pair), SideReason::of(forced)) {
        (None, None) => None,
        (Some(side), None) => Some(ClipReason::Pair(side)),
        (None, Some(side)) => Some(ClipReason::MissingInLangRun {
            run: label.to_owned(),
            side,
        }),
        (Some(pair), Some(side)) => Some(ClipReason::MissingInBoth {
            pair,
            run: label.to_owned(),
            side,
        }),
    }
}

/// Ties the clips of the pair run to the `--lang` runs by sha256, and gives
/// every clip that is not joined its reason. A clip is joined when a repeat of
/// it has `L_asr` in both runs. A clip with no sample in the pair run, or one
/// that no `--lang` run (or no sample of it) can speak for, is an error: the
/// record would have a clip with no reason, and the joined and the named would
/// not add up to the clips of the manifest.
pub fn name_unjoined_clips(
    pair_samples: &[Sample],
    pair_shas: &[String],
    forced: &[ForcedRun<'_>],
) -> Result<ClipNames, EvalError> {
    let pair = repeats_by_clip(pair_samples, pair_shas)?;
    let runs: Vec<(&str, BTreeMap<&str, Vec<Repeat>>)> = forced
        .iter()
        .map(|run| Ok((run.label, repeats_by_clip(run.samples, run.shas)?)))
        .collect::<Result<_, EvalError>>()?;
    let mut found = ClipNames {
        joined: Vec::new(),
        named: Vec::new(),
    };
    for (at, sha) in pair_shas.iter().enumerate() {
        if found.joined.contains(sha) || found.named.iter().any(|clip| clip.sha == *sha) {
            continue;
        }
        let utt = at + 1;
        let no_reason = |where_: &str| {
            invalid(format!(
                "clip {sha} (utterance {utt}) has no sample in {where_}: no reason can be given"
            ))
        };
        let pair_side = pair
            .get(sha.as_str())
            .ok_or_else(|| no_reason("the pair run"))?;
        let forced_side = runs
            .iter()
            .find_map(|(label, repeats)| repeats.get(sha.as_str()).map(|r| (*label, r.as_slice())))
            .ok_or_else(|| no_reason("any --lang run"))?;
        match clip_reason(pair_side, forced_side) {
            None => found.joined.push(sha.clone()),
            Some(reason) => found.named.push(NamedClip {
                sha: sha.clone(),
                utt: u32::try_from(utt).map_err(|_| invalid(String::from("too many clips")))?,
                reason,
            }),
        }
    }
    Ok(found)
}

/// The `latency_ms` of every `final` event of a WebSocket capture, in 10^-6 ms.
pub fn capture_final_latencies(text: &str) -> Result<Vec<i64>, EvalError> {
    let mut found = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let value: Value = serde_json::from_str(line)
            .map_err(|e| invalid(format!("an event line is not JSON: {e}")))?;
        if value.pointer("/event/type").and_then(Value::as_str) != Some("final") {
            continue;
        }
        let Some(ms) = value.pointer("/event/latency_ms").and_then(Value::as_f64) else {
            return Err(invalid("a final event has no latency_ms"));
        };
        found.push(ms_to_micros(ms));
    }
    Ok(found)
}

/// Every `latency=<value>ms` the server printed, in 10^-6 ms.
pub fn log_final_latencies(text: &str) -> Result<Vec<i64>, EvalError> {
    let mut found = Vec::new();
    for line in text.lines() {
        let Some((_, tail)) = line.rsplit_once("latency=") else {
            continue;
        };
        let Some((number, _)) = tail.split_once("ms") else {
            return Err(invalid(format!("the latency in {line:?} has no unit")));
        };
        let Ok(ms) = number.parse::<f64>() else {
            return Err(invalid(format!("{number:?} is not a latency")));
        };
        found.push(ms_to_micros(ms));
    }
    Ok(found)
}

fn mean_micros(values: &[i64]) -> Result<i64, EvalError> {
    let count = i64::try_from(values.len()).map_err(|_| invalid("too many values"))?;
    match values.iter().sum::<i64>().checked_div(count) {
        Some(mean) => Ok(mean),
        None => Err(invalid("the mean of no values")),
    }
}

/// The WebSocket capture and the server's log hold the same `final` events:
/// as many of them, and means within 0.5 ms (the log rounds each value to a
/// whole millisecond). Values are in 10^-6 ms. Two empty sides are an error,
/// not an agreement.
pub fn check_final_latency_agreement(ws: &[i64], log: &[i64]) -> Result<(), EvalError> {
    if ws.is_empty() && log.is_empty() {
        return Err(invalid("0 finals in the capture and 0 in the log"));
    }
    if ws.len() != log.len() {
        return Err(invalid(format!(
            "{} finals in the capture, {} in the log",
            ws.len(),
            log.len()
        )));
    }
    let gap = (mean_micros(ws)? - mean_micros(log)?).abs();
    if gap > MEAN_TOLERANCE_MICROS {
        return Err(invalid(format!(
            "the means differ by {gap} (10^-6 ms), the limit is {MEAN_TOLERANCE_MICROS}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SILENCE_MS: i64 = 350;

    /// A sample whose `final` closed `l_asr_ms` after the speech ended, `l_dec_ms` of it decoding.
    fn sample(utt: u32, l_asr_ms: i64, l_dec_ms: f64) -> Sample {
        Sample {
            repeat: 1,
            utt,
            t_start_us: 0,
            t_end_us: 1_000_000,
            t_partial_us: None,
            t_partial_tr_us: None,
            t_final_us: Some(1_000_000 + l_asr_ms * 1000),
            t_tr_us: None,
            finals: 1,
            text: String::new(),
            translation: None,
            final_latency_ms: Some(l_dec_ms),
            cer_dist: 0,
            cer_denom: 0,
        }
    }

    fn without_final(utt: u32) -> Sample {
        Sample {
            t_final_us: None,
            final_latency_ms: None,
            ..sample(utt, 0, 0.0)
        }
    }

    fn stat(p50_ms: i64, p95_ms: i64) -> Stat {
        Stat {
            n: 20,
            p50_ms,
            p95_ms,
        }
    }

    #[test]
    fn the_threshold_is_n_times_20_over_23_rounded_up() -> Result<(), EvalError> {
        assert_eq!(comparison_threshold(19)?, 17);
        assert_eq!(comparison_threshold(20)?, 18);
        assert_eq!(comparison_threshold(23)?, 20);
        assert_eq!(comparison_threshold(1)?, 1);
        Ok(())
    }

    #[test]
    fn an_empty_set_has_no_threshold() {
        assert!(comparison_threshold(0).is_err());
        assert!(check_paired(0, 0).is_err());
    }

    #[test]
    fn equal_percentiles_pass_and_one_millisecond_over_names_the_percentile() {
        assert!(check_percentiles("L_asr", &stat(800, 1500), &stat(800, 1500)).is_ok());
        let p50 = check_percentiles("L_asr", &stat(800, 1500), &stat(801, 1500));
        assert!(
            matches!(&p50, Err(EvalError::Invalid(m)) if m.contains("p50") && !m.contains("p95"))
        );
        let p95 = check_percentiles("L_asr", &stat(800, 1500), &stat(800, 1501));
        assert!(
            matches!(&p95, Err(EvalError::Invalid(m)) if m.contains("p95") && !m.contains("p50"))
        );
        let both = check_percentiles("L_asr", &stat(800, 1500), &stat(900, 1600));
        assert!(
            matches!(&both, Err(EvalError::Invalid(m)) if m.contains("p50") && m.contains("p95"))
        );
    }

    #[test]
    fn eighteen_of_twenty_pass_and_seventeen_do_not() {
        assert!(check_paired(20, 18).is_ok());
        assert!(check_paired(20, 17).is_err());
    }

    /// `valid` valid samples, the first `translated` of them with a translation.
    fn run_of(valid: usize, translated: usize) -> Vec<Sample> {
        (0..valid)
            .map(|index| Sample {
                t_tr_us: (index < translated).then_some(2_000_000),
                ..sample(1, 700, 300.0)
            })
            .collect()
    }

    #[test]
    fn twenty_of_twenty_three_have_enough_translations_and_nineteen_do_not() {
        assert!(translated_share(&run_of(23, 20), SILENCE_MS).is_enough());
        assert!(!translated_share(&run_of(23, 19), SILENCE_MS).is_enough());
        // The boundary moves with the count: 40 of 46 is exactly 20 / 23.
        assert!(check_translated_share(&run_of(46, 40), SILENCE_MS).is_ok());
        assert!(check_translated_share(&run_of(46, 39), SILENCE_MS).is_err());
    }

    #[test]
    fn a_share_counted_beforehand_is_judged_like_one_counted_here() {
        let run = run_of(23, 20);
        let valid: Vec<&Sample> = run.iter().collect();
        assert_eq!(share_of_valid(&valid), translated_share(&run, SILENCE_MS));
        assert!(check_share(share_of_valid(&valid)).is_ok());
        let short = run_of(23, 19);
        let valid: Vec<&Sample> = short.iter().collect();
        assert!(check_share(share_of_valid(&valid)).is_err());
        assert!(check_share(share_of_valid(&[])).is_err());
    }

    #[test]
    fn a_run_with_no_valid_sample_is_not_a_pass() {
        assert!(check_translated_share(&[], SILENCE_MS).is_err());
        assert!(check_translated_share(&[without_final(1)], SILENCE_MS).is_err());
    }

    #[test]
    fn samples_with_a_wrong_end_or_no_final_are_not_in_the_denominator() {
        let mut run = run_of(23, 20);
        // Neither of these is valid, so 20 of 23 stays 20 of 23.
        run.push(sample(2, 650, 300.001));
        run.push(without_final(3));
        let share = translated_share(&run, SILENCE_MS);
        assert_eq!((share.valid, share.translated), (23, 20));
        assert!(share.is_enough());
    }

    #[test]
    fn a_valid_sample_without_a_translation_is_in_the_denominator() {
        let share = translated_share(&run_of(93, 67), SILENCE_MS);
        assert_eq!(
            (share.valid, share.translated, share.missing()),
            (93, 67, 26)
        );
        let said = check_translated_share(&run_of(93, 67), SILENCE_MS);
        assert!(matches!(&said, Err(EvalError::Invalid(m)) if m.contains("67 of 93")));
    }

    #[test]
    fn a_sample_is_valid_when_the_silence_fits_in_l_asr() {
        // 650 ms of L_asr, 300 ms of it decoding: 350 ms of silence, exactly the limit.
        assert!(is_valid_l_asr(&sample(1, 650, 300.0), SILENCE_MS));
        assert!(!is_valid_l_asr(&sample(1, 650, 300.001), SILENCE_MS));
        assert!(!is_valid_l_asr(&without_final(1), SILENCE_MS));
    }

    #[test]
    fn the_set_is_the_utterances_valid_in_every_run() {
        let first = vec![
            sample(1, 700, 300.0),
            sample(2, 700, 300.0),
            without_final(3),
        ];
        let second = vec![
            sample(1, 700, 300.0),
            without_final(2),
            sample(3, 700, 300.0),
        ];
        let set = l_asr_comparison_set(&[&first, &second], SILENCE_MS);
        assert_eq!(set, vec![1]);
        assert!(l_asr_comparison_set(&[], SILENCE_MS).is_empty());
    }

    /// 20 utterances, five repeats of each. Python takes 800 ms except utterances 19 and 20,
    /// which take 5000 ms and put Python's p95 far above Rust's few slower utterances.
    /// The first `slower` utterances take 801 ms in Rust, the others 800 ms.
    fn runs(slower: u32) -> (Vec<Sample>, Vec<Sample>) {
        let make = |at: &dyn Fn(u32) -> i64| -> Vec<Sample> {
            (1..=20)
                .flat_map(|utt| (0..5).map(move |_| utt))
                .map(|utt| sample(utt, at(utt), 300.0))
                .collect()
        };
        let python = make(&|utt| if utt >= 19 { 5000 } else { 800 });
        let rust = make(&|utt| if utt <= slower { 801 } else { 800 });
        (python, rust)
    }

    #[test]
    fn a_rust_run_as_fast_on_eighteen_of_twenty_utterances_passes() -> Result<(), EvalError> {
        let (python, rust) = runs(2);
        let report = check_l_asr(&[&python], &rust, SILENCE_MS)?;
        assert_eq!(
            (report.set_size, report.not_slower, report.threshold),
            (20, 18, 18)
        );
        Ok(())
    }

    #[test]
    fn a_rust_run_as_fast_on_seventeen_of_twenty_utterances_is_rejected() {
        let (python, rust) = runs(3);
        let said = check_l_asr(&[&python], &rust, SILENCE_MS);
        assert!(
            matches!(&said, Err(EvalError::Invalid(m)) if m.contains("17 of 20")),
            "{said:?}"
        );
    }

    #[test]
    fn l_dec_is_recorded_but_does_not_decide() -> Result<(), EvalError> {
        let python: Vec<Sample> = (1..=20).map(|utt| sample(utt, 800, 300.0)).collect();
        // Twice the decoding time, but a shorter L_asr: the silence made up the difference.
        let rust: Vec<Sample> = (1..=20)
            .map(|utt| sample(utt, 790, 600.0 - 160.0))
            .collect();
        let report = check_l_asr(&[&python], &rust, SILENCE_MS)?;
        assert_eq!(report.python_l_dec_p50_ms, Some(300));
        assert_eq!(report.rust_l_dec_p50_ms, Some(440));
        Ok(())
    }

    #[test]
    fn the_python_runs_alone_fix_the_set() -> Result<(), EvalError> {
        let mut python: Vec<Sample> = (1..=20).map(|utt| sample(utt, 800, 300.0)).collect();
        python.push(without_final(21));
        let mut rust: Vec<Sample> = (1..=20).map(|utt| sample(utt, 800, 300.0)).collect();
        rust.push(sample(21, 700, 300.0));
        let report = check_l_asr(&[&python], &rust, SILENCE_MS)?;
        assert_eq!(report.set_size, 20);
        Ok(())
    }

    #[test]
    fn a_rust_run_with_no_valid_sample_is_an_error() {
        let python: Vec<Sample> = (1..=20).map(|utt| sample(utt, 800, 300.0)).collect();
        let rust: Vec<Sample> = (1..=20).map(without_final).collect();
        assert!(check_l_asr(&[&python], &rust, SILENCE_MS).is_err());
    }

    #[test]
    fn a_cpu_record_is_made_whatever_its_size() -> Result<(), EvalError> {
        let slow: Vec<Sample> = (1..=20).map(|utt| sample(utt, 5000, 300.0)).collect();
        let record = l_asr_record("cpu", &slow, SILENCE_MS)?;
        assert_eq!(
            record,
            LAsrRecord {
                provider: String::from("cpu"),
                samples: 20,
                p50_ms: 5000,
                p95_ms: 5000
            }
        );
        Ok(())
    }

    #[test]
    fn the_capture_and_the_log_are_read_into_the_same_unit() -> Result<(), EvalError> {
        let capture = "{\"event\":{\"type\":\"partial\"},\"t_us\":1}\n\
                       {\"event\":{\"type\":\"final\",\"latency_ms\":622.5},\"t_us\":2}\n";
        assert_eq!(capture_final_latencies(capture)?, vec![622_500_000]);
        assert_eq!(
            log_final_latencies("~ text latency=623ms\nother\n")?,
            vec![623_000_000]
        );
        assert!(capture_final_latencies("{\"event\":{\"type\":\"final\"},\"t_us\":2}").is_err());
        assert!(log_final_latencies("latency=623").is_err());
        Ok(())
    }

    #[test]
    fn final_latencies_agree_within_half_a_millisecond() {
        assert!(
            check_final_latency_agreement(&[1_000_000, 2_000_000], &[1_500_000, 2_500_000]).is_ok()
        );
        assert!(check_final_latency_agreement(&[1_000_000], &[1_500_001]).is_err());
        assert!(check_final_latency_agreement(&[1], &[1, 2]).is_err());
        assert!(check_final_latency_agreement(&[], &[]).is_err());
    }
}

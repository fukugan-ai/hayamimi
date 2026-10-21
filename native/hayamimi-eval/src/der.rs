//! The diarization error rate, scored the way `simpleder` scores it.
//!
//! The error is the load (the integral of the larger of the reference and the
//! hypothesis speaker count) minus the overlap of the best one-to-one pairing
//! of reference and hypothesis speakers, over the reference speech time. The
//! `collar` seconds around every reference turn boundary are left out of both.
//!
//! [`breakdown`] splits the same error into the miss, false alarm and
//! confusion of NIST md-eval. On every stretch between turn boundaries it
//! counts the reference turns `r`, the hypothesis turns `h` and the pairs of
//! turns of paired speakers `c`; the stretch adds `max(r - h, 0)` to the miss,
//! `max(h - r, 0)` to the false alarm and `min(r, h) - c` to the confusion.
//! The three sum to the load minus the shared time, which is what [`der`] computes.

use std::collections::BTreeMap;

use hayamimi_asr::hungarian;

use crate::error::EvalError;

/// One speech turn of a speaker, in seconds.
#[derive(Debug, Clone, PartialEq)]
pub struct Turn {
    pub speaker: String,
    pub start: f64,
    pub end: f64,
}

impl Turn {
    pub fn new(speaker: &str, start: f64, end: f64) -> Turn {
        Turn {
            speaker: speaker.to_owned(),
            start,
            end,
        }
    }
}

/// The `SPEAKER` lines of an RTTM file. Other lines are skipped; a malformed
/// `SPEAKER` line is an error that names `origin` and the line.
pub fn parse_rttm(text: &str, origin: &str) -> Result<Vec<Turn>, EvalError> {
    let mut turns = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.first() != Some(&"SPEAKER") {
            continue;
        }
        turns.push(parse_speaker_line(&fields, origin, index + 1)?);
    }
    Ok(turns)
}

fn parse_speaker_line(fields: &[&str], origin: &str, number: usize) -> Result<Turn, EvalError> {
    let bad = |what: &str| EvalError::Invalid(format!("{origin}:{number}: {what}"));
    if fields.len() < 8 {
        return Err(bad("malformed SPEAKER line"));
    }
    let Ok(start) = fields[3].parse::<f64>() else {
        return Err(bad("the start is not a number"));
    };
    let Ok(duration) = fields[4].parse::<f64>() else {
        return Err(bad("the duration is not a number"));
    };
    if duration < 0.0 {
        return Err(bad("negative duration"));
    }
    Ok(Turn::new(fields[7], start, start + duration))
}

/// The turns that carry scorable speech time: those that end after they start.
pub fn scorable(turns: Vec<Turn>) -> Vec<Turn> {
    turns
        .into_iter()
        .filter(|turn| turn.end > turn.start)
        .collect()
}

fn total_length(turns: &[Turn]) -> f64 {
    turns.iter().map(|turn| turn.end - turn.start).sum()
}

fn overlap(a: &Turn, b: &Turn) -> f64 {
    (a.end.min(b.end) - a.start.max(b.start)).max(0.0)
}

/// The intervals `collar` seconds either side of each reference boundary, sorted and merged.
fn exclusions(reference: &[Turn], collar: f64) -> Vec<(f64, f64)> {
    if collar == 0.0 {
        return Vec::new();
    }
    let mut intervals: Vec<(f64, f64)> = reference
        .iter()
        .flat_map(|turn| {
            [
                (turn.start - collar, turn.start + collar),
                (turn.end - collar, turn.end + collar),
            ]
        })
        .collect();
    intervals.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for (start, end) in intervals {
        match merged.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    merged
}

/// The parts of one turn that lie outside every exclusion.
fn cut_turn(turn: &Turn, exclusions: &[(f64, f64)]) -> Vec<Turn> {
    let mut parts = Vec::new();
    let mut now = turn.start;
    for &(from, to) in exclusions {
        if to <= now {
            continue;
        }
        if from >= turn.end {
            break;
        }
        if from > now {
            parts.push(Turn::new(&turn.speaker, now, from));
        }
        now = now.max(to);
        if now >= turn.end {
            break;
        }
    }
    if now < turn.end {
        parts.push(Turn::new(&turn.speaker, now, turn.end));
    }
    parts
}

fn without(turns: &[Turn], exclusions: &[(f64, f64)]) -> Vec<Turn> {
    if exclusions.is_empty() {
        return turns.to_vec();
    }
    turns
        .iter()
        .flat_map(|turn| cut_turn(turn, exclusions))
        .collect()
}

/// The stretches between consecutive turn boundaries of either side, in which no turn starts or ends.
fn stretches(reference: &[Turn], hypothesis: &[Turn]) -> Vec<(f64, f64)> {
    let mut bounds: Vec<f64> = reference
        .iter()
        .chain(hypothesis)
        .flat_map(|turn| [turn.start, turn.end])
        .collect();
    bounds.sort_by(f64::total_cmp);
    bounds.dedup();
    bounds.windows(2).map(|pair| (pair[0], pair[1])).collect()
}

/// The turns that cover the moment `at`.
fn active_at(turns: &[Turn], at: f64) -> impl Iterator<Item = &Turn> {
    turns
        .iter()
        .filter(move |turn| turn.start <= at && at <= turn.end)
}

/// The integral of the larger of the two speaker counts.
fn load_length(reference: &[Turn], hypothesis: &[Turn]) -> f64 {
    stretches(reference, hypothesis)
        .into_iter()
        .map(|(from, to)| {
            let middle = (from + to) / 2.0;
            let speakers = active_at(reference, middle)
                .count()
                .max(active_at(hypothesis, middle).count());
            (to - from) * speakers as f64
        })
        .sum()
}

/// Each speaker name's row or column number, in sorted order of the names.
fn speaker_index(turns: &[Turn]) -> BTreeMap<&str, usize> {
    let mut names: Vec<&str> = turns.iter().map(|turn| turn.speaker.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    names
        .into_iter()
        .enumerate()
        .map(|(index, name)| (name, index))
        .collect()
}

/// The best one-to-one pairing of reference and hypothesis speakers, as
/// `(reference name, hypothesis name)`, and the time the paired speakers share.
struct Pairing<'a> {
    pairs: Vec<(&'a str, &'a str)>,
    shared: f64,
}

fn best_pairing<'a>(reference: &'a [Turn], hypothesis: &'a [Turn]) -> Pairing<'a> {
    let rows = speaker_index(reference);
    let cols = speaker_index(hypothesis);
    let mut cost = vec![0.0_f64; rows.len() * cols.len()];
    for r in reference {
        for h in hypothesis {
            cost[rows[r.speaker.as_str()] * cols.len() + cols[h.speaker.as_str()]] -= overlap(r, h);
        }
    }
    let matched = hungarian::minimize(&cost, rows.len(), cols.len());
    let shared = matched
        .iter()
        .map(|&(row, col)| -cost[row * cols.len() + col])
        .sum();
    // The index of a name is its place in the sorted names, which is the order a map iterates in.
    let row_names: Vec<&str> = rows.keys().copied().collect();
    let col_names: Vec<&str> = cols.keys().copied().collect();
    let pairs = matched
        .iter()
        .map(|&(row, col)| (row_names[row], col_names[col]))
        .collect();
    Pairing { pairs, shared }
}

/// Both sides with the collar cut out, and the reference speech time that remains.
struct Scored {
    reference: Vec<Turn>,
    hypothesis: Vec<Turn>,
    total: f64,
}

fn scored(reference: &[Turn], hypothesis: &[Turn], collar: f64) -> Scored {
    let cut = if collar > 0.0 {
        exclusions(reference, collar)
    } else {
        Vec::new()
    };
    let reference = without(reference, &cut);
    let hypothesis = without(hypothesis, &cut);
    let total = total_length(&reference);
    Scored {
        reference,
        hypothesis,
        total,
    }
}

/// The diarization error rate of `hypothesis` against `reference`, as a fraction of the reference speech time.
/// Both are expected to be [`scorable`]. A reference with no speech time scores 0.
pub fn der(reference: &[Turn], hypothesis: &[Turn], collar: f64) -> f64 {
    let Scored {
        reference,
        hypothesis,
        total,
    } = scored(reference, hypothesis, collar);
    if total == 0.0 {
        return 0.0;
    }
    let load = load_length(&reference, &hypothesis);
    (load - best_pairing(&reference, &hypothesis).shared) / total
}

/// What the error rate is made of, each part as a fraction of the reference
/// speech time. The three parts add up to [`der`].
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Breakdown {
    /// Reference speech that has no hypothesis speaker to answer it.
    pub miss: f64,
    /// Hypothesis speech that has no reference speaker to answer.
    pub false_alarm: f64,
    /// Speech that both sides cover, answered by a speaker other than the paired one.
    pub confusion: f64,
}

/// How many turns of each side cover one stretch, and how many pairs of a
/// reference turn and a hypothesis turn of the paired speakers do.
struct Counts {
    reference: usize,
    hypothesis: usize,
    answered: usize,
}

impl Counts {
    fn in_stretch(
        (from, to): (f64, f64),
        reference: &[Turn],
        hypothesis: &[Turn],
        pairs: &[(&str, &str)],
    ) -> Counts {
        let middle = (from + to) / 2.0;
        let answered = active_at(reference, middle)
            .map(|r| {
                active_at(hypothesis, middle)
                    .filter(|h| pairs.contains(&(r.speaker.as_str(), h.speaker.as_str())))
                    .count()
            })
            .sum();
        Counts {
            reference: active_at(reference, middle).count(),
            hypothesis: active_at(hypothesis, middle).count(),
            answered,
        }
    }
}

impl Breakdown {
    /// Adds the error of one stretch of `length` seconds, as the md-eval of NIST SCTK counts it.
    fn add(&mut self, length: f64, counts: &Counts) {
        let (r, h) = (counts.reference, counts.hypothesis);
        self.miss += length * r.saturating_sub(h) as f64;
        self.false_alarm += length * h.saturating_sub(r) as f64;
        self.confusion += length * (r.min(h) as f64 - counts.answered as f64);
    }
}

/// [`der`] split into miss, false alarm and confusion, over the same time and with the same pairing.
/// The speakers are paired once for the whole file, so a speaker that is
/// paired wrongly on one stretch is a confusion there and not a miss.
pub fn breakdown(reference: &[Turn], hypothesis: &[Turn], collar: f64) -> Breakdown {
    let Scored {
        reference,
        hypothesis,
        total,
    } = scored(reference, hypothesis, collar);
    if total == 0.0 {
        return Breakdown::default();
    }
    let pairing = best_pairing(&reference, &hypothesis);
    let mut sums = Breakdown::default();
    for stretch in stretches(&reference, &hypothesis) {
        let counts = Counts::in_stretch(stretch, &reference, &hypothesis, &pairing.pairs);
        sums.add(stretch.1 - stretch.0, &counts);
    }
    Breakdown {
        miss: sums.miss / total,
        false_alarm: sums.false_alarm / total,
        confusion: sums.confusion / total,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hayamimi_asr::refine::{Span, group_ranges};

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn half_the_speech_given_to_the_wrong_speaker_is_half_the_error() {
        let reference = [Turn::new("A", 0.0, 10.0)];
        let hypothesis = [Turn::new("A", 0.0, 5.0), Turn::new("B", 5.0, 10.0)];
        assert!(close(der(&reference, &hypothesis, 0.0), 0.5));
    }

    #[test]
    fn identical_segments_score_zero() {
        let turns = [Turn::new("A", 0.0, 4.0), Turn::new("B", 4.0, 9.0)];
        assert!(close(der(&turns, &turns, 0.0), 0.0));
    }

    #[test]
    fn names_do_not_matter_only_the_pairing() {
        let reference = [Turn::new("A", 0.0, 4.0), Turn::new("B", 4.0, 9.0)];
        let hypothesis = [Turn::new("S2", 0.0, 4.0), Turn::new("S1", 4.0, 9.0)];
        assert!(close(der(&reference, &hypothesis, 0.0), 0.0));
    }

    #[test]
    fn missed_speech_counts_against_the_reference_time() {
        let reference = [Turn::new("A", 0.0, 10.0)];
        let hypothesis = [Turn::new("A", 0.0, 6.0)];
        assert!(close(der(&reference, &hypothesis, 0.0), 0.4));
    }

    #[test]
    fn false_alarm_counts_against_the_reference_time() {
        let reference = [Turn::new("A", 0.0, 10.0)];
        let hypothesis = [Turn::new("A", 0.0, 10.0), Turn::new("A", 12.0, 14.0)];
        assert!(close(der(&reference, &hypothesis, 0.0), 0.2));
    }

    #[test]
    fn an_empty_reference_scores_zero() {
        assert!(close(der(&[], &[Turn::new("A", 0.0, 1.0)], 0.0), 0.0));
    }

    #[test]
    fn an_empty_hypothesis_misses_everything() {
        assert!(close(der(&[Turn::new("A", 0.0, 3.0)], &[], 0.0), 1.0));
    }

    #[test]
    fn a_collar_forgives_a_late_turn_boundary() {
        let reference = [Turn::new("A", 0.0, 10.0), Turn::new("B", 10.0, 20.0)];
        let hypothesis = [Turn::new("A", 0.0, 10.2), Turn::new("B", 10.2, 20.0)];
        assert!(der(&reference, &hypothesis, 0.0) > 0.0);
        assert!(close(der(&reference, &hypothesis, 0.25), 0.0));
    }

    #[test]
    fn an_exclusion_cuts_a_turn_in_the_middle() {
        let turn = Turn::new("A", 0.0, 10.0);
        let parts = cut_turn(&turn, &[(4.0, 6.0)]);
        assert_eq!(parts, [Turn::new("A", 0.0, 4.0), Turn::new("A", 6.0, 10.0)]);
    }

    #[test]
    fn overlapping_exclusions_are_merged() {
        let reference = [Turn::new("A", 0.0, 1.0)];
        assert_eq!(exclusions(&reference, 0.75), [(-0.75, 1.75)]);
        assert!(exclusions(&reference, 0.0).is_empty());
    }

    #[test]
    fn overlapped_speech_raises_the_load() {
        let reference = [Turn::new("A", 0.0, 10.0), Turn::new("B", 5.0, 10.0)];
        let hypothesis = [Turn::new("A", 0.0, 10.0)];
        assert!(close(load_length(&reference, &hypothesis), 15.0));
        assert!(close(der(&reference, &hypothesis, 0.0), 5.0 / 15.0));
    }

    #[test]
    fn rttm_speaker_lines_become_turns_and_other_lines_are_skipped() -> Result<(), EvalError> {
        let text = "SPKR-INFO f 1 <NA> <NA> <NA> unknown A <NA> <NA>\n\nSPEAKER f 1 1.5 2.0 <NA> <NA> A <NA> <NA>\n";
        assert_eq!(parse_rttm(text, "f")?, [Turn::new("A", 1.5, 3.5)]);
        Ok(())
    }

    #[test]
    fn a_short_speaker_line_names_its_place() {
        let said = parse_rttm("SPEAKER f 1 0 1\n", "m.rttm")
            .err()
            .map(|error| error.to_string());
        assert!(
            said.as_deref()
                .is_some_and(|text| text.contains("m.rttm:1")),
            "{said:?}"
        );
    }

    #[test]
    fn a_negative_duration_is_refused() {
        let text = "SPEAKER f 1 5.0 -1.0 <NA> <NA> A <NA> <NA>\n";
        assert!(parse_rttm(text, "m.rttm").is_err());
    }

    #[test]
    fn turns_that_do_not_end_after_they_start_are_not_scorable() {
        let kept = scorable(vec![
            Turn::new("A", 1.0, 1.0),
            Turn::new("A", 1.0, 2.0),
            Turn::new("A", 3.0, 2.0),
        ]);
        assert_eq!(kept, [Turn::new("A", 1.0, 2.0)]);
    }

    /// The text of an RTTM `SPEAKER` line for `speaker` from `start` for `duration`.
    fn speaker_line(meeting: &str, start: &str, duration: &str, speaker: &str) -> String {
        format!("SPEAKER {meeting} 1 {start} {duration} <NA> <NA> {speaker} <NA> <NA>\n")
    }

    /// Whether `result` is an `Invalid` error whose text holds `part`.
    fn refused_with(result: Result<Vec<Turn>, EvalError>, part: &str) -> bool {
        matches!(result, Err(EvalError::Invalid(ref said)) if said.contains(part))
    }

    #[test]
    fn parse_rttm_basic() -> Result<(), EvalError> {
        let text = speaker_line("meet1", "34.27", "10.12", "FEE041")
            + &speaker_line("meet1", "46.43", "10.42", "FEE044");
        assert_eq!(
            parse_rttm(&text, "x.rttm")?,
            [
                Turn::new("FEE041", 34.27, 34.27 + 10.12),
                Turn::new("FEE044", 46.43, 46.43 + 10.42),
            ]
        );
        Ok(())
    }

    #[test]
    fn parse_rttm_ignores_non_speaker_lines_and_blanks() -> Result<(), EvalError> {
        let text = format!(
            "\nSPKR-INFO meet1 1 <NA> <NA> <NA> unknown FEE041 <NA> <NA>\n{}\n",
            speaker_line("meet1", "1.0", "2.0", "S1")
        );
        assert_eq!(parse_rttm(&text, "x.rttm")?, [Turn::new("S1", 1.0, 3.0)]);
        Ok(())
    }

    #[test]
    fn parse_rttm_empty_file() -> Result<(), EvalError> {
        assert!(parse_rttm("", "x.rttm")?.is_empty());
        Ok(())
    }

    #[test]
    fn parse_rttm_rejects_malformed_line() {
        // The speaker field and everything after the duration are missing.
        let result = parse_rttm("SPEAKER meet1 1 1.0 2.0\n", "x.rttm");
        assert!(refused_with(result, "x.rttm:1: malformed SPEAKER line"));
    }

    #[test]
    fn parse_rttm_rejects_negative_duration() {
        let text = speaker_line("meet1", "1.0", "-2.0", "S1");
        assert!(refused_with(
            parse_rttm(&text, "x.rttm"),
            "x.rttm:1: negative duration"
        ));
    }

    #[test]
    fn parse_rttm_round_trips_zero_duration() -> Result<(), EvalError> {
        // A zero-length turn is legal RTTM: the parser keeps it and `scorable` drops it.
        let text = speaker_line("meet1", "5.0", "0.0", "S1");
        let turns = parse_rttm(&text, "x.rttm")?;
        assert_eq!(turns, [Turn::new("S1", 5.0, 5.0)]);
        assert!(scorable(turns).is_empty());
        Ok(())
    }

    #[test]
    fn segments_to_der_tuples_coerces_types() {
        // The diarizer reports f32 seconds; scoring is in f64, and widening is exact.
        let narrow = Turn::new("S1", f64::from(1.0_f32), f64::from(2.5_f32));
        assert_eq!(scorable(vec![narrow]), [Turn::new("S1", 1.0, 2.5)]);
    }

    #[test]
    fn segments_to_der_tuples_drops_zero_and_negative_length() {
        let kept = scorable(vec![
            Turn::new("S1", 1.0, 1.0),
            Turn::new("S1", 2.0, 1.5),
            Turn::new("S1", 1.0, 2.0),
        ]);
        assert_eq!(kept, [Turn::new("S1", 1.0, 2.0)]);
    }

    #[test]
    fn segments_to_der_tuples_empty() {
        assert!(scorable(Vec::new()).is_empty());
    }

    #[test]
    fn der_zero_for_identical_ref_and_hyp() {
        let reference = [Turn::new("A", 0.0, 5.0), Turn::new("B", 5.0, 10.0)];
        assert!(close(der(&reference, &reference, 0.0), 0.0));
    }

    #[test]
    fn der_nonzero_for_swapped_speaker_labels_within_overlap() {
        // A global relabeling scores 0: the error depends on the partition, not on the names.
        let reference = [Turn::new("A", 0.0, 5.0), Turn::new("B", 5.0, 10.0)];
        let swapped = [Turn::new("B", 0.0, 5.0), Turn::new("A", 5.0, 10.0)];
        assert!(close(der(&reference, &swapped, 0.0), 0.0));
        // A real confusion on a sub-interval is not free.
        let whole = [Turn::new("A", 0.0, 10.0)];
        let confused = [Turn::new("A", 0.0, 5.0), Turn::new("B", 5.0, 10.0)];
        assert!(der(&whole, &confused, 0.0) > 0.0);
    }

    #[test]
    fn der_end_to_end_from_parsed_rttm() -> Result<(), EvalError> {
        let text =
            speaker_line("meet", "0.0", "5.0", "A") + &speaker_line("meet", "5.0", "5.0", "B");
        let reference = scorable(parse_rttm(&text, "ref.rttm")?);
        assert_eq!(reference.len(), 2);
        assert!(close(der(&reference, &reference, 0.25), 0.0));
        Ok(())
    }

    const SR: usize = 16_000;

    fn span(start: usize, end: usize) -> Span {
        Span {
            start,
            end,
            label: String::from("S1"),
        }
    }

    #[test]
    fn group_segments_single_segment() {
        let one_group = 0..1;
        assert_eq!(group_ranges(&[span(0, SR)]), [one_group]);
    }

    #[test]
    fn group_segments_merges_close_segments() {
        // Two 1 s segments 0.5 s apart: the gap is under 2 s, so one group.
        let spans = [span(0, SR), span(SR * 3 / 2, SR * 5 / 2)];
        let one_group = 0..2;
        assert_eq!(group_ranges(&spans), [one_group]);
    }

    #[test]
    fn group_segments_splits_on_silence_gap() {
        // A 3 s gap is over the 2 s limit, so two groups.
        let spans = [span(0, SR), span(4 * SR, 5 * SR)];
        assert_eq!(group_ranges(&spans), [0..1, 1..2]);
    }

    #[test]
    fn group_segments_splits_on_max_length() {
        // A group already 26 s long closes before the next segment, with no gap between them.
        let spans = [span(0, 26 * SR), span(26 * SR, 27 * SR)];
        assert_eq!(group_ranges(&spans), [0..1, 1..2]);
    }

    #[test]
    fn group_segments_empty_input() {
        assert!(group_ranges(&[]).is_empty());
    }

    // ---- the breakdown of the error (RP-57) ----

    /// How close two fractions must be to count as equal.
    const TOLERANCE: f64 = 1e-9;

    fn within(found: f64, wanted: f64) -> bool {
        (found - wanted).abs() <= TOLERANCE
    }

    /// A reference, a hypothesis and the collar to score them with.
    struct Case {
        reference: Vec<Turn>,
        hypothesis: Vec<Turn>,
        collar: f64,
    }

    type Spec<'a> = &'a [(&'a str, f64, f64)];

    fn case(reference: Spec, hypothesis: Spec, collar: f64) -> Case {
        let turns = |spec: Spec| {
            spec.iter()
                .map(|&(speaker, start, end)| Turn::new(speaker, start, end))
                .collect()
        };
        Case {
            reference: turns(reference),
            hypothesis: turns(hypothesis),
            collar,
        }
    }

    fn breakdown_of(case: &Case) -> Breakdown {
        breakdown(&case.reference, &case.hypothesis, case.collar)
    }

    fn der_of(case: &Case) -> f64 {
        der(&case.reference, &case.hypothesis, case.collar)
    }

    fn sum_of(parts: &Breakdown) -> f64 {
        parts.miss + parts.false_alarm + parts.confusion
    }

    /// Whether the three parts are the wanted ones; the error names the first part that is not.
    fn expect_parts(
        found: &Breakdown,
        miss: f64,
        false_alarm: f64,
        confusion: f64,
    ) -> Result<(), String> {
        let parts = [
            ("miss", found.miss, miss),
            ("false alarm", found.false_alarm, false_alarm),
            ("confusion", found.confusion, confusion),
        ];
        for (name, got, wanted) in parts {
            if !within(got, wanted) {
                return Err(format!("{name} is {got}, expected {wanted}"));
            }
        }
        Ok(())
    }

    fn expect_breakdown(
        case: &Case,
        miss: f64,
        false_alarm: f64,
        confusion: f64,
    ) -> Result<(), String> {
        expect_parts(&breakdown_of(case), miss, false_alarm, confusion)
    }

    /// Whether the three parts add up to `der_value`.
    fn check_breakdown_sum(der_value: f64, parts: &Breakdown) -> Result<(), String> {
        if within(sum_of(parts), der_value) {
            return Ok(());
        }
        Err(format!(
            "miss + false alarm + confusion is {}, der is {der_value}",
            sum_of(parts)
        ))
    }

    /// The one part that carries the error in an isolation check.
    enum Isolated {
        Confusion,
        Miss,
    }

    /// Whether `value` is all in the part `only` names and the other two are 0.
    fn check_isolation(parts: &Breakdown, only: Isolated, value: f64) -> Result<(), String> {
        match only {
            Isolated::Confusion => expect_parts(parts, 0.0, 0.0, value),
            Isolated::Miss => expect_parts(parts, value, 0.0, 0.0),
        }
    }

    fn identical() -> Case {
        let both = [("A", 0.0, 5.0), ("B", 5.0, 10.0)];
        case(&both, &both, 0.0)
    }

    fn swapped_names() -> Case {
        let reference = [("A", 0.0, 5.0), ("B", 5.0, 10.0)];
        let hypothesis = [("S2", 0.0, 5.0), ("S1", 5.0, 10.0)];
        case(&reference, &hypothesis, 0.0)
    }

    fn half_confused() -> Case {
        let hypothesis = [("A", 0.0, 5.0), ("B", 5.0, 10.0)];
        case(&[("A", 0.0, 10.0)], &hypothesis, 0.0)
    }

    fn half_dropped() -> Case {
        case(&[("A", 0.0, 10.0)], &[("A", 0.0, 5.0)], 0.0)
    }

    fn late_change() -> Case {
        let reference = [("A", 0.0, 4.0), ("B", 4.0, 10.0)];
        let hypothesis = [("X", 0.0, 6.0), ("Y", 6.0, 10.0)];
        case(&reference, &hypothesis, 0.0)
    }

    fn dropped_speaker() -> Case {
        let reference = [("A", 0.0, 5.0), ("B", 5.0, 10.0)];
        case(&reference, &[("A", 0.0, 5.0)], 0.0)
    }

    fn outside_reference() -> Case {
        let hypothesis = [("A", 0.0, 10.0), ("A", 12.0, 14.0)];
        case(&[("A", 0.0, 10.0)], &hypothesis, 0.0)
    }

    fn extra_speaker() -> Case {
        let hypothesis = [("A", 0.0, 10.0), ("B", 0.0, 5.0)];
        case(&[("A", 0.0, 10.0)], &hypothesis, 0.0)
    }

    fn unanswered_overlap() -> Case {
        let reference = [("A", 0.0, 10.0), ("B", 5.0, 10.0)];
        case(&reference, &[("A", 0.0, 10.0)], 0.0)
    }

    fn merged_speakers() -> Case {
        let reference = [("A", 0.0, 5.0), ("B", 5.0, 10.0)];
        case(&reference, &[("X", 0.0, 10.0)], 0.0)
    }

    fn all_three() -> Case {
        let reference = [("A", 0.0, 10.0), ("B", 10.0, 20.0)];
        let hypothesis = [("X", 0.0, 8.0), ("Y", 8.0, 14.0), ("X", 16.0, 24.0)];
        case(&reference, &hypothesis, 0.0)
    }

    fn collar_shift() -> Case {
        let reference = [("A", 0.0, 10.0), ("B", 10.0, 20.0)];
        let hypothesis = [("A", 0.0, 10.5), ("B", 10.5, 20.0)];
        case(&reference, &hypothesis, 0.25)
    }

    fn empty_reference() -> Case {
        case(&[], &[("A", 0.0, 1.0)], 0.0)
    }

    fn empty_hypothesis() -> Case {
        case(&[("A", 0.0, 3.0)], &[], 0.0)
    }

    fn collar_covers_reference() -> Case {
        case(&[("A", 0.0, 0.4)], &[("B", 0.0, 0.4)], 0.25)
    }

    /// Every input above, each with the name of the function that makes it.
    fn all_cases() -> Vec<(&'static str, Case)> {
        vec![
            ("identical", identical()),
            ("swapped_names", swapped_names()),
            ("half_confused", half_confused()),
            ("half_dropped", half_dropped()),
            ("late_change", late_change()),
            ("dropped_speaker", dropped_speaker()),
            ("outside_reference", outside_reference()),
            ("extra_speaker", extra_speaker()),
            ("unanswered_overlap", unanswered_overlap()),
            ("merged_speakers", merged_speakers()),
            ("all_three", all_three()),
            ("collar_shift", collar_shift()),
            ("empty_reference", empty_reference()),
            ("empty_hypothesis", empty_hypothesis()),
            ("collar_covers_reference", collar_covers_reference()),
        ]
    }

    #[test]
    fn der_breakdown_zero_for_identical_ref_and_hyp() -> Result<(), String> {
        expect_breakdown(&identical(), 0.0, 0.0, 0.0)
    }

    #[test]
    fn der_breakdown_ignores_speaker_names_when_pairing() -> Result<(), String> {
        expect_breakdown(&swapped_names(), 0.0, 0.0, 0.0)
    }

    #[test]
    fn der_breakdown_isolates_confusion_from_miss() -> Result<(), String> {
        let confused = half_confused();
        let parts = breakdown_of(&confused);
        check_isolation(&parts, Isolated::Confusion, 0.5)?;
        check_breakdown_sum(der_of(&confused), &parts)?;
        assert!(within(der_of(&confused), 0.5), "{}", der_of(&confused));
        let missed = half_dropped();
        let parts = breakdown_of(&missed);
        check_isolation(&parts, Isolated::Miss, 0.5)?;
        check_breakdown_sum(der_of(&missed), &parts)?;
        assert!(within(der_of(&missed), 0.5), "{}", der_of(&missed));
        Ok(())
    }

    #[test]
    fn der_breakdown_counts_a_late_speaker_change_as_confusion() -> Result<(), String> {
        expect_breakdown(&late_change(), 0.0, 0.0, 0.2)
    }

    #[test]
    fn der_breakdown_counts_a_dropped_speaker_as_miss() -> Result<(), String> {
        expect_breakdown(&dropped_speaker(), 0.5, 0.0, 0.0)
    }

    #[test]
    fn der_breakdown_counts_speech_outside_the_reference_as_false_alarm() -> Result<(), String> {
        expect_breakdown(&outside_reference(), 0.0, 0.2, 0.0)
    }

    #[test]
    fn der_breakdown_counts_an_extra_hypothesis_speaker_as_false_alarm() -> Result<(), String> {
        expect_breakdown(&extra_speaker(), 0.0, 0.5, 0.0)
    }

    #[test]
    fn der_breakdown_counts_unanswered_overlap_as_miss() -> Result<(), String> {
        expect_breakdown(&unanswered_overlap(), 1.0 / 3.0, 0.0, 0.0)
    }

    #[test]
    fn der_breakdown_counts_merged_speakers_as_confusion() -> Result<(), String> {
        expect_breakdown(&merged_speakers(), 0.0, 0.0, 0.5)
    }

    #[test]
    fn der_breakdown_separates_miss_false_alarm_and_confusion_in_one_input() -> Result<(), String> {
        let mixed = all_three();
        expect_breakdown(&mixed, 0.1, 0.2, 0.3)?;
        check_breakdown_sum(0.6, &breakdown_of(&mixed))
    }

    #[test]
    fn der_breakdown_scores_only_outside_the_collar() -> Result<(), String> {
        expect_breakdown(&collar_shift(), 0.0, 0.0, 0.25 / 19.0)
    }

    #[test]
    fn der_breakdown_of_an_empty_reference_is_zero_not_nan() -> Result<(), String> {
        expect_breakdown(&empty_reference(), 0.0, 0.0, 0.0)
    }

    #[test]
    fn der_breakdown_of_an_empty_hypothesis_is_all_miss() -> Result<(), String> {
        expect_breakdown(&empty_hypothesis(), 1.0, 0.0, 0.0)
    }

    #[test]
    fn der_breakdown_is_zero_when_the_collar_covers_all_reference_time() -> Result<(), String> {
        expect_breakdown(&collar_covers_reference(), 0.0, 0.0, 0.0)
    }

    #[test]
    fn der_breakdown_sums_to_der_on_every_fixture() -> Result<(), String> {
        let cases = all_cases();
        assert_eq!(cases.len(), 15);
        for (name, fixture) in cases {
            check_breakdown_sum(der_of(&fixture), &breakdown_of(&fixture))
                .map_err(|said| format!("{name}: {said}"))?;
        }
        Ok(())
    }

    /// The parts of the error 0.6, with `extra` added to the confusion.
    fn parts_off_by(extra: f64) -> Breakdown {
        Breakdown {
            miss: 0.1,
            false_alarm: 0.2,
            confusion: 0.3 + extra,
        }
    }

    #[test]
    fn der_breakdown_sum_check_accepts_a_difference_just_inside_1e_9() -> Result<(), String> {
        check_breakdown_sum(0.6, &parts_off_by(0.99e-9))
    }

    #[test]
    fn der_breakdown_sum_check_rejects_a_difference_just_outside_1e_9() {
        assert!(check_breakdown_sum(0.6, &parts_off_by(1.01e-9)).is_err());
    }

    #[test]
    fn der_breakdown_sum_check_rejects_a_dropped_confusion() {
        assert!(check_breakdown_sum(0.5, &Breakdown::default()).is_err());
    }

    #[test]
    fn der_breakdown_isolation_check_rejects_confusion_folded_into_miss() -> Result<(), String> {
        let folded = Breakdown {
            miss: 0.5,
            false_alarm: 0.0,
            confusion: 0.0,
        };
        // The sum still matches der, so only the isolation check can tell.
        check_breakdown_sum(0.5, &folded)?;
        let said = check_isolation(&folded, Isolated::Confusion, 0.5).err();
        assert!(
            said.as_deref()
                .is_some_and(|text| text.starts_with("miss is ")),
            "{said:?}"
        );
        Ok(())
    }
}

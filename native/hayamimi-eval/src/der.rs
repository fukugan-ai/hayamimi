//! The diarization error rate, scored the way `simpleder` scores it.
//!
//! The error is the load (the integral of the larger of the reference and the
//! hypothesis speaker count) minus the overlap of the best one-to-one pairing
//! of reference and hypothesis speakers, over the reference speech time. The
//! `collar` seconds around every reference turn boundary are left out of both.

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

/// The integral of the larger of the two speaker counts.
fn load_length(reference: &[Turn], hypothesis: &[Turn]) -> f64 {
    let mut bounds: Vec<f64> = reference
        .iter()
        .chain(hypothesis)
        .flat_map(|turn| [turn.start, turn.end])
        .collect();
    bounds.sort_by(f64::total_cmp);
    bounds.dedup();
    let count_at = |turns: &[Turn], at: f64| {
        turns
            .iter()
            .filter(|turn| turn.start <= at && at <= turn.end)
            .count()
    };
    bounds
        .windows(2)
        .map(|pair| {
            let middle = (pair[0] + pair[1]) / 2.0;
            let speakers = count_at(reference, middle).max(count_at(hypothesis, middle));
            (pair[1] - pair[0]) * speakers as f64
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

/// The overlap of the best one-to-one pairing of reference and hypothesis speakers.
fn best_pairing_overlap(reference: &[Turn], hypothesis: &[Turn]) -> f64 {
    let rows = speaker_index(reference);
    let cols = speaker_index(hypothesis);
    let mut cost = vec![0.0_f64; rows.len() * cols.len()];
    for r in reference {
        for h in hypothesis {
            cost[rows[r.speaker.as_str()] * cols.len() + cols[h.speaker.as_str()]] -= overlap(r, h);
        }
    }
    hungarian::minimize(&cost, rows.len(), cols.len())
        .into_iter()
        .map(|(row, col)| -cost[row * cols.len() + col])
        .sum()
}

/// The diarization error rate of `hypothesis` against `reference`, as a fraction of the reference speech time.
/// Both are expected to be [`scorable`]. A reference with no speech time scores 0.
pub fn der(reference: &[Turn], hypothesis: &[Turn], collar: f64) -> f64 {
    let cut = if collar > 0.0 {
        exclusions(reference, collar)
    } else {
        Vec::new()
    };
    let reference = without(reference, &cut);
    let hypothesis = without(hypothesis, &cut);
    let total = total_length(&reference);
    if total == 0.0 {
        return 0.0;
    }
    let load = load_length(&reference, &hypothesis);
    (load - best_pairing_overlap(&reference, &hypothesis)) / total
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
        assert_eq!(group_ranges(&[span(0, SR)]), [0..1]);
    }

    #[test]
    fn group_segments_merges_close_segments() {
        // Two 1 s segments 0.5 s apart: the gap is under 2 s, so one group.
        let spans = [span(0, SR), span(SR * 3 / 2, SR * 5 / 2)];
        assert_eq!(group_ranges(&spans), [0..2]);
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
}

//! One refine group: the utterances between two long silences are
//! re-diarized as a whole, and the local speakers the diarizer finds are
//! mapped onto the speakers of the session.
//!
//! A group with fewer than two local speakers keeps the speaker most of its
//! utterances were given on the fast path.

use std::collections::BTreeMap;
use std::ops::Range;

use crate::audio::SAMPLE_RATE;
use crate::diarize::{
    Cluster, GroupDiarizer, RemapOptions, SampleTurn, Turn, local_speakers, long_enough,
    remap_group, sample_turns, speaker_audio,
};
use crate::embedder::Embedder;
use crate::speaker::Centroids;

/// A silence of at least this many seconds closes a group.
pub const GROUP_GAP_SECONDS: f64 = 2.0;
/// A group that has run this many seconds is closed at the next utterance.
pub const GROUP_MAX_SECONDS: f64 = 25.0;

/// One utterance of the fast path: where it runs (sample indexes) and the speaker it was given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub label: String,
}

/// The ranges of `spans` that form groups, in order.
pub fn group_ranges(spans: &[Span]) -> Vec<Range<usize>> {
    let rate = f64::from(SAMPLE_RATE);
    let gap = (GROUP_GAP_SECONDS * rate) as usize;
    let longest = (GROUP_MAX_SECONDS * rate) as usize;
    let mut groups = Vec::new();
    let mut first = 0;
    for (index, span) in spans.iter().enumerate().skip(1) {
        let last_end = spans[index - 1].end;
        let silence = span.start.saturating_sub(last_end);
        let length = last_end.saturating_sub(spans[first].start);
        if silence >= gap || length >= longest {
            groups.push(first..index);
            first = index;
        }
    }
    if !spans.is_empty() {
        groups.push(first..spans.len());
    }
    groups
}

/// The speaker given to most of `spans`. On a tie in the number of
/// utterances, the one with more speech; on a tie in that too, the one met first.
pub fn majority_label(spans: &[Span]) -> Option<&str> {
    let mut best: Option<(&str, (usize, usize))> = None;
    for span in spans {
        let same = || spans.iter().filter(|other| other.label == span.label);
        let weight = (same().count(), same().map(|s| s.end - s.start).sum());
        if best.is_none_or(|(_, most)| weight > most) {
            best = Some((span.label.as_str(), weight));
        }
    }
    best.map(|(label, _)| label)
}

/// A speech turn with the session speaker it was given, in seconds from the start of the input.
#[derive(Debug, Clone, PartialEq)]
pub struct LabeledTurn {
    pub label: String,
    pub start: f64,
    pub end: f64,
}

/// One group to refine.
pub struct Group<'a> {
    /// The audio of the group, from the start of its first utterance to the end of its last.
    pub audio: &'a [f32],
    /// Index of the first sample of `audio` in the input.
    pub start: usize,
    /// The speaker most of the group's utterances were given on the fast path.
    pub majority: &'a str,
}

/// What a group came to.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupOutcome {
    pub turns: Vec<LabeledTurn>,
    /// The first failure of the diarizer or the embedder; the group then went on with what was left.
    pub error: Option<String>,
}

fn seconds_from(start: usize) -> f64 {
    start as f64 / f64::from(SAMPLE_RATE)
}

/// The whole group as one turn of its majority speaker.
fn whole_group(group: &Group) -> LabeledTurn {
    LabeledTurn {
        label: group.majority.to_owned(),
        start: seconds_from(group.start),
        end: seconds_from(group.start + group.audio.len()),
    }
}

/// Seconds of speech of local speaker `local` in `turns`.
fn cluster_seconds(turns: &[Turn], local: i32) -> f64 {
    turns
        .iter()
        .filter(|turn| turn.local == local)
        .map(|turn| f64::from(turn.end) - f64::from(turn.start))
        .sum()
}

/// The embedding of each local speaker's speech. A speaker with no audio, or
/// whose embedding failed, takes the majority speaker and is returned apart.
fn embed_clusters(
    embedder: &Embedder,
    group: &Group,
    turns: &[Turn],
    samples: &[SampleTurn],
    error: &mut Option<String>,
) -> (Vec<Cluster>, BTreeMap<i32, String>) {
    let mut clusters = Vec::new();
    let mut apart = BTreeMap::new();
    for local in local_speakers(samples) {
        let audio = speaker_audio(group.audio, samples, local);
        let embedded = if audio.is_empty() {
            None
        } else {
            match embedder.embed(&audio) {
                Ok(embedding) => Some(embedding),
                Err(failure) => {
                    error.get_or_insert_with(|| failure.to_string());
                    None
                }
            }
        };
        match embedded {
            Some(embedding) => clusters.push(Cluster {
                local,
                embedding,
                seconds: cluster_seconds(turns, local),
            }),
            None => {
                apart.insert(local, group.majority.to_owned());
            }
        }
    }
    (clusters, apart)
}

/// The turns of a group with the speaker of the session each belongs to.
pub fn refine_group(
    diarizer: &mut GroupDiarizer,
    embedder: &Embedder,
    centroids: &mut Centroids,
    group: &Group,
    options: RemapOptions,
) -> GroupOutcome {
    let (raw, mut error) = match diarizer.process(group.audio, None) {
        Ok(turns) => (turns, None),
        Err(failure) => (Vec::new(), Some(failure.to_string())),
    };
    let turns = long_enough(&raw);
    let samples = sample_turns(&turns, group.audio.len());
    if local_speakers(&samples).len() < 2 {
        return GroupOutcome {
            turns: vec![whole_group(group)],
            error,
        };
    }
    let (clusters, mut labels) = embed_clusters(embedder, group, &turns, &samples, &mut error);
    labels.extend(remap_group(centroids, &clusters, group.majority, options));
    let offset = seconds_from(group.start);
    let labeled = turns
        .iter()
        .map(|turn| LabeledTurn {
            label: match labels.get(&turn.local) {
                Some(label) => label.clone(),
                None => group.majority.to_owned(),
            },
            start: offset + f64::from(turn.start),
            end: offset + f64::from(turn.end),
        })
        .collect();
    GroupOutcome {
        turns: labeled,
        error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(start: usize, end: usize, label: &str) -> Span {
        Span {
            start,
            end,
            label: label.to_owned(),
        }
    }

    const SECOND: usize = 16_000;

    /// The groups as `(first, past the last)` pairs.
    fn bounds(spans: &[Span]) -> Vec<(usize, usize)> {
        group_ranges(spans)
            .into_iter()
            .map(|range| (range.start, range.end))
            .collect()
    }

    #[test]
    fn no_spans_make_no_groups() {
        assert!(group_ranges(&[]).is_empty());
    }

    #[test]
    fn one_span_is_one_group() {
        assert_eq!(bounds(&[span(0, SECOND, "S1")]), [(0, 1)]);
    }

    #[test]
    fn a_silence_of_two_seconds_closes_a_group_and_one_just_under_does_not() {
        let closed = [span(0, SECOND, "S1"), span(3 * SECOND, 4 * SECOND, "S1")];
        assert_eq!(bounds(&closed), [(0, 1), (1, 2)]);
        let kept = [
            span(0, SECOND, "S1"),
            span(3 * SECOND - 1, 4 * SECOND, "S1"),
        ];
        assert_eq!(bounds(&kept), [(0, 2)]);
    }

    #[test]
    fn a_group_that_has_run_twenty_five_seconds_closes_at_the_next_span() {
        let long = [
            span(0, 25 * SECOND, "S1"),
            span(25 * SECOND + 1, 26 * SECOND, "S2"),
        ];
        assert_eq!(bounds(&long), [(0, 1), (1, 2)]);
        let short = [
            span(0, 25 * SECOND - 1, "S1"),
            span(25 * SECOND, 26 * SECOND, "S2"),
        ];
        assert_eq!(bounds(&short), [(0, 2)]);
    }

    #[test]
    fn the_groups_cover_every_span_once() {
        let spans: Vec<Span> = (0..9)
            .map(|i| span(i * 5 * SECOND, i * 5 * SECOND + SECOND, "S1"))
            .collect();
        let covered: usize = group_ranges(&spans).iter().map(Range::len).sum();
        assert_eq!(covered, spans.len());
    }

    #[test]
    fn the_majority_is_the_most_frequent_label() {
        let spans = [span(0, 1, "S2"), span(1, 2, "S1"), span(2, 3, "S1")];
        assert_eq!(majority_label(&spans), Some("S1"));
    }

    #[test]
    fn a_tie_in_count_goes_to_the_label_with_more_speech() {
        let spans = [span(0, 1, "S2"), span(1, 5, "S1")];
        assert_eq!(majority_label(&spans), Some("S1"));
    }

    #[test]
    fn a_tie_in_count_and_speech_goes_to_the_label_met_first() {
        let spans = [span(0, 1, "S2"), span(1, 2, "S1")];
        assert_eq!(majority_label(&spans), Some("S2"));
    }

    #[test]
    fn more_utterances_win_over_more_speech() {
        let spans = [span(0, 1, "S1"), span(1, 2, "S1"), span(2, 100, "S2")];
        assert_eq!(majority_label(&spans), Some("S1"));
    }

    #[test]
    fn no_spans_have_no_majority() {
        assert_eq!(majority_label(&[]), None);
    }

    #[test]
    fn the_whole_group_is_one_turn_of_the_majority() {
        let audio = vec![0.0_f32; 2 * SECOND];
        let group = Group {
            audio: &audio,
            start: 3 * SECOND,
            majority: "S2",
        };
        assert_eq!(
            whole_group(&group),
            LabeledTurn {
                label: String::from("S2"),
                start: 3.0,
                end: 5.0
            }
        );
    }

    #[test]
    fn the_seconds_of_a_cluster_add_up_its_turns_only() {
        let turns = [
            Turn {
                local: 0,
                start: 0.0,
                end: 1.5,
            },
            Turn {
                local: 1,
                start: 1.5,
                end: 2.0,
            },
            Turn {
                local: 0,
                start: 4.0,
                end: 4.5,
            },
        ];
        assert!((cluster_seconds(&turns, 0) - 2.0).abs() < 1e-9);
        assert!(cluster_seconds(&turns, 9).abs() < 1e-12);
    }
}

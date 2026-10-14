//! A diarization hypothesis for one recording, made by the Rust route's own
//! stages, and its score against a reference.
//!
//! The recording goes through the voice detector, the fast speaker label of
//! each utterance, the grouping of utterances between long silences, and the
//! re-diarization of each group with its speakers mapped onto the session's.
//! No recognition is run: the speaker and the timing do not need it.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;

use hayamimi_asr::AsrError;
use hayamimi_asr::audio::{History, read_wav, windows};
use hayamimi_asr::diarize::{DiarizeSettings, GroupDiarizer, RemapOptions};
use hayamimi_asr::embedder::Embedder;
use hayamimi_asr::refine::{Group, Span, group_ranges, majority_label, refine_group};
use hayamimi_asr::speaker::{Centroids, Source};
use hayamimi_asr::vad::{Vad, VadSettings};

use crate::der::{self, Turn};
use crate::error::EvalError;

/// The voice detector's knobs for the hypothesis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Options {
    pub min_silence: f32,
    pub max_speech: f32,
}

/// The turns of one recording with what the run cost and found.
#[derive(Debug, Clone, PartialEq)]
pub struct Hypothesis {
    pub turns: Vec<Turn>,
    /// Seconds spent refining the groups: the diarizer, the embeddings and the mapping.
    pub refine_seconds: f64,
    pub groups: usize,
    pub audio_seconds: f64,
    /// How many speakers each path opened, by the path's name.
    pub opened_by: BTreeMap<&'static str, usize>,
    /// Groups where the diarizer or the embedder failed and the group went on with what was left.
    pub failed_groups: usize,
}

/// One meeting scored.
#[derive(Debug, Clone, PartialEq)]
pub struct MeetingScore {
    pub der: f64,
    pub reference_speakers: usize,
    pub hypothesis_speakers: usize,
    pub reference_turns: usize,
    pub hypothesis_segments: usize,
    /// Seconds from the start of the run to its end.
    pub run_seconds: f64,
    pub hypothesis: Hypothesis,
}

fn asr(error: AsrError) -> EvalError {
    EvalError::Invalid(error.to_string())
}

/// Labels the utterances the detector has finished, on the fast path.
fn label_segments(
    vad: &Vad,
    history: &mut History,
    embedder: &Embedder,
    centroids: &mut Centroids,
    spans: &mut Vec<Span>,
) -> Result<(), EvalError> {
    for segment in vad.take_segments().map_err(asr)? {
        let heard = history.with_preroll(segment.start, &segment.samples);
        let embedding = embedder.embed(&heard).map_err(asr)?;
        spans.push(Span {
            start: segment.start,
            end: segment.start + segment.samples.len(),
            label: centroids.label(&embedding, Source::Fast),
        });
    }
    Ok(())
}

/// Every utterance of the recording with the speaker the fast path gave it.
fn fast_spans(
    models: &Path,
    samples: &[f32],
    options: &Options,
    embedder: &Embedder,
    centroids: &mut Centroids,
) -> Result<Vec<Span>, EvalError> {
    let settings = VadSettings {
        min_silence: options.min_silence,
        max_speech: options.max_speech,
    };
    let mut vad = Vad::open(models, settings).map_err(asr)?;
    let mut history = History::new();
    let mut spans = Vec::new();
    for window in windows(samples) {
        history.push(&window);
        vad.accept(&window);
        label_segments(&vad, &mut history, embedder, centroids, &mut spans)?;
    }
    vad.flush();
    label_segments(&vad, &mut history, embedder, centroids, &mut spans)?;
    Ok(spans)
}

/// The turns of every group, refined one after the other.
fn refine_all(
    samples: &[f32],
    spans: &[Span],
    diarizer: &mut GroupDiarizer,
    embedder: &Embedder,
    centroids: &mut Centroids,
) -> (Vec<Turn>, usize) {
    let mut turns = Vec::new();
    let mut failed = 0;
    for range in group_ranges(spans) {
        let members = &spans[range];
        let (Some(first), Some(last), Some(majority)) =
            (members.first(), members.last(), majority_label(members))
        else {
            continue;
        };
        let end = last.end.min(samples.len());
        let start = first.start.min(end);
        let group = Group {
            audio: &samples[start..end],
            start,
            majority,
        };
        let outcome = refine_group(
            diarizer,
            embedder,
            centroids,
            &group,
            RemapOptions::default(),
        );
        failed += usize::from(outcome.error.is_some());
        turns.extend(
            outcome
                .turns
                .into_iter()
                .map(|turn| Turn::new(&turn.label, turn.start, turn.end)),
        );
    }
    (turns, failed)
}

/// The hypothesis for 16 kHz `samples`, with the models under `models`.
pub fn hypothesis(
    models: &Path,
    samples: &[f32],
    options: &Options,
) -> Result<Hypothesis, EvalError> {
    hayamimi_asr::check_library().map_err(asr)?;
    let embedder = Embedder::open(models).map_err(asr)?;
    let mut diarizer = GroupDiarizer::open(models, DiarizeSettings::default()).map_err(asr)?;
    let mut centroids = Centroids::default();
    let spans = fast_spans(models, samples, options, &embedder, &mut centroids)?;
    let started = Instant::now();
    let (turns, failed_groups) =
        refine_all(samples, &spans, &mut diarizer, &embedder, &mut centroids);
    Ok(Hypothesis {
        turns,
        refine_seconds: started.elapsed().as_secs_f64(),
        groups: group_ranges(&spans).len(),
        audio_seconds: samples.len() as f64 / f64::from(hayamimi_asr::audio::SAMPLE_RATE),
        opened_by: centroids.centroid_open_counts(),
        failed_groups,
    })
}

fn distinct_speakers(turns: &[Turn]) -> usize {
    let mut names: Vec<&str> = turns.iter().map(|turn| turn.speaker.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    names.len()
}

/// Runs the hypothesis for `wav` and scores it against `rttm` with `collar` seconds forgiven.
pub fn score_meeting(
    models: &Path,
    wav: &Path,
    rttm: &Path,
    collar: f64,
    options: &Options,
) -> Result<MeetingScore, EvalError> {
    let text = std::fs::read_to_string(rttm).map_err(|source| EvalError::io(rttm, source))?;
    let reference = der::scorable(der::parse_rttm(&text, &rttm.display().to_string())?);
    let samples = read_wav(wav).map_err(asr)?;
    let started = Instant::now();
    let made = hypothesis(models, &samples, options)?;
    let run_seconds = started.elapsed().as_secs_f64();
    let scored = der::scorable(made.turns.clone());
    Ok(MeetingScore {
        der: der::der(&reference, &scored, collar),
        reference_speakers: distinct_speakers(&reference),
        hypothesis_speakers: distinct_speakers(&scored),
        reference_turns: reference.len(),
        hypothesis_segments: scored.len(),
        run_seconds,
        hypothesis: made,
    })
}

/// A map as Python prints a dict of counts: `{'fast': 8, 'remap': 1}`.
pub fn python_counts(counts: &BTreeMap<&'static str, usize>) -> String {
    let inner: Vec<String> = counts
        .iter()
        .map(|(name, count)| format!("'{name}': {count}"))
        .collect();
    format!("{{{}}}", inner.join(", "))
}

/// The line one meeting prints, in the form the Python evaluation prints.
pub fn meeting_line(meeting: &str, score: &MeetingScore) -> String {
    let made = &score.hypothesis;
    let rtf = if made.audio_seconds > 0.0 {
        made.refine_seconds / made.audio_seconds
    } else {
        0.0
    };
    let mut line = format!(
        "[{meeting}] DER={:.1}%  ref_speakers={} hyp_speakers={}  ref_turns={} hyp_segments={}  decode={:.1}s  diar={:.1}s over {} groups (diar_rtf={rtf:.3})",
        score.der * 100.0,
        score.reference_speakers,
        score.hypothesis_speakers,
        score.reference_turns,
        score.hypothesis_segments,
        score.run_seconds,
        made.refine_seconds,
        made.groups,
    );
    if !made.opened_by.is_empty() {
        line.push_str(&format!("  opened_by={}", python_counts(&made.opened_by)));
    }
    line
}

/// The closing line: the mean of the meetings' DER.
pub fn mean_line(scores: &[MeetingScore], collar: f64) -> Option<String> {
    if scores.is_empty() {
        return None;
    }
    let mean = scores.iter().map(|score| score.der).sum::<f64>() / scores.len() as f64;
    Some(format!(
        "\n=== mean DER over {} meeting(s): {:.1}% (collar={collar:?}s) ===",
        scores.len(),
        mean * 100.0
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn score(der: f64, opened: &[(&'static str, usize)]) -> MeetingScore {
        MeetingScore {
            der,
            reference_speakers: 4,
            hypothesis_speakers: 5,
            reference_turns: 83,
            hypothesis_segments: 68,
            run_seconds: 51.44,
            hypothesis: Hypothesis {
                turns: Vec::new(),
                refine_seconds: 26.84,
                groups: 32,
                audio_seconds: 600.0,
                opened_by: opened.iter().copied().collect(),
                failed_groups: 0,
            },
        }
    }

    #[test]
    fn a_meeting_prints_the_line_the_python_evaluation_prints() {
        let line = meeting_line("ES2011a", &score(0.175, &[("fast", 8)]));
        assert_eq!(
            line,
            "[ES2011a] DER=17.5%  ref_speakers=4 hyp_speakers=5  ref_turns=83 hyp_segments=68  decode=51.4s  diar=26.8s over 32 groups (diar_rtf=0.045)  opened_by={'fast': 8}"
        );
    }

    #[test]
    fn counts_are_printed_as_a_python_dict() {
        let counts: BTreeMap<&'static str, usize> =
            [("fast", 6), ("remap", 3)].into_iter().collect();
        assert_eq!(python_counts(&counts), "{'fast': 6, 'remap': 3}");
        assert_eq!(python_counts(&BTreeMap::new()), "{}");
    }

    #[test]
    fn a_meeting_that_opened_no_speaker_prints_no_opened_by() {
        assert!(!meeting_line("X", &score(0.1, &[])).contains("opened_by"));
    }

    #[test]
    fn the_mean_line_averages_the_meetings_and_names_the_collar() {
        let scores = [score(0.10, &[]), score(0.20, &[])];
        assert_eq!(
            mean_line(&scores, 0.25).as_deref(),
            Some("\n=== mean DER over 2 meeting(s): 15.0% (collar=0.25s) ===")
        );
        assert!(mean_line(&scores, 0.0).is_some_and(|line| line.ends_with("(collar=0.0s) ===")));
    }

    #[test]
    fn no_meetings_print_no_mean() {
        assert_eq!(mean_line(&[], 0.25), None);
    }

    #[test]
    fn speakers_are_counted_by_name() {
        let turns = [
            Turn::new("A", 0.0, 1.0),
            Turn::new("B", 1.0, 2.0),
            Turn::new("A", 2.0, 3.0),
        ];
        assert_eq!(distinct_speakers(&turns), 2);
        assert_eq!(distinct_speakers(&[]), 0);
    }

    #[test]
    fn a_missing_reference_file_names_its_path() {
        let said = score_meeting(
            Path::new("no-models"),
            Path::new("a.wav"),
            Path::new("no-such.rttm"),
            0.25,
            &Options {
                min_silence: 0.35,
                max_speech: 12.0,
            },
        )
        .err()
        .map(|error| error.to_string());
        assert!(
            said.as_deref()
                .is_some_and(|text| text.contains("no-such.rttm")),
            "{said:?}"
        );
    }
}

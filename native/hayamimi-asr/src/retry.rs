//! The cutting of a long buffer at its inner silences, for the split retry.
//!
//! The retry itself, and the rule that decides when to try it, are in
//! `decode`. This module finds the utterances of a buffer and builds one
//! padded piece per utterance.

use std::path::{Path, PathBuf};

use sherpa_onnx::VoiceActivityDetector;

use crate::audio::{SAMPLE_RATE, windows};
use crate::error::AsrError;
use crate::vad::{VadSettings, silero_config, take_segments};

/// A buffer this short cannot hold two utterances.
pub(crate) const MIN_BUFFER_SECONDS: f64 = 4.0;
/// Only silences at least this long cut a piece; the same value as the live
/// detector's, so a live segment splits into exactly one piece.
const MIN_SILENCE_SECONDS: f32 = 0.35;
/// Real audio kept on both sides of a piece, and the synthesised silence around it.
const PAD_SECONDS: f64 = 0.35;
/// The shortest ring buffer of the cutting detector, in seconds.
const MIN_CAPACITY_SECONDS: f64 = 60.0;

/// One buffer per span: real audio a pad before and after the span, inside a
/// pad of synthesised silence. The silence is synthesised because the detector
/// places a start late, and a piece that opens on speech loses its first word.
pub fn build_pieces(samples: &[f32], spans: &[(usize, usize)]) -> Vec<Vec<f32>> {
    let pad = (PAD_SECONDS * f64::from(SAMPLE_RATE)) as usize;
    spans
        .iter()
        .filter_map(|(start, count)| {
            let from = start.saturating_sub(pad);
            let to = (start + count + pad).min(samples.len());
            if to <= from {
                return None;
            }
            let mut piece = vec![0.0_f32; pad];
            piece.extend_from_slice(&samples[from..to]);
            piece.resize(piece.len() + pad, 0.0);
            Some(piece)
        })
        .collect()
}

/// The pieces of a buffer to decode again, or none when it should not be split.
pub type Pieces = Option<Vec<Vec<f32>>>;

/// The detector that finds the real silences of a buffer. It never forces a
/// split on length: that is the recognizer's business.
pub struct Splitter {
    models: PathBuf,
    detector: Option<(VoiceActivityDetector, f64)>,
}

impl Splitter {
    pub fn new(models: &Path) -> Self {
        Splitter {
            models: models.to_path_buf(),
            detector: None,
        }
    }

    /// The speech spans (start, length) of `samples`.
    fn spans(&mut self, samples: &[f32], seconds: f64) -> Result<Vec<(usize, usize)>, AsrError> {
        let need = MIN_CAPACITY_SECONDS.max(seconds + 2.0);
        let reusable = self
            .detector
            .as_ref()
            .is_some_and(|(_, held)| *held >= need);
        if reusable {
            if let Some((detector, _)) = &self.detector {
                detector.reset();
            }
        } else {
            let settings = VadSettings {
                min_silence: MIN_SILENCE_SECONDS,
                max_speech: need as f32,
            };
            let config = silero_config(&self.models, settings)?;
            let detector = VoiceActivityDetector::create(&config, need as f32)
                .ok_or(AsrError::Create("the cutting VAD"))?;
            self.detector = Some((detector, need));
        }
        let Some((detector, _)) = &self.detector else {
            return Err(AsrError::Create("the cutting VAD"));
        };
        let mut spans = Vec::new();
        for window in windows(samples) {
            detector.accept_waveform(&window);
            spans.extend(
                take_segments(detector)?
                    .iter()
                    .map(|s| (s.start, s.samples.len())),
            );
        }
        detector.flush();
        spans.extend(
            take_segments(detector)?
                .iter()
                .map(|s| (s.start, s.samples.len())),
        );
        Ok(spans)
    }

    /// The pieces to decode again and the speech seconds, or no pieces when
    /// there is nothing to split: a buffer too short for two utterances, or
    /// one without an inner silence.
    pub fn speech_pieces(&mut self, samples: &[f32]) -> Result<(Pieces, f64), AsrError> {
        let rate = f64::from(SAMPLE_RATE);
        let seconds = samples.len() as f64 / rate;
        if seconds <= MIN_BUFFER_SECONDS {
            return Ok((None, seconds));
        }
        let spans = self.spans(samples, seconds)?;
        let speech_seconds = spans.iter().map(|(_, count)| *count).sum::<usize>() as f64 / rate;
        // Speech seconds of zero means the detector found nothing: measure against the buffer.
        let measured = if speech_seconds > 0.0 {
            speech_seconds
        } else {
            seconds
        };
        if spans.len() < 2 {
            return Ok((None, measured));
        }
        let pieces = build_pieces(samples, &spans);
        Ok((
            if pieces.is_empty() {
                None
            } else {
                Some(pieces)
            },
            measured,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_piece_is_real_audio_inside_synthesised_silence() {
        let samples = vec![1.0_f32; 100_000];
        let pieces = build_pieces(&samples, &[(50_000, 1_000)]);
        let pad = 5_600;
        assert_eq!(pieces.len(), 1);
        assert_eq!(pieces[0].len(), pad + (1_000 + 2 * pad) + pad);
        assert_eq!(pieces[0][0], 0.0);
        assert_eq!(pieces[0][pad], 1.0);
        assert_eq!(pieces[0][pieces[0].len() - 1], 0.0);
    }

    #[test]
    fn a_span_at_the_buffer_edges_is_clamped() {
        let samples = vec![1.0_f32; 1_000];
        let pieces = build_pieces(&samples, &[(0, 1_000)]);
        assert_eq!(pieces[0].len(), 5_600 + 1_000 + 5_600);
    }

    #[test]
    fn a_span_past_the_end_makes_no_piece() {
        let samples = vec![1.0_f32; 1_000];
        assert!(build_pieces(&samples, &[(100_000, 10)]).is_empty());
    }

    #[test]
    fn a_short_buffer_has_nothing_to_split() -> Result<(), AsrError> {
        let mut splitter = Splitter::new(Path::new("no-such-models"));
        let (pieces, seconds) = splitter.speech_pieces(&vec![0.0; 16_000])?;
        assert!(pieces.is_none());
        assert_eq!(seconds, 1.0);
        Ok(())
    }

    #[test]
    fn a_long_buffer_without_the_vad_model_is_an_error_not_a_pass() {
        let mut splitter = Splitter::new(Path::new("no-such-models"));
        let said = splitter.speech_pieces(&vec![0.0; 16_000 * 5]);
        assert!(matches!(said, Err(AsrError::MissingModel(_))), "{said:?}");
    }
}

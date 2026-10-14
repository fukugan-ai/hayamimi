//! The split retry for an utterance whose first words the recognizer dropped.
//!
//! A long buffer whose text is too sparse for its speech time is suspected of
//! having lost its beginning. It is then cut at its inner silences and decoded
//! piece by piece, and the result replaces the whole-buffer text only when it
//! clearly recovers it. Cutting every buffer would lose more than it recovers,
//! so the whole-buffer text stays whenever the answer is unclear.
//!
//! The live route never reaches this: the detector's own segments hold no
//! inner silence of the length used here. It insures a long file that is
//! decoded in one go.

use std::path::{Path, PathBuf};

use sherpa_onnx::VoiceActivityDetector;

use crate::audio::{SAMPLE_RATE, windows};
use crate::error::AsrError;
use crate::recognizer::Recognizer;
use crate::vad::{VadSettings, silero_config, take_segments};

/// A buffer this short cannot hold two utterances.
const MIN_BUFFER_SECONDS: f64 = 4.0;
/// Only silences at least this long cut a piece; the same value as the live
/// detector's, so a live segment splits into exactly one piece.
const MIN_SILENCE_SECONDS: f32 = 0.35;
/// Real audio kept on both sides of a piece, and the synthesised silence around it.
const PAD_SECONDS: f64 = 0.35;
/// Alphanumeric characters per second of speech below which a decode is
/// suspected of having dropped its beginning: the log-midpoint between the
/// slowest healthy clip measured and the known defect.
const DENSITY_FLOOR: f64 = 2.4;
/// How many trailing characters of the whole-buffer text must reappear in the retry text ...
const TAIL_CHARS: usize = 12;
/// ... and what share of them.
const TAIL_MATCH: f64 = 0.6;
/// The shortest ring buffer of the cutting detector, in seconds.
const MIN_CAPACITY_SECONDS: f64 = 60.0;

/// Alphanumeric characters per second. Punctuation is not counted, so the
/// figure does not move when ITN or punctuation rewrites the text.
pub fn text_density(text: &str, seconds: f64) -> f64 {
    if seconds <= 0.0 {
        return f64::INFINITY;
    }
    text.chars().filter(|c| c.is_alphanumeric()).count() as f64 / seconds
}

/// Does this whole-buffer decode look like it lost its beginning?
///
/// `speech_seconds` is the speech time the density is measured against;
/// `buffer_seconds` is the length of the buffer, and a buffer of four seconds
/// or less is never suspected. Empty text is not suspicious: decoding silence
/// in pieces would only invent words.
pub fn looks_truncated(text: &str, speech_seconds: f64, buffer_seconds: f64) -> bool {
    if buffer_seconds <= MIN_BUFFER_SECONDS || text.trim().is_empty() {
        return false;
    }
    text_density(text, speech_seconds) < DENSITY_FLOOR
}

/// The length of the longest run of characters that `a` and `b` share.
pub fn longest_common_substring(a: &[char], b: &[char]) -> usize {
    let mut best = 0;
    let mut previous = vec![0_usize; b.len() + 1];
    for x in a {
        let mut row = vec![0_usize; b.len() + 1];
        for (j, y) in b.iter().enumerate() {
            if x == y {
                row[j + 1] = previous[j] + 1;
                best = best.max(row[j + 1]);
            }
        }
        previous = row;
    }
    best
}

/// Is the retry a recovery rather than a different reading of the audio?
///
/// Both must hold: the retry is longer and its density is back in the healthy
/// range, and most of the whole decode's tail reappears in it. The whole
/// decode is the surviving end of the utterance, so a genuine recovery keeps
/// it and adds what was lost.
pub fn retry_is_better(whole: &str, retry: &str, speech_seconds: f64) -> bool {
    let (whole, retry) = (whole.trim(), retry.trim());
    if retry.is_empty() || retry.chars().count() <= whole.chars().count() {
        return false;
    }
    if text_density(retry, speech_seconds) < DENSITY_FLOOR {
        return false;
    }
    let whole: Vec<char> = whole.chars().collect();
    let tail = &whole[whole.len().saturating_sub(TAIL_CHARS)..];
    if tail.is_empty() {
        return false;
    }
    let retry: Vec<char> = retry.chars().collect();
    longest_common_substring(tail, &retry) as f64 / tail.len() as f64 >= TAIL_MATCH
}

/// The pieces joined with nothing between them (Japanese has no spaces).
pub fn join_pieces(parts: &[String]) -> String {
    parts
        .iter()
        .map(|part| part.trim())
        .filter(|part| !part.is_empty())
        .collect()
}

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

    /// Re-decodes a suspicious buffer one utterance at a time and returns the
    /// retry text only when it is a recovery; otherwise `text` unchanged.
    pub fn retry(
        &mut self,
        recognizer: &Recognizer,
        text: &str,
        samples: &[f32],
    ) -> Result<String, AsrError> {
        let (pieces, speech_seconds) = self.speech_pieces(samples)?;
        let Some(pieces) = pieces else {
            return Ok(text.to_owned());
        };
        // The second half of the two-stage gate: the caller's check used the
        // buffer's length, this one the speech the detector actually found.
        if !looks_truncated(text, speech_seconds, speech_seconds) {
            return Ok(text.to_owned());
        }
        let parts = pieces
            .iter()
            .map(|piece| recognizer.decode(piece))
            .collect::<Result<Vec<String>, AsrError>>()?;
        let joined = join_pieces(&parts);
        if retry_is_better(text, &joined, speech_seconds) {
            Ok(joined)
        } else {
            Ok(text.to_owned())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(text: &str) -> Vec<char> {
        text.chars().collect()
    }

    #[test]
    fn density_counts_letters_and_digits_not_marks() {
        assert_eq!(text_density("あ、い。a1", 2.0), 2.0);
    }

    #[test]
    fn density_of_no_time_is_unbounded() {
        assert!(text_density("あ", 0.0).is_infinite());
    }

    #[test]
    fn a_short_buffer_is_never_suspected() {
        assert!(!looks_truncated("あ", 5.0, 4.0));
        assert!(looks_truncated("あ", 5.0, 4.01));
    }

    #[test]
    fn empty_text_is_never_suspected() {
        assert!(!looks_truncated("", 10.0, 10.0));
        assert!(!looks_truncated("  ", 10.0, 10.0));
    }

    #[test]
    fn the_density_floor_is_exclusive() {
        // 24 characters over 10 s is exactly 2.4 per second: healthy.
        let text = "あ".repeat(24);
        assert!(!looks_truncated(&text, 10.0, 10.0));
        assert!(looks_truncated(&"あ".repeat(23), 10.0, 10.0));
    }

    #[test]
    fn the_longest_common_run_is_found() {
        assert_eq!(
            longest_common_substring(&chars("abcdef"), &chars("xxcdeyy")),
            3
        );
        assert_eq!(longest_common_substring(&chars("abc"), &chars("xyz")), 0);
        assert_eq!(longest_common_substring(&[], &chars("xyz")), 0);
    }

    #[test]
    fn a_retry_that_keeps_the_tail_and_adds_the_head_is_better() {
        let whole = "天気は晴れです";
        let retry = "東京の天気は晴れです";
        assert!(retry_is_better(whole, retry, 2.0));
    }

    #[test]
    fn a_retry_that_is_not_longer_is_not_better() {
        assert!(!retry_is_better("天気は晴れです", "天気は晴れです", 2.0));
        assert!(!retry_is_better("天気は晴れです", "", 2.0));
    }

    #[test]
    fn a_retry_that_is_still_sparse_is_not_better() {
        // Longer, but 8 characters over 10 s is far under the floor.
        assert!(!retry_is_better("天気は", "東京の天気は晴れ", 10.0));
    }

    #[test]
    fn a_retry_that_loses_the_tail_is_not_better() {
        assert!(!retry_is_better(
            "天気は晴れです",
            "全然違う文章が長く続きます",
            2.0
        ));
    }

    #[test]
    fn the_tail_match_threshold_is_inclusive() {
        // The tail is 12 characters; 8 shared of 12 is 0.667, 7 of 12 is 0.583.
        let whole = "あいうえおかきくけこさし";
        let eight = "xxxxあいうえおかきくxxxx";
        let seven = "xxxxあいうえおかきxxxxxx";
        assert!(retry_is_better(whole, eight, 1.0));
        assert!(!retry_is_better(whole, seven, 1.0));
    }

    #[test]
    fn pieces_are_joined_without_a_separator_and_empty_ones_drop_out() {
        let parts = [
            String::from(" 東京の "),
            String::new(),
            String::from("天気"),
        ];
        assert_eq!(join_pieces(&parts), "東京の天気");
    }

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

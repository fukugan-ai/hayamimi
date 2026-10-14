//! Audio in and out of the pipeline: reading a wav file, the rolling history
//! that supplies the pre-roll, and the fixed-size windows the VAD reads.

use std::collections::VecDeque;
use std::path::Path;

use crate::error::AsrError;

/// The sample rate of every model in the route.
pub const SAMPLE_RATE: u32 = 16_000;
/// Samples per VAD window (32 ms).
pub const WINDOW: usize = 512;
/// Seconds of real audio put in front of a detected speech start.
pub const PREROLL_SECONDS: f64 = 1.0;
/// Seconds of audio the history keeps.
const HISTORY_SECONDS: usize = 30;

/// Reads a 16-bit PCM wav file as mono float samples at 16 kHz. Channels are
/// averaged and other rates are resampled linearly.
pub fn read_wav(path: &Path) -> Result<Vec<f32>, AsrError> {
    let wav_error = |error: hound::Error| AsrError::Wav(format!("{}: {error}", path.display()));
    let mut reader = hound::WavReader::open(path).map_err(wav_error)?;
    let spec = reader.spec();
    if spec.bits_per_sample != 16 || spec.sample_format != hound::SampleFormat::Int {
        return Err(AsrError::Wav(format!(
            "{}: expected 16-bit PCM",
            path.display()
        )));
    }
    let mut interleaved = Vec::new();
    for sample in reader.samples::<i16>() {
        interleaved.push(f32::from(sample.map_err(wav_error)?) / 32768.0);
    }
    let channels = usize::from(spec.channels);
    let mono: Vec<f32> = if channels > 1 {
        interleaved
            .chunks_exact(channels)
            .map(|frame| frame.iter().sum::<f32>() / channels as f32)
            .collect()
    } else {
        interleaved
    };
    Ok(resample_linear(&mono, spec.sample_rate, SAMPLE_RATE))
}

/// Linear-interpolation resampling. A rate that is already the target passes through.
pub fn resample_linear(samples: &[f32], from: u32, to: u32) -> Vec<f32> {
    let Some(last) = samples.last().copied() else {
        return Vec::new();
    };
    if from == to {
        return samples.to_vec();
    }
    let duration = samples.len() as f64 / f64::from(from);
    let count = (duration * f64::from(to)).round() as usize;
    let step = f64::from(from) / f64::from(to);
    (0..count)
        .map(|i| {
            let position = i as f64 * step;
            let below = position.floor() as usize;
            match (samples.get(below), samples.get(below + 1)) {
                (Some(a), Some(b)) => a + (b - a) * (position - below as f64) as f32,
                // Past the last sample the signal holds its final value.
                _ => last,
            }
        })
        .collect()
}

/// The audio in windows of `WINDOW` samples, the last one padded with zeros.
pub fn windows(samples: &[f32]) -> impl Iterator<Item = Vec<f32>> + '_ {
    samples.chunks(WINDOW).map(|chunk| {
        let mut window = chunk.to_vec();
        window.resize(WINDOW, 0.0);
        window
    })
}

/// Recent input audio, so that a final can start before the detected speech
/// start. The detector places a speech start late and the recognizer drops the
/// first words of an utterance that starts at the first sample, so a second of
/// real audio goes in front.
pub struct History {
    buffer: VecDeque<f32>,
    keep: usize,
    preroll: usize,
    /// Absolute index of `buffer[0]`.
    offset: usize,
    /// End of the previous segment: the pre-roll never reaches back into it.
    last_segment_end: usize,
}

impl History {
    pub fn new() -> Self {
        History {
            buffer: VecDeque::new(),
            keep: HISTORY_SECONDS * SAMPLE_RATE as usize,
            preroll: (PREROLL_SECONDS * f64::from(SAMPLE_RATE)) as usize,
            offset: 0,
            last_segment_end: 0,
        }
    }

    pub fn push(&mut self, chunk: &[f32]) {
        self.buffer.extend(chunk);
        if self.buffer.len() > self.keep {
            let drop = self.buffer.len() - self.keep;
            self.buffer.drain(..drop);
            self.offset += drop;
        }
    }

    /// The absolute index the next pushed sample gets: the samples pushed so far.
    pub fn end(&self) -> usize {
        self.offset + self.buffer.len()
    }

    /// The audio of the utterance in progress, for a draft: from up to a
    /// second before `start` to now, at most the last `max_len` samples.
    /// Like [`History::with_preroll`] it never reaches back into the previous
    /// segment, and unlike it nothing is moved: the final that follows still
    /// gets its own pre-roll.
    pub fn draft(&self, start: usize, max_len: usize) -> Vec<f32> {
        let wanted = start
            .saturating_sub(self.preroll)
            .max(self.last_segment_end)
            .max(self.offset);
        let from = (wanted - self.offset).min(self.buffer.len());
        let from = from.max(self.buffer.len().saturating_sub(max_len));
        self.buffer.range(from..).copied().collect()
    }

    /// The segment with up to a second of real audio in front of it, clamped
    /// by the previous segment's end and by what the history still holds.
    pub fn with_preroll(&mut self, start: usize, segment: &[f32]) -> Vec<f32> {
        let want = start
            .saturating_sub(self.preroll)
            .max(self.last_segment_end)
            .max(self.offset);
        let from = (want - self.offset).min(self.buffer.len());
        let to = start.saturating_sub(self.offset).min(self.buffer.len());
        self.last_segment_end = start + segment.len();
        let mut out = Vec::with_capacity(to.saturating_sub(from) + segment.len());
        if from < to {
            out.extend(self.buffer.range(from..to));
        }
        out.extend_from_slice(segment);
        out
    }
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(from: usize, to: usize) -> Vec<f32> {
        (from..to).map(|i| i as f32).collect()
    }

    #[test]
    fn the_last_window_is_padded_with_zeros() {
        let all: Vec<Vec<f32>> = windows(&vec![1.0; WINDOW + 3]).collect();
        assert_eq!(all.len(), 2);
        assert_eq!(all[1].len(), WINDOW);
        assert_eq!(all[1][2], 1.0);
        assert_eq!(all[1][3], 0.0);
    }

    #[test]
    fn no_samples_make_no_windows() {
        assert_eq!(windows(&[]).count(), 0);
    }

    #[test]
    fn preroll_takes_a_second_before_the_start() {
        let mut history = History::new();
        history.push(&ramp(0, 40_000));
        let out = history.with_preroll(30_000, &[7.0; 4]);
        assert_eq!(out.len(), 16_000 + 4);
        assert_eq!(out[0], 14_000.0);
        assert_eq!(out[16_000], 7.0);
    }

    #[test]
    fn preroll_does_not_reach_into_the_previous_segment() {
        let mut history = History::new();
        history.push(&ramp(0, 40_000));
        let first = history.with_preroll(10_000, &[1.0; 2_000]);
        assert_eq!(first.len(), 10_000 + 2_000);
        // The previous segment ended at 12_000; this one starts 3_000 later.
        let second = history.with_preroll(15_000, &[2.0; 10]);
        assert_eq!(second.len(), 3_000 + 10);
        assert_eq!(second[0], 12_000.0);
    }

    #[test]
    fn preroll_is_limited_to_what_the_history_still_holds() {
        let mut history = History::new();
        history.push(&ramp(0, 500_000));
        // 30 s are kept, so the offset is 20_000.
        let out = history.with_preroll(20_100, &[9.0]);
        assert_eq!(out.len(), 100 + 1);
        assert_eq!(out[0], 20_000.0);
    }

    #[test]
    fn a_segment_at_the_very_start_has_no_preroll() {
        let mut history = History::new();
        history.push(&ramp(0, 1_000));
        assert_eq!(history.with_preroll(0, &[5.0; 3]), [5.0; 3]);
    }

    #[test]
    fn the_end_counts_every_pushed_sample() {
        let mut history = History::new();
        assert_eq!(history.end(), 0);
        history.push(&ramp(0, 1_000));
        history.push(&ramp(0, 24));
        assert_eq!(history.end(), 1_024);
    }

    #[test]
    fn a_draft_runs_from_a_second_before_the_start_to_now() {
        let mut history = History::new();
        history.push(&ramp(0, 40_000));
        let out = history.draft(30_000, usize::MAX);
        assert_eq!(out.len(), 40_000 - 14_000);
        assert_eq!(out[0], 14_000.0);
        assert_eq!(out.last().copied(), Some(39_999.0));
    }

    #[test]
    fn a_draft_does_not_reach_into_the_previous_segment() {
        let mut history = History::new();
        history.push(&ramp(0, 40_000));
        history.with_preroll(10_000, &[1.0; 2_000]);
        // The previous segment ended at 12_000, which is later than 15_000 - 16_000.
        let out = history.draft(15_000, usize::MAX);
        assert_eq!(out[0], 12_000.0);
    }

    #[test]
    fn a_draft_is_cut_to_its_last_samples() {
        let mut history = History::new();
        history.push(&ramp(0, 40_000));
        let out = history.draft(30_000, 100);
        assert_eq!(out.len(), 100);
        assert_eq!(out[0], 39_900.0);
    }

    #[test]
    fn a_draft_leaves_the_pre_roll_of_the_final_alone() {
        let mut history = History::new();
        history.push(&ramp(0, 40_000));
        let _ = history.draft(30_000, usize::MAX);
        let out = history.with_preroll(30_000, &[7.0; 4]);
        assert_eq!(out.len(), 16_000 + 4);
        assert_eq!(out[0], 14_000.0);
    }

    #[test]
    fn a_draft_of_nothing_is_empty() {
        let history = History::new();
        assert!(history.draft(5_000, 100).is_empty());
    }

    #[test]
    fn an_empty_history_adds_nothing() {
        let mut history = History::new();
        assert_eq!(history.with_preroll(50_000, &[5.0]), [5.0]);
    }

    #[test]
    fn the_same_rate_passes_through() {
        assert_eq!(resample_linear(&[0.0, 1.0], 16_000, 16_000), [0.0, 1.0]);
    }

    #[test]
    fn downsampling_by_two_keeps_every_other_sample_of_a_ramp() {
        let out = resample_linear(&[0.0, 1.0, 2.0, 3.0], 32_000, 16_000);
        assert_eq!(out, [0.0, 2.0]);
    }

    #[test]
    fn upsampling_interpolates_between_samples() {
        let out = resample_linear(&[0.0, 2.0], 8_000, 16_000);
        assert_eq!(out, [0.0, 1.0, 2.0, 2.0]);
    }

    #[test]
    fn nothing_resamples_to_nothing() {
        assert!(resample_linear(&[], 8_000, 16_000).is_empty());
    }

    #[test]
    fn a_missing_wav_is_a_wav_error() {
        let said = read_wav(Path::new("no-such-file.wav"));
        assert!(matches!(said, Err(AsrError::Wav(_))), "{said:?}");
    }
}

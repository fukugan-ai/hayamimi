//! Turns what a device or a network client delivers into what the detector
//! reads: mono, 16 kHz, in windows of `WINDOW` samples. Every step works on
//! a stream of buffers of any size and keeps its own remainder, so the output
//! does not depend on where the input was cut.

use hayamimi_asr::audio::WINDOW;

/// Averages interleaved frames of `channels` samples to one sample each. A
/// last, incomplete frame is dropped.
pub fn downmix(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    let scale = 1.0 / channels as f32;
    interleaved
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() * scale)
        .collect()
}

/// Little-endian signed 16-bit samples as floats in `[-1, 1)`. A last odd byte is dropped.
pub fn pcm16_to_f32(bytes: &[u8]) -> Vec<f32> {
    let (pairs, _) = bytes.as_chunks::<2>();
    pairs
        .iter()
        .map(|pair| f32::from(i16::from_le_bytes(*pair)) / 32768.0)
        .collect()
}

/// Little-endian 32-bit floats, the form a Windows capture buffer comes in.
/// A last incomplete value is dropped.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn le_f32(bytes: &[u8]) -> Vec<f32> {
    let (quads, _) = bytes.as_chunks::<4>();
    quads.iter().map(|quad| f32::from_le_bytes(*quad)).collect()
}

/// Linear-interpolation resampling of a stream. It gives, concatenated, the
/// same samples however the input is cut into buffers.
#[derive(Debug, Clone)]
pub struct Resampler {
    /// Input samples per output sample.
    step: f64,
    /// Position of the next output sample, counted in input samples from the start of the stream.
    next: f64,
    /// Input samples seen before the current buffer.
    consumed: usize,
    /// The last input sample of the previous buffer.
    previous: f32,
    passthrough: bool,
}

impl Resampler {
    pub fn new(from: u32, to: u32) -> Resampler {
        Resampler {
            step: f64::from(from) / f64::from(to),
            next: 0.0,
            consumed: 0,
            previous: 0.0,
            passthrough: from == to,
        }
    }

    /// The input sample at stream position `index`, which is either in
    /// `input` or the last one of the buffer before it.
    fn at(&self, index: usize, input: &[f32]) -> f32 {
        match index.checked_sub(self.consumed) {
            Some(inside) => input[inside],
            None => self.previous,
        }
    }

    /// Resamples one buffer and returns the output samples it completes.
    pub fn push(&mut self, input: &[f32]) -> Vec<f32> {
        if self.passthrough {
            return input.to_vec();
        }
        let end = self.consumed + input.len();
        let mut out = Vec::with_capacity((input.len() as f64 / self.step) as usize + 1);
        loop {
            let below = self.next.floor();
            let index = below as usize;
            // The output sample needs the input sample after `index` as well.
            if index + 1 >= end {
                break;
            }
            let a = self.at(index, input);
            let b = self.at(index + 1, input);
            out.push(a + (b - a) * (self.next - below) as f32);
            self.next += self.step;
        }
        if let Some(last) = input.last() {
            self.previous = *last;
        }
        self.consumed = end;
        out
    }
}

/// Cuts a stream of samples into windows of `WINDOW`.
#[derive(Debug, Default)]
pub struct Windower {
    pending: Vec<f32>,
}

impl Windower {
    /// Adds samples and returns the windows they complete.
    pub fn push(&mut self, samples: &[f32]) -> Vec<Vec<f32>> {
        self.pending.extend_from_slice(samples);
        let whole = self.pending.len() / WINDOW * WINDOW;
        let (whole_windows, _) = self.pending[..whole].as_chunks::<WINDOW>();
        let windows = whole_windows.iter().map(|window| window.to_vec()).collect();
        self.pending.drain(..whole);
        windows
    }

    /// How many samples wait for the rest of a window.
    pub fn waiting(&self) -> usize {
        self.pending.len()
    }
}

/// The whole chain for one source: mixes the channels down, resamples to 16
/// kHz and cuts windows.
#[derive(Debug)]
pub struct Chain {
    channels: usize,
    resampler: Resampler,
    windower: Windower,
}

impl Chain {
    pub fn new(from_rate: u32, channels: usize, to_rate: u32) -> Chain {
        Chain {
            channels,
            resampler: Resampler::new(from_rate, to_rate),
            windower: Windower::default(),
        }
    }

    /// Takes interleaved float samples and returns the windows they complete.
    pub fn push(&mut self, interleaved: &[f32]) -> Vec<Vec<f32>> {
        let mono = downmix(interleaved, self.channels);
        let resampled = self.resampler.push(&mono);
        self.windower.push(&resampled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stereo_frames_average_and_a_split_frame_is_dropped() {
        assert_eq!(downmix(&[1.0, 0.0, 0.5, 0.5, 9.0], 2), vec![0.5, 0.5]);
    }

    #[test]
    fn mono_passes_through_and_empty_stays_empty() {
        assert_eq!(downmix(&[0.25, -0.25], 1), vec![0.25, -0.25]);
        assert!(downmix(&[], 2).is_empty());
    }

    #[test]
    fn pcm16_reads_little_endian_and_scales() {
        let bytes = [0x00, 0x80, 0x00, 0x40, 0xff];
        assert_eq!(pcm16_to_f32(&bytes), vec![-1.0, 0.5]);
        assert!(pcm16_to_f32(&[]).is_empty());
    }

    #[test]
    fn floats_read_little_endian_and_a_partial_value_is_dropped() {
        let mut bytes = 0.5_f32.to_le_bytes().to_vec();
        bytes.extend_from_slice(&(-1.0_f32).to_le_bytes());
        bytes.push(0x7f);
        assert_eq!(le_f32(&bytes), vec![0.5, -1.0]);
        assert!(le_f32(&[]).is_empty());
    }

    #[test]
    fn the_same_rate_is_not_touched() {
        let mut resampler = Resampler::new(16_000, 16_000);
        assert_eq!(resampler.push(&[0.1, 0.2]), vec![0.1, 0.2]);
    }

    #[test]
    fn a_constant_stays_constant_and_the_length_follows_the_ratio() {
        let mut resampler = Resampler::new(48_000, 16_000);
        let out = resampler.push(&[0.5; 4800]);
        assert!(out.iter().all(|sample| (sample - 0.5).abs() < 1e-6));
        // 4800 samples at 3:1, less the one the last output waits for.
        assert!((1599..=1600).contains(&out.len()), "{}", out.len());
    }

    #[test]
    fn upsampling_interpolates_between_neighbours() {
        let mut resampler = Resampler::new(8_000, 16_000);
        let out = resampler.push(&[0.0, 1.0, 0.0]);
        assert_eq!(out, vec![0.0, 0.5, 1.0, 0.5]);
    }

    #[test]
    fn where_the_input_is_cut_does_not_change_the_output() {
        let input: Vec<f32> = (0..1000).map(|n| ((n * 7) % 13) as f32 / 13.0).collect();
        let mut whole = Resampler::new(44_100, 16_000);
        let expected = whole.push(&input);
        for cut in [1_usize, 2, 3, 100, 441, 999] {
            let mut parts = Resampler::new(44_100, 16_000);
            let mut got = parts.push(&input[..cut]);
            got.extend(parts.push(&input[cut..]));
            assert_eq!(got.len(), expected.len(), "cut at {cut}");
            for (a, b) in got.iter().zip(&expected) {
                assert!((a - b).abs() < 1e-5, "cut at {cut}");
            }
        }
    }

    #[test]
    fn an_empty_buffer_changes_nothing() {
        let mut resampler = Resampler::new(48_000, 16_000);
        assert!(resampler.push(&[]).is_empty());
        let out = resampler.push(&[0.5; 9]);
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn windows_come_out_whole_and_the_rest_waits() {
        let mut windower = Windower::default();
        assert!(windower.push(&[0.0; WINDOW - 1]).is_empty());
        assert_eq!(windower.waiting(), WINDOW - 1);
        let windows = windower.push(&[1.0; 2]);
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].len(), WINDOW);
        assert_eq!(windower.waiting(), 1);
        assert_eq!(windower.push(&[0.0; WINDOW - 1]).len(), 1);
        assert_eq!(windower.waiting(), 0);
        assert!(windower.push(&[]).is_empty());
    }

    #[test]
    fn speaker_capture_owns_com_objects_and_downmixes_native_channels() {
        // The device's own stereo format is averaged to one channel per frame.
        let mut chain = Chain::new(16_000, 2, 16_000);
        let interleaved: Vec<f32> = (0..WINDOW).flat_map(|_| [0.2_f32, 0.6]).collect();
        let windows = chain.push(&interleaved);
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].len(), WINDOW);
        assert!(windows[0].iter().all(|sample| (sample - 0.4).abs() < 1e-6));
    }

    #[test]
    fn the_chain_turns_a_second_of_stereo_48k_into_whole_windows() {
        let mut chain = Chain::new(48_000, 2, 16_000);
        let windows = chain.push(&vec![0.25; 48_000 * 2]);
        // 16 000 samples hold 31 windows of 512 and a remainder.
        assert_eq!(windows.len(), 31);
        assert!(windows.iter().all(|window| window.len() == WINDOW));
    }
}

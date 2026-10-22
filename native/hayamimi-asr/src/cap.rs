//! The cap on the audio one decode reads.
//!
//! The Japanese transducer drops runs of words when one call gets a buffer
//! much longer than the utterances it was trained on. A buffer longer than the
//! cap `C` is therefore cut at its quietest frames into `ceil(T / C)` balanced
//! pieces, each piece is read on its own inside a short synthetic silence, and
//! the sentences are joined in order. A buffer within the cap, a cap that is
//! `off`, and every tier but the Japanese one are read in one call, as they
//! were before the cap existed.
//!
//! Frames are 320 samples (20 ms) counted from the first sample of the buffer,
//! and a cut lies on the first sample of a frame. A cut `k` is searched in the
//! window `[lo, hi)` around the balanced target `floor(k * T / N)`:
//!
//! - `lo = max(target - W/2, T - (N - k) * C, previous + 320)`
//! - `hi = min(target + W/2, previous + C)`
//!
//! The second term of `lo` and of `hi` keep every piece within `C` samples and
//! the number of pieces at `N`. Among the frames that start inside the window
//! and lie wholly in the buffer the cut takes the smallest energy, then the
//! frame nearest the target, then the earlier one. A window that holds no frame
//! start puts the cut at `lo`.

use std::fmt;
use std::ops::Range;

use crate::audio::SAMPLE_RATE;
use crate::retry::PAD_SECONDS;
use crate::route::Tier;

const RATE: usize = SAMPLE_RATE as usize;
/// Samples in one frame of the quietness scan: 20 ms.
pub const FRAME: usize = 320;
/// The width `W` of the window a cut is searched in: 2.0 s.
pub const WINDOW: usize = 2 * RATE;
/// The shortest cap: two frames, so that a piece holds a frame and the window of a cut is never inverted.
const MIN_CAP: usize = 2 * FRAME;
/// The fewest characters of a seam that are folded into one.
const FOLD_MIN: usize = 2;
/// The most characters of a seam that are folded into one (0.5 s of the fastest speech measured).
const FOLD_MAX: usize = 6;

/// How much audio one decode of the Japanese tier reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DecodeCap {
    /// One call for a buffer of any length.
    #[default]
    Off,
    /// A buffer longer than `cap` samples is read in pieces; neighbouring
    /// pieces share `overlap` samples.
    Samples { cap: usize, overlap: usize },
}

/// A cap that is neither `off` nor a length in seconds the cut can work with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BadCap;

impl fmt::Display for BadCap {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            out,
            "a decode cap is `off` or a length in seconds of at least {} ms",
            MIN_CAP * 1000 / RATE
        )
    }
}

impl std::error::Error for BadCap {}

/// A length in seconds as samples; the seconds must be finite and not negative.
fn samples_of(seconds: f64) -> Result<usize, BadCap> {
    if !seconds.is_finite() || seconds < 0.0 {
        return Err(BadCap);
    }
    Ok((seconds * RATE as f64).round() as usize)
}

/// How a buffer was cut, for the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cut {
    pub pieces: usize,
    pub buffer_ms: usize,
    pub cap_ms: usize,
}

impl DecodeCap {
    /// A cap of `cap_s` seconds whose neighbouring pieces share `overlap_s` seconds.
    pub fn from_seconds(cap_s: f64, overlap_s: f64) -> Result<DecodeCap, BadCap> {
        let (cap, overlap) = (samples_of(cap_s)?, samples_of(overlap_s)?);
        if cap < MIN_CAP || overlap >= cap {
            return Err(BadCap);
        }
        Ok(DecodeCap::Samples { cap, overlap })
    }

    /// The value of `--decode-cap`: `off`, or a number of seconds.
    pub fn parse(text: &str) -> Result<DecodeCap, BadCap> {
        if text == "off" {
            return Ok(DecodeCap::Off);
        }
        let seconds = text.parse::<f64>().map_err(|_| BadCap)?;
        DecodeCap::from_seconds(seconds, 0.0)
    }

    /// The cap in seconds; `None` when it is off.
    pub fn cap_seconds(self) -> Option<f64> {
        match self {
            DecodeCap::Off => None,
            DecodeCap::Samples { cap, .. } => Some(cap as f64 / f64::from(SAMPLE_RATE)),
        }
    }

    /// The seconds neighbouring pieces share; zero when the cap is off.
    pub fn overlap_seconds(self) -> f64 {
        match self {
            DecodeCap::Off => 0.0,
            DecodeCap::Samples { overlap, .. } => overlap as f64 / f64::from(SAMPLE_RATE),
        }
    }

    /// Whether the sentences of neighbouring pieces fold a shared run of characters.
    pub fn folds(self) -> bool {
        matches!(self, DecodeCap::Samples { overlap, .. } if overlap > 0)
    }

    /// Whether a buffer of `length` samples read by `tier` is cut into pieces:
    /// the cap is on, the tier is the Japanese one, and the buffer is longer
    /// than the cap.
    pub fn cuts(self, tier: Tier, length: usize) -> bool {
        matches!(self, DecodeCap::Samples { cap, .. } if tier == Tier::Rz && length > cap)
    }

    /// The pieces of `samples` to read one by one, or `None` for one call on
    /// the whole buffer.
    pub fn plan(self, tier: Tier, samples: &[f32]) -> Option<Vec<Range<usize>>> {
        let DecodeCap::Samples { cap, overlap } = self else {
            return None;
        };
        self.cuts(tier, samples.len())
            .then(|| pieces(samples, cap, WINDOW, overlap))
    }

    /// The log record of a buffer of `total` samples cut into `pieces`.
    pub fn cut_of(self, total: usize, pieces: usize) -> Cut {
        let cap = match self {
            DecodeCap::Off => 0,
            DecodeCap::Samples { cap, .. } => cap,
        };
        Cut {
            pieces,
            buffer_ms: total * 1000 / RATE,
            cap_ms: cap * 1000 / RATE,
        }
    }
}

/// Where the cuts of one buffer are searched.
struct Search {
    total: usize,
    count: usize,
    cap: usize,
    half: usize,
}

impl Search {
    /// The balanced position of cut `k`: `floor(k * T / N)`.
    fn target(&self, k: usize) -> usize {
        // The quotient is at most `total`, so it fits a usize.
        (k as u64 * self.total as u64 / self.count as u64) as usize
    }

    /// The samples the first sample of cut `k` may lie in, given the cut before it.
    fn bounds(&self, k: usize, previous: usize) -> Range<usize> {
        let target = self.target(k);
        let lo = target
            .saturating_sub(self.half)
            .max(self.total.saturating_sub((self.count - k) * self.cap))
            .max(previous + FRAME);
        let hi = (target + self.half).min(previous + self.cap);
        lo..hi
    }
}

/// The sum of the squares of a frame; the same order of terms for every frame.
fn frame_energy(frame: &[f32]) -> f64 {
    frame
        .iter()
        .map(|sample| {
            let value = f64::from(*sample);
            value * value
        })
        .sum()
}

/// The first sample of cut `k`: the quietest frame of its window.
fn cut(samples: &[f32], search: &Search, k: usize, previous: usize) -> usize {
    let window = search.bounds(k, previous);
    let target = search.target(k);
    let first = window.start.div_ceil(FRAME);
    let end = window.end.div_ceil(FRAME).min(samples.len() / FRAME);
    let quietest = (first..end)
        .map(|frame| {
            let start = frame * FRAME;
            let energy = frame_energy(&samples[start..start + FRAME]);
            (energy, start.abs_diff(target), start)
        })
        .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    // DEFAULT: a window that holds no frame start puts the cut at its low end, as the cut rules fix.
    let Some((_, _, start)) = quietest else {
        return window.start;
    };
    start
}

/// The pieces of `samples` for a cap of `cap` samples, as sample ranges.
///
/// A buffer within the cap is one piece. A longer one is cut into
/// `ceil(len / cap)` pieces, and every piece but the first starts `overlap`
/// samples before its cut. `cap` must be at least two frames and `window` the
/// width `W` of the search around each balanced target.
pub fn pieces(samples: &[f32], cap: usize, window: usize, overlap: usize) -> Vec<Range<usize>> {
    let total = samples.len();
    if total <= cap {
        return std::iter::once(0..total).collect();
    }
    let search = Search {
        total,
        count: total.div_ceil(cap),
        cap,
        half: window / 2,
    };
    let mut cuts = Vec::with_capacity(search.count + 1);
    cuts.push(0);
    let mut previous = 0;
    for k in 1..search.count {
        previous = cut(samples, &search, k, previous);
        cuts.push(previous);
    }
    cuts.push(total);
    cuts.windows(2)
        .enumerate()
        .map(|(index, pair)| {
            let start = if index == 0 {
                pair[0]
            } else {
                pair[0].saturating_sub(overlap)
            };
            start..pair[1]
        })
        .collect()
}

/// The samples of synthetic silence on each side of a piece: the pad of the split retry.
fn pad_samples() -> usize {
    (PAD_SECONDS * f64::from(SAMPLE_RATE)) as usize
}

/// Writes `piece` into `padded` between two stretches of silence.
pub(crate) fn pad_into(padded: &mut Vec<f32>, piece: &[f32]) {
    let pad = pad_samples();
    padded.clear();
    padded.resize(pad, 0.0);
    padded.extend_from_slice(piece);
    padded.resize(pad + piece.len() + pad, 0.0);
}

/// Whether a space belongs between two sentences: ASCII letters or digits on both sides.
fn needs_space(joined: &str, next: &str) -> bool {
    let ends = joined.chars().next_back();
    let starts = next.chars().next();
    ends.is_some_and(|last| last.is_ascii_alphanumeric())
        && starts.is_some_and(|first| first.is_ascii_alphanumeric())
}

/// The last `count` characters of `text`; `None` when it has fewer.
fn tail_chars(text: &str, count: usize) -> Option<&str> {
    let (at, _) = text.char_indices().rev().nth(count - 1)?;
    Some(&text[at..])
}

/// What of `next` follows the longest run of 2 to 6 characters that ends
/// `joined` and starts `next`; `None` when there is no such run.
fn after_overlap<'a>(joined: &str, next: &'a str) -> Option<&'a str> {
    (FOLD_MIN..=FOLD_MAX).rev().find_map(|count| {
        let tail = tail_chars(joined, count)?;
        next.strip_prefix(tail)
    })
}

/// Appends the sentence `next` to `joined`.
fn append(joined: &mut String, next: &str, fold: bool) {
    if joined.is_empty() {
        joined.push_str(next);
        return;
    }
    let folded = if fold {
        after_overlap(joined, next)
    } else {
        None
    };
    match folded {
        Some(rest) => joined.push_str(rest),
        None => {
            if needs_space(joined, next) {
                joined.push(' ');
            }
            joined.push_str(next);
        }
    }
}

/// The sentences of the pieces, in order, as one sentence.
///
/// Each sentence is trimmed and an empty one is dropped. Nothing goes between
/// two sentences unless both sides of the seam are ASCII letters or digits,
/// which get one space. With `fold`, the longest run of 2 to 6 characters that
/// ends the text so far and starts the next sentence is kept once.
pub fn join_sentences<S: AsRef<str>>(parts: &[S], fold: bool) -> String {
    let mut joined = String::new();
    for part in parts
        .iter()
        .map(|part| part.as_ref().trim())
        .filter(|part| !part.is_empty())
    {
        append(&mut joined, part, fold);
    }
    joined
}

#[cfg(test)]
mod tests {
    use super::*;

    type Check = Result<(), Box<dyn std::error::Error>>;

    /// `C` = 10 s, `W` = 2.0 s.
    const CAP: usize = 160_000;
    const W: usize = 32_000;

    /// Square wave of 16 samples: 8 at `+amplitude`, 8 at `-amplitude`.
    fn wave(length: usize, amplitude: f32) -> Vec<f32> {
        (0..length)
            .map(|at| if at % 16 < 8 { amplitude } else { -amplitude })
            .collect()
    }

    /// A list holding one range.
    fn vec_of(range: Range<usize>) -> Vec<Range<usize>> {
        std::iter::once(range).collect()
    }

    fn tone(length: usize) -> Vec<f32> {
        wave(length, 0.5)
    }

    /// Frame `frame` of `samples` as a wave of `amplitude` (0.0 is silence).
    fn set_frame(samples: &mut [f32], frame: usize, amplitude: f32) {
        let start = frame * FRAME;
        let quiet = wave(FRAME, amplitude);
        samples[start..start + FRAME].copy_from_slice(&quiet);
    }

    fn with_frames(length: usize, frames: &[(usize, f32)]) -> Vec<f32> {
        let mut samples = tone(length);
        for (frame, amplitude) in frames {
            set_frame(&mut samples, *frame, *amplitude);
        }
        samples
    }

    /// The first sample of every piece but the first (no overlap).
    fn cuts_of(ranges: &[Range<usize>]) -> Vec<usize> {
        ranges.iter().skip(1).map(|range| range.start).collect()
    }

    fn lengths_of(ranges: &[Range<usize>]) -> Vec<usize> {
        ranges.iter().map(|range| range.len()).collect()
    }

    /// The pieces tile `0..total` with no gap and no overlap.
    fn tiles(ranges: &[Range<usize>], total: usize) -> bool {
        ranges.first().is_some_and(|range| range.start == 0)
            && ranges.last().is_some_and(|range| range.end == total)
            && ranges.windows(2).all(|pair| pair[0].end == pair[1].start)
    }

    fn cut_f16(frames: &[(usize, f32)]) -> Vec<usize> {
        cuts_of(&pieces(&with_frames(256_000, frames), CAP, W, 0))
    }

    fn cut_f24(frames: &[(usize, f32)]) -> Vec<Range<usize>> {
        pieces(&with_frames(384_000, frames), CAP, W, 0)
    }

    #[test]
    fn a_long_buffer_is_cut_at_its_quietest_frames_into_balanced_pieces() {
        let samples = with_frames(400_000, &[(440, 0.0), (840, 0.0)]);
        let ranges = pieces(&samples, CAP, W, 0);
        assert_eq!(ranges, [0..140_800, 140_800..268_800, 268_800..400_000]);
        assert_eq!(lengths_of(&ranges), [140_800, 128_000, 131_200]);
        assert!(lengths_of(&ranges).iter().all(|length| *length <= CAP));
        assert!(tiles(&ranges, 400_000));
    }

    #[test]
    fn cuts_land_in_valleys_after_the_target() {
        let ranges = cut_f24(&[(420, 0.0), (830, 0.0)]);
        assert_eq!(cuts_of(&ranges), [134_400, 265_600]);
        assert_eq!(lengths_of(&ranges), [134_400, 131_200, 118_400]);
    }

    #[test]
    fn cuts_land_in_valleys_before_the_target() {
        let ranges = cut_f24(&[(370, 0.0), (780, 0.0)]);
        assert_eq!(cuts_of(&ranges), [118_400, 249_600]);
        assert_eq!(lengths_of(&ranges), [118_400, 131_200, 134_400]);
    }

    #[test]
    fn a_valley_outside_the_search_window_is_ignored() {
        assert_eq!(cut_f16(&[(380, 0.25), (480, 0.0)]), [121_600]);
    }

    #[test]
    fn a_valley_inside_the_search_window_is_found() {
        assert_eq!(cut_f16(&[(380, 0.25), (440, 0.0)]), [140_800]);
    }

    #[test]
    fn the_last_frame_inside_the_window_is_found() {
        assert_eq!(cut_f16(&[(449, 0.0)]), [143_680]);
    }

    #[test]
    fn a_frame_beyond_the_window_is_ignored_at_the_high_end() {
        assert_eq!(cut_f16(&[(450, 0.0), (400, 0.25)]), [128_000]);
    }

    #[test]
    fn the_first_frame_inside_the_window_is_found() {
        assert_eq!(cut_f16(&[(350, 0.0)]), [112_000]);
    }

    #[test]
    fn a_frame_beyond_the_window_is_ignored_at_the_low_end() {
        assert_eq!(cut_f16(&[(349, 0.0), (400, 0.25)]), [128_000]);
    }

    #[test]
    fn a_buffer_of_exactly_the_cap_is_one_piece() {
        let samples = tone(CAP);
        assert_eq!(pieces(&samples, CAP, W, 0), vec_of(0..CAP));
    }

    #[test]
    fn a_buffer_one_sample_over_the_cap_is_two_pieces() {
        let ranges = pieces(&tone(160_001), CAP, W, 0);
        assert_eq!(cuts_of(&ranges), [80_000]);
        assert_eq!(lengths_of(&ranges), [80_000, 80_001]);
    }

    #[test]
    fn a_buffer_one_frame_over_the_cap_is_two_pieces() {
        let ranges = pieces(&tone(160_320), CAP, W, 0);
        assert_eq!(cuts_of(&ranges), [80_000]);
        assert_eq!(lengths_of(&ranges), [80_000, 80_320]);
    }

    #[test]
    fn a_buffer_of_exactly_twice_the_cap_is_two_pieces() {
        let ranges = pieces(&tone(320_000), CAP, W, 0);
        assert_eq!(cuts_of(&ranges), [160_000]);
        assert_eq!(lengths_of(&ranges), [160_000, 160_000]);
    }

    #[test]
    fn a_buffer_one_sample_over_twice_the_cap_is_three_pieces() {
        let ranges = pieces(&tone(320_001), CAP, W, 0);
        assert_eq!(cuts_of(&ranges), [106_560, 213_440]);
        assert_eq!(lengths_of(&ranges), [106_560, 106_880, 106_561]);
    }

    #[test]
    fn a_buffer_one_frame_over_twice_the_cap_is_three_pieces() {
        let ranges = pieces(&tone(320_320), CAP, W, 0);
        assert_eq!(cuts_of(&ranges), [106_880, 213_440]);
        assert_eq!(lengths_of(&ranges), [106_880, 106_560, 106_880]);
    }

    #[test]
    fn a_buffer_without_silence_is_cut_into_pieces_within_the_cap() {
        let ranges = pieces(&tone(400_000), CAP, W, 0);
        assert_eq!(cuts_of(&ranges), [133_440, 266_560]);
        assert_eq!(lengths_of(&ranges), [133_440, 133_120, 133_440]);
        assert!(lengths_of(&ranges).iter().all(|length| *length <= CAP));
        assert!(tiles(&ranges, 400_000));
    }

    #[test]
    fn an_uncapped_or_other_tier_buffer_is_one_piece() {
        let samples = with_frames(400_000, &[(440, 0.0), (840, 0.0)]);
        let ten_seconds = DecodeCap::Samples {
            cap: CAP,
            overlap: 0,
        };
        assert_eq!(DecodeCap::Off.plan(Tier::Rz, &samples), None);
        for tier in Tier::ALL.into_iter().filter(|tier| *tier != Tier::Rz) {
            assert_eq!(ten_seconds.plan(tier, &samples), None, "{tier:?}");
        }
        let planned = ten_seconds.plan(Tier::Rz, &samples);
        assert_eq!(planned.map(|ranges| ranges.len()), Some(3));
    }

    /// splitmix64: the noise of the property test.
    fn noise(length: usize) -> Vec<f32> {
        let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
        (0..length)
            .map(|_| {
                state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
                let mut mixed = state;
                mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
                mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
                mixed ^= mixed >> 31;
                ((mixed >> 11) as f64 / (1_u64 << 53) as f64 - 0.5) as f32
            })
            .collect()
    }

    /// The cuts by the rules, written as a plain search over every frame.
    fn reference_cuts(samples: &[f32], cap: usize, window: usize) -> Vec<usize> {
        let total = samples.len();
        let count = total.div_ceil(cap);
        let energies: Vec<f64> = samples
            .as_chunks::<FRAME>()
            .0
            .iter()
            .map(|frame| frame.iter().map(|x| f64::from(*x) * f64::from(*x)).sum())
            .collect();
        let half = (window / 2) as i64;
        let mut cuts = vec![0_usize];
        for k in 1..count {
            let target = (k * total / count) as i64;
            let previous = cuts[k - 1] as i64;
            let lo = (target - half)
                .max(total as i64 - ((count - k) * cap) as i64)
                .max(previous + FRAME as i64);
            let hi = (target + half).min(previous + cap as i64);
            let best = (0..energies.len())
                .filter(|frame| {
                    let start = (frame * FRAME) as i64;
                    lo <= start && start < hi
                })
                .min_by(|a, b| {
                    let (sa, sb) = ((a * FRAME) as i64, (b * FRAME) as i64);
                    energies[*a]
                        .total_cmp(&energies[*b])
                        .then((sa - target).abs().cmp(&(sb - target).abs()))
                        .then(sa.cmp(&sb))
                });
            let Some(frame) = best else {
                cuts.push(lo as usize);
                continue;
            };
            cuts.push(frame * FRAME);
        }
        cuts.remove(0);
        cuts
    }

    #[test]
    fn pieces_tile_the_buffer_and_each_cut_stays_near_its_target() {
        for (length, count) in [
            (160_001, 2),
            (200_000, 2),
            (250_001, 2),
            (320_001, 3),
            (400_000, 3),
            (450_001, 3),
        ] {
            let samples = noise(length);
            let ranges = pieces(&samples, CAP, W, 0);
            assert_eq!(ranges.len(), count, "{length}");
            assert!(tiles(&ranges, length), "{length}");
            assert!(
                ranges
                    .iter()
                    .all(|range| (FRAME..=CAP).contains(&range.len())),
                "{length}: {ranges:?}"
            );
            assert!(cuts_of(&ranges).iter().all(|cut| cut % FRAME == 0));
            assert_eq!(
                cuts_of(&ranges),
                reference_cuts(&samples, CAP, W),
                "{length}"
            );
        }
    }

    #[test]
    fn two_equally_distant_quiet_frames_give_the_earlier_one() {
        assert_eq!(cut_f16(&[(380, 0.0), (420, 0.0)]), [121_600]);
    }

    #[test]
    fn the_quiet_frame_nearest_the_target_is_taken_whichever_side_it_is_on() {
        assert_eq!(cut_f16(&[(390, 0.0), (415, 0.0)]), [124_800]);
        assert_eq!(cut_f16(&[(385, 0.0), (405, 0.0)]), [129_600]);
    }

    #[test]
    fn a_long_silence_is_cut_at_the_frame_nearest_the_target() {
        let silence: Vec<(usize, f32)> = (380..=420).map(|frame| (frame, 0.0)).collect();
        assert_eq!(cut_f16(&silence), [128_000]);
    }

    #[test]
    fn a_valley_that_would_leave_a_piece_over_the_cap_is_not_taken() {
        let samples = with_frames(319_680, &[(450, 0.0)]);
        let ranges = pieces(&samples, CAP, W, 0);
        assert_eq!(cuts_of(&ranges), [159_680]);
        assert_eq!(lengths_of(&ranges), [159_680, 160_000]);
    }

    #[test]
    fn a_valley_beyond_the_previous_cut_plus_the_cap_is_not_taken() {
        let samples = with_frames(400_000, &[(367, 0.0), (870, 0.0)]);
        let ranges = pieces(&samples, CAP, W, 0);
        assert_eq!(cuts_of(&ranges), [117_440, 266_560]);
        assert_eq!(lengths_of(&ranges), [117_440, 149_120, 133_440]);
    }

    #[test]
    fn with_an_overlap_each_later_piece_starts_one_overlap_before_its_cut() {
        let samples = with_frames(384_000, &[(420, 0.0), (830, 0.0)]);
        let ranges = pieces(&samples, CAP, W, 8_000);
        assert_eq!(ranges, [0..134_400, 126_400..265_600, 257_600..384_000]);
        assert!(
            lengths_of(&ranges)
                .iter()
                .all(|length| *length <= CAP + 8_000)
        );
    }

    #[test]
    fn an_empty_buffer_is_one_empty_piece() {
        assert_eq!(pieces(&[], CAP, W, 0), vec_of(0..0));
    }

    #[test]
    fn a_cap_is_off_or_a_length_of_at_least_two_frames() {
        assert_eq!(DecodeCap::parse("off"), Ok(DecodeCap::Off));
        assert_eq!(
            DecodeCap::parse("10"),
            Ok(DecodeCap::Samples {
                cap: CAP,
                overlap: 0
            })
        );
        assert_eq!(DecodeCap::parse("0.04").map(DecodeCap::folds), Ok(false));
        for refused in ["0", "0.039", "-3", "nan", "inf", "ten", "", "OFF"] {
            assert_eq!(DecodeCap::parse(refused), Err(BadCap), "{refused}");
        }
    }

    #[test]
    fn an_overlap_is_not_negative_and_shorter_than_the_cap() {
        assert!(DecodeCap::from_seconds(10.0, 0.5).is_ok_and(DecodeCap::folds));
        assert_eq!(DecodeCap::from_seconds(10.0, -0.5), Err(BadCap));
        assert_eq!(DecodeCap::from_seconds(10.0, 10.0), Err(BadCap));
        assert!(DecodeCap::from_seconds(10.0, 9.99).is_ok());
    }

    #[test]
    fn the_seconds_of_a_cap_read_back() -> Check {
        let cap = DecodeCap::from_seconds(10.0, 0.5)?;
        assert_eq!(cap.cap_seconds(), Some(10.0));
        assert_eq!(cap.overlap_seconds(), 0.5);
        assert_eq!(DecodeCap::Off.cap_seconds(), None);
        assert_eq!(DecodeCap::Off.overlap_seconds(), 0.0);
        assert_eq!(DecodeCap::default(), DecodeCap::Off);
        Ok(())
    }

    #[test]
    fn the_log_record_of_a_cut_is_in_milliseconds() -> Check {
        let cap = DecodeCap::from_seconds(10.0, 0.0)?;
        assert_eq!(
            cap.cut_of(400_000, 3),
            Cut {
                pieces: 3,
                buffer_ms: 25_000,
                cap_ms: 10_000
            }
        );
        Ok(())
    }

    #[test]
    fn a_piece_sits_between_two_stretches_of_the_retry_pad() {
        let mut padded = vec![9.0_f32; 3];
        pad_into(&mut padded, &[1.0, 2.0]);
        assert_eq!(padded.len(), 5_600 + 2 + 5_600);
        assert_eq!(padded[5_599], 0.0);
        assert_eq!(padded[5_600..5_602], [1.0, 2.0]);
        assert_eq!(padded[5_602], 0.0);
        let retry_piece = crate::retry::build_pieces(&[1.0, 2.0], &[(0, 2)]);
        assert_eq!(retry_piece[0].len(), padded.len());
    }

    #[test]
    fn a_fold_runs_over_every_seam() {
        assert_eq!(
            join_sentences(&["今日は晴れ", "晴れです", "ですね"], true),
            "今日は晴れですね"
        );
    }

    #[test]
    fn nothing_to_join_is_an_empty_sentence() {
        assert_eq!(join_sentences::<&str>(&[], false), "");
        assert_eq!(join_sentences(&["", "  "], true), "");
    }
}

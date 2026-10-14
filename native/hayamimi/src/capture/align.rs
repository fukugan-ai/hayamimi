//! Adding two audio sources into one stream. The first source (the
//! microphone) is the clock: for each of its windows, the window of the
//! second source (what the machine plays) that ended closest to the same
//! moment is added, and a second-source window too far away is left out.

use std::collections::VecDeque;

use hayamimi_asr::audio::{SAMPLE_RATE, WINDOW};

/// One window with the moment its last sample was captured, in nanoseconds
/// since the start of the run.
#[derive(Debug, Clone, PartialEq)]
pub struct Packet {
    pub end_ns: u64,
    pub samples: Vec<f32>,
}

/// How far apart two windows may end and still be taken as the same moment:
/// one window and a half.
pub const TOLERANCE_NS: u64 = (WINDOW as u64 * 3 * 1_000_000_000) / (2 * SAMPLE_RATE as u64);

/// The most second-source windows kept: about two seconds, as the first source can pause.
pub const KEPT: usize = 64;

/// The windows of the second source that have not been paired yet, oldest first.
#[derive(Debug, Default)]
pub struct Aligner {
    pending: VecDeque<Packet>,
}

impl Aligner {
    /// Adds a window of the second source. The oldest is dropped past [`KEPT`].
    pub fn absorb(&mut self, packet: Packet) {
        if self.pending.len() == KEPT {
            self.pending.pop_front();
        }
        self.pending.push_back(packet);
    }

    /// Whether a window that ends at `target` or later, within the tolerance, is here.
    pub fn has_candidate_from(&self, target: u64) -> bool {
        let upper = target.saturating_add(TOLERANCE_NS);
        self.pending
            .iter()
            .any(|packet| (target..=upper).contains(&packet.end_ns))
    }

    /// Whether a window past the tolerance is here: none can still arrive in it.
    pub fn has_later_than(&self, target: u64) -> bool {
        let upper = target.saturating_add(TOLERANCE_NS);
        self.pending.iter().any(|packet| packet.end_ns > upper)
    }

    /// Takes the window closest to `target` within the tolerance. Windows
    /// that ended before the tolerance, and those older than the one taken,
    /// are dropped; later ones are kept.
    pub fn take_closest(&mut self, target: u64) -> Option<Packet> {
        let lower = target.saturating_sub(TOLERANCE_NS);
        let upper = target.saturating_add(TOLERANCE_NS);
        while self.pending.front().is_some_and(|p| p.end_ns < lower) {
            self.pending.pop_front();
        }
        let best = self
            .pending
            .iter()
            .take_while(|packet| packet.end_ns <= upper)
            .enumerate()
            .min_by_key(|(_, packet)| packet.end_ns.abs_diff(target))
            .map(|(index, _)| index)?;
        self.pending.drain(..best);
        self.pending.pop_front()
    }
}

/// The window to pass on: the sum of the two, clipped to `[-1, 1]`, or the
/// first alone when there is no second or its length differs.
pub fn mix(first: &[f32], second: Option<&Packet>) -> Vec<f32> {
    match second {
        Some(other) if other.samples.len() == first.len() => first
            .iter()
            .zip(&other.samples)
            .map(|(a, b)| (a + b).clamp(-1.0, 1.0))
            .collect(),
        _ => first.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(end_ms: u64, value: f32) -> Packet {
        Packet {
            end_ns: end_ms * 1_000_000,
            samples: vec![value; 4],
        }
    }

    fn ms(value: u64) -> u64 {
        value * 1_000_000
    }

    #[test]
    fn the_tolerance_is_one_window_and_a_half() {
        assert_eq!(TOLERANCE_NS, 48_000_000);
    }

    #[test]
    fn the_closest_window_within_the_tolerance_is_taken() {
        let mut aligner = Aligner::default();
        aligner.absorb(packet(980, 0.1));
        aligner.absorb(packet(1010, 0.2));
        aligner.absorb(packet(1040, 0.3));
        let taken = aligner.take_closest(ms(1000));
        assert_eq!(taken.map(|p| p.samples[0]), Some(0.2));
    }

    #[test]
    fn an_older_window_than_the_one_taken_is_dropped_and_a_later_one_kept() {
        let mut aligner = Aligner::default();
        aligner.absorb(packet(990, 0.1));
        aligner.absorb(packet(1005, 0.2));
        aligner.absorb(packet(1030, 0.3));
        assert!(aligner.take_closest(ms(1000)).is_some());
        // 990 went with the one taken; 1030 is still there for the next target.
        assert_eq!(
            aligner.take_closest(ms(1030)).map(|p| p.samples[0]),
            Some(0.3)
        );
        assert!(aligner.take_closest(ms(1030)).is_none());
    }

    #[test]
    fn a_window_exactly_at_the_tolerance_is_taken_and_one_past_it_is_not() {
        let mut aligner = Aligner::default();
        aligner.absorb(packet(1048, 0.5));
        assert!(aligner.take_closest(ms(1000)).is_some());
        aligner.absorb(packet(1049, 0.5));
        assert!(aligner.take_closest(ms(1000)).is_none());
    }

    #[test]
    fn a_stale_window_is_dropped_and_the_future_one_stays() {
        let mut aligner = Aligner::default();
        aligner.absorb(packet(900, 0.1));
        aligner.absorb(packet(2000, 0.2));
        assert!(aligner.take_closest(ms(1000)).is_none());
        assert_eq!(
            aligner.take_closest(ms(2000)).map(|p| p.samples[0]),
            Some(0.2)
        );
    }

    #[test]
    fn nothing_pending_gives_nothing() {
        assert!(Aligner::default().take_closest(ms(5)).is_none());
        assert!(!Aligner::default().has_candidate_from(ms(5)));
        assert!(!Aligner::default().has_later_than(ms(5)));
    }

    #[test]
    fn a_candidate_must_not_be_in_the_past() {
        let mut aligner = Aligner::default();
        aligner.absorb(packet(990, 0.1));
        assert!(!aligner.has_candidate_from(ms(1000)));
        aligner.absorb(packet(1000, 0.1));
        assert!(aligner.has_candidate_from(ms(1000)));
    }

    #[test]
    fn a_window_past_the_tolerance_means_none_can_arrive_behind_it() {
        let mut aligner = Aligner::default();
        aligner.absorb(packet(1048, 0.1));
        assert!(!aligner.has_later_than(ms(1000)));
        aligner.absorb(packet(1049, 0.1));
        assert!(aligner.has_later_than(ms(1000)));
    }

    #[test]
    fn only_the_newest_windows_are_kept() {
        let mut aligner = Aligner::default();
        for index in 0..(KEPT as u64 + 5) {
            aligner.absorb(packet(index, 0.0));
        }
        assert_eq!(aligner.pending.len(), KEPT);
        assert_eq!(aligner.pending.front().map(|p| p.end_ns), Some(ms(5)));
    }

    #[test]
    fn the_sum_is_clipped_and_a_length_mismatch_leaves_the_first_alone() {
        let first = [0.75_f32, -0.75, 0.25, 0.0];
        let same = Packet {
            end_ns: 0,
            samples: vec![0.5, -0.5, 0.25, 0.0],
        };
        assert_eq!(mix(&first, Some(&same)), vec![1.0, -1.0, 0.5, 0.0]);
        let shorter = Packet {
            end_ns: 0,
            samples: vec![0.5],
        };
        assert_eq!(mix(&first, Some(&shorter)), first.to_vec());
        assert_eq!(mix(&first, None), first.to_vec());
    }
}

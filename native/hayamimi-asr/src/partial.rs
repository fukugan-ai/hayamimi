//! Drafts of the utterance in progress.
//!
//! While the detector is inside speech, the audio so far is decoded again
//! about every half second of input, so that the screen can show words before
//! the utterance ends. The clock here counts audio, not wall time: a file
//! played at full speed and a microphone are drafted at the same points of
//! the signal.

use crate::audio::SAMPLE_RATE;

/// Samples of input between two drafts: half a second.
pub const EVERY_SAMPLES: usize = SAMPLE_RATE as usize / 2;
/// Speech shorter than this has no draft: half a second.
pub const MIN_SPEECH_SAMPLES: usize = SAMPLE_RATE as usize / 2;
/// The most audio one draft decodes: the last eight seconds.
pub const MAX_SAMPLES: usize = SAMPLE_RATE as usize * 8;

/// When a draft is due, and where the speech it covers began.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DraftClock {
    speech_since: Option<usize>,
    last: usize,
}

impl DraftClock {
    pub fn new() -> Self {
        Self::default()
    }

    /// One window has been fed. `window_start` is its first sample on the
    /// timeline of the whole input, and `speech` whether the detector is
    /// inside speech once the window is in.
    pub fn observe(&mut self, speech: bool, window_start: usize) {
        if !speech {
            self.speech_since = None;
        } else if self.speech_since.is_none() {
            self.speech_since = Some(window_start);
        }
    }

    /// The start of the speech to draft when a draft is due at input position
    /// `now`, or `None`. A due draft is taken even when the speech is still
    /// too short for one, so that the next try is half a second later.
    pub fn due(&mut self, now: usize) -> Option<usize> {
        let since = self.speech_since?;
        if now.saturating_sub(self.last) < EVERY_SAMPLES {
            return None;
        }
        self.last = now;
        (now.saturating_sub(since) >= MIN_SPEECH_SAMPLES).then_some(since)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HALF: usize = EVERY_SAMPLES;

    fn speaking_from(start: usize) -> DraftClock {
        let mut clock = DraftClock::new();
        clock.observe(true, start);
        clock
    }

    #[test]
    fn nothing_is_drafted_outside_speech() {
        let mut clock = DraftClock::new();
        assert_eq!(clock.due(10 * HALF), None);
        clock.observe(false, 0);
        assert_eq!(clock.due(10 * HALF), None);
    }

    #[test]
    fn the_first_draft_waits_for_half_a_second_of_speech() {
        let mut clock = speaking_from(4 * HALF);
        // Speech began at 4 halves: a draft at 4 halves + 1 sample is too short.
        assert_eq!(clock.due(4 * HALF + 1), None);
        // The refused try took the draft, so the next one is half a second later.
        assert_eq!(clock.due(4 * HALF + 2), None);
        assert_eq!(clock.due(5 * HALF + 1), Some(4 * HALF));
    }

    #[test]
    fn drafts_come_every_half_second_of_input() {
        let mut clock = speaking_from(0);
        assert_eq!(clock.due(HALF), Some(0));
        assert_eq!(clock.due(HALF + HALF - 1), None);
        assert_eq!(clock.due(2 * HALF), Some(0));
    }

    #[test]
    fn the_start_stays_where_speech_began_until_speech_ends() {
        let mut clock = speaking_from(100);
        clock.observe(true, 612);
        assert_eq!(clock.due(HALF + 100), Some(100));
        clock.observe(false, 700);
        assert_eq!(clock.due(4 * HALF), None);
        clock.observe(true, 4 * HALF);
        assert_eq!(clock.due(5 * HALF), Some(4 * HALF));
    }

    #[test]
    fn the_limits_are_in_seconds_of_16k_audio() {
        assert_eq!(EVERY_SAMPLES, 8_000);
        assert_eq!(MIN_SPEECH_SAMPLES, 8_000);
        assert_eq!(MAX_SAMPLES, 128_000);
    }
}

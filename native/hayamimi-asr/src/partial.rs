//! Drafts of the utterance in progress.
//!
//! While the detector is inside speech, the audio so far is decoded again
//! about every half second of input, so that the screen can show words before
//! the utterance ends. The clock here counts audio, not wall time: a file
//! played at full speed and a microphone are drafted at the same points of
//! the signal.

use std::time::Instant;

use crate::audio::{History, SAMPLE_RATE};
use crate::lid::Identify;
use crate::route::{Decode, RouteError, Router, Tier, requested_tier};

/// Samples of input between two drafts: half a second.
pub const EVERY_SAMPLES: usize = SAMPLE_RATE as usize / 2;
/// Speech shorter than this has no draft: half a second.
pub const MIN_SPEECH_SAMPLES: usize = SAMPLE_RATE as usize / 2;
/// The most audio one draft decodes: the last eight seconds.
pub const MAX_SAMPLES: usize = SAMPLE_RATE as usize * 8;
/// The voice of an utterance, in samples, at which its language is asked once
/// for the drafts: two seconds. A shorter voice is too short for the
/// identifier to name the language of the speaker reliably.
pub const MIDWAY_SAMPLES: usize = SAMPLE_RATE as usize * 2;
/// The voice of an utterance, in samples, at which its language is asked a
/// second time, for the final: four seconds, the length of the clip the
/// final's own question reads ([`crate::lid::LID_MAX_SECONDS`]).
pub const MIDWAY_CLOSING_SAMPLES: usize = SAMPLE_RATE as usize * 4;
/// How far after the start of a segment the voice may begin, in samples, for
/// the closing answer to be the segment's: half a second. The detector places
/// the start of the voice about 0.3 s after the start of the segment.
pub const CLOSING_ALIGN_SAMPLES: usize = SAMPLE_RATE as usize / 2;

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

    /// Where the speech in progress began, or `None` outside speech.
    pub fn speech_start(&self) -> Option<usize> {
        self.speech_since
    }

    /// Moves the start of the speech in progress to `at`, once the text up to
    /// there is final. Does nothing outside speech.
    pub fn restart_at(&mut self, at: usize) {
        if self.speech_since.is_some() {
            self.speech_since = Some(at);
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

/// What the question put to the identifier in the middle of one utterance came to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Midway {
    /// Not asked yet: the voice is shorter than [`MIDWAY_SAMPLES`], or the run does not ask.
    #[default]
    Waiting,
    /// The identifier named `ja` or `en`.
    Heard(&'static str),
    /// The identifier named a language outside the pair.
    Outside,
    /// The identifier failed.
    Failed,
}

/// One question put to the identifier in the middle of an utterance: what it
/// came to, how much voice it was given and how long it took.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MidwayAsked {
    pub answer: Midway,
    /// The samples of audio the identifier was given.
    pub voice_samples: usize,
    pub lid_ms: f64,
}

/// The language of the utterance in progress, asked in a pair run. It is asked
/// once for the drafts, when the voice first reaches [`MIDWAY_SAMPLES`], with
/// all the voice so far; and once for the final, when the voice first reaches
/// [`MIDWAY_CLOSING_SAMPLES`], with the clip the final's own question would
/// read. Both answers last until the speech ends or is cut again.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MidwayLid {
    since: Option<usize>,
    answer: Midway,
    closing: Midway,
}

impl MidwayLid {
    pub fn new() -> Self {
        Self::default()
    }

    /// What the question came to for the utterance in progress.
    pub fn answer(&self) -> Midway {
        self.answer
    }

    /// The language heard in the middle of the utterance, when it is `ja` or `en`.
    pub fn heard(&self) -> Option<&'static str> {
        match self.answer {
            Midway::Heard(lang) => Some(lang),
            Midway::Waiting | Midway::Outside | Midway::Failed => None,
        }
    }

    /// The language the four-second question named, when it is `ja` or `en`
    /// and the voice it was asked about began no earlier than `from`, the start
    /// of the segment that is closing, and at most [`CLOSING_ALIGN_SAMPLES`]
    /// after it. A voice that began before the segment, or long after its
    /// start, belongs to another reading.
    pub fn closing_for(&self, from: usize) -> Option<&'static str> {
        let Midway::Heard(lang) = self.closing else {
            return None;
        };
        let ahead = self.since?.checked_sub(from)?;
        (ahead <= CLOSING_ALIGN_SAMPLES).then_some(lang)
    }

    /// One window has been fed. `speech_start` is where the speech in progress
    /// began (see [`DraftClock::speech_start`]). A new start is a new utterance
    /// and forgets the answers. The identifier is asked right after the window
    /// that takes the voice to [`MIDWAY_SAMPLES`] or more, for the drafts, and
    /// right after the window that takes it to [`MIDWAY_CLOSING_SAMPLES`] or
    /// more, for the final. Each is asked once, and only in a pair run whose
    /// policy asks for it. When both fall due in one call the identifier is
    /// asked once and both answers are that one. A failed question is kept as
    /// [`Midway::Failed`] and not asked again for this utterance. Gives back
    /// the question when this window asked it, and none otherwise.
    pub fn observe<D: Decode, L: Identify>(
        &mut self,
        router: &mut Router<D, L>,
        history: &History,
        speech_start: Option<usize>,
    ) -> Option<MidwayAsked> {
        if self.since != speech_start {
            *self = MidwayLid {
                since: speech_start,
                ..MidwayLid::default()
            };
        }
        let start = speech_start?;
        let voice = history.end().saturating_sub(start);
        let policy = router.policy();
        let for_drafts = policy.pair
            && policy.partial_lid
            && self.answer == Midway::Waiting
            && voice >= MIDWAY_SAMPLES;
        let for_final = policy.pair
            && policy.final_midway
            && self.closing == Midway::Waiting
            && voice >= MIDWAY_CLOSING_SAMPLES;
        if !for_drafts && !for_final {
            return None;
        }
        // The question for the final reads what the final's own question would:
        // the audio from up to a second before the voice. The drafts' question
        // reads the voice alone.
        let audio = if for_final {
            history.draft(start, usize::MAX)
        } else {
            history.slice(start, history.end())
        };
        let asked_at = Instant::now();
        let answer = answer_of(router.identify(&audio));
        if for_drafts {
            self.answer = answer;
        }
        if for_final {
            self.closing = answer;
        }
        Some(MidwayAsked {
            answer,
            voice_samples: audio.len(),
            lid_ms: asked_at.elapsed().as_secs_f64() * 1000.0,
        })
    }

    /// The tier a draft is read with: the language heard in the middle of the
    /// utterance, else the language of the last final, else Japanese (before
    /// the first final there is none).
    pub fn tier<D: Decode, L: Identify>(&self, router: &Router<D, L>) -> Tier {
        match self.heard().or_else(|| router.last_lang()) {
            Some(lang) => requested_tier(lang, router.policy().en_tier),
            None => Tier::Rz,
        }
    }
}

/// The identifier's answer for a pair run: `ja` and `en` are heard, any other
/// language is outside the pair, and an error is a failure.
fn answer_of(said: Result<String, RouteError>) -> Midway {
    match said {
        Ok(code) if code == "ja" => Midway::Heard("ja"),
        Ok(code) if code == "en" => Midway::Heard("en"),
        Ok(_) => Midway::Outside,
        Err(_) => Midway::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lid::LID_MAX_SECONDS;

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
    fn the_start_can_be_read_and_moved_inside_speech_only() {
        let mut clock = DraftClock::new();
        assert_eq!(clock.speech_start(), None);
        clock.restart_at(500);
        assert_eq!(clock.speech_start(), None);
        clock.observe(true, 100);
        assert_eq!(clock.speech_start(), Some(100));
        clock.restart_at(500);
        assert_eq!(clock.speech_start(), Some(500));
    }

    #[test]
    fn the_limits_are_in_seconds_of_16k_audio() {
        assert_eq!(EVERY_SAMPLES, 8_000);
        assert_eq!(MIN_SPEECH_SAMPLES, 8_000);
        assert_eq!(MAX_SAMPLES, 128_000);
        assert_eq!(MIDWAY_SAMPLES, 32_000);
    }

    /// T-BR-37-015
    #[test]
    fn the_four_second_limits_match_what_the_final_identifier_reads() {
        assert_eq!(MIDWAY_CLOSING_SAMPLES, 64_000);
        assert_eq!(
            MIDWAY_CLOSING_SAMPLES,
            (LID_MAX_SECONDS * SAMPLE_RATE as f32) as usize
        );
        assert_eq!(CLOSING_ALIGN_SAMPLES, 8_000);
    }
}

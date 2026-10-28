//! A reading of the speech taken as soon as the voice falls quiet, kept for the
//! early notice that follows.
//!
//! The early detector tells that a stretch of voice ended some hundreds of
//! milliseconds after the last sound, and the speech is read only then.
//! [`crate::quiet::Quiet`] tells at once that the level of the voice fell. The
//! engine reads the speech so far at that moment and keeps the reading here.
//! When the notice comes and no loud window has come since, it takes the
//! reading instead of making its own; otherwise the reading is dropped, and
//! the language the session had settled on is put back as it was before the
//! reading, so that a reading made twice is counted once. Nothing is ended or
//! finalised by a reading alone: what ends an utterance is still the detectors.

use crate::audio::SAMPLE_RATE;
use crate::endpoint;
use crate::route::SessionMark;

/// The quiet between the fall and the early notice that is taken for granted
/// when the length of the speech is judged: 200 ms. The notice comes after the
/// fall, so the speech it reads is longer by at least that much.
pub const LEAD_SAMPLES: usize = SAMPLE_RATE as usize / 5;

/// Whether speech of `span` samples, read at the fall, can still be speech that
/// an early notice finalises. Shorter speech is kept by the notice, not
/// finalised, and a reading of it ahead would not shorten any wait.
pub fn worth_reading(span: usize) -> bool {
    endpoint::long_enough_to_close(span + LEAD_SAMPLES)
}

/// A reading of the speech from sample `from` to sample `end`.
#[derive(Debug, Clone)]
struct Kept<T> {
    from: usize,
    /// The language the identifier had named for the speech, when it had.
    midway: Option<&'static str>,
    end: usize,
    read: T,
    /// The session as it was before the reading.
    before: SessionMark,
}

/// What asking for the reading of some speech found.
#[derive(Debug, Clone)]
pub enum Taken<T> {
    /// The reading of that speech, with nothing heard since: it stands.
    Used(T),
    /// A reading was kept, but of other speech or with a voice heard since. It
    /// is gone; the mark is the session as it was before that reading.
    Dropped(SessionMark),
    /// No reading was kept.
    Nothing,
}

/// The reading taken at the last fall, until a notice takes it.
#[derive(Debug, Clone)]
pub struct Ahead<T> {
    kept: Option<Kept<T>>,
}

impl<T> Default for Ahead<T> {
    fn default() -> Self {
        Ahead { kept: None }
    }
}

impl<T> Ahead<T> {
    /// Keeps the reading of the speech `from..end`, made with `before` as the
    /// session. The reading kept before has been dropped by the caller.
    pub fn keep(
        &mut self,
        from: usize,
        midway: Option<&'static str>,
        end: usize,
        read: T,
        before: SessionMark,
    ) {
        self.kept = Some(Kept {
            from,
            midway,
            end,
            read,
            before,
        });
    }

    /// The reading, when it is of the speech that starts at `from` with the
    /// language `midway` and no window louder than a fall came after it
    /// (`loud_end` is where the last loud window ended). The reading is taken
    /// either way.
    pub fn take_for(
        &mut self,
        from: usize,
        midway: Option<&'static str>,
        loud_end: usize,
    ) -> Taken<T> {
        match self.kept.take() {
            None => Taken::Nothing,
            Some(kept) if kept.from == from && kept.midway == midway && loud_end <= kept.end => {
                Taken::Used(kept.read)
            }
            Some(kept) => Taken::Dropped(kept.before),
        }
    }

    /// Drops the reading, if there is one, and returns the session as it was before it.
    pub fn drop_kept(&mut self) -> Option<SessionMark> {
        self.kept.take().map(|kept| kept.before)
    }

    /// Forgets the reading without returning the session: a new session has
    /// begun and the one the mark belongs to is gone.
    pub fn clear(&mut self) {
        self.kept = None;
    }

    /// Whether a reading is kept.
    pub fn is_kept(&self) -> bool {
        self.kept.is_some()
    }
}

/// What the engine did with a fall.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// The speech so far was read at the fall.
    Read,
    /// An early notice took the reading as its own.
    Used,
}

impl Step {
    /// The name the log gives the step.
    pub fn name(self) -> &'static str {
        match self {
            Step::Read => "read",
            Step::Used => "used",
        }
    }
}

/// One step of the reading ahead, for the log.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Note {
    pub step: Step,
    /// The speech the reading covers, in milliseconds.
    pub span_ms: f64,
    /// The time the reading took, in milliseconds: the time it held the
    /// feed back when read, the time it spared the notice when used.
    pub read_ms: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECOND: usize = SAMPLE_RATE as usize;

    fn kept_reading() -> Ahead<&'static str> {
        let mut ahead = Ahead::default();
        ahead.keep(
            SECOND,
            Some("ja"),
            4 * SECOND,
            "text",
            SessionMark::default(),
        );
        ahead
    }

    /// T-BR-43-001
    #[test]
    fn a_notice_for_the_same_speech_takes_the_reading() {
        let mut ahead = kept_reading();
        assert!(matches!(
            ahead.take_for(SECOND, Some("ja"), 3 * SECOND),
            Taken::Used("text")
        ));
        assert!(!ahead.is_kept());
    }

    /// T-BR-43-002
    #[test]
    fn a_reading_is_taken_once() {
        let mut ahead = kept_reading();
        assert!(matches!(
            ahead.take_for(SECOND, Some("ja"), 3 * SECOND),
            Taken::Used(_)
        ));
        assert!(matches!(
            ahead.take_for(SECOND, Some("ja"), 3 * SECOND),
            Taken::Nothing
        ));
    }

    /// T-BR-43-003
    #[test]
    fn a_loud_window_after_the_reading_drops_it() {
        let mut ahead = kept_reading();
        assert!(matches!(
            ahead.take_for(SECOND, Some("ja"), 4 * SECOND + 1),
            Taken::Dropped(_)
        ));
        assert!(!ahead.is_kept());
    }

    /// T-BR-43-004
    #[test]
    fn a_loud_window_that_ended_where_the_reading_ended_does_not_drop_it() {
        let mut ahead = kept_reading();
        assert!(matches!(
            ahead.take_for(SECOND, Some("ja"), 4 * SECOND),
            Taken::Used(_)
        ));
    }

    /// T-BR-43-005
    #[test]
    fn speech_that_starts_elsewhere_drops_the_reading() {
        let mut ahead = kept_reading();
        assert!(matches!(
            ahead.take_for(SECOND + 1, Some("ja"), 3 * SECOND),
            Taken::Dropped(_)
        ));
    }

    /// T-BR-43-006
    #[test]
    fn a_language_named_since_drops_the_reading() {
        for midway in [None, Some("en")] {
            let mut ahead = kept_reading();
            assert!(
                matches!(
                    ahead.take_for(SECOND, midway, 3 * SECOND),
                    Taken::Dropped(_)
                ),
                "{midway:?}"
            );
        }
    }

    /// T-BR-43-007
    #[test]
    fn nothing_kept_is_nothing_taken() {
        let mut ahead: Ahead<u8> = Ahead::default();
        assert!(matches!(ahead.take_for(0, None, 0), Taken::Nothing));
        assert!(ahead.drop_kept().is_none());
    }

    /// T-BR-43-008
    #[test]
    fn dropping_returns_the_session_of_before_the_reading_and_clearing_does_not() {
        let mut ahead = kept_reading();
        assert!(ahead.drop_kept().is_some());
        assert!(!ahead.is_kept());
        let mut ahead = kept_reading();
        ahead.clear();
        assert!(!ahead.is_kept());
        assert!(ahead.drop_kept().is_none());
    }

    /// T-BR-43-009
    #[test]
    fn speech_is_worth_reading_ahead_when_the_notice_would_make_it_long_enough() {
        let min = SECOND;
        assert!(worth_reading(min - LEAD_SAMPLES));
        assert!(!worth_reading(min - LEAD_SAMPLES - 1));
        assert!(worth_reading(10 * SECOND));
        assert!(!worth_reading(0));
    }

    /// T-BR-43-010
    #[test]
    fn the_lead_is_200_ms() {
        assert_eq!(LEAD_SAMPLES, 3200);
    }

    /// T-BR-43-033
    #[test]
    fn the_steps_are_named_for_the_log() {
        assert_eq!(Step::Read.name(), "read");
        assert_eq!(Step::Used.name(), "used");
    }
}

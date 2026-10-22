//! `--mode pair`: a live segment is read as `ja` or `en`, whatever the
//! identifier says, and the session's language is neither consulted nor held.
//!
//! The choice takes the first of five answers:
//!
//! 1. the identifier names `ja` or `en`;
//! 2. SenseVoice's own tag on the same audio names `ja` or `en` (the identifier
//!    named another language, answered nothing, failed, or is not there);
//! 3. the letters of SenseVoice's text: `ja` when kana and kanji are more than
//!    three tenths of them, `en` otherwise;
//! 4. `ja`, when the text has no letter or SenseVoice is not installed;
//! 5. `ja`, when SenseVoice's reading failed. The segment goes on.
//!
//! Steps 2 to 5 leave a [`LangFallback`] on the transcript, so that the run can
//! say how often the identifier alone was not enough.

use std::time::Instant;

use crate::lid::{Identify, pair_script_lang};

use super::RouteError;
use super::backend::Decode;
use super::router::{Router, millis};
use super::transcribe::{Chosen, KNOWN_LANG_MAX_S, Probe, Request, Standing};

/// The answer that chose the language when the identifier did not name `ja` or `en`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallbackStep {
    /// SenseVoice's tag named `ja` or `en`.
    SvTag,
    /// The letters of SenseVoice's text decided.
    Script,
    /// There was no text to read letters from, or SenseVoice is not installed.
    Default,
    /// SenseVoice's reading failed.
    SvFailed,
}

impl FallbackStep {
    /// The name the log and the records use.
    pub fn name(self) -> &'static str {
        match self {
            FallbackStep::SvTag => "sv_tag",
            FallbackStep::Script => "script",
            FallbackStep::Default => "default",
            FallbackStep::SvFailed => "sv_failed",
        }
    }
}

/// A segment whose language the identifier alone did not settle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LangFallback {
    pub step: FallbackStep,
    /// What the identifier answered; `None` when it answered nothing, failed, or is not there.
    pub lid: Option<String>,
}

/// What SenseVoice's reading of a segment came to.
enum SvReading {
    Read,
    /// The model is not installed.
    Missing,
    Failed,
}

impl<D: Decode, L: Identify> Router<D, L> {
    /// Whether the pair rule chooses the language of this segment: a live
    /// segment of a route that no language is forced on.
    pub(super) fn pairs(&self, request: &Request<'_>) -> bool {
        self.policy.pair && self.policy.forced_lang.is_none() && request.live
    }

    /// The language of a live segment in `ja` or `en`, and the identifier's time.
    pub(super) fn choose_pair(
        &mut self,
        request: &Request<'_>,
        probe: &mut Probe,
    ) -> Result<(Chosen, f64), RouteError> {
        let (heard, lid_ms) = self.heard(request);
        let (lang, fallback) = self.pair_language(heard, request.samples, probe)?;
        let chosen = Chosen {
            lang,
            suppress_fallback: false,
            suppress_seed: false,
            fallback,
        };
        Ok((chosen, lid_ms))
    }

    /// What the identifier answers for the segment: an early detection for a
    /// short segment, else the identifier's own answer. An answer that is empty,
    /// an identifier that fails and an identifier that is not there say nothing.
    fn heard(&self, request: &Request<'_>) -> (Option<String>, f64) {
        if let Some(known) = request.known_lang
            && request
                .speech_s
                .is_none_or(|speech| speech < KNOWN_LANG_MAX_S)
        {
            return (Some(known.to_owned()), 0.0);
        }
        let Some(lid) = &self.lid else {
            return (None, 0.0);
        };
        let started = Instant::now();
        // The failure is not lost: the step that follows records `lid` as absent.
        let answer = lid
            .identify(request.samples)
            .ok()
            .filter(|lang| !lang.is_empty());
        (answer, millis(started))
    }

    fn pair_language(
        &mut self,
        heard: Option<String>,
        samples: &[f32],
        probe: &mut Probe,
    ) -> Result<(String, Option<LangFallback>), RouteError> {
        if let Some(lang) = heard.as_deref().filter(|lang| matches!(*lang, "ja" | "en")) {
            return Ok((lang.to_owned(), None));
        }
        let (lang, step) = self.pair_by_sense_voice(samples, probe)?;
        Ok((lang.to_owned(), Some(LangFallback { step, lid: heard })))
    }

    /// Steps 2 to 5. A failed SenseVoice reading is the only failure that does
    /// not stop the segment, and the tier is asked again for the next one.
    fn pair_by_sense_voice(
        &mut self,
        samples: &[f32],
        probe: &mut Probe,
    ) -> Result<(&'static str, FallbackStep), RouteError> {
        match self.sense_voice_reading(samples, probe)? {
            SvReading::Failed => Ok(("ja", FallbackStep::SvFailed)),
            SvReading::Missing => Ok(("ja", FallbackStep::Default)),
            SvReading::Read => Ok(judge_reading(probe)),
        }
    }

    fn sense_voice_reading(
        &mut self,
        samples: &[f32],
        probe: &mut Probe,
    ) -> Result<SvReading, RouteError> {
        match self.sv_probe(probe, samples) {
            Ok(()) if probe.result.is_some() => Ok(SvReading::Read),
            Ok(()) => Ok(SvReading::Missing),
            Err(RouteError::Decode(_)) => Ok(SvReading::Failed),
            Err(other) => Err(other),
        }
    }

    /// The correction by script, kept inside `ja` and `en`: a text whose script
    /// names another language keeps the language it was chosen in.
    pub(super) fn correct_within_pair(
        &mut self,
        read: &mut Standing,
        corrected: &str,
        samples: &[f32],
    ) -> Result<(), RouteError> {
        if !matches!(corrected, "ja" | "en") {
            return Ok(());
        }
        self.read_again_as(read, corrected, samples)
    }
}

/// Steps 2 and 3 on a reading that was made.
fn judge_reading(probe: &Probe) -> (&'static str, FallbackStep) {
    if let Some(tag) = probe.lang().filter(|tag| matches!(*tag, "ja" | "en")) {
        return (tag, FallbackStep::SvTag);
    }
    let text = probe.result.as_ref().map(|read| read.text.as_str());
    match text.and_then(pair_script_lang) {
        Some(lang) => (lang, FallbackStep::Script),
        None => ("ja", FallbackStep::Default),
    }
}

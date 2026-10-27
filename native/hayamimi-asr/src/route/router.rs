//! The router: its settings, the state of one session, the decode through the
//! tiers that give way to each other, and the end of its life.

use std::time::Instant;

use crate::endpoint::HeldReuse;
use crate::fallback::{Fallback, Reason, Stage, To};
use crate::lid::Identify;

use super::RouteError;
use super::backend::{Decode, DecodeError, Decoded};
use super::second::SECOND_OPINION_THRESHOLD;
use super::sticky::Pending;
use super::tiers::{EnTier, TIER0_LANG, Tier, fallback_chain, is_routable, requested_tier};

/// How the router switches languages and what it asks for besides reading.
#[derive(Debug, Clone, PartialEq)]
pub struct Policy {
    /// Every segment is read in this language, without identification or switching.
    pub forced_lang: Option<String>,
    /// A switch among the five languages SenseVoice covers needs its identifier to agree.
    pub dual_confirm: bool,
    /// Detections of one new language in a row that confirm a switch, where the
    /// dual confirmation does not apply. At least one.
    pub lid_switch_confirm: u32,
    /// A new-language detection shorter than this many seconds does not count.
    pub min_switch_s: f64,
    pub en_tier: EnTier,
    /// The Japanese second opinion is asked for refine decodes.
    pub second_opinion: bool,
    /// Mutual error rate at or below which the second opinion is adopted.
    pub agree_threshold: f64,
    /// A live segment is read as `ja` or `en`, whatever the identifier says;
    /// the session's language is not consulted.
    pub pair: bool,
    /// In a pair run, the drafts of an utterance are read in the language the
    /// identifier names once the voice reaches two seconds (see
    /// [`crate::partial::MidwayLid`]), instead of the language of the last final.
    pub partial_lid: bool,
    /// In a pair run, a final whose voice reached four seconds takes the
    /// language the identifier named at that point (see
    /// [`crate::partial::MidwayLid::closing_for`]), instead of asking again
    /// when the segment closes.
    pub final_midway: bool,
    /// How far a sentence the early detector kept may have been read past the
    /// end of the segment and still be the segment's text.
    pub held_reuse: HeldReuse,
}

impl Policy {
    /// The default mode: dual confirmation, and two long detections for the rest.
    pub fn balanced() -> Policy {
        Policy {
            forced_lang: None,
            dual_confirm: true,
            lid_switch_confirm: 2,
            min_switch_s: 2.0,
            en_tier: EnTier::V3,
            second_opinion: false,
            agree_threshold: SECOND_OPINION_THRESHOLD,
            pair: false,
            partial_lid: false,
            final_midway: false,
            held_reuse: HeldReuse::Covering,
        }
    }

    /// Switches at the first detection of a new language, however short.
    pub fn fast() -> Policy {
        Policy {
            dual_confirm: false,
            lid_switch_confirm: 1,
            min_switch_s: 0.0,
            ..Policy::balanced()
        }
    }

    /// For two people taking turns in Japanese and English: every segment
    /// follows its own detection, and a detection that is not `ja` or `en`
    /// falls to one of the two instead of reaching a third language.
    pub fn pair() -> Policy {
        Policy {
            pair: true,
            partial_lid: true,
            final_midway: true,
            ..Policy::fast()
        }
    }

    /// Every segment in one language.
    pub fn single(lang: &str) -> Policy {
        Policy {
            forced_lang: Some(lang.to_owned()),
            ..Policy::balanced()
        }
    }
}

impl Default for Policy {
    fn default() -> Policy {
        Policy::balanced()
    }
}

/// A step the router took off the design, for the log and the screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteNotice {
    /// A tier that was asked for is not installed; `used` read in its place.
    Substituted { requested: Tier, used: Tier },
    /// A stage the router asked for was left out.
    Left(Fallback),
}

/// What the session has settled on so far.
#[derive(Debug, Default)]
pub(super) struct Session {
    /// The language of the last segment that produced text.
    pub(super) last_lang: Option<String>,
    /// A new language waiting for its confirmations.
    pub(super) pending: Option<Pending>,
}

pub struct Router<D: Decode, L: Identify> {
    pub(super) decoder: D,
    /// `None` when the identifier is left out: segments are then read in the
    /// session's language, or in the first tier's.
    pub(super) lid: Option<L>,
    pub(super) policy: Policy,
    pub(super) session: Session,
    /// Tiers whose model proved not to be installed.
    unavailable: Vec<Tier>,
    /// Tiers whose substitution was already reported.
    substituted: Vec<Tier>,
    notices: Vec<RouteNotice>,
    second_opinion_reported: bool,
    closed: bool,
}

pub(super) fn millis(since: Instant) -> f64 {
    since.elapsed().as_secs_f64() * 1000.0
}

impl<D: Decode, L: Identify> Router<D, L> {
    pub fn new(decoder: D, lid: Option<L>, policy: Policy) -> Router<D, L> {
        Router {
            decoder,
            lid,
            policy,
            session: Session::default(),
            unavailable: Vec::new(),
            substituted: Vec::new(),
            notices: Vec::new(),
            second_opinion_reported: false,
            closed: false,
        }
    }

    /// The recognizers behind the router.
    pub fn decoder(&self) -> &D {
        &self.decoder
    }

    /// The recognizers behind the router, for what they report about themselves.
    pub fn decoder_mut(&mut self) -> &mut D {
        &mut self.decoder
    }

    pub fn policy(&self) -> &Policy {
        &self.policy
    }

    /// The language of the last segment that produced text.
    pub fn last_lang(&self) -> Option<&str> {
        self.session.last_lang.as_deref()
    }

    /// The steps taken off the design since the last call.
    pub fn take_notices(&mut self) -> Vec<RouteNotice> {
        std::mem::take(&mut self.notices)
    }

    pub(super) fn check_open(&self) -> Result<(), RouteError> {
        if self.closed {
            Err(RouteError::Closed)
        } else {
            Ok(())
        }
    }

    /// Clears the session's language and the candidate for the next one. Call
    /// it between unrelated audio streams: without it the first segment of a
    /// new stream is a switch away from whatever the last stream said.
    pub fn reset_session(&mut self) {
        self.session = Session::default();
    }

    /// Reads every segment in `lang`, or in the identified language when `None`.
    /// The session is reset either way.
    pub fn set_forced_lang(&mut self, lang: Option<&str>) -> Result<(), RouteError> {
        if let Some(code) = lang
            && !is_routable(code)
        {
            return Err(RouteError::Invalid(format!(
                "unsupported language code: {code:?}"
            )));
        }
        self.policy.forced_lang = lang.map(str::to_owned);
        self.reset_session();
        Ok(())
    }

    pub fn set_dual_confirm(&mut self, enabled: bool) {
        self.policy.dual_confirm = enabled;
    }

    pub fn set_lid_switch_confirm(&mut self, count: u32) -> Result<(), RouteError> {
        if count < 1 {
            return Err(RouteError::Invalid(format!(
                "lid_switch_confirm must be >= 1, got {count}"
            )));
        }
        self.policy.lid_switch_confirm = count;
        Ok(())
    }

    pub fn set_min_switch_s(&mut self, seconds: f64) -> Result<(), RouteError> {
        if seconds.is_nan() || seconds < 0.0 {
            return Err(RouteError::Invalid(format!(
                "min_switch_s must be >= 0, got {seconds}"
            )));
        }
        self.policy.min_switch_s = seconds;
        Ok(())
    }

    pub(super) fn is_unavailable(&self, tier: Tier) -> bool {
        self.unavailable.contains(&tier)
    }

    /// Decodes with one tier. A tier whose model is not installed is
    /// remembered, never asked again, and answers `None`.
    pub(super) fn try_decode(
        &mut self,
        tier: Tier,
        samples: &[f32],
    ) -> Result<Option<Decoded>, RouteError> {
        if self.is_unavailable(tier) {
            return Ok(None);
        }
        match self.decoder.decode(tier, samples) {
            Ok(decoded) => Ok(Some(decoded)),
            Err(DecodeError::Unavailable) => {
                self.unavailable.push(tier);
                Ok(None)
            }
            Err(DecodeError::Failed(error)) => Err(RouteError::Decode(error)),
        }
    }

    /// Decodes with `requested`, or with the first tier of its chain that is
    /// installed. A substitution is reported once per requested tier, because
    /// a tier that is missing would otherwise be reported for every segment.
    pub(super) fn decode_with_fallback(
        &mut self,
        requested: Tier,
        samples: &[f32],
    ) -> Result<(Decoded, Tier), RouteError> {
        for candidate in fallback_chain(requested) {
            if let Some(decoded) = self.try_decode(candidate, samples)? {
                self.note_substitute(requested, candidate);
                return Ok((decoded, candidate));
            }
        }
        Err(RouteError::NoModels)
    }

    fn note_substitute(&mut self, requested: Tier, used: Tier) {
        if used != requested && !self.substituted.contains(&requested) {
            self.substituted.push(requested);
            self.notices
                .push(RouteNotice::Substituted { requested, used });
        }
    }

    /// Decodes in the tier `lang` is routed to.
    pub(super) fn decode_lang(
        &mut self,
        lang: &str,
        samples: &[f32],
    ) -> Result<(Decoded, Tier), RouteError> {
        let requested = requested_tier(lang, self.policy.en_tier);
        self.decode_with_fallback(requested, samples)
    }

    /// The language of the session when nothing identifies it.
    fn locked_lang(&self) -> String {
        match &self.session.last_lang {
            Some(language) => language.clone(),
            None => TIER0_LANG.to_owned(),
        }
    }

    pub(super) fn identify_lang(&self, samples: &[f32]) -> Result<String, RouteError> {
        match &self.lid {
            Some(lid) => lid.identify(samples).map_err(RouteError::Lid),
            None => Ok(self.locked_lang()),
        }
    }

    /// The identifier as a public hook, so that a caller can name the language
    /// in the middle of an utterance. The model of that language starts
    /// loading, so it is resident when the utterance ends. A pair run reads
    /// only `ja` and `en`, so a third language loads nothing.
    pub fn identify(&mut self, samples: &[f32]) -> Result<String, RouteError> {
        self.check_open()?;
        let lang = self.identify_lang(samples)?;
        if !self.policy.pair || matches!(lang.as_str(), "ja" | "en") {
            self.prefetch(requested_tier(&lang, self.policy.en_tier));
        }
        Ok(lang)
    }

    /// Starts loading a tier's model in the background. A closed router starts
    /// nothing and says so.
    pub fn prefetch(&mut self, tier: Tier) -> bool {
        if self.closed {
            return false;
        }
        self.decoder.prefetch(tier);
        true
    }

    /// The Japanese second opinion has no model: it is turned off, and said once.
    pub(super) fn second_opinion_missing(&mut self) {
        self.policy.second_opinion = false;
        if !self.second_opinion_reported {
            self.second_opinion_reported = true;
            self.notices.push(RouteNotice::Left(Fallback {
                stage: Stage::SecondOpinion,
                reason: Reason::Missing,
                to: To::Disabled,
            }));
        }
    }

    /// Ends the router's life: the recognizers' background work stops and
    /// every model is released. Later work is refused. Calling it again does nothing.
    pub fn close(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        self.decoder.close();
        self.lid = None;
        self.session = Session::default();
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }
}

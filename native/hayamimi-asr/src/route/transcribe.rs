//! One segment, from the audio to the text and the language it was read in.
//!
//! The steps follow the order in which each one is allowed to overrule the
//! one before: the language is chosen (forced, early-detected or identified,
//! then held or switched), the tier reads, an empty reading goes to the
//! generalist, a text its script contradicts is read again, a buffer that lost
//! its beginning is retried, and last the second opinion is asked. Inverse
//! text normalisation, punctuation, word spacing and the user's dictionary
//! come after, from the caller.

use std::time::Instant;

use crate::lid::{Identify, has_kana, script_corrected_lang, sv_lid_tag};

use super::RouteError;
use super::backend::{Decode, DecodeError, Decoded};
use super::router::{Router, millis};
use super::second::choose_second_opinion;
use super::sticky::{
    DUAL_CONFIRM_LANGS, Detection, Hysteresis, MIN_PROBE_S, resolve_dual_confirm, resolve_sticky,
};
use super::tiers::Tier;

/// An early detection is trusted for a segment shorter than this; a longer one
/// gives the full window a chance to overrule a guess made from its first seconds.
const KNOWN_LANG_MAX_S: f64 = 4.0;

/// One segment to read.
#[derive(Debug, Clone, Copy)]
pub struct Request<'a> {
    /// 16 kHz mono samples, the pre-roll included.
    pub samples: &'a [f32],
    /// A language detected earlier, in the middle of the utterance.
    pub known_lang: Option<&'a str>,
    /// Seconds of speech in the segment, when known.
    pub speech_s: Option<f64>,
    /// A final of the live stream. A decode that is not live (the refine pass)
    /// leaves the session's language alone.
    pub live: bool,
    /// Ask the second opinion for this decode; none leaves it to the policy,
    /// which asks it of refine decodes only.
    pub second_opinion: Option<bool>,
}

impl<'a> Request<'a> {
    /// A live segment with nothing known about it.
    pub fn live(samples: &'a [f32]) -> Request<'a> {
        Request {
            samples,
            known_lang: None,
            speech_s: None,
            live: true,
            second_opinion: None,
        }
    }
}

/// What a segment was read as.
#[derive(Debug, Clone, PartialEq)]
pub struct Transcript {
    pub text: String,
    pub lang: String,
    /// The tier that was asked for the language, which is not always the one
    /// whose text stands: the Chinese branch can keep SenseVoice's text.
    pub tier: Tier,
    pub lid_ms: f64,
    pub decode_ms: f64,
    /// The time of SenseVoice's confirmation probe.
    pub probe_ms: f64,
}

/// The language chosen for a segment, and what the choice forbids.
struct Chosen {
    lang: String,
    /// The generalist must not be asked when the specialist comes back empty.
    suppress_fallback: bool,
    /// The segment opens a session and is too short to say what its language is.
    suppress_seed: bool,
}

/// SenseVoice's reading of one segment, made at most once and used by every
/// step that wants it: the session's start, the switch confirmation, and the
/// Chinese and Cantonese arbitration.
#[derive(Default)]
pub(super) struct Probe {
    pub(super) result: Option<Decoded>,
    pub(super) ms: f64,
}

impl Probe {
    pub(super) fn lang(&self) -> Option<&'static str> {
        self.result.as_ref().and_then(|read| sv_lid_tag(&read.tag))
    }
}

/// The text that stands, the language it was read as, the tier asked for it,
/// and the recognizer that made the text (a retry is pinned to that one).
pub(super) struct Standing {
    pub(super) text: String,
    pub(super) lang: String,
    pub(super) tier: Tier,
    pub(super) model: Tier,
}

fn standing(text: String, lang: &str, tier: Tier, model: Tier) -> Standing {
    Standing {
        text,
        lang: lang.to_owned(),
        tier,
        model,
    }
}

pub(super) fn blank(text: &str) -> bool {
    text.trim().is_empty()
}

impl<D: Decode, L: Identify> Router<D, L> {
    /// Reads one segment.
    pub fn transcribe(&mut self, request: &Request<'_>) -> Result<Transcript, RouteError> {
        self.check_open()?;
        let (guess, lid_ms) = self.first_guess(request)?;
        let mut probe = Probe::default();
        let chosen = self.resolve_language(&guess, request, &mut probe)?;
        // A load before this point is not the decode's: drop it from the count.
        self.decoder.take_load_ms();
        let started = Instant::now();
        let mut read = self.read_segment(&chosen.lang, request.samples, &mut probe)?;
        self.generalist_for_blank(&mut read, request.samples, chosen.suppress_fallback)?;
        self.correct_by_script(&mut read, request)?;
        self.retry_cut_short(&mut read, request.samples)?;
        self.ask_second_opinion(&mut read, request)?;
        if request.live && !blank(&read.text) && !chosen.suppress_seed {
            self.session.last_lang = Some(read.lang.clone());
        }
        // The recognizer's time: a model loaded by a tier's first use is not in it.
        let decode_ms = (millis(started) - self.decoder.take_load_ms()).max(0.0);
        Ok(Transcript {
            text: read.text,
            lang: read.lang,
            tier: read.tier,
            lid_ms,
            decode_ms,
            probe_ms: probe.ms,
        })
    }

    /// The first language to try: the forced one, an early detection for a
    /// short segment, or the identifier's. Returns the identifier's time.
    fn first_guess(&self, request: &Request<'_>) -> Result<(String, f64), RouteError> {
        if let Some(forced) = &self.policy.forced_lang {
            return Ok((forced.clone(), 0.0));
        }
        if let Some(known) = request.known_lang
            && request
                .speech_s
                .is_none_or(|speech| speech < KNOWN_LANG_MAX_S)
        {
            return Ok((known.to_owned(), 0.0));
        }
        let started = Instant::now();
        let lang = self.identify_lang(request.samples)?;
        Ok((lang, millis(started)))
    }

    /// Runs SenseVoice's confirmation probe unless it already ran for this segment.
    pub(super) fn sv_probe(
        &mut self,
        probe: &mut Probe,
        samples: &[f32],
    ) -> Result<(), RouteError> {
        if probe.result.is_some() {
            return Ok(());
        }
        let started = Instant::now();
        probe.result = self.try_decode(Tier::Sv, samples)?;
        probe.ms += millis(started);
        Ok(())
    }

    /// Settles the language of the segment: held at the session's, or switched.
    fn resolve_language(
        &mut self,
        guess: &str,
        request: &Request<'_>,
        probe: &mut Probe,
    ) -> Result<Chosen, RouteError> {
        let mut chosen = Chosen {
            lang: guess.to_owned(),
            suppress_fallback: false,
            suppress_seed: false,
        };
        if self.policy.forced_lang.is_some() || !request.live {
            return Ok(chosen);
        }
        let last = self.session.last_lang.clone();
        // A session's first segment is always checked against SenseVoice's
        // identifier: the first identifier alone is unreliable at short
        // lengths and can name a language the second cannot arbitrate.
        if last.is_none() && self.policy.dual_confirm {
            self.sv_probe(probe, request.samples)?;
        }
        if self.policy.dual_confirm && DUAL_CONFIRM_LANGS.contains(&guess) {
            if last.as_deref() != Some(guess) {
                self.confirm_by_probe(&mut chosen, last.as_deref(), request, probe)?;
            }
        } else {
            self.hold_by_hysteresis(&mut chosen, last.as_deref(), request, probe);
        }
        Ok(chosen)
    }

    /// A switch among the five languages both identifiers cover.
    fn confirm_by_probe(
        &mut self,
        chosen: &mut Chosen,
        last: Option<&str>,
        request: &Request<'_>,
        probe: &mut Probe,
    ) -> Result<(), RouteError> {
        self.sv_probe(probe, request.samples)?;
        let guess = chosen.lang.clone();
        let (lang, switched) = resolve_dual_confirm(&guess, last, request.speech_s, probe.lang());
        chosen.lang = lang.to_owned();
        chosen.suppress_fallback = request.speech_s.is_some_and(|speech| speech < MIN_PROBE_S);
        chosen.suppress_seed = last.is_none() && chosen.suppress_fallback;
        if switched {
            // A confirmed switch supersedes the candidate the hysteresis was counting.
            self.session.pending = None;
        }
        Ok(())
    }

    /// A switch to any other language: the same one on several detections in a row.
    fn hold_by_hysteresis(
        &mut self,
        chosen: &mut Chosen,
        last: Option<&str>,
        request: &Request<'_>,
        probe: &Probe,
    ) {
        let guess = chosen.lang.clone();
        let detection = Detection {
            lang: &guess,
            speech_s: request.speech_s,
            probe_lang: probe.lang(),
        };
        let hysteresis = Hysteresis {
            min_switch_s: self.policy.min_switch_s,
            confirm: self.policy.lid_switch_confirm,
        };
        let decided = resolve_sticky(&detection, last, hysteresis, self.session.pending.take());
        chosen.lang = decided.lang.to_owned();
        chosen.suppress_fallback = decided.suppress_fallback;
        self.session.pending = decided.pending;
    }

    /// The first reading, in the tier the language is routed to.
    fn read_segment(
        &mut self,
        lang: &str,
        samples: &[f32],
        probe: &mut Probe,
    ) -> Result<Standing, RouteError> {
        if self.policy.forced_lang.is_some() {
            return self.read_routed(lang, samples);
        }
        if lang == "zh" {
            return self.read_chinese(lang, samples, probe);
        }
        if matches!(lang, "ko" | "yue")
            && probe.lang() == Some(lang)
            && let Some(read) = probe.result.take()
        {
            // The confirmation probe already read this audio through the tier
            // Korean and Cantonese are routed to.
            return Ok(standing(read.text, lang, Tier::Sv, Tier::Sv));
        }
        self.read_routed(lang, samples)
    }

    fn read_routed(&mut self, lang: &str, samples: &[f32]) -> Result<Standing, RouteError> {
        let (read, tier) = self.decode_lang(lang, samples)?;
        Ok(standing(read.text, lang, tier, tier))
    }

    /// The first identifier labels Cantonese as Chinese, so SenseVoice's own
    /// identifier arbitrates: its text stands for Cantonese, and Chinese is read again by Paraformer.
    fn read_chinese(
        &mut self,
        lang: &str,
        samples: &[f32],
        probe: &mut Probe,
    ) -> Result<Standing, RouteError> {
        self.sv_probe(probe, samples)?;
        let Some(sv) = probe.result.take() else {
            return self.read_routed(lang, samples);
        };
        if sv.tag.contains("yue") {
            return Ok(standing(sv.text, "yue", Tier::Sv, Tier::Sv));
        }
        let (second, tier) = self.decode_with_fallback(Tier::Pz, samples)?;
        if blank(&second.text) {
            return Ok(standing(sv.text, lang, tier, Tier::Sv));
        }
        Ok(standing(second.text, lang, tier, tier))
    }

    /// A specialist that came back empty was probably misled by the
    /// identifier: the generalist gets the last word. With none installed
    /// the empty text stands.
    fn generalist_for_blank(
        &mut self,
        read: &mut Standing,
        samples: &[f32],
        suppressed: bool,
    ) -> Result<(), RouteError> {
        if !blank(&read.text) || read.tier == Tier::Omni || suppressed {
            return Ok(());
        }
        if let Some(omni) = self.try_decode(Tier::Omni, samples)? {
            read.text = omni.text;
            read.tier = Tier::Omni;
            read.model = Tier::Omni;
        }
        Ok(())
    }

    /// A text whose script contradicts its language tag is read again, before
    /// anyone sees it, in the language the script names.
    fn correct_by_script(
        &mut self,
        read: &mut Standing,
        request: &Request<'_>,
    ) -> Result<(), RouteError> {
        if self.policy.forced_lang.is_some() || !request.live || blank(&read.text) {
            return Ok(());
        }
        let corrected = script_corrected_lang(&read.lang, &read.text);
        if corrected == read.lang {
            return Ok(());
        }
        let corrected = corrected.to_owned();
        if corrected == "ja" && !has_kana(&read.text) && !self.is_unavailable(Tier::Sv) {
            return self.arbitrate_han_only(read, request.samples);
        }
        self.read_again_as(read, &corrected, request.samples)
    }

    fn read_again_as(
        &mut self,
        read: &mut Standing,
        lang: &str,
        samples: &[f32],
    ) -> Result<(), RouteError> {
        let (again, tier) = self.decode_lang(lang, samples)?;
        if !blank(&again.text) {
            *read = standing(again.text, lang, tier, tier);
        }
        Ok(())
    }

    /// Han characters with no kana are as likely Chinese or Cantonese as
    /// Japanese: SenseVoice's identifier arbitrates instead of the script being assumed.
    fn arbitrate_han_only(
        &mut self,
        read: &mut Standing,
        samples: &[f32],
    ) -> Result<(), RouteError> {
        let Some(sv) = self.try_decode(Tier::Sv, samples)? else {
            return Ok(());
        };
        if blank(&sv.text) {
            return Ok(());
        }
        let (lang, tier) = if sv.tag.contains("yue") {
            ("yue", Tier::Sv)
        } else if sv.tag.contains("zh") {
            ("zh", Tier::Pz)
        } else if sv.tag.contains("ja") {
            ("ja", Tier::Rz)
        } else if sv.tag.contains("ko") {
            *read = standing(sv.text, "ko", Tier::Sv, Tier::Sv);
            return Ok(());
        } else {
            return Ok(());
        };
        if tier == Tier::Sv {
            *read = standing(sv.text, lang, tier, tier);
            return Ok(());
        }
        let (own, _) = self.decode_with_fallback(tier, samples)?;
        *read = if blank(&own.text) {
            standing(sv.text, lang, tier, Tier::Sv)
        } else {
            standing(own.text, lang, tier, tier)
        };
        Ok(())
    }

    /// A buffer that lost its beginning is read again piece by piece through
    /// the recognizer that made the text; the decision about the language is final.
    fn retry_cut_short(&mut self, read: &mut Standing, samples: &[f32]) -> Result<(), RouteError> {
        match self
            .decoder
            .split_retry(&read.text, samples, &read.lang, read.model)
        {
            Ok(text) => read.text = text,
            Err(DecodeError::Unavailable) => {}
            Err(DecodeError::Failed(error)) => return Err(RouteError::Decode(error)),
        }
        Ok(())
    }

    /// On Japanese, the second model's reading replaces the first when the two agree.
    fn ask_second_opinion(
        &mut self,
        read: &mut Standing,
        request: &Request<'_>,
    ) -> Result<(), RouteError> {
        let asked = match request.second_opinion {
            Some(wanted) => wanted,
            None => self.policy.second_opinion && !request.live,
        };
        if !asked || read.lang != "ja" || blank(&read.text) {
            return Ok(());
        }
        match self.try_decode(Tier::Pja, request.samples) {
            Ok(Some(second)) => {
                let (chosen, _) =
                    choose_second_opinion(&read.text, &second.text, self.policy.agree_threshold);
                read.text = chosen.to_owned();
            }
            Ok(None) => self.second_opinion_missing(),
            // A failure of the optional model must never lose the primary text.
            Err(_) => {}
        }
        Ok(())
    }
}

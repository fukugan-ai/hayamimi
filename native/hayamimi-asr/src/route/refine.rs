//! The refine pass: a group of finished segments, merged, read again.
//!
//! The merged audio is longer than any one segment, so the identifier is
//! asked again. Its new answer replaces the language the fast path settled on
//! only when SenseVoice's reading of the merged audio agrees with it and the
//! group is long enough to judge ([`resolve_refine_lang`]).

use crate::audio::SAMPLE_RATE;
use crate::lid::Identify;

use super::RouteError;
use super::backend::Decode;
use super::router::Router;
use super::sticky::{REFINE_MIN_REGROUP_S, resolve_refine_lang};
use super::tiers::SV_LANGS;
use super::transcribe::{Probe, Request, blank};

/// A merged group, read again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refined {
    pub text: String,
    pub lang: String,
}

impl<D: Decode, L: Identify> Router<D, L> {
    /// Reads a merged group of segments again. `lang` is the language the fast
    /// path settled on. The text is as the recognizer made it: the user's
    /// dictionary and Korean word spacing come after, from the caller.
    pub fn refine(&mut self, samples: &[f32], lang: &str) -> Result<Refined, RouteError> {
        self.check_open()?;
        let duration = samples.len() as f64 / f64::from(SAMPLE_RATE);
        let detected = self.identify_lang(samples)?;
        let mut probe = Probe::default();
        if detected != lang && duration >= REFINE_MIN_REGROUP_S {
            self.sv_probe(&mut probe, samples)?;
        }
        let (resolved, changed) = resolve_refine_lang(lang, &detected, probe.lang(), duration);
        if changed && let Some(refined) = self.read_resolved(resolved, samples, &mut probe)? {
            return Ok(refined);
        }
        let text = self.read_known(lang, samples)?;
        Ok(Refined {
            text,
            lang: lang.to_owned(),
        })
    }

    /// The reading in the language the re-judgment settled on. Nothing when it
    /// comes back empty, and then the fast path's language stands.
    ///
    /// A resolved language is only accepted when SenseVoice's probe agreed with
    /// it, and Korean and Cantonese are routed to SenseVoice, so for those the
    /// probe already read this audio and is not read a second time.
    fn read_resolved(
        &mut self,
        resolved: &str,
        samples: &[f32],
        probe: &mut Probe,
    ) -> Result<Option<Refined>, RouteError> {
        let text = match probe.result.take() {
            Some(read) if SV_LANGS.contains(&resolved) => read.text,
            _ => self.read_known(resolved, samples)?,
        };
        Ok((!blank(&text)).then(|| Refined {
            text,
            lang: resolved.to_owned(),
        }))
    }

    /// A decode that is not live, in a language that is already known.
    fn read_known(&mut self, lang: &str, samples: &[f32]) -> Result<String, RouteError> {
        let request = Request {
            known_lang: Some(lang),
            live: false,
            ..Request::live(samples)
        };
        Ok(self.transcribe(&request)?.text)
    }
}

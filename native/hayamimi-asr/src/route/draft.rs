//! The draft of an utterance still in progress.

use crate::lid::Identify;

use super::RouteError;
use super::backend::Decode;
use super::router::Router;
use super::tiers::{Tier, V3_LANGS};
use super::transcribe::blank;

impl<D: Decode, L: Identify> Router<D, L> {
    /// A fast draft of an utterance that is not finished.
    ///
    /// It is read in the forced language, else in the language detected early
    /// in this utterance, else in the session's own when that is a European
    /// language SenseVoice cannot probe. With none of these it follows
    /// SenseVoice's tag, so that the very first draft comes out in the right
    /// language instead of an English model's romanisation of Korean.
    pub fn partial(
        &mut self,
        samples: &[f32],
        lang_hint: Option<&str>,
    ) -> Result<String, RouteError> {
        self.check_open()?;
        if let Some(forced) = self.policy.forced_lang.clone() {
            return self.draft_in(&forced, samples);
        }
        if let Some(hint) = lang_hint {
            return self.draft_in(hint, samples);
        }
        if let Some(session) = self.session.last_lang.clone()
            && session != "en"
            && V3_LANGS.contains(&session.as_str())
        {
            return self.draft_in(&session, samples);
        }
        self.draft_by_probe(samples)
    }

    fn draft_in(&mut self, lang: &str, samples: &[f32]) -> Result<String, RouteError> {
        let (read, _) = self.decode_lang(lang, samples)?;
        Ok(read.text)
    }

    fn draft_by_probe(&mut self, samples: &[f32]) -> Result<String, RouteError> {
        let Some(sv) = self.try_decode(Tier::Sv, samples)? else {
            let (read, _) = self.decode_with_fallback(Tier::Rz, samples)?;
            return Ok(read.text);
        };
        if sv.tag.contains("ja")
            && let Some(rz) = self.try_decode(Tier::Rz, samples)?
            && !blank(&rz.text)
        {
            return Ok(rz.text);
        }
        Ok(sv.text)
    }
}

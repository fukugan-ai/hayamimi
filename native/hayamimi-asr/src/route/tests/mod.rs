//! Tests of the language route. Nothing here opens a model: the recognizers
//! and the identifier are scripted, so a test says what each one answers and
//! then reads what the router asked of it.

mod dual;
mod lifecycle;
mod partial;
mod refine;
mod routing;
mod second;
mod setters;
mod sticky;
mod transcribe;

use std::cell::Cell;
use std::rc::Rc;

use crate::error::AsrError;
use crate::lid::Identify;

use super::{Decode, DecodeError, Decoded, Policy, Router, Tier};

/// Audio the fakes ignore.
pub(super) static SAMPLES: [f32; 4800] = [0.0; 4800];

/// What a scripted recognizer answers for a tier: its text and its language tag.
type Answer = (Tier, &'static str, &'static str);

/// A recognizer behind [`Decode`] that answers from a script and remembers what it was asked.
#[derive(Default)]
pub(super) struct Script {
    answers: Vec<Answer>,
    failing: Vec<Tier>,
    pub(super) calls: Vec<Tier>,
    pub(super) retries: Vec<Tier>,
    pub(super) prefetched: Vec<Tier>,
    pub(super) closed: u32,
    /// Dropped with the script; a test watches it to see the script is gone.
    pub(super) life: Rc<()>,
}

impl Script {
    /// Installed tiers with the text and the tag each answers.
    pub(super) fn answering(answers: &[Answer]) -> Script {
        Script {
            answers: answers.to_vec(),
            ..Script::default()
        }
    }

    /// Installed tiers that answer with empty text.
    pub(super) fn present(tiers: &[Tier]) -> Script {
        let answers: Vec<Answer> = tiers.iter().map(|tier| (*tier, "", "")).collect();
        Script::answering(&answers)
    }

    /// A tier that is installed and fails when asked.
    pub(super) fn failing(mut self, tier: Tier) -> Script {
        self.failing.push(tier);
        self
    }

    pub(super) fn times_asked(&self, tier: Tier) -> usize {
        self.calls.iter().filter(|asked| **asked == tier).count()
    }
}

impl Decode for Script {
    fn decode(&mut self, tier: Tier, _samples: &[f32]) -> Result<Decoded, DecodeError> {
        self.calls.push(tier);
        if self.failing.contains(&tier) {
            return Err(DecodeError::Failed(AsrError::NoResult));
        }
        match self.answers.iter().find(|(known, _, _)| *known == tier) {
            Some((_, text, tag)) => Ok(Decoded {
                text: (*text).to_owned(),
                tag: (*tag).to_owned(),
            }),
            None => Err(DecodeError::Unavailable),
        }
    }

    fn split_retry(
        &mut self,
        text: &str,
        _samples: &[f32],
        _lang: &str,
        tier: Tier,
    ) -> Result<String, DecodeError> {
        self.retries.push(tier);
        Ok(text.to_owned())
    }

    fn prefetch(&mut self, tier: Tier) {
        self.prefetched.push(tier);
    }

    fn close(&mut self) {
        self.closed += 1;
    }
}

/// An identifier that always names one language and counts how often it was asked.
pub(super) struct FixedLid {
    lang: &'static str,
    pub(super) asked: Cell<u32>,
}

impl FixedLid {
    pub(super) fn naming(lang: &'static str) -> FixedLid {
        FixedLid {
            lang,
            asked: Cell::new(0),
        }
    }
}

impl Identify for FixedLid {
    fn identify(&self, _samples: &[f32]) -> Result<String, AsrError> {
        self.asked.set(self.asked.get() + 1);
        Ok(self.lang.to_owned())
    }
}

pub(super) type TestRouter = Router<Script, FixedLid>;

/// A router over a script and an identifier.
pub(super) fn router(script: Script, lid: Option<FixedLid>, policy: Policy) -> TestRouter {
    Router::new(script, lid, policy)
}

/// A router that reads every segment in `lang`, over a script.
pub(super) fn single(script: Script, lang: &str) -> TestRouter {
    router(script, None, Policy::single(lang))
}

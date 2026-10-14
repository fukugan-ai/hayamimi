//! The language route: which recognizer reads each segment.
//!
//! A spoken-language identifier names the language of a segment, a second
//! identifier built into SenseVoice confirms a switch for the five languages
//! it covers, a hysteresis holds the others, and the language picks a tier of
//! the catalogue. A tier that is not installed gives way to the next
//! ([`fallback_chain`]); an empty reading goes to the generalist; a text the
//! script contradicts is read again in the language the script names; a
//! buffer that lost its beginning is retried piece by piece; and on Japanese
//! a second model's reading is adopted when it agrees with the first.
//!
//! The router decides and the recognizers behind [`Decode`] read. Nothing
//! here opens a model except the identifier ([`crate::lid`]).

mod backend;
mod draft;
mod refine;
mod router;
mod second;
mod sticky;
mod tiers;
mod transcribe;

#[cfg(test)]
mod tests;

use std::fmt;

use crate::error::AsrError;

pub use backend::{Decode, DecodeError, Decoded};
pub use refine::Refined;
pub use router::{Policy, RouteNotice, Router};
pub use second::{SECOND_OPINION_THRESHOLD, choose_second_opinion, hyp_agreement_cer};
pub use sticky::{
    DUAL_CONFIRM_LANGS, Detection, Hysteresis, MIN_PROBE_S, Pending, REFINE_MIN_REGROUP_S, Sticky,
    resolve_dual_confirm, resolve_refine_lang, resolve_sticky,
};
pub use tiers::{
    EnTier, PARA_LANGS, RZ_LANGS, SV_LANGS, TIER0_LANG, Tier, V3_LANGS, fallback_chain,
    is_routable, requested_tier, resolve_en_tier,
};
pub use transcribe::{Request, Transcript};

/// Why the router could not answer.
#[derive(Debug)]
pub enum RouteError {
    /// The router was closed and takes no more work.
    Closed,
    /// No recognizer of the catalogue is installed.
    NoModels,
    /// A setting was given a value it does not accept.
    Invalid(String),
    /// A recognizer that is installed failed.
    Decode(AsrError),
    /// The language identifier failed.
    Lid(AsrError),
}

impl fmt::Display for RouteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RouteError::Closed => write!(
                f,
                "the router is closed; build a new one to transcribe again"
            ),
            RouteError::NoModels => {
                write!(f, "no recognizer is installed under the models directory")
            }
            RouteError::Invalid(what) => write!(f, "{what}"),
            RouteError::Decode(error) => write!(f, "recognizer: {error}"),
            RouteError::Lid(error) => write!(f, "language identifier: {error}"),
        }
    }
}

impl std::error::Error for RouteError {}

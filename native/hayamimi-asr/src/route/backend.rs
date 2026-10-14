//! What the router asks of the recognizers.
//!
//! The router decides which model reads a segment; the recognizers, their
//! residency and the cutting of a long buffer into utterances are behind this
//! trait. The router calls it from one thread.

use crate::error::AsrError;

use super::tiers::Tier;

/// What one decode produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    pub text: String,
    /// The language tag the model put on its result. Only SenseVoice
    /// identifies the language of what it reads; the others leave this empty.
    pub tag: String,
}

#[derive(Debug)]
pub enum DecodeError {
    /// The tier's model is not installed. The router goes on to the next tier.
    Unavailable,
    /// The model is there and the decode failed.
    Failed(AsrError),
}

pub trait Decode {
    /// Decodes 16 kHz mono samples with the recognizer of `tier`.
    fn decode(&mut self, tier: Tier, samples: &[f32]) -> Result<Decoded, DecodeError>;

    /// Decodes a buffer that looks to have lost its beginning piece by piece
    /// through `tier` alone, and returns the pieces' text when it clearly
    /// recovers the lost part, otherwise `text` unchanged. `tier` is the
    /// recognizer that made `text`: no piece is routed again, corrected by
    /// script or handed to the generalist. A buffer that does not look cut
    /// short is returned at once, so the router asks for every segment.
    fn split_retry(
        &mut self,
        text: &str,
        samples: &[f32],
        lang: &str,
        tier: Tier,
    ) -> Result<String, DecodeError>;

    /// Starts loading the model of `tier` in the background, so that the
    /// first segment in its language does not wait for it.
    fn prefetch(&mut self, tier: Tier);

    /// The milliseconds spent loading models since the last call, so that the
    /// router can keep a load out of the time it reports for a decode. A
    /// backend that loads nothing lazily answers 0.
    fn take_load_ms(&mut self) -> f64 {
        0.0
    }

    /// Ends any background work and releases every model. Called once.
    fn close(&mut self);
}

//! The recognizers as the route's backend: the route names a tier and gets
//! that tier's reading, or hears that it is not installed and chooses what
//! gives way. Nothing here substitutes a tier.

use crate::decode::Recognizers;
use crate::route::{Decode, DecodeError, Decoded, Tier};

impl Decode for Recognizers {
    fn decode(&mut self, tier: Tier, samples: &[f32]) -> Result<Decoded, DecodeError> {
        match self.decode_exact(tier, samples) {
            Ok(Some(done)) => Ok(Decoded {
                text: done.text,
                tag: done.lang.code().to_owned(),
            }),
            Ok(None) => Err(DecodeError::Unavailable),
            Err(error) => Err(DecodeError::Failed(error)),
        }
    }

    fn split_retry(
        &mut self,
        text: &str,
        samples: &[f32],
        lang: &str,
        tier: Tier,
    ) -> Result<String, DecodeError> {
        self.retry_if_truncated(tier, lang, text, samples)
            .map_err(DecodeError::Failed)
    }

    /// Loads the model now, on the caller's thread: a load is not run in the
    /// background. A failure that is not about the model shows again at the
    /// first decode with that tier, so it is not lost here.
    fn prefetch(&mut self, tier: Tier) {
        let _ = self.load(tier);
    }

    fn close(&mut self) {
        self.release_all();
    }
}

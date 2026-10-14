//! The speaker mark of each finished utterance.
//!
//! One embedding per utterance is matched against the speakers heard so far
//! (`crate::speaker`). A speaker that has matched only once is shown with a
//! `?` until it matches again. A model that is not there leaves the stage out;
//! the utterance then carries no mark (design 3.15).

use std::path::Path;
use std::time::Instant;

use crate::embedder::Embedder;
use crate::error::AsrError;
use crate::speaker::{Centroids, Source};

/// The speaker mark of one utterance and what it cost.
#[derive(Debug, Clone, PartialEq)]
pub struct Mark {
    /// `S1`, `S2?`, ... ; empty when there is no mark.
    pub label: String,
    pub ms: f64,
    /// Set when the embedding failed and the utterance went on without a mark.
    pub error: Option<String>,
}

impl Mark {
    /// The mark of an utterance when the stage is out: no label, no time.
    pub fn none() -> Mark {
        Mark {
            label: String::new(),
            ms: 0.0,
            error: None,
        }
    }
}

pub struct SpeakerStage {
    embedder: Embedder,
    centroids: Centroids,
}

impl SpeakerStage {
    /// Opens the embedding model under `models`.
    pub fn open(models: &Path) -> Result<SpeakerStage, AsrError> {
        Ok(SpeakerStage {
            embedder: Embedder::open(models)?,
            centroids: Centroids::default(),
        })
    }

    /// The mark of 16 kHz `samples`. A failure must not lose the
    /// transcription: it is handed on in [`Mark::error`].
    pub fn mark(&mut self, samples: &[f32]) -> Mark {
        let started = Instant::now();
        let outcome = self.label(samples);
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        match outcome {
            Ok(label) => Mark {
                label,
                ms,
                error: None,
            },
            Err(error) => Mark {
                error: Some(error.to_string()),
                ms,
                ..Mark::none()
            },
        }
    }

    fn label(&mut self, samples: &[f32]) -> Result<String, AsrError> {
        let embedding = self.embedder.embed(samples)?;
        let label = self.centroids.label(&embedding, Source::Fast);
        Ok(self.centroids.display_label(&label))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_embedding_model_leaves_the_stage_out() {
        let said = SpeakerStage::open(Path::new("no-such-models")).err();
        assert!(matches!(said, Some(AsrError::MissingModel(_))), "{said:?}");
    }

    #[test]
    fn no_mark_has_no_label_no_time_and_no_error() {
        let none = Mark::none();
        assert!(none.label.is_empty());
        assert!(none.ms.abs() < f64::EPSILON);
        assert!(none.error.is_none());
    }
}

//! The speaker embedding of one stretch of audio: CAM++ through sherpa-onnx,
//! on the CPU. The vector has length one, so that a dot product is a cosine.

use std::path::{Path, PathBuf};

use sherpa_onnx::{SpeakerEmbeddingExtractor, SpeakerEmbeddingExtractorConfig};

use crate::audio::SAMPLE_RATE;
use crate::error::AsrError;

/// The model file under the models directory.
pub const MODEL_FILE: &str = "campplus_sv.onnx";
/// Embeddings stop gaining from more audio; longer input is cut here.
pub const MAX_EMBED_SECONDS: usize = 6;
/// Threads of the extractor.
const THREADS: i32 = 2;
/// Added to the norm so that silence divides to zero, not to not-a-number.
const EPSILON: f32 = 1e-9;

/// Where the model file is.
pub fn model_path(models: &Path) -> PathBuf {
    models.join(MODEL_FILE)
}

/// The part of `samples` the embedding is computed from: the first [`MAX_EMBED_SECONDS`].
pub fn capped(samples: &[f32]) -> &[f32] {
    let longest = MAX_EMBED_SECONDS * SAMPLE_RATE as usize;
    &samples[..samples.len().min(longest)]
}

/// Scales `vector` to length one (a zero vector stays zero).
pub fn normalize(vector: &mut [f32]) {
    let length = vector.iter().map(|x| x * x).sum::<f32>().sqrt() + EPSILON;
    for value in vector {
        *value /= length;
    }
}

pub struct Embedder {
    extractor: SpeakerEmbeddingExtractor,
}

impl Embedder {
    /// Opens the extractor under `models`. A missing file is refused first:
    /// the C library ends the whole process on a missing model.
    pub fn open(models: &Path) -> Result<Self, AsrError> {
        let path = model_path(models);
        if !path.is_file() {
            return Err(AsrError::MissingModel(path));
        }
        let model = path
            .to_str()
            .ok_or(AsrError::Create(
                "speaker embedding (the model path is not UTF-8)",
            ))?
            .to_owned();
        let config = SpeakerEmbeddingExtractorConfig {
            model: Some(model),
            num_threads: THREADS,
            debug: false,
            provider: Some(String::from("cpu")),
        };
        let extractor = SpeakerEmbeddingExtractor::create(&config)
            .ok_or(AsrError::Create("speaker embedding extractor"))?;
        Ok(Embedder { extractor })
    }

    /// The embedding of 16 kHz `samples`.
    pub fn embed(&self, samples: &[f32]) -> Result<Vec<f32>, AsrError> {
        let rate = i32::try_from(SAMPLE_RATE).map_err(|_| AsrError::Overflow("the sample rate"))?;
        let stream = self
            .extractor
            .create_stream()
            .ok_or(AsrError::Create("speaker embedding stream"))?;
        stream.accept_waveform(rate, capped(samples));
        stream.input_finished();
        let mut vector = self.extractor.compute(&stream).ok_or(AsrError::NoResult)?;
        normalize(&mut vector);
        Ok(vector)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_longer_than_six_seconds_is_cut_to_six() {
        let long = vec![0.0_f32; 7 * SAMPLE_RATE as usize];
        assert_eq!(capped(&long).len(), 6 * SAMPLE_RATE as usize);
    }

    #[test]
    fn audio_of_six_seconds_or_less_is_kept_whole() {
        let exact = vec![0.0_f32; 6 * SAMPLE_RATE as usize];
        assert_eq!(capped(&exact).len(), exact.len());
        assert_eq!(capped(&[0.5; 10]).len(), 10);
        assert!(capped(&[]).is_empty());
    }

    #[test]
    fn a_vector_is_scaled_to_length_one() {
        let mut v = [3.0_f32, 4.0];
        normalize(&mut v);
        let length = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((length - 1.0).abs() < 1e-6, "{length}");
    }

    #[test]
    fn a_zero_vector_stays_zero() {
        let mut v = [0.0_f32; 4];
        normalize(&mut v);
        assert_eq!(v, [0.0; 4]);
    }

    #[test]
    fn a_missing_model_is_refused_before_the_c_library_is_called() {
        let said = Embedder::open(Path::new("no-such-models")).err();
        assert!(matches!(said, Some(AsrError::MissingModel(_))), "{said:?}");
    }

    #[test]
    fn the_model_is_looked_for_under_the_models_directory() {
        assert_eq!(
            model_path(Path::new("models")),
            Path::new("models").join("campplus_sv.onnx")
        );
    }
}

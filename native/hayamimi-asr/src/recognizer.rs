//! The Japanese recognizer: the ReazonSpeech zipformer transducer through
//! sherpa-onnx, int8, on the CPU.

use std::path::{Path, PathBuf};

use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig};

use crate::audio::SAMPLE_RATE;
use crate::error::AsrError;

const MODEL_DIR: &str = "sherpa-onnx-zipformer-ja-en-reazonspeech-2025-01-17";
const ENCODER: &str = "encoder-epoch-35-avg-1.int8.onnx";
const DECODER: &str = "decoder-epoch-35-avg-1.int8.onnx";
const JOINER: &str = "joiner-epoch-35-avg-1.int8.onnx";
const TOKENS: &str = "tokens.txt";
/// Threads of the recognizer.
pub const THREADS: i32 = 4;
/// The weight of hot words. The recognizer's tokens cannot encode hot words,
/// so none are given and this only matches the measured configuration.
const HOTWORDS_SCORE: f32 = 2.0;
/// Annotation brackets the model puts around boundary words; they carry no speech.
const JUNK: [char; 4] = ['［', '］', '〈', '〉'];

/// The model files of the recognizer, in the order encoder, decoder, joiner, tokens.
pub fn model_files(models: &Path) -> [PathBuf; 4] {
    let directory = models.join(MODEL_DIR);
    [ENCODER, DECODER, JOINER, TOKENS].map(|name| directory.join(name))
}

/// The text of a recognition result without the annotation brackets.
pub fn clean(text: &str) -> String {
    text.chars().filter(|c| !JUNK.contains(c)).collect()
}

fn utf8(path: &Path) -> Result<String, AsrError> {
    if !path.is_file() {
        return Err(AsrError::MissingModel(path.to_path_buf()));
    }
    path.to_str()
        .map(str::to_owned)
        .ok_or(AsrError::Create("recognizer (the model path is not UTF-8)"))
}

pub struct Recognizer {
    inner: OfflineRecognizer,
}

impl Recognizer {
    /// Opens the transducer under `models`. A missing file is refused first:
    /// the C library ends the whole process on a missing model.
    pub fn open(models: &Path) -> Result<Self, AsrError> {
        let [encoder, decoder, joiner, tokens] = model_files(models);
        let mut config = OfflineRecognizerConfig::default();
        config.model_config.transducer = OfflineTransducerModelConfig {
            encoder: Some(utf8(&encoder)?),
            decoder: Some(utf8(&decoder)?),
            joiner: Some(utf8(&joiner)?),
        };
        config.model_config.tokens = Some(utf8(&tokens)?);
        config.model_config.provider = Some(String::from("cpu"));
        config.model_config.num_threads = THREADS;
        // No `model_type`: "zipformer" is not a value the offline recognizer
        // knows, and it finds the type from the model itself either way.
        config.model_config.modeling_unit = Some(String::from("cjkchar"));
        config.decoding_method = Some(String::from("modified_beam_search"));
        config.hotwords_score = HOTWORDS_SCORE;
        let inner = OfflineRecognizer::create(&config).ok_or(AsrError::Create("recognizer"))?;
        Ok(Recognizer { inner })
    }

    /// Decodes one buffer of 16 kHz samples.
    pub fn decode(&self, samples: &[f32]) -> Result<String, AsrError> {
        let rate = i32::try_from(SAMPLE_RATE).map_err(|_| AsrError::Overflow("the sample rate"))?;
        let stream = self.inner.create_stream();
        stream.accept_waveform(rate, samples);
        self.inner.decode(&stream);
        let result = stream.get_result().ok_or(AsrError::NoResult)?;
        Ok(clean(&result.text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn annotation_brackets_are_removed() {
        assert_eq!(clean("［東京］の〈天気〉"), "東京の天気");
    }

    #[test]
    fn text_without_brackets_is_unchanged() {
        assert_eq!(clean("今日は晴れ"), "今日は晴れ");
        assert_eq!(clean(""), "");
    }

    #[test]
    fn the_four_model_files_are_int8_and_tokens() {
        let files = model_files(Path::new("models"));
        let names: Vec<String> = files
            .iter()
            .filter_map(|path| path.file_name())
            .filter_map(|name| name.to_str())
            .map(str::to_owned)
            .collect();
        assert_eq!(
            names,
            [
                "encoder-epoch-35-avg-1.int8.onnx",
                "decoder-epoch-35-avg-1.int8.onnx",
                "joiner-epoch-35-avg-1.int8.onnx",
                "tokens.txt"
            ]
        );
    }

    #[test]
    fn a_missing_model_is_refused_before_the_c_library_is_called() {
        let said = Recognizer::open(Path::new("no-such-models")).err();
        assert!(matches!(said, Some(AsrError::MissingModel(_))), "{said:?}");
    }
}

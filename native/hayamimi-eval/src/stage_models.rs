//! The sherpa-onnx models the stage measurement opens, with the settings the
//! Python pipeline gives them (threads, decoding method, VAD thresholds), on
//! the provider the run asks for.

use std::path::{Path, PathBuf};

use sherpa_onnx::{
    FastClusteringConfig, OfflineRecognizer, OfflineRecognizerConfig, OfflineSpeakerDiarization,
    OfflineSpeakerDiarizationConfig, OfflineSpeakerSegmentationModelConfig,
    OfflineSpeakerSegmentationPyannoteModelConfig, OfflineTransducerModelConfig,
    SileroVadModelConfig, SpeakerEmbeddingExtractor, SpeakerEmbeddingExtractorConfig,
    SpokenLanguageIdentification, SpokenLanguageIdentificationConfig,
    SpokenLanguageIdentificationWhisperConfig, VadModelConfig, VoiceActivityDetector,
};

use crate::error::EvalError;

/// The sample rate every model here takes.
pub const SAMPLE_RATE: i32 = 16_000;
/// The window the VAD reads at a time.
pub const VAD_WINDOW: usize = 512;
/// Seconds of audio the language identifier is given.
pub const LID_SECONDS: usize = 4;

const RECOGNIZER_DIR: &str = "sherpa-onnx-zipformer-ja-en-reazonspeech-2025-01-17";
const WHISPER_DIR: &str = "sherpa-onnx-whisper-tiny";
const SEGMENTATION_DIR: &str = "sherpa-onnx-pyannote-segmentation-3-0";
const VAD_FILE: &str = "silero_vad.onnx";
const SPEAKER_FILE: &str = "campplus_sv.onnx";

const RECOGNIZER_THREADS: i32 = 4;
const LID_THREADS: i32 = 4;
const VAD_THREADS: i32 = 1;
const SPEAKER_THREADS: i32 = 2;
const DIARIZATION_THREADS: i32 = 2;
const VAD_BUFFER_SECONDS: f32 = 30.0;

/// How a model's weights are stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Precision {
    Fp32,
    Fp16,
    Int8,
}

impl Precision {
    pub fn name(self) -> &'static str {
        match self {
            Precision::Fp32 => "fp32",
            Precision::Fp16 => "fp16",
            Precision::Int8 => "int8",
        }
    }

    /// The part of the file name between the model name and `.onnx`.
    fn infix(self) -> &'static str {
        match self {
            Precision::Fp32 => "",
            Precision::Fp16 => ".fp16",
            Precision::Int8 => ".int8",
        }
    }
}

/// `root/parts...` as text, refused when the file is not there: the C library
/// ends the whole process on a missing model instead of reporting it.
fn existing(root: &Path, parts: &[&str]) -> Result<String, EvalError> {
    let mut path = PathBuf::from(root);
    for part in parts {
        path.push(part);
    }
    if !path.is_file() {
        return Err(EvalError::Invalid(format!(
            "model file not found: {}",
            path.display()
        )));
    }
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| EvalError::Invalid(format!("{} is not UTF-8", path.display())))
}

fn missing(what: &str, precision: Precision, provider: &str) -> EvalError {
    EvalError::Invalid(format!(
        "cannot create the {what} ({}, {provider})",
        precision.name()
    ))
}

/// The Japanese and English transducer, decoded the way the Python pipeline decodes it.
pub fn recognizer(
    models: &Path,
    provider: &str,
    precision: Precision,
) -> Result<OfflineRecognizer, EvalError> {
    let file = |kind: &str| {
        let name = format!("{kind}-epoch-35-avg-1{}.onnx", precision.infix());
        existing(models, &[RECOGNIZER_DIR, &name])
    };
    let mut config = OfflineRecognizerConfig::default();
    config.model_config.transducer = OfflineTransducerModelConfig {
        encoder: Some(file("encoder")?),
        decoder: Some(file("decoder")?),
        joiner: Some(file("joiner")?),
    };
    config.model_config.tokens = Some(existing(models, &[RECOGNIZER_DIR, "tokens.txt"])?);
    config.model_config.provider = Some(provider.to_owned());
    config.model_config.num_threads = RECOGNIZER_THREADS;
    config.model_config.modeling_unit = Some(String::from("cjkchar"));
    config.decoding_method = Some(String::from("modified_beam_search"));
    OfflineRecognizer::create(&config).ok_or_else(|| missing("recognizer", precision, provider))
}

/// Silero with the thresholds of the live pipeline.
pub fn vad(models: &Path, provider: &str) -> Result<VoiceActivityDetector, EvalError> {
    let config = VadModelConfig {
        silero_vad: SileroVadModelConfig {
            model: Some(existing(models, &[VAD_FILE])?),
            threshold: 0.5,
            min_silence_duration: 0.30,
            min_speech_duration: 0.25,
            window_size: i32::try_from(VAD_WINDOW)
                .map_err(|_| EvalError::Invalid(String::from("the VAD window does not fit i32")))?,
            max_speech_duration: 12.0,
        },
        sample_rate: SAMPLE_RATE,
        num_threads: VAD_THREADS,
        provider: Some(provider.to_owned()),
        ..VadModelConfig::default()
    };
    VoiceActivityDetector::create(&config, VAD_BUFFER_SECONDS)
        .ok_or_else(|| missing("VAD", Precision::Fp32, provider))
}

/// Whisper tiny as a language identifier.
pub fn language_id(
    models: &Path,
    provider: &str,
    precision: Precision,
) -> Result<SpokenLanguageIdentification, EvalError> {
    let file = |kind: &str| {
        let name = format!("tiny-{kind}{}.onnx", precision.infix());
        existing(models, &[WHISPER_DIR, &name])
    };
    let config = SpokenLanguageIdentificationConfig {
        whisper: SpokenLanguageIdentificationWhisperConfig {
            encoder: Some(file("encoder")?),
            decoder: Some(file("decoder")?),
            tail_paddings: 0,
        },
        num_threads: LID_THREADS,
        debug: false,
        provider: Some(provider.to_owned()),
    };
    SpokenLanguageIdentification::create(&config)
        .ok_or_else(|| missing("language identifier", precision, provider))
}

/// The speaker embedding model.
pub fn speaker_extractor(
    models: &Path,
    provider: &str,
) -> Result<SpeakerEmbeddingExtractor, EvalError> {
    let config = SpeakerEmbeddingExtractorConfig {
        model: Some(existing(models, &[SPEAKER_FILE])?),
        num_threads: SPEAKER_THREADS,
        debug: false,
        provider: Some(provider.to_owned()),
    };
    SpeakerEmbeddingExtractor::create(&config)
        .ok_or_else(|| missing("speaker embedding extractor", Precision::Fp32, provider))
}

/// Pyannote segmentation, the speaker embedding model and fast clustering.
pub fn diarizer(
    models: &Path,
    provider: &str,
    precision: Precision,
) -> Result<OfflineSpeakerDiarization, EvalError> {
    let segmentation_file = format!("model{}.onnx", precision.infix());
    let config = OfflineSpeakerDiarizationConfig {
        segmentation: OfflineSpeakerSegmentationModelConfig {
            pyannote: OfflineSpeakerSegmentationPyannoteModelConfig {
                model: Some(existing(models, &[SEGMENTATION_DIR, &segmentation_file])?),
                ..OfflineSpeakerSegmentationPyannoteModelConfig::default()
            },
            num_threads: DIARIZATION_THREADS,
            debug: false,
            provider: Some(provider.to_owned()),
        },
        embedding: SpeakerEmbeddingExtractorConfig {
            model: Some(existing(models, &[SPEAKER_FILE])?),
            num_threads: DIARIZATION_THREADS,
            debug: false,
            provider: Some(provider.to_owned()),
        },
        clustering: FastClusteringConfig {
            num_clusters: -1,
            threshold: 0.5,
            ..FastClusteringConfig::default()
        },
        ..OfflineSpeakerDiarizationConfig::default()
    };
    OfflineSpeakerDiarization::create(&config)
        .ok_or_else(|| missing("diarizer", precision, provider))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_model_is_an_error_that_names_the_file() {
        let said = existing(Path::new("no-such-models"), &["a", "b.onnx"])
            .err()
            .map(|e| e.to_string());
        assert!(
            said.as_deref().is_some_and(|t| t.contains("b.onnx")),
            "{said:?}"
        );
    }

    #[test]
    fn precisions_name_their_file_infix() {
        assert_eq!(Precision::Fp32.infix(), "");
        assert_eq!(Precision::Fp16.infix(), ".fp16");
        assert_eq!(Precision::Int8.infix(), ".int8");
        assert_eq!(Precision::Int8.name(), "int8");
    }

    #[test]
    fn a_recognizer_without_models_is_refused_before_the_c_library_is_called() {
        let made = recognizer(Path::new("no-such-models"), "cpu", Precision::Fp32);
        assert!(matches!(made, Err(EvalError::Invalid(_))));
    }
}

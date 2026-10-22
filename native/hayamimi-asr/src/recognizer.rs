//! The recognizers: one sherpa-onnx offline recognizer per tier, int8, on the
//! CPU. The Japanese tier is the ReazonSpeech zipformer transducer.

use std::ops::Range;
use std::path::{Path, PathBuf};

use hayamimi_gpu::session_lock::lock_for_session;
use hayamimi_gpu::stream_result::result_lang;
use sherpa_onnx::{
    OfflineModelConfig, OfflineNemoEncDecCtcModelConfig, OfflineOmnilingualAsrCtcModelConfig,
    OfflineParaformerModelConfig, OfflineRecognizer, OfflineRecognizerConfig,
    OfflineSenseVoiceModelConfig, OfflineStream, OfflineTransducerModelConfig,
};

use crate::audio::SAMPLE_RATE;
use crate::cap::{Cut, DecodeCap, join_sentences, pad_into};
use crate::error::AsrError;
use crate::lid::sv_lid_tag;
use crate::route::Tier;
use crate::tier::find_model;

const ENCODER: &str = "encoder-epoch-35-avg-1.int8.onnx";
const DECODER: &str = "decoder-epoch-35-avg-1.int8.onnx";
const JOINER: &str = "joiner-epoch-35-avg-1.int8.onnx";
const TOKENS: &str = "tokens.txt";
/// Threads of the recognizer.
pub const THREADS: i32 = 4;
/// The weight of hot words. The recognizer's tokens cannot encode hot words,
/// so none are given and this only matches the measured configuration.
pub const HOTWORDS_SCORE: f32 = 2.0;
/// Annotation brackets the model puts around boundary words; they carry no speech.
const JUNK: [char; 4] = ['［', '］', '〈', '〉'];

/// The model files of the Japanese recognizer, in the order encoder, decoder, joiner, tokens.
pub fn model_files(models: &Path) -> [PathBuf; 4] {
    let directory = Tier::Rz.path(models);
    [ENCODER, DECODER, JOINER, TOKENS].map(|name| directory.join(name))
}

/// The seconds of silence the start-up decode reads: longer than the longest
/// utterance of the measured sets (about 9.6 s), so that the work areas the
/// runtime takes while decoding are taken before the first utterance.
pub const WARMUP_SECONDS: u32 = 10;
const WARMUP_SAMPLES: usize = (WARMUP_SECONDS * SAMPLE_RATE) as usize;

/// The silence [`WARMUP_SECONDS`] long, as 16 kHz samples.
pub fn warmup_silence() -> Vec<f32> {
    vec![0.0; WARMUP_SAMPLES]
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

/// The path of a model file found by name prefix, as text.
fn found(directory: &Path, prefix: &str) -> Result<String, AsrError> {
    utf8(&find_model(directory, prefix)?)
}

fn transducer_of(directory: &Path) -> Result<OfflineTransducerModelConfig, AsrError> {
    Ok(OfflineTransducerModelConfig {
        encoder: Some(found(directory, "encoder")?),
        decoder: Some(found(directory, "decoder")?),
        joiner: Some(found(directory, "joiner")?),
    })
}

/// The Japanese tier: the transducer by exact file name.
fn reazon(models: &Path, config: &mut OfflineModelConfig) -> Result<(), AsrError> {
    let [encoder, decoder, joiner, _] = model_files(models);
    config.transducer = OfflineTransducerModelConfig {
        encoder: Some(utf8(&encoder)?),
        decoder: Some(utf8(&decoder)?),
        joiner: Some(utf8(&joiner)?),
    };
    // No `model_type`: "zipformer" is not a value the offline recognizer
    // knows, and it finds the type from the model itself either way.
    config.modeling_unit = Some(String::from("cjkchar"));
    Ok(())
}

/// Fills the model part of the configuration for `tier`.
fn model_of(tier: Tier, models: &Path, config: &mut OfflineModelConfig) -> Result<(), AsrError> {
    let directory = tier.path(models);
    let directory = directory.as_path();
    match tier {
        Tier::Rz => reazon(models, config)?,
        Tier::Pz => {
            config.paraformer = OfflineParaformerModelConfig {
                model: Some(found(directory, "model")?),
            };
        }
        Tier::Sv => {
            config.sense_voice = OfflineSenseVoiceModelConfig {
                model: Some(found(directory, "model")?),
                // SenseVoice finds the language itself among its five.
                language: Some(String::from("auto")),
                use_itn: true,
            };
        }
        Tier::V3 | Tier::V2 => {
            config.transducer = transducer_of(directory)?;
            config.model_type = Some(String::from("nemo_transducer"));
        }
        Tier::Omni => {
            config.omnilingual = OfflineOmnilingualAsrCtcModelConfig {
                model: Some(found(directory, "model")?),
            };
        }
        Tier::Pja => {
            config.nemo_ctc = OfflineNemoEncDecCtcModelConfig {
                model: Some(found(directory, "model")?),
            };
        }
    }
    Ok(())
}

/// The decoding of the tier: beam search for Japanese (it lowers the error
/// rate on broadcast speech for a quarter more decoding time), greedy for the rest.
fn decoding_of(tier: Tier, config: &mut OfflineRecognizerConfig) {
    if tier == Tier::Rz {
        config.decoding_method = Some(String::from("modified_beam_search"));
        config.hotwords_score = HOTWORDS_SCORE;
    } else {
        config.decoding_method = Some(String::from("greedy_search"));
    }
}

/// The language a model put on what it read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LangTag {
    /// The model does not identify languages.
    NotApplicable,
    /// The model identified this language: one of ja, en, zh, ko, yue.
    Known(String),
    /// The model identifies languages, and the tag could not be read or is none of the five.
    Unknown,
}

impl LangTag {
    /// The language code, empty when there is none to give.
    pub fn code(&self) -> &str {
        match self {
            LangTag::Known(code) => code,
            LangTag::NotApplicable | LangTag::Unknown => "",
        }
    }
}

/// The tag SenseVoice writes (`<|ja|>`) as a [`LangTag`].
pub fn tag_from(raw: &str) -> LangTag {
    match sv_lid_tag(raw) {
        Some(code) => LangTag::Known(code.to_owned()),
        None => LangTag::Unknown,
    }
}

pub struct Recognizer {
    inner: OfflineRecognizer,
    tier: Tier,
    cap: DecodeCap,
}

/// What one buffer was read as: one call on the whole buffer, which still
/// holds the whole result, or a call per piece, which leaves only the text.
enum Reading {
    Whole(OfflineStream, String),
    Pieces(String, Cut),
}

impl Recognizer {
    /// Opens the Japanese transducer under `models`, with no cap on the length
    /// of a decode. A missing file is refused first: the C library ends the
    /// whole process on a missing model.
    pub fn open(models: &Path) -> Result<Self, AsrError> {
        Recognizer::open_tier(models, Tier::Rz, THREADS, DecodeCap::Off)
    }

    /// Opens the recognizer of `tier` under `models` with `threads` threads.
    /// `cap` limits the audio of one decode of the Japanese tier.
    pub fn open_tier(
        models: &Path,
        tier: Tier,
        threads: i32,
        cap: DecodeCap,
    ) -> Result<Self, AsrError> {
        let mut config = OfflineRecognizerConfig::default();
        model_of(tier, models, &mut config.model_config)?;
        decoding_of(tier, &mut config);
        config.model_config.tokens = Some(utf8(&tier.path(models).join(TOKENS))?);
        config.model_config.provider = Some(String::from("cpu"));
        config.model_config.num_threads = threads;
        let inner = {
            let _building = lock_for_session();
            OfflineRecognizer::create(&config)
        }
        .ok_or(AsrError::Create("recognizer"))?;
        Ok(Recognizer { inner, tier, cap })
    }

    /// Runs the model on one buffer; the stream still holds the whole result.
    fn run(&self, samples: &[f32]) -> Result<(OfflineStream, String), AsrError> {
        let rate = i32::try_from(SAMPLE_RATE).map_err(|_| AsrError::Overflow("the sample rate"))?;
        let stream = self.inner.create_stream();
        stream.accept_waveform(rate, samples);
        self.inner.decode(&stream);
        let result = stream.get_result().ok_or(AsrError::NoResult)?;
        let text = clean(&result.text);
        Ok((stream, text))
    }

    /// Runs the model on each piece of `samples` and joins the sentences.
    fn run_pieces(&self, samples: &[f32], pieces: &[Range<usize>]) -> Result<String, AsrError> {
        let mut padded = Vec::new();
        let mut sentences = Vec::with_capacity(pieces.len());
        for piece in pieces {
            pad_into(&mut padded, &samples[piece.clone()]);
            sentences.push(self.run(&padded)?.1);
        }
        Ok(join_sentences(&sentences, self.cap.folds()))
    }

    /// Reads one buffer: in one call, or in pieces when the cap cuts it.
    fn read(&self, samples: &[f32]) -> Result<Reading, AsrError> {
        let Some(pieces) = self.cap.plan(self.tier, samples) else {
            let (stream, text) = self.run(samples)?;
            return Ok(Reading::Whole(stream, text));
        };
        let text = self.run_pieces(samples, &pieces)?;
        let cut = self.cap.cut_of(samples.len(), pieces.len());
        Ok(Reading::Pieces(text, cut))
    }

    /// Decodes one buffer of 16 kHz samples.
    pub fn decode(&self, samples: &[f32]) -> Result<String, AsrError> {
        match self.read(samples)? {
            Reading::Whole(_, text) | Reading::Pieces(text, _) => Ok(text),
        }
    }

    /// Decodes one buffer and reads the language the model put on it. Only
    /// SenseVoice identifies languages; a tag it did not write, or one that
    /// cannot be read, is [`LangTag::Unknown`] and does not stop the decode.
    /// A buffer read in pieces has no tag to read: it is the Japanese tier's.
    pub fn decode_tagged(&self, samples: &[f32]) -> Result<(String, LangTag), AsrError> {
        let (text, tag, _) = self.decode_noted(samples)?;
        Ok((text, tag))
    }

    /// [`Recognizer::decode_tagged`] and how the buffer was cut, when it was.
    pub(crate) fn decode_noted(
        &self,
        samples: &[f32],
    ) -> Result<(String, LangTag, Option<Cut>), AsrError> {
        Ok(match self.read(samples)? {
            Reading::Whole(stream, text) => (text, self.tag_of(&stream), None),
            Reading::Pieces(text, cut) => (text, LangTag::NotApplicable, Some(cut)),
        })
    }

    fn tag_of(&self, stream: &OfflineStream) -> LangTag {
        if self.tier != Tier::Sv {
            return LangTag::NotApplicable;
        }
        match result_lang(stream) {
            Ok(Some(raw)) => tag_from(&raw),
            Ok(None) | Err(_) => LangTag::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sensevoice_tag_is_read_as_its_language() {
        for code in ["ja", "en", "zh", "ko", "yue"] {
            assert_eq!(
                tag_from(&format!("<|{code}|>")),
                LangTag::Known(code.to_owned())
            );
        }
        assert_eq!(tag_from("<|ja|>").code(), "ja");
    }

    #[test]
    fn a_tag_of_another_language_or_none_is_unknown() {
        assert_eq!(tag_from("<|fr|>"), LangTag::Unknown);
        assert_eq!(tag_from(""), LangTag::Unknown);
        assert_eq!(tag_from("<|nospeech|>"), LangTag::Unknown);
        assert_eq!(LangTag::Unknown.code(), "");
        assert_eq!(LangTag::NotApplicable.code(), "");
    }

    #[test]
    fn the_start_up_silence_is_ten_seconds_of_zeros_at_16_khz() {
        let silence = warmup_silence();
        assert_eq!(silence.len(), 160_000);
        assert!(silence.iter().all(|sample| *sample == 0.0));
        assert_eq!(WARMUP_SECONDS, 10);
    }

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

    #[test]
    fn every_tier_without_its_files_is_refused_before_the_c_library_is_called() {
        for tier in Tier::ALL {
            let said =
                Recognizer::open_tier(Path::new("no-such-models"), tier, 1, DecodeCap::Off).err();
            assert!(
                matches!(said, Some(AsrError::MissingModel(_))),
                "{tier:?}: {said:?}"
            );
        }
    }
}

//! Japanese punctuation restoration.
//!
//! The work is split in three so that each part can be judged on its own:
//! [`tokenizer`] turns text into the ids the model reads (a table lookup),
//! `bert` runs the model (on the GPU), and [`rules`] turns the scores back into
//! a string (string rules with no model and no device).

mod bert;
pub mod rules;
pub mod tokenizer;

use std::path::Path;

use crate::error::AsrError;
use crate::ort_runtime::{Device, OrtRuntime};

pub use bert::PunctBert;
pub use tokenizer::Tokenizer;

/// The directory under the models directory that holds the punctuation model.
const MODEL_DIR: &str = "mojicast-punct-onnx";
/// The fp32 model. The int8 file of the same package gives scores that do not
/// depend on the input, and is not used.
pub const MODEL_FILE: &str = "punct_bert.onnx";
const VOCAB_FILE: &str = "vocab.txt";

/// The length, in characters, of the sentence that warms the model at
/// start-up. It is longer than the longest final sentence the evaluation
/// produced (45 characters), so the work areas the first read takes also fit
/// the shorter inputs that follow.
pub const WARMUP_CHARS: usize = 64;
// The model reads at most `MAX_CHARS` characters; a longer warm-up sentence would be cut.
const _: () = assert!(WARMUP_CHARS <= rules::MAX_CHARS);
/// The character the warm-up sentence repeats.
const WARMUP_CHAR: char = 'あ';

/// The sentence the start-up read feeds the model: [`WARMUP_CHARS`] copies of
/// one kana. It is model input only; it is never shown or logged.
pub fn warmup_sentence() -> String {
    std::iter::repeat_n(WARMUP_CHAR, WARMUP_CHARS).collect()
}

pub struct Punctuator {
    tokenizer: Tokenizer,
    bert: PunctBert,
}

impl Punctuator {
    /// Opens the vocabulary and the model under `models` on `device`.
    pub fn open(
        runtime: &OrtRuntime,
        models: &Path,
        device: Device,
        verbose: bool,
    ) -> Result<Self, AsrError> {
        let directory = models.join(MODEL_DIR);
        let vocab_path = directory.join(VOCAB_FILE);
        if !vocab_path.is_file() {
            return Err(AsrError::MissingModel(vocab_path));
        }
        let vocab = std::fs::read_to_string(&vocab_path)
            .map_err(|error| AsrError::Io(format!("{}: {error}", vocab_path.display())))?;
        let tokenizer = Tokenizer::from_vocab(&vocab)?;
        let bert = PunctBert::open(runtime, &directory.join(MODEL_FILE), device, verbose)?;
        Ok(Self { tokenizer, bert })
    }

    pub fn tokenizer(&self) -> &Tokenizer {
        &self.tokenizer
    }

    /// Inserts commas and periods into `text`, and question marks where a
    /// sentence ends in a question suffix. Text that is empty after trimming,
    /// or empty after normalisation, comes back as it is without running the model.
    pub fn restore(&mut self, text: &str) -> Result<String, AsrError> {
        let text = text.trim();
        let encoded = self.tokenizer.encode(text);
        if encoded.chars.is_empty() {
            return Ok(text.to_owned());
        }
        let logits = self.bert.logits(&encoded.ids)?;
        rules::punctuate(&encoded.chars, &logits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VOCAB: &str = "[PAD]\n[UNK]\n[CLS]\n[SEP]\nあ\n";

    #[test]
    fn the_warmup_sentence_is_sixty_four_characters_of_one_kana() {
        let sentence = warmup_sentence();
        assert_eq!(sentence.chars().count(), WARMUP_CHARS);
        assert_eq!(WARMUP_CHARS, 64);
        assert!(sentence.chars().all(|c| c == 'あ'));
    }

    #[test]
    fn the_warmup_sentence_reaches_the_model_whole() -> Result<(), AsrError> {
        let encoded = Tokenizer::from_vocab(VOCAB)?.encode(&warmup_sentence());
        assert_eq!(encoded.chars.len(), WARMUP_CHARS);
        assert_eq!(encoded.ids.len(), WARMUP_CHARS + 2);
        // Every id after [CLS] and before [SEP] is the kana, never [UNK].
        assert!(encoded.ids[1..=WARMUP_CHARS].iter().all(|id| *id == 4));
        Ok(())
    }
}

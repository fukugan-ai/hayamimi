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

//! The punctuation BERT as an ONNX Runtime session.

use std::path::Path;

use ort::session::Session;
use ort::value::Tensor;

use crate::error::AsrError;
use crate::ort_runtime::{Device, OrtRuntime};

/// Threads of the session; the recognizer shares the machine.
const THREADS: usize = 2;
/// Scores per position: comma and period.
const CLASSES: usize = 2;

fn ort_error(error: impl std::fmt::Display) -> AsrError {
    AsrError::Ort(error.to_string())
}

pub struct PunctBert {
    session: Session,
}

impl PunctBert {
    /// Opens the model file on `device`. `verbose` makes ONNX Runtime log the node placement.
    pub fn open(
        runtime: &OrtRuntime,
        model: &Path,
        device: Device,
        verbose: bool,
    ) -> Result<Self, AsrError> {
        if !model.is_file() {
            return Err(AsrError::MissingModel(model.to_path_buf()));
        }
        let session = runtime.session(model, device, THREADS, verbose)?;
        Ok(Self { session })
    }

    /// One forward pass over `ids` (`[CLS]`, characters, `[SEP]`) with an
    /// all-ones attention mask. Returns the logits as a flat
    /// `[ids.len(), 2]` array.
    pub fn logits(&mut self, ids: &[i64]) -> Result<Vec<f32>, AsrError> {
        let len = ids.len();
        let input_ids = Tensor::from_array(([1_usize, len], ids.to_vec())).map_err(ort_error)?;
        let mask = Tensor::from_array(([1_usize, len], vec![1_i64; len])).map_err(ort_error)?;
        let outputs = self
            .session
            .run(ort::inputs!["input_ids" => input_ids, "attention_mask" => mask])
            .map_err(ort_error)?;
        let (shape, data) = outputs["logits"]
            .try_extract_tensor::<f32>()
            .map_err(|error| AsrError::Shape(format!("the logits are not float32 ({error})")))?;
        let rows = i64::try_from(len).map_err(|_| AsrError::Overflow("the sequence length"))?;
        let classes = i64::try_from(CLASSES).map_err(|_| AsrError::Overflow("the class count"))?;
        if shape[..] != [1, rows, classes] {
            return Err(AsrError::Shape(format!(
                "shape {shape:?}, want [1, {rows}, {classes}]"
            )));
        }
        Ok(data.to_vec())
    }
}

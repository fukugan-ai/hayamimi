//! The errors of the recognition route.

use std::fmt;
use std::path::PathBuf;

#[derive(Debug)]
pub enum AsrError {
    /// A model file is not there. The C library ends the whole process on a
    /// missing model instead of reporting it, so the path is checked first.
    MissingModel(PathBuf),
    /// sherpa-onnx refused to build a model; the name says which.
    Create(&'static str),
    /// The recognizer answered a decode with no result.
    NoResult,
    /// ONNX Runtime (through `ort`) failed.
    Ort(String),
    /// The vocabulary file lacks a token the tokenizer needs.
    Vocab(String),
    /// The punctuation model answered with another shape than the design says.
    Shape(String),
    /// A wav file could not be read.
    Wav(String),
    /// A file could not be read.
    Io(String),
    /// Loading the ONNX Runtime beside the executable failed.
    Gpu(hayamimi_gpu::error::GpuError),
    /// A count does not fit the integer type of the C API.
    Overflow(&'static str),
    /// The clock reads a time before 1970.
    Clock,
}

impl fmt::Display for AsrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AsrError::MissingModel(path) => write!(f, "model file not found: {}", path.display()),
            AsrError::Create(what) => write!(f, "cannot create the {what}"),
            AsrError::NoResult => write!(f, "the recognizer returned no result"),
            AsrError::Ort(message) => write!(f, "ort: {message}"),
            AsrError::Vocab(message) => write!(f, "vocab: {message}"),
            AsrError::Shape(message) => write!(f, "punctuation model output: {message}"),
            AsrError::Wav(message) => write!(f, "wav: {message}"),
            AsrError::Io(message) => write!(f, "io: {message}"),
            AsrError::Gpu(error) => write!(f, "gpu: {error}"),
            AsrError::Overflow(what) => write!(f, "{what} does not fit the integer type"),
            AsrError::Clock => write!(f, "the clock reads a time before 1970"),
        }
    }
}

impl std::error::Error for AsrError {}

impl From<hayamimi_gpu::error::GpuError> for AsrError {
    fn from(error: hayamimi_gpu::error::GpuError) -> Self {
        AsrError::Gpu(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_model_names_its_path() {
        let said = AsrError::MissingModel(PathBuf::from("models/x.onnx")).to_string();
        assert!(said.contains("models/x.onnx"), "{said}");
    }

    #[test]
    fn a_creation_failure_names_the_model() {
        assert!(AsrError::Create("VAD").to_string().contains("VAD"));
    }
}

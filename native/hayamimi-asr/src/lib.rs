//! The recognition route of hayamimi.
//!
//! `itn` and `punct::rules` are string rules: they read and write text and
//! have no model and no device. The punctuation model (`punct`) runs on the
//! GPU through `ort`; the voice detector and the recognizers run on the CPU
//! through sherpa-onnx. `decode` keeps the recognizers (`resident` decides
//! which stay in memory) and decodes with the tier a caller names.

pub mod audio;
pub mod decode;
pub mod error;
pub mod fallback;
pub mod itn;
pub mod ort_runtime;
pub mod pipeline;
pub mod punct;
pub mod recognizer;
pub mod replace;
pub mod resident;
pub mod retry;
pub mod tier;
pub mod vad;

pub use decode::{Decoded, Recognizers, ResidentEvent};
pub use error::AsrError;
pub use fallback::{Fallback, Gpu, Reason, Stage, To};
pub use ort_runtime::check_library;
pub use pipeline::{
    DEFAULT_MAX_RESIDENT, Demotion, Engine, Final, SessionReport, Settings, Stages, Timings,
};
pub use recognizer::LangTag;
pub use tier::Tier;

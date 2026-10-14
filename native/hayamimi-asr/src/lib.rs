//! The Japanese recognition route of hayamimi.
//!
//! `itn` and `punct::rules` are string rules: they read and write text and
//! have no model and no device. The punctuation model (`punct`) runs on the
//! GPU through `ort`; the voice detector and the recognizer run on the CPU
//! through sherpa-onnx.

pub mod audio;
pub mod error;
pub mod fallback;
pub mod itn;
pub mod ort_runtime;
pub mod partial;
pub mod pipeline;
pub mod punct;
pub mod recognizer;
pub mod replace;
pub mod retry;
pub mod vad;

pub use error::AsrError;
pub use fallback::{Fallback, Gpu, Reason, Stage, To};
pub use ort_runtime::check_library;
pub use pipeline::{Engine, Final, SessionReport, Settings, Stages, Timings};

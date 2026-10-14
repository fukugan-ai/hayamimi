//! `hayamimi llama start`: starts the translation model server (llama.cpp's
//! `llama-server`) as a Windows process of its own, so that it keeps running
//! when the terminal that started it is closed and when WSL stops.
//!
//! The server runs on the GPU when the start finds a CUDA device. Without one,
//! or when the server cannot come up on the GPU, the same model is started on
//! the CPU and the start says so (stage `translate`, event `gpu.fallback.cpu`).
//! It is not an error: the translation is slower and the subtitles go on.

pub mod cli;
pub mod handles;
pub mod keys;
pub mod layout;
pub mod options;
pub mod plan;
pub mod report;
pub mod spawn;
pub mod start;

pub use cli::run;

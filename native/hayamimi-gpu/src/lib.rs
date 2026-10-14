//! Opens the self-built CUDA flavour of sherpa-onnx and reports where
//! inference runs: how many CUDA devices exist, which provider library the
//! session loaded, and which CUDA runtime libraries came with it.
//!
//! The libraries are built by `cargo run -p xtask -- sherpa-gpu`; the steps are
//! in `docs/guide/build_gpu.ja.md`.

pub mod cuda;
pub mod error;
pub mod link_args;
pub mod memory;
pub mod modules;
pub mod probe;
pub mod rp17;
pub mod runtime;
pub mod stream_result;
pub mod transfer;

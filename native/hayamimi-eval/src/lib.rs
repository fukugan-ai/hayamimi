//! Development-only measurement for hayamimi: it plays recordings into a
//! running server and times the answers, records what a server says on the
//! wire, pins evaluation sets to their bytes, and builds the sentence pairs
//! a translation scorer needs. Nothing here is on the runtime path and the
//! runtime binary does not link it.

pub mod cer;
pub mod cli;
pub mod error;
pub mod fleurs;
pub mod ingest;
pub mod latency;
pub mod log;
pub mod manifest;
pub mod mt;
pub mod percentile;
pub mod record;
pub mod sha;
pub mod stream;
pub mod wav;
pub mod ws;

#[cfg(test)]
mod testutil;

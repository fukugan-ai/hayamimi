//! Development-only measurement for hayamimi: it plays recordings into a
//! running server and times the answers, records what a server says on the
//! wire, pins evaluation sets to their bytes, and builds the sentence pairs
//! a translation scorer needs. Nothing here is on the runtime path and the
//! runtime binary does not link it.

pub mod cer;
pub mod cli;
pub mod cli_stages;
pub mod error;
pub mod fleurs;
pub mod gpu_memory;
pub mod gpu_nodes;
pub mod gpu_results;
pub mod gpu_sessions;
pub mod ingest;
pub mod latency;
pub mod log;
pub mod manifest;
pub mod mdtable;
pub mod models_manifest;
pub mod mt;
pub mod nsys;
pub mod ort_stages;
pub mod percentile;
pub mod record;
pub mod sha;
pub mod speaker;
pub mod stage_load;
pub mod stage_models;
pub mod stage_report;
pub mod stage_runs;
pub mod stage_table;
pub mod stages;
pub mod stream;
pub mod translate_call;
pub mod wav;
pub mod ws;

#[cfg(test)]
mod testutil;

//! hayamimi: the command line, the locale tables, the log events and the
//! subtitle event server around the Japanese recognition route.

pub mod api;
pub mod args;
pub mod capture;
pub mod config_dump;
pub mod control;
pub mod feed;
pub mod gpu_check;
pub mod ingest;
pub mod inputs;
pub mod llama;
pub mod log;
pub mod memory_guard;
pub mod models_fetch;
pub mod notice;
pub mod report;
pub mod routes;
pub mod run;
pub mod server;
pub mod session;
pub mod strings;
pub mod ui;
pub mod wire;

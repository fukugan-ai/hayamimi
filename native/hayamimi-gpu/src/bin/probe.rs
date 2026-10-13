//! `hayamimi-gpu-probe --encoder <f> --decoder <f> --joiner <f> --tokens <f> --wav <f>`
//!
//! Opens one wave file on the CUDA provider and prints one JSON line with what
//! the process loaded to do it.

use std::io::Write;
use std::process::ExitCode;

use hayamimi_gpu::error::GpuError;
use hayamimi_gpu::modules;
use hayamimi_gpu::probe::{self, ProbeArgs};

/// `--modules` alone prints the reported modules loaded before any model is opened.
fn run(args: &[String]) -> Result<(), GpuError> {
    if args == ["--modules"] {
        let rows = probe::reported(modules::loaded_modules()?);
        let line = serde_json::to_string(&rows).map_err(GpuError::Json)?;
        return writeln!(std::io::stdout(), "{line}").map_err(GpuError::Io);
    }
    let parsed = ProbeArgs::parse(args)?;
    let report = probe::run(&parsed)?;
    let line = serde_json::to_string(&report).map_err(GpuError::Json)?;
    writeln!(std::io::stdout(), "{line}").map_err(GpuError::Io)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("hayamimi-gpu-probe: {error}");
            ExitCode::FAILURE
        }
    }
}

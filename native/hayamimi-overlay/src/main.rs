//! `hayamimi-overlay`: the subtitle window. See the library for what it does.
//!
//! On Windows it runs under the GUI subsystem, so it has no console and its
//! log is a file (`--log`). Elsewhere it only says that it cannot run.

#![cfg_attr(windows, windows_subsystem = "windows")]

use std::process::ExitCode;

#[cfg(windows)]
fn main() -> ExitCode {
    hayamimi_overlay::app::main(std::env::args().skip(1))
}

#[cfg(not(windows))]
fn main() -> ExitCode {
    eprintln!("hayamimi-overlay: the subtitle window runs on Windows only");
    ExitCode::from(2)
}

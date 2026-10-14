//! `hayamimi llama start`: the GPU check, the attempts, and what is said.
//!
//! A machine without a GPU is not stopped: the server runs on the CPU and the
//! start says so once in the log and once to the user. A start that leaves no
//! server ends with a code other than 0; the lines of a translation are then
//! held by the relay and the original text goes on.

use std::path::PathBuf;
use std::time::Duration;

use hayamimi_asr::Gpu;

use super::keys;
use super::layout::Layout;
use super::options::{self, OptionError, Options, Parsed};
use super::plan::{Plan, attempts, fallback_reason};
use super::report;
use super::start::{self, AttemptError, Installed, Started};
use crate::gpu_check;
use crate::log;
use crate::notice::Notice;
use crate::strings::{self, Strings};

/// A server answers.
pub const EXIT_OK: u8 = 0;
/// The start left no server.
pub const EXIT_FAILED: u8 = 1;
/// The command line was refused.
pub const EXIT_REFUSED: u8 = 2;

fn say(strings: &Strings, keys: &[&str]) {
    for key in keys {
        eprintln!("{}", strings.cli_text(key));
    }
}

/// The install root: the given directory, or `hayamimi` in the user's local
/// data directory (`%LOCALAPPDATA%` on Windows).
fn root_of(options: &Options) -> Option<PathBuf> {
    match &options.dir {
        Some(dir) => Some(dir.clone()),
        None => dirs::data_local_dir().map(|local| local.join("hayamimi")),
    }
}

/// The first GPU check; a machine without a device is logged and goes on.
fn check_gpu(strings: &Strings) -> Gpu {
    let gpu = gpu_check::preflight();
    if let Gpu::Absent(why) = gpu {
        log::print(&log::device_unavailable_line(strings, why));
    }
    gpu
}

/// One event for each way of starting that did not come up, in the order tried.
fn print_failures(strings: &Strings, failed: &[(Plan, AttemptError)]) {
    for (plan, error) in failed {
        log::print(&report::failed_line(strings, *plan, *error));
    }
}

/// Says the outcome of a start that left a server: the ways that failed
/// first, then where the server runs and, when it is on the CPU, why.
fn tell_started(
    strings: &Strings,
    started: &Started,
    gpu: Gpu,
    options: &Options,
    tag: Option<&str>,
) {
    print_failures(strings, &started.failed);
    if let Some(reason) = fallback_reason(gpu, started.plan) {
        let notice = Notice::translate_on_cpu(reason);
        log::print(&log::fallback_line(strings, &notice));
        for key in report::fallback_keys(&notice) {
            eprintln!("{}", strings.cli_text(&key));
        }
    }
    log::print(&report::started_line(strings, started, options.port, tag));
    say(strings, &[keys::STARTED]);
    eprintln!("{}", start::base_url(options.port));
}

/// Says a start that left no server, and returns the exit code.
fn tell_unavailable(strings: &Strings, failed: &[(Plan, AttemptError)]) -> u8 {
    print_failures(strings, failed);
    log::print(&report::unavailable_line(strings, "attempts_failed"));
    say(strings, &[keys::UNAVAILABLE]);
    EXIT_FAILED
}

fn start_server(options: &Options, strings: &Strings) -> u8 {
    let Some(root) = root_of(options) else {
        say(strings, &[keys::NO_DATA_DIR]);
        return EXIT_REFUSED;
    };
    let layout = Layout::new(root);
    let model = layout.model(&options.model);
    if !model.is_file() {
        log::print(&report::unavailable_line(strings, "model_missing"));
        say(strings, &[keys::MODEL_MISSING]);
        eprintln!("{}", model.display());
        return EXIT_FAILED;
    }
    if start::answers(options.port) {
        log::print(&report::running_line(strings, options.port));
        say(strings, &[keys::RUNNING]);
        eprintln!("{}", start::base_url(options.port));
        return EXIT_OK;
    }
    let gpu = check_gpu(strings);
    let tag = layout.tag();
    let mut launcher = Installed {
        layout,
        model,
        port: options.port,
        wait: Duration::from_secs(options.wait_seconds),
    };
    match start::first_that_runs(&mut launcher, &attempts(gpu)) {
        Ok(started) => {
            tell_started(strings, &started, gpu, options, tag.as_deref());
            EXIT_OK
        }
        Err(failed) => tell_unavailable(strings, &failed),
    }
}

/// Runs `hayamimi llama <args>` and returns the exit code.
pub fn run(args: &[String], locale_tag: Option<&str>) -> u8 {
    let Ok(strings) = Strings::embedded(strings::resolve(locale_tag)) else {
        // The compiled-in tables are checked by a test; without one there is no text to print.
        return EXIT_FAILED;
    };
    match options::parse(args) {
        Ok(Parsed::Help) => {
            println!("{}", strings.cli_text(keys::HELP));
            EXIT_OK
        }
        Ok(Parsed::Start(options)) => start_server(&options, &strings),
        Err(error) => refuse(&strings, error),
    }
}

fn refuse(strings: &Strings, error: OptionError) -> u8 {
    say(strings, &[error.key(), strings::keys::USAGE_HINT]);
    EXIT_REFUSED
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn a_command_line_without_a_command_is_refused() {
        assert_eq!(run(&[], None), EXIT_REFUSED);
        assert_eq!(run(&words("start"), None), EXIT_REFUSED);
    }

    #[test]
    fn help_succeeds() {
        assert_eq!(run(&words("--help"), None), EXIT_OK);
    }

    #[test]
    fn a_model_that_is_not_there_ends_the_start_before_anything_is_tried() {
        let root = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../target/unit-tmp/no-llama-root"
        );
        let line = format!("start --model absent.gguf --dir {root}");
        assert_eq!(run(&words(&line), None), EXIT_FAILED);
    }

    #[test]
    fn the_exit_codes_are_distinct_and_only_ok_is_zero() {
        assert_eq!(EXIT_OK, 0);
        assert_ne!(EXIT_FAILED, EXIT_REFUSED);
        const { assert!(EXIT_FAILED != 0 && EXIT_REFUSED != 0) };
    }
}

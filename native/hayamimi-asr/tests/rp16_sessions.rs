//! RP-16, the session-creation row: sessions of the identifier, the
//! recognizer and the punctuation model opened by threads of one process never
//! end it, because every build takes the process-wide lock of `hayamimi-gpu`.
//!
//! The work is done by `child_*` tests that the parents start in copies of this
//! test binary with two test threads. The parents count how many copies ended
//! on "Rust cannot catch foreign exceptions" or on a signal. The lock is
//! switched off inside the `child_unlocked_*` tests only, which shows that the
//! count is not zero for want of a race.

mod support;

use std::process::{Command, Output};

use hayamimi_asr::lid::{self, Identify};
use hayamimi_asr::ort_runtime::{Device, OrtRuntime};
use hayamimi_asr::punct::Punctuator;
use hayamimi_asr::recognizer::Recognizer;
use hayamimi_asr::{Gpu, Reason, Tier};
use hayamimi_gpu::session_lock::set_enabled_for_test;
use support::{GOLDEN, TestResult, models_dir};

const RUNS: usize = 20;
const ABORT_MESSAGE: &str = "Rust cannot catch foreign exceptions";

/// Runs the `child_*` tests whose names contain `filter`, two at a time.
fn run_children(filter: &str) -> Result<Output, std::io::Error> {
    Command::new(std::env::current_exe()?)
        .args([filter, "--ignored", "--nocapture", "--test-threads=2"])
        .output()
}

fn text_of(output: &Output) -> String {
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    text
}

/// Whether the copy ended on the foreign exception or on a signal.
fn aborted(output: &Output) -> bool {
    !output.status.success()
        && (output.status.code().is_none() || text_of(output).contains(ABORT_MESSAGE))
}

/// How many of `RUNS` copies aborted. A copy that failed another way is an error.
fn aborts_in(filter: &str) -> Result<usize, Box<dyn std::error::Error>> {
    let mut aborts = 0;
    for run in 0..RUNS {
        let output = run_children(filter)?;
        if aborted(&output) {
            aborts += 1;
        } else if !output.status.success() {
            return Err(format!("run {run} failed without an abort: {}", text_of(&output)).into());
        }
    }
    Ok(aborts)
}

// ---- the work of the children ----

/// Opens the identifier and names the clip `passes` times, so that one test
/// is still running inference while the other builds its session.
fn open_identifier(gpu: Gpu, passes: usize) -> TestResult {
    let opened = lid::open(&models_dir()?, gpu)?;
    let Some(identifier) = opened.lid else {
        return Err("the identifier was not built".into());
    };
    let clip = hayamimi_asr::audio::read_wav(&std::path::Path::new(GOLDEN).join("ja_024.wav"))?;
    for _ in 0..passes {
        identifier.identify(&clip)?;
    }
    Ok(())
}

fn open_identifier_on_cpu() -> TestResult {
    open_identifier(Gpu::Absent(Reason::NoDevice), 8)
}

fn open_identifier_on_gpu() -> TestResult {
    open_identifier(Gpu::Try, 1)
}

fn open_punctuation() -> TestResult {
    let runtime = OrtRuntime::shared()?;
    Punctuator::open(runtime, &models_dir()?, Device::Cuda, false)?;
    Ok(())
}

fn open_recognizer() -> TestResult {
    Recognizer::open_tier(&models_dir()?, Tier::Rz, 1)?;
    Ok(())
}

#[test]
#[ignore = "run by the parents"]
fn child_locked_a_identifier() -> TestResult {
    open_identifier_on_cpu()
}

#[test]
#[ignore = "run by the parents"]
fn child_locked_b_identifier() -> TestResult {
    open_identifier_on_gpu()
}

#[test]
#[ignore = "run by the parents"]
fn child_locked_c_punctuation() -> TestResult {
    open_punctuation()
}

#[test]
#[ignore = "run by the parents"]
fn child_locked_d_recognizer() -> TestResult {
    open_recognizer()
}

#[test]
#[ignore = "run by the parents, with the lock off"]
fn child_unlocked_a_identifier() -> TestResult {
    set_enabled_for_test(false);
    open_identifier_on_cpu()
}

#[test]
#[ignore = "run by the parents, with the lock off"]
fn child_unlocked_b_identifier() -> TestResult {
    set_enabled_for_test(false);
    open_identifier_on_gpu()
}

#[test]
#[ignore = "run by the parents, with the lock off"]
fn child_unlocked_c_punctuation() -> TestResult {
    set_enabled_for_test(false);
    open_punctuation()
}

#[test]
#[ignore = "run by the parents, with the lock off"]
fn child_unlocked_d_recognizer() -> TestResult {
    set_enabled_for_test(false);
    open_recognizer()
}

// ---- the parents ----

/// T-RP-16-036
#[test]
#[ignore = "starts 20 child processes that open model sessions"]
fn opening_sessions_in_parallel_never_aborts() -> TestResult {
    let aborts = aborts_in("child_locked_")?;
    assert_eq!(
        aborts, 0,
        "{aborts} of {RUNS} runs aborted with the lock on"
    );
    Ok(())
}

/// T-RP-16-037, the negative control: without the lock the same test can abort.
#[test]
#[ignore = "starts 20 child processes that abort on purpose"]
fn opening_sessions_without_the_lock_can_abort() -> TestResult {
    let aborts = aborts_in("child_unlocked_")?;
    eprintln!("aborts without the lock: {aborts} of {RUNS}");
    assert!(aborts >= 1, "no run aborted in {RUNS} without the lock");
    Ok(())
}

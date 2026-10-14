//! A start that is told `Gpu::Try` while no device is visible must go to the
//! CPU, say so once, and stay alive. Asking the inference library for a CUDA
//! session with no device behind it ends the process, so the driver is asked
//! again before any such session is built (`Gpu::confirmed`).
//!
//! Each case runs a child copy of this test binary with the device hidden
//! (`CUDA_VISIBLE_DEVICES` empty, set on the child only), because a process
//! that ends cannot report on itself. The `child_*` tests are the work the
//! child does; they are ignored so that only the parents start them.

mod support;

use std::process::{Command, Output};

use hayamimi_asr::lid::{self, Identify};
use hayamimi_asr::{Engine, Fallback, Gpu, Settings, check_library};
use support::{GOLDEN, TestResult, models_dir};

const ALIVE: &str = "GUARD alive";

fn fallback_line(step: &Fallback) -> String {
    format!(
        "GUARD fallback {} {} {}",
        step.stage.name(),
        step.reason.name(),
        step.to.name()
    )
}

/// Runs one `child_*` test in a copy of this binary, with the device hidden or not.
fn run_child(name: &str, hidden: bool) -> Result<Output, std::io::Error> {
    let mut child = Command::new(std::env::current_exe()?);
    child.args([
        name,
        "--exact",
        "--ignored",
        "--nocapture",
        "--test-threads=1",
    ]);
    if hidden {
        child.env("CUDA_VISIBLE_DEVICES", "");
    }
    child.output()
}

fn lines_of(output: &Output) -> Vec<String> {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_owned)
        .collect()
}

fn count(lines: &[String], wanted: &str) -> usize {
    // The harness starts the first line of a test's output after its own "test name ... ".
    lines.iter().filter(|line| line.contains(wanted)).count()
}

fn identifies_the_clip(opened: &lid::Opened) -> TestResult {
    let Some(identifier) = &opened.lid else {
        return Err("the identifier was not built".into());
    };
    let clip = hayamimi_asr::audio::read_wav(&std::path::Path::new(GOLDEN).join("ja_024.wav"))?;
    assert_eq!(identifier.identify(&clip)?, "ja");
    Ok(())
}

// ---- the work of the child ----

#[test]
#[ignore = "run by the parents, with the device hidden"]
fn child_lid_try() -> TestResult {
    let opened = lid::open(&models_dir()?, Gpu::Try)?;
    if let Some(step) = &opened.fallback {
        println!("{}", fallback_line(step));
    }
    identifies_the_clip(&opened)?;
    println!("{ALIVE}");
    Ok(())
}

#[test]
#[ignore = "run by the parents, with the device hidden"]
fn child_lid_unchecked() -> TestResult {
    let opened = lid::open_as_told(&models_dir()?, Gpu::Try)?;
    identifies_the_clip(&opened)?;
    println!("{ALIVE}");
    Ok(())
}

#[test]
#[ignore = "run by the parents, with the device hidden"]
fn child_engine_try() -> TestResult {
    check_library()?;
    let engine = Engine::open(Settings::new(models_dir()?))?;
    for step in engine.fallbacks() {
        println!("{}", fallback_line(step));
    }
    for session in engine.sessions() {
        println!("GUARD session {} {}", session.model, session.actual);
    }
    println!("{ALIVE}");
    Ok(())
}

// ---- the parents ----

/// T-RP-16-027
#[test]
#[ignore = "runs the whisper-tiny identifier in a child process"]
fn hidden_gpu_with_try_sends_the_identifier_to_the_cpu_and_stays_alive() -> TestResult {
    let output = run_child("child_lid_try", true)?;
    let lines = lines_of(&output);
    assert!(output.status.success(), "{:?}", output.status);
    assert_eq!(count(&lines, ALIVE), 1);
    assert_eq!(
        count(&lines, "GUARD fallback lid no_device cpu"),
        1,
        "{lines:?}"
    );
    Ok(())
}

/// T-RP-16-028
#[test]
#[ignore = "opens the whole engine in a child process"]
fn hidden_gpu_with_try_sends_the_punctuation_model_to_the_cpu_and_stays_alive() -> TestResult {
    let output = run_child("child_engine_try", true)?;
    let lines = lines_of(&output);
    assert!(output.status.success(), "{:?}", output.status);
    assert_eq!(count(&lines, ALIVE), 1);
    assert_eq!(
        count(&lines, "GUARD fallback punct no_device cpu"),
        1,
        "{lines:?}"
    );
    let on_cuda = lines.iter().filter(|line| line.ends_with(" cuda")).count();
    assert_eq!(on_cuda, 0, "{lines:?}");
    Ok(())
}

/// T-RP-16-029, the positive control: with the device visible nothing is said.
#[test]
#[ignore = "runs the whisper-tiny identifier in a child process, on the GPU"]
fn visible_gpu_with_try_stays_on_the_gpu() -> TestResult {
    let output = run_child("child_lid_try", false)?;
    let lines = lines_of(&output);
    assert!(output.status.success(), "{:?}", output.status);
    assert_eq!(count(&lines, ALIVE), 1);
    assert!(
        lines.iter().all(|line| !line.contains("GUARD fallback")),
        "{lines:?}"
    );
    Ok(())
}

/// T-RP-16-030, the negative control: without the check the child ends.
#[test]
#[ignore = "ends a child process on purpose"]
fn hidden_gpu_without_the_check_ends_the_process() -> TestResult {
    let output = run_child("child_lid_unchecked", true)?;
    assert!(
        !output.status.success(),
        "the child survived: {:?}",
        output.status
    );
    assert_eq!(count(&lines_of(&output), ALIVE), 0);
    Ok(())
}

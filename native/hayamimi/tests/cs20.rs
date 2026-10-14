//! CS-20: the translation server that `hayamimi llama start` leaves keeps
//! answering `/health` after the command, and the terminal it was typed in,
//! are gone. The cases run the real command against a stand-in server; the
//! acceptance test (`--ignored`, on Windows) asks the real server after the
//! terminals are closed and WSL is shut down, by hand.

mod support;

use std::process::Command;
use std::time::Duration;

use hayamimi_app::llama::options::DEFAULT_PORT;
use hayamimi_app::llama::plan::Build;
use support::llama::{Session, alive, health};
use support::{Failure, HIDE_GPU, TestResult};

/// The longest a start of a server that answers at once may take.
const QUICK: Duration = Duration::from_secs(10);

fn started(name: &str) -> Result<Session, Failure> {
    Session::start(
        name,
        &[Build::Cuda, Build::Cpu],
        "ok.gguf",
        None,
        &[HIDE_GPU],
    )
}

/// T-CS-20-001
#[test]
fn server_answers_after_the_start_command_ended() -> TestResult {
    let session = started("answers")?;
    let launch = &session.launch;
    assert_eq!(launch.output.code, Some(0), "{}", launch.output.stderr);
    let pid = launch.pid().ok_or("the start named no server")?;
    assert!(alive(pid), "process {pid} is not running");
    assert_eq!(health(launch.port)?, "ok");
    Ok(())
}

/// T-CS-20-002
#[test]
fn killed_server_does_not_answer() -> TestResult {
    let mut session = started("killed")?;
    // The control: the same request is answered before the server is stopped.
    assert_eq!(health(session.launch.port)?, "ok");
    session.launch.stop()?;
    let after = health(session.launch.port);
    assert!(
        after
            .as_ref()
            .is_err_and(|text| text.contains("cannot connect")),
        "{after:?}"
    );
    Ok(())
}

/// T-CS-20-003
#[test]
fn start_command_returns_while_the_server_keeps_running() -> TestResult {
    let session = started("returns")?;
    let launch = &session.launch;
    assert!(launch.took < QUICK, "the start took {:?}", launch.took);
    assert_eq!(health(launch.port)?, "ok");
    Ok(())
}

/// The process group of a process, from the fifth field of its stat line.
#[cfg(target_os = "linux")]
fn group_of(pid: &str) -> Result<String, Failure> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .map_err(|error| format!("cannot read the stat of {pid}: {error}"))?;
    // The command name is in parentheses and may hold spaces; the fields
    // that follow it start after the last closing parenthesis.
    let (_, after) = stat
        .rsplit_once(')')
        .ok_or("no command name in the stat line")?;
    let group = after.split_whitespace().nth(2).ok_or("no group field")?;
    Ok(group.to_owned())
}

/// T-CS-20-004
#[cfg(target_os = "linux")]
#[test]
fn server_is_in_a_process_group_of_its_own() -> TestResult {
    let session = started("group")?;
    let pid = session
        .launch
        .pid()
        .ok_or("the start named no server")?
        .to_string();
    let (mine, theirs) = (group_of("self")?, group_of(&pid)?);
    assert_ne!(mine, theirs);
    assert_eq!(theirs, pid);
    Ok(())
}

/// T-CS-20-005: the acceptance test of CS-20. Run it on Windows after
/// `hayamimi llama start` was typed, every terminal was closed and
/// `wsl --shutdown` was typed (docs/guide/llama_windows.ja.md).
#[test]
#[ignore = "needs the manual steps of docs/guide/llama_windows.ja.md"]
fn cs20_windows_translator_survives_wsl_shutdown() -> TestResult {
    let script = format!("(Invoke-RestMethod http://127.0.0.1:{DEFAULT_PORT}/health).status");
    let answer = Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", &script])
        .output()?;
    let said = String::from_utf8_lossy(&answer.stdout);
    assert_eq!(
        said.trim(),
        "ok",
        "{}",
        String::from_utf8_lossy(&answer.stderr)
    );
    Ok(())
}

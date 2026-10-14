//! Starting the server so that it outlives this process and the terminal it
//! was typed in. The server's output goes to a log file; its input is closed,
//! so nothing of this process's pipes is held open by the child.

use std::ffi::OsString;
use std::fs::File;
use std::io;
use std::path::Path;
use std::process::{Child, Command, Stdio};

use super::handles::keep_inherited_handles_from_children;

/// The new process has no console of its own and is not attached to this one.
const DETACHED_PROCESS: u32 = 0x0000_0008;
/// The new process is the root of a process group, so a Ctrl+C or a Ctrl+Break
/// sent to this console's group does not reach it.
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
/// The new process leaves the job object of this one, whose closing would
/// otherwise end it (a terminal or a remote session closing). Refused with
/// access denied when the job forbids leaving.
const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;

/// The creation flags to try on Windows, strongest first.
pub const DETACH_FLAGS: [u32; 2] = [
    DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB,
    DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP,
];

/// Opens the log for this start. The previous start's log is kept as
/// `<name>.prev.log`, one generation, so that the output of a server that died
/// is still there after the next start and the file does not grow without end.
fn open_log(log: &Path) -> io::Result<File> {
    if let Some(directory) = log.parent() {
        std::fs::create_dir_all(directory)?;
    }
    if log.exists() {
        std::fs::rename(log, log.with_extension("prev.log"))?;
    }
    File::create(log)
}

/// A command for `exe` whose three standard streams are closed or in the log.
fn command(exe: &Path, args: &[OsString], log: &File) -> io::Result<Command> {
    let mut command = Command::new(exe);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log.try_clone()?);
    Ok(command)
}

#[cfg(windows)]
fn start(exe: &Path, args: &[OsString], log: &File) -> io::Result<Child> {
    use std::os::windows::process::CommandExt;
    let mut last = io::Error::from(io::ErrorKind::Unsupported);
    for flags in DETACH_FLAGS {
        match command(exe, args, log)?.creation_flags(flags).spawn() {
            Ok(child) => return Ok(child),
            // Leaving a job that forbids it is refused; the next flags stay in it.
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => last = error,
            Err(error) => return Err(error),
        }
    }
    Err(last)
}

#[cfg(not(windows))]
fn start(exe: &Path, args: &[OsString], log: &File) -> io::Result<Child> {
    use std::os::unix::process::CommandExt;
    command(exe, args, log)?.process_group(0).spawn()
}

/// Starts `exe` detached and returns its handle. The caller waits for the
/// child or kills and waits for it; a child that is left to run on purpose is
/// dropped without a wait.
pub fn spawn_detached(exe: &Path, args: &[OsString], log: &Path) -> io::Result<Child> {
    let log = open_log(log)?;
    keep_inherited_handles_from_children();
    start(exe, args, &log)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/unit-tmp");
        base.join(format!("llama-spawn-{}-{name}", std::process::id()))
    }

    #[test]
    fn the_strongest_flags_detach_leave_the_job_and_start_a_group() {
        let [strong, weak] = DETACH_FLAGS;
        for flag in [DETACHED_PROCESS, CREATE_NEW_PROCESS_GROUP] {
            assert_eq!(strong & flag, flag);
            assert_eq!(weak & flag, flag);
        }
        assert_eq!(
            strong & CREATE_BREAKAWAY_FROM_JOB,
            CREATE_BREAKAWAY_FROM_JOB
        );
        assert_eq!(weak & CREATE_BREAKAWAY_FROM_JOB, 0);
    }

    #[test]
    fn a_second_start_keeps_the_first_log_as_the_previous_one()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = scratch("rotate");
        let log = directory.join("log").join("server.log");
        drop(open_log(&log)?);
        std::fs::write(&log, "first")?;
        drop(open_log(&log)?);
        let previous = std::fs::read_to_string(directory.join("log").join("server.prev.log"))?;
        let current = std::fs::read_to_string(&log)?;
        std::fs::remove_dir_all(&directory)?;
        assert_eq!(previous, "first");
        assert_eq!(current, "");
        Ok(())
    }

    #[test]
    fn a_program_that_does_not_exist_is_an_error_and_leaves_no_child()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = scratch("missing");
        let started = spawn_detached(
            Path::new("no-such-program-for-hayamimi"),
            &[],
            &directory.join("server.log"),
        );
        std::fs::remove_dir_all(&directory)?;
        assert!(started.is_err());
        Ok(())
    }

    /// The process group of a process, from the fifth field of its stat line.
    #[cfg(target_os = "linux")]
    fn group_of(pid: &str) -> Result<String, Box<dyn std::error::Error>> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
        // The command name is in parentheses and may hold spaces; the fields
        // that follow it start after the last closing parenthesis.
        let (_, after) = stat
            .rsplit_once(')')
            .ok_or("no command name in the stat line")?;
        let group = after.split_whitespace().nth(2).ok_or("no group field")?;
        Ok(group.to_owned())
    }

    /// The child is the root of a group of its own, so the signals sent to the
    /// group of this process (a terminal's hangup, Ctrl+C) do not reach it.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_detached_child_is_in_a_group_of_its_own() -> Result<(), Box<dyn std::error::Error>> {
        let directory = scratch("group");
        let mut child = spawn_detached(
            Path::new("sleep"),
            &[OsString::from("30")],
            &directory.join("server.log"),
        )?;
        let pid = child.id().to_string();
        let (mine, theirs) = (group_of("self")?, group_of(&pid)?);
        child.kill()?;
        child.wait()?;
        std::fs::remove_dir_all(&directory)?;
        assert_ne!(mine, theirs);
        assert_eq!(theirs, pid);
        Ok(())
    }
}

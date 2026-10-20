//! RP-9: no Python at run time. No process below `hayamimi` or
//! `hayamimi-relay` is a Python interpreter, no DLL the program depends on is
//! Python's, and the repository holds no `*.py` once the Python files are gone.
//!
//! The cases on fixtures (T-RP-9-001 to 009) run everywhere. The acceptance
//! run `rp09_no_python_at_run_time` starts the real programs, reads the
//! process list every second and asks `check_descendants` of every process it
//! saw below each of them. It is `#[ignore]` and needs the models:
//! `cargo test --release -p hayamimi --test rp09 -- --ignored --nocapture`.
//!
//! - Windows (T-RP-9-010): `hayamimi --input mix --serve` and
//!   `hayamimi-relay live` run for ten minutes. The relay is built first with
//!   `cargo build --release -p hayamimi-relay`. `dumpbin /dependents` of the
//!   binary is read by hand; the cases 005 and 006 pin how its output is judged.
//! - Elsewhere: `hayamimi` plays a recorded clip to its end.
//!
//! The negative control (T-RP-9-011) starts a process named like an
//! interpreter below a shell and asks the same check. The process is a renamed
//! link to a shell on Unix and a renamed copy of the command interpreter on
//! Windows.

mod support;

use std::fs::File;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use hayamimi_eval::descendants::{
    Process, TreeError, check_descendants, is_python, python_below, watch,
};
use hayamimi_eval::error::EvalError;
use support::{BINARY, Failure, TestResult, models_dir, path_text, strings};

/// How long the acceptance run waits between two reads of the process list.
const SAMPLE_EVERY: Duration = Duration::from_secs(1);

/// Where the programs of the acceptance run write their output.
const LOGS: &str = env!("CARGO_TARGET_TMPDIR");

fn process(pid: u32, parent: u32, name: &str) -> Process {
    Process {
        pid,
        parent,
        name: name.to_owned(),
    }
}

/// `Ok` when no DLL in the `dumpbin /dependents` output is Python's.
fn check_dependents(listing: &str) -> Result<(), Failure> {
    let is_dll = |line: &str| {
        Path::new(line)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("dll"))
    };
    match listing
        .lines()
        .map(str::trim)
        .find(|line| is_dll(line) && is_python(line))
    {
        Some(name) => Err(format!("the program depends on {name}")),
        None => Ok(()),
    }
}

/// How many lines of a `git ls-files` listing name a `*.py` file.
fn count_py_files(listing: &str) -> usize {
    listing
        .lines()
        .filter(|line| line.trim().ends_with(".py"))
        .count()
}

const DEPENDENTS_WITH_PYTHON: &str = "Image has the following dependencies:\n\n    KERNEL32.dll\n    python311.dll\n    sherpa-onnx-c-api.dll\n\n  Summary\n";
const DEPENDENTS_WITHOUT_PYTHON: &str = "Image has the following dependencies:\n\n    KERNEL32.dll\n    sherpa-onnx-c-api.dll\n\n  Summary\n";

/// T-RP-9-001
#[test]
fn tree_without_python_passes() {
    let tree = [process(100, 1, "hayamimi"), process(101, 100, "ffmpeg.exe")];
    assert_eq!(check_descendants(&tree, 100), Ok(()));
}

/// T-RP-9-002
#[test]
fn python_grandchild_is_rejected() {
    let tree = [
        process(100, 1, "hayamimi"),
        process(101, 100, "ffmpeg.exe"),
        process(102, 101, "python.exe"),
    ];
    assert_eq!(
        check_descendants(&tree, 100),
        Err(TreeError::Python {
            name: String::from("python.exe"),
            pid: 102
        })
    );
}

/// T-RP-9-003
#[test]
fn python_name_is_matched_ignoring_case() {
    let tree = [
        process(100, 1, "hayamimi"),
        process(101, 100, "Pythonw.EXE"),
    ];
    assert!(matches!(
        check_descendants(&tree, 100),
        Err(TreeError::Python { pid: 101, .. })
    ));
}

/// T-RP-9-004
#[test]
fn python_outside_the_tree_is_not_counted() {
    let tree = [process(100, 1, "hayamimi"), process(200, 1, "python.exe")];
    assert_eq!(check_descendants(&tree, 100), Ok(()));
}

/// T-RP-9-005
#[test]
fn python_dll_dependency_is_rejected() {
    let said = check_dependents(DEPENDENTS_WITH_PYTHON);
    assert!(
        matches!(said, Err(ref why) if why.contains("python311.dll")),
        "{said:?}"
    );
}

/// T-RP-9-006
#[test]
fn dependency_list_without_python_passes() {
    assert_eq!(check_dependents(DEPENDENTS_WITHOUT_PYTHON), Ok(()));
}

/// T-RP-9-007: the same check counts the interpreter of a Python run.
#[test]
fn python_run_sample_counts_python_exe() {
    let tree = [
        process(100, 1, "cmd.exe"),
        process(101, 100, "python.exe"),
        process(102, 101, "conhost.exe"),
    ];
    assert_eq!(python_below(&tree, 100).len(), 1);
}

/// T-RP-9-008
#[test]
fn py_file_counter_counts_two_in_a_three_file_list() {
    assert_eq!(count_py_files("a.py\nb.rs\nc.py\n"), 2);
}

/// T-RP-9-009
#[test]
fn py_file_counter_reads_zero_for_an_empty_list() {
    assert_eq!(count_py_files(""), 0);
}

/// What was seen of one process while it ran.
struct Observed {
    root: u32,
    samples: usize,
    /// The root and every process seen below it in any sample.
    seen: Vec<Process>,
    /// How the root ended, when it ended inside the watched span.
    ended: Option<ExitStatus>,
}

/// Ends a child that may already have ended, and collects it.
fn end(child: &mut Child) -> std::io::Result<ExitStatus> {
    // A child that already ended has nothing to kill.
    let _ = child.kill();
    child.wait()
}

/// Reads the process list every `every` while `child` runs, for at most
/// `span`; then ends the child and collects it.
fn observe(mut child: Child, span: Duration, every: Duration) -> Result<Observed, Failure> {
    let root = child.id();
    let until = Instant::now() + span;
    let mut ended = None;
    let watched = watch(root, every, || {
        ended = child.try_wait().map_err(|source| EvalError::Io {
            path: String::from("child"),
            source,
        })?;
        Ok(ended.is_none() && Instant::now() < until)
    });
    // The child is collected whether or not the watch worked.
    let collected = end(&mut child);
    let watched = watched.map_err(|error| error.to_string())?;
    collected.map_err(|error| error.to_string())?;
    Ok(Observed {
        root,
        samples: watched.samples,
        seen: watched.seen,
        ended,
    })
}

/// One program of the acceptance run.
struct Program {
    label: &'static str,
    binary: String,
    args: Vec<String>,
}

fn program(label: &'static str, binary: &str, args: &[&str]) -> Program {
    Program {
        label,
        binary: binary.to_owned(),
        args: strings(args),
    }
}

/// What the acceptance run starts, and how it must end.
struct Plan {
    programs: Vec<Program>,
    /// The longest the programs are watched.
    span: Duration,
    /// Whether a program is expected to end inside the span with code 0
    /// (a clip played to its end) or to be still running at its end (a live run).
    ends_by_itself: bool,
}

/// The `hayamimi-relay` binary next to the `hayamimi` binary.
#[cfg(windows)]
fn relay_binary() -> Result<String, Failure> {
    let directory = Path::new(BINARY)
        .parent()
        .ok_or("the hayamimi binary has no directory")?;
    let path = directory.join("hayamimi-relay.exe");
    if !path.is_file() {
        return Err(format!(
            "{} is not built: run `cargo build --release -p hayamimi-relay`",
            path.display()
        ));
    }
    path_text(&path)
}

/// T-RP-9-010: the live run of the two programs, ten minutes.
#[cfg(windows)]
fn plan() -> Result<Plan, Failure> {
    let models = path_text(&models_dir()?)?;
    Ok(Plan {
        programs: vec![
            program(
                "hayamimi",
                BINARY,
                &["--input", "mix", "--models", &models, "--serve"],
            ),
            program("hayamimi-relay", &relay_binary()?, &["live"]),
        ],
        span: Duration::from_secs(600),
        ends_by_itself: false,
    })
}

/// A recorded clip played to its end.
#[cfg(not(windows))]
fn plan() -> Result<Plan, Failure> {
    let models = path_text(&models_dir()?)?;
    let clip = path_text(&support::clip())?;
    let args = [
        "--wav",
        clip.as_str(),
        "--no-realtime",
        "--models",
        models.as_str(),
        "--mode",
        "single",
        "--lang",
        "ja",
    ];
    Ok(Plan {
        programs: vec![program("hayamimi", BINARY, &args)],
        span: Duration::from_secs(600),
        ends_by_itself: true,
    })
}

/// Starts a program with its output in a file of [`LOGS`].
fn spawn_logged(program: &Program) -> Result<Child, Failure> {
    let log = Path::new(LOGS).join(format!("rp09-{}.log", program.label));
    let named = |error: std::io::Error| format!("{}: {error}", log.display());
    let out = File::create(&log).map_err(named)?;
    let err = out.try_clone().map_err(named)?;
    Command::new(&program.binary)
        .args(&program.args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err))
        .spawn()
        .map_err(|error| format!("cannot start {}: {error}", program.binary))
}

/// Starts every program; when one does not start, ends the ones before it.
fn start_all(programs: &[Program]) -> Result<Vec<(&'static str, Child)>, Failure> {
    let mut started: Vec<(&'static str, Child)> = Vec::new();
    for program in programs {
        match spawn_logged(program) {
            Ok(child) => started.push((program.label, child)),
            Err(why) => {
                for (_, child) in &mut started {
                    // The start failed already; a child that will not end is the lesser report.
                    let _ = end(child);
                }
                return Err(why);
            }
        }
    }
    Ok(started)
}

/// Starts the programs, watches them side by side, and returns what each saw.
fn run(plan: &Plan) -> Result<Vec<(&'static str, Observed)>, Failure> {
    let watchers: Vec<_> = start_all(&plan.programs)?
        .into_iter()
        .map(|(label, child)| {
            let span = plan.span;
            std::thread::spawn(move || observe(child, span, SAMPLE_EVERY).map(|seen| (label, seen)))
        })
        .collect();
    let mut observed = Vec::new();
    let mut failure = None;
    for watcher in watchers {
        match watcher.join() {
            Ok(Ok(one)) => observed.push(one),
            Ok(Err(why)) => failure = Some(why),
            Err(_) => failure = Some(String::from("a watcher panicked")),
        }
    }
    match failure {
        Some(why) => Err(why),
        None => Ok(observed),
    }
}

/// One line that says what a watch saw, for the log of the run.
fn summary(label: &str, observed: &Observed) -> String {
    let mut names: Vec<&str> = observed.seen.iter().map(|p| p.name.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    format!(
        "rp09 {label}: root {} samples {} processes {} names {} {names:?}",
        observed.root,
        observed.samples,
        observed.seen.len(),
        names.len()
    )
}

/// The program ended the way the plan says: with code 0 inside the span, or
/// still running at its end.
fn check_ending(label: &str, observed: &Observed, ends_by_itself: bool) -> Result<(), Failure> {
    match (observed.ended, ends_by_itself) {
        (Some(status), true) if status.success() => Ok(()),
        (None, false) => Ok(()),
        (ended, _) => Err(format!(
            "{label} ended as {ended:?}; its output is in {LOGS}/rp09-{label}.log"
        )),
    }
}

/// Every program ended as planned and none has a Python below it.
fn judge(plan: &Plan, observed: &[(&str, Observed)]) -> Result<(), Failure> {
    for (label, one) in observed {
        eprintln!("{}", summary(label, one));
        check_ending(label, one, plan.ends_by_itself)?;
        check_descendants(&one.seen, one.root).map_err(|error| format!("{label}: {error}"))?;
    }
    Ok(())
}

/// The acceptance run (T-RP-9-010): no process below the programs is Python.
#[test]
#[ignore = "starts the real programs, ten minutes on Windows, and needs the models"]
fn rp09_no_python_at_run_time() -> TestResult {
    let plan = plan()?;
    let observed = run(&plan)?;
    judge(&plan, &observed)?;
    Ok(())
}

/// The directory of the stand-in interpreter.
fn fake_dir() -> Result<std::path::PathBuf, Failure> {
    let dir = Path::new(LOGS).join("rp09-fake-python");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(dir)
}

/// A program named like an interpreter: a link to the system shell called
/// `python3`. A shell does not look at the name it is started under, unlike a
/// multi-call `sleep`, which ends at once under a name it does not know.
#[cfg(unix)]
fn fake_interpreter() -> Result<String, Failure> {
    let shell = std::fs::canonicalize("/bin/sh").map_err(|e| format!("/bin/sh: {e}"))?;
    let link = fake_dir()?.join("python3");
    // A link left by an earlier run is replaced; one that is not there is not an error.
    match std::fs::remove_file(&link) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("{}: {e}", link.display())),
    }
    std::os::unix::fs::symlink(&shell, &link).map_err(|e| format!("{}: {e}", link.display()))?;
    path_text(&link)
}

/// A program named like an interpreter: a copy of the command interpreter
/// called `python.exe`, which also runs under any name.
#[cfg(windows)]
fn fake_interpreter() -> Result<String, Failure> {
    let copy = fake_dir()?.join("python.exe");
    std::fs::copy(WINDOWS_SHELL, &copy).map_err(|e| format!("{}: {e}", copy.display()))?;
    path_text(&copy)
}

#[cfg(windows)]
const WINDOWS_SHELL: &str = r"C:\Windows\System32\cmd.exe";

/// A shell that starts the stand-in and waits for it, a few seconds.
#[cfg(unix)]
fn shell_starting(fake: &str) -> Command {
    let mut shell = Command::new("sh");
    // The `; true` after each command keeps a shell from replacing itself with it.
    shell.args(["-c", "\"$0\" -c 'sleep 2; true'; true", fake]);
    shell
}

#[cfg(windows)]
fn shell_starting(fake: &str) -> Command {
    let mut shell = Command::new(WINDOWS_SHELL);
    shell.args(["/c", fake, "/c", "ping", "-n", "8", "127.0.0.1"]);
    shell
}

/// T-RP-9-011, with the stand-in program inside the test: a shell starts a
/// process named like an interpreter and the check names it.
#[test]
fn build_spawning_python_is_rejected() -> TestResult {
    let fake = fake_interpreter()?;
    let shell = shell_starting(&fake)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| error.to_string())?;
    let seen = observe(shell, Duration::from_secs(30), Duration::from_millis(50))?;
    let said = check_descendants(&seen.seen, seen.root);
    assert!(matches!(said, Err(TreeError::Python { .. })), "{said:?}");
    Ok(())
}

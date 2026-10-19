//! RP-9: no Python at run time. No process below `hayamimi` is a Python
//! interpreter, no DLL it depends on is Python's, and the repository holds no
//! `*.py` once the Python files are gone.
//!
//! The cases on fixtures run everywhere. `rp09_no_python_at_run_time` runs the
//! binary on a recorded clip and watches its process tree to the end; it needs
//! the models and is `#[ignore]`: run it with
//! `cargo test --release -p hayamimi --test rp09 -- --ignored`. The negative
//! control starts a process named like an interpreter below a shell and asks
//! the same check; it runs on Unix, where such a process is a renamed link.

mod support;

use std::time::Duration;

use hayamimi_eval::descendants::{Process, TreeError, check_descendants, python_below, watch};
use hayamimi_eval::error::EvalError;
use support::{BINARY, Failure, Output, TestResult, clip, collect, path_text, spawn};

fn process(pid: u32, parent: u32, name: &str) -> Process {
    Process {
        pid,
        parent,
        name: name.to_owned(),
    }
}

/// The names of the DLLs in `dumpbin /dependents` output that are Python's.
fn python_dependents(listing: &str) -> Vec<&str> {
    listing
        .lines()
        .map(str::trim)
        .filter(|line| line.to_ascii_lowercase().ends_with(".dll"))
        .filter(|line| line.to_ascii_lowercase().contains("python"))
        .collect()
}

/// `Ok` when no DLL in the `dumpbin /dependents` output is Python's.
fn check_dependents(listing: &str) -> Result<(), Failure> {
    match python_dependents(listing).first() {
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

/// What `watch` saw of a running child, and how it ended.
struct Watched {
    output: Output,
    samples: usize,
    tree: Result<(), TreeError>,
}

fn joined(handle: Option<std::thread::JoinHandle<String>>) -> Result<String, Failure> {
    match handle {
        Some(handle) => handle.join().map_err(|_| String::from("a reader panicked")),
        None => Err(String::from("the child had no pipe")),
    }
}

/// Watches the process tree below `child` until it exits, then asks the check.
fn watch_to_the_end(mut child: std::process::Child) -> Result<Watched, Failure> {
    let out = child.stdout.take().map(collect);
    let err = child.stderr.take().map(collect);
    let root = child.id();
    let seen = watch(root, Duration::from_millis(50), || {
        child
            .try_wait()
            .map(|status| status.is_none())
            .map_err(|source| EvalError::Io {
                path: String::from("child"),
                source,
            })
    })
    .map_err(|e| e.to_string())?;
    let status = child.wait().map_err(|e| e.to_string())?;
    Ok(Watched {
        output: Output {
            code: status.code(),
            stdout: joined(out)?,
            stderr: joined(err)?,
        },
        samples: seen.samples,
        tree: check_descendants(&seen.seen, root),
    })
}

/// The acceptance run: `hayamimi` plays a recorded clip to its end while its
/// process tree is read; no process below it is Python.
#[test]
#[ignore = "runs the binary on a clip and needs the models"]
fn rp09_no_python_at_run_time() -> TestResult {
    let clip = path_text(&clip())?;
    let args = [
        "--wav",
        clip.as_str(),
        "--no-realtime",
        "--mode",
        "single",
        "--lang",
        "ja",
    ];
    let watched = watch_to_the_end(spawn(&args, None, &[])?)?;
    assert_eq!(
        watched.output.code,
        Some(0),
        "{BINARY}: {}",
        watched.output.stderr
    );
    assert!(watched.samples >= 1);
    assert_eq!(watched.tree, Ok(()));
    Ok(())
}

/// A program named like an interpreter: a link to `sleep` called `python3`.
#[cfg(unix)]
fn fake_interpreter() -> Result<String, Failure> {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("rp09-fake-python");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let link = dir.join("python3");
    // A link left by an earlier run is replaced; one that is not there is not an error.
    match std::fs::remove_file(&link) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("{}: {e}", link.display())),
    }
    std::os::unix::fs::symlink("/bin/sleep", &link)
        .map_err(|e| format!("{}: {e}", link.display()))?;
    path_text(&link)
}

/// T-RP-9-011, with the stand-in program inside the test: a shell starts a
/// process named `python3` and the check names it.
#[cfg(unix)]
#[test]
fn build_spawning_python_is_rejected() -> TestResult {
    let fake = fake_interpreter()?;
    // The `; true` keeps the shell from replacing itself with the last command.
    let child = std::process::Command::new("sh")
        .args(["-c", "\"$0\" 2; true", fake.as_str()])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let watched = watch_to_the_end(child)?;
    assert!(
        matches!(watched.tree, Err(TreeError::Python { .. })),
        "{:?}",
        watched.tree
    );
    Ok(())
}

//! RP-9: whether a Python interpreter runs below a process of the program.
//!
//! The tests read the process tree while a command runs, keep every process
//! that was ever seen at or below the root, and ask `check_descendants` whether
//! one of them is a Python interpreter. Only processes below the root count: a
//! Python of another user of the machine is not the program's.

use std::time::Duration;

use crate::error::EvalError;

/// The word that marks an interpreter in a process name, compared without case.
const INTERPRETER: &str = "python";

/// One process of the machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Process {
    pub pid: u32,
    pub parent: u32,
    pub name: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum TreeError {
    /// The root process is not in the tree, so nothing was observed below it.
    NoRoot(u32),
    /// A process below the root is a Python interpreter.
    Python { name: String, pid: u32 },
}

impl std::fmt::Display for TreeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TreeError::NoRoot(pid) => write!(f, "process {pid} is not in the tree"),
            TreeError::Python { name, pid } => {
                write!(f, "process {pid} ({name}) below the root is Python")
            }
        }
    }
}

impl std::error::Error for TreeError {}

/// The root and every process below it, found by following parent links.
pub fn subtree(tree: &[Process], root: u32) -> Vec<&Process> {
    let mut found: Vec<&Process> = tree.iter().filter(|p| p.pid == root).collect();
    let mut at = 0;
    while at < found.len() {
        let parent = found[at].pid;
        for child in tree.iter().filter(|p| p.parent == parent) {
            if !found.iter().any(|seen| std::ptr::eq(*seen, child)) {
                found.push(child);
            }
        }
        at += 1;
    }
    found
}

/// Whether a process name is a Python interpreter (`python.exe`, `Pythonw.EXE`, `python3`).
pub fn is_python(name: &str) -> bool {
    name.to_ascii_lowercase().contains(INTERPRETER)
}

/// The processes below `root` (the root itself excluded) that are Python.
pub fn python_below(tree: &[Process], root: u32) -> Vec<&Process> {
    subtree(tree, root)
        .into_iter()
        .filter(|p| p.pid != root && is_python(&p.name))
        .collect()
}

/// `Ok` when `root` is in the tree and no process below it is Python.
pub fn check_descendants(tree: &[Process], root: u32) -> Result<(), TreeError> {
    if !tree.iter().any(|p| p.pid == root) {
        return Err(TreeError::NoRoot(root));
    }
    match python_below(tree, root).first() {
        Some(found) => Err(TreeError::Python {
            name: found.name.clone(),
            pid: found.pid,
        }),
        None => Ok(()),
    }
}

/// One process from a `/proc/<pid>/stat` line: `pid (name) state ppid ...`.
/// The name may hold spaces and parentheses, so it runs to the last `)`.
pub fn parse_proc_stat(text: &str) -> Option<Process> {
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    if close < open {
        return None;
    }
    let pid = text[..open].trim().parse().ok()?;
    let mut fields = text[close + 1..].split_whitespace();
    fields.next()?;
    let parent = fields.next()?.parse().ok()?;
    Some(Process {
        pid,
        parent,
        name: text[open + 1..close].to_owned(),
    })
}

/// The cells of one CSV line whose cells are all quoted (`"a","b"`).
fn quoted_cells(line: &str) -> Option<Vec<&str>> {
    let inner = line.trim().strip_prefix('"')?.strip_suffix('"')?;
    Some(inner.split("\",\"").collect())
}

/// The processes of `ConvertTo-Csv` output with the columns
/// `ProcessId`, `ParentProcessId`, `Name`; the header line is skipped.
pub fn parse_process_csv(text: &str) -> Result<Vec<Process>, EvalError> {
    let mut processes = Vec::new();
    for line in text.lines().skip(1).filter(|l| !l.trim().is_empty()) {
        let bad = || EvalError::Invalid(format!("not a process row: {line}"));
        let cells = quoted_cells(line).ok_or_else(bad)?;
        let &[pid, parent, name] = cells.as_slice() else {
            return Err(bad());
        };
        processes.push(Process {
            pid: pid.parse().map_err(|_| bad())?,
            parent: parent.parse().map_err(|_| bad())?,
            name: name.to_owned(),
        });
    }
    Ok(processes)
}

#[cfg(target_os = "linux")]
fn current_processes() -> Result<Vec<Process>, EvalError> {
    let proc = std::path::Path::new("/proc");
    let mut processes = Vec::new();
    for entry in std::fs::read_dir(proc).map_err(|e| EvalError::io(proc, e))? {
        let entry = entry.map_err(|e| EvalError::io(proc, e))?;
        // A process that ended since the listing has no stat file any more.
        if let Ok(text) = std::fs::read_to_string(entry.path().join("stat")) {
            processes.extend(parse_proc_stat(&text));
        }
    }
    Ok(processes)
}

#[cfg(windows)]
fn current_processes() -> Result<Vec<Process>, EvalError> {
    const LISTING: &str = "Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,Name | ConvertTo-Csv -NoTypeInformation";
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", LISTING])
        .output()
        .map_err(|e| EvalError::io(std::path::Path::new("powershell"), e))?;
    if !output.status.success() {
        return Err(EvalError::Invalid(format!(
            "the process listing exited with {}",
            output.status
        )));
    }
    parse_process_csv(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(not(any(target_os = "linux", windows)))]
fn current_processes() -> Result<Vec<Process>, EvalError> {
    Err(EvalError::Invalid(String::from(
        "the process tree can be read on Linux and Windows only",
    )))
}

/// What `watch` saw.
#[derive(Debug)]
pub struct Watched {
    /// How many times the process list was read.
    pub samples: usize,
    /// The root and every process seen below it in any sample.
    pub seen: Vec<Process>,
}

/// Reads the process list every `interval` while `running` says the command
/// runs, and keeps each process seen at or below `root`.
pub fn watch(
    root: u32,
    interval: Duration,
    mut running: impl FnMut() -> Result<bool, EvalError>,
) -> Result<Watched, EvalError> {
    let mut watched = Watched {
        samples: 0,
        seen: Vec::new(),
    };
    loop {
        let tree = current_processes()?;
        watched.samples += 1;
        for found in subtree(&tree, root) {
            if !watched.seen.contains(found) {
                watched.seen.push(found.clone());
            }
        }
        if !running()? {
            return Ok(watched);
        }
        std::thread::sleep(interval);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(pid: u32, parent: u32, name: &str) -> Process {
        Process {
            pid,
            parent,
            name: name.to_owned(),
        }
    }

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

    /// An empty observation is not "no Python": the root was never seen.
    #[test]
    fn a_tree_without_the_root_is_an_error_and_not_a_pass() {
        assert_eq!(check_descendants(&[], 100), Err(TreeError::NoRoot(100)));
        let tree = [process(200, 1, "python.exe")];
        assert_eq!(check_descendants(&tree, 100), Err(TreeError::NoRoot(100)));
    }

    /// The root itself is not below itself, whatever it is called.
    #[test]
    fn the_root_is_not_counted_below_itself() {
        let tree = [process(100, 1, "python.exe")];
        assert_eq!(check_descendants(&tree, 100), Ok(()));
    }

    /// T-RP-9-007: the same function counts the interpreter of a Python run.
    #[test]
    fn python_run_sample_counts_python_exe() {
        let tree = [
            process(100, 1, "cmd.exe"),
            process(101, 100, "python.exe"),
            process(102, 101, "conhost.exe"),
        ];
        assert_eq!(python_below(&tree, 100).len(), 1);
    }

    #[test]
    fn a_child_listed_before_its_parent_is_still_found() {
        let tree = [
            process(102, 101, "python3"),
            process(101, 100, "sh"),
            process(100, 1, "hayamimi"),
        ];
        assert_eq!(subtree(&tree, 100).len(), 3);
        assert!(check_descendants(&tree, 100).is_err());
    }

    #[test]
    fn a_stat_line_gives_pid_parent_and_name() {
        let line = "4242 (hayamimi) S 4000 4242 4000 34816 4242 4194304 100 0 0";
        assert_eq!(parse_proc_stat(line), Some(process(4242, 4000, "hayamimi")));
    }

    #[test]
    fn a_name_with_spaces_and_parentheses_runs_to_the_last_one() {
        let line = "7 (a (b) c) R 3 7 3 0 -1";
        assert_eq!(parse_proc_stat(line), Some(process(7, 3, "a (b) c")));
    }

    #[test]
    fn a_stat_line_that_is_cut_short_is_not_a_process() {
        for line in ["", "7", "7 (x)", "7 (x) S", "x (y) S 3", "7 (x) S z"] {
            assert_eq!(parse_proc_stat(line), None, "{line}");
        }
    }

    #[test]
    fn a_csv_listing_gives_one_process_per_row() -> Result<(), EvalError> {
        let text = "\"ProcessId\",\"ParentProcessId\",\"Name\"\r\n\"4\",\"0\",\"System\"\r\n\"1234\",\"4\",\"python.exe\"\r\n";
        assert_eq!(
            parse_process_csv(text)?,
            [process(4, 0, "System"), process(1234, 4, "python.exe")]
        );
        Ok(())
    }

    #[test]
    fn a_csv_row_that_is_not_three_numbers_and_a_name_is_refused() {
        for body in [
            "\"1\",\"2\"",
            "\"x\",\"2\",\"n\"",
            "1,2,n",
            "\"1\",\"2\",\"n\",\"m\"",
        ] {
            let text = format!("\"ProcessId\",\"ParentProcessId\",\"Name\"\n{body}\n");
            assert!(parse_process_csv(&text).is_err(), "{body}");
        }
    }

    #[test]
    fn the_listing_of_this_machine_holds_the_running_test() -> Result<(), EvalError> {
        let mut seen = false;
        let watched = watch(std::process::id(), Duration::from_millis(1), || {
            let more = !seen;
            seen = true;
            Ok(more)
        })?;
        assert_eq!(watched.samples, 2);
        assert!(watched.seen.iter().any(|p| p.pid == std::process::id()));
        Ok(())
    }
}

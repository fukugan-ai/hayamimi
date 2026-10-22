//! The state of the machine a `stream` run stands on, written beside the
//! samples as `<run>.machine.jsonl`.
//!
//! A row has `t_us` (microseconds since the run began), `load1`, `load5` and
//! `load15` (the three fields of `/proc/loadavg`, kept as the JSON strings the
//! kernel wrote, so a value on a limit is not moved by a rounding) and `cargo`
//! and `rustc` (the number of processes whose `/proc/<pid>/comm` is exactly
//! that name). `io_some_total` and `io_full_total` are the `total=` fields of
//! `/proc/pressure/io` (microseconds, `null` when the kernel does not show
//! them); they are evidence of a stalled disk and no judgement reads them.
//!
//! [`begin`] writes the first row before any audio is sent and refuses the run
//! when `load1` or `load15` is over its limit or a build stands on the
//! machine. A thread then writes a row every [`ROW_PERIOD`] until [`Gate::end`]
//! writes the last one. A row that shows a build is the last row: the gate sets
//! its halt flag, the sender stops, and the run ends as aborted.
//!
//! The run's own processes (`hayamimi`, its relay and the translation
//! `llama-server`) raise the load of the machine they run on. `--own-pid` names
//! them, and each row then adds `own_cpu_ticks` (the sum of `utime` and `stime`
//! of their `/proc/<pid>/stat`, in clock ticks) and `cpu_some_total` (the
//! `total=` of the `some` line of `/proc/pressure/cpu`, recorded and never
//! judged); the first row adds `own` and `clk_tck`. A judgement subtracts the
//! run's own share from the last `load5`. A run with no `--own-pid` writes none
//! of these fields.
//!
//! The pids are checked once, before the first row is kept, in the order they
//! were given and by the first fault found. The check refuses a pid that has no
//! readable `comm` and `stat`, a `comm` that is not one of the three names, a
//! `llama-server` that is not started with `--port 18084`, and a set that does
//! not hold each of the three names exactly once. A refusal is the reason
//! `own_pid`, asked before `load1`, `load15` and a build, because a wrong
//! argument does not mend by waiting.
//!
//! Where `/proc` stands is an argument, so a test points it at a directory of
//! its own.

use std::ffi::OsStr;
use std::fs::File;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;

use crate::error::EvalError;
use crate::ingest::micros_since;
use crate::log;

/// The extension of the record: `run.jsonl` gives `run.machine.jsonl`.
pub const FILE_SUFFIX: &str = "machine.jsonl";
/// How often a row is written while the run lasts.
pub const ROW_PERIOD: Duration = Duration::from_secs(5);
/// The longest gap between two rows a judgement accepts, in microseconds.
pub const MAX_GAP_US: i64 = 15_000_000;
/// The first row's `load1` must be below this, in hundredths (2.00).
pub const LOAD1_LIMIT: u32 = 200;
/// The first row's `load15` must be below this, in hundredths (2.50).
pub const LOAD15_LIMIT: u32 = 250;
/// The last row's `load5` must be below this, in hundredths (2.50).
pub const LOAD5_LIMIT: u32 = 250;
/// The longest sleep between two looks at a flag.
const SLICE: Duration = Duration::from_millis(50);
/// Clock ticks per second in the `utime` and `stime` of `/proc/<pid>/stat`
/// (`sysconf(_SC_CLK_TCK)`, which is the kernel's `USER_HZ` and 100 on every
/// Linux target of this tool).
pub const CLK_TCK: u32 = 100;
/// The names a `--own-pid` process may have, each exactly once.
const OWN_COMMS: [&str; 3] = ["hayamimi", "hayamimi-relay", "llama-server"];
/// Of those, the one whose command line is checked as well.
const LLAMA_COMM: &str = "llama-server";
/// The port the window's translation server listens on.
const LLAMA_PORT: &[u8] = b"18084";
/// How many fields of `stat` follow the `)` of the name before `utime` (the
/// state is the first of them, `utime` is field 14 and the state is field 3).
const FIELDS_BEFORE_UTIME: usize = 11;

/// A load of `/proc/loadavg` in hundredths: `1.99` is 199. Only digits, a dot
/// and two decimals are a load.
pub fn hundredths(text: &str) -> Option<u32> {
    let (whole, fraction) = text.split_once('.')?;
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    if !digits(whole) || fraction.len() != 2 || !digits(fraction) {
        return None;
    }
    whole
        .parse::<u32>()
        .ok()?
        .checked_mul(100)?
        .checked_add(fraction.parse::<u32>().ok()?)
}

/// One row of `<run>.machine.jsonl`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Row {
    pub t_us: i64,
    pub load1: String,
    pub load5: String,
    pub load15: String,
    pub cargo: u32,
    pub rustc: u32,
    /// `total=` of the `some` line of `/proc/pressure/io`, in microseconds;
    /// `null` when the kernel does not show it. Recorded, never judged.
    pub io_some_total: Option<u64>,
    /// Likewise for the `full` line.
    pub io_full_total: Option<u64>,
    /// The fields of a run that names its own processes; absent otherwise.
    #[serde(flatten)]
    pub columns: Option<OwnColumns>,
}

/// A process whose CPU time a run counts as its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OwnProcess {
    pub pid: u32,
    pub comm: String,
}

/// The fields a row adds when `--own-pid` names the run's processes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OwnColumns {
    /// The processes, in the order they were given; the first row only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub own: Option<Vec<OwnProcess>>,
    /// Clock ticks per second of `own_cpu_ticks`; the first row only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clk_tck: Option<u32>,
    /// `utime` plus `stime`, in clock ticks, summed over the processes; `null`
    /// when one of their `stat` files cannot be read (the process has ended).
    pub own_cpu_ticks: Option<u64>,
    /// `total=` of the `some` line of `/proc/pressure/cpu`, in microseconds;
    /// `null` when the kernel does not show it. Recorded, never judged.
    pub cpu_some_total: Option<u64>,
}

/// Why a pid given to `--own-pid` is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnFault {
    /// Its `comm` or `stat` cannot be read.
    Unreadable,
    /// Its `comm` is not one of the three names.
    Comm,
    /// A `llama-server` that is not started with `--port 18084`.
    Port,
    /// The pids together do not hold each of the three names once.
    Set,
}

impl OwnFault {
    pub fn name(self) -> &'static str {
        match self {
            OwnFault::Unreadable => "unreadable",
            OwnFault::Comm => "comm",
            OwnFault::Port => "port",
            OwnFault::Set => "set",
        }
    }
}

/// The first fault found in the pids given, and the pid that has it (none for
/// a fault of the set).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnRefusal {
    pub fault: OwnFault,
    pub pid: Option<u32>,
}

/// Why a run is refused, in the order the reasons are asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Load1,
    Load15,
    Build,
}

impl Reason {
    pub fn name(self) -> &'static str {
        match self {
            Reason::Load1 => "load1",
            Reason::Load15 => "load15",
            Reason::Build => "build",
        }
    }
}

/// Whether `text`, a load read by [`Row::read`], reaches `limit`.
fn reaches(text: &str, limit: u32) -> bool {
    hundredths(text).is_some_and(|value| value >= limit)
}

/// The first three fields of `loadavg` under `proc_root`; each must be a load.
fn read_loads(proc_root: &Path) -> Result<[String; 3], EvalError> {
    let path = proc_root.join("loadavg");
    let text = std::fs::read_to_string(&path).map_err(|error| EvalError::io(&path, error))?;
    let found: Vec<&str> = text
        .split_whitespace()
        .take(3)
        .filter(|word| hundredths(word).is_some())
        .collect();
    let [one, five, fifteen] = found.as_slice() else {
        return Err(EvalError::Invalid(format!(
            "{}: the first three fields are not loads",
            path.display()
        )));
    };
    Ok([one, five, fifteen].map(|word| (*word).to_owned()))
}

/// Whether a directory name of `/proc` is a process id.
fn is_pid(name: &OsStr) -> bool {
    name.to_str()
        .is_some_and(|text| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit()))
}

/// The `comm` of a process without its newline; `None` when the process has ended.
fn comm_of(path: &Path) -> Result<Option<Vec<u8>>, EvalError> {
    match std::fs::read(path) {
        Ok(mut bytes) => {
            if bytes.last() == Some(&b'\n') {
                bytes.pop();
            }
            Ok(Some(bytes))
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(EvalError::io(path, error)),
    }
}

/// The number of processes named `cargo` and `rustc` under `proc_root`.
fn count_builds(proc_root: &Path) -> Result<(u32, u32), EvalError> {
    let entries = std::fs::read_dir(proc_root).map_err(|error| EvalError::io(proc_root, error))?;
    let (mut cargo, mut rustc) = (0, 0);
    for entry in entries {
        let entry = entry.map_err(|error| EvalError::io(proc_root, error))?;
        if !is_pid(&entry.file_name()) {
            continue;
        }
        match comm_of(&entry.path().join("comm"))?.as_deref() {
            Some(b"cargo") => cargo += 1,
            Some(b"rustc") => rustc += 1,
            _ => {}
        }
    }
    Ok((cargo, rustc))
}

/// The `total=` field of the line of `text` that starts with `kind`.
fn pressure_total(text: &str, kind: &str) -> Option<u64> {
    let line = text
        .lines()
        .find(|line| line.split_whitespace().next() == Some(kind))?;
    line.split_whitespace()
        .find_map(|field| field.strip_prefix("total="))?
        .parse()
        .ok()
}

/// The `some` and `full` totals of `pressure/io` under `proc_root`. A kernel
/// without pressure stall information has no such file, and a file that is not
/// in the kernel's format is as good as none: both are `None`.
fn read_pressure(proc_root: &Path) -> Option<(u64, u64)> {
    let text = std::fs::read_to_string(proc_root.join("pressure").join("io")).ok()?;
    Some((
        pressure_total(&text, "some")?,
        pressure_total(&text, "full")?,
    ))
}

/// The `total=` of the `some` line of `pressure/cpu` under `proc_root`; `None`
/// when the kernel does not show it.
fn read_cpu_pressure(proc_root: &Path) -> Option<u64> {
    let text = std::fs::read_to_string(proc_root.join("pressure").join("cpu")).ok()?;
    pressure_total(&text, "some")
}

/// `utime` plus `stime`, in clock ticks, of the text of one `/proc/<pid>/stat`.
///
/// The fields are counted from the last `)`: the name in front of it is in
/// parentheses and may hold spaces and `)` itself.
pub fn ticks_of_stat(text: &str) -> Result<u64, EvalError> {
    parse_ticks(text).ok_or_else(|| {
        EvalError::Invalid(String::from(
            "not a /proc/<pid>/stat: no utime and stime after the name",
        ))
    })
}

fn parse_ticks(text: &str) -> Option<u64> {
    let (_, after_name) = text.rsplit_once(')')?;
    let mut fields = after_name.split_whitespace().skip(FIELDS_BEFORE_UTIME);
    let utime: u64 = fields.next()?.parse().ok()?;
    let stime: u64 = fields.next()?.parse().ok()?;
    utime.checked_add(stime)
}

/// The ticks of one process, read from its `stat`.
fn pid_ticks(proc_root: &Path, pid: u32) -> Result<u64, EvalError> {
    let path = proc_root.join(pid.to_string()).join("stat");
    let bytes = std::fs::read(&path).map_err(|error| EvalError::io(&path, error))?;
    ticks_of_stat(&String::from_utf8_lossy(&bytes))
}

/// The CPU time the processes `pids` have used, in clock ticks: the sum of the
/// `utime` and `stime` of each `stat` under `proc_root`. The kernel keeps a
/// process's time over all its threads, ended ones included, in that file;
/// the `task/*/stat` files hold the live threads only and are not read.
pub fn own_cpu_ticks(
    proc_root: &Path,
    pids: impl IntoIterator<Item = u32>,
) -> Result<u64, EvalError> {
    let mut total: u64 = 0;
    for pid in pids {
        total = total
            .checked_add(pid_ticks(proc_root, pid)?)
            .ok_or_else(|| EvalError::Invalid(String::from("the CPU ticks overflow")))?;
    }
    Ok(total)
}

/// Whether `cmdline` (words each ended by a NUL) holds the word `--port`
/// exactly once and the word after it is the translation server's port.
fn cmdline_names_own_port(cmdline: &[u8]) -> bool {
    let body = match cmdline.strip_suffix(&[0]) {
        Some(body) => body,
        None => cmdline,
    };
    let words: Vec<&[u8]> = body.split(|byte| *byte == 0).collect();
    let mut flags = words
        .iter()
        .enumerate()
        .filter(|(_, word)| **word == b"--port");
    let (Some((at, _)), None) = (flags.next(), flags.next()) else {
        return false;
    };
    words.get(at + 1).is_some_and(|word| *word == LLAMA_PORT)
}

/// Whether the process in `dir` was started with the translation server's
/// port. A `cmdline` that cannot be read, or is empty (a zombie's), is not.
fn serves_own_port(dir: &Path) -> bool {
    std::fs::read(dir.join("cmdline")).is_ok_and(|bytes| cmdline_names_own_port(&bytes))
}

/// One pid of `--own-pid`: its name, and for the server its port. The faults
/// are asked in the order `Unreadable`, `Comm`, `Port`.
fn check_pid(proc_root: &Path, pid: u32) -> Result<OwnProcess, OwnFault> {
    let dir = proc_root.join(pid.to_string());
    let comm = comm_of(&dir.join("comm")).ok().flatten();
    let (Some(comm), Ok(_)) = (comm, pid_ticks(proc_root, pid)) else {
        return Err(OwnFault::Unreadable);
    };
    let Some(name) = OWN_COMMS
        .iter()
        .find(|name| name.as_bytes() == comm.as_slice())
    else {
        return Err(OwnFault::Comm);
    };
    if *name == LLAMA_COMM && !serves_own_port(&dir) {
        return Err(OwnFault::Port);
    }
    Ok(OwnProcess {
        pid,
        comm: (*name).to_owned(),
    })
}

/// The processes `pids` name, or the first fault, found pid by pid in the order
/// given; the fault of the set is asked after every pid has passed. No pid at
/// all is a run that does not name its processes: an empty set.
pub fn check_own(proc_root: &Path, pids: &[u32]) -> Result<Vec<OwnProcess>, OwnRefusal> {
    if pids.is_empty() {
        return Ok(Vec::new());
    }
    let mut set = Vec::with_capacity(pids.len());
    for &pid in pids {
        let process = check_pid(proc_root, pid).map_err(|fault| OwnRefusal {
            fault,
            pid: Some(pid),
        })?;
        set.push(process);
    }
    let once_each = OWN_COMMS
        .iter()
        .all(|name| set.iter().filter(|process| process.comm == *name).count() == 1);
    if !once_each {
        return Err(OwnRefusal {
            fault: OwnFault::Set,
            pid: None,
        });
    }
    Ok(set)
}

/// The fields of a row of a run that names its processes; `None` for a run
/// that names none. A `stat` that cannot be read is a `null` count, not an error.
fn own_columns(proc_root: &Path, own: &[OwnProcess]) -> Option<OwnColumns> {
    if own.is_empty() {
        return None;
    }
    Some(OwnColumns {
        own: None,
        clk_tck: None,
        own_cpu_ticks: own_cpu_ticks(proc_root, own.iter().map(|process| process.pid)).ok(),
        cpu_some_total: read_cpu_pressure(proc_root),
    })
}

impl Row {
    /// The machine as `proc_root` shows it, stamped `t_us`; `own` are the
    /// run's processes (none: the row has no `own_cpu_ticks`).
    pub fn read(proc_root: &Path, t_us: i64, own: &[OwnProcess]) -> Result<Row, EvalError> {
        let [load1, load5, load15] = read_loads(proc_root)?;
        let (cargo, rustc) = count_builds(proc_root)?;
        let pressure = read_pressure(proc_root);
        Ok(Row {
            t_us,
            load1,
            load5,
            load15,
            cargo,
            rustc,
            io_some_total: pressure.map(|(some, _)| some),
            io_full_total: pressure.map(|(_, full)| full),
            columns: own_columns(proc_root, own),
        })
    }

    /// The first row of a run that names its processes also names them, and
    /// says how many ticks a second its counts are in.
    fn with_header(mut self, own: &[OwnProcess]) -> Row {
        if let Some(columns) = &mut self.columns {
            columns.own = Some(own.to_vec());
            columns.clk_tck = Some(CLK_TCK);
        }
        self
    }

    /// Whether a build stands on the machine.
    pub fn builds(&self) -> bool {
        self.cargo > 0 || self.rustc > 0
    }

    /// The first reason, in the order `load1`, `load15`, build, to refuse a run
    /// that starts under this row.
    pub fn refusal(&self) -> Option<Reason> {
        if reaches(&self.load1, LOAD1_LIMIT) {
            Some(Reason::Load1)
        } else if reaches(&self.load15, LOAD15_LIMIT) {
            Some(Reason::Load15)
        } else if self.builds() {
            Some(Reason::Build)
        } else {
            None
        }
    }

    /// The four values a refusal carries.
    fn values(&self) -> [(&'static str, Value); 4] {
        [
            ("load1", Value::from(self.load1.as_str())),
            ("load15", Value::from(self.load15.as_str())),
            ("cargo", Value::from(self.cargo)),
            ("rustc", Value::from(self.rustc)),
        ]
    }
}

/// The value that failed, in words: `load1 2.00`, `cargo 1, rustc 0`.
fn describe(row: &Row, reason: Reason) -> String {
    match reason {
        Reason::Load1 => format!("load1 {}", row.load1),
        Reason::Load15 => format!("load15 {}", row.load15),
        Reason::Build => describe_build(row),
    }
}

fn describe_build(row: &Row) -> String {
    format!("cargo {}, rustc {}", row.cargo, row.rustc)
}

/// The reason of a refusal by `--own-pid`, asked before the others.
const OWN_PID_REASON: &str = "own_pid";

/// The refusal in words: `own_pid 4246 comm`, `own_pid set`.
fn describe_own(refusal: &OwnRefusal) -> String {
    match refusal.pid {
        Some(pid) => format!("{OWN_PID_REASON} {pid} {}", refusal.fault.name()),
        None => format!("{OWN_PID_REASON} {}", refusal.fault.name()),
    }
}

/// Says why the run was refused; `own` is the refusal of `--own-pid` when that
/// is the reason, and adds its fault and pid (`null` for a fault of the set).
fn log_refused(row: &Row, reason: &str, own: Option<&OwnRefusal>) {
    let mut fields = vec![("reason", Value::from(reason))];
    fields.extend(row.values());
    if let Some(refusal) = own {
        fields.push(("own_fault", Value::from(refusal.fault.name())));
        fields.push(("own_pid", Value::from(refusal.pid)));
    }
    log::warn(
        "stream.machine.refused",
        "worker",
        "the run was refused before any audio was sent",
        &fields,
    );
}

fn log_aborted(row: &Row) {
    log::warn(
        "stream.machine.aborted",
        "worker",
        "a build started during the run",
        &[
            ("cargo", Value::from(row.cargo)),
            ("rustc", Value::from(row.rustc)),
        ],
    );
}

/// Said once, with the first row: the pressure columns of every row are `null`.
fn log_pressure_unavailable() {
    log::warn(
        "stream.machine.pressure_unavailable",
        "worker",
        "pressure/io cannot be read; io_some_total and io_full_total are null",
        &[],
    );
}

/// Creates `path`, and the directory it stands in.
fn create_file(path: &Path) -> Result<File, EvalError> {
    if let Some(parent) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|error| EvalError::io(parent, error))?;
    }
    File::create(path).map_err(|error| EvalError::io(path, error))
}

/// Appends one row as one line.
fn write_row(file: &mut File, path: &Path, row: &Row) -> Result<(), EvalError> {
    let mut line = serde_json::to_string(row).map_err(|error| EvalError::json(path, error))?;
    line.push('\n');
    file.write_all(line.as_bytes())
        .map_err(|error| EvalError::io(path, error))
}

/// Sleeps in short slices until `until`. `false` when `stop` was set first.
fn wait_until(until: Instant, stop: &AtomicBool) -> bool {
    while !stop.load(Ordering::Relaxed) {
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return true;
        }
        std::thread::sleep(left.min(SLICE));
    }
    false
}

/// Sleeps `total`, or until `halt` is set.
pub fn pause(total: Duration, halt: &AtomicBool) {
    wait_until(Instant::now() + total, halt);
}

/// The file being written and where the machine is read from.
struct Sampling {
    file: File,
    path: PathBuf,
    proc_root: PathBuf,
    origin: Instant,
    /// The run's own processes; none for a run that names none.
    own: Vec<OwnProcess>,
}

impl Sampling {
    /// Reads the machine and appends the row.
    fn take_row(&mut self) -> Result<Row, EvalError> {
        let row = Row::read(&self.proc_root, micros_since(self.origin), &self.own)?;
        write_row(&mut self.file, &self.path, &row)?;
        Ok(row)
    }
}

/// What the sampler thread hands back: the file, and the row that showed a
/// build if one did.
struct Sampled {
    state: Sampling,
    build: Option<Row>,
}

/// Writes a row every [`ROW_PERIOD`] until `stop` is set or a row shows a
/// build. A build and a failure both set `halt`.
fn sample(mut state: Sampling, stop: &AtomicBool, halt: &AtomicBool) -> Result<Sampled, EvalError> {
    let mut due = Instant::now() + ROW_PERIOD;
    while wait_until(due, stop) {
        let row = match state.take_row() {
            Ok(row) => row,
            Err(error) => {
                halt.store(true, Ordering::Relaxed);
                return Err(error);
            }
        };
        if row.builds() {
            log_aborted(&row);
            halt.store(true, Ordering::Relaxed);
            return Ok(Sampled {
                state,
                build: Some(row),
            });
        }
        due += ROW_PERIOD;
    }
    Ok(Sampled { state, build: None })
}

/// A run's hold on the machine record: the thread that writes it and the flag
/// that tells the sender to stop.
pub struct Gate {
    halt: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    sampler: Option<JoinHandle<Result<Sampled, EvalError>>>,
}

/// The first row: the run's own processes are read into it when `--own-pid`
/// named a set that passed. A refused set is not read, so the row of a refused
/// run has none of the own fields.
fn first_row(
    proc_root: &Path,
    origin: Instant,
    checked: &Result<Vec<OwnProcess>, OwnRefusal>,
) -> Result<Row, EvalError> {
    let t_us = micros_since(origin);
    match checked {
        Ok(own) => Ok(Row::read(proc_root, t_us, own)?.with_header(own)),
        Err(_) => Row::read(proc_root, t_us, &[]),
    }
}

/// Lets the run start, or refuses it: first by `--own-pid`, then by the first
/// row's `load1`, `load15` and a build. The reason is logged once. The own
/// processes come back on a pass.
fn admit(
    first: &Row,
    checked: Result<Vec<OwnProcess>, OwnRefusal>,
) -> Result<Vec<OwnProcess>, EvalError> {
    let own = match checked {
        Ok(own) => own,
        Err(refusal) => {
            log_refused(first, OWN_PID_REASON, Some(&refusal));
            return Err(EvalError::MachineRefused(describe_own(&refusal)));
        }
    };
    if let Some(reason) = first.refusal() {
        log_refused(first, reason.name(), None);
        return Err(EvalError::MachineRefused(describe(first, reason)));
    }
    Ok(own)
}

/// Writes the first row to `path` and, when the machine is free, starts the
/// rows that follow. A busy machine is `EvalError::MachineRefused`, after one
/// `stream.machine.refused` event; the file then holds that one row.
/// `own_pids` are the run's own processes (`--own-pid`); none is allowed.
pub fn begin(
    proc_root: &Path,
    path: &Path,
    origin: Instant,
    own_pids: &[u32],
) -> Result<Gate, EvalError> {
    let checked = check_own(proc_root, own_pids);
    let first = first_row(proc_root, origin, &checked)?;
    let mut file = create_file(path)?;
    write_row(&mut file, path, &first)?;
    if first.io_some_total.is_none() {
        log_pressure_unavailable();
    }
    let own = admit(&first, checked)?;
    let halt = Arc::new(AtomicBool::new(false));
    let stop = Arc::new(AtomicBool::new(false));
    let state = Sampling {
        file,
        path: path.to_path_buf(),
        proc_root: proc_root.to_path_buf(),
        origin,
        own,
    };
    let sampler = {
        let (stop, halt) = (Arc::clone(&stop), Arc::clone(&halt));
        std::thread::spawn(move || sample(state, &stop, &halt))
    };
    Ok(Gate {
        halt,
        stop,
        sampler: Some(sampler),
    })
}

impl Gate {
    /// The flag the sender watches; it is set when the run must stop.
    pub fn halt(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.halt)
    }

    /// Stops the rows and writes the last one. A build seen by any row is
    /// `EvalError::MachineAborted`; that row is the last of the file.
    pub fn end(mut self) -> Result<(), EvalError> {
        self.stop.store(true, Ordering::Relaxed);
        let Some(sampler) = self.sampler.take() else {
            return Err(EvalError::Invalid(String::from(
                "the machine record was already closed",
            )));
        };
        let sampled = sampler
            .join()
            .map_err(|_| EvalError::Invalid(String::from("the machine sampler panicked")))??;
        if let Some(row) = &sampled.build {
            return Err(EvalError::MachineAborted(describe_build(row)));
        }
        let mut state = sampled.state;
        let last = state.take_row()?;
        if last.builds() {
            log_aborted(&last);
            return Err(EvalError::MachineAborted(describe_build(&last)));
        }
        Ok(())
    }
}

impl Drop for Gate {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::scratch;

    /// A `/proc` of its own: `loadavg` and a `<pid>/comm` for each name given.
    fn proc_with(name: &str, loadavg: &str, comms: &[(&str, &str)]) -> Result<PathBuf, String> {
        let root = scratch(name);
        std::fs::create_dir_all(&root).map_err(|error| error.to_string())?;
        std::fs::write(root.join("loadavg"), loadavg).map_err(|error| error.to_string())?;
        for (dir, comm) in comms {
            let pid = root.join(dir);
            std::fs::create_dir_all(&pid).map_err(|error| error.to_string())?;
            std::fs::write(pid.join("comm"), comm).map_err(|error| error.to_string())?;
        }
        Ok(root)
    }

    #[test]
    fn a_load_is_digits_a_dot_and_two_decimals() {
        assert_eq!(hundredths("1.99"), Some(199));
        assert_eq!(hundredths("0.00"), Some(0));
        assert_eq!(hundredths("12.50"), Some(1250));
        for bad in [
            "", "2", "2.5", "2.500", ".50", "a.50", "2.5x", "-1.00", "+1.00", "n/a",
        ] {
            assert_eq!(hundredths(bad), None, "{bad:?}");
        }
        assert_eq!(hundredths("99999999999.00"), None);
    }

    #[test]
    fn the_loads_are_kept_as_the_kernel_wrote_them() -> Result<(), Box<dyn std::error::Error>> {
        let root = proc_with("m-loads", "1.99 0.05 12.50 1/300 1000\n", &[])?;
        let row = Row::read(&root, 7, &[])?;
        assert_eq!(
            (row.t_us, &row.load1[..], &row.load5[..], &row.load15[..]),
            (7, "1.99", "0.05", "12.50")
        );
        assert_eq!((row.cargo, row.rustc), (0, 0));
        Ok(())
    }

    #[test]
    fn a_loadavg_that_is_not_three_loads_is_an_error() -> Result<(), Box<dyn std::error::Error>> {
        for (index, text) in ["", "1.00 2.00", "1.00 x 2.00 3.00", "1.0 2.00 3.00"]
            .into_iter()
            .enumerate()
        {
            let root = proc_with(&format!("m-bad-{index}"), text, &[])?;
            assert!(Row::read(&root, 0, &[]).is_err(), "{text:?}");
        }
        Ok(())
    }

    #[test]
    fn only_a_pid_directory_named_exactly_cargo_or_rustc_is_counted()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = proc_with(
            "m-count",
            "0.50 0.50 0.50 1/300 1000\n",
            &[
                ("1", "cargo\n"),
                ("2", "cargo\n"),
                ("3", "rustc\n"),
                ("4", "cargo-nextest\n"),
                ("5", "mycargo\n"),
                ("6", "Cargo\n"),
                ("sys", "cargo\n"),
            ],
        )?;
        std::fs::create_dir_all(root.join("7"))?;
        let row = Row::read(&root, 0, &[])?;
        assert_eq!((row.cargo, row.rustc), (2, 1));
        Ok(())
    }

    #[test]
    fn the_refusal_reason_is_the_first_of_load1_load15_build() {
        let busy = |load1: &str, load15: &str, cargo: u32| Row {
            t_us: 0,
            load1: load1.to_owned(),
            load5: String::from("9.99"),
            load15: load15.to_owned(),
            cargo,
            rustc: 0,
            io_some_total: None,
            io_full_total: None,
            columns: None,
        };
        assert_eq!(busy("2.00", "2.50", 1).refusal(), Some(Reason::Load1));
        assert_eq!(busy("1.99", "2.50", 1).refusal(), Some(Reason::Load15));
        assert_eq!(busy("1.99", "2.49", 1).refusal(), Some(Reason::Build));
        assert_eq!(busy("1.99", "2.49", 0).refusal(), None);
    }

    #[test]
    fn the_pressure_totals_are_read_from_the_some_and_full_lines()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = proc_with("m-psi", "0.50 0.50 0.50 1/300 1000\n", &[])?;
        std::fs::create_dir_all(root.join("pressure"))?;
        std::fs::write(
            root.join("pressure").join("io"),
            "some avg10=0.00 avg60=0.10 avg300=0.20 total=12345\n\
             full avg10=0.00 avg60=0.00 avg300=0.00 total=678\n",
        )?;
        let row = Row::read(&root, 0, &[])?;
        assert_eq!(
            (row.io_some_total, row.io_full_total),
            (Some(12345), Some(678))
        );
        let line: Value = serde_json::to_value(&row)?;
        assert_eq!(
            (&line["io_some_total"], &line["io_full_total"]),
            (&Value::from(12345), &Value::from(678))
        );
        Ok(())
    }

    #[test]
    fn missing_or_malformed_pressure_is_null_and_not_an_error()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = proc_with("m-no-psi", "0.50 0.50 0.50 1/300 1000\n", &[])?;
        let row = Row::read(&root, 0, &[])?;
        assert_eq!((row.io_some_total, row.io_full_total), (None, None));
        let line: Value = serde_json::to_value(&row)?;
        assert!(line["io_some_total"].is_null() && line["io_full_total"].is_null());
        std::fs::create_dir_all(root.join("pressure"))?;
        for text in [
            "",
            "some avg10=0.00\nfull total=1\n",
            "some total=x\nfull total=1\n",
            "some total=1\n",
        ] {
            std::fs::write(root.join("pressure").join("io"), text)?;
            assert_eq!(read_pressure(&root), None, "{text:?}");
        }
        Ok(())
    }

    #[test]
    fn a_busy_machine_is_refused_with_one_row_in_the_file() -> Result<(), Box<dyn std::error::Error>>
    {
        let root = proc_with("m-refused", "2.00 0.50 0.50 1/300 1000\n", &[])?;
        let path = scratch("m-refused.machine.jsonl");
        let refused = begin(&root, &path, Instant::now(), &[]);
        assert!(
            matches!(&refused, Err(EvalError::MachineRefused(why)) if why == "load1 2.00"),
            "{:?}",
            refused.err()
        );
        assert_eq!(std::fs::read_to_string(&path)?.lines().count(), 1);
        Ok(())
    }

    #[test]
    fn a_free_machine_gets_a_first_row_and_a_last_row() -> Result<(), Box<dyn std::error::Error>> {
        let root = proc_with("m-free", "0.50 0.50 0.50 1/300 1000\n", &[])?;
        let path = scratch("m-free.machine.jsonl");
        let gate = begin(&root, &path, Instant::now(), &[])?;
        assert!(!gate.halt().load(Ordering::Relaxed));
        gate.end()?;
        let text = std::fs::read_to_string(&path)?;
        assert_eq!(text.lines().count(), 2);
        let Some(line) = text.lines().next() else {
            return Err("the file has no row".into());
        };
        let first: Value = serde_json::from_str(line)?;
        assert_eq!(first["load15"], "0.50");
        Ok(())
    }

    #[test]
    fn a_build_in_the_last_row_ends_the_run_as_aborted() -> Result<(), Box<dyn std::error::Error>> {
        let root = proc_with("m-late", "0.50 0.50 0.50 1/300 1000\n", &[])?;
        let path = scratch("m-late.machine.jsonl");
        let gate = begin(&root, &path, Instant::now(), &[])?;
        let pid = root.join("42");
        std::fs::create_dir_all(&pid)?;
        std::fs::write(pid.join("comm"), "rustc\n")?;
        let ended = gate.end();
        assert!(
            matches!(&ended, Err(EvalError::MachineAborted(why)) if why == "cargo 0, rustc 1"),
            "{:?}",
            ended.err()
        );
        Ok(())
    }

    #[test]
    fn a_pause_ends_at_once_when_the_flag_is_set() {
        let halt = AtomicBool::new(true);
        let began = Instant::now();
        pause(Duration::from_secs(30), &halt);
        assert!(began.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_pause_lasts_its_time_when_the_flag_stays_down() {
        let halt = AtomicBool::new(false);
        let began = Instant::now();
        pause(Duration::from_millis(120), &halt);
        assert!(began.elapsed() >= Duration::from_millis(120));
    }

    // ---- the run's own processes ----

    type Check = Result<(), Box<dyn std::error::Error>>;

    /// A `stat` in the kernel's format; `utime` and `stime` are fields 14 and 15.
    fn stat_text(comm: &str, utime: u64, stime: u64) -> String {
        format!(
            "4242 ({comm}) S 1 4242 4242 0 -1 4194560 100 0 0 0 {utime} {stime} 0 0 20 0 8 0 \
             12345 1000000 100 18446744073709551615\n"
        )
    }

    /// A process `pid` named `comm` with these ticks.
    fn add_own(root: &Path, pid: u32, comm: &str, ticks: (u64, u64)) -> Check {
        let dir = root.join(pid.to_string());
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("comm"), format!("{comm}\n"))?;
        std::fs::write(dir.join("stat"), stat_text(comm, ticks.0, ticks.1))?;
        Ok(())
    }

    /// The `cmdline` of `pid`: each word followed by a NUL, as the kernel writes it.
    fn add_cmdline(root: &Path, pid: u32, words: &[&str]) -> Check {
        std::fs::write(root.join(pid.to_string()).join("cmdline"), nul_words(words))?;
        Ok(())
    }

    fn nul_words(words: &[&str]) -> Vec<u8> {
        words
            .iter()
            .flat_map(|word| word.bytes().chain([0]))
            .collect()
    }

    const SERVER: [&str; 5] = ["llama-server", "-m", "model.gguf", "--port", "18084"];

    /// The three processes of a run: 1250, 700 and 1801 ticks, 3751 together.
    fn proc_with_own(name: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let root = proc_with(name, "0.50 0.50 0.50 1/300 1000\n", &[])?;
        add_own(&root, 4242, "hayamimi", (1000, 250))?;
        add_own(&root, 4243, "hayamimi-relay", (600, 100))?;
        add_own(&root, 4244, "llama-server", (1500, 301))?;
        add_cmdline(&root, 4244, &SERVER)?;
        Ok(root)
    }

    fn refused(root: &Path, pids: &[u32]) -> Option<(OwnFault, Option<u32>)> {
        check_own(root, pids)
            .err()
            .map(|refusal| (refusal.fault, refusal.pid))
    }

    #[test]
    fn utime_and_stime_are_counted_from_the_last_parenthesis() {
        let tail = " S 1 4242 4242 0 -1 4194560 100 0 0 0 1000 250 0 0 20 0 8 0 12345";
        let plain = format!("4242 (hayamimi){tail}");
        assert_eq!(ticks_of_stat(&plain).ok(), Some(1250));
        let awkward = format!("4242 (a b) c){tail}");
        assert_eq!(ticks_of_stat(&awkward).ok(), Some(1250));
        let short = "4242 (hayamimi) S 1 4242 4242 0 -1 4194560 100 0 0 0";
        assert!(ticks_of_stat(short).is_err());
        assert!(ticks_of_stat("").is_err());
        assert!(ticks_of_stat("4242 (hayamimi) S 1 2 3 4 5 6 7 8 9 10 x 250").is_err());
    }

    #[test]
    fn the_port_is_the_word_after_the_one_port_flag() {
        assert!(cmdline_names_own_port(&nul_words(&SERVER)));
        for words in [
            &["llama-server", "--port=18084"][..],
            &["llama-server", "--port", "8080"],
            &["llama-server", "--port", "180840"],
            &["llama-server", "--port", "118084"],
            &["llama-server", "--port"],
            &["llama-server", "--port", "18084", "--port", "18084"],
            &["llama-server", "--port", "8080", "--port", "18084"],
            &["llama-server", "--port", "8080", "--parallel", "18084"],
            &["llama-server", "18084", "--port", "8080"],
            &[],
        ] {
            assert!(!cmdline_names_own_port(&nul_words(words)), "{words:?}");
        }
        assert!(!cmdline_names_own_port(b""));
        // A cmdline whose last word has lost its NUL is still read word by word.
        let unended = [&b"llama-server\0--port\0"[..], b"18084"].concat();
        assert!(cmdline_names_own_port(&unended));
    }

    #[test]
    fn a_complete_set_is_admitted_and_its_ticks_are_summed() -> Check {
        let root = proc_with_own("m-own-set")?;
        let Ok(own) = check_own(&root, &[4242, 4243, 4244]) else {
            return Err("the complete set was refused".into());
        };
        let comms: Vec<(u32, &str)> = own.iter().map(|p| (p.pid, p.comm.as_str())).collect();
        assert_eq!(
            comms,
            [
                (4242, "hayamimi"),
                (4243, "hayamimi-relay"),
                (4244, "llama-server")
            ]
        );
        assert_eq!(own_cpu_ticks(&root, [4242, 4243, 4244]).ok(), Some(3751));
        assert_eq!(check_own(&root, &[]).ok(), Some(Vec::new()));
        Ok(())
    }

    #[test]
    fn each_fault_of_a_pid_is_named_with_that_pid() -> Check {
        let root = proc_with_own("m-own-faults")?;
        add_own(&root, 4246, "sudo", (10, 10))?;
        add_own(&root, 4247, "hayamimi-foo", (10, 10))?;
        add_own(&root, 4248, "hayamimi-eval", (10, 10))?;
        add_own(&root, 4249, "llama-server", (10, 10))?;
        add_cmdline(&root, 4249, &["llama-server", "--port", "8080"])?;
        std::fs::create_dir_all(root.join("4250"))?;
        std::fs::write(root.join("4250").join("comm"), "hayamimi\n")?;
        for (pid, fault) in [
            (4246, OwnFault::Comm),
            (4247, OwnFault::Comm),
            (4248, OwnFault::Comm),
            (4249, OwnFault::Port),
            (4250, OwnFault::Unreadable),
            (4251, OwnFault::Unreadable),
        ] {
            let pids = [4242, 4243, 4244, pid];
            assert_eq!(refused(&root, &pids), Some((fault, Some(pid))), "{pid}");
        }
        assert_eq!(refused(&root, &[4242, 4243, 4244]), None);
        Ok(())
    }

    #[test]
    fn the_first_failing_pid_in_the_order_given_is_the_one_named() -> Check {
        let root = proc_with_own("m-own-order")?;
        add_own(&root, 4246, "sudo", (10, 10))?;
        let sudo_first = [4246, 4250, 4242, 4243, 4244];
        assert_eq!(
            refused(&root, &sudo_first),
            Some((OwnFault::Comm, Some(4246)))
        );
        let missing_first = [4250, 4246, 4242, 4243, 4244];
        assert_eq!(
            refused(&root, &missing_first),
            Some((OwnFault::Unreadable, Some(4250)))
        );
        Ok(())
    }

    #[test]
    fn a_set_that_is_not_the_three_names_once_each_is_a_fault_of_the_set() -> Check {
        let root = proc_with_own("m-own-sets")?;
        add_own(&root, 4248, "hayamimi", (10, 10))?;
        add_own(&root, 4249, "llama-server", (10, 10))?;
        add_cmdline(&root, 4249, &SERVER)?;
        for pids in [
            &[4243, 4244][..],
            &[4242, 4244],
            &[4242, 4243],
            &[4242],
            &[4242, 4243, 4244, 4248],
            &[4248, 4243, 4244, 4242],
            &[4242, 4243, 4244, 4249],
            &[4242, 4242, 4243],
            &[4242, 4243, 4248],
        ] {
            assert_eq!(
                refused(&root, pids),
                Some((OwnFault::Set, None)),
                "{pids:?}"
            );
        }
        Ok(())
    }

    #[test]
    fn a_zombie_server_with_an_empty_cmdline_is_refused_for_its_port() -> Check {
        let root = proc_with_own("m-own-zombie")?;
        std::fs::write(root.join("4244").join("cmdline"), b"")?;
        let expected = Some((OwnFault::Port, Some(4244)));
        assert_eq!(refused(&root, &[4242, 4243, 4244]), expected);
        std::fs::remove_file(root.join("4244").join("cmdline"))?;
        assert_eq!(refused(&root, &[4242, 4243, 4244]), expected);
        Ok(())
    }

    #[test]
    fn a_row_of_a_run_without_own_pids_has_none_of_the_own_fields() -> Check {
        let root = proc_with_own("m-own-none")?;
        let line = serde_json::to_value(Row::read(&root, 0, &[])?)?;
        for key in ["own", "clk_tck", "own_cpu_ticks", "cpu_some_total"] {
            assert!(line.get(key).is_none(), "{key}");
        }
        Ok(())
    }

    #[test]
    fn only_the_first_row_names_the_processes_and_every_row_counts_them() -> Check {
        let root = proc_with_own("m-own-row")?;
        let Ok(own) = check_own(&root, &[4242, 4243, 4244]) else {
            return Err("the set was refused".into());
        };
        let first = serde_json::to_value(Row::read(&root, 0, &own)?.with_header(&own))?;
        let later = serde_json::to_value(Row::read(&root, 5, &own)?)?;
        assert_eq!(first["clk_tck"], 100);
        assert_eq!(
            first["own"],
            serde_json::json!([
                {"pid": 4242, "comm": "hayamimi"},
                {"pid": 4243, "comm": "hayamimi-relay"},
                {"pid": 4244, "comm": "llama-server"},
            ])
        );
        assert!(later.get("own").is_none() && later.get("clk_tck").is_none());
        for row in [&first, &later] {
            assert_eq!(row["own_cpu_ticks"], 3751);
            assert!(row["cpu_some_total"].is_null());
        }
        std::fs::create_dir_all(root.join("pressure"))?;
        let some = "some avg10=0.00 avg60=0.00 avg300=0.00 total=777\n";
        std::fs::write(root.join("pressure").join("cpu"), some)?;
        let with_pressure = serde_json::to_value(Row::read(&root, 9, &own)?)?;
        assert_eq!(with_pressure["cpu_some_total"], 777);
        Ok(())
    }

    #[test]
    fn a_stat_that_vanishes_makes_the_count_null_and_not_an_error() -> Check {
        let root = proc_with_own("m-own-gone")?;
        let Ok(own) = check_own(&root, &[4242, 4243, 4244]) else {
            return Err("the set was refused".into());
        };
        std::fs::remove_file(root.join("4244").join("stat"))?;
        let row = serde_json::to_value(Row::read(&root, 5, &own)?)?;
        assert!(row["own_cpu_ticks"].is_null());
        assert!(row.get("own_cpu_ticks").is_some());
        Ok(())
    }

    #[test]
    fn a_set_that_passes_starts_the_rows_and_a_set_that_fails_refuses_with_one_row() -> Check {
        let root = proc_with_own("m-own-begin")?;
        let path = scratch("m-own-begin.machine.jsonl");
        begin(&root, &path, Instant::now(), &[4242, 4243, 4244])?.end()?;
        let text = std::fs::read_to_string(&path)?;
        assert_eq!(text.lines().count(), 2);
        for line in text.lines() {
            assert_eq!(serde_json::from_str::<Value>(line)?["own_cpu_ticks"], 3751);
        }
        add_own(&root, 4246, "sudo", (10, 10))?;
        let failed = begin(&root, &path, Instant::now(), &[4246, 4242, 4243, 4244]);
        assert!(
            matches!(&failed, Err(EvalError::MachineRefused(why)) if why == "own_pid 4246 comm"),
            "{:?}",
            failed.err()
        );
        let text = std::fs::read_to_string(&path)?;
        assert_eq!(text.lines().count(), 1);
        let row: Value = serde_json::from_str(text.trim())?;
        assert!(row.get("own").is_none() && row.get("own_cpu_ticks").is_none());
        Ok(())
    }

    #[test]
    fn a_refusal_names_the_fault_and_the_pid_or_null_for_the_set() {
        let said = |fault, pid| describe_own(&OwnRefusal { fault, pid });
        assert_eq!(said(OwnFault::Port, Some(4244)), "own_pid 4244 port");
        assert_eq!(said(OwnFault::Set, None), "own_pid set");
        let faults = [
            OwnFault::Unreadable,
            OwnFault::Comm,
            OwnFault::Port,
            OwnFault::Set,
        ];
        assert_eq!(
            faults.map(OwnFault::name),
            ["unreadable", "comm", "port", "set"]
        );
    }

    /// The `utime` plus `stime` of the calling thread, as its own `stat` shows it now.
    #[cfg(target_os = "linux")]
    fn this_thread_ticks() -> Result<u64, String> {
        let text = std::fs::read_to_string("/proc/thread-self/stat").map_err(|e| e.to_string())?;
        ticks_of_stat(&text).map_err(|e| e.to_string())
    }

    /// Runs until the thread has used one clock tick, then returns the ticks it
    /// has used, read just before it ends, and its thread id.
    #[cfg(target_os = "linux")]
    fn spin_one_tick() -> Result<(u64, String), String> {
        while this_thread_ticks()? < 1 {
            std::hint::spin_loop();
        }
        let link = std::fs::read_link("/proc/thread-self").map_err(|e| e.to_string())?;
        let tid = link
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());
        let Some(tid) = tid else {
            return Err(String::from("/proc/thread-self names no thread"));
        };
        Ok((this_thread_ticks()?, tid))
    }

    /// Waits until the ended thread `tid` is gone from the task list, so that
    /// its time is in the process total and not also in a live thread's file.
    #[cfg(target_os = "linux")]
    fn wait_until_unlisted(tid: &str) -> Check {
        let entry = Path::new("/proc/self/task").join(tid);
        for _ in 0..500 {
            if !entry.exists() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Err(format!("thread {tid} is still listed").into())
    }

    /// The ticks of the threads that are alive: the sum of `task/*/stat`.
    #[cfg(target_os = "linux")]
    fn live_thread_ticks() -> Result<u64, Box<dyn std::error::Error>> {
        let mut total = 0_u64;
        for entry in std::fs::read_dir("/proc/self/task")? {
            // A thread that ended after the listing has no file; its time is in the process total.
            let Ok(text) = std::fs::read_to_string(entry?.path().join("stat")) else {
                continue;
            };
            total += ticks_of_stat(&text)?;
        }
        Ok(total)
    }

    /// A process's `stat` holds the time of its ended threads, and the
    /// `task/*/stat` files do not. The count is read through the product's own
    /// reader, so that a reader which summed the live threads would miss the
    /// ended thread's ticks and fail. The thread spins until it has used a
    /// tick: with 0 ticks the comparison would be `0 >= 0` and prove nothing.
    /// Linux only: the per-thread files under `/proc` exist nowhere else.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_process_stat_counts_the_ticks_of_its_ended_threads() -> Check {
        let worker = std::thread::spawn(spin_one_tick);
        let (ended, tid) = worker
            .join()
            .map_err(|_| "the spinning thread panicked")??;
        wait_until_unlisted(&tid)?;
        // The live threads first and the process after: a thread that ends in
        // between moves its time into the process total, which only grows.
        let live = live_thread_ticks()?;
        let process = own_cpu_ticks(Path::new("/proc"), [std::process::id()])?;
        println!("ended thread {ended} ticks, live threads {live} ticks, process {process} ticks");
        assert!(ended >= 1, "the thread ended before it used a tick");
        assert!(
            process >= live + ended,
            "process {process} < live threads {live} + ended thread {ended}"
        );
        Ok(())
    }
}

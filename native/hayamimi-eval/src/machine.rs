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

impl Row {
    /// The machine as `proc_root` shows it, stamped `t_us`.
    pub fn read(proc_root: &Path, t_us: i64) -> Result<Row, EvalError> {
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
        })
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

fn log_refused(row: &Row, reason: Reason) {
    let mut fields = vec![("reason", Value::from(reason.name()))];
    fields.extend(row.values());
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
}

impl Sampling {
    /// Reads the machine and appends the row.
    fn take_row(&mut self) -> Result<Row, EvalError> {
        let row = Row::read(&self.proc_root, micros_since(self.origin))?;
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

/// Writes the first row to `path` and, when the machine is free, starts the
/// rows that follow. A busy machine is `EvalError::MachineRefused`, after one
/// `stream.machine.refused` event; the file then holds that one row.
pub fn begin(proc_root: &Path, path: &Path, origin: Instant) -> Result<Gate, EvalError> {
    let first = Row::read(proc_root, micros_since(origin))?;
    let mut file = create_file(path)?;
    write_row(&mut file, path, &first)?;
    if first.io_some_total.is_none() {
        log_pressure_unavailable();
    }
    if let Some(reason) = first.refusal() {
        log_refused(&first, reason);
        return Err(EvalError::MachineRefused(describe(&first, reason)));
    }
    let halt = Arc::new(AtomicBool::new(false));
    let stop = Arc::new(AtomicBool::new(false));
    let state = Sampling {
        file,
        path: path.to_path_buf(),
        proc_root: proc_root.to_path_buf(),
        origin,
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
        let row = Row::read(&root, 7)?;
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
            assert!(Row::read(&root, 0).is_err(), "{text:?}");
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
        let row = Row::read(&root, 0)?;
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
        let row = Row::read(&root, 0)?;
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
        let row = Row::read(&root, 0)?;
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
        let refused = begin(&root, &path, Instant::now());
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
        let gate = begin(&root, &path, Instant::now())?;
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
        let gate = begin(&root, &path, Instant::now())?;
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
}

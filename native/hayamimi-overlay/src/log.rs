//! The overlay's log: JSONL in `observability-log/v1`, written to a file.
//!
//! The window runs under the GUI subsystem and has no stdout, so the file is
//! the only place a log line can go. The file turns over when it is a day old
//! or 256 MiB large, whichever comes first: the old file is renamed to
//! `overlay-<YYYY-MM-DD>-<n>.jsonl`, compressed beside it, and the compressed
//! files older than 30 days are deleted. The first line of the new file is
//! `overlay.log.rotated`, which carries the line count of the file before it.
//! Event text carries no subtitle, no settings path and no font path.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use flate2::Compression;
use flate2::write::GzEncoder;
use hayamimi_relay::calendar::{self, Date, JST_OFFSET_SECS};
use hayamimi_relay::logging::{Level, LogEvent, LogSink};

/// Identifies this build in every line.
pub const BUILD_REVISION: &str = concat!("hayamimi-overlay-", env!("CARGO_PKG_VERSION"));
/// The size at which the file turns over.
pub const ROTATE_BYTES: u64 = 256 * 1024 * 1024;
/// Compressed files older than this many days are deleted.
pub const KEEP_DAYS: i64 = 30;
/// After a failed turnover the next attempt waits this many seconds, so that
/// a full disk costs one failure line per attempt and not one per event.
const RETRY_AFTER_FAILURE_SECS: i64 = 300;

/// `%LOCALAPPDATA%\hayamimi\logs\overlay.jsonl`; `None` when the machine
/// names no per-user local data directory.
pub fn default_path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|dir| dir.join("hayamimi").join("logs").join("overlay.jsonl"))
}

/// The JST calendar day, counted from 1970-01-01, that `secs` falls on.
fn day_of(secs: i64) -> i64 {
    (secs + JST_OFFSET_SECS).div_euclid(86_400)
}

fn epoch_secs(time: SystemTime) -> Option<i64> {
    let since = time.duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(since.as_secs()).ok()
}

/// Whether a file that was started on `opened_day` and holds `bytes` is due to
/// turn over on `today`. A file with nothing in it never is.
fn due(opened_day: i64, today: i64, bytes: u64, max_bytes: u64) -> bool {
    bytes > 0 && (today > opened_day || bytes >= max_bytes)
}

/// `path` with `suffix` added to its name.
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// The first `overlay-<date>-<n>.jsonl` in `dir`, counting `n` from 1, that
/// neither it nor its compressed form takes.
fn dated_path(dir: &Path, day: i64) -> PathBuf {
    let date = Date::from_days_since_epoch(day);
    let mut n = 1_u32;
    loop {
        let name = format!(
            "overlay-{:04}-{:02}-{:02}-{n}.jsonl",
            date.year, date.month, date.day
        );
        let path = dir.join(name);
        if !path.exists() && !with_suffix(&path, ".gz").exists() {
            return path;
        }
        n += 1;
    }
}

fn open_append(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

fn count_lines(path: &Path) -> io::Result<u64> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut lines = 0_u64;
    loop {
        let chunk = reader.fill_buf()?;
        if chunk.is_empty() {
            return Ok(lines);
        }
        lines += chunk.iter().filter(|byte| **byte == b'\n').count() as u64;
        let used = chunk.len();
        reader.consume(used);
    }
}

/// Writes `<path>.gz` beside `path` through a part file, then deletes `path`.
fn compress(path: &Path) -> io::Result<()> {
    let target = with_suffix(path, ".gz");
    let part = with_suffix(path, ".gz.part");
    let mut input = File::open(path)?;
    let mut encoder = GzEncoder::new(File::create(&part)?, Compression::default());
    io::copy(&mut input, &mut encoder)?;
    let written = encoder.finish()?;
    written.sync_all()?;
    drop(written);
    fs::rename(&part, &target)?;
    fs::remove_file(path)
}

/// Deletes the compressed logs in `dir` that were last written more than
/// [`KEEP_DAYS`] days before `now`.
fn purge_old(dir: &Path, now: i64) -> io::Result<()> {
    let limit = now - KEEP_DAYS * 86_400;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !(name.starts_with("overlay-") && name.ends_with(".jsonl.gz")) {
            continue;
        }
        let Some(written) = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(epoch_secs)
        else {
            continue;
        };
        if written < limit {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

/// The line an event is written as: this build's revision, and the event's
/// own name as the message when it has none.
fn line_of(mut event: LogEvent) -> String {
    event.build_revision = BUILD_REVISION;
    if event.message.is_empty() {
        event.message.clone_from(&event.event);
    }
    event.to_json_line()
}

/// A log file that turns over by itself.
pub struct FileSink {
    path: PathBuf,
    dir: PathBuf,
    /// `None` only while the file is being turned over, or when it could not
    /// be opened again; events are then dropped.
    file: Option<File>,
    opened_day: i64,
    bytes: u64,
    lines: u64,
    max_bytes: u64,
    retry_at: i64,
}

impl FileSink {
    /// Opens `path` for appending, making its directory if it is missing.
    pub fn open(path: &Path) -> io::Result<FileSink> {
        FileSink::open_with(path, ROTATE_BYTES)
    }

    /// Like [`FileSink::open`], with the size at which the file turns over.
    pub fn open_with(path: &Path, max_bytes: u64) -> io::Result<FileSink> {
        let Some(dir) = path.parent() else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the log path has no directory",
            ));
        };
        fs::create_dir_all(dir)?;
        let file = open_append(path)?;
        let meta = file.metadata()?;
        let today = calendar::now_epoch_secs().map(day_of);
        let started = meta.modified().ok().and_then(epoch_secs).map(day_of);
        let Some(opened_day) = started.or(today) else {
            return Err(io::Error::other("the clock is set before 1970"));
        };
        let lines = if meta.len() > 0 {
            count_lines(path)?
        } else {
            0
        };
        Ok(FileSink {
            path: path.to_owned(),
            dir: dir.to_owned(),
            file: Some(file),
            opened_day,
            bytes: meta.len(),
            lines,
            max_bytes,
            retry_at: 0,
        })
    }

    fn append(&mut self, line: &str) {
        let Some(file) = self.file.as_mut() else {
            return;
        };
        let mut record = String::with_capacity(line.len() + 1);
        record.push_str(line);
        record.push('\n');
        // A log that cannot be written has nowhere to say so.
        if file.write_all(record.as_bytes()).is_ok() {
            self.bytes += record.len() as u64;
            self.lines += 1;
        }
    }

    /// Writes one event, turning the file over first if it is due at `now`.
    fn emit_at(&mut self, event: LogEvent, now: Option<i64>) {
        if let Some(now) = now {
            self.turn_over_if_due(now);
        }
        self.append(&line_of(event));
    }

    fn turn_over_if_due(&mut self, now: i64) {
        let today = day_of(now);
        if self.bytes == 0 {
            self.opened_day = today;
            return;
        }
        if now < self.retry_at || !due(self.opened_day, today, self.bytes, self.max_bytes) {
            return;
        }
        if let Err(error) = self.turn_over(now) {
            self.retry_at = now + RETRY_AFTER_FAILURE_SECS;
            let failed = LogEvent::new(Level::Warn, "overlay.log.rotate_failed", "browser")
                .with("reason", format!("{:?}", error.kind()))
                .with_error("LOG_ROTATE_FAILED");
            self.append(&line_of(failed));
        }
    }

    /// Renames the file to its dated name, opens a new one under the old name,
    /// and compresses the old one. The new file is open whatever the later
    /// steps do; only a failed rename leaves the old file in use.
    fn turn_over(&mut self, now: i64) -> io::Result<()> {
        // The file is closed first: Windows does not rename a file that is open.
        drop(self.file.take());
        let dated = dated_path(&self.dir, self.opened_day);
        let renamed = fs::rename(&self.path, &dated);
        let reopened = open_append(&self.path);
        if let Err(error) = renamed {
            self.file = reopened.ok();
            return Err(error);
        }
        self.file = Some(reopened?);
        let previous_lines = std::mem::take(&mut self.lines);
        self.bytes = 0;
        self.opened_day = day_of(now);
        let rotated = LogEvent::new(Level::Info, "overlay.log.rotated", "browser")
            .with("lines", previous_lines);
        self.append(&line_of(rotated));
        compress(&dated)?;
        purge_old(&self.dir, now)
    }
}

impl LogSink for FileSink {
    fn emit(&mut self, event: LogEvent) {
        self.emit_at(event, calendar::now_epoch_secs());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::GzDecoder;
    use std::io::Read;
    use std::time::Duration;

    fn scratch(name: &str) -> Result<tempfile::TempDir, io::Error> {
        tempfile::Builder::new()
            .prefix(name)
            .tempdir_in(concat!(env!("CARGO_MANIFEST_DIR"), "/../target"))
    }

    fn at(year: i32, month: u8, day: u8, hour: i64) -> i64 {
        Date { year, month, day }.days_since_epoch() * 86_400 + hour * 3600 - JST_OFFSET_SECS
    }

    fn note(name: &str) -> LogEvent {
        LogEvent::new(Level::Info, name, "browser")
    }

    fn read_lines(path: &Path) -> Result<Vec<serde_json::Value>, Box<dyn std::error::Error>> {
        fs::read_to_string(path)?
            .lines()
            .map(|line| Ok(serde_json::from_str(line)?))
            .collect()
    }

    fn names_in(dir: &Path) -> Result<Vec<String>, io::Error> {
        let mut names = Vec::new();
        for entry in fs::read_dir(dir)? {
            names.push(entry?.file_name().to_string_lossy().into_owned());
        }
        names.sort();
        Ok(names)
    }

    #[test]
    fn a_line_carries_this_builds_revision_and_the_event_name_as_its_message()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = scratch("line")?;
        let path = dir.path().join("overlay.jsonl");
        let mut sink = FileSink::open(&path)?;
        sink.emit(note("overlay.window.opened"));
        let lines = read_lines(&path)?;
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0]["build_revision"], BUILD_REVISION);
        assert_eq!(lines[0]["message"], "overlay.window.opened");
        assert_eq!(lines[0]["schema_version"], "observability-log/v1");
        assert!(BUILD_REVISION.starts_with("hayamimi-overlay-"));
        Ok(())
    }

    #[test]
    fn a_message_the_event_already_has_is_kept() -> Result<(), Box<dyn std::error::Error>> {
        let dir = scratch("message")?;
        let path = dir.path().join("overlay.jsonl");
        let mut sink = FileSink::open(&path)?;
        let mut event = note("a.b.c");
        event.message = String::from("kept");
        sink.emit(event);
        assert_eq!(read_lines(&path)?[0]["message"], "kept");
        Ok(())
    }

    #[test]
    fn the_directory_of_the_log_is_made_when_it_is_missing()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = scratch("mkdir")?;
        let path = dir.path().join("a").join("b").join("overlay.jsonl");
        let mut sink = FileSink::open(&path)?;
        sink.emit(note("x.y.z"));
        assert_eq!(read_lines(&path)?.len(), 1);
        Ok(())
    }

    #[test]
    fn a_path_with_no_directory_part_is_refused() {
        assert!(FileSink::open(Path::new("")).is_err());
    }

    #[test]
    fn a_new_day_turns_the_file_over_and_the_old_file_is_kept_compressed()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = scratch("daily")?;
        let path = dir.path().join("overlay.jsonl");
        let mut sink = FileSink::open_with(&path, ROTATE_BYTES)?;
        sink.emit_at(note("first"), Some(at(2026, 10, 20, 9)));
        sink.emit_at(note("second"), Some(at(2026, 10, 20, 23)));
        sink.emit_at(note("third"), Some(at(2026, 10, 21, 0)));
        assert_eq!(
            names_in(dir.path())?,
            ["overlay-2026-10-20-1.jsonl.gz", "overlay.jsonl"]
        );
        let mut old = String::new();
        GzDecoder::new(File::open(
            dir.path().join("overlay-2026-10-20-1.jsonl.gz"),
        )?)
        .read_to_string(&mut old)?;
        assert_eq!(old.lines().count(), 2);
        let now = read_lines(&path)?;
        assert_eq!(now[0]["event"], "overlay.log.rotated");
        assert_eq!(now[0]["lines"], 2);
        assert_eq!(now[1]["event"], "third");
        Ok(())
    }

    #[test]
    fn the_size_limit_turns_the_file_over_and_a_second_turn_on_one_day_is_numbered_2()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = scratch("size")?;
        let path = dir.path().join("overlay.jsonl");
        let mut sink = FileSink::open_with(&path, 400)?;
        let noon = at(2026, 10, 20, 12);
        for _ in 0..12 {
            sink.emit_at(note("x.y.z"), Some(noon));
        }
        let names = names_in(dir.path())?;
        assert!(
            names.contains(&String::from("overlay-2026-10-20-1.jsonl.gz")),
            "{names:?}"
        );
        assert!(
            names.contains(&String::from("overlay-2026-10-20-2.jsonl.gz")),
            "{names:?}"
        );
        assert!(names.contains(&String::from("overlay.jsonl")));
        Ok(())
    }

    #[test]
    fn an_empty_file_does_not_turn_over_on_a_new_day_and_is_dated_by_its_first_line()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = scratch("empty")?;
        let path = dir.path().join("overlay.jsonl");
        let mut sink = FileSink::open_with(&path, ROTATE_BYTES)?;
        sink.emit_at(note("a"), Some(at(2026, 10, 25, 9)));
        sink.emit_at(note("b"), Some(at(2026, 10, 26, 9)));
        assert_eq!(
            names_in(dir.path())?,
            ["overlay-2026-10-25-1.jsonl.gz", "overlay.jsonl"]
        );
        Ok(())
    }

    #[test]
    fn a_file_left_by_an_earlier_day_turns_over_at_the_first_event_and_counts_its_lines()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = scratch("restart")?;
        let path = dir.path().join("overlay.jsonl");
        fs::write(&path, "{\"a\":1}\n{\"a\":2}\n{\"a\":3}\n")?;
        let long_ago = File::options().write(true).open(&path)?;
        long_ago.set_modified(UNIX_EPOCH + Duration::from_secs(1_000_000_000))?;
        drop(long_ago);
        let mut sink = FileSink::open_with(&path, ROTATE_BYTES)?;
        sink.emit_at(note("now"), Some(at(2026, 10, 20, 9)));
        let first = &read_lines(&path)?[0];
        assert_eq!(first["event"], "overlay.log.rotated");
        assert_eq!(first["lines"], 3);
        assert!(
            names_in(dir.path())?
                .iter()
                .any(|name| name.starts_with("overlay-2001-09-09-1"))
        );
        Ok(())
    }

    #[test]
    fn compressed_files_older_than_30_days_are_deleted_and_the_rest_stay()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = scratch("purge")?;
        let now = at(2026, 10, 20, 9);
        let old = dir.path().join("overlay-2026-09-01-1.jsonl.gz");
        let recent = dir.path().join("overlay-2026-10-10-1.jsonl.gz");
        let other = dir.path().join("other-2020-01-01-1.jsonl.gz");
        for (path, age_days) in [(&old, 31_u64), (&recent, 29), (&other, 400)] {
            fs::write(path, b"x")?;
            let file = File::options().write(true).open(path)?;
            let written = u64::try_from(now)? - age_days * 86_400;
            file.set_modified(UNIX_EPOCH + Duration::from_secs(written))?;
        }
        purge_old(dir.path(), now)?;
        assert!(!old.exists());
        assert!(recent.exists());
        assert!(
            other.exists(),
            "a file that is not the overlay's log is left alone"
        );
        Ok(())
    }

    #[test]
    fn a_clock_before_1970_never_turns_the_file_over() -> Result<(), Box<dyn std::error::Error>> {
        let dir = scratch("noclock")?;
        let path = dir.path().join("overlay.jsonl");
        let mut sink = FileSink::open_with(&path, 50)?;
        for _ in 0..10 {
            sink.emit_at(note("x.y.z"), None);
        }
        assert_eq!(names_in(dir.path())?, ["overlay.jsonl"]);
        assert_eq!(read_lines(&path)?.len(), 10);
        Ok(())
    }

    #[test]
    fn a_turnover_is_due_for_a_later_day_or_a_full_file_but_never_an_empty_one() {
        assert!(due(10, 11, 1, 100));
        assert!(due(10, 10, 100, 100));
        assert!(!due(10, 10, 99, 100));
        assert!(
            !due(10, 9, 99, 100),
            "a clock that went back is not a new day"
        );
        assert!(!due(10, 11, 0, 100));
    }

    #[test]
    fn the_dated_name_skips_a_name_taken_by_a_file_or_its_compressed_form()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = scratch("dated")?;
        let day = Date {
            year: 2026,
            month: 10,
            day: 20,
        }
        .days_since_epoch();
        fs::write(dir.path().join("overlay-2026-10-20-1.jsonl"), b"")?;
        fs::write(dir.path().join("overlay-2026-10-20-2.jsonl.gz"), b"")?;
        assert_eq!(
            dated_path(dir.path(), day),
            dir.path().join("overlay-2026-10-20-3.jsonl")
        );
        Ok(())
    }

    #[test]
    fn the_calendar_day_is_counted_in_jst() {
        // One second before midnight JST is still the 20th; at midnight it is the 21st.
        let midnight = at(2026, 10, 21, 0);
        assert_eq!(day_of(midnight - 1) + 1, day_of(midnight));
    }

    #[test]
    fn the_default_path_ends_in_logs_overlay_jsonl() {
        if let Some(path) = default_path() {
            assert!(path.ends_with("hayamimi/logs/overlay.jsonl"), "{path:?}");
        }
    }
}

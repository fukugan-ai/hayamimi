//! A file that is read again whenever it changes, without a restart.
//!
//! Used for the exchange-rate file and the table files. A file that cannot be
//! read or parsed leaves the previous content in use and costs one `warn`
//! event, once per distinct bad content.

use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use crate::logging::{Level, LogEvent, LogSink};

/// A file's modification time and length are trusted only once the file is
/// at least this much older than the moment it was read; a newer file may be
/// rewritten inside the timestamp's resolution, so its bytes are compared.
const SETTLE: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, PartialEq, Eq)]
struct Stat {
    modified: Option<SystemTime>,
    len: u64,
}

struct Seen {
    bytes: Vec<u8>,
    stat: Stat,
    read_at: SystemTime,
}

impl Seen {
    fn settled(&self) -> bool {
        self.stat
            .modified
            .is_some_and(|modified| modified + SETTLE <= self.read_at)
    }
}

/// A parsed value backed by an optional file, with a built-in fallback.
pub struct Watched<T> {
    table: &'static str,
    path: Option<PathBuf>,
    make_default: fn() -> Result<T, String>,
    parse: fn(&[u8]) -> Result<T, String>,
    current: T,
    from_file: bool,
    seen: Option<Seen>,
    last_error: Option<String>,
}

impl<T> Watched<T> {
    /// Opens the value: the built-in default first, then the file at `path`
    /// when it exists. `table` names the table in log events.
    pub fn open(
        table: &'static str,
        path: Option<PathBuf>,
        make_default: fn() -> Result<T, String>,
        parse: fn(&[u8]) -> Result<T, String>,
        sink: &mut dyn LogSink,
    ) -> Result<Watched<T>, String> {
        let mut watched = Watched {
            table,
            path,
            make_default,
            parse,
            current: make_default()?,
            from_file: false,
            seen: None,
            last_error: None,
        };
        watched.refresh(sink);
        Ok(watched)
    }

    /// The value now in use.
    pub fn current(&self) -> &T {
        &self.current
    }

    /// Re-reads the file if it changed since the last call.
    pub fn refresh(&mut self, sink: &mut dyn LogSink) {
        let Some(path) = self.path.clone() else {
            return;
        };
        let meta = match fs::metadata(&path) {
            Ok(meta) => meta,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                self.fall_back_to_default(sink);
                return;
            }
            Err(error) => {
                self.note_failure(sink, &format!("stat:{:?}", error.kind()));
                return;
            }
        };
        let stat = Stat {
            modified: meta.modified().ok(),
            len: meta.len(),
        };
        if let Some(seen) = &self.seen
            && seen.stat == stat
            && seen.settled()
        {
            return;
        }
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.note_failure(sink, &format!("read:{:?}", error.kind()));
                return;
            }
        };
        if self.seen.as_ref().is_some_and(|seen| seen.bytes == bytes) {
            self.remember(bytes, stat);
            return;
        }
        match (self.parse)(&bytes) {
            Ok(value) => {
                self.current = value;
                self.from_file = true;
                self.last_error = None;
            }
            Err(_) => self.note_failure(sink, "parse"),
        }
        self.remember(bytes, stat);
    }

    fn remember(&mut self, bytes: Vec<u8>, stat: Stat) {
        self.seen = Some(Seen {
            bytes,
            stat,
            read_at: SystemTime::now(),
        });
    }

    /// The file is gone: a value that came from it gives way to the default.
    fn fall_back_to_default(&mut self, sink: &mut dyn LogSink) {
        self.seen = None;
        if !self.from_file {
            return;
        }
        match (self.make_default)() {
            Ok(value) => {
                self.current = value;
                self.from_file = false;
                self.last_error = None;
            }
            Err(_) => self.note_failure(sink, "removed"),
        }
    }

    fn note_failure(&mut self, sink: &mut dyn LogSink, reason: &str) {
        if self.last_error.as_deref() == Some(reason) {
            return;
        }
        self.last_error = Some(reason.to_owned());
        sink.emit(
            LogEvent::new(Level::Warn, "table.load.failed", "storage")
                .with("table", self.table)
                .with("reason", reason)
                .with_error("TABLE_LOAD"),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::MemorySink;

    fn parse_number(bytes: &[u8]) -> Result<u32, String> {
        std::str::from_utf8(bytes)
            .map_err(|error| error.to_string())?
            .trim()
            .parse()
            .map_err(|_| String::from("not a number"))
    }

    fn default_seven() -> Result<u32, String> {
        Ok(7)
    }

    fn default_nine() -> Result<u32, String> {
        Ok(9)
    }

    fn default_broken() -> Result<u32, String> {
        Err(String::from("no default"))
    }

    fn scratch(name: &str) -> PathBuf {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/unit-tmp");
        let dir = base.join(format!("watched-{}-{name}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        dir.join("value.txt")
    }

    #[test]
    fn without_a_path_the_default_stays() -> Result<(), Box<dyn std::error::Error>> {
        let mut sink = MemorySink::default();
        let watched = Watched::open("t", None, default_seven, parse_number, &mut sink)?;
        assert_eq!(watched.current(), &7);
        Ok(())
    }

    #[test]
    fn a_bad_default_is_an_error() {
        let mut sink = MemorySink::default();
        let opened = Watched::open("t", None, default_broken, parse_number, &mut sink);
        assert!(opened.is_err());
    }

    #[test]
    fn a_changed_file_is_used_on_the_next_refresh() -> Result<(), Box<dyn std::error::Error>> {
        let path = scratch("changed");
        fs::write(&path, "1")?;
        let mut sink = MemorySink::default();
        let mut watched = Watched::open(
            "t",
            Some(path.clone()),
            default_seven,
            parse_number,
            &mut sink,
        )?;
        assert_eq!(watched.current(), &1);
        fs::write(&path, "2")?;
        watched.refresh(&mut sink);
        assert_eq!(watched.current(), &2);
        assert!(sink.events.is_empty());
        Ok(())
    }

    #[test]
    fn a_broken_file_keeps_the_previous_value_and_warns_once()
    -> Result<(), Box<dyn std::error::Error>> {
        let path = scratch("broken");
        fs::write(&path, "1")?;
        let mut sink = MemorySink::default();
        let mut watched = Watched::open(
            "t",
            Some(path.clone()),
            default_seven,
            parse_number,
            &mut sink,
        )?;
        fs::write(&path, "oops")?;
        watched.refresh(&mut sink);
        watched.refresh(&mut sink);
        assert_eq!(watched.current(), &1);
        assert_eq!(sink.events.len(), 1);
        assert_eq!(sink.events[0].level, Level::Warn);
        Ok(())
    }

    #[test]
    fn a_valid_file_after_a_broken_one_is_used() -> Result<(), Box<dyn std::error::Error>> {
        let path = scratch("recover");
        fs::write(&path, "1")?;
        let mut sink = MemorySink::default();
        let mut watched = Watched::open(
            "t",
            Some(path.clone()),
            default_seven,
            parse_number,
            &mut sink,
        )?;
        fs::write(&path, "")?;
        watched.refresh(&mut sink);
        fs::write(&path, "3")?;
        watched.refresh(&mut sink);
        assert_eq!(watched.current(), &3);
        Ok(())
    }

    #[test]
    fn a_missing_file_leaves_the_default() -> Result<(), Box<dyn std::error::Error>> {
        let path = scratch("missing").with_file_name("absent.txt");
        let mut sink = MemorySink::default();
        let watched = Watched::open("t", Some(path), default_seven, parse_number, &mut sink)?;
        assert_eq!(watched.current(), &7);
        assert!(sink.events.is_empty());
        Ok(())
    }

    #[test]
    fn a_removed_override_gives_way_to_the_default() -> Result<(), Box<dyn std::error::Error>> {
        let path = scratch("removed");
        fs::write(&path, "5")?;
        let mut sink = MemorySink::default();
        let mut watched = Watched::open(
            "t",
            Some(path.clone()),
            default_nine,
            parse_number,
            &mut sink,
        )?;
        assert_eq!(watched.current(), &5);
        fs::remove_file(&path)?;
        watched.refresh(&mut sink);
        assert_eq!(watched.current(), &9);
        Ok(())
    }
}

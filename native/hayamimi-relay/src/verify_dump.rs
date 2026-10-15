//! A developer's record of the translations V was handed.
//!
//! The log never carries the text of a translation. When a developer needs to
//! see why V withheld a line, the relay is started with `--verify-text <file>`
//! and every translation handed to V (T's before E, E's after it) is written to
//! that file, one JSON object per line: `{"id", "pass", "text"}`. The file is
//! named by the caller and belongs outside the repository. Without the option
//! nothing is written.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use serde_json::json;

use crate::logging::{Level, LogEvent, LogSink};
use crate::pipeline::{Observer, Pass};

/// The source texts V judged translations against, in memory, as
/// `(id, source)` rows in the order V ran. A developer or a test reads it back
/// to see which text a reused answer was verified against; the file never
/// carries it.
#[derive(Clone, Default)]
pub struct SourceLog(Arc<Mutex<Vec<(String, String)>>>);

impl SourceLog {
    /// The sources V ran against for the line `id`, in order.
    pub fn sources_of(&self, id: &str) -> Vec<String> {
        self.rows()
            .iter()
            .filter(|(row_id, _)| row_id == id)
            .map(|(_, source)| source.clone())
            .collect()
    }

    fn push(&self, id: &str, source: &str) {
        self.rows().push((id.to_owned(), source.to_owned()));
    }

    fn rows(&self) -> MutexGuard<'_, Vec<(String, String)>> {
        match self.0.lock() {
            Ok(rows) => rows,
            // A thread that panicked while holding the lock left whole rows.
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

/// The open file, shared by the lanes.
#[derive(Clone)]
pub struct VerifyDump {
    file: Arc<Mutex<File>>,
    sources: Option<SourceLog>,
}

impl VerifyDump {
    /// Opens `path` for appending, creating it. A file that cannot be opened is
    /// left out and the run goes on without it.
    pub fn open(path: &Path, sink: &mut dyn LogSink) -> Option<VerifyDump> {
        match OpenOptions::new().create(true).append(true).open(path) {
            Ok(file) => Some(VerifyDump {
                file: Arc::new(Mutex::new(file)),
                sources: None,
            }),
            Err(_) => {
                sink.emit(
                    LogEvent::new(Level::Error, "translate.dump.failed", "storage")
                        .with_error("DUMP_OPEN"),
                );
                None
            }
        }
    }

    /// Also keeps the source texts V is run against in `log`.
    pub fn keeping_sources(mut self, log: SourceLog) -> VerifyDump {
        self.sources = Some(log);
        self
    }

    /// The observer of the line `id`.
    pub fn observer<'a>(&'a self, id: &'a str) -> DumpObserver<'a> {
        DumpObserver { dump: self, id }
    }

    /// Appends one row. A dump that cannot be written loses that row only; it
    /// is a developer's aid and must not stop a line from being translated.
    fn write(&self, id: &str, pass: Pass, text: &str) {
        let row = row(id, pass, text);
        if let Ok(mut file) = self.file.lock() {
            let _ = file.write_all(row.as_bytes());
        }
    }
}

/// One row of the file: a JSON object and a newline.
fn row(id: &str, pass: Pass, text: &str) -> String {
    let mut row = json!({ "id": id, "pass": pass.as_str(), "text": text }).to_string();
    row.push('\n');
    row
}

/// Hands the translations V sees for one line to a [`VerifyDump`].
pub struct DumpObserver<'a> {
    dump: &'a VerifyDump,
    id: &'a str,
}

impl Observer for DumpObserver<'_> {
    fn verify_source(&mut self, source: &str) {
        if let Some(log) = &self.dump.sources {
            log.push(self.id, source);
        }
    }

    fn verify_input(&mut self, translation: &str) {
        self.dump.write(self.id, Pass::E, translation);
    }

    fn draft_verify_input(&mut self, translation: &str) {
        self.dump.write(self.id, Pass::T, translation);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_is_one_json_line_with_the_id_the_pass_and_the_text() -> Result<(), serde_json::Error> {
        let made = row("mic-1", Pass::T, "line one\nline \"two\"");
        assert!(made.ends_with('\n') && made.matches('\n').count() == 1);
        let parsed: serde_json::Value = serde_json::from_str(made.trim_end())?;
        assert_eq!(parsed["id"], "mic-1");
        assert_eq!(parsed["pass"], "t");
        assert_eq!(parsed["text"], "line one\nline \"two\"");
        Ok(())
    }

    #[test]
    fn the_pass_of_an_edited_translation_is_e() -> Result<(), serde_json::Error> {
        let parsed: serde_json::Value = serde_json::from_str(row("x", Pass::E, "").trim_end())?;
        assert_eq!(parsed["pass"], "e");
        assert_eq!(parsed["text"], "");
        Ok(())
    }
}

//! Structured JSONL log events (`observability-log/v1`).
//!
//! One event is one JSON object on one line. Subtitle text, source text and
//! file paths never appear in an event; callers pass identifiers and counts.

use std::collections::BTreeMap;
use std::io::Write;

use serde::Serialize;
use serde_json::Value;

use crate::calendar;
use crate::strings::Strings;

pub const SCHEMA_VERSION: &str = "observability-log/v1";
pub const PROJECT_SLUG: &str = "hayamimi";
/// Prefix of every error code this project emits.
pub const ERROR_PREFIX: &str = "HYM";
/// Identifies the running build: the crate version fixed at compile time.
pub const BUILD_REVISION: &str = concat!("hayamimi-relay-", env!("CARGO_PKG_VERSION"));

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

/// One log line. Extra fields are flattened next to the common ones.
#[derive(Clone, Debug, Serialize)]
pub struct LogEvent {
    pub schema_version: &'static str,
    pub project_slug: &'static str,
    pub timestamp: String,
    pub level: Level,
    pub event: String,
    pub component: &'static str,
    pub message: String,
    pub error_code: Option<String>,
    pub request_id: Option<String>,
    pub build_revision: &'static str,
    #[serde(flatten)]
    pub fields: BTreeMap<String, Value>,
}

impl LogEvent {
    /// An event stamped with the current time. `message` is filled in by the
    /// sink from its string table when left empty.
    pub fn new(level: Level, event: &str, component: &'static str) -> LogEvent {
        let timestamp = match calendar::now_epoch_secs() {
            Some(secs) => calendar::rfc3339_jst(secs),
            // A clock set before 1970 is stamped with the epoch itself.
            None => calendar::rfc3339_jst(0),
        };
        LogEvent {
            schema_version: SCHEMA_VERSION,
            project_slug: PROJECT_SLUG,
            timestamp,
            level,
            event: event.to_owned(),
            component,
            message: String::new(),
            error_code: None,
            request_id: None,
            build_revision: BUILD_REVISION,
            fields: BTreeMap::new(),
        }
    }

    /// Adds one extra field.
    pub fn with(mut self, key: &str, value: impl Into<Value>) -> LogEvent {
        self.fields.insert(key.to_owned(), value.into());
        self
    }

    /// Sets the error code as `<PREFIX>_<COMPONENT>_<REASON>`.
    pub fn with_error(mut self, reason: &str) -> LogEvent {
        self.error_code = Some(format!(
            "{ERROR_PREFIX}_{}_{reason}",
            self.component.to_ascii_uppercase()
        ));
        self
    }

    /// The one-line JSON form, without a trailing newline.
    pub fn to_json_line(&self) -> String {
        // The event holds only strings, numbers and maps of them, so
        // serialisation cannot fail; the event name is what is left to say
        // if it ever does.
        match serde_json::to_string(self) {
            Ok(line) => line,
            Err(_) => self.event.clone(),
        }
    }
}

/// Where events go.
pub trait LogSink {
    fn emit(&mut self, event: LogEvent);
}

/// Writes events to stdout, one JSON object per line, with the message taken
/// from the locale's string table.
pub struct StdoutSink {
    strings: Strings,
}

impl StdoutSink {
    pub fn new(strings: Strings) -> StdoutSink {
        StdoutSink { strings }
    }
}

impl LogSink for StdoutSink {
    fn emit(&mut self, mut event: LogEvent) {
        if event.message.is_empty() {
            event.message = self.strings.message_for(&event.event).to_owned();
        }
        let line = event.to_json_line();
        // A closed stdout leaves nowhere to report to.
        let _ = writeln!(std::io::stdout().lock(), "{line}");
    }
}

/// Keeps events in memory.
#[derive(Default)]
pub struct MemorySink {
    pub events: Vec<LogEvent>,
}

impl LogSink for MemorySink {
    fn emit(&mut self, event: LogEvent) {
        self.events.push(event);
    }
}

/// Drops events.
pub struct NullSink;

impl LogSink for NullSink {
    fn emit(&mut self, _event: LogEvent) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_json_line_carries_the_common_fields_and_extras() {
        let event =
            LogEvent::new(Level::Warn, "table.load.failed", "storage").with("table", "units");
        let parsed: Result<Value, _> = serde_json::from_str(&event.to_json_line());
        let Ok(parsed) = parsed else {
            panic!("the log line is not JSON");
        };
        assert_eq!(parsed["schema_version"], SCHEMA_VERSION);
        assert_eq!(parsed["level"], "warn");
        assert_eq!(parsed["table"], "units");
        assert!(parsed["error_code"].is_null());
    }

    #[test]
    fn the_error_code_joins_prefix_component_and_reason() {
        let event = LogEvent::new(Level::Error, "fx.fetch.failed", "scheduler").with_error("FETCH");
        assert_eq!(event.error_code.as_deref(), Some("HYM_SCHEDULER_FETCH"));
    }

    #[test]
    fn a_line_has_no_newline() {
        let event = LogEvent::new(Level::Info, "x", "core").with("k", "a\nb");
        assert!(!event.to_json_line().contains('\n'));
    }

    #[test]
    fn the_memory_sink_keeps_order() {
        let mut sink = MemorySink::default();
        sink.emit(LogEvent::new(Level::Info, "a", "core"));
        sink.emit(LogEvent::new(Level::Info, "b", "core"));
        let names: Vec<&str> = sink.events.iter().map(|e| e.event.as_str()).collect();
        assert_eq!(names, ["a", "b"]);
    }

    #[test]
    fn the_null_sink_accepts_events() {
        NullSink.emit(LogEvent::new(Level::Info, "a", "core"));
    }
}

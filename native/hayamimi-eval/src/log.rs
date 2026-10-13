//! Progress and failure events of a run, as `observability-log/v1` lines on
//! stderr. Results go to stdout and to files; this stream carries only what
//! happened and how much of it, never text, audio or paths.

use std::io::Write;

use hayamimi_relay::logging::{Level, LogEvent};
use serde_json::Value;

/// Identifies this build in every line.
pub const BUILD_REVISION: &str = concat!("hayamimi-eval-", env!("CARGO_PKG_VERSION"));

/// The one-line JSON form of an event.
pub fn line(
    level: Level,
    event: &str,
    component: &'static str,
    message: &str,
    fields: &[(&str, Value)],
) -> String {
    let mut built = LogEvent::new(level, event, component);
    built.build_revision = BUILD_REVISION;
    built.message = message.to_owned();
    for (key, value) in fields {
        built = built.with(key, value.clone());
    }
    built.to_json_line()
}

/// Writes one event to stderr.
pub fn emit(
    level: Level,
    event: &str,
    component: &'static str,
    message: &str,
    fields: &[(&str, Value)],
) {
    let text = line(level, event, component, message, fields);
    // A closed stderr leaves nowhere to report to.
    let _ = writeln!(std::io::stderr().lock(), "{text}");
}

/// An `info` event.
pub fn info(event: &str, component: &'static str, message: &str, fields: &[(&str, Value)]) {
    emit(Level::Info, event, component, message, fields);
}

/// A `warn` event.
pub fn warn(event: &str, component: &'static str, message: &str, fields: &[(&str, Value)]) {
    emit(Level::Warn, event, component, message, fields);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_carries_the_event_message_and_fields() -> Result<(), serde_json::Error> {
        let text = line(
            Level::Info,
            "eval.stream.started",
            "worker",
            "stream started",
            &[("repeat", Value::from(5))],
        );
        let parsed: Value = serde_json::from_str(&text)?;
        assert_eq!(parsed["event"], "eval.stream.started");
        assert_eq!(parsed["message"], "stream started");
        assert_eq!(parsed["repeat"], 5);
        assert_eq!(parsed["build_revision"], BUILD_REVISION);
        Ok(())
    }

    #[test]
    fn a_line_has_no_newline() {
        let text = line(Level::Warn, "a.b.c", "core", "x\ny", &[]);
        assert!(!text.contains('\n'));
    }

    #[test]
    fn emitting_does_not_panic() {
        info("eval.test.ok", "core", "ok", &[]);
        warn("eval.test.warn", "core", "warn", &[]);
    }
}

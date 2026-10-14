//! A follower of one recognizer's `GET /events` stream (server-sent events).
//!
//! The recognizer may start after the relay, stop, and start again. The
//! follower does not stop with it: when the stream ends or cannot be opened it
//! waits and connects again, and the relay goes on with the lines that arrive.

use std::convert::Infallible;
use std::io::{self, BufRead, BufReader, Read};
use std::time::Duration;

use crate::logging::{Level, LogEvent, LogSink};

/// A line of the stream longer than this is not a subtitle event; the
/// connection is dropped and opened again.
const MAX_LINE_BYTES: usize = 1 << 20;
/// Time allowed to open the connection, and then to receive the response head.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const HEAD_TIMEOUT: Duration = Duration::from_secs(5);

/// How long to wait before the next connection attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Backoff {
    pub initial: Duration,
    pub max: Duration,
}

impl Backoff {
    /// A quarter of a second, doubling up to five seconds.
    pub const STANDARD: Backoff = Backoff {
        initial: Duration::from_millis(250),
        max: Duration::from_secs(5),
    };

    /// The wait after a wait of `current` that did not help.
    pub fn next(&self, current: Duration) -> Duration {
        current.saturating_mul(2).min(self.max)
    }
}

/// Collects the `data:` lines of server-sent events and hands over each event
/// when its blank line arrives. Other fields and comments are ignored.
#[derive(Default)]
pub struct SseParser {
    data: String,
}

impl SseParser {
    /// Takes one line, without its line break. Returns the data of an event
    /// that this line completes.
    pub fn push_line(&mut self, line: &str) -> Option<String> {
        if line.is_empty() {
            return self.dispatch();
        }
        if line.starts_with(':') {
            return None;
        }
        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').map_or(value, |rest| rest)),
            // DEFAULT: the server-sent events format reads a line without a colon as a field name with an empty value.
            None => (line, ""),
        };
        if field == "data" {
            self.data.push_str(value);
            self.data.push('\n');
        }
        None
    }

    fn dispatch(&mut self) -> Option<String> {
        if self.data.is_empty() {
            return None;
        }
        let mut data = std::mem::take(&mut self.data);
        data.pop();
        Some(data)
    }
}

/// Why a connection ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
    /// The stream could not be opened.
    Connect,
    /// The server closed the stream.
    Closed,
    /// Reading failed.
    Read,
    /// A line was longer than [`MAX_LINE_BYTES`].
    TooLong,
}

impl Cause {
    pub fn as_str(self) -> &'static str {
        match self {
            Cause::Connect => "connect",
            Cause::Closed => "closed",
            Cause::Read => "read",
            Cause::TooLong => "too_long",
        }
    }
}

enum Next {
    Line,
    Closed,
    TooLong,
}

/// Reads one line, at most [`MAX_LINE_BYTES`] of it.
fn read_bounded_line(reader: &mut impl BufRead, line: &mut String) -> io::Result<Next> {
    let count = reader
        .by_ref()
        .take(MAX_LINE_BYTES as u64)
        .read_line(line)?;
    Ok(if count == 0 {
        Next::Closed
    } else if count >= MAX_LINE_BYTES && !line.ends_with('\n') {
        Next::TooLong
    } else {
        Next::Line
    })
}

/// What happens to each event of the stream.
pub type OnEvent<'a> = dyn FnMut(&str, &mut dyn LogSink) + 'a;

/// Reads the stream until it ends, handing each event to `on_event`.
fn pump(mut reader: impl BufRead, sink: &mut dyn LogSink, on_event: &mut OnEvent<'_>) -> Cause {
    let mut parser = SseParser::default();
    let mut line = String::new();
    loop {
        line.clear();
        match read_bounded_line(&mut reader, &mut line) {
            Ok(Next::Line) => {}
            Ok(Next::Closed) => return Cause::Closed,
            Ok(Next::TooLong) => return Cause::TooLong,
            Err(_) => return Cause::Read,
        }
        if let Some(data) = parser.push_line(line.trim_end_matches(['\r', '\n'])) {
            on_event(&data, sink);
        }
    }
}

fn agent() -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_recv_response(Some(HEAD_TIMEOUT))
        .build();
    ureq::Agent::new_with_config(config)
}

fn connect(agent: &ureq::Agent, url: &str) -> Result<impl BufRead, ureq::Error> {
    let response = agent
        .get(url)
        .header("Accept", "text/event-stream")
        .call()?;
    Ok(BufReader::new(response.into_body().into_reader()))
}

fn subscribe_event(name: &str, level: Level, endpoint: usize) -> LogEvent {
    LogEvent::new(level, name, "transport").with("endpoint", endpoint)
}

/// One connection: opens the stream, reads it to its end, and reports how it
/// ended together with whether it was ever open.
fn session(
    agent: &ureq::Agent,
    (endpoint, url): (usize, &str),
    sink: &mut dyn LogSink,
    on_event: &mut OnEvent<'_>,
) -> (bool, Cause) {
    let Ok(reader) = connect(agent, url) else {
        return (false, Cause::Connect);
    };
    sink.emit(subscribe_event(
        "events.subscribe.opened",
        Level::Info,
        endpoint,
    ));
    (true, pump(reader, sink, on_event))
}

/// Follows the stream at `url` for ever. `endpoint` is the number the log
/// events name this stream by. A connection that ended is logged once, then
/// opened again after a wait that grows while attempts fail and starts over
/// once a connection has been open.
pub fn follow(
    (endpoint, url): (usize, &str),
    backoff: Backoff,
    sink: &mut dyn LogSink,
    on_event: &mut OnEvent<'_>,
) -> Infallible {
    let agent = agent();
    let mut wait = backoff.initial;
    loop {
        let (opened, cause) = session(&agent, (endpoint, url), sink, on_event);
        if opened {
            wait = backoff.initial;
        }
        sink.emit(
            subscribe_event("events.subscribe.lost", Level::Warn, endpoint)
                .with("reason", cause.as_str())
                .with("retry_after_ms", u64::try_from(wait.as_millis()).ok())
                .with_error("LOST"),
        );
        std::thread::sleep(wait);
        wait = backoff.next(wait);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn events_of(text: &str) -> Vec<String> {
        let mut parser = SseParser::default();
        text.lines()
            .filter_map(|line| parser.push_line(line))
            .collect()
    }

    #[test]
    fn a_data_line_and_a_blank_line_make_an_event() {
        assert_eq!(events_of("data: {\"a\":1}\n\n"), ["{\"a\":1}"]);
    }

    #[test]
    fn a_space_after_the_colon_is_dropped_once() {
        assert_eq!(
            events_of("data:x\n\ndata: y\n\ndata:  z\n\n"),
            ["x", "y", " z"]
        );
    }

    #[test]
    fn several_data_lines_join_with_a_line_break() {
        assert_eq!(events_of("data: a\ndata: b\n\n"), ["a\nb"]);
    }

    #[test]
    fn comments_and_other_fields_are_not_data() {
        assert_eq!(
            events_of(": keep-alive\nevent: x\nid: 4\nretry: 100\ndata: a\n\n"),
            ["a"]
        );
    }

    #[test]
    fn a_blank_line_with_no_data_makes_no_event() {
        assert!(events_of("\n\n: c\n\n").is_empty());
    }

    #[test]
    fn an_event_not_yet_ended_is_not_handed_over() {
        assert!(events_of("data: a\n").is_empty());
    }

    #[test]
    fn a_line_without_a_colon_is_a_field_with_no_value() {
        assert_eq!(events_of("data\n\n"), [""]);
        assert!(events_of("other\n\n").is_empty());
    }

    #[test]
    fn the_wait_doubles_and_stops_at_the_maximum() {
        let backoff = Backoff {
            initial: Duration::from_millis(100),
            max: Duration::from_millis(350),
        };
        let first = backoff.next(backoff.initial);
        let second = backoff.next(first);
        let third = backoff.next(second);
        assert_eq!(
            [first, second, third],
            [
                Duration::from_millis(200),
                Duration::from_millis(350),
                Duration::from_millis(350)
            ]
        );
    }

    #[test]
    fn the_standard_wait_never_exceeds_five_seconds() {
        let mut wait = Backoff::STANDARD.initial;
        for _ in 0..40 {
            wait = Backoff::STANDARD.next(wait);
        }
        assert_eq!(wait, Duration::from_secs(5));
    }

    fn pumped(text: &[u8]) -> (Vec<String>, Cause) {
        let mut seen = Vec::new();
        let mut sink = crate::logging::NullSink;
        let cause = pump(
            Cursor::new(text.to_vec()),
            &mut sink,
            &mut |data: &str, _: &mut dyn LogSink| seen.push(data.to_owned()),
        );
        (seen, cause)
    }

    #[test]
    fn a_stream_that_ends_hands_over_its_events_and_reports_closed() {
        let (seen, cause) = pumped(b"data: a\r\n\r\ndata: b\r\n\r\n");
        assert_eq!(seen, ["a", "b"]);
        assert_eq!(cause, Cause::Closed);
    }

    #[test]
    fn an_event_cut_off_by_the_end_of_the_stream_is_not_handed_over() {
        let (seen, cause) = pumped(b"data: a\n\ndata: b\n");
        assert_eq!(seen, ["a"]);
        assert_eq!(cause, Cause::Closed);
    }

    #[test]
    fn a_line_without_a_break_that_is_too_long_ends_the_connection() {
        let mut long = b"data: ".to_vec();
        long.resize(MAX_LINE_BYTES + 10, b'x');
        let (seen, cause) = pumped(&long);
        assert!(seen.is_empty());
        assert_eq!(cause, Cause::TooLong);
    }

    #[test]
    fn a_line_of_exactly_the_longest_size_with_its_break_is_read() {
        let mut line = b"data: ".to_vec();
        line.resize(MAX_LINE_BYTES - 1, b'x');
        line.push(b'\n');
        line.push(b'\n');
        let (seen, cause) = pumped(&line);
        assert_eq!(seen.len(), 1);
        assert_eq!(cause, Cause::Closed);
    }

    #[test]
    fn bytes_that_are_not_utf8_end_the_connection_as_a_read_failure() {
        let (seen, cause) = pumped(&[b'd', b'a', b't', b'a', b':', 0xff, 0xfe, b'\n']);
        assert!(seen.is_empty());
        assert_eq!(cause, Cause::Read);
    }
}

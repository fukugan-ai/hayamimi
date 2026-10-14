//! One read of a server's `GET /events`, a stream of server-sent events, with
//! every event stamped on arrival.
//!
//! Both subtitle servers answer `/events` with `text/event-stream`: one event
//! is a `data: <json>` line and a blank line, whether the body is chunked (the
//! recognizer) or runs to the end of the connection (the relay). The request
//! goes through `ureq`, which decodes either, and the lines go through
//! [`SseParser`], the parser the relay reads recognizer streams with.
//!
//! The reader runs on a thread of its own, because a read cannot be woken
//! while the server is silent. [`Listener::finish`] stops keeping events at
//! once; the thread ends at the server's next line or at the end of the
//! stream, and holds nothing but its socket.

use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use hayamimi_relay::follow::SseParser;

use crate::error::EvalError;
use crate::ingest::{CONNECT_TIMEOUT, EVENTS_PATH, Heard, is_type, micros_since, wait_ready};
use crate::log;

/// The event a server keeps for a client that connects late.
const READY_EVENT: &str = "session_start";
const EVENT_STREAM: &str = "text/event-stream";

type Events = Vec<(i64, String)>;

/// What the reader has kept, and whether the listener is done with it.
#[derive(Default)]
struct Kept {
    events: Events,
    stopped: bool,
}

/// What the listener and its reader thread share.
#[derive(Default)]
struct Shared {
    kept: Mutex<Kept>,
    ready: AtomicBool,
    ended: AtomicBool,
}

/// The kept events. A thread that panicked while holding the lock left a
/// list of whole events, which is still valid.
fn locked(kept: &Mutex<Kept>) -> MutexGuard<'_, Kept> {
    match kept.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

impl Shared {
    /// Keeps one event. Returns false once the listener has stopped.
    fn keep(&self, at_us: i64, data: String) -> bool {
        if is_type(&data, READY_EVENT) {
            self.ready.store(true, Ordering::Relaxed);
        }
        let mut kept = locked(&self.kept);
        if kept.stopped {
            return false;
        }
        kept.events.push((at_us, data));
        true
    }

    /// Stops keeping events and hands over the ones kept.
    fn stop(&self) -> Events {
        let mut kept = locked(&self.kept);
        kept.stopped = true;
        std::mem::take(&mut kept.events)
    }
}

/// Reads `body` until it ends, or until the first event after the listener
/// stopped.
fn read_stream(body: impl BufRead, origin: Instant, shared: &Shared) {
    let mut parser = SseParser::default();
    for line in body.lines() {
        let Ok(line) = line else {
            break;
        };
        let Some(data) = parser.push_line(&line) else {
            continue;
        };
        if !shared.keep(micros_since(origin), data) {
            return;
        }
    }
    shared.ended.store(true, Ordering::Relaxed);
}

fn events_address(host: &str, port: u16) -> String {
    format!("{host}:{port}")
}

/// A request that failed before any event: a socket that did not open
/// (refused, or no answer within [`CONNECT_TIMEOUT`]) is a `Connect` error,
/// anything else (a status, a malformed head) an `Http` one.
fn request_error(address: String, error: ureq::Error) -> EvalError {
    match error {
        ureq::Error::Io(source) => EvalError::Connect { address, source },
        ureq::Error::Timeout(ureq::Timeout::Connect) => EvalError::Connect {
            address,
            source: std::io::ErrorKind::TimedOut.into(),
        },
        other => EvalError::Http {
            url: format!("http://{address}{EVENTS_PATH}"),
            message: other.to_string(),
        },
    }
}

/// Whether the answer's `Content-Type` is an event stream.
fn is_event_stream(response: &ureq::http::Response<ureq::Body>) -> bool {
    response
        .headers()
        .get("content-type")
        .is_some_and(|value| value.as_bytes().starts_with(EVENT_STREAM.as_bytes()))
}

/// Sends `GET /events` and returns the body of the answer.
fn request(host: &str, port: u16) -> Result<BufReader<ureq::BodyReader<'static>>, EvalError> {
    let address = events_address(host, port);
    let config = ureq::Agent::config_builder()
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_recv_response(Some(CONNECT_TIMEOUT))
        .build();
    let response = ureq::Agent::new_with_config(config)
        .get(format!("http://{address}{EVENTS_PATH}"))
        .header("Accept", EVENT_STREAM)
        .call()
        .map_err(|error| request_error(address.clone(), error))?;
    if !is_event_stream(&response) {
        return Err(EvalError::Protocol(format!(
            "{address}{EVENTS_PATH} did not answer {EVENT_STREAM}"
        )));
    }
    Ok(BufReader::new(response.into_body().into_reader()))
}

/// An open `/events` stream and the thread reading it.
pub struct Listener {
    shared: Arc<Shared>,
}

impl Listener {
    /// Opens `GET /events` of the server at `host:port` and starts reading.
    /// Times are measured from `origin`.
    pub fn open(host: &str, port: u16, origin: Instant) -> Result<Listener, EvalError> {
        let body = request(host, port)?;
        log::info(
            "eval.events.connected",
            "transport",
            "event stream opened",
            &[("path", EVENTS_PATH.into())],
        );
        let shared = Arc::new(Shared::default());
        let reading = Arc::clone(&shared);
        std::thread::spawn(move || read_stream(body, origin, &reading));
        Ok(Listener { shared })
    }

    /// Waits for the server's kept `session_start`.
    pub fn start(&mut self) -> Result<(), EvalError> {
        let shared = &self.shared;
        wait_ready(
            &shared.ready,
            || shared.ended.load(Ordering::Relaxed),
            "the server sent no session_start event on /events",
        )?;
        log::info("eval.events.ready", "transport", "server is ready", &[]);
        Ok(())
    }

    /// Waits `drain` for late events, stops, and returns everything heard.
    pub fn finish(self, drain: Duration) -> Result<Heard, EvalError> {
        std::thread::sleep(drain);
        let events = self.shared.stop();
        Ok(Heard {
            events,
            closed_by_server: self.shared.ended.load(Ordering::Relaxed),
        })
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.shared.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn read_all(text: &str) -> (Events, bool) {
        let shared = Shared::default();
        read_stream(Cursor::new(text.as_bytes()), Instant::now(), &shared);
        (shared.stop(), shared.ended.load(Ordering::Relaxed))
    }

    #[test]
    fn each_data_line_with_its_blank_line_is_one_event() {
        let (events, ended) = read_all("data: {\"a\":1}\n\ndata: {\"b\":2}\r\n\r\n");
        let texts: Vec<&str> = events.iter().map(|(_, text)| text.as_str()).collect();
        assert_eq!(texts, ["{\"a\":1}", "{\"b\":2}"]);
        assert!(ended);
    }

    #[test]
    fn an_event_cut_off_before_its_blank_line_is_not_kept() {
        let (events, ended) = read_all("data: {\"a\":1}\n");
        assert!(events.is_empty());
        assert!(ended);
    }

    #[test]
    fn comments_and_blank_lines_are_not_events() {
        let (events, _) = read_all(": keep-alive\n\n\n");
        assert!(events.is_empty());
    }

    #[test]
    fn the_kept_session_start_makes_the_stream_ready() {
        let shared = Shared::default();
        assert!(shared.keep(5, String::from(r#"{"type":"final"}"#)));
        assert!(!shared.ready.load(Ordering::Relaxed));
        assert!(shared.keep(6, String::from(r#"{"type":"session_start"}"#)));
        assert!(shared.ready.load(Ordering::Relaxed));
    }

    #[test]
    fn nothing_is_kept_after_the_listener_stopped() {
        let shared = Shared::default();
        assert!(shared.keep(1, String::from("a")));
        assert_eq!(shared.stop().len(), 1);
        assert!(!shared.keep(2, String::from("b")));
        assert!(shared.stop().is_empty());
    }

    #[test]
    fn a_reader_stops_at_the_first_event_after_the_stop() {
        let shared = Shared::default();
        shared.stop();
        read_stream(
            Cursor::new(&b"data: a\n\ndata: b\n\n"[..]),
            Instant::now(),
            &shared,
        );
        assert!(shared.stop().is_empty());
        assert!(!shared.ended.load(Ordering::Relaxed));
    }

    #[test]
    fn a_refused_socket_is_a_connect_error_and_a_status_is_not() {
        let refused = std::io::Error::from(std::io::ErrorKind::ConnectionRefused);
        let said = request_error(String::from("h:1"), ureq::Error::Io(refused));
        assert!(matches!(said, EvalError::Connect { .. }));
        let said = request_error(String::from("h:1"), ureq::Error::StatusCode(404));
        assert!(matches!(said, EvalError::Http { .. }));
        let silent = request_error(
            String::from("h:1"),
            ureq::Error::Timeout(ureq::Timeout::Connect),
        );
        assert!(matches!(silent, EvalError::Connect { .. }));
    }
}

//! RP-41: `hayamimi-eval stream` listens to a server's `/events` as
//! server-sent events (`text/event-stream`), the form both subtitle servers
//! answer in, and records the time each event arrives.
//!
//! The servers here are fakes that answer in the two framings the real ones
//! use: a chunked body (the recognizer) and a body that runs to the end of the
//! connection (the relay). A fake answers one connection, then closes.

use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use hayamimi_eval::error::EvalError;
use hayamimi_eval::ingest::listen;
use serde_json::Value;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const FINAL: &str = r#"{"type":"final","id":"mic-1","text":"hello"}"#;
const TRANSLATION: &str = r#"{"type":"translation","id":"mic-1","text":"hallo"}"#;
const SESSION_START: &str = r#"{"type":"session_start"}"#;
const LOCALHOST: &str = "127.0.0.1";
const REQUEST_LINE: &str = "GET /events HTTP/1.1";
/// How long a fake keeps the stream open after its last event.
const HELD: Duration = Duration::from_millis(500);
/// How long the listener waits for late events: less than `HELD`.
const DRAIN: Duration = Duration::from_millis(150);

/// How a fake frames the body of its answer.
#[derive(Clone, Copy, Debug)]
enum Framing {
    Chunked,
    UntilClose,
}

const FRAMINGS: [Framing; 2] = [Framing::Chunked, Framing::UntilClose];

/// What a fake server answers.
#[derive(Clone, Copy)]
struct Fake {
    status: &'static str,
    content_type: &'static str,
    events: &'static [&'static str],
    framing: Framing,
    held: Duration,
}

impl Fake {
    fn stream(events: &'static [&'static str], framing: Framing, held: Duration) -> Fake {
        Fake {
            status: "200 OK",
            content_type: "text/event-stream",
            events,
            framing,
            held,
        }
    }

    fn head(&self) -> String {
        let framing = match self.framing {
            Framing::Chunked => "Transfer-Encoding: chunked",
            Framing::UntilClose => "Connection: close",
        };
        format!(
            "HTTP/1.1 {}\r\nContent-Type: {}\r\n{framing}\r\n\r\n",
            self.status, self.content_type
        )
    }

    /// One event as it goes on the wire.
    fn piece(&self, event: &str) -> String {
        let line = format!("data: {event}\n\n");
        match self.framing {
            Framing::Chunked => format!("{:x}\r\n{line}\r\n", line.len()),
            Framing::UntilClose => line,
        }
    }
}

/// The request line of what the client sent, read up to the end of its head.
fn read_request_line(socket: &mut TcpStream) -> io::Result<String> {
    socket.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut head = Vec::new();
    let mut chunk = [0_u8; 512];
    while !head.windows(4).any(|window| window == b"\r\n\r\n") {
        let count = socket.read(&mut chunk)?;
        if count == 0 {
            return Err(io::Error::other("the client closed before its head ended"));
        }
        head.extend_from_slice(&chunk[..count]);
    }
    let text = String::from_utf8_lossy(&head);
    let line = text.lines().next();
    line.map(str::to_owned)
        .ok_or_else(|| io::Error::other("an empty request head"))
}

fn answer(mut socket: TcpStream, fake: Fake) -> io::Result<String> {
    let request_line = read_request_line(&mut socket)?;
    socket.write_all(fake.head().as_bytes())?;
    for event in fake.events {
        socket.write_all(fake.piece(event).as_bytes())?;
        socket.flush()?;
    }
    std::thread::sleep(fake.held);
    if matches!(fake.framing, Framing::Chunked) {
        socket.write_all(b"0\r\n\r\n")?;
    }
    Ok(request_line)
}

/// Starts a fake on a free port. The handle gives the request line it saw.
fn serve(fake: Fake) -> io::Result<(u16, JoinHandle<io::Result<String>>)> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let port = listener.local_addr()?.port();
    let handle = std::thread::spawn(move || {
        let (socket, _) = listener.accept()?;
        answer(socket, fake)
    });
    Ok((port, handle))
}

fn served(handle: JoinHandle<io::Result<String>>) -> Result<String, Box<dyn std::error::Error>> {
    Ok(handle.join().map_err(|_| "the fake server panicked")??)
}

fn type_of(text: &str) -> Result<String, Box<dyn std::error::Error>> {
    let event: Value = serde_json::from_str(text)?;
    match event.get("type").and_then(Value::as_str) {
        Some(kind) => Ok(kind.to_owned()),
        None => Err(format!("an event with no type: {text}").into()),
    }
}

fn micros(at: i64) -> Result<u128, Box<dyn std::error::Error>> {
    Ok(u128::try_from(at)?)
}

/// T-RP-41-001: a fake that sends one `final` and one `translation` as
/// server-sent events. The listener hears both, in order, each stamped with
/// a time between the origin and now.
#[test]
fn rp41_stream_listen_reads_events_over_sse() -> TestResult {
    for framing in FRAMINGS {
        let fake = Fake::stream(&[FINAL, TRANSLATION], framing, HELD);
        let (port, server) = serve(fake)?;
        let origin = Instant::now();
        let heard = listen(LOCALHOST, port, origin)?.finish(DRAIN)?;
        let elapsed = origin.elapsed().as_micros();
        assert_eq!(heard.events.len(), 2, "{framing:?}: {:?}", heard.events);
        let kinds = [type_of(&heard.events[0].1)?, type_of(&heard.events[1].1)?];
        assert_eq!(kinds, ["final", "translation"], "{framing:?}");
        let (first, second) = (micros(heard.events[0].0)?, micros(heard.events[1].0)?);
        assert!(first <= second && second <= elapsed, "{framing:?}");
        assert_eq!(served(server)?, REQUEST_LINE, "{framing:?}");
    }
    Ok(())
}

/// T-RP-41-002: `start` returns once the kept `session_start` has arrived,
/// and that event is among the ones heard.
#[test]
fn rp41_start_waits_for_the_kept_session_start() -> TestResult {
    for framing in FRAMINGS {
        let (port, server) = serve(Fake::stream(&[SESSION_START, FINAL], framing, HELD))?;
        let mut listener = listen(LOCALHOST, port, Instant::now())?;
        listener.start()?;
        let heard = listener.finish(DRAIN)?;
        let kinds: Vec<String> = heard
            .events
            .iter()
            .map(|(_, text)| type_of(text))
            .collect::<Result<_, _>>()?;
        assert_eq!(kinds, ["session_start", "final"], "{framing:?}");
        served(server)?;
    }
    Ok(())
}

/// T-RP-41-003: a stream that ends without a `session_start` is an error at
/// `start`, not a wait for the full ready timeout.
#[test]
fn rp41_start_fails_when_the_stream_ends_without_session_start() -> TestResult {
    let begun = Instant::now();
    let (port, server) = serve(Fake::stream(&[FINAL], Framing::UntilClose, Duration::ZERO))?;
    let mut listener = listen(LOCALHOST, port, begun)?;
    let said = listener.start().err();
    assert!(matches!(said, Some(EvalError::Protocol(_))), "{said:?}");
    assert!(begun.elapsed() < Duration::from_secs(20));
    served(server)?;
    Ok(())
}

/// T-RP-41-004: a server that closes the stream is reported as having closed
/// it; a server still holding it open is not.
#[test]
fn rp41_a_stream_the_server_closed_is_reported() -> TestResult {
    let short = Duration::from_millis(50);
    let (port, server) = serve(Fake::stream(&[FINAL], Framing::Chunked, short))?;
    let closed = listen(LOCALHOST, port, Instant::now())?.finish(Duration::from_millis(600))?;
    assert!(closed.closed_by_server);
    served(server)?;
    let (port, server) = serve(Fake::stream(&[FINAL], Framing::Chunked, HELD))?;
    let open = listen(LOCALHOST, port, Instant::now())?.finish(DRAIN)?;
    assert!(!open.closed_by_server);
    assert_eq!(open.events.len(), 1);
    served(server)?;
    Ok(())
}

/// T-RP-41-005: an answer that is not an event stream, or not a 200, is
/// refused when `listen` is called.
#[test]
fn rp41_listen_refuses_an_answer_that_is_not_an_event_stream() -> TestResult {
    let page = Fake {
        content_type: "text/html",
        ..Fake::stream(&[], Framing::UntilClose, Duration::ZERO)
    };
    let (port, server) = serve(page)?;
    let said = listen(LOCALHOST, port, Instant::now()).err();
    assert!(matches!(said, Some(EvalError::Protocol(_))), "{said:?}");
    served(server)?;
    let missing = Fake {
        status: "404 Not Found",
        ..Fake::stream(&[], Framing::UntilClose, Duration::ZERO)
    };
    let (port, server) = serve(missing)?;
    let said = listen(LOCALHOST, port, Instant::now()).err();
    assert!(matches!(said, Some(EvalError::Http { .. })), "{said:?}");
    served(server)?;
    Ok(())
}

/// T-RP-41-006: a port nothing listens on is a connect error.
#[test]
fn rp41_listen_to_a_closed_port_is_a_connect_error() -> TestResult {
    let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?
        .local_addr()?
        .port();
    let said = listen(LOCALHOST, port, Instant::now()).err();
    assert!(matches!(said, Some(EvalError::Connect { .. })), "{said:?}");
    Ok(())
}

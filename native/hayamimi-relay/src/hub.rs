//! The relay's own event stream: `GET /events` serves each `translation` as
//! one server-sent event, in the same wire form as the recognizers' streams
//! (`data: <json>` and a blank line).
//!
//! The relay only writes to its clients. A client that went away is found at
//! the next event it would have received, and dropped there.
//!
//! A `fallback` event stays in effect until a later one for the same stage
//! says it is over (`to` is null). A client that connects while one is in
//! effect receives it first, so that a screen opened late still shows it.
//!
//! A page of another origin reads the stream only when the hub's [`Guard`]
//! lets that origin in: the reply then says that origin in
//! `Access-Control-Allow-Origin`, and never `*`. A request the guard turns
//! away is answered 403 and is not subscribed.

use std::collections::BTreeSet;
use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use crate::logging::{Level, LogEvent, LogSink, SharedSink};
use crate::origin::{self, Guard, Refusal, Verdict};

/// Ports tried after the requested one when it is taken.
pub const NEXT_PORTS: u16 = 10;
/// A request head longer than this is not a request for `/events`.
const MAX_HEAD_BYTES: usize = 8192;
/// Time a client has to send its request head.
const HEAD_TIMEOUT: Duration = Duration::from_secs(5);
/// The most refused requests said in the log, one for each distinct one. A
/// page that goes on asking is counted by the refusals it gets, not said again.
const MAX_SAID: usize = 32;

const NOT_FOUND: &[u8] =
    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
/// The refusal carries no `Access-Control-Allow-Origin`, so the page that was
/// turned away cannot read even this.
const FORBIDDEN: &[u8] =
    b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

/// The head of the `/events` reply. `echo` is the origin of the page that
/// asked, which the reply lets read the stream.
fn events_head(echo: Option<&str>) -> Vec<u8> {
    let mut head = String::from(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nVary: Origin\r\n",
    );
    if let Some(origin) = echo {
        head.push_str("Access-Control-Allow-Origin: ");
        head.push_str(origin);
        head.push_str("\r\n");
    }
    head.push_str("Connection: close\r\n\r\n");
    head.into_bytes()
}

type Message = Arc<str>;

/// The clients, and the `fallback` events in effect.
#[derive(Default)]
struct State {
    clients: Vec<Sender<Message>>,
    /// By stage, in the order they came.
    fallbacks: Vec<(String, Message)>,
}

/// Where the requests turned away are said, one line for each distinct one.
#[derive(Clone)]
struct Refusals {
    sink: SharedSink,
    said: Arc<Mutex<BTreeSet<String>>>,
}

impl Refusals {
    /// Says that a request was turned away for `reason`, naming what it
    /// showed in `seen`, unless that has been said or too much has been.
    fn say(&self, reason: Refusal, seen: &str) {
        let key = format!("{}:{seen}", reason.as_str());
        {
            // A thread that panicked while holding the set left a set that is
            // still whole.
            let mut said = match self.said.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            if said.len() >= MAX_SAID || !said.insert(key) {
                return;
            }
        }
        let event = LogEvent::new(Level::Warn, "relay.events.refused", "transport")
            .with("reason", reason.as_str())
            .with("seen", seen);
        self.sink.clone().emit(event);
    }
}

/// Where translations are published.
#[derive(Clone, Default)]
pub struct Hub {
    state: Arc<Mutex<State>>,
    /// The pages of other origins let in to read `/events`.
    guard: Guard,
    /// Where a request turned away is said, when it is said at all.
    refusals: Option<Refusals>,
}

/// The state of the hub. A thread that panicked while holding the lock left
/// whole senders and whole messages, which are still valid.
fn locked(state: &Mutex<State>) -> MutexGuard<'_, State> {
    match state.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

impl Hub {
    /// A hub that lets in the pages of this machine, and says nothing of the
    /// requests it turns away.
    pub fn new() -> Hub {
        Hub::default()
    }

    /// A hub that lets in the pages `guard` lets in, and says on `sink` each
    /// distinct request it turns away.
    pub fn guarded(guard: Guard, sink: SharedSink) -> Hub {
        Hub {
            guard,
            refusals: Some(Refusals {
                sink,
                said: Arc::default(),
            }),
            ..Hub::default()
        }
    }

    /// Says a request was turned away, when this hub says such things.
    fn say_refused(&self, head: &str, reason: Refusal) {
        if let Some(refusals) = &self.refusals {
            refusals.say(reason, &origin::seen(head, reason));
        }
    }

    /// Sends one event, a JSON text, to every connected client. A client
    /// that has gone is dropped. With nobody connected there is nobody to
    /// tell, which is not an error.
    pub fn publish(&self, json: &str) {
        let message: Message = Arc::from(json);
        locked(&self.state)
            .clients
            .retain(|client| client.send(Arc::clone(&message)).is_ok());
    }

    /// Sends a `fallback` event, a JSON text, and keeps it in effect for the
    /// clients that connect later when `in_effect` is true. A `false` ends
    /// the stage's earlier fallback.
    pub fn publish_fallback(&self, stage: &str, in_effect: bool, json: &str) {
        let message: Message = Arc::from(json);
        let mut state = locked(&self.state);
        state.fallbacks.retain(|(kept, _)| kept != stage);
        if in_effect {
            state
                .fallbacks
                .push((stage.to_owned(), Arc::clone(&message)));
        }
        state
            .clients
            .retain(|client| client.send(Arc::clone(&message)).is_ok());
    }

    /// A new client's channel, and the fallbacks in effect at the moment it
    /// was added. Both are taken under one lock, so that none is missed or
    /// sent twice.
    pub(crate) fn subscribe(&self) -> (Receiver<Message>, Vec<Message>) {
        let (sender, receiver) = mpsc::channel();
        let mut state = locked(&self.state);
        let in_effect = state
            .fallbacks
            .iter()
            .map(|(_, message)| Arc::clone(message))
            .collect();
        state.clients.push(sender);
        (receiver, in_effect)
    }

    /// How many clients are connected.
    pub fn clients(&self) -> usize {
        locked(&self.state).clients.len()
    }
}

/// Binds `ip:requested`, or the first free one of the [`NEXT_PORTS`] ports
/// after it. Port 0 asks the system for any port and has no next ones. The
/// error is the last one when none could be bound.
pub fn bind_on(ip: IpAddr, requested: u16) -> io::Result<TcpListener> {
    let last = last_port(requested);
    let mut bound = TcpListener::bind((ip, requested));
    for port in (requested..=last).skip(1) {
        if bound.is_ok() {
            break;
        }
        bound = TcpListener::bind((ip, port));
    }
    bound
}

/// The last port [`bind_on`] tries for `requested`: [`NEXT_PORTS`] ports
/// further, or the top port when fewer are left. Port 0 has none.
fn last_port(requested: u16) -> u16 {
    if requested == 0 {
        0
    } else {
        requested.saturating_add(NEXT_PORTS)
    }
}

/// [`bind_on`] the loopback address: only this machine can connect.
pub fn bind(requested: u16) -> io::Result<TcpListener> {
    bind_on(IpAddr::V4(Ipv4Addr::LOCALHOST), requested)
}

/// The path of the request whose head is in `raw`, when it is a `GET`.
fn get_path(raw: &[u8]) -> Option<&str> {
    let first = raw.split(|byte| *byte == b'\n').next()?;
    let line = std::str::from_utf8(first).ok()?.trim_end();
    let mut parts = line.split(' ');
    if parts.next()? != "GET" {
        return None;
    }
    parts.next()
}

fn head_is_complete(raw: &[u8]) -> bool {
    raw.windows(4).any(|window| window == b"\r\n\r\n")
}

/// Reads the head of the client's request.
fn read_head(stream: &mut TcpStream) -> Option<Vec<u8>> {
    stream.set_read_timeout(Some(HEAD_TIMEOUT)).ok()?;
    let mut raw = Vec::new();
    let mut chunk = [0_u8; 1024];
    while !head_is_complete(&raw) {
        let count = stream.read(&mut chunk).ok().filter(|count| *count > 0)?;
        raw.extend_from_slice(&chunk[..count]);
        if raw.len() > MAX_HEAD_BYTES {
            return None;
        }
    }
    Some(raw)
}

/// What the head asks of the hub: the origin to echo, or the 403 that turns
/// the client away. A head that is not text is no browser's and is not found.
enum Asked {
    Echo(Option<String>),
    NotFound,
    Forbidden,
}

fn ask_of(raw: &[u8], hub: &Hub) -> Asked {
    let Ok(head) = std::str::from_utf8(raw) else {
        return Asked::NotFound;
    };
    match hub.guard.judge_head(head) {
        Verdict::Serve { echo } if get_path(raw) == Some("/events") => Asked::Echo(echo),
        Verdict::Serve { .. } => Asked::NotFound,
        Verdict::Refuse(reason) => {
            hub.say_refused(head, reason);
            Asked::Forbidden
        }
    }
}

/// Serves one client until it goes away.
fn answer(mut stream: TcpStream, hub: &Hub) {
    let Some(raw) = read_head(&mut stream) else {
        return;
    };
    let echo = match ask_of(&raw, hub) {
        Asked::Echo(echo) => echo,
        // A client that hung up has nothing left to be told.
        Asked::NotFound => {
            let _ = stream.write_all(NOT_FOUND);
            return;
        }
        Asked::Forbidden => {
            let _ = stream.write_all(FORBIDDEN);
            return;
        }
    };
    // Subscribed before the head goes out, so that a client that has the head
    // misses nothing published after it.
    let (events, in_effect) = hub.subscribe();
    if stream.write_all(&events_head(echo.as_deref())).is_err() {
        return;
    }
    for message in in_effect.into_iter().chain(events) {
        let sent = write!(stream, "data: {message}\n\n").and_then(|()| stream.flush());
        if sent.is_err() {
            return;
        }
    }
}

/// Serves `hub` on `listener` from threads of their own; they end with the
/// process. Returns the address the server is on.
pub fn serve(listener: TcpListener, hub: Hub) -> io::Result<SocketAddr> {
    serve_while(listener, hub, Arc::new(AtomicBool::new(true)))
}

/// Like [`serve`], but the accepting thread ends, and the port is given up, at
/// the first connection after `running` has turned false. Clients already
/// connected are served until they go away.
pub fn serve_while(
    listener: TcpListener,
    hub: Hub,
    running: Arc<AtomicBool>,
) -> io::Result<SocketAddr> {
    let address = listener.local_addr()?;
    std::thread::Builder::new()
        .name(String::from("relay-events"))
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                if !running.load(Ordering::SeqCst) {
                    break;
                }
                let hub = hub.clone();
                std::thread::spawn(move || answer(stream, &hub));
            }
        })?;
    Ok(address)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::origin::Origin;

    fn request(address: SocketAddr, path: &str) -> io::Result<TcpStream> {
        let mut stream = TcpStream::connect(address)?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        write!(stream, "GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n")?;
        Ok(stream)
    }

    fn read_until(stream: &mut TcpStream, needle: &str) -> io::Result<String> {
        let mut seen = String::new();
        let mut buffer = [0_u8; 1024];
        while !seen.contains(needle) {
            let count = stream.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            seen.push_str(&String::from_utf8_lossy(&buffer[..count]));
        }
        Ok(seen)
    }

    /// Waits until the server has added the one client of the test.
    fn wait_until_subscribed(hub: &Hub) -> io::Result<()> {
        for _ in 0..250 {
            if hub.clients() == 1 {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        Err(io::Error::other("the client was never subscribed"))
    }

    #[test]
    fn a_connected_client_receives_a_published_event_as_a_data_line() -> io::Result<()> {
        let hub = Hub::new();
        let address = serve(bind(0)?, hub.clone())?;
        let mut stream = request(address, "/events")?;
        let head = read_until(&mut stream, "\r\n\r\n")?;
        assert!(head.starts_with("HTTP/1.1 200"), "{head}");
        assert!(
            head.to_lowercase()
                .contains("content-type: text/event-stream"),
            "{head}"
        );
        wait_until_subscribed(&hub)?;
        hub.publish(r#"{"type":"translation","id":"mic-1"}"#);
        // The frame may arrive in pieces; read to its blank-line end.
        let body = read_until(&mut stream, "\n\n")?;
        assert!(
            body.contains("data: {\"type\":\"translation\",\"id\":\"mic-1\"}\n\n"),
            "{body}"
        );
        Ok(())
    }

    #[test]
    fn every_connected_client_receives_every_event_in_order() -> io::Result<()> {
        let hub = Hub::new();
        let address = serve(bind(0)?, hub.clone())?;
        let mut first = request(address, "/events")?;
        let mut second = request(address, "/events")?;
        read_until(&mut first, "\r\n\r\n")?;
        read_until(&mut second, "\r\n\r\n")?;
        assert_eq!(hub.clients(), 2);
        hub.publish("one");
        hub.publish("two");
        for stream in [&mut first, &mut second] {
            let body = read_until(stream, "two")?;
            let (one, two) = (body.find("data: one"), body.find("data: two"));
            assert!(one.is_some() && two.is_some() && one < two, "{body}");
        }
        Ok(())
    }

    #[test]
    fn an_unknown_path_is_not_found_and_is_not_subscribed() -> io::Result<()> {
        let hub = Hub::new();
        let address = serve(bind(0)?, hub.clone())?;
        let mut stream = request(address, "/nothing")?;
        let head = read_until(&mut stream, "\r\n\r\n")?;
        assert!(head.starts_with("HTTP/1.1 404"), "{head}");
        assert_eq!(hub.clients(), 0);
        Ok(())
    }

    /// A request for `/events` with the header lines given, each ended by CRLF.
    fn request_with(address: SocketAddr, headers: &str) -> io::Result<TcpStream> {
        let mut stream = TcpStream::connect(address)?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        write!(stream, "GET /events HTTP/1.1\r\n{headers}\r\n")?;
        Ok(stream)
    }

    /// The head the hub answers with, in lower case.
    fn head_of(stream: &mut TcpStream) -> io::Result<String> {
        Ok(read_until(stream, "\r\n\r\n")?.to_lowercase())
    }

    fn named(text: &str) -> io::Result<Origin> {
        Origin::parse(text).ok_or_else(|| io::Error::other(format!("{text} is not an origin")))
    }

    #[test]
    fn the_events_head_says_the_origin_it_was_given_and_that_the_reply_varies() {
        let said = events_head(Some("http://a.test:1"));
        let said = String::from_utf8_lossy(&said).to_lowercase();
        assert!(
            said.contains("access-control-allow-origin: http://a.test:1\r\n"),
            "{said}"
        );
        assert!(said.contains("vary: origin\r\n"), "{said}");
        let silent = events_head(None);
        let silent = String::from_utf8_lossy(&silent).to_lowercase();
        assert!(!silent.contains("access-control-allow-origin"), "{silent}");
        assert!(silent.contains("vary: origin\r\n"), "{silent}");
        assert!(silent.ends_with("\r\n\r\n"), "{silent}");
    }

    #[test]
    fn a_page_of_this_machine_is_told_its_own_origin_and_never_a_star() -> io::Result<()> {
        let address = serve(bind(0)?, Hub::new())?;
        let mut stream = request_with(
            address,
            "Host: 127.0.0.1:8835\r\nOrigin: http://127.0.0.1:8833\r\n",
        )?;
        let head = head_of(&mut stream)?;
        assert!(head.starts_with("http/1.1 200"), "{head}");
        assert!(
            head.contains("access-control-allow-origin: http://127.0.0.1:8833\r\n"),
            "{head}"
        );
        assert!(head.contains("vary: origin\r\n"), "{head}");
        assert!(!head.contains("access-control-allow-origin: *"), "{head}");
        Ok(())
    }

    #[test]
    fn a_request_naming_no_origin_is_served_without_a_cors_header() -> io::Result<()> {
        let address = serve(bind(0)?, Hub::new())?;
        let mut stream = request_with(address, "Host: 127.0.0.1:8835\r\n")?;
        let head = head_of(&mut stream)?;
        assert!(head.starts_with("http/1.1 200"), "{head}");
        assert!(!head.contains("access-control-allow-origin"), "{head}");
        assert!(head.contains("vary: origin\r\n"), "{head}");
        Ok(())
    }

    #[test]
    fn a_page_of_another_origin_is_turned_away_and_not_subscribed() -> io::Result<()> {
        let hub = Hub::new();
        let address = serve(bind(0)?, hub.clone())?;
        let mut stream = request_with(
            address,
            "Host: 127.0.0.1:8835\r\nOrigin: https://evil.test\r\n",
        )?;
        let head = head_of(&mut stream)?;
        assert!(head.starts_with("http/1.1 403"), "{head}");
        assert!(!head.contains("access-control-allow-origin"), "{head}");
        assert_eq!(hub.clients(), 0);
        hub.publish("after");
        let mut rest = String::new();
        stream.read_to_string(&mut rest)?;
        assert_eq!(rest, "", "nothing follows the refusal");
        Ok(())
    }

    #[test]
    fn the_origin_null_is_turned_away() -> io::Result<()> {
        let address = serve(bind(0)?, Hub::new())?;
        let mut stream = request_with(address, "Host: 127.0.0.1\r\nOrigin: null\r\n")?;
        assert!(head_of(&mut stream)?.starts_with("http/1.1 403"));
        Ok(())
    }

    #[test]
    fn a_host_that_is_a_rebound_name_is_turned_away_whatever_it_asks_for() -> io::Result<()> {
        let hub = Hub::new();
        let address = serve(bind(0)?, hub.clone())?;
        let mut stream = request_with(address, "Host: rebound.test:8835\r\n")?;
        let head = head_of(&mut stream)?;
        assert!(head.starts_with("http/1.1 403"), "{head}");
        assert_eq!(hub.clients(), 0);
        Ok(())
    }

    #[test]
    fn a_named_origin_is_told_its_own_origin() -> io::Result<()> {
        let guard = Guard::new(vec![named("http://desk:8833")?]);
        let hub = Hub::guarded(guard, SharedSink::new(crate::logging::NullSink));
        let address = serve(bind(0)?, hub)?;
        let mut stream = request_with(address, "Host: desk:8835\r\nOrigin: http://desk:8833\r\n")?;
        let head = head_of(&mut stream)?;
        assert!(head.starts_with("http/1.1 200"), "{head}");
        assert!(
            head.contains("access-control-allow-origin: http://desk:8833\r\n"),
            "{head}"
        );
        let mut other = request_with(
            address,
            "Host: desk:8835\r\nOrigin: http://elsewhere:8833\r\n",
        )?;
        assert!(head_of(&mut other)?.starts_with("http/1.1 403"));
        Ok(())
    }

    /// Keeps the events a hub says, for a test to read.
    struct Collect(Arc<Mutex<Vec<LogEvent>>>);

    impl LogSink for Collect {
        fn emit(&mut self, event: LogEvent) {
            match self.0.lock() {
                Ok(mut held) => held.push(event),
                Err(poisoned) => poisoned.into_inner().push(event),
            }
        }
    }

    fn said_by(events: &Mutex<Vec<LogEvent>>) -> io::Result<Vec<LogEvent>> {
        events
            .lock()
            .map(|held| held.clone())
            .map_err(|_| io::Error::other("the event list was poisoned"))
    }

    fn refused_by_origin(address: SocketAddr, origin: &str) -> io::Result<()> {
        let mut stream =
            request_with(address, &format!("Host: 127.0.0.1\r\nOrigin: {origin}\r\n"))?;
        let head = head_of(&mut stream)?;
        assert!(head.starts_with("http/1.1 403"), "{head}");
        Ok(())
    }

    #[test]
    fn a_refusal_is_said_once_for_each_distinct_request() -> io::Result<()> {
        let events = Arc::new(Mutex::new(Vec::new()));
        let hub = Hub::guarded(
            Guard::default(),
            SharedSink::new(Collect(Arc::clone(&events))),
        );
        let address = serve(bind(0)?, hub)?;
        refused_by_origin(address, "https://evil.test")?;
        refused_by_origin(address, "https://evil.test")?;
        refused_by_origin(address, "https://other.test")?;
        let said = said_by(&events)?;
        assert_eq!(said.len(), 2, "{said:?}");
        let first = &said[0];
        assert_eq!(first.event, "relay.events.refused");
        assert_eq!(first.level, Level::Warn);
        assert_eq!(
            first.fields.get("reason"),
            Some(&serde_json::json!("origin"))
        );
        assert_eq!(
            first.fields.get("seen"),
            Some(&serde_json::json!("https://evil.test"))
        );
        Ok(())
    }

    #[test]
    fn a_page_that_goes_on_asking_does_not_fill_the_log() -> io::Result<()> {
        let events = Arc::new(Mutex::new(Vec::new()));
        let hub = Hub::guarded(
            Guard::default(),
            SharedSink::new(Collect(Arc::clone(&events))),
        );
        let address = serve(bind(0)?, hub)?;
        for number in 0..MAX_SAID + 8 {
            refused_by_origin(address, &format!("https://e{number}.test"))?;
        }
        assert_eq!(said_by(&events)?.len(), MAX_SAID);
        Ok(())
    }

    #[test]
    fn a_hub_made_without_a_log_turns_away_without_saying_so() -> io::Result<()> {
        let address = serve(bind(0)?, Hub::new())?;
        refused_by_origin(address, "https://evil.test")
    }

    #[test]
    fn a_client_that_went_away_is_dropped_at_the_next_event() -> io::Result<()> {
        let hub = Hub::new();
        let address = serve(bind(0)?, hub.clone())?;
        let mut stream = request(address, "/events")?;
        read_until(&mut stream, "\r\n\r\n")?;
        drop(stream);
        for _ in 0..50 {
            hub.publish("gone");
            if hub.clients() == 0 {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        Err(io::Error::other("the client was never dropped"))
    }

    #[test]
    fn publishing_with_nobody_connected_is_fine() {
        Hub::new().publish("x");
    }

    #[test]
    fn only_a_get_has_a_path() {
        assert_eq!(
            get_path(b"GET /events HTTP/1.1\r\nHost: x\r\n\r\n"),
            Some("/events")
        );
        assert_eq!(get_path(b"POST /events HTTP/1.1\r\n\r\n"), None);
        assert_eq!(get_path(b""), None);
        assert_eq!(get_path(b"GET"), None);
    }

    const SINK_DOWN: &str =
        r#"{"type":"fallback","stage":"sink","reason":"unreachable","to":"local"}"#;
    const SINK_UP: &str = r#"{"type":"fallback","stage":"sink","to":null}"#;

    #[test]
    fn a_client_that_connects_late_receives_the_fallback_in_effect_first() -> io::Result<()> {
        let hub = Hub::new();
        let address = serve(bind(0)?, hub.clone())?;
        hub.publish_fallback("sink", true, SINK_DOWN);
        let mut stream = request(address, "/events")?;
        wait_until_subscribed(&hub)?;
        hub.publish("after");
        let body = read_until(&mut stream, "after")?;
        let (down, after) = (body.find("\"to\":\"local\""), body.find("data: after"));
        assert!(down.is_some() && after.is_some() && down < after, "{body}");
        Ok(())
    }

    #[test]
    fn a_fallback_that_ended_is_not_replayed() -> io::Result<()> {
        let hub = Hub::new();
        let address = serve(bind(0)?, hub.clone())?;
        hub.publish_fallback("sink", true, SINK_DOWN);
        hub.publish_fallback("sink", false, SINK_UP);
        let mut stream = request(address, "/events")?;
        wait_until_subscribed(&hub)?;
        hub.publish("after");
        let body = read_until(&mut stream, "after")?;
        assert!(!body.contains("fallback"), "{body}");
        Ok(())
    }

    #[test]
    fn a_fallback_of_a_stage_replaces_the_earlier_one_of_that_stage() -> io::Result<()> {
        let hub = Hub::new();
        let address = serve(bind(0)?, hub.clone())?;
        hub.publish_fallback("sink", true, SINK_DOWN);
        hub.publish_fallback("sink", true, SINK_DOWN);
        hub.publish_fallback(
            "other",
            true,
            r#"{"type":"fallback","stage":"other","to":"cpu"}"#,
        );
        let mut stream = request(address, "/events")?;
        wait_until_subscribed(&hub)?;
        hub.publish("after");
        let body = read_until(&mut stream, "after")?;
        assert_eq!(body.matches("\"stage\":\"sink\"").count(), 1, "{body}");
        assert_eq!(body.matches("\"stage\":\"other\"").count(), 1, "{body}");
        Ok(())
    }

    #[test]
    fn a_connected_client_receives_a_fallback_once() -> io::Result<()> {
        let hub = Hub::new();
        let address = serve(bind(0)?, hub.clone())?;
        let mut stream = request(address, "/events")?;
        read_until(&mut stream, "\r\n\r\n")?;
        hub.publish_fallback("sink", true, SINK_DOWN);
        hub.publish("after");
        let body = read_until(&mut stream, "after")?;
        assert_eq!(body.matches("\"to\":\"local\"").count(), 1, "{body}");
        Ok(())
    }

    /// A listener on a port that is not the top one, so a next port exists.
    fn bound_below_the_top() -> io::Result<TcpListener> {
        for _ in 0..50 {
            let held = bind(0)?;
            if held.local_addr()?.port() < u16::MAX {
                return Ok(held);
            }
        }
        Err(io::Error::other("every port tried was the top one"))
    }

    #[test]
    fn a_taken_port_moves_to_one_of_the_next_ten() -> io::Result<()> {
        let held = bound_below_the_top()?;
        let taken = held.local_addr()?.port();
        let moved = bind(taken)?.local_addr()?.port();
        assert!(
            moved > taken && moved <= last_port(taken),
            "{taken} -> {moved}"
        );
        Ok(())
    }

    #[test]
    fn the_next_ports_stop_at_the_top_port() {
        assert_eq!(last_port(100), 100 + NEXT_PORTS);
        assert_eq!(last_port(u16::MAX - NEXT_PORTS), u16::MAX);
        assert_eq!(last_port(u16::MAX - 2), u16::MAX);
        assert_eq!(last_port(u16::MAX), u16::MAX);
        assert_eq!(last_port(0), 0);
    }

    #[test]
    fn port_zero_asks_for_any_port() -> io::Result<()> {
        assert_ne!(bind(0)?.local_addr()?.port(), 0);
        Ok(())
    }
}

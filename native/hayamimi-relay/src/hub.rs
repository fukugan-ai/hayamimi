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

use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

/// Ports tried after the requested one when it is taken.
pub const NEXT_PORTS: u16 = 10;
/// A request head longer than this is not a request for `/events`.
const MAX_HEAD_BYTES: usize = 8192;
/// Time a client has to send its request head.
const HEAD_TIMEOUT: Duration = Duration::from_secs(5);

const EVENTS_HEAD: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n";
const NOT_FOUND: &[u8] =
    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

type Message = Arc<str>;

/// The clients, and the `fallback` events in effect.
#[derive(Default)]
struct State {
    clients: Vec<Sender<Message>>,
    /// By stage, in the order they came.
    fallbacks: Vec<(String, Message)>,
}

/// Where translations are published.
#[derive(Clone, Default)]
pub struct Hub {
    state: Arc<Mutex<State>>,
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
    pub fn new() -> Hub {
        Hub::default()
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

/// Binds `127.0.0.1:requested`, or the first free one of the [`NEXT_PORTS`]
/// ports after it. Port 0 asks the system for any port and has no next ones.
/// The error is the last one when none could be bound.
pub fn bind(requested: u16) -> io::Result<TcpListener> {
    let last = if requested == 0 {
        0
    } else {
        requested.saturating_add(NEXT_PORTS)
    };
    let mut bound = TcpListener::bind((Ipv4Addr::LOCALHOST, requested));
    for port in (requested..=last).skip(1) {
        if bound.is_ok() {
            break;
        }
        bound = TcpListener::bind((Ipv4Addr::LOCALHOST, port));
    }
    bound
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

/// Serves one client until it goes away.
fn answer(mut stream: TcpStream, hub: &Hub) {
    let Some(raw) = read_head(&mut stream) else {
        return;
    };
    if get_path(&raw) != Some("/events") {
        // A client that hung up has nothing left to be told.
        let _ = stream.write_all(NOT_FOUND);
        return;
    }
    // Subscribed before the head goes out, so that a client that has the head
    // misses nothing published after it.
    let (events, in_effect) = hub.subscribe();
    if stream.write_all(EVENTS_HEAD).is_err() {
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
        hub.publish(r#"{"type":"translation","id":"mic-1"}"#);
        let body = read_until(&mut stream, "mic-1")?;
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

    #[test]
    fn a_taken_port_moves_to_one_of_the_next_ten() -> io::Result<()> {
        let held = bind(0)?;
        let taken = held.local_addr()?.port();
        let moved = bind(taken)?.local_addr()?.port();
        assert!(
            moved > taken && moved <= taken + NEXT_PORTS,
            "{taken} -> {moved}"
        );
        Ok(())
    }

    #[test]
    fn port_zero_asks_for_any_port() -> io::Result<()> {
        assert_ne!(bind(0)?.local_addr()?.port(), 0);
        Ok(())
    }
}

//! The relay's own event stream: `GET /events` serves each `translation` as
//! one server-sent event, in the same wire form as the recognizers' streams
//! (`data: <json>` and a blank line).
//!
//! The relay only writes to its clients. A client that went away is found at
//! the next event it would have received, and dropped there.

use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
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

/// Where translations are published.
#[derive(Clone, Default)]
pub struct Hub {
    clients: Arc<Mutex<Vec<Sender<Message>>>>,
}

/// The list of clients. A thread that panicked while holding the lock left a
/// list of whole senders, which is still valid.
fn locked(clients: &Mutex<Vec<Sender<Message>>>) -> MutexGuard<'_, Vec<Sender<Message>>> {
    match clients.lock() {
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
        locked(&self.clients).retain(|client| client.send(Arc::clone(&message)).is_ok());
    }

    fn subscribe(&self) -> Receiver<Message> {
        let (sender, receiver) = mpsc::channel();
        locked(&self.clients).push(sender);
        receiver
    }

    /// How many clients are connected.
    pub fn clients(&self) -> usize {
        locked(&self.clients).len()
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
    let events = hub.subscribe();
    if stream.write_all(EVENTS_HEAD).is_err() {
        return;
    }
    for message in events {
        let sent = write!(stream, "data: {message}\n\n").and_then(|()| stream.flush());
        if sent.is_err() {
            return;
        }
    }
}

/// Serves `hub` on `listener` from threads of their own; they end with the
/// process. Returns the address the server is on.
pub fn serve(listener: TcpListener, hub: Hub) -> io::Result<SocketAddr> {
    let address = listener.local_addr()?;
    std::thread::Builder::new()
        .name(String::from("relay-events"))
        .spawn(move || {
            for stream in listener.incoming().flatten() {
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

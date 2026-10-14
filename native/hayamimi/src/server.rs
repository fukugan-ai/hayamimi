//! The subtitle event server: `GET /events`, a server-sent event stream.
//!
//! The pipeline runs on the main thread and publishes events to a [`Hub`];
//! the server runs on a thread of its own and sends each event to every
//! client that is connected. Each event is one JSON object in a `data:` line,
//! the form the existing overlay pages read.

use std::convert::Infallible;
use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::sync::{Arc, Mutex, MutexGuard};

use axum::Router;
use axum::extract::State;
use axum::http::header::ACCESS_CONTROL_ALLOW_ORIGIN;
use axum::response::IntoResponse;
use axum::response::sse::{Event, Sse};
use axum::routing::get;
use serde_json::Value;
use tokio::sync::broadcast;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;

/// Events a slow client may fall behind by before it loses the oldest ones.
const BACKLOG: usize = 1024;

/// Ports tried after the requested one when it is taken.
pub const NEXT_PORTS: u16 = 10;

/// Where events are published.
#[derive(Clone)]
pub struct Hub {
    sender: broadcast::Sender<String>,
    /// The `fallback` events that are in effect, which a client that connects
    /// later receives before anything else. Held under the lock that also
    /// guards sending, so a client sees each fallback exactly once.
    active: Arc<Mutex<Vec<String>>>,
}

/// The guard of the list of active fallbacks. A thread that panicked while
/// holding the lock left a list of whole strings, which is still valid.
fn locked(active: &Mutex<Vec<String>>) -> MutexGuard<'_, Vec<String>> {
    match active.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

impl Hub {
    pub fn new() -> Hub {
        let (sender, _) = broadcast::channel(BACKLOG);
        Hub {
            sender,
            active: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Sends an event to every connected client. With nobody connected there
    /// is nobody to tell, which is not an error.
    pub fn publish(&self, event: &Value) {
        let _ = self.sender.send(event.to_string());
    }

    /// Sends a `fallback` event and keeps it for the clients that connect later.
    pub fn publish_fallback(&self, event: &Value) {
        self.publish_kept(event);
    }

    /// Sends `session_start` and keeps it, so that a client that connects
    /// after it still learns that the session is running.
    pub fn publish_session_start(&self) {
        self.publish_kept(&serde_json::json!({"type": "session_start"}));
    }

    fn publish_kept(&self, event: &Value) {
        let text = event.to_string();
        let mut active = locked(&self.active);
        active.push(text.clone());
        let _ = self.sender.send(text);
    }

    /// The fallbacks in effect and a receiver of everything published after them.
    fn subscribe(&self) -> (Vec<String>, broadcast::Receiver<String>) {
        let active = locked(&self.active);
        (active.clone(), self.sender.subscribe())
    }
}

impl Default for Hub {
    fn default() -> Self {
        Hub::new()
    }
}

async fn events(State(hub): State<Hub>) -> impl IntoResponse {
    // The fallbacks in effect come first. A client that fell too far behind
    // loses the oldest events; the stream goes on.
    let (active, receiver) = hub.subscribe();
    let stream = tokio_stream::iter(active)
        .chain(BroadcastStream::new(receiver).filter_map(Result::ok))
        .map(|data| Ok::<Event, Infallible>(Event::default().data(data)));
    ([(ACCESS_CONTROL_ALLOW_ORIGIN, "*")], Sse::new(stream))
}

/// The routes of the server.
pub fn router(hub: Hub) -> Router {
    Router::new().route("/events", get(events)).with_state(hub)
}

/// Binds `127.0.0.1:requested`, or the first free one of the [`NEXT_PORTS`]
/// ports after it. Port 0 asks the system for any port and has no next ones.
/// The error is the last one when none could be bound.
pub fn bind(requested: u16) -> Result<TcpListener, std::io::Error> {
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

/// Serves `hub` on `listener` from a thread of its own; the thread ends with the process.
pub fn start(hub: Hub, listener: TcpListener) -> Result<SocketAddr, std::io::Error> {
    listener.set_nonblocking(true)?;
    let address = listener.local_addr()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()?;
    std::thread::Builder::new()
        .name(String::from("events"))
        .spawn(move || {
            runtime.block_on(async move {
                let Ok(listener) = tokio::net::TcpListener::from_std(listener) else {
                    return;
                };
                // After a successful bind the only failures left are accept errors, which
                // axum retries itself; a fatal one ends the server and nothing else.
                let _ = axum::serve(listener, router(hub)).await;
            });
        })?;
    Ok(address)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    fn get_events(address: SocketAddr) -> std::io::Result<TcpStream> {
        let mut stream = TcpStream::connect(address)?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        stream.write_all(b"GET /events HTTP/1.1\r\nHost: localhost\r\n\r\n")?;
        Ok(stream)
    }

    fn read_until(stream: &mut TcpStream, needle: &str) -> std::io::Result<String> {
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
    fn a_connected_client_receives_a_published_event_as_a_data_line() -> std::io::Result<()> {
        let hub = Hub::new();
        let address = start(hub.clone(), bind(0)?)?;
        let mut stream = get_events(address)?;
        // The response head arrives once the client is subscribed.
        let head = read_until(&mut stream, "\r\n\r\n")?;
        assert!(head.starts_with("HTTP/1.1 200"), "{head}");
        let lower = head.to_lowercase();
        assert!(lower.contains("content-type: text/event-stream"), "{head}");
        assert!(lower.contains("access-control-allow-origin: *"), "{head}");
        hub.publish(&serde_json::json!({"type": "final", "text": "こんにちは"}));
        let body = read_until(&mut stream, "こんにちは")?;
        assert!(body.contains("data: {"), "{body}");
        assert!(body.contains("\"type\":\"final\""), "{body}");
        Ok(())
    }

    #[test]
    fn an_unknown_path_is_not_found() -> std::io::Result<()> {
        let address = start(Hub::new(), bind(0)?)?;
        let mut stream = TcpStream::connect(address)?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        stream.write_all(b"GET /nothing HTTP/1.1\r\nHost: localhost\r\n\r\n")?;
        let head = read_until(&mut stream, "\r\n\r\n")?;
        assert!(head.starts_with("HTTP/1.1 404"), "{head}");
        Ok(())
    }

    /// A free port with the ten after it also free, found by holding them.
    fn free_run_of_ports() -> std::io::Result<(u16, Vec<TcpListener>)> {
        for _ in 0..50 {
            let probe = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
            let first = probe.local_addr()?.port();
            drop(probe);
            let held: Result<Vec<TcpListener>, _> = (first..=first.saturating_add(NEXT_PORTS))
                .map(|port| TcpListener::bind((Ipv4Addr::LOCALHOST, port)))
                .collect();
            if let Ok(held) = held
                && held.len() == usize::from(NEXT_PORTS) + 1
            {
                return Ok((first, held));
            }
        }
        Err(std::io::Error::other("no run of eleven free ports"))
    }

    #[test]
    fn a_taken_port_moves_to_the_next_free_one() -> std::io::Result<()> {
        let (first, mut held) = free_run_of_ports()?;
        // Free the third port of the run: the first two are taken, so that one is the answer.
        drop(held.remove(2));
        let bound = bind(first)?;
        assert_eq!(bound.local_addr()?.port(), first + 2);
        Ok(())
    }

    #[test]
    fn the_tenth_port_after_the_requested_one_is_used_and_the_eleventh_is_not()
    -> std::io::Result<()> {
        let (first, mut held) = free_run_of_ports()?;
        let tenth = held.remove(usize::from(NEXT_PORTS));
        let tenth_port = tenth.local_addr()?.port();
        drop(tenth);
        assert_eq!(bind(first)?.local_addr()?.port(), tenth_port);
        // With the tenth taken again, nothing in reach is free.
        let again = TcpListener::bind((Ipv4Addr::LOCALHOST, tenth_port))?;
        assert!(bind(first).is_err());
        drop(again);
        Ok(())
    }

    #[test]
    fn port_zero_asks_for_any_port_and_has_no_next_ones() -> std::io::Result<()> {
        assert_ne!(bind(0)?.local_addr()?.port(), 0);
        Ok(())
    }

    #[test]
    fn a_client_that_connects_later_receives_the_fallbacks_in_effect_first() -> std::io::Result<()>
    {
        let hub = Hub::new();
        hub.publish_fallback(
            &serde_json::json!({"type": "fallback", "stage": "punct", "to": "cpu"}),
        );
        hub.publish_fallback(
            &serde_json::json!({"type": "fallback", "stage": "vad", "to": "disabled"}),
        );
        let address = start(hub.clone(), bind(0)?)?;
        let mut stream = get_events(address)?;
        let body = read_until(&mut stream, "\"stage\":\"vad\"")?;
        let punct = body.find("\"stage\":\"punct\"");
        let vad = body.find("\"stage\":\"vad\"");
        assert!(punct.is_some() && vad.is_some() && punct < vad, "{body}");
        hub.publish(&serde_json::json!({"type": "final", "text": "あと"}));
        let rest = read_until(&mut stream, "あと")?;
        assert_eq!(rest.matches("\"stage\":\"punct\"").count(), 0, "{rest}");
        Ok(())
    }

    #[test]
    fn a_client_that_connects_after_session_start_still_receives_it_after_the_fallbacks()
    -> std::io::Result<()> {
        let hub = Hub::new();
        hub.publish_fallback(
            &serde_json::json!({"type": "fallback", "stage": "punct", "to": "cpu"}),
        );
        hub.publish_session_start();
        let address = start(hub.clone(), bind(0)?)?;
        let mut stream = get_events(address)?;
        let body = read_until(&mut stream, "session_start")?;
        let fallback = body.find("\"type\":\"fallback\"");
        let started = body.find("\"type\":\"session_start\"");
        assert!(
            fallback.is_some() && started.is_some() && fallback < started,
            "{body}"
        );
        Ok(())
    }

    #[test]
    fn publishing_with_nobody_connected_is_fine() {
        Hub::new().publish(&serde_json::json!({"type": "final", "text": "x"}));
    }
}

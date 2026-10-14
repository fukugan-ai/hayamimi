//! The event hub and the plumbing of the HTTP servers.
//!
//! The recognition loop runs on the main thread and publishes events to a
//! [`Hub`]; a server runs on a thread of its own and sends each event to
//! every client that is connected. Each event is one JSON object in a `data:`
//! line of `GET /events`, the form the existing overlay pages read.

use std::convert::Infallible;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};
use std::sync::{Arc, Mutex, MutexGuard};

use axum::Router;
use axum::extract::State;
use axum::http::header::ACCESS_CONTROL_ALLOW_ORIGIN;
use axum::response::IntoResponse;
use axum::response::sse::{Event, Sse};
use axum::routing::get;
use serde_json::Value;
use tokio::sync::{broadcast, watch};
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;

/// Events a slow client may fall behind by before it loses the oldest ones.
const BACKLOG: usize = 1024;

/// Ports tried after the requested one when it is taken.
pub const NEXT_PORTS: u16 = 10;

/// The key under which `session_start` is kept.
const SESSION_START_KEY: &str = "session_start";

/// An event kept for the clients that connect later, with what replaces it.
#[derive(Debug)]
struct Kept {
    key: String,
    text: String,
}

/// Where events are published.
#[derive(Clone)]
pub struct Hub {
    sender: broadcast::Sender<String>,
    /// The events in effect that a client which connects later receives
    /// before anything else: one `fallback` per stage, and `session_start`.
    /// Held under the lock that also guards sending, so a client sees each
    /// kept event exactly once.
    active: Arc<Mutex<Vec<Kept>>>,
    /// Turns true when `session_start` is published: the models are open and
    /// the loop is about to run.
    started: Arc<watch::Sender<bool>>,
}

/// The guard of the list of kept events. A thread that panicked while
/// holding the lock left a list of whole entries, which is still valid.
fn locked(active: &Mutex<Vec<Kept>>) -> MutexGuard<'_, Vec<Kept>> {
    match active.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

impl Hub {
    pub fn new() -> Hub {
        let (sender, _) = broadcast::channel(BACKLOG);
        let (started, _) = watch::channel(false);
        Hub {
            sender,
            active: Arc::new(Mutex::new(Vec::new())),
            started: Arc::new(started),
        }
    }

    /// Sends an event to every connected client. With nobody connected there
    /// is nobody to tell, which is not an error.
    pub fn publish(&self, event: &Value) {
        let _ = self.sender.send(event.to_string());
    }

    /// Sends a `fallback` event. One per stage is kept for the clients that
    /// connect later: a newer one replaces it, and one whose `to` is null
    /// (the stage is back on its first road) removes it.
    pub fn publish_fallback(&self, event: &Value) {
        let Some(stage) = event.get("stage").and_then(Value::as_str) else {
            self.publish(event);
            return;
        };
        let key = format!("fallback:{stage}");
        if event.get("to").is_some_and(Value::is_null) {
            self.publish_forgetting(&key, event);
        } else {
            self.publish_kept(key, event);
        }
    }

    /// Sends `session_start` and keeps it, so that a client that connects
    /// after it still learns that the session is running.
    pub fn publish_session_start(&self) {
        let event = serde_json::json!({"type": "session_start"});
        self.publish_kept(String::from(SESSION_START_KEY), &event);
        self.started.send_replace(true);
    }

    /// Resolves once `session_start` has been published, at once if it was.
    /// A client of `/ingest` is answered `ready` only then, so that the audio
    /// it sends meets recognizers that are already loaded.
    pub async fn session_started(&self) {
        let mut started = self.started.subscribe();
        // The hub holds the sender, so the wait ends only when the flag is true.
        let _ = started.wait_for(|started| *started).await;
    }

    fn publish_kept(&self, key: String, event: &Value) {
        let text = event.to_string();
        let mut active = locked(&self.active);
        match active.iter_mut().find(|kept| kept.key == key) {
            Some(kept) => kept.text.clone_from(&text),
            None => active.push(Kept {
                key,
                text: text.clone(),
            }),
        }
        let _ = self.sender.send(text);
    }

    fn publish_forgetting(&self, key: &str, event: &Value) {
        let mut active = locked(&self.active);
        active.retain(|kept| kept.key != key);
        let _ = self.sender.send(event.to_string());
    }

    /// The kept events and a receiver of everything published after them.
    pub fn subscribe(&self) -> (Vec<String>, broadcast::Receiver<String>) {
        let active = locked(&self.active);
        let kept = active.iter().map(|kept| kept.text.clone()).collect();
        (kept, self.sender.subscribe())
    }

    /// A receiver of everything published from now on, with no replay.
    pub fn subscribe_live(&self) -> broadcast::Receiver<String> {
        self.sender.subscribe()
    }
}

impl Default for Hub {
    fn default() -> Self {
        Hub::new()
    }
}

/// `GET /events`: the kept events first, then every event as it is published.
pub async fn events(State(hub): State<Hub>) -> impl IntoResponse {
    // A client that fell too far behind loses the oldest events; the stream goes on.
    let (kept, receiver) = hub.subscribe();
    let stream = tokio_stream::iter(kept)
        .chain(BroadcastStream::new(receiver).filter_map(Result::ok))
        .map(|data| Ok::<Event, Infallible>(Event::default().data(data)));
    ([(ACCESS_CONTROL_ALLOW_ORIGIN, "*")], Sse::new(stream))
}

/// A router with `/events` only.
pub fn events_router(hub: Hub) -> Router {
    Router::new().route("/events", get(events)).with_state(hub)
}

/// Binds `ip:requested`, or the first free one of the [`NEXT_PORTS`] ports
/// after it. Port 0 asks the system for any port and has no next ones. The
/// error is the last one when none could be bound.
pub fn bind_on(ip: IpAddr, requested: u16) -> Result<TcpListener, std::io::Error> {
    let last = if requested == 0 {
        0
    } else {
        requested.saturating_add(NEXT_PORTS)
    };
    let mut bound = TcpListener::bind((ip, requested));
    for port in (requested..=last).skip(1) {
        if bound.is_ok() {
            break;
        }
        bound = TcpListener::bind((ip, port));
    }
    bound
}

/// [`bind_on`] the loopback address.
pub fn bind(requested: u16) -> Result<TcpListener, std::io::Error> {
    bind_on(IpAddr::V4(Ipv4Addr::LOCALHOST), requested)
}

/// Serves `router` on `listener` from a thread called `name`; the thread ends with the process.
pub fn serve(
    router: Router,
    listener: TcpListener,
    name: &str,
) -> Result<SocketAddr, std::io::Error> {
    listener.set_nonblocking(true)?;
    let address = listener.local_addr()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()?;
    std::thread::Builder::new()
        .name(name.to_owned())
        .spawn(move || {
            runtime.block_on(async move {
                let Ok(listener) = tokio::net::TcpListener::from_std(listener) else {
                    return;
                };
                // After a successful bind the only failures left are accept errors, which
                // axum retries itself; a fatal one ends the server and nothing else.
                let _ = axum::serve(listener, router).await;
            });
        })?;
    Ok(address)
}

/// Serves `/events` of `hub` on `listener`.
pub fn start(hub: Hub, listener: TcpListener) -> Result<SocketAddr, std::io::Error> {
    serve(events_router(hub), listener, "events")
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

    fn fallback(stage: &str, to: Value) -> Value {
        serde_json::json!({"type": "fallback", "stage": stage, "to": to})
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
        hub.publish_fallback(&fallback("punct", Value::from("cpu")));
        hub.publish_fallback(&fallback("vad", Value::from("disabled")));
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
        hub.publish_fallback(&fallback("punct", Value::from("cpu")));
        hub.publish_session_start();
        let address = start(hub.clone(), bind(0)?)?;
        let mut stream = get_events(address)?;
        let body = read_until(&mut stream, "session_start")?;
        let first = body.find("\"type\":\"fallback\"");
        let started = body.find("\"type\":\"session_start\"");
        assert!(
            first.is_some() && started.is_some() && first < started,
            "{body}"
        );
        assert_eq!(body.matches("\"type\":\"fallback\"").count(), 1, "{body}");
        assert_eq!(
            body.matches("\"type\":\"session_start\"").count(),
            1,
            "{body}"
        );
        hub.publish(&serde_json::json!({"type": "final", "text": "あと"}));
        let rest = read_until(&mut stream, "あと")?;
        assert!(rest.contains("\"type\":\"final\""), "{rest}");
        Ok(())
    }

    #[test]
    fn publishing_with_nobody_connected_is_fine() {
        Hub::new().publish(&serde_json::json!({"type": "final", "text": "x"}));
    }

    #[test]
    fn a_newer_fallback_of_a_stage_replaces_the_kept_one() {
        let hub = Hub::new();
        hub.publish_fallback(&fallback("capture", Value::from("single")));
        hub.publish_fallback(&fallback("capture", Value::from("none")));
        let (kept, _) = hub.subscribe();
        assert_eq!(kept.len(), 1);
        assert!(kept[0].contains("\"none\""), "{}", kept[0]);
    }

    #[test]
    fn a_fallback_with_a_null_destination_removes_the_kept_one_and_is_still_sent() {
        let hub = Hub::new();
        hub.publish_fallback(&fallback("capture", Value::from("single")));
        hub.publish_fallback(&fallback("punct", Value::from("cpu")));
        let live = hub.subscribe_live();
        hub.publish_fallback(&fallback("capture", Value::Null));
        let (kept, _) = hub.subscribe();
        assert_eq!(kept.len(), 1);
        assert!(kept[0].contains("\"punct\""), "{}", kept[0]);
        let mut live = live;
        assert!(
            live.try_recv()
                .is_ok_and(|sent| sent.contains("\"to\":null"))
        );
    }

    #[test]
    fn a_null_destination_for_a_stage_never_kept_keeps_nothing() {
        let hub = Hub::new();
        hub.publish_fallback(&fallback("sink", Value::Null));
        assert!(hub.subscribe().0.is_empty());
    }

    #[test]
    fn session_start_is_kept_once_however_often_it_is_published() {
        let hub = Hub::new();
        hub.publish_session_start();
        hub.publish_session_start();
        assert_eq!(hub.subscribe().0.len(), 1);
    }

    #[tokio::test]
    async fn the_start_of_the_session_is_awaited_and_then_known()
    -> Result<(), Box<dyn std::error::Error>> {
        let hub = Hub::new();
        let waiting = tokio::spawn({
            let hub = hub.clone();
            async move { hub.session_started().await }
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!waiting.is_finished(), "it did not wait for the start");
        hub.publish_session_start();
        tokio::time::timeout(Duration::from_secs(5), waiting).await??;
        // After the start, a new wait ends at once.
        tokio::time::timeout(Duration::from_secs(5), hub.session_started()).await?;
        Ok(())
    }

    #[test]
    fn a_fallback_alone_does_not_start_the_session() {
        let hub = Hub::new();
        hub.publish_fallback(&fallback("punct", Value::from("cpu")));
        assert!(!*hub.started.borrow());
        hub.publish_session_start();
        assert!(*hub.started.borrow());
    }

    #[test]
    fn a_live_subscription_has_no_replay() {
        let hub = Hub::new();
        hub.publish_session_start();
        let mut live = hub.subscribe_live();
        assert!(live.try_recv().is_err());
        hub.publish(&serde_json::json!({"type": "partial", "text": "x"}));
        assert!(live.try_recv().is_ok());
    }
}

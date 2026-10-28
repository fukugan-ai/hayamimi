//! `/ingest`: a WebSocket that takes audio from a network client.
//!
//! The client sends one JSON text frame with the format of its audio, then
//! binary frames of 16-bit little-endian PCM. The server answers the config
//! with `ready` once the session has started (the models are loaded) and
//! mirrors the subtitle events onto the connection. One
//! audio client is accepted at a time; a second one is told so and closed.
//! When the client leaves, what the detector holds is cut, as no silence will
//! follow it.
//!
//! The handshake is judged by its `Host` and `Origin` as a page's request is
//! (see [`crate::routes`]): a page of another web site, and a `Host` that is a
//! DNS name this machine was not told of, get a 403 and no socket.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::Router;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Request, State};
use axum::middleware;
use axum::response::Response;
use axum::routing::get;
use hayamimi_asr::audio::SAMPLE_RATE;
use hayamimi_relay::origin::{Guard, Verdict};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::broadcast::error::RecvError;

use crate::capture::convert::{Chain, pcm16_to_f32};
use crate::feed::{AudioItem, AudioTx};
use crate::log;
use crate::routes;
use crate::server::Hub;
use crate::strings::Strings;
use crate::wire;

/// The most channels and the highest sample rate a client may declare.
const MAX_CHANNELS: usize = 64;
const MAX_RATE: u32 = 384_000;
/// The only audio format of the protocol.
const FORMAT: &str = "pcm_s16le";

/// What the routes share.
#[derive(Clone)]
pub struct IngestState {
    pub hub: Hub,
    pub audio: AudioTx,
    pub strings: Arc<Strings>,
    busy: Arc<AtomicBool>,
}

impl IngestState {
    pub fn new(hub: Hub, audio: AudioTx, strings: Arc<Strings>) -> IngestState {
        IngestState {
            hub,
            audio,
            strings,
            busy: Arc::new(AtomicBool::new(false)),
        }
    }
}

/// The routes of the `/ingest` server, for the pages of this machine.
pub fn router(state: IngestState) -> Router {
    router_for(state, Guard::default())
}

/// The routes of the `/ingest` server for the pages of this machine and the
/// origins `guard` names. A WebSocket is not held to the same-origin policy:
/// a page of any web site can open one to this machine, read the events it
/// mirrors and feed it audio. The browser names the page in the `Origin` of
/// the handshake, so the handshake is judged as the request of a page is.
pub fn router_for(state: IngestState, guard: Guard) -> Router {
    Router::new()
        .route("/ingest", get(upgrade))
        .layer(middleware::from_fn_with_state(
            guard,
            pages_that_may_open_the_socket,
        ))
        .with_state(state)
}

/// Lets a handshake through when its `Host` and `Origin` are those of a page
/// of this machine (see [`routes::judge_headers`]); a client that names no
/// `Origin` is not a browser and is let in. A refusal is a 403 before the
/// upgrade, so no frame is read from it.
async fn pages_that_may_open_the_socket(
    State(guard): State<Guard>,
    request: Request,
    next: middleware::Next,
) -> Response {
    match routes::judge_headers(&guard, request.headers()) {
        Verdict::Refuse(refusal) => routes::refuse_page(refusal),
        Verdict::Serve { .. } => next.run(request).await,
    }
}

async fn upgrade(State(state): State<IngestState>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| serve(socket, state))
}

/// The one place for an audio client. Dropping it frees the place.
struct Lease(Arc<AtomicBool>);

impl Lease {
    fn take(busy: &Arc<AtomicBool>) -> Option<Lease> {
        busy.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
            .then(|| Lease(Arc::clone(busy)))
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// The first frame of a connection. A member left out is what the protocol
/// says it is: 16 kHz, one channel, 16-bit PCM.
#[derive(Debug, Deserialize)]
struct Handshake {
    #[serde(default = "default_rate")]
    sr: u32,
    #[serde(default = "default_channels")]
    channels: usize,
    #[serde(default = "default_format")]
    format: String,
}

fn default_rate() -> u32 {
    SAMPLE_RATE
}

fn default_channels() -> usize {
    1
}

fn default_format() -> String {
    String::from(FORMAT)
}

/// The audio format a client declared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Declared {
    rate: u32,
    channels: usize,
}

/// Reads the config frame. The error is the text sent back to the client.
fn parse_handshake(text: &str) -> Result<Declared, String> {
    let bad = || String::from("bad handshake JSON");
    // A struct would also be read from a JSON array; the config must be an object.
    let value: Value = serde_json::from_str(text).map_err(|_| bad())?;
    if !value.is_object() {
        return Err(bad());
    }
    let handshake: Handshake = serde_json::from_value(value).map_err(|_| bad())?;
    if handshake.format != FORMAT {
        return Err(format!(
            "unsupported format '{}', only {FORMAT}",
            handshake.format
        ));
    }
    if handshake.sr == 0 || handshake.sr > MAX_RATE {
        return Err(bad());
    }
    if handshake.channels == 0 || handshake.channels > MAX_CHANNELS {
        return Err(bad());
    }
    Ok(Declared {
        rate: handshake.sr,
        channels: handshake.channels,
    })
}

/// A connection after its config: converts what arrives to windows.
struct Stream {
    chain: Chain,
    windows: u64,
}

/// Whether the connection goes on.
enum Next {
    Continue,
    End,
}

async fn send_json(socket: &mut WebSocket, event: &Value) -> bool {
    socket
        .send(Message::Text(event.to_string().into()))
        .await
        .is_ok()
}

/// Tells a second client that the place is taken, and closes.
async fn refuse(mut socket: WebSocket, state: &IngestState) {
    log::print(&log::ingest_rejected_line(&state.strings));
    let busy = wire::error_event("already streaming; only one audio client is accepted at a time");
    // The client may have gone already; the refusal is logged either way.
    let _ = send_json(&mut socket, &busy).await;
    let _ = socket.send(Message::Close(None)).await;
}

/// Handles the config frame: answers `ready`, or tells the client why not.
/// `ready` waits for the session to start, which is after every model the
/// session needs is loaded: a client that streams in real time on `ready`
/// then meets a recognizer that is already there.
/// Returns the stream to convert audio with, or `None` to end the connection.
async fn configure(socket: &mut WebSocket, text: &str, state: &IngestState) -> Option<Stream> {
    match parse_handshake(text) {
        Ok(declared) => {
            state.hub.session_started().await;
            let ready = wire::ready_event(SAMPLE_RATE, state.audio.stream_us());
            if !send_json(socket, &ready).await {
                return None;
            }
            log::print(&log::ingest_connected_line(
                &state.strings,
                declared.rate,
                declared.channels,
            ));
            Some(Stream {
                chain: Chain::new(declared.rate, declared.channels, SAMPLE_RATE),
                windows: 0,
            })
        }
        Err(message) => {
            // The client is being sent away; whether it hears why does not change that.
            let _ = send_json(socket, &wire::error_event(&message)).await;
            None
        }
    }
}

/// Puts the windows of one binary frame on the audio queue.
fn feed(stream: &mut Stream, audio: &AudioTx, bytes: &[u8]) {
    let samples = pcm16_to_f32(bytes);
    for window in stream.chain.push(&samples) {
        stream.windows += 1;
        // A refused window is dropped on purpose: see `AudioTx::send`.
        audio.send(AudioItem::Window(window));
    }
}

/// One frame from the client.
async fn handle(
    socket: &mut WebSocket,
    stream: &mut Option<Stream>,
    state: &IngestState,
    message: Message,
) -> Next {
    match (message, stream.as_mut()) {
        (Message::Close(_), _) => Next::End,
        (Message::Text(text), None) => {
            *stream = configure(socket, text.as_str(), state).await;
            if stream.is_some() {
                Next::Continue
            } else {
                Next::End
            }
        }
        (Message::Binary(bytes), Some(open)) => {
            feed(open, &state.audio, &bytes);
            Next::Continue
        }
        // Audio before the config is dropped; text after it, pings and pongs are not part of the protocol.
        _ => Next::Continue,
    }
}

async fn deliver(socket: &mut WebSocket, text: String) -> Next {
    if socket.send(Message::Text(text.into())).await.is_ok() {
        Next::Continue
    } else {
        Next::End
    }
}

/// Sends one event of the hub to the client.
async fn forward(socket: &mut WebSocket, event: Result<String, RecvError>) -> Next {
    match event {
        Ok(text) => deliver(socket, text).await,
        // A client that fell behind loses events; the connection goes on.
        Err(RecvError::Lagged(_)) => Next::Continue,
        Err(RecvError::Closed) => Next::End,
    }
}

/// One connection, from the upgrade to the end.
async fn serve(mut socket: WebSocket, state: IngestState) {
    let Some(_place) = Lease::take(&state.busy) else {
        refuse(socket, &state).await;
        return;
    };
    let mut events = state.hub.subscribe_live();
    let mut stream: Option<Stream> = None;
    loop {
        let next = tokio::select! {
            message = socket.recv() => match message {
                Some(Ok(message)) => handle(&mut socket, &mut stream, &state, message).await,
                _ => Next::End,
            },
            event = events.recv(), if stream.is_some() => forward(&mut socket, event).await,
        };
        if matches!(next, Next::End) {
            break;
        }
    }
    if let Some(open) = stream {
        // No silence follows a client that left: cut what the detector holds.
        state.audio.send(AudioItem::Flush);
        log::print(&log::ingest_closed_line(&state.strings, open.windows));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_config_is_read() {
        let declared = parse_handshake(r#"{"sr":8000,"format":"pcm_s16le","channels":2}"#);
        assert_eq!(
            declared,
            Ok(Declared {
                rate: 8000,
                channels: 2
            })
        );
    }

    #[test]
    fn a_config_without_members_is_the_16k_mono_of_the_protocol() {
        assert_eq!(
            parse_handshake("{}"),
            Ok(Declared {
                rate: 16_000,
                channels: 1
            })
        );
    }

    #[test]
    fn a_config_that_is_not_json_or_not_an_object_is_bad() {
        assert_eq!(
            parse_handshake("{"),
            Err(String::from("bad handshake JSON"))
        );
        assert_eq!(
            parse_handshake("[1]"),
            Err(String::from("bad handshake JSON"))
        );
        assert_eq!(
            parse_handshake(r#"{"sr":"x"}"#),
            Err(String::from("bad handshake JSON"))
        );
    }

    #[test]
    fn another_format_is_named() {
        let said = parse_handshake(r#"{"format":"f32le"}"#);
        assert!(said.is_err_and(|text| text.contains("f32le") && text.contains("pcm_s16le")));
    }

    #[test]
    fn rates_and_channels_at_the_limits() {
        assert!(parse_handshake(r#"{"sr":1}"#).is_ok());
        assert!(parse_handshake(r#"{"sr":384000}"#).is_ok());
        assert!(parse_handshake(r#"{"sr":384001}"#).is_err());
        assert!(parse_handshake(r#"{"sr":0}"#).is_err());
        assert!(parse_handshake(r#"{"channels":64}"#).is_ok());
        assert!(parse_handshake(r#"{"channels":65}"#).is_err());
        assert!(parse_handshake(r#"{"channels":0}"#).is_err());
    }

    /// T-BR-13-008
    #[test]
    fn the_ready_frame_carries_the_stream_position_in_seconds() {
        let ready = wire::ready_event(SAMPLE_RATE, 30_904_000);
        assert_eq!(ready["type"], "ready");
        assert_eq!(ready["sr"], 16_000);
        assert_eq!(ready["stream_s"].as_f64(), Some(30.904));
        let start = wire::ready_event(SAMPLE_RATE, 0);
        assert!(start["stream_s"].is_number(), "{start}");
        assert_eq!(start["stream_s"].as_f64(), Some(0.0));
    }

    #[test]
    fn the_place_is_taken_once_and_freed_when_the_holder_goes() {
        let busy = Arc::new(AtomicBool::new(false));
        let first = Lease::take(&busy);
        assert!(first.is_some());
        assert!(Lease::take(&busy).is_none());
        drop(first);
        assert!(Lease::take(&busy).is_some());
    }
}

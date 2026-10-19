//! `/ingest`: a WebSocket that takes audio from a network client.
//!
//! The client sends one JSON text frame with the format of its audio, then
//! binary frames of 16-bit little-endian PCM. The server answers the config
//! with `ready` and mirrors the subtitle events onto the connection. One
//! audio client is accepted at a time; a second one is told so and closed.
//! When the client leaves, what the detector holds is cut, as no silence will
//! follow it.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::Router;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use axum::routing::get;
use hayamimi_asr::audio::SAMPLE_RATE;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::broadcast::error::RecvError;

use crate::capture::convert::{Chain, pcm16_to_f32};
use crate::feed::{AudioItem, AudioTx};
use crate::log;
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

/// The routes of the `/ingest` server.
pub fn router(state: IngestState) -> Router {
    Router::new()
        .route("/ingest", get(upgrade))
        .with_state(state)
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
/// Returns the stream to convert audio with, or `None` to end the connection.
async fn configure(socket: &mut WebSocket, text: &str, state: &IngestState) -> Option<Stream> {
    match parse_handshake(text) {
        Ok(declared) => {
            if !send_json(socket, &wire::ready_event(SAMPLE_RATE)).await {
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

    #[test]
    fn the_place_is_taken_once_and_freed_when_the_holder_goes() {
        let busy = Arc::new(AtomicBool::new(false));
        let first = Lease::take(&busy);
        assert!(first.is_some());
        assert!(Lease::take(&busy).is_none());
        drop(first);
        assert!(Lease::take(&busy).is_some());
    }

    // The upgrade handshake, judged on the real router over a loopback socket.

    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpStream};
    use std::time::Duration;

    use hayamimi_eval::ws;
    use hayamimi_relay::lang::Lang;

    use crate::feed::{self, AudioRx};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    /// The worked example of RFC 6455 section 1.3.
    const RFC_KEY: &str = "dGhlIHNhbXBsZSBub25jZQ==";
    const RFC_ACCEPT: &str = "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=";
    /// A second key and its accept value, so that the answer is computed from the key.
    const OTHER_KEY: &str = "x3JJHMbDL1EzLkh9GBhXDw==";
    const OTHER_ACCEPT: &str = "HSmrc0sMlYUkAGmm5OPpG2HaGWk=";

    /// An `/ingest` server on a free loopback port; the receiver keeps the queue open.
    fn serve_ingest() -> Result<(SocketAddr, AudioRx), Box<dyn std::error::Error>> {
        let (audio, queue) = feed::channel();
        let strings = Arc::new(Strings::embedded(Lang::En)?);
        let state = IngestState::new(Hub::new(), audio, strings);
        let address = crate::server::serve(router(state), crate::server::bind(0)?, "ingest-unit")?;
        Ok((address, queue))
    }

    /// The status, the accept value and the head of the answer to `request`.
    fn answer_to(
        address: SocketAddr,
        request: &str,
    ) -> Result<(u16, Option<String>, String), Box<dyn std::error::Error>> {
        let mut stream = TcpStream::connect(address)?;
        stream.set_read_timeout(Some(Duration::from_secs(10)))?;
        stream.write_all(request.as_bytes())?;
        let mut head = Vec::new();
        let mut byte = [0_u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte)?;
            head.push(byte[0]);
        }
        let head = String::from_utf8_lossy(&head).into_owned();
        let (status, accept) = ws::parse_upgrade_response(&head)?;
        Ok((status, accept, head))
    }

    /// A request that asks for an upgrade of `path`, with `key` when there is one.
    fn upgrade_of(path: &str, key: Option<&str>) -> String {
        let key_line = key.map_or_else(String::new, |key| format!("Sec-WebSocket-Key: {key}\r\n"));
        format!(
            "GET {path} HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\n\
             Connection: Upgrade\r\n{key_line}Sec-WebSocket-Version: 13\r\n\r\n"
        )
    }

    #[test]
    fn compute_accept_key_matches_rfc_example() -> TestResult {
        let (address, _queue) = serve_ingest()?;
        let (status, accept, _) = answer_to(address, &upgrade_of("/ingest", Some(RFC_KEY)))?;
        assert_eq!(status, 101);
        assert_eq!(accept.as_deref(), Some(RFC_ACCEPT));
        // The value follows the key: another key gets its own answer.
        let (_, other, _) = answer_to(address, &upgrade_of("/ingest", Some(OTHER_KEY)))?;
        assert_eq!(other.as_deref(), Some(OTHER_ACCEPT));
        Ok(())
    }

    #[test]
    fn parse_handshake_request_extracts_path_and_key() -> TestResult {
        let (address, _queue) = serve_ingest()?;
        // The key is read: the answer is the one for this key.
        let (status, accept, _) = answer_to(address, &upgrade_of("/ingest", Some(RFC_KEY)))?;
        assert_eq!((status, accept.as_deref()), (101, Some(RFC_ACCEPT)));
        // The path is read: the same request for another path is not upgraded.
        let (status, accept, _) = answer_to(address, &upgrade_of("/elsewhere", Some(RFC_KEY)))?;
        assert_eq!((status, accept), (404, None));
        Ok(())
    }

    #[test]
    fn parse_handshake_request_rejects_non_websocket() -> TestResult {
        let (address, _queue) = serve_ingest()?;
        let plain = "GET /ingest HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let (status, accept, _) = answer_to(address, plain)?;
        assert_eq!((status, accept), (400, None));
        Ok(())
    }

    #[test]
    fn parse_handshake_request_rejects_missing_key() -> TestResult {
        let (address, _queue) = serve_ingest()?;
        let (status, accept, _) = answer_to(address, &upgrade_of("/ingest", None))?;
        assert_eq!((status, accept), (400, None));
        Ok(())
    }

    #[test]
    fn build_handshake_response_contains_correct_accept() -> TestResult {
        let (address, _queue) = serve_ingest()?;
        let (_, _, head) = answer_to(address, &upgrade_of("/ingest", Some(RFC_KEY)))?;
        assert!(head.starts_with("HTTP/1.1 101"), "{head}");
        let lowered = head.to_ascii_lowercase();
        assert!(lowered.contains("upgrade: websocket"), "{head}");
        assert!(head.contains(RFC_ACCEPT), "{head}");
        Ok(())
    }

    #[test]
    fn build_handshake_request_roundtrips_through_parse() -> TestResult {
        let (address, _queue) = serve_ingest()?;
        let request = ws::upgrade_request("127.0.0.1", address.port(), "/ingest", RFC_KEY);
        let (status, accept, _) = answer_to(address, &request)?;
        assert_eq!((status, accept.as_deref()), (101, Some(RFC_ACCEPT)));
        // The path the builder wrote is the one the server routed.
        let other = ws::upgrade_request("127.0.0.1", address.port(), "/elsewhere", RFC_KEY);
        let (status, _, _) = answer_to(address, &other)?;
        assert_eq!(status, 404);
        Ok(())
    }

    // The frame codec the clients of `/ingest` speak (the server side is axum's).

    use hayamimi_relay::ws_client::{
        Frame, OP_BINARY, OP_CLOSE, OP_TEXT, decode_frame, encode_frame,
    };

    const OP_PING: u8 = 0x9;
    const MASK: [u8; 4] = [0x12, 0x34, 0x56, 0x78];

    /// The frame at the front of `wire` and how many bytes it used.
    fn first_frame(wire: &[u8]) -> Result<(Frame, usize), Box<dyn std::error::Error>> {
        decode_frame(wire)?.ok_or_else(|| "the buffer holds no whole frame".into())
    }

    #[test]
    fn frame_roundtrip_unmasked_binary() -> TestResult {
        // More than 125 bytes, so the 16-bit length form is used.
        let payload: Vec<u8> = (0..=255_u8).cycle().take(1024).collect();
        let wire = encode_frame(OP_BINARY, &payload, None);
        assert_eq!(wire[1], 126, "the 16-bit length marker, no mask bit");
        let (frame, used) = first_frame(&wire)?;
        assert_eq!(frame.opcode, OP_BINARY);
        assert_eq!(frame.payload, payload);
        assert_eq!(used, wire.len());
        Ok(())
    }

    #[test]
    fn frame_roundtrip_masked_text() -> TestResult {
        let payload = br#"{"sr": 16000, "format": "pcm_s16le", "channels": 1}"#;
        let wire = encode_frame(OP_TEXT, payload, Some(MASK));
        let (frame, used) = first_frame(&wire)?;
        assert_eq!(frame.opcode, OP_TEXT);
        assert_eq!(frame.payload, payload);
        assert_eq!(used, wire.len());
        Ok(())
    }

    #[test]
    fn frame_masked_bytes_differ_from_plaintext_on_wire() -> TestResult {
        // An all-zero payload would show a missing mask as itself.
        let payload = [0_u8; 32];
        let wire = encode_frame(OP_BINARY, &payload, Some(MASK));
        let header = 2 + 4;
        assert_ne!(&wire[header..], &payload[..]);
        assert_eq!(first_frame(&wire)?.0.payload, payload);
        Ok(())
    }

    #[test]
    fn decode_frame_incomplete_returns_none() -> TestResult {
        let wire = encode_frame(OP_BINARY, &[b'x'; 50], Some(MASK));
        assert_eq!(decode_frame(&wire[..5])?, None);
        Ok(())
    }

    #[test]
    fn decode_frame_two_frames_in_one_buffer() -> TestResult {
        let mut wire = encode_frame(OP_TEXT, b"one", Some(MASK));
        let first_len = wire.len();
        wire.extend(encode_frame(OP_TEXT, b"two", Some(MASK)));
        let (first, used_first) = first_frame(&wire)?;
        assert_eq!(
            (first.opcode, first.payload.as_slice()),
            (OP_TEXT, &b"one"[..])
        );
        assert_eq!(used_first, first_len);
        let (second, used_second) = first_frame(&wire[used_first..])?;
        assert_eq!(
            (second.opcode, second.payload.as_slice()),
            (OP_TEXT, &b"two"[..])
        );
        assert_eq!(used_first + used_second, wire.len());
        Ok(())
    }

    #[test]
    fn decode_frame_large_payload_uses_64bit_length() -> TestResult {
        // Past 65535 bytes the 127 marker and the 8-byte length are used.
        let payload = vec![b'z'; 70_000];
        let wire = encode_frame(OP_BINARY, &payload, Some(MASK));
        assert_eq!(wire[1] & 0x7f, 127);
        let (frame, used) = first_frame(&wire)?;
        assert_eq!(frame.opcode, OP_BINARY);
        assert_eq!(frame.payload.len(), 70_000);
        assert_eq!(used, wire.len());
        Ok(())
    }

    #[test]
    fn frame_close_and_ping_opcodes_roundtrip() -> TestResult {
        for opcode in [OP_CLOSE, OP_PING] {
            let wire = encode_frame(opcode, b"", Some(MASK));
            let (frame, _) = first_frame(&wire)?;
            assert_eq!(frame.opcode, opcode);
            assert!(frame.payload.is_empty());
        }
        Ok(())
    }
}

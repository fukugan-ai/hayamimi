//! Playing a recording into the server's `/ingest` endpoint at the pace of
//! real speech, and stamping every event that comes back.
//!
//! Audio is cut into 20 ms chunks, with extra cuts at the marks the caller
//! names (an utterance's first sample and the sample after its last), so
//! that "the moment that sample was sent" is the moment one whole chunk was
//! sent. A chunk is sent when its last sample would have been captured by a
//! live microphone: at `end / 16000` seconds after the start.

use std::io::Write;
use std::net::{Shutdown, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::error::EvalError;
use crate::log;
use crate::sse::Listener;
use crate::wav::SAMPLE_RATE;
use crate::ws::{self, FrameReader, Handshake, Incoming, OP_CLOSE, OP_TEXT};

pub const INGEST_PATH: &str = "/ingest";
pub const EVENTS_PATH: &str = "/events";
/// Samples per network frame: 20 ms.
pub const CHUNK_SAMPLES: usize = 320;
/// The first message of an ingest connection.
const CONFIG_JSON: &str = r#"{"sr":16000,"format":"pcm_s16le","channels":1}"#;
pub(crate) const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
pub(crate) const READY_TIMEOUT: Duration = Duration::from_secs(60);
const READ_TICK: Duration = Duration::from_millis(100);

/// One network frame's worth of audio, as sample positions in the stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chunk {
    pub start: usize,
    pub end: usize,
}

/// The whole stream: `repeat` copies of the recording, each followed by
/// `gap` samples of silence.
#[derive(Debug, PartialEq, Eq)]
pub struct Plan {
    pub chunks: Vec<Chunk>,
    pub audio_len: usize,
    /// Samples from one copy's start to the next's.
    pub period: usize,
}

/// Cuts the stream into chunks. `marks` are positions inside one copy where
/// a chunk must start or end.
pub fn plan(audio_len: usize, marks: &[usize], repeat: u32, gap: usize) -> Plan {
    let period = audio_len + gap;
    let total = period * repeat as usize;
    let mut cuts: Vec<usize> = Vec::new();
    for copy in 0..repeat as usize {
        let base = copy * period;
        cuts.extend(marks.iter().filter(|m| **m <= audio_len).map(|m| base + m));
        cuts.push(base + audio_len);
        cuts.push(base + period);
    }
    cuts.sort_unstable();
    cuts.dedup();
    let mut chunks = Vec::with_capacity(total / CHUNK_SAMPLES + cuts.len());
    let mut position = 0;
    while position < total {
        let next_cut = cuts.partition_point(|c| *c <= position);
        let end = (position + CHUNK_SAMPLES).min(cuts[next_cut]);
        chunks.push(Chunk {
            start: position,
            end,
        });
        position = end;
    }
    Plan {
        chunks,
        audio_len,
        period,
    }
}

/// The little-endian 16-bit PCM bytes of a chunk. A chunk never spans the
/// end of the recording, because that end is always a cut.
pub fn chunk_pcm(plan: &Plan, audio: &[i16], chunk: &Chunk) -> Vec<u8> {
    let offset = chunk.start % plan.period;
    let count = chunk.end - chunk.start;
    let mut pcm = Vec::with_capacity(count * 2);
    if offset < plan.audio_len {
        for sample in &audio[offset..offset + count] {
            pcm.extend_from_slice(&sample.to_le_bytes());
        }
    } else {
        pcm.resize(count * 2, 0);
    }
    pcm
}

/// When the chunk that starts (`at_end = false`) or ends (`at_end = true`)
/// at stream position `position` was sent, from the per-chunk send times.
pub fn sent_at(plan: &Plan, sent_us: &[i64], position: usize, at_end: bool) -> Option<i64> {
    let index = if at_end {
        plan.chunks
            .binary_search_by_key(&position, |c| c.end)
            .ok()?
    } else {
        plan.chunks
            .binary_search_by_key(&position, |c| c.start)
            .ok()?
    };
    sent_us.get(index).copied()
}

/// Microseconds since `origin`.
pub fn micros_since(origin: Instant) -> i64 {
    origin.elapsed().as_micros() as i64
}

/// What the reader thread collected.
struct Collected {
    events: Vec<(i64, String)>,
    closed_by_server: bool,
}

/// An open ingest connection and the thread reading its events.
pub struct Ingest {
    stream: TcpStream,
    handshake: Handshake,
    origin: Instant,
    stop: Arc<AtomicBool>,
    ready: Arc<AtomicBool>,
    reader: JoinHandle<Result<Collected, EvalError>>,
}

/// What a finished connection delivered.
#[derive(Debug)]
pub struct Received {
    pub handshake: Handshake,
    /// Every text frame the server sent, with its arrival time in
    /// microseconds since `origin`.
    pub events: Vec<(i64, String)>,
    pub closed_by_server: bool,
}

/// What a finished connection heard, whatever it was connected by.
#[derive(Debug)]
pub struct Heard {
    /// Every event the server sent, with its arrival time in microseconds
    /// since `origin`.
    pub events: Vec<(i64, String)>,
    pub closed_by_server: bool,
}

impl Received {
    /// What was heard, without the upgrade exchange.
    pub fn into_heard(self) -> Heard {
        Heard {
            events: self.events,
            closed_by_server: self.closed_by_server,
        }
    }
}

/// True when `text` is an event of type `kind`.
pub(crate) fn is_type(text: &str, kind: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(text)
        .is_ok_and(|v| v.get("type").and_then(serde_json::Value::as_str) == Some(kind))
}

fn read_events(
    stream: TcpStream,
    leftover: Vec<u8>,
    origin: Instant,
    stop: Arc<AtomicBool>,
    (ready, awaited): (Arc<AtomicBool>, &'static str),
) -> Result<Collected, EvalError> {
    let mut reader = FrameReader::new(stream, leftover);
    let mut events = Vec::new();
    let mut closed_by_server = false;
    while !stop.load(Ordering::Relaxed) {
        match reader.next_frame()? {
            Incoming::Frame(frame) if frame.opcode == OP_TEXT => {
                let text = String::from_utf8_lossy(&frame.payload).into_owned();
                if is_type(&text, awaited) {
                    ready.store(true, Ordering::Relaxed);
                }
                events.push((micros_since(origin), text));
            }
            Incoming::Frame(frame) if frame.opcode == OP_CLOSE => {
                closed_by_server = true;
                break;
            }
            Incoming::Frame(_) | Incoming::Idle => {}
            Incoming::Closed => {
                closed_by_server = true;
                break;
            }
        }
    }
    Ok(Collected {
        events,
        closed_by_server,
    })
}

/// Connects to `/ingest` and starts reading events. Times are measured from
/// `origin`.
pub fn open(host: &str, port: u16, origin: Instant) -> Result<Ingest, EvalError> {
    connect_at(host, port, (INGEST_PATH, "ready"), origin)
}

/// Connects to `/events` (server-sent events) and starts reading events,
/// sending no audio: the audio reaches the server some other way (the
/// loopback sink). The connection is up once the server's kept
/// `session_start` has arrived.
pub fn listen(host: &str, port: u16, origin: Instant) -> Result<Listener, EvalError> {
    Listener::open(host, port, origin)
}

/// `route` is the path and the event type that says the server is ready.
fn connect_at(
    host: &str,
    port: u16,
    route: (&'static str, &'static str),
    origin: Instant,
) -> Result<Ingest, EvalError> {
    let (path, awaited) = route;
    let connection = ws::connect(host, port, path, CONNECT_TIMEOUT)?;
    log::info(
        "eval.ingest.connected",
        "transport",
        "connection upgraded",
        &[("path", path.into())],
    );
    let stream = connection.stream;
    stream
        .set_read_timeout(Some(READ_TICK))
        .map_err(|e| EvalError::Protocol(format!("setting the read timeout: {e}")))?;
    let reading = stream
        .try_clone()
        .map_err(|e| EvalError::Protocol(format!("cloning the socket: {e}")))?;
    let stop = Arc::new(AtomicBool::new(false));
    let ready = Arc::new(AtomicBool::new(false));
    let reader = {
        let (stop, ready) = (Arc::clone(&stop), Arc::clone(&ready));
        let flag = (Arc::clone(&ready), awaited);
        std::thread::spawn(move || read_events(reading, connection.leftover, origin, stop, flag))
    };
    Ok(Ingest {
        stream,
        handshake: connection.handshake,
        origin,
        stop,
        ready,
        reader,
    })
}

fn send_error(error: std::io::Error) -> EvalError {
    EvalError::Protocol(format!("sending to the server: {error}"))
}

/// Waits until `ready` is set. Gives up at [`READY_TIMEOUT`], or as soon as
/// `gone` says the connection that would set it has ended.
pub(crate) fn wait_ready(
    ready: &AtomicBool,
    gone: impl Fn() -> bool,
    missing: &str,
) -> Result<(), EvalError> {
    let deadline = Instant::now() + READY_TIMEOUT;
    while !ready.load(Ordering::Relaxed) {
        if Instant::now() >= deadline || gone() {
            return Err(EvalError::Protocol(missing.to_owned()));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

impl Ingest {
    /// Sends the config message and waits for the server's `ready`.
    pub fn start(&mut self) -> Result<(), EvalError> {
        self.stream
            .write_all(&ws::client_frame(OP_TEXT, CONFIG_JSON.as_bytes()))
            .map_err(send_error)?;
        wait_ready(
            &self.ready,
            || self.reader.is_finished(),
            "the server sent no ready event after the config",
        )?;
        log::info("eval.ingest.ready", "transport", "server is ready", &[]);
        Ok(())
    }

    /// Sends `plan` paced to real time from now. Returns, for each chunk,
    /// the microseconds since the connection's origin at which its last
    /// byte had been written.
    pub fn play(&mut self, plan: &Plan, audio: &[i16]) -> Result<Vec<i64>, EvalError> {
        let begun = Instant::now();
        let mut sent_us = Vec::with_capacity(plan.chunks.len());
        for chunk in &plan.chunks {
            let due = begun
                + Duration::from_micros(chunk.end as u64 * 1_000_000 / u64::from(SAMPLE_RATE));
            if let Some(wait) = due.checked_duration_since(Instant::now()) {
                std::thread::sleep(wait);
            }
            let pcm = chunk_pcm(plan, audio, chunk);
            self.stream
                .write_all(&ws::client_frame(ws::OP_BINARY, &pcm))
                .map_err(send_error)?;
            sent_us.push(micros_since(self.origin));
        }
        Ok(sent_us)
    }

    /// Waits `drain` for late events, closes, and returns everything heard.
    pub fn finish(self, drain: Duration) -> Result<Received, EvalError> {
        std::thread::sleep(drain);
        // The server may already have closed; a failed close frame is not news.
        let _ = (&self.stream).write_all(&ws::client_frame(OP_CLOSE, &[]));
        self.stop.store(true, Ordering::Relaxed);
        let collected = self
            .reader
            .join()
            .map_err(|_| EvalError::Protocol(String::from("the reader thread panicked")))??;
        // Closing a socket the peer already closed is not an error worth a report.
        let _ = self.stream.shutdown(Shutdown::Both);
        Ok(Received {
            handshake: self.handshake,
            events: collected.events,
            closed_by_server: collected.closed_by_server,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::TcpListener;

    #[test]
    fn chunks_tile_the_stream_without_gaps() {
        let plan = plan(1_000, &[], 3, 400);
        assert_eq!(plan.chunks.first().map(|c| c.start), Some(0));
        assert_eq!(plan.chunks.last().map(|c| c.end), Some(3 * 1_400));
        assert!(plan.chunks.windows(2).all(|w| w[0].end == w[1].start));
        assert!(
            plan.chunks
                .iter()
                .all(|c| c.end - c.start <= CHUNK_SAMPLES && c.end > c.start)
        );
    }

    #[test]
    fn a_mark_is_always_the_edge_of_a_chunk() {
        let plan = plan(1_000, &[130, 777], 2, 400);
        for position in [130, 777, 1_400 + 130, 1_400 + 777] {
            assert!(
                plan.chunks.iter().any(|c| c.start == position),
                "start {position}"
            );
            assert!(
                plan.chunks.iter().any(|c| c.end == position),
                "end {position}"
            );
        }
    }

    #[test]
    fn the_end_of_the_recording_is_always_an_edge() {
        let plan = plan(1_000, &[], 1, 400);
        assert!(plan.chunks.iter().any(|c| c.end == 1_000));
        assert!(plan.chunks.iter().any(|c| c.start == 1_000));
    }

    #[test]
    fn no_repeats_make_no_chunks() {
        assert!(plan(1_000, &[], 0, 400).chunks.is_empty());
    }

    #[test]
    fn audio_chunks_carry_little_endian_samples_and_silence_is_zero() {
        let audio: Vec<i16> = (0..10).collect();
        let plan = plan(10, &[], 2, 5);
        let first = plan.chunks[0];
        assert_eq!(chunk_pcm(&plan, &audio, &first)[..4], [0, 0, 1, 0]);
        let silent = Chunk { start: 10, end: 15 };
        assert_eq!(chunk_pcm(&plan, &audio, &silent), vec![0_u8; 10]);
        let second_copy = Chunk { start: 15, end: 20 };
        assert_eq!(chunk_pcm(&plan, &audio, &second_copy)[..2], [0, 0]);
        assert_eq!(chunk_pcm(&plan, &audio, &second_copy)[2..4], [1, 0]);
    }

    #[test]
    fn the_send_time_of_a_position_is_that_of_the_chunk_with_that_edge() {
        let plan = plan(1_000, &[500], 1, 0);
        let times: Vec<i64> = (0..plan.chunks.len() as i64).map(|i| i * 10).collect();
        let at_start = sent_at(&plan, &times, 500, false);
        let at_end = sent_at(&plan, &times, 500, true);
        assert_eq!(at_start.map(|t| t / 10), at_end.map(|t| t / 10 + 1));
        assert_eq!(sent_at(&plan, &times, 501, true), None);
    }

    #[test]
    fn the_awaited_event_is_recognised() {
        assert!(is_type(r#"{"type":"ready","sr":16000}"#, "ready"));
        assert!(!is_type(r#"{"type":"final","text":"a"}"#, "ready"));
        assert!(!is_type("not json", "ready"));
        assert!(is_type(r#"{"type":"session_start"}"#, "session_start"));
        assert!(!is_type(r#"{"type":"session_start"}"#, "ready"));
    }

    /// Accepts one connection, answers the upgrade, sends `ready` after the
    /// config and one `partial` after the first audio frame, and counts the
    /// audio bytes until the client closes. Returns that count.
    fn serve_ingest(listener: TcpListener) -> JoinHandle<Result<usize, EvalError>> {
        std::thread::spawn(move || {
            let (mut socket, _) = listener
                .accept()
                .map_err(|e| EvalError::Protocol(e.to_string()))?;
            let mut request = [0_u8; 2048];
            let n = socket
                .read(&mut request)
                .map_err(|e| EvalError::Protocol(e.to_string()))?;
            let text = String::from_utf8_lossy(&request[..n]).into_owned();
            let key = text
                .lines()
                .find_map(|l| l.strip_prefix("Sec-WebSocket-Key: "))
                .ok_or_else(|| EvalError::Protocol(String::from("no key")))?;
            let reply = format!(
                "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
                 Sec-WebSocket-Accept: {}\r\n\r\n",
                ws::accept_key(key)
            );
            socket
                .write_all(reply.as_bytes())
                .map_err(|e| EvalError::Protocol(e.to_string()))?;
            socket
                .set_read_timeout(Some(Duration::from_secs(10)))
                .map_err(|e| EvalError::Protocol(e.to_string()))?;
            let mut reader = FrameReader::new(
                socket
                    .try_clone()
                    .map_err(|e| EvalError::Protocol(e.to_string()))?,
                Vec::new(),
            );
            let mut audio_bytes = 0;
            let mut sent_partial = false;
            loop {
                match reader.next_frame()? {
                    Incoming::Frame(frame) if frame.opcode == OP_TEXT => {
                        let ready =
                            ws::encode_frame(OP_TEXT, br#"{"type":"ready","sr":16000}"#, None);
                        socket
                            .write_all(&ready)
                            .map_err(|e| EvalError::Protocol(e.to_string()))?;
                    }
                    Incoming::Frame(frame) if frame.opcode == ws::OP_BINARY => {
                        audio_bytes += frame.payload.len();
                        if !sent_partial {
                            sent_partial = true;
                            let partial = ws::encode_frame(
                                OP_TEXT,
                                br#"{"type":"partial","text":"a"}"#,
                                None,
                            );
                            socket
                                .write_all(&partial)
                                .map_err(|e| EvalError::Protocol(e.to_string()))?;
                        }
                    }
                    Incoming::Frame(_) | Incoming::Idle => {}
                    Incoming::Closed => return Ok(audio_bytes),
                }
            }
        })
    }

    #[test]
    fn a_short_recording_is_played_and_its_events_are_stamped()
    -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let server = serve_ingest(listener);
        let origin = Instant::now();
        let mut ingest = open("127.0.0.1", port, origin)?;
        ingest.start()?;
        let audio = vec![100_i16; 1_600];
        let plan = plan(audio.len(), &[800], 1, 320);
        let sent = ingest.play(&plan, &audio)?;
        assert_eq!(sent.len(), plan.chunks.len());
        assert!(sent.windows(2).all(|w| w[0] <= w[1]));
        let received = ingest.finish(Duration::from_millis(300))?;
        let types: Vec<&str> = received
            .events
            .iter()
            .map(|(_, text)| {
                if text.contains("ready") {
                    "ready"
                } else {
                    "partial"
                }
            })
            .collect();
        assert_eq!(types, ["ready", "partial"]);
        assert_eq!(received.handshake.response_status, 101);
        let heard = server.join().map_err(|_| "server thread panicked")??;
        assert!(heard >= audio.len() * 2, "server heard {heard} bytes");
        Ok(())
    }

    #[test]
    fn opening_a_closed_port_is_a_connect_error() -> Result<(), std::io::Error> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        drop(listener);
        assert!(matches!(
            open("127.0.0.1", port, Instant::now()),
            Err(EvalError::Connect { .. })
        ));
        Ok(())
    }
}

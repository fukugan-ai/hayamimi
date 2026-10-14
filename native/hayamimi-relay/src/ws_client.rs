//! The client half of RFC 6455 that the recognizer's `/ingest` speaks: one
//! upgrade request, then unfragmented text and binary frames. Client frames
//! are masked, server frames are not.
//!
//! The peer is a recognizer this process has just started, on the loopback
//! address, so the handshake sends a fixed key and takes the status line as
//! the answer: `Sec-WebSocket-Accept` guards against a caching proxy, and
//! there is none on this path. Pings from the server are not answered.

use std::hash::{BuildHasher, RandomState};
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

pub const OP_TEXT: u8 = 0x1;
pub const OP_BINARY: u8 = 0x2;
pub const OP_CLOSE: u8 = 0x8;

/// The sample key of RFC 6455.
const KEY: &str = "dGhlIHNhbXBsZSBub25jZQ==";
/// A frame larger than this is not something the recognizer sends.
const MAX_PAYLOAD: u64 = 16 << 20;
/// The longest response head read before the upgrade is given up on.
const MAX_HEAD: usize = 16 << 10;

/// One decoded frame.
#[derive(Debug, PartialEq, Eq)]
pub struct Frame {
    pub opcode: u8,
    pub payload: Vec<u8>,
}

fn invalid(what: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, what)
}

/// One frame with FIN set. `mask` is the four-byte key of a client frame.
pub fn encode_frame(opcode: u8, payload: &[u8], mask: Option<[u8; 4]>) -> Vec<u8> {
    let mask_bit = if mask.is_some() { 0x80 } else { 0 };
    let mut frame = Vec::with_capacity(payload.len() + 14);
    frame.push(0x80 | (opcode & 0x0f));
    match (payload.len(), u16::try_from(payload.len())) {
        (0..=125, _) => frame.push(payload.len() as u8 | mask_bit),
        (_, Ok(short)) => {
            frame.push(126 | mask_bit);
            frame.extend_from_slice(&short.to_be_bytes());
        }
        _ => {
            frame.push(127 | mask_bit);
            frame.extend_from_slice(&(payload.len() as u64).to_be_bytes());
        }
    }
    match mask {
        Some(key) => {
            frame.extend_from_slice(&key);
            frame.extend(payload.iter().zip(key.iter().cycle()).map(|(b, k)| b ^ k));
        }
        None => frame.extend_from_slice(payload),
    }
    frame
}

/// The payload length and the offset it ends at in the frame head.
fn length_of(buf: &[u8]) -> Result<Option<(u64, usize)>, io::Error> {
    let length = match buf[1] & 0x7f {
        126 => buf
            .get(2..4)
            .map(|b| (u64::from(u16::from_be_bytes([b[0], b[1]])), 4)),
        127 => match buf.get(2..10) {
            Some(b) => {
                let mut eight = [0_u8; 8];
                eight.copy_from_slice(b);
                Some((u64::from_be_bytes(eight), 10))
            }
            None => None,
        },
        short => Some((u64::from(short), 2)),
    };
    match length {
        Some((length, _)) if length > MAX_PAYLOAD => Err(invalid("a frame that large")),
        other => Ok(other),
    }
}

/// Decodes one frame from the front of `buf`. `Ok(None)` means more bytes are
/// needed; the second value is how many bytes the frame used.
pub fn decode_frame(buf: &[u8]) -> Result<Option<(Frame, usize)>, io::Error> {
    if buf.len() < 2 {
        return Ok(None);
    }
    if buf[0] & 0x80 == 0 || buf[0] & 0x0f == 0 {
        return Err(invalid("fragmented frames are not supported"));
    }
    let Some((length, mut at)) = length_of(buf)? else {
        return Ok(None);
    };
    let mut key = [0_u8; 4];
    let masked = buf[1] & 0x80 != 0;
    if masked {
        let Some(bytes) = buf.get(at..at + 4) else {
            return Ok(None);
        };
        key.copy_from_slice(bytes);
        at += 4;
    }
    // The cap on `length` keeps this within `usize` on a 32-bit target too.
    let end = at + length as usize;
    let Some(body) = buf.get(at..end) else {
        return Ok(None);
    };
    let payload = if masked {
        body.iter()
            .zip(key.iter().cycle())
            .map(|(b, k)| b ^ k)
            .collect()
    } else {
        body.to_vec()
    };
    let frame = Frame {
        opcode: buf[0] & 0x0f,
        payload,
    };
    Ok(Some((frame, end)))
}

/// What waiting for the next frame produced.
#[derive(Debug, PartialEq, Eq)]
pub enum Incoming {
    Frame(Frame),
    /// The deadline passed with no complete frame.
    Idle,
    /// The peer closed the connection.
    Closed,
}

/// An upgraded connection and the bytes read past what was decoded.
pub struct Socket {
    stream: TcpStream,
    buf: Vec<u8>,
}

/// The upgrade request for `path`.
fn upgrade_request(port: u16, path: &str) -> String {
    format!(
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\n\
         Connection: Upgrade\r\nSec-WebSocket-Key: {KEY}\r\nSec-WebSocket-Version: 13\r\n\r\n"
    )
}

/// Reads up to the blank line that ends an HTTP head. Returns the head and
/// whatever followed it in the same reads.
fn read_head(stream: &mut TcpStream) -> io::Result<(String, Vec<u8>)> {
    let mut buf: Vec<u8> = Vec::new();
    loop {
        if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..at]).into_owned();
            return Ok((head, buf[at + 4..].to_vec()));
        }
        if buf.len() > MAX_HEAD {
            return Err(invalid("the response head is too long"));
        }
        let mut chunk = [0_u8; 1024];
        match stream.read(&mut chunk)? {
            0 => return Err(invalid("the server closed during the upgrade")),
            n => buf.extend_from_slice(&chunk[..n]),
        }
    }
}

/// Whether a response head answers the upgrade with status 101.
fn is_switching(head: &str) -> bool {
    head.lines()
        .next()
        .and_then(|status| status.split_whitespace().nth(1))
        .is_some_and(|code| code == "101")
}

/// Opens `127.0.0.1:port` and upgrades `path` to a WebSocket.
pub fn connect(port: u16, path: &str, timeout: Duration) -> io::Result<Socket> {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&address, timeout)?;
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.write_all(upgrade_request(port, path).as_bytes())?;
    let (head, buf) = read_head(&mut stream)?;
    if !is_switching(&head) {
        return Err(invalid("the server did not switch protocols"));
    }
    Ok(Socket { stream, buf })
}

impl Socket {
    /// Sends one masked frame.
    pub fn send(&mut self, opcode: u8, payload: &[u8]) -> io::Result<()> {
        let key = RandomState::new().hash_one(0_u8).to_le_bytes();
        let mask = [key[0], key[1], key[2], key[3]];
        self.stream
            .write_all(&encode_frame(opcode, payload, Some(mask)))
    }

    /// The next frame, `Idle` once `deadline` has passed, `Closed` when the
    /// peer has gone.
    pub fn next_before(&mut self, deadline: Instant) -> io::Result<Incoming> {
        use io::ErrorKind::{ConnectionAborted, ConnectionReset, TimedOut, WouldBlock};
        loop {
            if let Some((frame, used)) = decode_frame(&self.buf)? {
                self.buf.drain(..used);
                return Ok(Incoming::Frame(frame));
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(Incoming::Idle);
            }
            self.stream.set_read_timeout(Some(left))?;
            let mut chunk = [0_u8; 4096];
            match self.stream.read(&mut chunk) {
                Ok(0) => return Ok(Incoming::Closed),
                Ok(n) => self.buf.extend_from_slice(&chunk[..n]),
                Err(e) if matches!(e.kind(), WouldBlock | TimedOut) => return Ok(Incoming::Idle),
                Err(e) if matches!(e.kind(), ConnectionReset | ConnectionAborted) => {
                    return Ok(Incoming::Closed);
                }
                Err(e) => return Err(e),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn frames_round_trip_at_the_length_boundaries() -> io::Result<()> {
        for len in [0_usize, 125, 126, 65_535, 65_536] {
            let payload = vec![0xa5_u8; len];
            for mask in [None, Some([1, 2, 3, 4])] {
                let wire = encode_frame(OP_BINARY, &payload, mask);
                let decoded = decode_frame(&wire)?;
                let expected = Frame {
                    opcode: OP_BINARY,
                    payload: payload.clone(),
                };
                assert_eq!(decoded, Some((expected, wire.len())), "{len} {mask:?}");
            }
        }
        Ok(())
    }

    #[test]
    fn a_short_buffer_asks_for_more() -> io::Result<()> {
        let wire = encode_frame(OP_TEXT, b"hello", Some([9, 8, 7, 6]));
        for cut in 0..wire.len() {
            assert_eq!(decode_frame(&wire[..cut])?, None, "cut at {cut}");
        }
        Ok(())
    }

    #[test]
    fn a_fragmented_or_huge_frame_is_refused() {
        assert!(decode_frame(&[0x01, 0x00]).is_err());
        assert!(decode_frame(&[0x00, 0x00]).is_err());
        let mut huge = vec![0x82, 127];
        huge.extend_from_slice(&(MAX_PAYLOAD + 1).to_be_bytes());
        assert!(decode_frame(&huge).is_err());
    }

    #[test]
    fn two_frames_in_one_buffer_decode_one_at_a_time() -> io::Result<()> {
        let mut wire = encode_frame(OP_TEXT, b"a", None);
        let first = wire.len();
        wire.extend(encode_frame(OP_TEXT, b"b", None));
        let decoded = decode_frame(&wire)?;
        assert_eq!(
            decoded.map(|(frame, used)| (frame.payload, used)),
            Some((b"a".to_vec(), first))
        );
        Ok(())
    }

    #[test]
    fn only_a_101_status_switches() {
        assert!(is_switching(
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket"
        ));
        assert!(!is_switching("HTTP/1.1 404 Not Found"));
        assert!(!is_switching(""));
    }

    /// Answers one upgrade, sends a text frame and reads the client's frame.
    fn serve_once(listener: TcpListener) -> std::thread::JoinHandle<io::Result<Vec<u8>>> {
        std::thread::spawn(move || {
            let (mut peer, _) = listener.accept()?;
            read_head(&mut peer)?;
            peer.write_all(b"HTTP/1.1 101 Switching Protocols\r\n\r\n")?;
            peer.write_all(&encode_frame(OP_TEXT, b"hi", None))?;
            let mut buf = Vec::new();
            loop {
                let mut chunk = [0_u8; 64];
                let n = peer.read(&mut chunk)?;
                buf.extend_from_slice(&chunk[..n]);
                if let Some((frame, _)) = decode_frame(&buf)? {
                    return Ok(frame.payload);
                }
                if n == 0 {
                    return Err(invalid("the client left"));
                }
            }
        })
    }

    #[test]
    fn a_connection_sends_masked_frames_and_receives_plain_ones() -> io::Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let server = serve_once(listener);
        let mut socket = connect(port, "/ingest", Duration::from_secs(5))?;
        socket.send(OP_BINARY, b"pcm")?;
        let heard = socket.next_before(Instant::now() + Duration::from_secs(5))?;
        let expected = Frame {
            opcode: OP_TEXT,
            payload: b"hi".to_vec(),
        };
        assert_eq!(heard, Incoming::Frame(expected));
        let got = server
            .join()
            .map_err(|_| invalid("the server panicked"))??;
        assert_eq!(got, b"pcm");
        Ok(())
    }

    #[test]
    fn waiting_past_the_deadline_is_idle() -> io::Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let server = std::thread::spawn(move || -> io::Result<()> {
            let (mut peer, _) = listener.accept()?;
            read_head(&mut peer)?;
            peer.write_all(b"HTTP/1.1 101 Switching Protocols\r\n\r\n")?;
            // Hold the connection until the client has given up waiting.
            std::thread::sleep(Duration::from_millis(400));
            Ok(())
        });
        let mut socket = connect(port, "/ingest", Duration::from_secs(5))?;
        let heard = socket.next_before(Instant::now() + Duration::from_millis(50))?;
        assert_eq!(heard, Incoming::Idle);
        server
            .join()
            .map_err(|_| invalid("the server panicked"))??;
        Ok(())
    }

    #[test]
    fn a_port_nobody_listens_on_is_refused() -> io::Result<()> {
        let port = TcpListener::bind("127.0.0.1:0")?.local_addr()?.port();
        assert!(connect(port, "/ingest", Duration::from_secs(2)).is_err());
        Ok(())
    }
}

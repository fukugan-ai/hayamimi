//! The client half of RFC 6455 that the subtitle server's `/ingest` speaks:
//! one upgrade request, then unfragmented text and binary frames. Client
//! frames are masked, server frames are not.

use std::hash::{BuildHasher, RandomState};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use sha1::{Digest, Sha1};

use crate::error::EvalError;

pub const OP_TEXT: u8 = 0x1;
pub const OP_BINARY: u8 = 0x2;
pub const OP_CLOSE: u8 = 0x8;

const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
/// A frame larger than this is not something the server sends.
const MAX_PAYLOAD: usize = 64 << 20;
/// The longest response head read before the upgrade is given up on.
const MAX_HEAD: usize = 16 << 10;

/// One decoded frame.
#[derive(Debug, PartialEq, Eq)]
pub struct Frame {
    pub opcode: u8,
    pub payload: Vec<u8>,
}

/// What the upgrade exchange said, kept so a run can record it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Handshake {
    pub request_key: String,
    pub response_status: u16,
    pub response_accept: String,
}

/// The `Sec-WebSocket-Accept` value a server must answer `client_key` with.
pub fn accept_key(client_key: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(client_key.as_bytes());
    hasher.update(WS_GUID.as_bytes());
    STANDARD.encode(hasher.finalize())
}

/// Eight bytes that differ from run to run. Masking keys only have to be
/// unpredictable to an intermediary, which a loopback connection has none of.
fn random_u64() -> u64 {
    RandomState::new().hash_one(0_u8)
}

/// A fresh `Sec-WebSocket-Key`.
pub fn random_key() -> String {
    let mut bytes = [0_u8; 16];
    bytes[..8].copy_from_slice(&random_u64().to_le_bytes());
    bytes[8..].copy_from_slice(&random_u64().to_le_bytes());
    STANDARD.encode(bytes)
}

/// One frame with FIN set. `mask` is the four-byte key of a client frame.
pub fn encode_frame(opcode: u8, payload: &[u8], mask: Option<[u8; 4]>) -> Vec<u8> {
    let mask_bit = if mask.is_some() { 0x80 } else { 0 };
    let mut frame = Vec::with_capacity(payload.len() + 14);
    frame.push(0x80 | (opcode & 0x0f));
    match payload.len() {
        0..=125 => frame.push(payload.len() as u8 | mask_bit),
        126..=65535 => {
            frame.push(126 | mask_bit);
            frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        }
        _ => {
            frame.push(127 | mask_bit);
            frame.extend_from_slice(&(payload.len() as u64).to_be_bytes());
        }
    }
    match mask {
        Some(key) => {
            frame.extend_from_slice(&key);
            frame.extend(payload.iter().enumerate().map(|(i, b)| b ^ key[i % 4]));
        }
        None => frame.extend_from_slice(payload),
    }
    frame
}

/// A masked client frame with a fresh key.
pub fn client_frame(opcode: u8, payload: &[u8]) -> Vec<u8> {
    let key = (random_u64() as u32).to_le_bytes();
    encode_frame(opcode, payload, Some(key))
}

/// Decodes one frame from the front of `buf`. `Ok(None)` means more bytes are
/// needed; the second value is how many bytes the frame used.
pub fn decode_frame(buf: &[u8]) -> Result<Option<(Frame, usize)>, EvalError> {
    if buf.len() < 2 {
        return Ok(None);
    }
    if buf[0] & 0x80 == 0 || buf[0] & 0x0f == 0 {
        return Err(EvalError::Protocol(String::from(
            "fragmented frames are not supported",
        )));
    }
    let opcode = buf[0] & 0x0f;
    let masked = buf[1] & 0x80 != 0;
    let (length, mut pos) = match buf[1] & 0x7f {
        126 => {
            let Some(bytes) = buf.get(2..4) else {
                return Ok(None);
            };
            (usize::from(u16::from_be_bytes([bytes[0], bytes[1]])), 4)
        }
        127 => {
            let Some(bytes) = buf.get(2..10) else {
                return Ok(None);
            };
            let mut eight = [0_u8; 8];
            eight.copy_from_slice(bytes);
            let length = u64::from_be_bytes(eight);
            if length > MAX_PAYLOAD as u64 {
                return Err(EvalError::Protocol(format!("a frame of {length} bytes")));
            }
            (length as usize, 10)
        }
        short => (usize::from(short), 2),
    };
    let mut key = [0_u8; 4];
    if masked {
        let Some(bytes) = buf.get(pos..pos + 4) else {
            return Ok(None);
        };
        key.copy_from_slice(bytes);
        pos += 4;
    }
    let Some(body) = buf.get(pos..pos + length) else {
        return Ok(None);
    };
    let payload = if masked {
        body.iter()
            .enumerate()
            .map(|(i, b)| b ^ key[i % 4])
            .collect()
    } else {
        body.to_vec()
    };
    Ok(Some((Frame { opcode, payload }, pos + length)))
}

/// What one read from the socket produced.
#[derive(Debug, PartialEq, Eq)]
pub enum Incoming {
    Frame(Frame),
    /// The read timed out with no complete frame yet.
    Idle,
    /// The peer closed the connection.
    Closed,
}

/// Reassembles frames from a byte stream.
pub struct FrameReader<R> {
    inner: R,
    buf: Vec<u8>,
}

impl<R: Read> FrameReader<R> {
    /// A reader that starts with bytes already read past the upgrade head.
    pub fn new(inner: R, leftover: Vec<u8>) -> FrameReader<R> {
        FrameReader {
            inner,
            buf: leftover,
        }
    }

    /// The next frame, `Idle` on a read timeout, `Closed` on end of stream.
    pub fn next_frame(&mut self) -> Result<Incoming, EvalError> {
        use std::io::ErrorKind::{ConnectionAborted, ConnectionReset, TimedOut, WouldBlock};
        loop {
            if let Some((frame, used)) = decode_frame(&self.buf)? {
                self.buf.drain(..used);
                return Ok(Incoming::Frame(frame));
            }
            let mut chunk = [0_u8; 4096];
            match self.inner.read(&mut chunk) {
                Ok(0) => return Ok(Incoming::Closed),
                Ok(n) => self.buf.extend_from_slice(&chunk[..n]),
                Err(e) if matches!(e.kind(), WouldBlock | TimedOut) => return Ok(Incoming::Idle),
                Err(e) if matches!(e.kind(), ConnectionReset | ConnectionAborted) => {
                    return Ok(Incoming::Closed);
                }
                Err(e) => return Err(EvalError::Protocol(format!("socket read: {e}"))),
            }
        }
    }
}

/// The upgrade request for `path`.
pub fn upgrade_request(host: &str, port: u16, path: &str, key: &str) -> String {
    format!(
        "GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nUpgrade: websocket\r\n\
         Connection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
    )
}

/// The status code and the `Sec-WebSocket-Accept` value, if any, of an
/// upgrade response head (everything before the blank line).
pub fn parse_upgrade_response(head: &str) -> Result<(u16, Option<String>), EvalError> {
    let mut lines = head.split("\r\n");
    let not_status = || EvalError::Handshake(format!("not an HTTP status line: {head:?}"));
    let Some(status_line) = lines.next() else {
        return Err(not_status());
    };
    let mut parts = status_line.split_whitespace();
    let (Some(version), Some(code)) = (parts.next(), parts.next()) else {
        return Err(not_status());
    };
    if !version.starts_with("HTTP/") {
        return Err(not_status());
    }
    let Ok(status) = code.parse::<u16>() else {
        return Err(EvalError::Handshake(format!("not a status code: {code:?}")));
    };
    let accept = lines
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.trim().eq_ignore_ascii_case("sec-websocket-accept"))
        .map(|(_, value)| value.trim().to_owned());
    Ok((status, accept))
}

/// An open `/ingest`-style connection: the socket, the upgrade exchange and
/// any bytes that arrived behind the response head.
pub struct Connection {
    pub stream: TcpStream,
    pub handshake: Handshake,
    pub leftover: Vec<u8>,
}

/// Opens `host:port`, upgrades `path` to a WebSocket and checks the answer.
pub fn connect(
    host: &str,
    port: u16,
    path: &str,
    timeout: Duration,
) -> Result<Connection, EvalError> {
    let address = format!("{host}:{port}");
    let connect_error = |source| EvalError::Connect {
        address: address.clone(),
        source,
    };
    let target = address
        .to_socket_addrs()
        .map_err(connect_error)?
        .next()
        .ok_or_else(|| EvalError::Invalid(format!("{address} resolves to no address")))?;
    let mut stream = TcpStream::connect_timeout(&target, timeout).map_err(connect_error)?;
    stream.set_nodelay(true).map_err(connect_error)?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(connect_error)?;
    let key = random_key();
    stream
        .write_all(upgrade_request(host, port, path, &key).as_bytes())
        .map_err(connect_error)?;
    let (head, leftover) = read_head(&mut stream)?;
    let (status, accept) = parse_upgrade_response(&head)?;
    if status != 101 {
        return Err(EvalError::Handshake(format!(
            "the server answered {status}"
        )));
    }
    let Some(accept) = accept else {
        return Err(EvalError::Handshake(String::from(
            "the 101 response has no Sec-WebSocket-Accept",
        )));
    };
    if accept != accept_key(&key) {
        return Err(EvalError::Handshake(String::from(
            "Sec-WebSocket-Accept does not match the key sent",
        )));
    }
    Ok(Connection {
        stream,
        handshake: Handshake {
            request_key: key,
            response_status: status,
            response_accept: accept,
        },
        leftover,
    })
}

/// Reads up to the blank line that ends an HTTP head. Returns the head and
/// whatever followed it in the same reads.
fn read_head(stream: &mut TcpStream) -> Result<(String, Vec<u8>), EvalError> {
    let mut buf: Vec<u8> = Vec::new();
    loop {
        if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..at]).into_owned();
            return Ok((head, buf[at + 4..].to_vec()));
        }
        if buf.len() > MAX_HEAD {
            return Err(EvalError::Handshake(String::from(
                "the response head is too long",
            )));
        }
        let mut chunk = [0_u8; 1024];
        match stream.read(&mut chunk) {
            Ok(0) => {
                return Err(EvalError::Handshake(String::from(
                    "the server closed during the upgrade",
                )));
            }
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) => {
                return Err(EvalError::Handshake(format!(
                    "reading the upgrade response: {e}"
                )));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn the_accept_key_matches_the_rfc_example() {
        assert_eq!(
            accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    #[test]
    fn a_random_key_is_16_bytes_of_base64() -> Result<(), base64::DecodeError> {
        assert_eq!(STANDARD.decode(random_key())?.len(), 16);
        Ok(())
    }

    #[test]
    fn two_random_keys_differ() {
        assert_ne!(random_key(), random_key());
    }

    #[test]
    fn frames_round_trip_at_the_length_boundaries() -> Result<(), EvalError> {
        for len in [0_usize, 125, 126, 65_535, 65_536] {
            let payload = vec![0xa5_u8; len];
            for mask in [None, Some([1, 2, 3, 4])] {
                let wire = encode_frame(OP_BINARY, &payload, mask);
                let Some((frame, used)) = decode_frame(&wire)? else {
                    panic!("a complete frame of {len} bytes did not decode");
                };
                assert_eq!(used, wire.len());
                assert_eq!(
                    frame,
                    Frame {
                        opcode: OP_BINARY,
                        payload: payload.clone()
                    }
                );
            }
        }
        Ok(())
    }

    #[test]
    fn a_client_frame_is_masked_and_decodes_to_its_payload() -> Result<(), EvalError> {
        let wire = client_frame(OP_TEXT, b"hello");
        assert_eq!(wire[1] & 0x80, 0x80);
        let Some((frame, _)) = decode_frame(&wire)? else {
            panic!("a complete client frame did not decode");
        };
        assert_eq!(frame.payload, b"hello");
        Ok(())
    }

    #[test]
    fn a_short_buffer_asks_for_more() -> Result<(), EvalError> {
        let wire = encode_frame(OP_TEXT, b"hello", None);
        for cut in 0..wire.len() {
            assert!(decode_frame(&wire[..cut])?.is_none(), "cut at {cut}");
        }
        Ok(())
    }

    #[test]
    fn a_fragmented_frame_is_refused() {
        assert!(matches!(
            decode_frame(&[0x01, 0x00]),
            Err(EvalError::Protocol(_))
        ));
        assert!(matches!(
            decode_frame(&[0x00, 0x00]),
            Err(EvalError::Protocol(_))
        ));
    }

    fn text_frame(text: &[u8]) -> Frame {
        Frame {
            opcode: OP_TEXT,
            payload: text.to_vec(),
        }
    }

    #[test]
    fn two_frames_in_one_read_come_out_one_at_a_time() -> Result<(), EvalError> {
        let mut wire = encode_frame(OP_TEXT, b"a", None);
        wire.extend(encode_frame(OP_TEXT, b"b", None));
        let mut reader = FrameReader::new(std::io::Cursor::new(wire), Vec::new());
        assert_eq!(reader.next_frame()?, Incoming::Frame(text_frame(b"a")));
        assert_eq!(reader.next_frame()?, Incoming::Frame(text_frame(b"b")));
        assert_eq!(reader.next_frame()?, Incoming::Closed);
        Ok(())
    }

    #[test]
    fn bytes_left_over_from_the_head_are_decoded_first() -> Result<(), EvalError> {
        let wire = encode_frame(OP_TEXT, b"x", None);
        let mut reader = FrameReader::new(std::io::empty(), wire);
        assert_eq!(reader.next_frame()?, Incoming::Frame(text_frame(b"x")));
        Ok(())
    }

    #[test]
    fn a_101_response_yields_status_and_accept() -> Result<(), EvalError> {
        let head =
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nSec-WebSocket-Accept: abc=";
        assert_eq!(
            parse_upgrade_response(head)?,
            (101, Some(String::from("abc=")))
        );
        Ok(())
    }

    #[test]
    fn a_404_response_has_no_accept() -> Result<(), EvalError> {
        assert_eq!(
            parse_upgrade_response("HTTP/1.1 404 Not Found")?,
            (404, None)
        );
        Ok(())
    }

    #[test]
    fn text_that_is_not_http_is_refused() {
        assert!(matches!(
            parse_upgrade_response("hello"),
            Err(EvalError::Handshake(_))
        ));
    }

    #[test]
    fn connecting_to_a_closed_port_is_a_connect_error() -> Result<(), std::io::Error> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        drop(listener);
        let result = connect("127.0.0.1", port, "/ingest", Duration::from_secs(2));
        assert!(matches!(result, Err(EvalError::Connect { .. })));
        Ok(())
    }

    /// A one-connection server that answers the upgrade with `reply(key)`.
    fn serve_once(
        listener: TcpListener,
        reply: fn(&str) -> String,
    ) -> std::thread::JoinHandle<Result<(), std::io::Error>> {
        std::thread::spawn(move || {
            let (mut socket, _) = listener.accept()?;
            let mut request = [0_u8; 2048];
            let n = socket.read(&mut request)?;
            let text = String::from_utf8_lossy(&request[..n]).into_owned();
            let key = text
                .lines()
                .find_map(|l| l.strip_prefix("Sec-WebSocket-Key: "))
                .map(str::to_owned)
                .ok_or_else(|| std::io::Error::other("no key in the request"))?;
            socket.write_all(reply(&key).as_bytes())
        })
    }

    fn honest_reply(key: &str) -> String {
        format!(
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
             Sec-WebSocket-Accept: {}\r\n\r\n",
            accept_key(key)
        )
    }

    fn wrong_reply(_key: &str) -> String {
        String::from("HTTP/1.1 101 Switching Protocols\r\nSec-WebSocket-Accept: wrong\r\n\r\n")
    }

    #[test]
    fn a_server_that_upgrades_is_accepted() -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let server = serve_once(listener, honest_reply);
        let connection = connect("127.0.0.1", port, "/ingest", Duration::from_secs(5))?;
        server.join().map_err(|_| "server thread panicked")??;
        assert_eq!(connection.handshake.response_status, 101);
        assert_eq!(
            connection.handshake.response_accept,
            accept_key(&connection.handshake.request_key)
        );
        Ok(())
    }

    #[test]
    fn a_wrong_accept_is_a_handshake_error() -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let server = serve_once(listener, wrong_reply);
        let result = connect("127.0.0.1", port, "/ingest", Duration::from_secs(5));
        server.join().map_err(|_| "server thread panicked")??;
        assert!(matches!(result, Err(EvalError::Handshake(_))));
        Ok(())
    }
}

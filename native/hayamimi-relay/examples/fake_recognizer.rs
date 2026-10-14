//! A stand-in for `hayamimi` in the tests of `media`: it takes the options of
//! `--input ws --ws-port <port>`, serves `/ingest` to one client, and plays
//! the part of the recognizer with the audio it is sent.
//!
//! The "audio" of a test is the text of the events to answer with, one JSON
//! object per line, carried as the samples of a wav file; this program
//! collects the samples, and when a frame of nothing but zeros comes after
//! them (the silence a client ends with) it sends each line as a text frame.
//! A line that starts with `#` is not an event, and a first line `#exit=<n>`
//! makes the process end with that code as soon as the samples reach it,
//! with nothing sent. `--models <dir>` is taken as a scratch directory: the
//! process id is written to `<dir>/fake.pid`, so that a test can look for
//! the process after the step has ended.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::ExitCode;

use hayamimi_relay::ws_client::{OP_BINARY, OP_CLOSE, OP_TEXT, decode_frame, encode_frame};

const SWITCH: &str = "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\
                      Connection: Upgrade\r\nSec-WebSocket-Accept: unchecked\r\n\r\n";
const READY: &str = r#"{"type":"ready","sr":16000}"#;

fn value_after(args: &[String], flag: &str) -> Option<String> {
    let at = args.iter().position(|arg| arg == flag)?;
    args.get(at + 1).cloned()
}

/// Reads the request head of the client, up to its blank line.
fn read_head(stream: &mut TcpStream) -> std::io::Result<()> {
    let mut seen = Vec::new();
    let mut byte = [0_u8; 1];
    while !seen.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte)?;
        seen.push(byte[0]);
    }
    Ok(())
}

/// The code a first line `#exit=<n>` asks for.
fn exit_code_of(text: &str) -> Option<u8> {
    text.lines()
        .next()?
        .strip_prefix("#exit=")?
        .parse::<u8>()
        .ok()
}

fn send_text(stream: &mut TcpStream, text: &str) -> std::io::Result<()> {
    stream.write_all(&encode_frame(OP_TEXT, text.as_bytes(), None))
}

/// What the connection has come to so far.
#[derive(Default)]
struct Heard {
    samples: Vec<u8>,
    answered: bool,
}

/// What one frame from the client leads to.
enum Step {
    Go,
    Leave,
    Exit(u8),
}

fn on_audio(stream: &mut TcpStream, heard: &mut Heard, audio: &[u8]) -> std::io::Result<Step> {
    let silent = audio.iter().all(|byte| *byte == 0);
    if !silent {
        heard.samples.extend_from_slice(audio);
        let text = String::from_utf8_lossy(&heard.samples);
        return Ok(exit_code_of(&text).map_or(Step::Go, Step::Exit));
    }
    if !heard.answered && !heard.samples.is_empty() {
        heard.answered = true;
        let text = String::from_utf8_lossy(&heard.samples).into_owned();
        for event in text.lines().filter(|line| !line.starts_with('#')) {
            send_text(stream, event.trim_end())?;
        }
    }
    Ok(Step::Go)
}

fn on_frame(
    stream: &mut TcpStream,
    heard: &mut Heard,
    opcode: u8,
    payload: &[u8],
) -> std::io::Result<Step> {
    match opcode {
        OP_TEXT => send_text(stream, READY).map(|()| Step::Go),
        OP_BINARY => on_audio(stream, heard, payload),
        OP_CLOSE => Ok(Step::Leave),
        _ => Ok(Step::Go),
    }
}

/// Serves the one client until it leaves; the code the samples asked for.
fn converse(stream: &mut TcpStream) -> Result<Option<u8>, String> {
    let io = |error: std::io::Error| error.to_string();
    read_head(stream).map_err(io)?;
    stream.write_all(SWITCH.as_bytes()).map_err(io)?;
    let (mut buf, mut heard) = (Vec::new(), Heard::default());
    loop {
        while let Some((frame, used)) = decode_frame(&buf).map_err(io)? {
            buf.drain(..used);
            match on_frame(stream, &mut heard, frame.opcode, &frame.payload).map_err(io)? {
                Step::Go => {}
                Step::Leave => return Ok(None),
                Step::Exit(code) => return Ok(Some(code)),
            }
        }
        let mut chunk = [0_u8; 4096];
        match stream.read(&mut chunk).map_err(io)? {
            0 => return Ok(None),
            n => buf.extend_from_slice(&chunk[..n]),
        }
    }
}

fn serve(args: &[String]) -> Result<Option<u8>, String> {
    let port = value_after(args, "--ws-port").ok_or("no --ws-port")?;
    let port: u16 = port.parse().map_err(|_| "bad port")?;
    if let Some(dir) = value_after(args, "--models") {
        std::fs::write(format!("{dir}/fake.pid"), std::process::id().to_string())
            .map_err(|error| error.to_string())?;
    }
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|error| error.to_string())?;
    let (mut stream, _) = listener.accept().map_err(|error| error.to_string())?;
    converse(&mut stream)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match serve(&args) {
        Ok(Some(code)) => ExitCode::from(code),
        Ok(None) => ExitCode::SUCCESS,
        Err(reason) => {
            eprintln!("fake recognizer: {reason}");
            ExitCode::from(2)
        }
    }
}

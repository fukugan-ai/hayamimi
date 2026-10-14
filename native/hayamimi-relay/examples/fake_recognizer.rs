//! A stand-in for `hayamimi` in the tests of `media`: it serves the events in
//! the file given as `--wav` on `GET /events` to the first client that comes,
//! and ends.
//!
//! The file holds one event (a JSON object) per line; a line that starts with
//! `#` is not an event. A first line `#exit=<n>` makes the process end with
//! that code. The process id is written to `<file>.pid`, so that a test can
//! look for the process after the step has ended.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::ExitCode;

const HEAD: &str = "HTTP/1.0 200 OK\r\nContent-Type: text/event-stream\r\n\r\n";

fn value_after(args: &[String], flag: &str) -> Option<String> {
    let at = args.iter().position(|arg| arg == flag)?;
    args.get(at + 1).cloned()
}

/// Reads the request head of the client, up to its blank line.
fn read_head(stream: &mut std::net::TcpStream) -> std::io::Result<()> {
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

/// Serves the events; the code the file asked for, when it asked for one.
fn serve(args: &[String]) -> Result<Option<u8>, String> {
    let wav = value_after(args, "--wav").ok_or("no --wav")?;
    let port = value_after(args, "--serve").ok_or("no --serve")?;
    let port: u16 = port.parse().map_err(|_| "bad port")?;
    let text = std::fs::read_to_string(&wav).map_err(|error| error.to_string())?;
    std::fs::write(format!("{wav}.pid"), std::process::id().to_string())
        .map_err(|error| error.to_string())?;
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|error| error.to_string())?;
    let (mut stream, _) = listener.accept().map_err(|error| error.to_string())?;
    read_head(&mut stream).map_err(|error| error.to_string())?;
    stream
        .write_all(HEAD.as_bytes())
        .map_err(|error| error.to_string())?;
    for event in text.lines().filter(|line| !line.starts_with('#')) {
        write!(stream, "data: {event}\n\n").map_err(|error| error.to_string())?;
    }
    stream.flush().map_err(|error| error.to_string())?;
    Ok(exit_code_of(&text))
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

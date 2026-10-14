//! A stand-in for llama.cpp's `llama-server`, for the tests of
//! `hayamimi llama start`. It is not part of the product.
//!
//! It takes the arguments the real server takes and reads `--port` and `-m`.
//! What it does depends on the name of the model file, which the test names:
//!
//! - `exits.gguf`: ends at once with code 3, as a server that cannot start does;
//! - `silent.gguf`: runs and never listens, as a server that never finishes loading does;
//! - anything else: answers every request with `{"status":"ok"}` until it is stopped.
//!
//! It prints its arguments on the first line, so the log the start gives it
//! shows how it was started.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::process::ExitCode;
use std::time::Duration;

const ANSWER: &str = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 15\r\nConnection: close\r\n\r\n{\"status\":\"ok\"}";

/// The value that follows `name` in the arguments.
fn value_after<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    let at = args.iter().position(|arg| arg == name)?;
    args.get(at + 1).map(String::as_str)
}

/// Reads the request and answers it.
fn answer(mut stream: TcpStream) {
    let mut request = [0_u8; 2048];
    // A client that sent nothing, or hung up, gets no answer.
    if stream.read(&mut request).is_err() {
        return;
    }
    // A client that hung up has no use for the answer.
    let _ = stream.write_all(ANSWER.as_bytes());
}

fn serve(port: u16) -> ExitCode {
    let Ok(listener) = TcpListener::bind((Ipv4Addr::LOCALHOST, port)) else {
        return ExitCode::from(1);
    };
    for stream in listener.incoming().flatten() {
        answer(stream);
    }
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    println!("fake-llama-server {}", args.join(" "));
    let model = value_after(&args, "-m");
    if model.is_some_and(|name| name.ends_with("exits.gguf")) {
        return ExitCode::from(3);
    }
    let Some(port) = value_after(&args, "--port").and_then(|text| text.parse::<u16>().ok()) else {
        return ExitCode::from(2);
    };
    if model.is_some_and(|name| name.ends_with("silent.gguf")) {
        loop {
            std::thread::sleep(Duration::from_secs(1));
        }
    }
    serve(port)
}

//! Serves the page bundle and a scripted `/events` stream, so that the three
//! pages can be opened in a browser before the real server serves them.
//!
//! ```text
//! cargo run --release -p hayamimi-ui --example preview -- <bundle dir> <port>
//! ```
//!
//! `/`, `/dashboard` and `/transcript` give `web/index.html`; `/ui/<file>` gives
//! a file of the bundle directory (its `snippets/` too); `/events` sends a short session, one message
//! every 300 ms, and then keeps the connection open.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

const INDEX: &str = include_str!("../web/index.html");

const SESSION: [&str; 12] = [
    r#"{"type":"fallback","stage":"translate","reason":"unreachable","to":"cpu"}"#,
    r#"{"type":"session_start"}"#,
    r#"{"type":"partial","text":"予算は五百万"}"#,
    r#"{"type":"translation","of":"partial","lang":"en","text":"The budget is five million","status":"ok"}"#,
    r#"{"type":"partial","text":"予算は五百万円を想定"}"#,
    r#"{"type":"translation","of":"partial","lang":"en","text":"The budget is five million yen expected","status":"ok"}"#,
    r#"{"type":"final","id":"mic-1","text":"予算は五百万円を想定しています。","lang":"ja","speaker":"","latency_ms":412.6}"#,
    r#"{"type":"translation","of":"final","id":"mic-1","lang":"en","text":"We expect 5M yen.","status":"ok","settled":false}"#,
    r#"{"type":"speaker","id":"mic-1","speaker":"S2"}"#,
    r#"{"type":"translation","of":"final","id":"mic-1","lang":"en","text":"We are planning on a budget of 5M yen.","status":"ok","settled":true,"annotations":[{"start":0,"end":38,"text":" (about $33,000)"}]}"#,
    r#"{"type":"refine","id":"mic-1","text":"予算は五百万円を想定しています。","lang":"ja","speaker":"S2"}"#,
    r#"{"type":"translation","of":"refine","id":"mic-1","lang":"en","text":"The budget is assumed to be 5M yen.","status":"ok","settled":true}"#,
];

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(bundle), Some(port)) = (args.next(), args.next()) else {
        eprintln!("usage: preview <bundle dir> <port>");
        std::process::exit(2);
    };
    let Ok(port) = port.parse::<u16>() else {
        eprintln!("the port is not a number: {port}");
        std::process::exit(2);
    };
    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("cannot listen on {port}: {error}");
            std::process::exit(1);
        }
    };
    let bundle = PathBuf::from(bundle);
    for stream in listener.incoming().flatten() {
        let bundle = bundle.clone();
        std::thread::spawn(move || {
            if let Err(error) = serve(stream, &bundle) {
                eprintln!("a connection ended: {error}");
            }
        });
    }
}

/// The path of the request line of `head`, without its query.
fn path_of(head: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(head);
    let target = text.split_whitespace().nth(1)?;
    let path = target.split('?').next()?;
    Some(path.to_owned())
}

fn serve(mut stream: TcpStream, bundle: &Path) -> std::io::Result<()> {
    let mut head = Vec::new();
    let mut byte = [0_u8; 1];
    while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte)? == 1 {
        head.push(byte[0]);
    }
    let Some(path) = path_of(&head) else {
        return not_found(&mut stream);
    };
    match path.as_str() {
        "/" | "/dashboard" | "/transcript" => {
            reply(&mut stream, "text/html; charset=utf-8", INDEX.as_bytes())
        }
        "/events" => events(&mut stream),
        other => match other.strip_prefix("/ui/") {
            Some(name) if !name.contains("..") => match std::fs::read(bundle.join(name)) {
                Ok(body) => reply(&mut stream, content_type(name), &body),
                Err(_) => not_found(&mut stream),
            },
            _ => not_found(&mut stream),
        },
    }
}

fn content_type(name: &str) -> &'static str {
    if name.ends_with(".wasm") {
        "application/wasm"
    } else if name.ends_with(".js") {
        "text/javascript"
    } else {
        "application/octet-stream"
    }
}

fn reply(stream: &mut TcpStream, kind: &str, body: &[u8]) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: {kind}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)
}

fn not_found(stream: &mut TcpStream) -> std::io::Result<()> {
    stream.write_all(b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")
}

fn events(stream: &mut TcpStream) -> std::io::Result<()> {
    stream.write_all(b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncache-control: no-cache\r\nconnection: close\r\n\r\n")?;
    for data in SESSION {
        stream.write_all(format!("data: {data}\n\n").as_bytes())?;
        stream.flush()?;
        std::thread::sleep(Duration::from_millis(300));
    }
    // Keep the connection open, as the real server does.
    loop {
        stream.write_all(b": keep-alive\n\n")?;
        stream.flush()?;
        std::thread::sleep(Duration::from_secs(15));
    }
}

//! RB-8: the wire recording of the Python server (event types, the eight
//! routes, the `/ingest` handshake) and the baseline summary that names
//! them.
//!
//! The inputs are `wire/events.jsonl`, `wire/routes.json` and
//! `wire/ingest_handshake.json` under the baseline directory, and
//! `docs/results/baseline_python.md`.

mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hayamimi_eval::error::EvalError;
use hayamimi_eval::record;
use hayamimi_eval::wav::{SAMPLE_RATE, write_wav};
use hayamimi_eval::ws::{FrameReader, Incoming, OP_CLOSE, OP_TEXT, encode_frame};
use serde_json::Value;
use sha1::{Digest, Sha1};
use support::{FIXTURES, Failure, RESULTS, TestResult, base, read, read_json, section};

/// The event types the Python server sends.
const KNOWN_TYPES: [&str; 13] = [
    "model_load",
    "warning",
    "error",
    "refine",
    "translation",
    "session_summary",
    "session_start",
    "session_reset",
    "recluster",
    "ready",
    "partial",
    "model_fallback",
    "final",
];

/// The routes of the server, each recorded with a status and a key set.
const ROUTES: [&str; 8] = [
    "/events",
    "/dashboard",
    "/transcript",
    "/replacements",
    "/itn_overrides",
    "/config",
    "/reset",
    "/",
];

const SECTION_IDS: [&str; 5] = ["RB-3", "RB-4", "RB-5", "RB-6", "RB-7"];
const ENVIRONMENT_LINES: [&str; 4] = ["測った機械：", "Python の版：", "package の版：", "日時："];
const PACKAGE_VERSIONS: [&str; 6] = [
    "sacrebleu 2.6.0",
    "jiwer 4.0.0",
    "simpleder 0.0.5",
    "OpenCC 1.4.2",
    "pyarrow 25.0.1",
    "sherpa-onnx 1.13.6",
];

/// RFC 6455 section 1.3.
const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// How many events of each type were recorded.
type Tally = BTreeMap<String, usize>;

fn wire(name: &str) -> Result<std::path::PathBuf, Failure> {
    Ok(base()?.join("wire").join(name))
}

fn tally(events_jsonl: &str) -> Result<Tally, Failure> {
    let mut found = Tally::new();
    for line in events_jsonl.lines().filter(|line| !line.trim().is_empty()) {
        let value: Value =
            serde_json::from_str(line).map_err(|e| format!("a recorded line is not JSON: {e}"))?;
        let Some(event) = value.get("event") else {
            return Err(String::from("a recorded line has no event"));
        };
        let Some(name) = event.get("type").and_then(Value::as_str) else {
            return Err(format!(
                "a recorded event is not a JSON object with a type: {line}"
            ));
        };
        *found.entry(name.to_owned()).or_insert(0) += 1;
    }
    Ok(found)
}

fn check_types_known(found: &Tally) -> Result<(), Failure> {
    for name in found.keys() {
        if !KNOWN_TYPES.contains(&name.as_str()) {
            return Err(format!("unknown event type {name}"));
        }
    }
    Ok(())
}

/// The event types the summary lists: the first code span of each bullet.
fn listed_types(summary_section: &str) -> BTreeSet<String> {
    summary_section
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("- `")?;
            Some(rest.split_once('`')?.0.to_owned())
        })
        .collect()
}

/// The summary lists exactly the recorded types.
fn check_type_list(summary: &str, recorded: &BTreeSet<String>) -> Result<(), Failure> {
    let listed = listed_types(section(summary, "RB-8")?);
    if let Some(name) = recorded.difference(&listed).next() {
        return Err(format!("{name} is recorded but not listed"));
    }
    if let Some(name) = listed.difference(recorded).next() {
        return Err(format!("{name} is listed but not recorded"));
    }
    Ok(())
}

/// Each of the eight routes has a record with an integer status and a key list.
fn check_routes(routes: &Value) -> Result<(), Failure> {
    let Some(list) = routes.as_array() else {
        return Err(String::from("routes.json is not a list"));
    };
    for path in ROUTES {
        let found = list.iter().any(|record| {
            record.get("path").and_then(Value::as_str) == Some(path)
                && record.get("status").is_some_and(Value::is_u64)
                && record.get("keys").is_some_and(Value::is_array)
        });
        if !found {
            return Err(format!("{path} has no record with a status and keys"));
        }
    }
    Ok(())
}

fn accept_of(key: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(key.as_bytes());
    hasher.update(WS_GUID.as_bytes());
    STANDARD.encode(hasher.finalize())
}

fn check_handshake(record: &Value) -> Result<(), Failure> {
    let status = record.get("response_status").and_then(Value::as_u64);
    let key = record.get("request_key").and_then(Value::as_str);
    let accept = record.get("response_accept").and_then(Value::as_str);
    let (Some(status), Some(key), Some(accept)) = (status, key, accept) else {
        return Err(String::from("the handshake record lacks a field"));
    };
    if status != 101 {
        return Err(format!("the upgrade answered {status}"));
    }
    if accept != accept_of(key) {
        return Err(format!("accept {accept} is not the one for key {key}"));
    }
    Ok(())
}

fn check_summary(summary: &str) -> Result<(), Failure> {
    for id in SECTION_IDS {
        section(summary, id)?;
    }
    for prefix in ENVIRONMENT_LINES {
        if !summary.lines().any(|line| line.starts_with(prefix)) {
            return Err(format!("no line starts with {prefix}"));
        }
    }
    let Some(packages) = summary
        .lines()
        .find(|line| line.starts_with("package の版："))
    else {
        return Err(String::from("no package line"));
    };
    for version in PACKAGE_VERSIONS {
        if !packages.contains(version) {
            return Err(format!("the package line lacks {version}"));
        }
    }
    Ok(())
}

fn recorded_tally() -> Result<Tally, Failure> {
    tally(&read(&wire("events.jsonl")?)?)
}

fn recorded_types() -> Result<BTreeSet<String>, Failure> {
    Ok(recorded_tally()?.into_keys().collect())
}

/// A small HTTP server for the nine routes of the Python server. `/events`
/// sends one `final` and then closes. It runs until `stop` is set.
fn fake_http(listener: TcpListener, stop: Arc<AtomicBool>) -> JoinHandle<()> {
    std::thread::spawn(move || {
        if listener.set_nonblocking(true).is_err() {
            return;
        }
        while !stop.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, _)) => {
                    std::thread::spawn(move || serve_one(stream));
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(_) => return,
            }
        }
    })
}

fn serve_one(mut stream: TcpStream) {
    if stream.set_nonblocking(false).is_err() {
        return;
    }
    let Some((method, path)) = read_request(&mut stream) else {
        return;
    };
    let reply = reply_for(&method, &path);
    // The client may already have gone; nothing is left to tell it.
    let _ = stream.write_all(reply.as_bytes());
}

/// The method and path of one request; a body is read and dropped.
fn read_request(stream: &mut TcpStream) -> Option<(String, String)> {
    let mut buf = Vec::new();
    let mut chunk = [0_u8; 1024];
    let head_end = loop {
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break at + 4;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let length = head.lines().find_map(|line| {
        let lower = line.to_ascii_lowercase();
        lower
            .strip_prefix("content-length:")?
            .trim()
            .parse::<usize>()
            .ok()
    });
    if let Some(length) = length {
        while buf.len() < head_end + length {
            let n = stream.read(&mut chunk).ok()?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
    }
    let mut words = head.lines().next()?.split_whitespace();
    Some((words.next()?.to_owned(), words.next()?.to_owned()))
}

fn http(status: &str, content_type: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn reply_for(method: &str, path: &str) -> String {
    match (method, path) {
        ("GET", "/events") => String::from(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n\
             data: {\"type\":\"final\",\"text\":\"x\"}\n\n",
        ),
        ("GET", "/" | "/dashboard" | "/transcript") => {
            http("200 OK", "text/html; charset=utf-8", "<html></html>")
        }
        ("GET", "/replacements" | "/itn_overrides" | "/config") => {
            http("200 OK", "application/json", "{\"a\":1}")
        }
        ("POST", "/replacements" | "/itn_overrides" | "/config" | "/reset") => {
            http("200 OK", "application/json", "{\"ok\":true}")
        }
        _ => http("404 Not Found", "application/json", "{\"error\":\"no\"}"),
    }
}

/// A WebSocket `/ingest` that answers the upgrade with the accept this test
/// computes, sends `ready` after the config and counts nothing else.
fn fake_ingest(listener: TcpListener) -> JoinHandle<Result<(), String>> {
    std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().map_err(|e| e.to_string())?;
        let mut head = [0_u8; 2048];
        let n = socket.read(&mut head).map_err(|e| e.to_string())?;
        let text = String::from_utf8_lossy(&head[..n]).into_owned();
        let Some(key) = text
            .lines()
            .find_map(|line| line.strip_prefix("Sec-WebSocket-Key: "))
        else {
            return Err(String::from("the upgrade request has no key"));
        };
        let reply = format!(
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
             Sec-WebSocket-Accept: {}\r\n\r\n",
            accept_of(key)
        );
        socket
            .write_all(reply.as_bytes())
            .map_err(|e| e.to_string())?;
        socket
            .set_read_timeout(Some(Duration::from_secs(10)))
            .map_err(|e| e.to_string())?;
        let mut reader =
            FrameReader::new(socket.try_clone().map_err(|e| e.to_string())?, Vec::new());
        let mut idle = 0;
        loop {
            match reader.next_frame().map_err(|e| e.to_string())? {
                Incoming::Frame(frame) if frame.opcode == OP_TEXT => {
                    let ready = encode_frame(OP_TEXT, br#"{"type":"ready","sr":16000}"#, None);
                    socket.write_all(&ready).map_err(|e| e.to_string())?;
                }
                Incoming::Frame(frame) if frame.opcode == OP_CLOSE => return Ok(()),
                Incoming::Frame(_) => {}
                Incoming::Idle => {
                    idle += 1;
                    if idle >= 3 {
                        return Err(String::from("the client went quiet"));
                    }
                }
                Incoming::Closed => return Ok(()),
            }
        }
    })
}

fn record_options(
    dir: &Path,
    http_port: u16,
    ingest_port: u16,
) -> Result<record::Options, Box<dyn std::error::Error>> {
    std::fs::create_dir_all(dir)?;
    let wav = dir.join("tone.wav");
    write_wav(&wav, SAMPLE_RATE, &[100_i16; 3_200])?;
    Ok(record::Options {
        host: String::from("127.0.0.1"),
        http_port,
        ingest_port,
        wav,
        out: dir.join("out"),
        gap_ms: 0,
        tail_s: 0,
        reset_wait_s: 0,
    })
}

#[test]
fn rb08_wire_recording_and_baseline_summary_exist_ready_and_final_are_recorded() -> TestResult {
    let found = recorded_tally()?;
    for name in ["ready", "final"] {
        assert!(found.get(name).is_some_and(|count| *count >= 1), "{name}");
    }
    Ok(())
}

#[test]
fn rb08_wire_recording_and_baseline_summary_exist_recorded_types_are_among_the_13_known_types()
-> TestResult {
    check_types_known(&recorded_tally()?)?;
    Ok(())
}

#[test]
fn rb08_wire_recording_and_baseline_summary_exist_unknown_event_type_is_rejected() -> TestResult {
    let found = tally(include_str!("fixtures/rb08_bogus_event.jsonl"))?;
    let said = check_types_known(&found);
    assert!(matches!(&said, Err(text) if text.contains("bogus")));
    Ok(())
}

#[test]
fn rb08_wire_recording_and_baseline_summary_exist_every_recorded_type_has_a_parsable_json()
-> TestResult {
    // `tally` refuses a line that is not JSON and an event that is not an
    // object with a type, so a tally that comes back counts parsable JSON only.
    let found = recorded_tally()?;
    assert!(!found.is_empty());
    for (name, count) in found {
        assert!(count >= 1, "{name} has no JSON");
    }
    Ok(())
}

#[test]
fn rb08_wire_recording_and_baseline_summary_exist_baseline_type_list_equals_recorded_types()
-> TestResult {
    let summary = read(&Path::new(RESULTS).join("baseline_python.md"))?;
    check_type_list(&summary, &recorded_types()?)?;
    Ok(())
}

fn three_types() -> BTreeSet<String> {
    ["ready", "partial", "final"].map(String::from).into()
}

#[test]
fn rb08_wire_recording_and_baseline_summary_exist_type_missing_from_baseline_list_is_named() {
    let said = check_type_list(
        include_str!("fixtures/rb08_list_missing_one.md"),
        &three_types(),
    );
    assert!(matches!(&said, Err(text) if text.starts_with("partial")));
}

#[test]
fn rb08_wire_recording_and_baseline_summary_exist_extra_type_in_baseline_list_is_named() {
    let said = check_type_list(
        include_str!("fixtures/rb08_list_with_extra.md"),
        &three_types(),
    );
    assert!(matches!(&said, Err(text) if text.starts_with("model_fallback")));
}

#[test]
fn rb08_wire_recording_and_baseline_summary_exist_eight_routes_are_recorded_with_status_and_keys()
-> TestResult {
    check_routes(&read_json(&wire("routes.json")?)?)?;
    Ok(())
}

#[test]
fn rb08_wire_recording_and_baseline_summary_exist_seven_routes_are_rejected() -> TestResult {
    let routes: Value = serde_json::from_str(include_str!("fixtures/rb08_routes_seven.json"))?;
    let said = check_routes(&routes);
    assert!(matches!(&said, Err(text) if text.starts_with("/itn_overrides")));
    Ok(())
}

#[test]
fn rb08_wire_recording_and_baseline_summary_exist_accept_key_matches_the_rfc_6455_example() {
    assert_eq!(
        accept_of("dGhlIHNhbXBsZSBub25jZQ=="),
        "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
    );
}

#[test]
fn rb08_wire_recording_and_baseline_summary_exist_recorded_ingest_handshake_has_a_valid_accept()
-> TestResult {
    check_handshake(&read_json(&wire("ingest_handshake.json")?)?)?;
    Ok(())
}

#[test]
fn rb08_wire_recording_and_baseline_summary_exist_wrong_accept_in_recorded_handshake_is_rejected()
-> TestResult {
    let record: Value =
        serde_json::from_str(include_str!("fixtures/rb08_handshake_wrong_accept.json"))?;
    assert!(check_handshake(&record).is_err());
    Ok(())
}

#[test]
fn rb08_wire_recording_and_baseline_summary_exist_baseline_summary_has_sections_and_environment_lines()
-> TestResult {
    check_summary(&read(&Path::new(RESULTS).join("baseline_python.md"))?)?;
    Ok(())
}

#[test]
fn rb08_wire_recording_and_baseline_summary_exist_summary_missing_a_section_is_named() {
    let said = check_summary(include_str!("fixtures/rb08_summary_without_rb06.md"));
    assert!(matches!(&said, Err(text) if text.contains("RB-6")));
}

#[test]
fn rb08_wire_recording_and_baseline_summary_exist_summary_missing_the_date_line_is_named() {
    let said = check_summary(include_str!("fixtures/rb08_summary_without_date.md"));
    assert!(matches!(&said, Err(text) if text.contains("日時")));
}

#[test]
fn rb08_wire_recording_and_baseline_summary_exist_record_against_fake_server_captures_each_event_type()
-> TestResult {
    let http_listener = TcpListener::bind("127.0.0.1:0")?;
    let ingest_listener = TcpListener::bind("127.0.0.1:0")?;
    let (http_port, ingest_port) = (
        http_listener.local_addr()?.port(),
        ingest_listener.local_addr()?.port(),
    );
    let stop = Arc::new(AtomicBool::new(false));
    let web = fake_http(http_listener, Arc::clone(&stop));
    let socket = fake_ingest(ingest_listener);
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("rb08_record_fake");
    let options = record_options(&dir, http_port, ingest_port)?;
    let recorded = record::run(&options);
    stop.store(true, Ordering::Relaxed);
    web.join().map_err(|_| "the fake web server panicked")?;
    socket.join().map_err(|_| "the fake ingest panicked")??;
    let want: BTreeMap<String, usize> = [("final", 1), ("ready", 1)]
        .map(|(k, n)| (k.to_owned(), n))
        .into();
    assert_eq!(recorded?.events_by_type, want);
    let events = read(&options.out.join("events.jsonl"))?;
    assert_eq!(tally(&events)?.len(), 2);
    check_routes(&read_json(&options.out.join("routes.json"))?)?;
    check_handshake(&read_json(&options.out.join("ingest_handshake.json"))?)?;
    Ok(())
}

#[test]
fn rb08_wire_recording_and_baseline_summary_exist_record_against_closed_port_is_a_connect_error()
-> TestResult {
    let (first, second) = (
        TcpListener::bind("127.0.0.1:0")?,
        TcpListener::bind("127.0.0.1:0")?,
    );
    let ports = (first.local_addr()?.port(), second.local_addr()?.port());
    drop((first, second));
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("rb08_record_closed");
    let recorded = record::run(&record_options(&dir, ports.0, ports.1)?);
    assert!(matches!(recorded, Err(EvalError::Connect { .. })));
    Ok(())
}

#[test]
fn rb08_wire_recording_and_baseline_summary_exist_missing_wire_directory_names_its_path() {
    let absent = Path::new(FIXTURES)
        .join("absent")
        .join("wire")
        .join("events.jsonl");
    assert!(
        matches!(read(&absent), Err(path) if path.contains("wire") && path.ends_with("events.jsonl"))
    );
}

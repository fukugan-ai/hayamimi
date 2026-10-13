//! `record`: captures what a running server says on the wire, so another
//! server can later be held to the same shapes.
//!
//! One run plays a recording into `/ingest` (recording the upgrade
//! exchange and the events sent back on that socket), listens to `/events`
//! for the whole run, then calls every route once. It writes into `--out`:
//!
//! - `events.jsonl`: one line per event, `{"source","t_us","event"}`, where
//!   `source` is `sse` (from `/events`) or `ingest` (from the ingest socket);
//! - `routes.json`: one entry per request, with status, content type, the
//!   top-level JSON keys of the answer and, for JSON, the answer itself;
//! - `ingest_handshake.json`: the key sent and the status and accept value
//!   received.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Value, json};
use ureq::Agent;

use crate::error::EvalError;
use crate::ingest::{self, micros_since};
use crate::log;
use crate::manifest::write_text;
use crate::wav::{SAMPLE_RATE, read_wav};
use crate::ws::Handshake;

pub struct Options {
    pub host: String,
    pub http_port: u16,
    pub ingest_port: u16,
    pub wav: PathBuf,
    pub out: PathBuf,
    pub gap_ms: u32,
    /// How long to listen after the audio ends, before the routes are called.
    pub tail_s: u32,
    /// How long to listen after the session reset.
    pub reset_wait_s: u32,
}

/// One request made to the server and what came back.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RouteRecord {
    pub method: String,
    pub path: String,
    pub status: u16,
    pub content_type: String,
    pub body_bytes: usize,
    /// Sorted top-level keys of a JSON object answer; empty otherwise.
    pub keys: Vec<String>,
    /// The answer, when it is JSON.
    pub body: Option<Value>,
}

/// What a run captured, in counts.
#[derive(Debug)]
pub struct Recorded {
    pub events_by_type: BTreeMap<String, usize>,
    pub routes: usize,
}

/// The routes of the server, in the order they are called. A route with a
/// JSON body is also posted back with the body it just returned, which
/// leaves the server's state as it was.
const PAGES: [&str; 3] = ["/", "/dashboard", "/transcript"];
const JSON_ROUTES: [&str; 3] = ["/replacements", "/itn_overrides", "/config"];

/// Sorted top-level keys of a JSON object.
pub fn json_keys(value: &Value) -> Vec<String> {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<String> = map.keys().cloned().collect();
            keys.sort();
            keys
        }
        _ => Vec::new(),
    }
}

/// Builds the record of one answer from its parts.
pub fn route_record(
    method: &str,
    path: &str,
    status: u16,
    content_type: &str,
    body: &str,
) -> RouteRecord {
    let parsed = if content_type.contains("json") {
        serde_json::from_str::<Value>(body).ok()
    } else {
        None
    };
    let keys = match &parsed {
        Some(value) => json_keys(value),
        // DEFAULT: an answer that is not JSON has no keys; an empty list is the true count, and `body_bytes` still shows it was non-empty.
        None => Vec::new(),
    };
    RouteRecord {
        method: method.to_owned(),
        path: path.to_owned(),
        status,
        content_type: content_type.to_owned(),
        body_bytes: body.len(),
        keys,
        body: parsed,
    }
}

fn http_error(url: &str, message: impl ToString) -> EvalError {
    EvalError::Http {
        url: url.to_owned(),
        message: message.to_string(),
    }
}

fn content_type_of(response: &ureq::http::Response<ureq::Body>) -> String {
    match response.headers().get("content-type") {
        Some(value) => String::from_utf8_lossy(value.as_bytes()).into_owned(),
        // DEFAULT: the capture shows what the server sent; an answer with no Content-Type header is recorded with an empty one.
        None => String::new(),
    }
}

/// Makes one request and records the answer.
fn call(
    agent: &Agent,
    method: &str,
    base: &str,
    path: &str,
    body: Option<&str>,
) -> Result<RouteRecord, EvalError> {
    let url = format!("{base}{path}");
    let mut response = match (method, body) {
        ("GET", _) => agent.get(&url).call(),
        (_, Some(text)) => agent
            .post(&url)
            .header("Content-Type", "application/json")
            .send(text),
        _ => agent.post(&url).send_empty(),
    }
    .map_err(|e| http_error(&url, e))?;
    let status = response.status().as_u16();
    let content_type = content_type_of(&response);
    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|e| http_error(&url, e))?;
    Ok(route_record(method, path, status, &content_type, &text))
}

/// Calls every route once, the session reset last.
pub fn probe_routes(agent: &Agent, base: &str) -> Result<Vec<RouteRecord>, EvalError> {
    let mut records = Vec::new();
    for path in PAGES {
        records.push(call(agent, "GET", base, path, None)?);
    }
    let mut echoes: Vec<(&str, String)> = Vec::new();
    for path in JSON_ROUTES {
        let record = call(agent, "GET", base, path, None)?;
        if let Some(body) = &record.body {
            echoes.push((path, body.to_string()));
        }
        records.push(record);
    }
    for (path, body) in &echoes {
        records.push(call(agent, "POST", base, path, Some(body))?);
    }
    records.push(call(agent, "POST", base, "/reset", None)?);
    Ok(records)
}

/// What the `/events` listener returned.
struct Listened {
    status: u16,
    content_type: String,
    lines: Vec<(i64, String)>,
}

/// Listens to `/events` until the agent's global timeout ends the read.
fn listen(agent: &Agent, base: &str, origin: Instant) -> Result<Listened, EvalError> {
    let url = format!("{base}/events");
    let response = agent.get(&url).call().map_err(|e| http_error(&url, e))?;
    let status = response.status().as_u16();
    let content_type = content_type_of(&response);
    let mut reader = BufReader::new(response.into_body().into_reader());
    let mut lines = Vec::new();
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                if let Some(data) = line.trim_end().strip_prefix("data: ") {
                    lines.push((micros_since(origin), data.to_owned()));
                }
            }
            // The global timeout ends the read; what arrived before it is kept.
            Err(_) => break,
        }
    }
    Ok(Listened {
        status,
        content_type,
        lines,
    })
}

/// The `type` of an event's JSON text, or `unparsed`.
pub fn event_type(text: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return String::from("unparsed");
    };
    match value.get("type").and_then(Value::as_str) {
        Some(kind) => kind.to_owned(),
        // DEFAULT: an event with no `type` is counted under the visible label "unparsed" so the count shows it instead of hiding it.
        None => String::from("unparsed"),
    }
}

/// The lines of `events.jsonl`, in arrival order.
fn event_lines(
    sse: &[(i64, String)],
    ingest_events: &[(i64, String)],
) -> Result<Vec<String>, EvalError> {
    let mut tagged: Vec<(i64, &str, &str)> = sse
        .iter()
        .map(|(t, text)| (*t, "sse", text.as_str()))
        .chain(
            ingest_events
                .iter()
                .map(|(t, text)| (*t, "ingest", text.as_str())),
        )
        .collect();
    tagged.sort_by_key(|(t, _, _)| *t);
    tagged
        .into_iter()
        .map(|(t_us, source, text)| {
            let event = match serde_json::from_str::<Value>(text) {
                Ok(parsed) => parsed,
                // DEFAULT: the capture keeps what arrived; text that is not JSON is stored verbatim as a string.
                Err(_) => Value::String(text.to_owned()),
            };
            serde_json::to_string(&json!({ "source": source, "t_us": t_us, "event": event }))
                .map_err(|e| EvalError::Invalid(format!("an event line: {e}")))
        })
        .collect()
}

fn handshake_json(handshake: &Handshake) -> Value {
    json!({
        "request_key": handshake.request_key,
        "response_status": handshake.response_status,
        "response_accept": handshake.response_accept,
    })
}

fn agent_for(total: Duration) -> Agent {
    Agent::config_builder()
        .timeout_global(Some(total))
        .http_status_as_error(false)
        .build()
        .into()
}

/// Records a run.
pub fn run(options: &Options) -> Result<Recorded, EvalError> {
    let audio = read_wav(&options.wav)?;
    if audio.sample_rate != SAMPLE_RATE {
        return Err(EvalError::Wav {
            path: options.wav.display().to_string(),
            message: format!("sample rate {} is not {SAMPLE_RATE}", audio.sample_rate),
        });
    }
    let gap = options.gap_ms as usize * (SAMPLE_RATE / 1000) as usize;
    let plan = ingest::plan(audio.samples.len(), &[], 1, gap);
    let audio_s = (audio.samples.len() / SAMPLE_RATE as usize) as u64 + 1;
    let total =
        Duration::from_secs(audio_s + u64::from(options.tail_s + options.reset_wait_s) + 60);
    let base = format!("http://{}:{}", options.host, options.http_port);
    let origin = Instant::now();

    let listener = {
        let (agent, base) = (agent_for(total), base.clone());
        std::thread::spawn(move || listen(&agent, &base, origin))
    };
    // The listener subscribes before any audio is sent.
    std::thread::sleep(Duration::from_millis(500));
    let mut connection = ingest::open(&options.host, options.ingest_port, origin)?;
    connection.start()?;
    log::info("eval.record.started", "worker", "recording a run", &[]);
    connection.play(&plan, &audio.samples)?;
    std::thread::sleep(Duration::from_secs(u64::from(options.tail_s)));
    let routes = probe_routes(&agent_for(Duration::from_secs(60)), &base)?;
    let received = connection.finish(Duration::from_secs(u64::from(options.reset_wait_s)))?;
    let listened = listener
        .join()
        .map_err(|_| EvalError::Protocol(String::from("the listener thread panicked")))??;

    let mut events_text = String::new();
    for line in event_lines(&listened.lines, &received.events)? {
        events_text.push_str(&line);
        events_text.push('\n');
    }
    let mut events_by_type: BTreeMap<String, usize> = BTreeMap::new();
    for (_, text) in listened.lines.iter().chain(received.events.iter()) {
        *events_by_type.entry(event_type(text)).or_insert(0) += 1;
    }

    let sse_keys: BTreeSet<String> = listened
        .lines
        .iter()
        .filter_map(|(_, text)| serde_json::from_str::<Value>(text).ok())
        .flat_map(|v| json_keys(&v))
        .collect();
    let mut all_routes = routes;
    all_routes.push(RouteRecord {
        method: String::from("GET"),
        path: String::from("/events"),
        status: listened.status,
        content_type: listened.content_type,
        body_bytes: 0,
        keys: sse_keys.into_iter().collect(),
        body: None,
    });

    write_text(&options.out.join("events.jsonl"), &events_text)?;
    let routes_path = options.out.join("routes.json");
    let routes_text =
        serde_json::to_string_pretty(&all_routes).map_err(|e| EvalError::json(&routes_path, e))?;
    write_text(&routes_path, &format!("{routes_text}\n"))?;
    let handshake_path = options.out.join("ingest_handshake.json");
    let handshake_text = serde_json::to_string_pretty(&handshake_json(&received.handshake))
        .map_err(|e| EvalError::json(&handshake_path, e))?;
    write_text(&handshake_path, &format!("{handshake_text}\n"))?;
    log::info(
        "eval.record.finished",
        "worker",
        "recording finished",
        &[
            ("event_types", events_by_type.len().into()),
            ("routes", all_routes.len().into()),
        ],
    );
    Ok(Recorded {
        events_by_type,
        routes: all_routes.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn json_keys_are_sorted_and_empty_for_other_values() {
        assert_eq!(json_keys(&json!({"b": 1, "a": 2})), vec!["a", "b"]);
        assert!(json_keys(&json!([1])).is_empty());
    }

    #[test]
    fn a_json_answer_keeps_its_keys_and_body() {
        let record = route_record(
            "GET",
            "/config",
            200,
            "application/json; charset=utf-8",
            r#"{"lang":null,"vad":{}}"#,
        );
        assert_eq!(record.keys, vec!["lang", "vad"]);
        assert_eq!(record.body, Some(json!({"lang": null, "vad": {}})));
        assert_eq!(record.body_bytes, 22);
    }

    #[test]
    fn an_html_answer_has_no_keys() {
        let record = route_record("GET", "/", 200, "text/html; charset=utf-8", "<html>");
        assert!(record.keys.is_empty());
        assert_eq!(record.body, None);
    }

    #[test]
    fn a_json_content_type_with_a_broken_body_has_no_body() {
        assert_eq!(
            route_record("GET", "/x", 200, "application/json", "{").body,
            None
        );
    }

    #[test]
    fn the_type_of_an_event_is_read_or_unparsed() {
        assert_eq!(event_type(r#"{"type":"final","text":"a"}"#), "final");
        assert_eq!(event_type("nope"), "unparsed");
        assert_eq!(event_type(r#"{"text":"a"}"#), "unparsed");
    }

    #[test]
    fn event_lines_are_tagged_and_ordered_by_time() -> Result<(), Box<dyn std::error::Error>> {
        let sse = vec![(20, String::from(r#"{"type":"final"}"#))];
        let ingest_side = vec![(10, String::from(r#"{"type":"ready"}"#))];
        let lines = event_lines(&sse, &ingest_side)?;
        assert_eq!(lines.len(), 2);
        let first: Value = serde_json::from_str(&lines[0])?;
        assert_eq!(first["source"], "ingest");
        assert_eq!(first["event"]["type"], "ready");
        Ok(())
    }

    #[test]
    fn the_handshake_is_recorded_with_all_three_values() {
        let handshake = Handshake {
            request_key: String::from("k"),
            response_status: 101,
            response_accept: String::from("a"),
        };
        assert_eq!(
            handshake_json(&handshake),
            json!({"request_key": "k", "response_status": 101, "response_accept": "a"})
        );
    }

    /// Serves `count` requests: HTML for GET `/`, JSON for anything on
    /// `/config`, and 404 for the rest.
    fn serve_routes(listener: TcpListener, count: usize) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            for _ in 0..count {
                let Ok((mut socket, _)) = listener.accept() else {
                    return;
                };
                let mut request = [0_u8; 4096];
                let Ok(n) = socket.read(&mut request) else {
                    return;
                };
                let head = String::from_utf8_lossy(&request[..n]).into_owned();
                let first = head.lines().next().map(str::to_owned);
                let (status, kind, body) = match first.as_deref() {
                    Some(line) if line.starts_with("GET / ") => {
                        ("200 OK", "text/html", "<html></html>")
                    }
                    Some(line) if line.contains("/config") => {
                        ("200 OK", "application/json", r#"{"lang":null}"#)
                    }
                    _ => (
                        "404 Not Found",
                        "application/json",
                        r#"{"error":"not found"}"#,
                    ),
                };
                let reply = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                if socket.write_all(reply.as_bytes()).is_err() {
                    return;
                }
            }
        })
    }

    #[test]
    fn a_route_is_recorded_with_its_status_even_when_it_is_a_404()
    -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let base = format!("http://127.0.0.1:{}", listener.local_addr()?.port());
        let server = serve_routes(listener, 3);
        let agent = agent_for(Duration::from_secs(10));
        let page = call(&agent, "GET", &base, "/", None)?;
        let config = call(&agent, "GET", &base, "/config", None)?;
        let missing = call(&agent, "POST", &base, "/nowhere", None)?;
        server.join().map_err(|_| "server thread panicked")?;
        assert_eq!((page.status, page.keys.len()), (200, 0));
        assert_eq!(config.keys, vec!["lang"]);
        assert_eq!(
            (missing.status, missing.keys),
            (404, vec![String::from("error")])
        );
        Ok(())
    }

    #[test]
    fn an_unreachable_server_is_an_http_error_naming_the_url() -> Result<(), std::io::Error> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        drop(listener);
        let agent = agent_for(Duration::from_secs(5));
        let base = format!("http://127.0.0.1:{port}");
        let said = call(&agent, "GET", &base, "/", None)
            .err()
            .map(|e| e.to_string());
        assert!(
            said.as_deref().is_some_and(|t| t.starts_with(&base)),
            "{said:?}"
        );
        Ok(())
    }
}

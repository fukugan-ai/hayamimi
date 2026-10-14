//! RP-12: the wire form is compatible. For each event type the Python
//! server recorded (`testdata/baseline/wire/`), the Rust event has the same
//! keys and the same value types, apart from the keys the design lets it add;
//! the eight routes and the `/ingest` handshake answer with the same status
//! and the same keys as the recorded ones.
//!
//! The Rust side is the real code, not a copy: the event builders of
//! `hayamimi_app::wire`, and the routes of `hayamimi_app::routes` served on a
//! port of their own with a stand-in for the recognition loop.

mod support;

use std::collections::BTreeSet;
use std::error::Error;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use hayamimi_app::control::{Command, Controls, Initial, Live};
use hayamimi_app::routes::{self, AppState};
use hayamimi_app::server::{self, Hub};
use hayamimi_app::{feed, ingest, strings::Strings, wire};
use hayamimi_asr::itn::Overrides;
use hayamimi_asr::replace::Replacements;
use hayamimi_asr::vad::Sensitivity;
use hayamimi_asr::{Final, Timings};
use hayamimi_eval::ws;
use hayamimi_relay::lang::Lang;
use serde_json::{Value, json};
use support::{Failure, TestResult, find_above, read};

/// The keys Rust may add to a `final` and a `refine` (the conversation
/// subtitles design, 3.3).
const FINAL_EXTRAS: [&str; 5] = ["id", "source", "t0_s", "t1_s", "end_wall_ms"];
/// The key Rust may add to a `translation`.
const TRANSLATION_EXTRAS: [&str; 1] = ["src_sha256"];
/// The event types whose producers are later slices of the port: the refine pass and the translation.
const LATER: [&str; 2] = ["refine", "translation"];

fn allowed_for(kind: &str) -> &'static [&'static str] {
    match kind {
        "final" | "refine" => &FINAL_EXTRAS,
        "translation" => &TRANSLATION_EXTRAS,
        _ => &[],
    }
}

fn json_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn kind_of(event: &Value) -> Result<&str, Failure> {
    event
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{event} has no `type`"))
}

/// The two events have the same keys and value types; Rust may add the keys
/// the design names for the type of the event.
fn compare_event(python: &Value, rust: &Value) -> Result<(), Failure> {
    let (Some(wanted), Some(found)) = (python.as_object(), rust.as_object()) else {
        return Err(String::from("an event is not a JSON object"));
    };
    let kind = kind_of(python)?;
    if let Some(key) = wanted.keys().find(|key| !found.contains_key(*key)) {
        return Err(format!("`{kind}`: the key `{key}` is missing"));
    }
    let allowed = allowed_for(kind);
    if let Some(key) = found
        .keys()
        .find(|key| !wanted.contains_key(*key) && !allowed.contains(&key.as_str()))
    {
        return Err(format!(
            "`{kind}`: the key `{key}` is not in the recording and may not be added to `{kind}`"
        ));
    }
    for (key, value) in wanted {
        let other = &found[key];
        if json_type(value) != json_type(other) {
            return Err(format!(
                "`{kind}`: `{key}` is {} in Rust and {} in the recording",
                json_type(other),
                json_type(value)
            ));
        }
    }
    Ok(())
}

/// Every recorded event has a Rust event of its type with the same shape. A
/// Rust type the recording does not have (`fallback`) is not a difference. The
/// types in `later` have no producer yet and are left out.
fn check_wire(python: &[Value], rust: &[Value], later: &[&str]) -> Result<(), Failure> {
    for recorded in python {
        let kind = kind_of(recorded)?;
        if later.contains(&kind) {
            continue;
        }
        let sample = rust
            .iter()
            .find(|event| kind_of(event).is_ok_and(|found| found == kind))
            .ok_or_else(|| format!("no Rust sample of the `{kind}` event"))?;
        compare_event(recorded, sample)?;
    }
    Ok(())
}

/// The events of the Python recording, in the order they were heard.
fn recorded_events() -> Result<Vec<Value>, Failure> {
    let path = find_above("testdata/baseline/wire/events.jsonl")?;
    read(&path)?
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let row: Value = serde_json::from_str(line).map_err(|error| error.to_string())?;
            row.get("event")
                .cloned()
                .ok_or_else(|| format!("`{line}` has no `event`"))
        })
        .collect()
}

fn recorded_of(kind: &str) -> Result<Value, Failure> {
    recorded_events()?
        .into_iter()
        .find(|event| kind_of(event).is_ok_and(|found| found == kind))
        .ok_or_else(|| format!("the recording has no `{kind}` event"))
}

fn finished() -> Final {
    Final {
        id: 1,
        text: String::from("警察のソランキ総監によれば被告人は顔を隠して出頭した。"),
        audio_seconds: 6.7,
        t0_s: 0.0,
        t1_s: 6.7,
        end_wall_ms: 1_790_000_000_000,
        timings: Timings {
            vad_ms: 1.0,
            lid_ms: 0.0,
            decode_ms: 185.0,
            punct_ms: 10.0,
            itn_ms: 1.0,
            l_dec_ms: 533.0,
        },
        punct_error: None,
        speaker_audio: Vec::new(),
    }
}

/// One event of each type the Rust server makes, from the real builders.
fn rust_samples() -> Vec<Value> {
    vec![
        wire::final_event(&finished(), "file"),
        wire::partial_event("警察の"),
        wire::session_summary(11.0, 1, Some(533.0), Some(533.0)),
        wire::session_reset(),
        wire::ready_event(16_000),
        json!({"type": "fallback", "stage": "capture", "reason": "device_missing", "to": "single"}),
    ]
}

fn with_key(mut event: Value, key: &str, value: Value) -> Result<Value, Failure> {
    event
        .as_object_mut()
        .ok_or("not an object")?
        .insert(key.to_owned(), value);
    Ok(event)
}

fn without_key(mut event: Value, key: &str) -> Result<Value, Failure> {
    event
        .as_object_mut()
        .ok_or("not an object")?
        .remove(key)
        .ok_or_else(|| format!("no key {key}"))?;
    Ok(event)
}

/// T-RP-12-001
#[test]
fn python_recording_compared_with_itself_has_no_difference() -> TestResult {
    let recorded = recorded_events()?;
    assert!(recorded.len() >= 7, "{} events", recorded.len());
    check_wire(&recorded, &recorded, &[])?;
    Ok(())
}

/// T-RP-12-002
#[test]
fn added_allowed_keys_on_final_pass() -> TestResult {
    let python = recorded_of("final")?;
    let rust = with_key(
        with_key(python.clone(), "id", json!(7))?,
        "t0_s",
        json!(0.5),
    )?;
    compare_event(&python, &rust)?;
    Ok(())
}

/// T-RP-12-003
#[test]
fn added_src_sha256_on_translation_passes() -> TestResult {
    let python = recorded_of("translation")?;
    let rust = with_key(python.clone(), "src_sha256", json!("ab12"))?;
    compare_event(&python, &rust)?;
    Ok(())
}

/// T-RP-12-004
#[test]
fn allowed_key_on_the_wrong_event_type_is_rejected() -> TestResult {
    let python = recorded_of("partial")?;
    let rust = with_key(python.clone(), "id", json!(1))?;
    let said = compare_event(&python, &rust).err().ok_or("it passed")?;
    assert!(
        said.contains("`id`") && said.contains("`partial`"),
        "{said}"
    );
    Ok(())
}

/// T-RP-12-005
#[test]
fn missing_key_is_named() -> TestResult {
    let python = recorded_of("final")?;
    let rust = without_key(python.clone(), "lang")?;
    let said = compare_event(&python, &rust).err().ok_or("it passed")?;
    assert!(said.contains("`lang`"), "{said}");
    Ok(())
}

/// T-RP-12-006
#[test]
fn changed_value_type_is_named() -> TestResult {
    let python = recorded_of("final")?;
    let rust = with_key(python.clone(), "audio_s", json!("6.7"))?;
    let said = compare_event(&python, &rust).err().ok_or("it passed")?;
    assert!(said.contains("`audio_s`"), "{said}");
    Ok(())
}

/// T-RP-12-007
#[test]
fn integer_and_float_are_the_same_json_type() {
    let python = json!({"type": "x", "n": 1});
    let rust = json!({"type": "x", "n": 1.0});
    assert!(compare_event(&python, &rust).is_ok());
}

/// T-RP-12-008
#[test]
fn null_and_string_are_different_types() {
    let python = json!({"type": "x", "n": null});
    let rust = json!({"type": "x", "n": "text"});
    assert!(compare_event(&python, &rust).is_err());
}

/// T-RP-12-009
#[test]
fn type_without_a_rust_sample_is_named() -> TestResult {
    let python = recorded_events()?;
    let without_refine: Vec<Value> = python
        .iter()
        .filter(|event| kind_of(event).is_ok_and(|kind| kind != "refine"))
        .cloned()
        .collect();
    let said = check_wire(&python, &without_refine, &[])
        .err()
        .ok_or("it passed")?;
    assert!(said.contains("`refine`"), "{said}");
    Ok(())
}

/// T-RP-12-012
#[test]
fn fallback_event_type_is_excluded_from_the_difference() -> TestResult {
    let python = recorded_events()?;
    let mut rust = python.clone();
    rust.push(json!({"type": "fallback", "stage": "punct", "reason": "no_device", "to": "cpu"}));
    check_wire(&python, &rust, &[])?;
    Ok(())
}

/// T-RP-12-013
#[test]
fn fallback_exclusion_does_not_hide_a_missing_key() -> TestResult {
    let python = recorded_events()?;
    let mut rust: Vec<Value> = python
        .iter()
        .filter(|event| kind_of(event).is_ok_and(|kind| kind != "final"))
        .cloned()
        .collect();
    rust.push(without_key(recorded_of("final")?, "lang")?);
    rust.push(json!({"type": "fallback", "stage": "punct", "to": "cpu"}));
    let said = check_wire(&python, &rust, &[]).err().ok_or("it passed")?;
    assert!(said.contains("`lang`"), "{said}");
    Ok(())
}

/// One route as the recording and the Rust server answer it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Route {
    method: String,
    path: String,
    status: u16,
    content_type: String,
    keys: BTreeSet<String>,
}

fn text_field(row: &Value, key: &str) -> Result<String, Failure> {
    row.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("{row} has no `{key}`"))
}

fn route_of(row: &Value) -> Result<Route, Failure> {
    let status = row
        .get("status")
        .and_then(Value::as_u64)
        .and_then(|status| u16::try_from(status).ok())
        .ok_or_else(|| format!("{row} has no status"))?;
    let keys = row
        .get("keys")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{row} has no keys"))?
        .iter()
        .filter_map(|key| key.as_str().map(str::to_owned))
        .collect();
    Ok(Route {
        method: text_field(row, "method")?,
        path: text_field(row, "path")?,
        status,
        content_type: text_field(row, "content_type")?,
        keys,
    })
}

fn recorded_routes() -> Result<Vec<Route>, Failure> {
    let path = find_above("testdata/baseline/wire/routes.json")?;
    let rows: Vec<Value> =
        serde_json::from_str(&read(&path)?).map_err(|error| error.to_string())?;
    rows.iter().map(route_of).collect()
}

/// A route's status is the recorded one. `POST /reset` has two documented
/// answers, 200 when the loop did it and 202 when it is queued behind a long decode.
fn same_status(route: &Route, other: &Route) -> bool {
    let reset = route.method == "POST" && route.path == "/reset";
    route.status == other.status || (reset && [200, 202].contains(&other.status))
}

/// Every recorded route answers the same way: status, content type, and keys
/// (the keys of `/events` are those of the events, not of one reply).
fn check_routes(recorded: &[Route], found: &[Route]) -> Result<(), Failure> {
    for route in recorded {
        let name = format!("{} {}", route.method, route.path);
        let other = found
            .iter()
            .find(|other| other.method == route.method && other.path == route.path)
            .ok_or_else(|| format!("route {name} is not answered"))?;
        if !same_status(route, other) {
            return Err(format!(
                "route {name}: status {} in Rust, {} in the recording",
                other.status, route.status
            ));
        }
        if route.content_type != other.content_type {
            return Err(format!(
                "route {name}: content type `{}` in Rust, `{}` in the recording",
                other.content_type, route.content_type
            ));
        }
        if route.path != "/events" && route.status == other.status && route.keys != other.keys {
            return Err(format!(
                "route {name}: keys {:?} in Rust, {:?} in the recording",
                other.keys, route.keys
            ));
        }
    }
    Ok(())
}

/// What a request got back.
struct Reply {
    status: u16,
    content_type: String,
    body: String,
}

/// The status line and the headers of a reply; the content type is what is kept.
fn read_head(reader: &mut BufReader<TcpStream>) -> Result<(u16, String), Box<dyn Error>> {
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let status = line
        .split_whitespace()
        .nth(1)
        .ok_or("no status line")?
        .parse::<u16>()?;
    let mut content_type = String::new();
    loop {
        line.clear();
        reader.read_line(&mut line)?;
        let header = line.trim_end();
        if header.is_empty() {
            return Ok((status, content_type));
        }
        if let Some((name, value)) = header.split_once(':')
            && name.eq_ignore_ascii_case("content-type")
        {
            content_type = value.trim().to_owned();
        }
    }
}

fn http(
    address: SocketAddr,
    method: &str,
    path: &str,
    body: &str,
) -> Result<Reply, Box<dyn Error>> {
    let mut stream = TcpStream::connect(address)?;
    stream.set_read_timeout(Some(Duration::from_secs(15)))?;
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\
         Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes())?;
    let mut reader = BufReader::new(stream);
    let (status, content_type) = read_head(&mut reader)?;
    // An event stream never ends; the head says all this check needs.
    let mut text = String::new();
    if !content_type.starts_with("text/event-stream") {
        reader.read_to_string(&mut text)?;
    }
    Ok(Reply {
        status,
        content_type,
        body: text,
    })
}

/// The recognition loop's stand-in: answers a reset.
fn answer_resets(commands: Receiver<Command>) {
    std::thread::spawn(move || {
        for command in commands {
            if let Command::Reset(done) = command {
                // The route may have stopped waiting; then nobody needs the answer.
                let _ = done.send(());
            }
        }
    });
}

/// The address the routes are served on, and the commands receiver when nobody reads it.
type Served = (SocketAddr, Option<Receiver<Command>>);

/// The routes served on a port of their own. With `answering`, a stand-in
/// loop answers resets; without it the commands pile up unread, as in a loop
/// busy with a long decode, and the receiver is handed back to be held.
fn serve_routes(answering: bool) -> Result<Served, Box<dyn Error>> {
    let (commands, received) = mpsc::channel();
    let initial = Initial {
        replacements: Replacements::default(),
        overrides: Overrides::default(),
        punctuate: true,
        can_punctuate: true,
        vad: Sensitivity {
            threshold: 0.5,
            min_silence: 0.35,
            max_speech: 12.0,
        },
    };
    let state = AppState {
        hub: Hub::new(),
        controls: Arc::new(Controls::new(Live::new(initial), commands)),
    };
    let address = server::serve(routes::router(state), server::bind(0)?, "routes-test")?;
    if answering {
        answer_resets(received);
        Ok((address, None))
    } else {
        Ok((address, Some(received)))
    }
}

/// The request bodies of the POST routes: an empty object is a valid body for each.
fn body_of(method: &str, path: &str) -> &'static str {
    if method == "POST" && path != "/reset" {
        "{}"
    } else {
        ""
    }
}

/// The keys of a reply that is a JSON object; a page or a stream has none.
fn keys_of(body: &str) -> BTreeSet<String> {
    match serde_json::from_str::<Value>(body) {
        Ok(Value::Object(object)) => object.keys().cloned().collect(),
        _ => BTreeSet::new(),
    }
}

fn answered_routes(address: SocketAddr, asked: &[Route]) -> Result<Vec<Route>, Box<dyn Error>> {
    asked
        .iter()
        .map(|route| {
            let body = body_of(&route.method, &route.path);
            let reply = http(address, &route.method, &route.path, body)?;
            Ok(Route {
                method: route.method.clone(),
                path: route.path.clone(),
                status: reply.status,
                content_type: reply.content_type,
                keys: keys_of(&reply.body),
            })
        })
        .collect()
}

/// The handshake of `/ingest`, answered for the recorded request key.
fn handshake_answer() -> Result<(u16, Option<String>, Value), Box<dyn Error>> {
    let recorded: Value = serde_json::from_str(&read(&find_above(
        "testdata/baseline/wire/ingest_handshake.json",
    )?)?)?;
    let key = text_field(&recorded, "request_key")?;
    let (tx, _audio) = feed::channel();
    let state = ingest::IngestState::new(Hub::new(), tx, Arc::new(Strings::embedded(Lang::En)?));
    let address = server::serve(ingest::router(state), server::bind(0)?, "ingest-test")?;
    let mut stream = TcpStream::connect(address)?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let request = ws::upgrade_request("127.0.0.1", address.port(), "/ingest", &key);
    stream.write_all(request.as_bytes())?;
    let mut head = Vec::new();
    let mut byte = [0_u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte)?;
        head.push(byte[0]);
    }
    let (status, accept) = ws::parse_upgrade_response(&String::from_utf8_lossy(&head))?;
    Ok((status, accept, recorded))
}

fn check_handshake(recorded: &Value, status: u16, accept: Option<&str>) -> Result<(), Failure> {
    let want_status = recorded.get("response_status").and_then(Value::as_u64);
    if want_status != Some(u64::from(status)) {
        return Err(format!(
            "the handshake answered {status}, the recording {want_status:?}"
        ));
    }
    let want_accept = recorded.get("response_accept").and_then(Value::as_str);
    if want_accept != accept {
        return Err(format!(
            "the accept value is {accept:?}, the recording {want_accept:?}"
        ));
    }
    Ok(())
}

/// The answers with one route's status and another's keys changed.
fn damaged(found: &[Route]) -> (Vec<Route>, Vec<Route>) {
    let mut off = found.to_vec();
    for route in &mut off {
        if route.path == "/config" && route.method == "GET" {
            route.status = 500;
        }
    }
    let mut thin = found.to_vec();
    for route in &mut thin {
        if route.path == "/itn_overrides" && route.method == "GET" {
            route.keys.remove("force");
        }
    }
    (off, thin)
}

/// T-RP-12-010
#[test]
fn routes_and_handshake_match_status_and_keys() -> TestResult {
    let recorded = recorded_routes()?;
    assert_eq!(
        recorded.len(),
        11,
        "the recording holds the routes and their methods"
    );
    let (address, _held) = serve_routes(true)?;
    let found = answered_routes(address, &recorded)?;
    check_routes(&recorded, &found)?;
    let (status, accept, handshake) = handshake_answer()?;
    check_handshake(&handshake, status, accept.as_deref())?;

    let (off, thin) = damaged(&found);
    let said = check_routes(&recorded, &off).err().ok_or("it passed")?;
    assert!(said.contains("GET /config"), "{said}");
    let said = check_routes(&recorded, &thin).err().ok_or("it passed")?;
    assert!(said.contains("/itn_overrides"), "{said}");
    Ok(())
}

/// A reset the loop does not answer in time is accepted as pending, the other answer the recording holds.
#[test]
fn a_reset_behind_a_busy_loop_answers_202_pending() -> TestResult {
    let (address, _held) = serve_routes(false)?;
    let reply = http(address, "POST", "/reset", "")?;
    assert_eq!(reply.status, 202);
    let body: Value = serde_json::from_str(&reply.body)?;
    assert_eq!(body, json!({"ok": false, "pending": true}));
    Ok(())
}

/// The control routes change what the view shows, and a bad body changes nothing.
#[test]
fn control_routes_apply_valid_bodies_and_refuse_bad_ones() -> TestResult {
    let (address, _held) = serve_routes(true)?;
    let pairs = r#"{"b":"1","a":"2"}"#;
    assert_eq!(http(address, "POST", "/replacements", pairs)?.status, 200);
    assert_eq!(http(address, "GET", "/replacements", "")?.body, pairs);
    let bad = http(address, "POST", "/replacements", "[1]")?;
    assert_eq!(bad.status, 400);
    assert!(bad.body.contains("JSON object"), "{}", bad.body);
    assert_eq!(http(address, "GET", "/replacements", "")?.body, pairs);
    assert_eq!(http(address, "POST", "/replacements", "")?.status, 400);

    let itn = r#"{"exclude":["二"],"force":{"x":"y"}}"#;
    assert_eq!(http(address, "POST", "/itn_overrides", itn)?.status, 200);
    let shown: Value = serde_json::from_str(&http(address, "GET", "/itn_overrides", "")?.body)?;
    assert_eq!(shown, json!({"exclude": ["二"], "force": {"x": "y"}}));

    let punct = http(address, "POST", "/config", r#"{"punctuate":false}"#)?;
    assert_eq!(punct.status, 200);
    let config: Value = serde_json::from_str(&http(address, "GET", "/config", "")?.body)?;
    assert_eq!(config["punctuate"], false);
    assert_eq!(config["vad"]["min_silence"], 0.35);
    let too_high = r#"{"vad":{"threshold":2}}"#;
    assert_eq!(http(address, "POST", "/config", too_high)?.status, 400);
    assert_eq!(http(address, "GET", "/nothing", "")?.status, 404);
    Ok(())
}

/// The acceptance test of RP-12: the events and the routes the Rust server
/// makes match the recording of the Python server.
#[test]
fn rp12_wire_format_matches_python() -> TestResult {
    let recorded_events = recorded_events()?;
    check_wire(&recorded_events, &rust_samples(), &LATER)?;
    let recorded = recorded_routes()?;
    let (address, _held) = serve_routes(true)?;
    check_routes(&recorded, &answered_routes(address, &recorded)?)?;
    // A key taken out of an event is found.
    let thin = rust_samples()
        .into_iter()
        .map(|event| match kind_of(&event) {
            Ok("final") => without_key(event, "lang"),
            _ => Ok(event),
        })
        .collect::<Result<Vec<Value>, Failure>>()?;
    let said = check_wire(&recorded_events, &thin, &LATER)
        .err()
        .ok_or("it passed")?;
    assert!(said.contains("`lang`"), "{said}");
    Ok(())
}

/// T-RP-12-011: the Dart client's tests, run where Dart is installed.
#[test]
#[ignore = "runs the Dart toolchain on the mobile package"]
fn mobile_dart_tests_pass_with_rust_fixtures() -> TestResult {
    let package = find_above("mobile/hayamimi_core")?;
    let status = std::process::Command::new("dart")
        .args(["test", "test/remote_event_test.dart"])
        .current_dir(package)
        .status()
        .map_err(|error| format!("cannot run dart: {error}"))?;
    assert!(status.success());
    Ok(())
}

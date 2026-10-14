//! RP-46: a session that reads English loads the English recognizer before it
//! answers `ready`, and the time of its first utterance does not hold that
//! load. A session that reads Japanese loads the Japanese recognizer before
//! `ready` and no English one.
//!
//! The order is read from the lines the program prints. The program prints
//! `asr.model.loaded` while it opens, answers `ready` once the session has
//! started, and prints `ingest.client.connected` right after it sent `ready`,
//! so a load that stands before that line was done before `ready` was sent.
//!
//! `--ws-port 0` does not tell the client which port the system chose, so the
//! runs ask for a port that was free a moment ago, as RP-11 does.

mod support;

use std::error::Error;
use std::io::Write;
use std::sync::Arc;
use std::time::{Duration, Instant};

use hayamimi_app::feed;
use hayamimi_app::ingest::{IngestState, router as ingest_router};
use hayamimi_app::server::{self, Hub};
use hayamimi_app::strings::Strings;
use hayamimi_eval::ingest::{self, Ingest, Received};
use hayamimi_eval::wav::read_wav;
use hayamimi_eval::ws::{self, FrameReader, Incoming, OP_TEXT};
use hayamimi_relay::lang::Lang;
use serde_json::Value;
use support::{Failure, TestResult, events, find_above, models_dir, named, path_text};

const CLIP: &str = "testdata/eval_real/en_01.wav";
/// The silence after each copy of the clip, so that the detector cuts an utterance after it.
const GAP_SECONDS: usize = 2;
/// The sample rate of the clip and of the stream.
const RATE: usize = 16_000;
/// How long a client keeps trying to reach a server that is still starting.
const CONNECT_WITHIN: Duration = Duration::from_secs(120);
/// The first utterance may take this many times the second one and still be of its order.
const SAME_ORDER: f64 = 10.0;

/// The position of the first event named `name`, of `model` when one is given.
fn position(all: &[Value], name: &str, model: Option<&str>) -> Option<usize> {
    all.iter().position(|event| {
        event.get("event").and_then(Value::as_str) == Some(name)
            && model.is_none_or(|wanted| event.get("model").and_then(Value::as_str) == Some(wanted))
    })
}

/// `model` was loaded before the client was answered `ready`.
fn check_loaded_before_ready(all: &[Value], model: &str) -> Result<(), Failure> {
    let Some(loaded) = position(all, "asr.model.loaded", Some(model)) else {
        return Err(format!("`{model}` was never loaded"));
    };
    let Some(ready) = position(all, "ingest.client.connected", None) else {
        return Err(String::from("the client was never answered `ready`"));
    };
    if loaded < ready {
        Ok(())
    } else {
        Err(format!(
            "`{model}` was loaded at line {loaded}, after the client was answered `ready` at line {ready}"
        ))
    }
}

/// The load of `model`, in milliseconds, as the program reported it.
fn load_ms_of(all: &[Value], model: &str) -> Result<f64, Failure> {
    named(all, "asr.model.loaded")
        .into_iter()
        .filter(|event| event.get("model").and_then(Value::as_str) == Some(model))
        .find_map(|event| event.get("load_ms").and_then(Value::as_f64))
        .ok_or_else(|| format!("no load time was reported for `{model}`"))
}

/// The first utterance's time holds no model load, and is of the order of the second one's.
fn check_first_decode_excludes_load(first: f64, second: f64, load_ms: f64) -> Result<(), Failure> {
    if first >= load_ms {
        return Err(format!(
            "the first decode_ms {first} holds the model load of {load_ms}"
        ));
    }
    if first > second * SAME_ORDER {
        return Err(format!(
            "the first decode_ms {first} is not of the order of the second {second}"
        ));
    }
    Ok(())
}

/// The `decode_ms` of each `final` a client heard, in order.
fn decode_times(received: &Received) -> Vec<f64> {
    received
        .events
        .iter()
        .filter_map(|(_, text)| serde_json::from_str::<Value>(text).ok())
        .filter(|event| event.get("type").and_then(Value::as_str) == Some("final"))
        .filter_map(|event| event.get("decode_ms").and_then(Value::as_f64))
        .collect()
}

/// A client connected as soon as the server listens, which is before the models are open.
fn connect_early(port: u16) -> Result<Ingest, Box<dyn Error>> {
    let origin = Instant::now();
    let deadline = origin + CONNECT_WITHIN;
    loop {
        match ingest::open("127.0.0.1", port, origin) {
            Ok(client) => return Ok(client),
            Err(error) if Instant::now() > deadline => return Err(error.into()),
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

/// What a run of the program printed, and what its client heard.
struct Run {
    printed: Vec<Value>,
    received: Received,
    /// From starting the program to being answered `ready`.
    ready_after: Duration,
}

/// Starts the program for `--lang <lang>`, connects a client at once, and
/// plays the clip twice when `play` is set; stops the program afterwards.
fn run_session(lang: &str, play: bool) -> Result<Run, Box<dyn Error>> {
    let models = path_text(&models_dir()?)?;
    let port = support::free_port()?.to_string();
    let args = [
        "--input",
        "ws",
        "--ws-port",
        &port,
        "--lang",
        lang,
        "--models",
        &models,
    ];
    let started = Instant::now();
    let mut child = support::spawn(&args, Some("C.UTF-8"), &[])?;
    let printed = support::collect(child.stdout.take().ok_or("no stdout pipe")?);
    let received = drive_client(port.parse()?, play);
    child.kill()?;
    child.wait()?;
    let (received, ready_after) = received?;
    let printed = printed.join().map_err(|_| "the reader thread panicked")?;
    Ok(Run {
        printed: events(&printed)?,
        received,
        ready_after: ready_after.duration_since(started),
    })
}

/// The client side of a run: connect, wait for `ready`, play, and listen.
fn drive_client(port: u16, play: bool) -> Result<(Received, Instant), Box<dyn Error>> {
    let mut client = connect_early(port)?;
    client.start()?;
    let ready_at = Instant::now();
    let drain = if play {
        let wav = read_wav(&find_above(CLIP)?)?;
        let plan = ingest::plan(wav.samples.len(), &[], 2, GAP_SECONDS * RATE);
        client.play(&plan, &wav.samples)?;
        Duration::from_secs(20)
    } else {
        // The program prints its line for the client right after it sent `ready`.
        Duration::from_millis(500)
    };
    Ok((client.finish(drain)?, ready_at))
}

/// RP-46
#[test]
#[ignore = "runs the hayamimi binary with the models"]
fn rp46_english_recognizer_loads_before_ready() -> TestResult {
    let run = run_session("en", true)?;
    check_loaded_before_ready(&run.printed, "v3")?;
    let loaded = named(&run.printed, "asr.model.loaded")
        .into_iter()
        .filter(|event| event.get("model").and_then(Value::as_str) == Some("v3"))
        .count();
    assert_eq!(loaded, 1, "v3 was loaded {loaded} times");
    let times = decode_times(&run.received);
    let [first, second, ..] = times[..] else {
        return Err(format!("{} final(s) came out, two are needed", times.len()).into());
    };
    check_first_decode_excludes_load(first, second, load_ms_of(&run.printed, "v3")?)?;
    println!(
        "rp46: en ready after {:?}; first decode_ms {first}, second {second}",
        run.ready_after
    );
    Ok(())
}

/// RP-46, the control: a Japanese session loads the Japanese recognizer
/// before `ready` and reads no English one, so its start is what it was.
#[test]
#[ignore = "runs the hayamimi binary with the models"]
fn rp46_japanese_session_loads_only_the_japanese_recognizer_before_ready() -> TestResult {
    let run = run_session("ja", false)?;
    check_loaded_before_ready(&run.printed, "rz")?;
    assert!(
        position(&run.printed, "asr.model.loaded", Some("v3")).is_none(),
        "a Japanese session loaded the English recognizer"
    );
    println!("rp46: ja ready after {:?}", run.ready_after);
    Ok(())
}

/// The next text frame within `within`, as JSON; `None` when none came.
fn next_text(
    reader: &mut FrameReader<std::net::TcpStream>,
    within: Duration,
) -> Result<Option<Value>, Box<dyn Error>> {
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        match reader.next_frame()? {
            Incoming::Frame(frame) if frame.opcode == OP_TEXT => {
                return Ok(Some(serde_json::from_slice(&frame.payload)?));
            }
            Incoming::Closed => return Err("the server closed the connection".into()),
            Incoming::Frame(_) | Incoming::Idle => {}
        }
    }
    Ok(None)
}

/// A client that connected to the `/ingest` of `hub` and sent its config.
struct ConfiguredClient {
    reader: FrameReader<std::net::TcpStream>,
    /// Held so that the connection stays open.
    _stream: std::net::TcpStream,
}

fn send_config(hub: &Hub) -> Result<ConfiguredClient, Box<dyn Error>> {
    let (audio, _queue) = feed::channel();
    let state = IngestState::new(hub.clone(), audio, Arc::new(Strings::embedded(Lang::En)?));
    let address = server::serve(ingest_router(state), server::bind(0)?, "ingest-test")?;
    let connection = ws::connect(
        "127.0.0.1",
        address.port(),
        "/ingest",
        Duration::from_secs(5),
    )?;
    connection
        .stream
        .set_read_timeout(Some(Duration::from_millis(100)))?;
    let reader = FrameReader::new(connection.stream.try_clone()?, connection.leftover);
    let mut stream = connection.stream;
    stream.write_all(&ws::client_frame(OP_TEXT, b"{}"))?;
    Ok(ConfiguredClient {
        reader,
        _stream: stream,
    })
}

fn type_of(event: &Option<Value>) -> Option<&str> {
    event.as_ref().and_then(|event| event["type"].as_str())
}

/// `/ingest` answers a client's config with `ready` only once the session has
/// started, which is after every model the session needs is loaded.
#[test]
fn ready_waits_for_the_session_to_start() -> TestResult {
    let hub = Hub::new();
    let mut client = send_config(&hub)?;
    let early = next_text(&mut client.reader, Duration::from_millis(600))?;
    assert!(
        early.is_none(),
        "`ready` came before the session started: {early:?}"
    );
    hub.publish_session_start();
    let answered = next_text(&mut client.reader, Duration::from_secs(5))?;
    assert_eq!(type_of(&answered), Some("ready"));
    Ok(())
}

/// A session that has started already answers `ready` at once.
#[test]
fn ready_is_answered_at_once_after_the_session_started() -> TestResult {
    let hub = Hub::new();
    hub.publish_session_start();
    let mut client = send_config(&hub)?;
    let answered = next_text(&mut client.reader, Duration::from_secs(5))?;
    assert_eq!(type_of(&answered), Some("ready"));
    Ok(())
}

fn loaded_line(model: &str, load_ms: f64) -> Value {
    serde_json::json!({"event": "asr.model.loaded", "model": model, "load_ms": load_ms})
}

fn named_line(event: &str) -> Value {
    serde_json::json!({ "event": event })
}

#[test]
fn a_load_before_the_client_was_answered_passes() -> TestResult {
    let all = [
        loaded_line("rz", 900.0),
        loaded_line("v3", 8000.0),
        named_line("ingest.client.connected"),
    ];
    check_loaded_before_ready(&all, "v3")?;
    Ok(())
}

/// The shape RP-46 exists to refuse: the load comes with the first utterance.
#[test]
fn a_load_after_the_client_was_answered_is_refused() {
    let all = [
        loaded_line("rz", 900.0),
        named_line("ingest.client.connected"),
        named_line("asr.utterance.timed"),
        loaded_line("v3", 8000.0),
    ];
    let said = check_loaded_before_ready(&all, "v3").err();
    assert!(
        said.as_deref()
            .is_some_and(|text| text.contains("after the client")),
        "{said:?}"
    );
}

#[test]
fn a_model_that_was_never_loaded_or_a_client_never_answered_is_refused() {
    let none = [named_line("ingest.client.connected")];
    assert!(check_loaded_before_ready(&none, "v3").is_err());
    let unanswered = [loaded_line("v3", 8000.0)];
    assert!(check_loaded_before_ready(&unanswered, "v3").is_err());
}

#[test]
fn the_load_time_is_read_from_the_model_it_names() {
    let all = [loaded_line("rz", 900.0), loaded_line("v3", 8000.0)];
    assert_eq!(load_ms_of(&all, "v3"), Ok(8000.0));
    assert!(load_ms_of(&all, "pz").is_err());
}

#[test]
fn a_first_decode_without_the_load_is_of_the_order_of_the_second() {
    assert_eq!(
        check_first_decode_excludes_load(900.0, 700.0, 8000.0),
        Ok(())
    );
}

/// The first utterance of the old shape: the load of 7980 ms is inside 8400 ms.
#[test]
fn a_first_decode_that_holds_the_load_is_refused() {
    let said = check_first_decode_excludes_load(8401.0, 420.0, 7979.6).err();
    assert!(
        said.as_deref()
            .is_some_and(|text| text.contains("holds the model load")),
        "{said:?}"
    );
}

#[test]
fn a_first_decode_far_slower_than_the_second_is_refused() {
    let said = check_first_decode_excludes_load(4000.0, 300.0, 8000.0).err();
    assert!(
        said.as_deref().is_some_and(|text| text.contains("order")),
        "{said:?}"
    );
}

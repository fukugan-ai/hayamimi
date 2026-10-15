//! RP-11: it runs on WSL. Both inputs, `--wav` and `/ingest`, make `final`s,
//! and the sessions the run created sit where design 3.5 puts them (the
//! stages decided as GPU on `cuda`, the others on `cpu`). A client that
//! declares a sample rate that does not match its audio gets worse text, and
//! the check says so.
//!
//! The `/ingest` server is also exercised here as it is: a real WebSocket
//! client against the server of this crate, with no model loaded.

mod support;

use std::error::Error;
use std::io::Write;
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;
use std::time::{Duration, Instant};

use hayamimi_app::feed::{self, AudioItem, AudioRx};
use hayamimi_app::ingest::{self, IngestState};
use hayamimi_app::server::{self, Hub};
use hayamimi_app::strings::Strings;
use hayamimi_eval::ws::{self, FrameReader, Incoming, OP_BINARY, OP_CLOSE, OP_TEXT};
use hayamimi_relay::lang::Lang;
use serde_json::{Value, json};
use support::{Failure, TestResult, events, models_dir, named, path_text};

const WAIT: Duration = Duration::from_secs(5);

/// What one input made: how many `final`s, and the model sessions with the
/// provider each ran on.
#[derive(Debug, Clone, PartialEq, Eq)]
struct InputRun {
    finals: usize,
    sessions: Vec<(String, String)>,
}

/// The provider design 3.5 gives a model: the punctuation and the language
/// detection are decided as GPU, every other model as CPU.
fn expected_provider(model: &str) -> &'static str {
    if model.starts_with("punct_bert") || model.starts_with("tiny-") {
        "cuda"
    } else {
        "cpu"
    }
}

fn check_placement(label: &str, run: &InputRun) -> Result<(), Failure> {
    for (model, provider) in &run.sessions {
        let want = expected_provider(model);
        if provider != want {
            return Err(format!(
                "{label}: `{model}` ran on {provider}, design 3.5 places it on {want}"
            ));
        }
    }
    Ok(())
}

/// Both inputs made `final`s, and both created their sessions where the table puts them.
fn check_both_inputs(wav: &InputRun, ingest: &InputRun) -> Result<(), Failure> {
    if wav.finals == 0 {
        return Err(String::from("--wav: no `final` came out"));
    }
    if ingest.finals == 0 {
        return Err(String::from("/ingest: no `final` came out"));
    }
    check_placement("--wav", wav)?;
    check_placement("/ingest", ingest)
}

/// A declaration that does not match the audio must make the text worse.
/// The error is the finding that it did, which is what the check is for.
fn check_declaration_effect(right_cer: f64, declared_cer: f64) -> Result<(), Failure> {
    if declared_cer > right_cer {
        return Err(format!(
            "the declared rate raised the CER from {right_cer} to {declared_cer}"
        ));
    }
    Ok(())
}

fn run_of(finals: usize, sessions: &[(&str, &str)]) -> InputRun {
    InputRun {
        finals,
        sessions: sessions
            .iter()
            .map(|(model, provider)| ((*model).to_owned(), (*provider).to_owned()))
            .collect(),
    }
}

fn placed() -> Vec<(&'static str, &'static str)> {
    vec![
        ("silero_vad.onnx", "cpu"),
        ("encoder-epoch-35-avg-1.int8.onnx", "cpu"),
        ("tiny-encoder.onnx", "cuda"),
        ("punct_bert.onnx", "cuda"),
    ]
}

/// T-RP-11-001
#[test]
fn both_inputs_with_cuda_sessions_pass() -> TestResult {
    check_both_inputs(&run_of(2, &placed()), &run_of(1, &placed()))?;
    Ok(())
}

/// T-RP-11-002
#[test]
fn ingest_run_without_final_is_rejected() {
    let said = check_both_inputs(&run_of(2, &placed()), &run_of(0, &placed()));
    assert!(said.is_err_and(|text| text.contains("/ingest")));
}

/// T-RP-11-003
#[test]
fn wav_run_on_cpu_is_rejected() {
    let on_cpu = [("tiny-encoder.onnx", "cpu")];
    let said = check_both_inputs(&run_of(2, &on_cpu), &run_of(1, &placed()));
    assert!(said.is_err_and(|text| text.contains("tiny-encoder.onnx") && text.contains("--wav")));
}

/// T-RP-11-004
#[test]
fn mismatched_sample_rate_declaration_raises_cer() {
    assert!(check_declaration_effect(0.05, 0.40).is_err_and(|text| text.contains("declared")));
    // The same audio declared right does not raise it.
    assert!(check_declaration_effect(0.05, 0.05).is_ok());
}

/// An `/ingest` server on a free port, and the queue it feeds.
struct Rig {
    port: u16,
    audio: AudioRx,
    hub: Hub,
}

fn rig() -> Result<Rig, Box<dyn Error>> {
    let hub = Hub::new();
    // `/ingest` answers `ready` once the session has started.
    hub.publish_session_start();
    let (tx, audio) = feed::channel();
    let strings = Arc::new(Strings::embedded(Lang::En)?);
    let state = IngestState::new(hub.clone(), tx, strings);
    let address: SocketAddr =
        server::serve(ingest::router(state), server::bind(0)?, "ingest-test")?;
    Ok(Rig {
        port: address.port(),
        audio,
        hub,
    })
}

/// A WebSocket client of `/ingest`.
struct Client {
    stream: TcpStream,
    reader: FrameReader<TcpStream>,
}

impl Client {
    fn connect(port: u16) -> Result<Client, Box<dyn Error>> {
        let connection = ws::connect("127.0.0.1", port, "/ingest", WAIT)?;
        connection
            .stream
            .set_read_timeout(Some(Duration::from_millis(100)))?;
        let reader = FrameReader::new(connection.stream.try_clone()?, connection.leftover);
        Ok(Client {
            stream: connection.stream,
            reader,
        })
    }

    fn send_text(&mut self, text: &str) -> Result<(), Box<dyn Error>> {
        self.stream
            .write_all(&ws::client_frame(OP_TEXT, text.as_bytes()))?;
        Ok(())
    }

    /// Sends 16-bit samples as one binary frame.
    fn send_pcm(&mut self, samples: &[i16]) -> Result<(), Box<dyn Error>> {
        let bytes: Vec<u8> = samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect();
        self.stream
            .write_all(&ws::client_frame(OP_BINARY, &bytes))?;
        Ok(())
    }

    fn close(mut self) -> Result<(), Box<dyn Error>> {
        self.stream.write_all(&ws::client_frame(OP_CLOSE, &[]))?;
        Ok(())
    }

    /// The next text frame, as JSON; the connection closing first is an error.
    fn next_event(&mut self) -> Result<Value, Box<dyn Error>> {
        let deadline = Instant::now() + WAIT;
        while Instant::now() < deadline {
            match self.reader.next_frame()? {
                Incoming::Frame(frame) if frame.opcode == OP_TEXT => {
                    return Ok(serde_json::from_slice(&frame.payload)?);
                }
                Incoming::Frame(frame) if frame.opcode == OP_CLOSE => {
                    return Err("the server closed the connection".into());
                }
                Incoming::Closed => return Err("the server closed the connection".into()),
                Incoming::Frame(_) | Incoming::Idle => {}
            }
        }
        Err("no event came in time".into())
    }

    /// Whether the server closes the connection within the wait.
    fn is_closed_by_server(&mut self) -> Result<bool, Box<dyn Error>> {
        let deadline = Instant::now() + WAIT;
        while Instant::now() < deadline {
            match self.reader.next_frame()? {
                Incoming::Closed => return Ok(true),
                Incoming::Frame(frame) if frame.opcode == OP_CLOSE => return Ok(true),
                Incoming::Frame(_) | Incoming::Idle => {}
            }
        }
        Ok(false)
    }
}

/// Connects and sends a config; returns the client after `ready`.
fn ready_client(port: u16, config: &str) -> Result<Client, Box<dyn Error>> {
    let mut client = Client::connect(port)?;
    client.send_text(config)?;
    let ready = client.next_event()?;
    assert_eq!(ready["type"], "ready", "{ready}");
    assert_eq!(ready["sr"], 16_000);
    Ok(client)
}

/// The windows that arrive on the queue within the wait, until `count` came or time ran out.
fn windows(audio: &AudioRx, count: usize) -> Vec<Vec<f32>> {
    let deadline = Instant::now() + WAIT;
    let mut found = Vec::new();
    while found.len() < count && Instant::now() < deadline {
        if let Ok(AudioItem::Window(window)) = audio.recv_timeout(Duration::from_millis(100)) {
            found.push(window);
        }
    }
    found
}

fn pcm_of(count: usize) -> Vec<i16> {
    (0..count).map(|n| ((n % 200) as i16 - 100) * 100).collect()
}

/// A client of 16 kHz makes one window per 512 samples; a client that declares
/// 8 kHz is stretched to twice as many, which is what T-RP-11-004 makes worse.
#[test]
fn the_declared_rate_decides_how_many_windows_the_audio_makes() -> TestResult {
    let rig = rig()?;
    let mut right = ready_client(
        rig.port,
        r#"{"sr":16000,"format":"pcm_s16le","channels":1}"#,
    )?;
    right.send_pcm(&pcm_of(3200))?;
    let at_16k = windows(&rig.audio, 6);
    right.close()?;
    assert_eq!(at_16k.len(), 6);
    assert!(at_16k.iter().all(|window| window.len() == 512));
    // The close of the first client is followed by a flush; wait for it and for the place to be free.
    assert_eq!(rig.audio.recv_timeout(WAIT), Ok(AudioItem::Flush));
    std::thread::sleep(Duration::from_millis(100));

    let mut stretched = ready_client(rig.port, r#"{"sr":8000,"format":"pcm_s16le","channels":1}"#)?;
    stretched.send_pcm(&pcm_of(3200))?;
    let at_8k = windows(&rig.audio, 12);
    assert!((12..=13).contains(&at_8k.len()), "{}", at_8k.len());
    Ok(())
}

#[test]
fn stereo_audio_is_mixed_down_to_one_channel() -> TestResult {
    let rig = rig()?;
    let mut client = ready_client(
        rig.port,
        r#"{"sr":16000,"format":"pcm_s16le","channels":2}"#,
    )?;
    // 1024 interleaved samples are 512 frames: one window.
    client.send_pcm(&vec![8192; 1024])?;
    let got = windows(&rig.audio, 1);
    assert_eq!(got.len(), 1);
    assert!(got[0].iter().all(|sample| (sample - 0.25).abs() < 1e-6));
    Ok(())
}

#[test]
fn a_second_client_is_refused_while_the_first_streams() -> TestResult {
    let rig = rig()?;
    let mut first = ready_client(rig.port, r#"{"sr":16000}"#)?;
    let mut second = Client::connect(rig.port)?;
    let refusal = second.next_event()?;
    assert_eq!(refusal["type"], "error", "{refusal}");
    assert!(second.is_closed_by_server()?);
    // The first client is unaffected.
    first.send_pcm(&pcm_of(512))?;
    assert_eq!(windows(&rig.audio, 1).len(), 1);
    Ok(())
}

#[test]
fn a_client_that_leaves_flushes_the_detector_and_frees_the_place() -> TestResult {
    let rig = rig()?;
    let mut first = ready_client(rig.port, "{}")?;
    first.send_pcm(&pcm_of(512))?;
    assert_eq!(windows(&rig.audio, 1).len(), 1);
    first.close()?;
    assert_eq!(rig.audio.recv_timeout(WAIT), Ok(AudioItem::Flush));
    std::thread::sleep(Duration::from_millis(100));
    let again = ready_client(rig.port, "{}");
    assert!(again.is_ok());
    Ok(())
}

#[test]
fn a_client_that_never_sent_its_config_leaves_no_flush() -> TestResult {
    let rig = rig()?;
    let silent = Client::connect(rig.port)?;
    silent.close()?;
    assert!(rig.audio.recv_timeout(Duration::from_millis(300)).is_err());
    Ok(())
}

#[test]
fn a_bad_config_is_answered_with_an_error_and_the_connection_closes() -> TestResult {
    let rig = rig()?;
    for config in [r#"{"format":"f32le"}"#, "not json", "[1]", r#"{"sr":0}"#] {
        let mut client = Client::connect(rig.port)?;
        client.send_text(config)?;
        let answer = client.next_event()?;
        assert_eq!(answer["type"], "error", "{config}: {answer}");
        assert!(client.is_closed_by_server()?, "{config}");
    }
    Ok(())
}

#[test]
fn audio_sent_before_the_config_is_dropped() -> TestResult {
    let rig = rig()?;
    let mut client = Client::connect(rig.port)?;
    client.send_pcm(&pcm_of(512))?;
    client.send_text("{}")?;
    assert_eq!(client.next_event()?["type"], "ready");
    assert!(rig.audio.recv_timeout(Duration::from_millis(300)).is_err());
    Ok(())
}

#[test]
fn the_events_of_the_hub_are_mirrored_to_the_client_without_a_replay() -> TestResult {
    let rig = rig()?;
    rig.hub.publish_session_start();
    let mut client = ready_client(rig.port, "{}")?;
    rig.hub.publish(&json!({"type": "partial", "text": "こん"}));
    let event = client.next_event()?;
    assert_eq!(event["type"], "partial", "{event}");
    Ok(())
}

/// The log events a run printed that name a model session: model and provider.
fn sessions_of(stdout: &str) -> Result<Vec<(String, String)>, Failure> {
    let all = events(stdout)?;
    Ok(named(&all, "gpu.session.created")
        .into_iter()
        .filter_map(|event| {
            let model = event.get("model")?.as_str()?;
            let provider = event.get("actual_provider")?.as_str()?;
            Some((model.to_owned(), provider.to_owned()))
        })
        .collect())
}

fn count_finals(wire: &[Value]) -> usize {
    wire.iter()
        .filter(|event| event.get("type").and_then(Value::as_str) == Some("final"))
        .count()
}

fn wav_run() -> Result<InputRun, Failure> {
    let wav = path_text(&support::clip())?;
    let models = path_text(&models_dir()?)?;
    let served = support::serve(
        &["--wav", &wav, "--no-realtime", "--models", &models],
        Some("C.UTF-8"),
        &[],
    )?;
    Ok(InputRun {
        finals: count_finals(&served.wire),
        sessions: sessions_of(&served.output.stdout)?,
    })
}

/// Plays the clip into `/ingest` of a running binary and returns what it made.
fn ingest_run() -> Result<InputRun, Box<dyn Error>> {
    let models = path_text(&models_dir()?)?;
    let port = support::free_port()?;
    let number = port.to_string();
    let mut child = support::spawn(
        &["--input", "ws", "--ws-port", &number, "--models", &models],
        Some("C.UTF-8"),
        &[],
    )?;
    let stdout = support::collect(child.stdout.take().ok_or("no stdout pipe")?);
    let received = play_clip(port);
    // The run has no end of its own: it is stopped once the audio was played and answered.
    child.kill()?;
    child.wait()?;
    let received = received?;
    let sessions = sessions_of(&stdout.join().map_err(|_| "the reader thread panicked")?)?;
    Ok(InputRun {
        finals: received
            .events
            .iter()
            .filter(|(_, text)| text.contains("\"type\":\"final\""))
            .count(),
        sessions,
    })
}

fn play_clip(port: u16) -> Result<hayamimi_eval::ingest::Received, Box<dyn Error>> {
    let wav = hayamimi_eval::wav::read_wav(&support::clip())?;
    let origin = Instant::now();
    let deadline = origin + Duration::from_secs(120);
    // The server listens before the models are open; connect as soon as it does.
    let mut client = loop {
        match hayamimi_eval::ingest::open("127.0.0.1", port, origin) {
            Ok(client) => break client,
            Err(error) if Instant::now() > deadline => return Err(error.into()),
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    };
    client.start()?;
    let gap = wav.samples.len() / 2;
    let plan = hayamimi_eval::ingest::plan(wav.samples.len(), &[], 1, gap);
    client.play(&plan, &wav.samples)?;
    // The models may still be opening when the audio ends: the answers come after they are.
    Ok(client.finish(Duration::from_secs(60))?)
}

/// T-RP-11-005
#[test]
#[ignore = "runs the hayamimi binary with the models"]
fn wsl_wav_and_ingest_reach_events_on_cuda() -> TestResult {
    let wav = wav_run()?;
    let ingest = ingest_run()?;
    check_both_inputs(&wav, &ingest)?;
    Ok(())
}

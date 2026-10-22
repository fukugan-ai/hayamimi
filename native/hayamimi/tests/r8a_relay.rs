//! R8a: the relay translates a `final` event that this program's own event
//! server serves, over the real server-sent event stream (chunked HTTP/1.1 from
//! axum), not a fake of it.
//!
//! The translator is fixed: it answers every request with one English
//! sentence. Nothing here leaves the loopback address.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use hayamimi_app::run::final_event;
use hayamimi_app::server::{self, Hub};
use hayamimi_asr::{Final, Timings};
use hayamimi_relay::follow::Backoff;
use hayamimi_relay::hub as relay_hub;
use hayamimi_relay::llm::{Completion, Llm, LlmError};
use hayamimi_relay::logging::{LogEvent, LogSink, SharedSink};
use hayamimi_relay::pipeline::{Pipeline, PipelineConfig};
use hayamimi_relay::relay::{self, Lanes, MakePipeline, RelayConfig};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Answers every request with the same English sentence.
struct Fixed;

impl Llm for Fixed {
    fn complete(&self, _request: &Completion<'_>) -> Result<String, LlmError> {
        Ok(String::from("Hello."))
    }
}

/// Sets a flag when the relay has opened the recognizer's stream.
struct OpenedFlag(Arc<AtomicBool>);

impl LogSink for OpenedFlag {
    fn emit(&mut self, event: LogEvent) {
        if event.event == "events.subscribe.opened" {
            self.0.store(true, Ordering::SeqCst);
        }
    }
}

fn utterance() -> Final {
    Final {
        id: 1,
        text: String::from("テスト"),
        lang: String::from("ja"),
        tier: "rz",
        audio_seconds: 1.5,
        t0_s: 0.25,
        t1_s: 1.75,
        end_wall_ms: 1_790_000_000_123,
        timings: Timings {
            vad_ms: 1.0,
            lid_ms: 0.0,
            decode_ms: 100.0,
            punct_ms: 10.0,
            itn_ms: 1.0,
            l_dec_ms: 120.0,
        },
        punct_error: None,
        speaker_audio: Vec::new(),
        endpoint: hayamimi_asr::endpoint::Decision {
            rule: hayamimi_asr::endpoint::Rule::Silence,
            silence_ms: 300,
            plan: None,
            held_gap_ms: None,
        },
        fallback: None,
    }
}

fn read_until(stream: &mut TcpStream, needle: &str) -> std::io::Result<String> {
    let mut seen = String::new();
    let mut buffer = [0_u8; 1024];
    while !seen.contains(needle) {
        let count = stream.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        seen.push_str(&String::from_utf8_lossy(&buffer[..count]));
    }
    Ok(seen)
}

/// Connects to `/events` of the relay and reads the response head, which
/// arrives once the client is subscribed.
fn watch_relay(address: SocketAddr) -> std::io::Result<TcpStream> {
    let mut stream = TcpStream::connect(address)?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.write_all(b"GET /events HTTP/1.1\r\nHost: localhost\r\n\r\n")?;
    read_until(&mut stream, "\r\n\r\n")?;
    Ok(stream)
}

fn wait_until(flag: &AtomicBool) -> Result<(), &'static str> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !flag.load(Ordering::SeqCst) {
        if Instant::now() > deadline {
            return Err("the relay never opened the recognizer's stream");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}

#[test]
fn r8a_the_relay_translates_a_final_event_served_by_this_programs_server() -> TestResult {
    let recognizer = Hub::new();
    let events = server::start(recognizer.clone(), server::bind(0)?)?;
    let opened = Arc::new(AtomicBool::new(false));
    let make: MakePipeline<Fixed> =
        Arc::new(|sink: &mut dyn LogSink| Pipeline::open(Fixed, &PipelineConfig::default(), sink));
    let config = RelayConfig {
        events: vec![format!("http://{events}/events")],
        lanes: Lanes::PerSource,
        backoff: Backoff::STANDARD,
        started_epoch: 1_790_000_000,
        translate: None,
        verify_dump: None,
    };
    let sink = SharedSink::new(OpenedFlag(Arc::clone(&opened)));
    let served = relay::start(relay_hub::bind(0)?, config, make, sink)?;
    let mut watch = watch_relay(served)?;
    wait_until(&opened)?;
    recognizer.publish(&final_event(&utterance(), "mic", false));
    let body = read_until(&mut watch, "\n\n")?;
    assert!(body.contains("\"type\":\"translation\""), "{body}");
    assert!(body.contains("\"id\":\"mic-1\""), "{body}");
    assert!(body.contains("\"source\":\"mic\""), "{body}");
    assert!(body.contains("\"status\":\"ok\""), "{body}");
    assert!(body.contains("\"text\":\"Hello.\""), "{body}");
    Ok(())
}

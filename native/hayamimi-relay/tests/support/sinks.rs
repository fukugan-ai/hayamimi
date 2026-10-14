//! A relay whose outside destinations are the real Zoom and Teams ones, read
//! from a `send.toml`, against a recognizer and a translator that are fake.
//! The destinations' addresses in the tests point at servers on 127.0.0.1.

#![allow(dead_code)]

use std::error::Error;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use hayamimi_relay::captions::{HttpCaptions, Kind};
use hayamimi_relay::deliver::Destination;
use hayamimi_relay::llm::{Completion, Llm, LlmError};
use hayamimi_relay::logging::{LogSink, SharedSink};
use hayamimi_relay::partial::PartialTranslation;
use hayamimi_relay::pipeline::{Pipeline, PipelineConfig};
use hayamimi_relay::relay::{Lanes, MakePipeline};
use hayamimi_relay::send_config::SendFile;
use serde_json::json;

use super::events::{Buffer, FakeRecognizer, Logs, Running, outputs, run_relay_logging};

pub const SOON: Duration = Duration::from_secs(10);

/// Answers every request with a greeting.
pub struct Greets;

impl Llm for Greets {
    fn complete(&self, _: &Completion<'_>) -> Result<String, LlmError> {
        Ok(String::from("Hello."))
    }
}

/// A relay at work and the ways to feed it and to read what it kept.
pub struct Scene {
    pub recognizer: FakeRecognizer,
    pub running: Running,
    /// The subtitle file.
    pub srt: Buffer,
}

/// Waits for `done` to hold, up to [`SOON`].
pub fn wait_until(done: impl Fn() -> bool) {
    let deadline = Instant::now() + SOON;
    while !done() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn final_event(seq: usize) -> String {
    json!({
        "type": "final", "id": format!("mic-{seq}"), "source": "mic", "lang": "ja",
        "text": "こんにちは。", "t0_s": 0.5, "t1_s": 2.0, "end_wall_ms": 1_790_000_000_000_u64,
    })
    .to_string()
}

/// The Zoom and the Teams destination, both reading `send`.
fn destinations(send: &Path, sink: &SharedSink) -> Result<Vec<Box<dyn Destination>>, String> {
    [Kind::Zoom, Kind::Teams]
        .into_iter()
        .map(|kind| {
            let file = SendFile::open(send.to_path_buf(), sink.clone())?;
            Ok(Box::new(HttpCaptions::new(kind, file)) as Box<dyn Destination>)
        })
        .collect()
}

/// Starts the relay with the two destinations reading `send`.
pub fn start(send: &Path) -> Result<Scene, Box<dyn Error>> {
    let logs = Logs::default();
    let sink = SharedSink::new(logs.clone());
    let recognizer = FakeRecognizer::start()?;
    let make: MakePipeline<Greets> =
        Arc::new(|sink: &mut dyn LogSink| Pipeline::open(Greets, &PipelineConfig::default(), sink));
    let srt = Buffer::default();
    let outputs = outputs(destinations(send, &sink)?, Some(Box::new(srt.clone())))?;
    let running = run_relay_logging(
        logs,
        vec![recognizer.url()],
        Lanes::PerSource,
        make,
        outputs,
        PartialTranslation::Off,
    )?;
    Ok(Scene {
        recognizer,
        running,
        srt,
    })
}

impl Scene {
    /// Feeds the line number `seq` and waits until its cue is in the subtitle
    /// file. The cue is written before the outside destinations are tried,
    /// so what they did is waited for by what they log.
    pub fn say(&self, seq: usize) {
        self.recognizer.send(&final_event(seq));
        self.running.translations.wait_for(seq, SOON);
        wait_until(|| self.srt.text().matches("-->").count() >= seq);
    }

    /// Waits until the log holds `count` events named `name`, and fails the
    /// test if it never does.
    pub fn wait_for_log(&self, name: &str, count: usize) {
        wait_until(|| self.running.logs.named(name).len() >= count);
        let held = self.running.logs.named(name).len();
        assert!(held >= count, "the log held {held} of {count} `{name}`");
    }
}

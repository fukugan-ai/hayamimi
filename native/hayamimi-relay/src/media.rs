//! The real steps of a video job (`video`): `ffmpeg` takes the audio out, and
//! `hayamimi` reads it, serving the lines on `/events` as it does for a live
//! run; they are collected here until its stream ends.
//!
//! Both programs are child processes. Each one is waited for, whatever way
//! the step ends, so that none is left behind.

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::follow::follow_once;
use crate::line::{Heard, Line, hear};
use crate::logging::{Level, LogEvent, LogSink};
use crate::video::{Extract, Recognize, StepFailed};

/// How long the recognizer is given to start serving.
const OPEN_LIMIT: Duration = Duration::from_secs(60);
/// How often the recognizer's address is tried while it starts.
const OPEN_POLL: Duration = Duration::from_millis(100);
/// The number the log events name the recognizer's stream by.
const ENDPOINT: usize = 0;

/// Says that a step failed, and fails it.
fn failed(sink: &mut dyn LogSink, event: &str, reason: &str) -> StepFailed {
    sink.emit(
        LogEvent::new(Level::Error, event, "worker")
            .with("reason", reason)
            .with_error("SRT_STEP"),
    );
    StepFailed
}

/// `ffmpeg`, which writes the audio of a video as 16 kHz mono 16-bit PCM.
pub struct Ffmpeg {
    pub program: PathBuf,
}

impl Ffmpeg {
    fn command(&self, video: &Path, wav: &Path) -> Command {
        let mut command = Command::new(&self.program);
        command
            .args(["-nostdin", "-v", "error", "-y", "-i"])
            .arg(video)
            .args(["-vn", "-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le"])
            .arg(wav)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
        command
    }
}

impl Extract for Ffmpeg {
    fn extract(&self, video: &Path, wav: &Path, sink: &mut dyn LogSink) -> Result<(), StepFailed> {
        match self.command(video, wav).status() {
            Ok(status) if status.success() => Ok(()),
            Ok(_) => Err(failed(sink, "srt.extract.failed", "exit")),
            Err(_) => Err(failed(sink, "srt.extract.failed", "spawn")),
        }
    }
}

/// A child process that is stopped and waited for when it goes out of scope.
struct Reaped(Child);

impl Drop for Reaped {
    fn drop(&mut self) {
        // A child that has exited already has nothing to stop; waiting for it
        // is what removes its entry from the process table.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A port on the loopback address that nothing listens on now.
fn free_port() -> std::io::Result<u16> {
    Ok(TcpListener::bind("127.0.0.1:0")?.local_addr()?.port())
}

/// `hayamimi`, run on a wav file as fast as it can read it.
pub struct Recognizer {
    pub program: PathBuf,
    /// The models directory, when the default is not the one wanted.
    pub models: Option<PathBuf>,
}

impl Recognizer {
    fn command(&self, wav: &Path, port: u16) -> Command {
        let mut command = Command::new(&self.program);
        command
            .arg("--wav")
            .arg(wav)
            .args(["--no-realtime", "--serve"])
            .arg(port.to_string());
        if let Some(models) = &self.models {
            command.arg("--models").arg(models);
        }
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
        command
    }
}

/// Keeps the line an event carries; says so when an event looks like a line
/// and cannot be used.
fn keep(data: &str, lines: &mut Vec<Line>, sink: &mut dyn LogSink) {
    match hear(data) {
        Ok(Heard::Line(line)) => lines.push(line),
        Ok(_) => {}
        Err(reason) => sink.emit(
            LogEvent::new(Level::Warn, "events.line.skipped", "service")
                .with("endpoint", ENDPOINT)
                .with("reason", reason.as_str()),
        ),
    }
}

/// Reads the stream at `url` to its end once the recognizer serves it.
/// `None` when it never did: it exited, or [`OPEN_LIMIT`] passed.
fn collect(url: &str, child: &mut Child, sink: &mut dyn LogSink) -> Option<Vec<Line>> {
    let started = Instant::now();
    loop {
        let mut lines = Vec::new();
        let on_event = &mut |data: &str, sink: &mut dyn LogSink| keep(data, &mut lines, sink);
        let (opened, _) = follow_once((ENDPOINT, url), sink, on_event);
        if opened {
            return Some(lines);
        }
        let exited = !matches!(child.try_wait(), Ok(None));
        if exited || started.elapsed() > OPEN_LIMIT {
            return None;
        }
        std::thread::sleep(OPEN_POLL);
    }
}

/// Waits for the recognizer to end and says whether it ended well.
fn ended_well(child: &mut Child) -> bool {
    child.wait().is_ok_and(|status| status.success())
}

impl Recognize for Recognizer {
    fn recognize(&self, wav: &Path, sink: &mut dyn LogSink) -> Result<Vec<Line>, StepFailed> {
        let port = free_port().map_err(|_| failed(sink, "srt.recognize.failed", "port"))?;
        let spawned = self.command(wav, port).spawn();
        let mut child = Reaped(spawned.map_err(|_| failed(sink, "srt.recognize.failed", "spawn"))?);
        let url = format!("http://127.0.0.1:{port}/events");
        let lines = collect(&url, &mut child.0, sink)
            .ok_or_else(|| failed(sink, "srt.recognize.failed", "no_stream"))?;
        if !ended_well(&mut child.0) {
            return Err(failed(sink, "srt.recognize.failed", "exit"));
        }
        Ok(lines)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::MemorySink;
    use crate::pipeline::LineKind;

    #[test]
    fn a_final_event_is_kept_as_a_line() {
        let mut lines = Vec::new();
        let mut sink = MemorySink::default();
        let event = r#"{"type":"final","id":"file-1","source":"file","lang":"en","text":"Hi.","t0_s":0.5,"t1_s":1.0}"#;
        keep(event, &mut lines, &mut sink);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].kind, LineKind::Final);
        assert!(sink.events.is_empty());
    }

    #[test]
    fn other_events_are_not_lines_and_say_nothing() {
        let mut lines = Vec::new();
        let mut sink = MemorySink::default();
        for event in [
            r#"{"type":"partial","text":"Hel"}"#,
            r#"{"type":"session_start"}"#,
            r#"{"type":"fallback","stage":"gpu"}"#,
        ] {
            keep(event, &mut lines, &mut sink);
        }
        assert!(lines.is_empty() && sink.events.is_empty());
    }

    #[test]
    fn a_line_that_lacks_what_the_relay_needs_is_said_once() {
        let mut lines = Vec::new();
        let mut sink = MemorySink::default();
        keep(r#"{"type":"final","text":"Hi."}"#, &mut lines, &mut sink);
        assert!(lines.is_empty());
        assert_eq!(sink.events.len(), 1);
        assert_eq!(sink.events[0].event, "events.line.skipped");
    }

    #[test]
    fn a_program_that_is_not_there_fails_the_step_and_says_so() {
        let ffmpeg = Ffmpeg {
            program: PathBuf::from("no-such-ffmpeg-program"),
        };
        let mut sink = MemorySink::default();
        let done = ffmpeg.extract(Path::new("a.mp4"), Path::new("a.wav"), &mut sink);
        assert_eq!(done, Err(StepFailed));
        assert_eq!(sink.events.len(), 1);
        assert_eq!(sink.events[0].event, "srt.extract.failed");
    }

    #[test]
    fn a_recognizer_that_is_not_there_fails_the_step_and_says_so() {
        let recognizer = Recognizer {
            program: PathBuf::from("no-such-hayamimi-program"),
            models: None,
        };
        let mut sink = MemorySink::default();
        let done = recognizer.recognize(Path::new("a.wav"), &mut sink);
        assert_eq!(done, Err(StepFailed));
        assert_eq!(sink.events[0].event, "srt.recognize.failed");
    }

    #[test]
    fn the_recognizer_is_asked_for_a_fast_run_on_the_port_it_was_given() {
        let recognizer = Recognizer {
            program: PathBuf::from("hayamimi"),
            models: Some(PathBuf::from("m")),
        };
        let command = recognizer.command(Path::new("a.wav"), 8844);
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            [
                "--wav",
                "a.wav",
                "--no-realtime",
                "--serve",
                "8844",
                "--models",
                "m"
            ]
        );
    }
}

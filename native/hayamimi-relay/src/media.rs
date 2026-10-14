//! The real steps of a video job (`video`): `ffmpeg` takes the audio out, and
//! `hayamimi` reads it from `/ingest`, the way a live run feeds it. The lines
//! come back on the same connection, so none that is said before the stream
//! is opened can be missed.
//!
//! Both programs are child processes. Each one is waited for, whatever way
//! the step ends, so that none is left behind.

use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::line::{Heard, Line, hear};
use crate::logging::{Level, LogEvent, LogSink};
use crate::pcm::{self, Pcm};
use crate::video::{Extract, Recognize, StepFailed};
use crate::ws_client::{self, Frame, Incoming, OP_BINARY, OP_CLOSE, OP_TEXT, Socket};

/// How long the recognizer is given to start serving.
const OPEN_LIMIT: Duration = Duration::from_secs(60);
/// How often the recognizer's address is tried while it starts.
const OPEN_POLL: Duration = Duration::from_millis(100);
/// How long one try to connect, and the upgrade, may take. The address is
/// the loopback one, which answers at once or refuses at once; a try that
/// takes longer than this is not going to succeed, and the next one is made.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
/// How long the recognizer is given to load its models, once the audio has
/// all been sent and no event has said that it has.
const LOAD_LIMIT: Duration = Duration::from_secs(60);
/// The config message that opens an ingest connection.
const CONFIG: &str = r#"{"sr":16000,"format":"pcm_s16le","channels":1}"#;
/// Bytes of audio in one frame: 20 ms of 16 kHz 16-bit samples.
const FRAME_BYTES: usize = 640;
const BYTES_PER_SECOND: u64 = 32_000;
/// The number the log events name the recognizer's stream by.
const ENDPOINT: usize = 0;
const FAILED: &str = "srt.recognize.failed";

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

/// `hayamimi`, fed a wav file through `/ingest`.
pub struct Recognizer {
    pub program: PathBuf,
    /// The models directory, when the default is not the one wanted.
    pub models: Option<PathBuf>,
    /// Silence sent after the audio, so that the last utterance is closed by
    /// the detector the way it is in a live run.
    pub tail: Duration,
    /// How long the connection may be quiet, after the audio, before the
    /// lines are taken as complete.
    pub idle: Duration,
}

impl Recognizer {
    pub const TAIL: Duration = Duration::from_secs(2);
    pub const IDLE: Duration = Duration::from_secs(8);

    /// A recognizer with the tail and the quiet time of a real run.
    pub fn new(program: PathBuf, models: Option<PathBuf>) -> Recognizer {
        Recognizer {
            program,
            models,
            tail: Recognizer::TAIL,
            idle: Recognizer::IDLE,
        }
    }

    /// `--source file` makes the lines those of a file, which are translated
    /// whichever of English and Japanese they are in.
    fn command(&self, port: u16) -> Command {
        let mut command = Command::new(&self.program);
        command
            .args(["--input", "ws", "--source", "file", "--ws-port"])
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

/// Whether the text frame is an event of type `kind`.
fn is_event(payload: &[u8], kind: &str) -> bool {
    serde_json::from_slice::<Value>(payload)
        .is_ok_and(|event| event.get("type").and_then(Value::as_str) == Some(kind))
}

/// Why a stream stopped being usable; the `reason` of the log event.
type Reason = &'static str;
const LOST: Reason = "lost";

/// Real time: when the audio sent so far would have been captured live.
struct Clock {
    begun: Instant,
    bytes: u64,
}

impl Clock {
    /// When `more` bytes after those already counted would be complete.
    fn due_after(&mut self, more: usize) -> Instant {
        self.bytes += more as u64;
        self.begun + Duration::from_micros(self.bytes * 1_000_000 / BYTES_PER_SECOND)
    }
}

/// An open `/ingest` connection and the lines heard on it so far.
struct Stream<'a> {
    socket: Socket,
    lines: Vec<Line>,
    sink: &'a mut dyn LogSink,
    /// Whether any event has come, which says the recognizer is running.
    running: bool,
}

impl Stream<'_> {
    fn hear_text(&mut self, payload: &[u8]) {
        self.running = true;
        keep(
            &String::from_utf8_lossy(payload),
            &mut self.lines,
            self.sink,
        );
    }

    /// Sends the config and waits for the recognizer's `ready`.
    fn await_ready(&mut self) -> Result<(), Reason> {
        self.socket
            .send(OP_TEXT, CONFIG.as_bytes())
            .map_err(|_| LOST)?;
        let deadline = Instant::now() + OPEN_LIMIT;
        loop {
            match self.socket.next_before(deadline).map_err(|_| LOST)? {
                Incoming::Frame(Frame {
                    opcode: OP_TEXT,
                    payload,
                }) => {
                    if is_event(&payload, "ready") {
                        return Ok(());
                    }
                    if is_event(&payload, "error") {
                        return Err("refused");
                    }
                }
                Incoming::Frame(Frame {
                    opcode: OP_CLOSE, ..
                })
                | Incoming::Closed => {
                    return Err(LOST);
                }
                Incoming::Frame(_) => {}
                Incoming::Idle => return Err("no_ready"),
            }
        }
    }

    /// Reads events until `deadline`; whether the server closed first.
    fn listen(&mut self, deadline: Instant) -> Result<bool, Reason> {
        loop {
            match self.socket.next_before(deadline).map_err(|_| LOST)? {
                Incoming::Frame(Frame {
                    opcode: OP_TEXT,
                    payload,
                }) => self.hear_text(&payload),
                Incoming::Frame(Frame {
                    opcode: OP_CLOSE, ..
                })
                | Incoming::Closed => {
                    return Ok(true);
                }
                Incoming::Frame(_) => {}
                Incoming::Idle => return Ok(false),
            }
        }
    }

    /// Sends one frame of audio when its last sample would have been captured.
    fn send_paced(&mut self, clock: &mut Clock, audio: &[u8]) -> Result<(), Reason> {
        let due = clock.due_after(audio.len());
        if self.listen(due)? {
            return Err(LOST);
        }
        self.socket.send(OP_BINARY, audio).map_err(|_| LOST)
    }

    /// Sends the samples and then the tail of silence, at the speed of speech.
    fn play<R: Read + Seek>(&mut self, pcm: &mut Pcm<R>, tail: Duration) -> Result<(), Reason> {
        let mut clock = Clock {
            begun: Instant::now(),
            bytes: 0,
        };
        let mut frame = [0_u8; FRAME_BYTES];
        loop {
            match pcm.fill(&mut frame).map_err(|_| "audio")? {
                0 => break,
                n => self.send_paced(&mut clock, &frame[..n])?,
            }
        }
        let silence = [0_u8; FRAME_BYTES];
        let mut owed = (tail.as_millis() as u64 * BYTES_PER_SECOND / 1000) & !1;
        while owed > 0 {
            let n = (owed as usize).min(FRAME_BYTES);
            self.send_paced(&mut clock, &silence[..n])?;
            owed -= n as u64;
        }
        Ok(())
    }

    /// Takes what is still coming until the connection has been quiet for
    /// `idle`. Before any event has come, the recognizer may still be loading
    /// its models, which takes longer.
    fn drain(&mut self, idle: Duration) -> Result<(), Reason> {
        loop {
            let wait = if self.running { idle } else { LOAD_LIMIT };
            match self
                .socket
                .next_before(Instant::now() + wait)
                .map_err(|_| LOST)?
            {
                Incoming::Frame(Frame {
                    opcode: OP_TEXT,
                    payload,
                }) => self.hear_text(&payload),
                // Nothing closes the connection but the recognizer going down.
                Incoming::Frame(Frame {
                    opcode: OP_CLOSE, ..
                })
                | Incoming::Closed => {
                    return Err(LOST);
                }
                Incoming::Frame(_) => {}
                Incoming::Idle => return Ok(()),
            }
        }
    }

    /// The whole exchange: ready, audio, tail, and the lines left over.
    fn run<R: Read + Seek>(
        &mut self,
        pcm: &mut Pcm<R>,
        wait: (Duration, Duration),
    ) -> Result<(), Reason> {
        self.await_ready()?;
        self.play(pcm, wait.0)?;
        self.drain(wait.1)?;
        // The server may have closed already; a failed close frame is not news.
        let _ = self.socket.send(OP_CLOSE, &[]);
        Ok(())
    }
}

/// Connects to `/ingest` once the recognizer serves it. `None` when it never
/// did: it exited, or [`OPEN_LIMIT`] passed.
fn connect_when_up(port: u16, child: &mut Child) -> Option<Socket> {
    let started = Instant::now();
    loop {
        if let Ok(socket) = ws_client::connect(port, "/ingest", CONNECT_TIMEOUT) {
            return Some(socket);
        }
        let exited = !matches!(child.try_wait(), Ok(None));
        if exited || started.elapsed() > OPEN_LIMIT {
            return None;
        }
        std::thread::sleep(OPEN_POLL);
    }
}

type WavReader = Pcm<BufReader<File>>;

impl Recognizer {
    /// Starts the program and opens its stream, or says why not.
    fn start(&self, port: u16, sink: &mut dyn LogSink) -> Result<(Reaped, Socket), StepFailed> {
        let spawned = self.command(port).spawn();
        let mut child = Reaped(spawned.map_err(|_| failed(sink, FAILED, "spawn"))?);
        let socket =
            connect_when_up(port, &mut child.0).ok_or_else(|| failed(sink, FAILED, "no_stream"))?;
        Ok((child, socket))
    }

    fn feed(
        &self,
        mut pcm: WavReader,
        port: u16,
        sink: &mut dyn LogSink,
    ) -> Result<Vec<Line>, StepFailed> {
        // Held until the lines are back: dropping it stops the program.
        let (_child, socket) = self.start(port, sink)?;
        let mut stream = Stream {
            socket,
            lines: Vec::new(),
            sink,
            running: false,
        };
        let done = stream.run(&mut pcm, (self.tail, self.idle));
        match done {
            Ok(()) => Ok(stream.lines),
            Err(reason) => Err(failed(stream.sink, FAILED, reason)),
        }
    }
}

impl Recognize for Recognizer {
    fn recognize(&self, wav: &Path, sink: &mut dyn LogSink) -> Result<Vec<Line>, StepFailed> {
        let pcm = pcm::open(wav).map_err(|_| failed(sink, FAILED, "audio"))?;
        let port = free_port().map_err(|_| failed(sink, FAILED, "port"))?;
        self.feed(pcm, port, sink)
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
    fn the_awaited_event_is_told_by_its_type() {
        assert!(is_event(br#"{"type":"ready","sr":16000}"#, "ready"));
        assert!(!is_event(br#"{"type":"final"}"#, "ready"));
        assert!(!is_event(b"not json", "ready"));
    }

    #[test]
    fn the_clock_counts_real_time_from_the_bytes_sent() {
        let begun = Instant::now();
        let mut clock = Clock { begun, bytes: 0 };
        // 640 bytes are 20 ms, and a second is 32,000 bytes.
        assert_eq!(clock.due_after(640) - begun, Duration::from_millis(20));
        assert_eq!(clock.due_after(31_360) - begun, Duration::from_secs(1));
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
    fn a_file_that_is_not_a_wav_fails_the_step_before_anything_is_started() {
        let recognizer = Recognizer::new(PathBuf::from("no-such-hayamimi-program"), None);
        let mut sink = MemorySink::default();
        let done = recognizer.recognize(Path::new("no-such-file.wav"), &mut sink);
        assert_eq!(done, Err(StepFailed));
        assert_eq!(sink.events[0].event, FAILED);
        assert_eq!(sink.events[0].fields["reason"], "audio");
    }

    #[test]
    fn the_recognizer_is_started_on_ingest_with_the_lines_of_a_file() {
        let recognizer = Recognizer::new(PathBuf::from("hayamimi"), Some(PathBuf::from("m")));
        let command = recognizer.command(8844);
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            [
                "--input",
                "ws",
                "--source",
                "file",
                "--ws-port",
                "8844",
                "--models",
                "m"
            ]
        );
    }
}

//! RP-83: a `stream` process under test (S83 of T 4.75).
//!
//! The process reads the machine from a `/proc` of the test's own (`--proc-root`)
//! and sends to a fake `/ingest` that counts the audio bytes it receives. The
//! real `/proc` is never read: the test runs under `cargo`, so the real one
//! always holds a `cargo` and every run would be refused.

use std::error::Error;
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use hayamimi_eval::ws::{self, FrameReader, Incoming, OP_BINARY, OP_TEXT};
use serde_json::{Value, json};

use super::rp32::scratch_dir;

/// The slowest a run may take before the test kills it.
const RUN_LIMIT: Duration = Duration::from_secs(90);
const POLL: Duration = Duration::from_millis(20);

type Outcome<T> = Result<T, Box<dyn Error>>;

/// A `/proc` of its own: `loadavg`, and a `<pid>/comm` for each process added.
pub struct ProcRoot {
    dir: PathBuf,
}

impl ProcRoot {
    /// A directory `target/rp83-proc/<name>` with a clean machine (`0.50` loads).
    pub fn new(name: &str) -> Outcome<ProcRoot> {
        let root = ProcRoot {
            dir: scratch_dir("rp83-proc", name)?,
        };
        root.set_loadavg("0.50 0.50 0.50 1/300 1000")?;
        Ok(root)
    }

    pub fn path(&self) -> &Path {
        &self.dir
    }

    pub fn set_loadavg(&self, line: &str) -> Outcome<()> {
        std::fs::write(self.dir.join("loadavg"), format!("{line}\n"))?;
        Ok(())
    }

    /// A `pressure/io` with these two totals, in the kernel's format.
    pub fn set_pressure(&self, some: u64, full: u64) -> Outcome<()> {
        let dir = self.dir.join("pressure");
        std::fs::create_dir_all(&dir)?;
        std::fs::write(
            dir.join("io"),
            format!(
                "some avg10=0.00 avg60=0.00 avg300=0.00 total={some}\n\
                 full avg10=0.00 avg60=0.00 avg300=0.00 total={full}\n"
            ),
        )?;
        Ok(())
    }

    /// A process `dir` whose `comm` is `comm` and a newline. `dir` need not be a pid.
    pub fn add_process(&self, dir: &str, comm: &str) -> Outcome<()> {
        let process = self.dir.join(dir);
        std::fs::create_dir_all(&process)?;
        std::fs::write(process.join("comm"), format!("{comm}\n"))?;
        Ok(())
    }

    /// The `stat` of process `pid`, in the kernel's format: `utime` and
    /// `stime` are its fields 14 and 15.
    pub fn set_stat(&self, pid: u32, comm: &str, utime: u64, stime: u64) -> Outcome<()> {
        let text = format!(
            "{pid} ({comm}) S 1 {pid} {pid} 0 -1 4194560 100 0 0 0 {utime} {stime} 0 0 20 0 8 0 \
             12345 1000000 100 18446744073709551615\n"
        );
        std::fs::write(self.dir.join(pid.to_string()).join("stat"), text)?;
        Ok(())
    }

    /// The `cmdline` of process `pid`: each word followed by a NUL, as
    /// `/proc/<pid>/cmdline` is written (proc_pid_cmdline(5)). Built here, in
    /// the test, and never read from a file of the repository.
    pub fn set_cmdline(&self, pid: u32, words: &[&str]) -> Outcome<()> {
        let bytes: Vec<u8> = words
            .iter()
            .flat_map(|word| word.bytes().chain([0]))
            .collect();
        self.set_cmdline_bytes(pid, &bytes)
    }

    /// The `cmdline` of process `pid` as these bytes.
    pub fn set_cmdline_bytes(&self, pid: u32, bytes: &[u8]) -> Outcome<()> {
        std::fs::write(self.dir.join(pid.to_string()).join("cmdline"), bytes)?;
        Ok(())
    }

    /// Takes `file` (`comm`, `stat` or `cmdline`) out of process `pid`.
    pub fn remove(&self, pid: u32, file: &str) -> Outcome<()> {
        std::fs::remove_file(self.dir.join(pid.to_string()).join(file))?;
        Ok(())
    }

    /// A process of the run: `comm` and a `stat` with these ticks.
    pub fn add_own(&self, pid: u32, comm: &str, utime: u64, stime: u64) -> Outcome<()> {
        self.add_process(&pid.to_string(), comm)?;
        self.set_stat(pid, comm, utime, stime)
    }

    /// A `llama-server` of the run, started with these words.
    pub fn add_server(&self, pid: u32, words: &[&str]) -> Outcome<()> {
        self.add_own(pid, "llama-server", 10, 10)?;
        self.set_cmdline(pid, words)
    }

    /// A `pressure/cpu` with this `total=` on its one `some` line.
    pub fn set_cpu_pressure(&self, total: u64) -> Outcome<()> {
        let dir = self.dir.join("pressure");
        std::fs::create_dir_all(&dir)?;
        std::fs::write(
            dir.join("cpu"),
            format!("some avg10=0.00 avg60=0.00 avg300=0.00 total={total}\n"),
        )?;
        Ok(())
    }
}

/// The port of the window's translation server, which S83o names.
pub const WINDOW_PORT: u16 = 18084;

/// The words the translation server of a run is started with (S83o).
pub const SERVER_WORDS: [&str; 5] = ["llama-server", "-m", "model.gguf", "--port", "18084"];

/// The three processes of a run (S83o) and what their `stat` files add up to:
/// (1000 + 250) + (600 + 100) + (1500 + 301).
pub const OWN_PIDS: [u32; 3] = [4242, 4243, 4244];
pub const OWN_TICKS: u64 = 3751;

/// A fake `/ingest`: it upgrades one connection, answers the config with
/// `ready`, and counts the bytes of the binary frames.
pub struct FakeIngest {
    pub port: u16,
    bytes: Arc<AtomicUsize>,
    done: Arc<AtomicBool>,
    handle: JoinHandle<Result<(), String>>,
}

/// The connection of a client, or `None` when `done` was set first.
fn wait_for_client(listener: &TcpListener, done: &AtomicBool) -> Result<Option<TcpStream>, String> {
    loop {
        match listener.accept() {
            Ok((socket, _)) => return Ok(Some(socket)),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if done.load(Ordering::Relaxed) {
                    return Ok(None);
                }
                std::thread::sleep(POLL);
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}

/// Answers the upgrade request that `socket` sent.
fn upgrade(socket: &mut TcpStream) -> Result<(), String> {
    use std::io::Read;
    let mut request = [0_u8; 2048];
    let read = socket.read(&mut request).map_err(|why| why.to_string())?;
    let text = String::from_utf8_lossy(&request[..read]).into_owned();
    let Some(key) = text
        .lines()
        .find_map(|line| line.strip_prefix("Sec-WebSocket-Key: "))
    else {
        return Err(String::from("the request has no Sec-WebSocket-Key"));
    };
    let reply = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
         Sec-WebSocket-Accept: {}\r\n\r\n",
        ws::accept_key(key)
    );
    socket
        .write_all(reply.as_bytes())
        .map_err(|why| why.to_string())
}

/// Reads frames until the client closes or `done` is set.
fn serve_frames(
    mut socket: TcpStream,
    bytes: &AtomicUsize,
    done: &AtomicBool,
) -> Result<(), String> {
    let reading = socket.try_clone().map_err(|why| why.to_string())?;
    let mut reader = FrameReader::new(reading, Vec::new());
    loop {
        match reader.next_frame().map_err(|why| why.to_string())? {
            Incoming::Frame(frame) if frame.opcode == OP_TEXT => {
                let ready = ws::encode_frame(OP_TEXT, br#"{"type":"ready","sr":16000}"#, None);
                socket.write_all(&ready).map_err(|why| why.to_string())?;
            }
            Incoming::Frame(frame) if frame.opcode == OP_BINARY => {
                bytes.fetch_add(frame.payload.len(), Ordering::Relaxed);
            }
            Incoming::Frame(_) => {}
            Incoming::Idle if done.load(Ordering::Relaxed) => return Ok(()),
            Incoming::Idle => {}
            Incoming::Closed => return Ok(()),
        }
    }
}

fn serve(listener: TcpListener, bytes: &AtomicUsize, done: &AtomicBool) -> Result<(), String> {
    let Some(mut socket) = wait_for_client(&listener, done)? else {
        return Ok(());
    };
    socket
        .set_nonblocking(false)
        .map_err(|why| why.to_string())?;
    socket
        .set_read_timeout(Some(Duration::from_millis(100)))
        .map_err(|why| why.to_string())?;
    upgrade(&mut socket)?;
    serve_frames(socket, bytes, done)
}

impl FakeIngest {
    /// Listens on a port the system picks.
    pub fn start() -> Outcome<FakeIngest> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let bytes = Arc::new(AtomicUsize::new(0));
        let done = Arc::new(AtomicBool::new(false));
        let handle = {
            let (bytes, done) = (Arc::clone(&bytes), Arc::clone(&done));
            std::thread::spawn(move || serve(listener, &bytes, &done))
        };
        Ok(FakeIngest {
            port,
            bytes,
            done,
            handle,
        })
    }

    /// The audio bytes received so far.
    pub fn audio_bytes(&self) -> usize {
        self.bytes.load(Ordering::Relaxed)
    }

    /// Stops listening and returns the audio bytes received in all.
    pub fn finish(self) -> Outcome<usize> {
        self.done.store(true, Ordering::Relaxed);
        self.handle
            .join()
            .map_err(|_| "the fake ingest panicked")??;
        Ok(self.bytes.load(Ordering::Relaxed))
    }
}

/// The files of one run: a silent recording, its manifest, and where the
/// output goes.
pub struct Setup {
    pub proc_root: ProcRoot,
    pub out: PathBuf,
    wav: PathBuf,
    manifest: PathBuf,
}

/// A 16 kHz silent recording of `seconds` and a manifest of one utterance over all of it.
fn write_recording(dir: &Path, seconds: u32) -> Outcome<(PathBuf, PathBuf)> {
    let wav = dir.join("silence.wav");
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&wav, spec)?;
    for _ in 0..seconds * 16_000 {
        writer.write_sample(0_i16)?;
    }
    writer.finalize()?;
    let manifest = dir.join("silence.json");
    let rows = json!([{ "start_sample": 0, "end_sample": seconds * 16_000, "ref": "a" }]);
    std::fs::write(&manifest, rows.to_string())?;
    Ok((wav, manifest))
}

impl Setup {
    /// Files for the run `name`, with a recording of `seconds`.
    pub fn new(name: &str, seconds: u32) -> Outcome<Setup> {
        let dir = scratch_dir("rp83-run", name)?;
        let (wav, manifest) = write_recording(&dir, seconds)?;
        Ok(Setup {
            proc_root: ProcRoot::new(name)?,
            out: dir.join("s83.jsonl"),
            wav,
            manifest,
        })
    }

    /// Files for the run `name` (S83o): the `/proc` also holds the three
    /// processes of a run, `hayamimi`, its relay and the server on port 18084.
    pub fn new_own(name: &str, seconds: u32) -> Outcome<Setup> {
        let setup = Setup::new(name, seconds)?;
        setup.proc_root.add_own(4242, "hayamimi", 1000, 250)?;
        setup.proc_root.add_own(4243, "hayamimi-relay", 600, 100)?;
        setup.proc_root.add_own(4244, "llama-server", 1500, 301)?;
        setup.proc_root.set_cmdline(4244, &SERVER_WORDS)?;
        Ok(setup)
    }

    /// `<dir>/s83.machine.jsonl`.
    pub fn machine_file(&self) -> PathBuf {
        self.out.with_extension("machine.jsonl")
    }

    /// The text of the machine record, or `None` when it does not stand.
    pub fn machine_text(&self) -> Option<String> {
        std::fs::read_to_string(self.machine_file()).ok()
    }

    fn command(&self, port: u16, drain_s: u32, extra: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_hayamimi-eval"));
        command
            .args(["stream", "--host", "127.0.0.1", "--port", &port.to_string()])
            .arg("--wav")
            .arg(&self.wav)
            .arg("--manifest")
            .arg(&self.manifest)
            .args(["--repeat", "1", "--gap-ms", "0"])
            .args(["--drain-s", &drain_s.to_string()])
            .arg("--out")
            .arg(&self.out)
            .arg("--proc-root")
            .arg(self.proc_root.path())
            .args(extra)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    /// Starts `stream` against `port`. `extra` are more arguments.
    pub fn spawn(&self, port: u16, drain_s: u32, extra: &[&str]) -> Outcome<Child> {
        Ok(self.command(port, drain_s, extra).spawn()?)
    }
}

/// `--own-pid <pid>` for each pid, in the order given.
pub fn own_pid_args(pids: &[u32]) -> Vec<String> {
    pids.iter()
        .flat_map(|pid| [String::from("--own-pid"), pid.to_string()])
        .collect()
}

/// `--own-pid <pid>` for each pid, in the order given, and the port the
/// run's translation server listens on as `--own-llama-port`.
pub fn own_args(pids: &[u32], llama_port: u16) -> Vec<String> {
    let mut args = own_pid_args(pids);
    args.extend([String::from("--own-llama-port"), llama_port.to_string()]);
    args
}

/// What a finished process left.
pub struct Ended {
    pub code: Option<i32>,
    pub stderr: String,
    /// How long after `since` the process had ended.
    pub elapsed: Duration,
}

impl Ended {
    /// The events of `name` the process wrote to stderr, as JSON.
    pub fn events(&self, name: &str) -> Vec<Value> {
        self.stderr
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter(|event| event["event"] == name)
            .collect()
    }
}

/// Waits for `child` to end, killing it at [`RUN_LIMIT`] (an error then).
pub fn wait_for(mut child: Child, since: Instant) -> Outcome<Ended> {
    let began = Instant::now();
    while child.try_wait()?.is_none() {
        if began.elapsed() > RUN_LIMIT {
            child.kill()?;
            child.wait()?;
            return Err("the stream process did not end in time".into());
        }
        std::thread::sleep(POLL);
    }
    let elapsed = since.elapsed();
    let Output { status, stderr, .. } = child.wait_with_output()?;
    Ok(Ended {
        code: status.code(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        elapsed,
    })
}

/// Starts `stream` and waits for it.
pub fn run(setup: &Setup, port: u16, drain_s: u32, extra: &[&str]) -> Outcome<Ended> {
    let since = Instant::now();
    wait_for(setup.spawn(port, drain_s, extra)?, since)
}

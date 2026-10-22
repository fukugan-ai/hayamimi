//! Runs `lat-dialog` and `lat-short` through the real stack for IP-2 and IP-7:
//! `hayamimi` on a websocket input, the relay in front of the translation
//! `llama-server`, and `hayamimi-eval stream` playing the recording in.
//!
//! The stack is the command of design 10.9: `hayamimi` on 8833 (events) and
//! 8766 (ingest), the relay on 8835. The translation `llama-server` is not
//! started here; the one that runs on the machine is found by its `--port`.
//! Every process this module starts is stopped and waited for, whether the run
//! came out or not.

use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::{BINARY, Failure, collect, models_dir, path_text, spawn_binary};

const EVENTS_PORT: u16 = 8833;
const RELAY_PORT: u16 = 8835;
const INGEST_PORT: u16 = 8766;
/// How long the stack is given to open its ports: the models are read first.
const START_WAIT: Duration = Duration::from_secs(300);
/// How long `hayamimi` is given to end after an interrupt.
const STOP_WAIT: Duration = Duration::from_secs(60);

/// The `llama-server` of the machine: its pid and the port it listens on.
pub struct Llama {
    pub pid: u32,
    pub port: u16,
}

/// The port that follows `--port` in the words of a command line.
fn port_of(words: &[&str]) -> Option<u16> {
    let at = words.iter().position(|word| *word == "--port")?;
    words.get(at + 1)?.parse().ok()
}

/// The one `llama-server` running on this machine. None or several is an
/// error: the run's three `--own-pid` must name exactly one translation server.
pub fn find_llama() -> Result<Llama, Failure> {
    let entries =
        std::fs::read_dir("/proc").map_err(|error| format!("cannot read /proc: {error}"))?;
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        let directory = entry.path();
        let comm = std::fs::read_to_string(directory.join("comm")).ok();
        if comm.as_deref().map(str::trim) != Some("llama-server") {
            continue;
        }
        let bytes = std::fs::read(directory.join("cmdline"))
            .map_err(|error| format!("cannot read the command line of {pid}: {error}"))?;
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let words: Vec<&str> = text.split('\0').collect();
        let port = port_of(&words).ok_or_else(|| format!("llama-server {pid} has no --port"))?;
        found.push(Llama { pid, port });
    }
    match found.len() {
        1 => found.pop().ok_or_else(|| String::from("no llama-server")),
        0 => Err(String::from("no llama-server is running")),
        count => Err(format!("{count} llama-server processes are running")),
    }
}

/// The ports the stack opens must be free before it starts.
pub fn require_free_ports() -> Result<(), Failure> {
    for port in [EVENTS_PORT, RELAY_PORT, INGEST_PORT] {
        TcpListener::bind((Ipv4Addr::LOCALHOST, port))
            .map_err(|error| format!("port {port} is held by another process: {error}"))?;
    }
    Ok(())
}

/// A program of this workspace that `cargo test -p hayamimi` does not build:
/// it stands beside the test binary's own directory under `target/release/`.
pub fn sibling_binary(name: &str) -> Result<PathBuf, Failure> {
    let exe = std::env::current_exe().map_err(|error| format!("no current exe: {error}"))?;
    let release = exe
        .ancestors()
        .nth(2)
        .ok_or_else(|| format!("{} has no target directory", exe.display()))?;
    let path = release.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    if path.is_file() {
        Ok(path)
    } else {
        Err(format!(
            "{} is not built; run `cargo build --release -p hayamimi-relay -p hayamimi-eval`",
            path.display()
        ))
    }
}

/// A child process that is stopped and waited for when it goes out of scope.
struct Owned(Option<Child>);

impl Owned {
    fn child(&self) -> Result<&Child, Failure> {
        self.0
            .as_ref()
            .ok_or_else(|| String::from("the process is gone"))
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            // A process that already ended has nothing to kill; the wait reaps it either way.
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn wait_for_port(port: u16, within: Duration) -> Result<(), Failure> {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        if TcpStream::connect_timeout(&address, Duration::from_millis(200)).is_ok() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Err(format!("port {port} did not open within {within:?}"))
}

fn wait_for_exit(child: &mut Child, within: Duration) -> Result<(), Failure> {
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        if child
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_some()
        {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Err(format!("the process did not end within {within:?}"))
}

/// `hayamimi` in `mode` and its stdout, read on a thread.
fn start_hayamimi(mode: &str) -> Result<(Owned, JoinHandle<String>), Failure> {
    let models = path_text(&models_dir()?)?;
    let events = EVENTS_PORT.to_string();
    let args = [
        "--input",
        "ws",
        "--mode",
        mode,
        "--source",
        "file",
        "--models",
        models.as_str(),
        "--serve",
        events.as_str(),
    ];
    let mut child = spawn_binary(BINARY, &args, None, &[])?;
    let stdout = child.stdout.take().ok_or("no stdout pipe")?;
    let printed = collect(stdout);
    Ok((Owned(Some(child)), printed))
}

fn start_relay(llama: &Llama) -> Result<Owned, Failure> {
    let binary = path_text(&sibling_binary("hayamimi-relay")?)?;
    let events = format!("http://127.0.0.1:{EVENTS_PORT}/events");
    let llm = format!("http://127.0.0.1:{}", llama.port);
    let serve = RELAY_PORT.to_string();
    let args = [
        "live", "--events", &events, "--llm", &llm, "--serve", &serve,
    ];
    Ok(Owned(Some(spawn_binary(&binary, &args, None, &[])?)))
}

/// `hayamimi-eval stream` over `wav`, `repeat` times, into `out`.
fn stream(
    wav: &Path,
    repeat: u32,
    out: &Path,
    pids: [u32; 3],
    llama: &Llama,
) -> Result<(), Failure> {
    let binary = sibling_binary("hayamimi-eval")?;
    let mut command = Command::new(binary);
    command
        .args(["stream", "--wav", &path_text(wav)?])
        .args(["--repeat", &repeat.to_string()])
        .args(["--relay-port", &RELAY_PORT.to_string()])
        .args(["--port", &INGEST_PORT.to_string()])
        .args(["--out", &path_text(out)?]);
    for pid in pids {
        command.args(["--own-pid", &pid.to_string()]);
    }
    command.args(["--own-llama-port", &llama.port.to_string()]);
    let status = command
        .status()
        .map_err(|error| format!("cannot run hayamimi-eval stream: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("hayamimi-eval stream ended with {status}"))
    }
}

/// Plays `wav` `repeat` times through a stack whose `hayamimi` runs in `mode`
/// and leaves `out` (the samples), its `.events.jsonl` sidecar and `out` with
/// the extension `log` (what `hayamimi` printed).
pub fn run_stack(mode: &str, wav: &Path, repeat: u32, out: &Path) -> Result<(), Failure> {
    require_free_ports()?;
    let llama = find_llama()?;
    sibling_binary("hayamimi-relay")?;
    sibling_binary("hayamimi-eval")?;
    let (hayamimi, printed) = start_hayamimi(mode)?;
    wait_for_port(EVENTS_PORT, START_WAIT)?;
    wait_for_port(INGEST_PORT, START_WAIT)?;
    let relay = start_relay(&llama)?;
    wait_for_port(RELAY_PORT, START_WAIT)?;
    let pids = [hayamimi.child()?.id(), relay.child()?.id(), llama.pid];
    let played = stream(wav, repeat, out, pids, &llama);
    drop(relay);
    stop_hayamimi(hayamimi)?;
    let log = printed
        .join()
        .map_err(|_| String::from("the stdout reader panicked"))?;
    std::fs::write(out.with_extension("log"), log)
        .map_err(|error| format!("cannot write the log: {error}"))?;
    played
}

#[cfg(unix)]
fn ask_to_stop(child: &Child) -> Result<(), Failure> {
    super::interrupt(child)
}

#[cfg(not(unix))]
fn ask_to_stop(_child: &Child) -> Result<(), Failure> {
    Err(String::from(
        "this run reads /proc and needs a unix machine",
    ))
}

/// Ends `hayamimi` the way a user does with Ctrl-C and waits for it.
fn stop_hayamimi(mut hayamimi: Owned) -> Result<(), Failure> {
    ask_to_stop(hayamimi.child()?)?;
    let Some(child) = hayamimi.0.as_mut() else {
        return Err(String::from("the process is gone"));
    };
    wait_for_exit(child, STOP_WAIT)
}

//! Helpers shared by the acceptance tests of this package: running the
//! `hayamimi` binary and reading what it printed. Each test binary uses its
//! own subset.
#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::Value;

pub mod llama;

/// What a `check_*` function returns for a wrong input: a message that
/// names what is wrong.
pub type Failure = String;

/// The return type of every test.
pub type TestResult = Result<(), Box<dyn std::error::Error>>;

pub const REPO: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
pub const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/golden/ja");
pub const DESIGN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/design/rust_port.ja.md"
);
pub const RESULTS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/results/gpu_stages.md"
);
pub const BINARY: &str = env!("CARGO_BIN_EXE_hayamimi");

/// The environment that hides every GPU from the child. An empty
/// `CUDA_VISIBLE_DEVICES` hides them on Linux; the Windows driver ignores an
/// empty value and hides them for an index that does not exist.
pub const HIDE_GPU: (&str, &str) = if cfg!(windows) {
    ("CUDA_VISIBLE_DEVICES", "-1")
} else {
    ("CUDA_VISIBLE_DEVICES", "")
};

/// The clip the acceptance runs play.
pub fn clip() -> PathBuf {
    Path::new(GOLDEN).join("ja_014.wav")
}

/// Everything a finished run left behind.
#[derive(Debug, Clone)]
pub struct Output {
    /// `None` when a signal ended the process.
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

/// The directory that holds `models/` at or above the repository checkout:
/// model files are not in git, so a worktree reads them from the main checkout.
pub fn models_dir() -> Result<PathBuf, Failure> {
    let start = Path::new(REPO)
        .canonicalize()
        .map_err(|error| format!("cannot resolve {REPO}: {error}"))?;
    start
        .ancestors()
        .map(|directory| directory.join("models"))
        .find(|models| models.join("silero_vad.onnx").is_file())
        .ok_or_else(|| format!("no models/silero_vad.onnx at or above {}", start.display()))
}

/// The path at or above the repository checkout that ends in `relative`:
/// recordings and recorded data are not in git, so a worktree reads them from
/// the main checkout.
pub fn find_above(relative: &str) -> Result<PathBuf, Failure> {
    let start = Path::new(REPO)
        .canonicalize()
        .map_err(|error| format!("cannot resolve {REPO}: {error}"))?;
    start
        .ancestors()
        .map(|directory| directory.join(relative))
        .find(|path| path.exists())
        .ok_or_else(|| format!("no {relative} at or above {}", start.display()))
}

/// A path as text, or an error that names it.
pub fn path_text(path: &Path) -> Result<String, Failure> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("{} is not UTF-8", path.display()))
}

/// The binary with a clean locale environment, then `locale` and `extra`
/// set. The test names the values; the program reads them.
fn command(binary: &str, args: &[String], locale: Option<&str>, extra: &[(&str, &str)]) -> Command {
    let mut command = Command::new(binary);
    command
        .args(args)
        .env_remove("LC_ALL")
        .env_remove("LC_MESSAGES")
        .env_remove("LANGUAGE")
        .env_remove("LANG");
    if let Some(locale) = locale {
        command.env("LANG", locale);
    }
    for (key, value) in extra {
        command.env(key, value);
    }
    command
}

pub fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_owned()).collect()
}

/// Runs the binary to its end.
pub fn run(args: &[&str], locale: Option<&str>, extra: &[(&str, &str)]) -> Result<Output, Failure> {
    let output = command(BINARY, &strings(args), locale, extra)
        .output()
        .map_err(|error| format!("cannot run {BINARY}: {error}"))?;
    Ok(Output {
        code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// Starts the binary and leaves it running, its output piped.
pub fn spawn(
    args: &[&str],
    locale: Option<&str>,
    extra: &[(&str, &str)],
) -> Result<Child, Failure> {
    spawn_binary(BINARY, args, locale, extra)
}

/// Starts the program at `binary` and leaves it running, its output piped.
pub fn spawn_binary(
    binary: &str,
    args: &[&str],
    locale: Option<&str>,
    extra: &[(&str, &str)],
) -> Result<Child, Failure> {
    command(binary, &strings(args), locale, extra)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot start {binary}: {error}"))
}

/// The events on stdout, one per line. A line that is not a JSON object is an error.
pub fn events(stdout: &str) -> Result<Vec<Value>, Failure> {
    stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let value: Value = serde_json::from_str(line)
                .map_err(|error| format!("`{line}` is not JSON: {error}"))?;
            if value.is_object() {
                Ok(value)
            } else {
                Err(format!("`{line}` is not a JSON object"))
            }
        })
        .collect()
}

/// The events whose `event` field is `name`.
pub fn named<'a>(events: &'a [Value], name: &str) -> Vec<&'a Value> {
    events
        .iter()
        .filter(|event| event.get("event").and_then(Value::as_str) == Some(name))
        .collect()
}

/// A run with the subtitle server on: what the process printed, and what a
/// client connected to `/events` for the whole run received.
#[derive(Debug, Clone)]
pub struct Served {
    pub output: Output,
    pub wire: Vec<Value>,
}

/// A port that was free a moment ago.
pub fn free_port() -> Result<u16, Failure> {
    let listener =
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|error| error.to_string())?;
    listener
        .local_addr()
        .map(|address| address.port())
        .map_err(|error| error.to_string())
}

/// Reads a pipe to its end on a thread of its own.
pub fn collect(mut pipe: impl Read + Send + 'static) -> JoinHandle<String> {
    std::thread::spawn(move || {
        let mut text = String::new();
        // A pipe that breaks early only shortens the text, which the checks then judge.
        let _ = pipe.read_to_string(&mut text);
        text
    })
}

/// Whether a connection to a port nobody listens on came back to its own
/// socket: a loopback connect whose ephemeral local port equals the target
/// port completes against itself, and a read on it never ends.
fn connected_to_itself(stream: &TcpStream) -> bool {
    matches!((stream.local_addr(), stream.peer_addr()), (Ok(local), Ok(peer)) if local == peer)
}

/// Connects to `/events` on the first of `ports` that answers, as soon as the
/// server is up, and reads until it closes.
fn read_events(ports: &[u16]) -> String {
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut text = String::new();
    while Instant::now() < deadline {
        for port in ports {
            if let Ok(mut stream) = TcpStream::connect((Ipv4Addr::LOCALHOST, *port)) {
                if connected_to_itself(&stream) {
                    continue;
                }
                let request = b"GET /events HTTP/1.1\r\nHost: localhost\r\n\r\n";
                // A server that closes at once leaves whatever was read, which the checks judge.
                let _ = stream.write_all(request);
                let _ = stream.read_to_string(&mut text);
                return text;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    text
}

/// The JSON objects of the `data:` lines of an event stream.
pub fn data_events(stream: &str) -> Vec<Value> {
    stream
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|data| serde_json::from_str::<Value>(data).ok())
        .collect()
}

/// Runs the binary with `--serve` on a free port and reads `/events` throughout.
pub fn serve(
    args: &[&str],
    locale: Option<&str>,
    extra: &[(&str, &str)],
) -> Result<Served, Failure> {
    let port = free_port()?;
    serve_from(args, port, vec![port], locale, extra)
}

/// Runs the binary with `--serve <requested>` and reads `/events` from the
/// first of `client_ports` that answers.
pub fn serve_from(
    args: &[&str],
    requested: u16,
    client_ports: Vec<u16>,
    locale: Option<&str>,
    extra: &[(&str, &str)],
) -> Result<Served, Failure> {
    serve_binary(BINARY, args, (requested, client_ports), locale, extra)
}

/// Like [`serve_from`], for the program at `binary`; `ports` is the port asked
/// for and the ports the client tries.
pub fn serve_binary(
    binary: &str,
    args: &[&str],
    ports: (u16, Vec<u16>),
    locale: Option<&str>,
    extra: &[(&str, &str)],
) -> Result<Served, Failure> {
    serve_driven(binary, args, ports, (locale, extra), |_| Ok(()))
}

/// Ends a running child the way a user does with Ctrl-C, so that it can
/// finish and exit with its own code.
#[cfg(unix)]
pub fn interrupt(child: &Child) -> Result<(), Failure> {
    let status = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .map_err(|error| format!("cannot run kill: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("kill -INT {} failed", child.id()))
    }
}

/// Runs the program at `binary` with `--serve`, reads `/events` throughout,
/// and calls `drive` once the process is started: it may talk to the process
/// (play audio into it, end it with [`interrupt`]). The run is judged after
/// the process has exited.
pub fn serve_driven(
    binary: &str,
    args: &[&str],
    ports: (u16, Vec<u16>),
    environment: (Option<&str>, &[(&str, &str)]),
    drive: impl FnOnce(&mut Child) -> Result<(), Failure>,
) -> Result<Served, Failure> {
    let (requested, client_ports) = ports;
    let (locale, extra) = environment;
    let number = requested.to_string();
    let mut all: Vec<&str> = args.to_vec();
    all.extend(["--serve", number.as_str()]);
    let mut child = spawn_binary(binary, &all, locale, extra)?;
    let stdout = child.stdout.take().ok_or("no stdout pipe")?;
    let stderr = child.stderr.take().ok_or("no stderr pipe")?;
    let (out, err) = (collect(stdout), collect(stderr));
    let wire = std::thread::spawn(move || read_events(&client_ports));
    let driven = drive(&mut child);
    if driven.is_err() {
        // A drive that failed leaves the process running; end it so that the run can be read.
        let _ = child.kill();
    }
    let status = child.wait().map_err(|error| error.to_string())?;
    driven?;
    let join = |handle: JoinHandle<String>| {
        handle
            .join()
            .map_err(|_| String::from("a reader thread panicked"))
    };
    Ok(Served {
        output: Output {
            code: status.code(),
            stdout: join(out)?,
            stderr: join(err)?,
        },
        wire: data_events(&join(wire)?),
    })
}

pub fn read(path: &Path) -> Result<String, Failure> {
    std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))
}

#[cfg(unix)]
fn link(from: &Path, to: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(from, to)
}

#[cfg(windows)]
fn link(from: &Path, to: &Path) -> std::io::Result<()> {
    if from.is_dir() {
        std::os::windows::fs::symlink_dir(from, to)
    } else {
        std::os::windows::fs::symlink_file(from, to)
    }
}

static NEXT_DIRECTORY: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// A models directory that holds everything the real one does except the entry `without`.
pub fn models_without(without: &str) -> Result<PathBuf, Failure> {
    let real = models_dir()?;
    // Tests run on several threads of one process; each call gets a directory of its own.
    let number = NEXT_DIRECTORY.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let name = format!("models-without-{without}-{number}");
    let target = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    if target.exists() {
        std::fs::remove_dir_all(&target).map_err(|error| error.to_string())?;
    }
    std::fs::create_dir_all(&target).map_err(|error| error.to_string())?;
    let entries = std::fs::read_dir(&real).map_err(|error| error.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        if entry.file_name() == without {
            continue;
        }
        link(&entry.path(), &target.join(entry.file_name())).map_err(|error| error.to_string())?;
    }
    Ok(target)
}

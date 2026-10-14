//! The install the tests of `hayamimi llama start` run against, and the
//! stand-in server (`fake_llama_server`) it starts in the place of
//! `llama-server`.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use hayamimi_app::llama::layout::Layout;
use hayamimi_app::llama::plan::Build;
use serde_json::Value;

use super::{Failure, Output, events, free_port, named, run};

const FAKE_SERVER: &str = env!("CARGO_BIN_EXE_fake_llama_server");
const STARTED: &str = "translate.server.started";
/// Seconds the start waits for the stand-in to answer; it answers at once.
const WAIT_SECONDS: &str = "20";
/// Seconds a stopped server is given to close its port.
const CLOSE_SECONDS: u64 = 10;

static NEXT: AtomicU32 = AtomicU32::new(0);

fn io<T>(result: std::io::Result<T>, what: &str) -> Result<T, Failure> {
    result.map_err(|error| format!("{what}: {error}"))
}

fn text_of(path: &Path) -> Result<&str, Failure> {
    path.to_str()
        .ok_or_else(|| format!("{} is not UTF-8", path.display()))
}

/// A directory laid out as `hayamimi llama start` expects, removed on drop.
pub struct Install {
    root: PathBuf,
}

impl Install {
    /// An install whose `builds` are the stand-in server and whose `models`
    /// are empty files.
    pub fn new(name: &str, builds: &[Build], models: &[&str]) -> Result<Install, Failure> {
        let number = NEXT.fetch_add(1, Ordering::Relaxed);
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/unit-tmp");
        let root = base.join(format!("llama-{name}-{}-{number}", std::process::id()));
        let install = Install { root };
        let layout = Layout::new(install.root.clone());
        for build in builds {
            copy_server(&layout.server(*build))?;
        }
        for model in models {
            empty_file(&layout.model(model))?;
        }
        Ok(install)
    }

    /// Runs `hayamimi llama start` for `model` on a free port.
    pub fn start(
        &self,
        model: &str,
        locale: Option<&str>,
        extra: &[(&str, &str)],
    ) -> Result<Launch, Failure> {
        let port = free_port()?;
        let number = port.to_string();
        let root = text_of(&self.root)?;
        let args = [
            "llama",
            "start",
            "--model",
            model,
            "--port",
            &number,
            "--dir",
            root,
            "--wait",
            WAIT_SECONDS,
        ];
        let began = Instant::now();
        let output = run(&args, locale, extra)?;
        let took = began.elapsed();
        let guard = Guard {
            pid: started_pid(&output.stdout)?,
            port,
        };
        let server_args = first_line(&Layout::new(self.root.clone()).log());
        Ok(Launch {
            guard,
            output,
            port,
            took,
            server_args,
        })
    }
}

/// A start and the install it ran on. The server is stopped before the
/// install is removed.
pub struct Session {
    pub launch: Launch,
    _install: Install,
}

impl Session {
    /// A fresh install with the stand-in as each of `builds` and an empty file
    /// named `model`, and a start of that model.
    pub fn start(
        name: &str,
        builds: &[Build],
        model: &str,
        locale: Option<&str>,
        extra: &[(&str, &str)],
    ) -> Result<Session, Failure> {
        let install = Install::new(name, builds, &[model])?;
        let launch = install.start(model, locale, extra)?;
        Ok(Session {
            launch,
            _install: install,
        })
    }
}

impl Drop for Install {
    fn drop(&mut self) {
        // A directory that cannot be removed (a server still holds a file of it)
        // stays in the scratch directory, which holds nothing of value.
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn make_parent(target: &Path) -> Result<(), Failure> {
    if let Some(directory) = target.parent() {
        io(
            std::fs::create_dir_all(directory),
            "cannot create a directory",
        )?;
    }
    Ok(())
}

fn copy_server(target: &Path) -> Result<(), Failure> {
    make_parent(target)?;
    io(
        std::fs::copy(FAKE_SERVER, target),
        "cannot copy the stand-in server",
    )?;
    Ok(())
}

fn empty_file(target: &Path) -> Result<(), Failure> {
    make_parent(target)?;
    io(std::fs::File::create(target), "cannot create a model file")?;
    Ok(())
}

/// What one `hayamimi llama start` left: its output, the arguments the
/// server it started was given, and the server itself.
pub struct Launch {
    /// Declared first, so that the server is stopped before anything else goes.
    guard: Guard,
    pub output: Output,
    pub port: u16,
    /// How long the start command ran.
    pub took: Duration,
    /// The first line the stand-in printed into the log: the arguments it got.
    pub server_args: Option<String>,
}

impl Launch {
    /// The process id of the server, when the start left one.
    pub fn pid(&self) -> Option<u32> {
        self.guard.pid
    }

    /// Stops the server and waits for its port to close.
    pub fn stop(&mut self) -> Result<(), Failure> {
        self.guard.stop()
    }
}

/// The server a start left, stopped when this is dropped.
struct Guard {
    pid: Option<u32>,
    port: u16,
}

impl Guard {
    fn stop(&mut self) -> Result<(), Failure> {
        let Some(pid) = self.pid.take() else {
            return Ok(());
        };
        io(kill(pid), "cannot stop the server")?;
        let deadline = Instant::now() + Duration::from_secs(CLOSE_SECONDS);
        while health(self.port).is_ok() {
            if Instant::now() > deadline {
                return Err(format!("the server on port {} did not stop", self.port));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Ok(())
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        // A server that cannot be stopped is a leak; nothing more can be done
        // from a destructor, and a test that stops it by hand sees the error.
        let _ = self.stop();
    }
}

/// The `pid` of the `translate.server.started` event, when there is one.
fn started_pid(stdout: &str) -> Result<Option<u32>, Failure> {
    let all = events(stdout)?;
    Ok(named(&all, STARTED)
        .first()
        .and_then(|event| event.get("pid"))
        .and_then(Value::as_u64)
        .and_then(|pid| u32::try_from(pid).ok()))
}

fn first_line(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    text.lines().next().map(str::to_owned)
}

/// The `status` of `GET /health` on the local port, or why there is none.
pub fn health(port: u16) -> Result<String, Failure> {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(5))
        .map_err(|error| format!("cannot connect to port {port}: {error}"))?;
    io(
        stream.set_read_timeout(Some(Duration::from_secs(5))),
        "cannot set a timeout",
    )?;
    let request = "GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    io(
        stream.write_all(request.as_bytes()),
        "cannot send the request",
    )?;
    let mut answer = String::new();
    io(stream.read_to_string(&mut answer), "cannot read the answer")?;
    let (_, body) = answer
        .split_once("\r\n\r\n")
        .ok_or("the answer has no body")?;
    let json: Value = serde_json::from_str(body).map_err(|error| error.to_string())?;
    json.get("status")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| String::from("the answer has no status"))
}

#[cfg(unix)]
fn kill(pid: u32) -> std::io::Result<()> {
    Command::new("kill").arg(pid.to_string()).status()?;
    Ok(())
}

#[cfg(windows)]
fn kill(pid: u32) -> std::io::Result<()> {
    Command::new("taskkill")
        .args(["/F", "/PID", &pid.to_string()])
        .output()?;
    Ok(())
}

/// Whether the process `pid` is running.
#[cfg(unix)]
pub fn alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .is_ok_and(|status| status.success())
}

/// Whether the process `pid` is running.
#[cfg(windows)]
pub fn alive(pid: u32) -> bool {
    Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
        .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).contains(&pid.to_string()))
}

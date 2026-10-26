//! Runs `lat-dialog` and `lat-short` through the real stack for IP-2 and IP-7:
//! `hayamimi` on a websocket input, the relay in front of the translation
//! `llama-server`, and `hayamimi-eval stream` playing the recording in.
//!
//! The stack is the command of design 10.9: `hayamimi` on 8833 (events) and
//! 8766 (ingest), the relay on 8835. The translation `llama-server` is not
//! started here and is never stopped; the run uses the one started with
//! `--port 18084` (the window's unit), and a server on another port is left
//! alone. Every process this module starts is stopped and waited for, whether
//! the run came out or not.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use hayamimi_eval::machine::llama_server_pids;
use hayamimi_eval::manifest::{Row, read_rows, write_rows};
use hayamimi_eval::wav::{SAMPLE_RATE, read_wav, write_wav};
use serde::Deserialize;
use serde_json::{Value, json};

use super::dialog::PageStamp;
use super::{BINARY, Failure, collect, find_above, models_dir, path_text, spawn_binary};

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

/// The port of the translation server the window's unit starts.
pub const LLAMA_PORT: u16 = 18084;
/// The user unit that starts that server; the run needs it up before it starts.
const LLAMA_UNIT: &str = "hayamimi-llama-e2e.service";

/// The `llama-server` of the run: the one process under `proc_root` started
/// with `--port <port>`, read the way `stream --own-pid` reads it. A server on
/// another port does not count. None or several on `port` is an error: the
/// run's three `--own-pid` must name exactly one translation server.
pub fn find_llama(proc_root: &Path, port: u16) -> Result<Llama, Failure> {
    let pids = llama_server_pids(proc_root, port).map_err(|error| error.to_string())?;
    match pids.as_slice() {
        [pid] => Ok(Llama { pid: *pid, port }),
        [] => Err(format!(
            "no llama-server is running with --port {port}; start {LLAMA_UNIT}"
        )),
        _ => Err(format!(
            "{} llama-server processes run with --port {port}: pids {}",
            pids.len(),
            pids.iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        )),
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
    let llama = find_llama(Path::new("/proc"), LLAMA_PORT)?;
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

// ---- a run with the dashboard open (BR-2 and BR-3) --------------------------
//
// The stack is the one of `run_stack`, on ports the system chooses, with a
// browser that has the dashboard open and a script in the page that times
// itself. The browser is driven over the DevTools protocol and not asked for a
// dump of the page as it starts (`--dump-dom`): that dump is taken when the
// page has loaded, before the app has drawn anything.

const FAKE_SERVER: &str = env!("CARGO_BIN_EXE_fake_llama_server");
/// How long a browser is given to start and to draw the page.
const BROWSER_WAIT: Duration = Duration::from_secs(60);
/// How long a page that is drawn is given to connect its streams, before the
/// first line is played.
const CONNECT_GRACE: Duration = Duration::from_secs(3);
/// How long the pages are given after the stream has ended to draw the last translations.
const DRAW_WAIT: Duration = Duration::from_secs(180);
/// How long a page is given after its last translation, for a page that should show none.
const SHOW_GRACE: Duration = Duration::from_secs(2);

/// The translation server of a paged run.
pub enum Translator {
    /// `fake_llama_server -m translates.gguf`, started and stopped by the run.
    Fake,
    /// The `llama-server` already running with `--port <port>`; the run neither starts nor stops it.
    Real(u16),
}

/// What is played into the stack.
pub struct Playback<'a> {
    pub wav: &'a Path,
    pub manifest: Option<&'a Path>,
    pub repeat: u32,
}

/// What the measuring script of one page reported.
#[derive(Clone, Debug, PartialEq)]
pub struct PageRecord {
    /// The translations the page recorded, in the order of the lines.
    pub stamps: Vec<PageStamp>,
    /// How many `row` elements the page holds.
    pub rows: usize,
    /// How many `tr` elements inside a `row` show a translation (hold a
    /// `tr-text`), counted by the page when it was read.
    pub trs_in_rows: usize,
    /// How many `tr` elements inside a `row` show a mark and no text: a
    /// translation the relay withheld (a digit, a name, a negation in the
    /// source) or could not make.
    pub marks_in_rows: usize,
}

/// What a paged run left: the samples file (its `.events.jsonl` beside it) and
/// the report of each page.
#[derive(Debug)]
pub struct PagedRun {
    pub played: PathBuf,
    pub with_relay: PageRecord,
    /// The page opened without `?relay=`, when the run was asked for it.
    pub without_relay: Option<PageRecord>,
}

/// What the script in the page sends back.
#[derive(Deserialize)]
struct Report {
    finals: Vec<String>,
    arrive: BTreeMap<String, f64>,
    draw: Vec<Option<f64>>,
    rows: usize,
    trs_in_rows: usize,
    marks_in_rows: usize,
}

/// Reads the text the script's `__br_report` returns. A line the page received
/// but has neither an arrival nor a draw for is not a translation and has no stamp.
pub fn page_record_of(report: &str) -> Result<PageRecord, Failure> {
    let read: Report =
        serde_json::from_str(report).map_err(|error| format!("the page report: {error}"))?;
    let stamps = read
        .finals
        .iter()
        .enumerate()
        .filter_map(|(at, id)| {
            let p_arrive = read.arrive.get(id).copied();
            let p_draw = read.draw.get(at).copied().flatten();
            (p_arrive.is_some() || p_draw.is_some()).then(|| PageStamp {
                id: id.clone(),
                p_arrive,
                p_draw,
                tool_us: None,
            })
        })
        .collect();
    Ok(PageRecord {
        stamps,
        rows: read.rows,
        trs_in_rows: read.trs_in_rows,
        marks_in_rows: read.marks_in_rows,
    })
}

/// The first measuring script, put into the page before the page's own code
/// runs. It wraps `EventSource` so that the streams the page itself opens are
/// listened to first: the `final`s of `hayamimi` give the order of the lines,
/// and the translations of the relay give `p_arrive`, the first shown
/// translation of a line, on the connection the page draws from. A connection
/// of the script's own would be told of a translation a little after the page
/// is, and `p_draw` could then come before `p_arrive`. It is put into the page
/// by the tests and is no part of the product.
const START_SCRIPT: &str = r#"(() => {
  const state = { finals: [], arrive: {}, draw: [] };
  window.__br = state;
  const Native = window.EventSource;
  window.EventSource = class extends Native {
    constructor(url, init) {
      super(url, init);
      const from = this.url;
      this.addEventListener('message', (message) => {
        let event;
        try { event = JSON.parse(message.data); } catch (error) { return; }
        if (from.includes(':@EVENTS@/') && event.type === 'final') state.finals.push(event.id);
        const shown = from.includes(':@RELAY@/') && event.type === 'translation'
          && event.of === 'final' && typeof event.text === 'string' && event.text.length > 0
          && (event.status === undefined || event.status === 'ok');
        if (shown && state.arrive[event.id] === undefined) {
          state.arrive[event.id] = performance.now();
        }
      });
    }
  };
})()"#;

/// The second measuring script, put into the page once the dashboard is
/// drawn. `p_draw` is the first time a `row` holds a `tr` that shows text: a
/// `tr` with only a mark (a withheld translation) is no draw. A row is told
/// from the others by the order it first appears in.
const SWEEP_SCRIPT: &str = r#"(() => {
  const state = window.__br;
  const ordinals = new WeakMap();
  let next = 0;
  const sweep = () => {
    const now = performance.now();
    for (const row of document.querySelectorAll('.row')) {
      if (!ordinals.has(row)) ordinals.set(row, next++);
      const at = ordinals.get(row);
      if (state.draw[at] === undefined && row.querySelector('.tr .tr-text')) state.draw[at] = now;
    }
  };
  new MutationObserver(sweep).observe(document.body, { childList: true, subtree: true });
  window.__br_done = () => {
    const rows = Array.from(document.querySelectorAll('.row'));
    return rows.length > 0 && rows.length === state.finals.length
      && rows.every((row) => row.querySelector('.tr'));
  };
  window.__br_report = () => JSON.stringify({
    finals: state.finals,
    arrive: state.arrive,
    draw: Array.from(state.draw, (time) => (time === undefined ? null : time)),
    rows: document.querySelectorAll('.row').length,
    trs_in_rows: document.querySelectorAll('.row .tr .tr-text').length,
    marks_in_rows: Array.from(document.querySelectorAll('.row .tr'))
      .filter((tr) => !tr.querySelector('.tr-text')).length,
  });
  return true;
})()"#;

/// The first script with the ports of the two streams the page opens.
fn start_script_for(events: u16, relay: u16) -> String {
    START_SCRIPT
        .replace("@EVENTS@", &events.to_string())
        .replace("@RELAY@", &relay.to_string())
}

impl Owned {
    /// Starts `binary` with its output in the file `log`.
    fn logged(binary: &str, args: &[&str], log: &Path) -> Result<Owned, Failure> {
        let out = File::create(log).map_err(|error| format!("{}: {error}", log.display()))?;
        let err = out
            .try_clone()
            .map_err(|error| format!("{}: {error}", log.display()))?;
        Command::new(binary)
            .args(args)
            .stdin(Stdio::null())
            .stdout(out)
            .stderr(err)
            .spawn()
            .map(|child| Owned(Some(child)))
            .map_err(|error| format!("cannot start {binary}: {error}"))
    }
}

/// The ports of a paged run, which the system chooses and which differ.
struct Ports {
    events: u16,
    ingest: u16,
    llm: u16,
    relay: u16,
    /// The debugging port of each browser.
    browsers: [u16; 2],
}

fn free_ports(translator: &Translator) -> Result<Ports, Failure> {
    // All are held at once, so that no two are the same port.
    let held = (0..6)
        .map(|_| TcpListener::bind((Ipv4Addr::LOCALHOST, 0)))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let numbers = held
        .iter()
        .map(|listener| listener.local_addr().map(|address| address.port()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let [events, ingest, llm, relay, first, second] = numbers[..] else {
        return Err(String::from("six ports were not made"));
    };
    let llm = match translator {
        Translator::Fake => llm,
        Translator::Real(port) => *port,
    };
    Ok(Ports {
        events,
        ingest,
        llm,
        relay,
        browsers: [first, second],
    })
}

/// A server that is not started by the run must be the one on its port and be ready.
fn check_translator(translator: &Translator) -> Result<(), Failure> {
    let Translator::Real(port) = translator else {
        return Ok(());
    };
    find_llama(Path::new("/proc"), *port)?;
    match super::llama::health(*port)?.as_str() {
        "ok" => Ok(()),
        other => Err(format!(
            "the llama-server on port {port} says `{other}`, not `ok`"
        )),
    }
}

/// The programs and files of a paged run, found before anything is started.
struct Tools {
    relay: String,
    eval: PathBuf,
    shell: PathBuf,
    models: String,
}

/// The newest `chrome-headless-shell` in the cache of the browser installer.
fn headless_shell() -> Result<PathBuf, Failure> {
    let cache = dirs::home_dir()
        .ok_or("no home directory")?
        .join(".cache/ms-playwright");
    let entries =
        std::fs::read_dir(&cache).map_err(|error| format!("{}: {error}", cache.display()))?;
    let mut shells: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("chromium_headless_shell-")
        })
        .map(|entry| {
            entry
                .path()
                .join("chrome-headless-shell-linux64/chrome-headless-shell")
        })
        .filter(|path| path.is_file())
        .collect();
    shells.sort();
    shells
        .pop()
        .ok_or_else(|| format!("no chrome-headless-shell under {}", cache.display()))
}

/// The page bundle the `hayamimi` binary serves stands beside the binary.
fn check_bundle() -> Result<(), Failure> {
    let bundle = Path::new(BINARY)
        .parent()
        .ok_or("the binary has no directory")?
        .join("ui/hayamimi-ui.js");
    if bundle.is_file() {
        Ok(())
    } else {
        Err(format!(
            "{} is missing; build it: `cargo build --release --target wasm32-unknown-unknown -p hayamimi-ui --features web` and `wasm-bindgen --target web --no-typescript --out-dir target/release/ui --out-name hayamimi-ui target/wasm32-unknown-unknown/release/hayamimi-ui.wasm`",
            bundle.display()
        ))
    }
}

impl Tools {
    fn find() -> Result<Tools, Failure> {
        check_bundle()?;
        Ok(Tools {
            relay: path_text(&sibling_binary("hayamimi-relay")?)?,
            eval: sibling_binary("hayamimi-eval")?,
            shell: headless_shell()?,
            models: path_text(&models_dir()?)?,
        })
    }
}

/// The stand-in translation server, when the run is to start one.
fn start_fake(
    translator: &Translator,
    ports: &Ports,
    work: &Path,
) -> Result<Option<Owned>, Failure> {
    if !matches!(translator, Translator::Fake) {
        return Ok(None);
    }
    let port = ports.llm.to_string();
    let fake = Owned::logged(
        FAKE_SERVER,
        &["-m", "translates.gguf", "--port", &port],
        &work.join("fake.log"),
    )?;
    wait_for_port(ports.llm, START_WAIT)?;
    Ok(Some(fake))
}

fn start_paged_hayamimi(tools: &Tools, ports: &Ports, work: &Path) -> Result<Owned, Failure> {
    let (events, ingest) = (ports.events.to_string(), ports.ingest.to_string());
    let args = [
        "--input",
        "ws",
        "--ws-port",
        ingest.as_str(),
        "--mode",
        "pair",
        "--source",
        "file",
        "--models",
        tools.models.as_str(),
        "--serve",
        events.as_str(),
    ];
    let hayamimi = Owned::logged(BINARY, &args, &work.join("hayamimi.log"))?;
    wait_for_port(ports.events, START_WAIT)?;
    wait_for_port(ports.ingest, START_WAIT)?;
    Ok(hayamimi)
}

fn start_paged_relay(tools: &Tools, ports: &Ports, work: &Path) -> Result<Owned, Failure> {
    let source = format!("http://127.0.0.1:{}/events", ports.events);
    let translator = format!("http://127.0.0.1:{}", ports.llm);
    let serve = ports.relay.to_string();
    let args = [
        "live",
        "--events",
        &source,
        "--llm",
        &translator,
        "--serve",
        &serve,
    ];
    let relay = Owned::logged(&tools.relay, &args, &work.join("relay.log"))?;
    wait_for_port(ports.relay, START_WAIT)?;
    Ok(relay)
}

/// `hayamimi`, the translation server and the relay in front of it, stopped in
/// that order when this goes out of scope.
struct PagedStack {
    _fake: Option<Owned>,
    _hayamimi: Owned,
    _relay: Owned,
}

impl PagedStack {
    fn start(
        translator: &Translator,
        tools: &Tools,
        ports: &Ports,
        work: &Path,
    ) -> Result<PagedStack, Failure> {
        Ok(PagedStack {
            _fake: start_fake(translator, ports, work)?,
            _hayamimi: start_paged_hayamimi(tools, ports, work)?,
            _relay: start_paged_relay(tools, ports, work)?,
        })
    }
}

/// The key of the websocket handshake; the browser does not look at its value.
const WS_KEY: &str = "dGhlIHNhbXBsZSBub25jZQ==";
/// The mask of the frames this client sends; any four bytes will do.
const WS_MASK: [u8; 4] = [0x12, 0x34, 0x56, 0x78];
/// The longest frame this client reads.
const WS_LONGEST: u64 = 64 * 1024 * 1024;

/// One text frame of a websocket, as a client sends it: masked.
fn text_frame(payload: &[u8]) -> Result<Vec<u8>, Failure> {
    let length = u64::try_from(payload.len()).map_err(|error| error.to_string())?;
    let bytes = length.to_be_bytes();
    let mut frame = vec![0x81];
    if length < 126 {
        frame.push(0x80 | bytes[7]);
    } else if length < 65_536 {
        frame.extend([0x80 | 126, bytes[6], bytes[7]]);
    } else {
        frame.push(0x80 | 127);
        frame.extend(bytes);
    }
    frame.extend(WS_MASK);
    frame.extend(
        payload
            .iter()
            .zip(WS_MASK.iter().cycle())
            .map(|(byte, mask)| byte ^ mask),
    );
    Ok(frame)
}

/// The length a frame declares, read after its two-byte head.
fn frame_length(reader: &mut impl Read, small: u8) -> Result<u64, Failure> {
    let socket = |error: std::io::Error| format!("websocket: {error}");
    match small {
        126 => {
            let mut size = [0_u8; 2];
            reader.read_exact(&mut size).map_err(socket)?;
            Ok(u64::from(u16::from_be_bytes(size)))
        }
        127 => {
            let mut size = [0_u8; 8];
            reader.read_exact(&mut size).map_err(socket)?;
            Ok(u64::from_be_bytes(size))
        }
        short => Ok(u64::from(short)),
    }
}

/// One frame from the server: whether it ends a message, its opcode and its payload.
fn read_frame(reader: &mut impl Read) -> Result<(bool, u8, Vec<u8>), Failure> {
    let mut head = [0_u8; 2];
    reader
        .read_exact(&mut head)
        .map_err(|error| format!("websocket: {error}"))?;
    let length = frame_length(reader, head[1] & 0x7f)?;
    if length > WS_LONGEST {
        return Err(format!("websocket: a frame of {length} bytes"));
    }
    let size = usize::try_from(length).map_err(|error| error.to_string())?;
    let mut payload = vec![0_u8; size];
    reader
        .read_exact(&mut payload)
        .map_err(|error| format!("websocket: {error}"))?;
    Ok((head[0] & 0x80 != 0, head[0] & 0x0f, payload))
}

/// The head of an HTTP message, up to and including the blank line, read a
/// byte at a time so that nothing of the body is taken.
fn read_head(stream: &mut TcpStream) -> Result<String, Failure> {
    let mut head = Vec::new();
    let mut byte = [0_u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        stream
            .read_exact(&mut byte)
            .map_err(|error| error.to_string())?;
        head.push(byte[0]);
    }
    String::from_utf8(head).map_err(|error| error.to_string())
}

/// The `Content-Length` a head declares.
fn content_length(head: &str) -> Option<usize> {
    head.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse::<usize>().ok())?
    })
}

/// The body of `GET path` from the HTTP server at `port`.
fn http_body(port: u16, path: &str) -> Result<String, Failure> {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port))
        .map_err(|error| format!("cannot connect to port {port}: {error}"))?;
    let request =
        format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(BROWSER_WAIT))
        .map_err(|error| error.to_string())?;
    let head = read_head(&mut stream).map_err(|error| format!("{path}: {error}"))?;
    let length =
        content_length(&head).ok_or_else(|| format!("{path}: the answer has no Content-Length"))?;
    let mut body = vec![0_u8; length];
    stream
        .read_exact(&mut body)
        .map_err(|error| format!("{path}: {error}"))?;
    String::from_utf8(body).map_err(|error| error.to_string())
}

/// The websocket path of the first page of the browser at `port`.
fn page_socket_path(port: u16) -> Result<String, Failure> {
    let targets: Value = serde_json::from_str(&http_body(port, "/json/list")?)
        .map_err(|error| format!("/json/list: {error}"))?;
    let url = targets
        .as_array()
        .into_iter()
        .flatten()
        .find(|target| target.get("type").and_then(Value::as_str) == Some("page"))
        .and_then(|target| target.get("webSocketDebuggerUrl"))
        .and_then(Value::as_str)
        .ok_or("the browser lists no page")?;
    let at = url
        .find("/devtools/")
        .ok_or("the page socket has no path")?;
    Ok(url[at..].to_owned())
}

/// A connection to one page of a browser.
struct DevTools {
    stream: TcpStream,
    next_id: u64,
}

impl DevTools {
    fn connect(port: u16, path: &str) -> Result<DevTools, Failure> {
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port))
            .map_err(|error| format!("cannot connect to port {port}: {error}"))?;
        let request = format!(
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {WS_KEY}\r\nSec-WebSocket-Version: 13\r\n\r\n"
        );
        stream
            .write_all(request.as_bytes())
            .map_err(|error| error.to_string())?;
        stream
            .set_read_timeout(Some(BROWSER_WAIT))
            .map_err(|error| error.to_string())?;
        let head =
            read_head(&mut stream).map_err(|error| format!("websocket handshake: {error}"))?;
        if !head.starts_with("HTTP/1.1 101") {
            return Err(format!("websocket handshake: {head}"));
        }
        Ok(DevTools { stream, next_id: 1 })
    }

    /// The next message of the browser, its fragments joined.
    fn read_message(&mut self) -> Result<String, Failure> {
        let mut message = Vec::new();
        loop {
            let (last, opcode, payload) = read_frame(&mut self.stream)?;
            match opcode {
                0x0 | 0x1 => message.extend(payload),
                0x8 => return Err(String::from("the browser closed the websocket")),
                _ => continue,
            }
            if last {
                return String::from_utf8(message).map_err(|error| error.to_string());
            }
        }
    }

    /// Calls `method` and returns its `result`.
    fn call(&mut self, method: &str, params: Value) -> Result<Value, Failure> {
        let id = self.next_id;
        self.next_id += 1;
        let request = json!({"id": id, "method": method, "params": params});
        self.stream
            .write_all(&text_frame(request.to_string().as_bytes())?)
            .map_err(|error| error.to_string())?;
        loop {
            let answer: Value = serde_json::from_str(&self.read_message()?)
                .map_err(|error| format!("{method}: {error}"))?;
            if answer.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = answer.get("error") {
                return Err(format!("{method}: {error}"));
            }
            return answer
                .get("result")
                .cloned()
                .ok_or_else(|| format!("{method}: the answer has no result"));
        }
    }

    /// The value of `expression` in the page. An expression that throws, or
    /// that has no value, is an error.
    fn evaluate(&mut self, expression: &str) -> Result<Value, Failure> {
        let result = self.call(
            "Runtime.evaluate",
            json!({"expression": expression, "returnByValue": true}),
        )?;
        if let Some(thrown) = result.get("exceptionDetails") {
            return Err(format!("the page threw: {thrown}"));
        }
        result
            .get("result")
            .and_then(|value| value.get("value"))
            .cloned()
            .ok_or_else(|| format!("the page gave no value: {result}"))
    }
}

/// A browser with one page open, which is closed when this goes out of scope.
struct Page {
    _browser: Owned,
    devtools: DevTools,
}

impl Page {
    /// Opens a blank page, has `start` run before the code of every page it
    /// loads, and then loads `url`.
    fn open(
        shell: &Path,
        url: &str,
        start: &str,
        port: u16,
        work: &Path,
        name: &str,
    ) -> Result<Page, Failure> {
        let debugging = format!("--remote-debugging-port={port}");
        let profile = format!(
            "--user-data-dir={}",
            path_text(&work.join(format!("{name}-profile")))?
        );
        let browser = Owned::logged(
            &path_text(shell)?,
            &["--disable-gpu", &debugging, &profile, "about:blank"],
            &work.join(format!("{name}.log")),
        )?;
        wait_for_port(port, BROWSER_WAIT)?;
        let path = page_socket_path(port)?;
        let mut devtools = DevTools::connect(port, &path)?;
        devtools.call("Page.enable", json!({}))?;
        devtools.call(
            "Page.addScriptToEvaluateOnNewDocument",
            json!({"source": start}),
        )?;
        devtools.call("Page.navigate", json!({"url": url}))?;
        Ok(Page {
            _browser: browser,
            devtools,
        })
    }

    /// Whether `expression` is true in the page, asked until `within` passes.
    fn holds_within(&mut self, within: Duration, expression: &str) -> Result<bool, Failure> {
        let deadline = Instant::now() + within;
        loop {
            if self.devtools.evaluate(expression)? == Value::Bool(true) {
                return Ok(true);
            }
            if Instant::now() >= deadline {
                return Ok(false);
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// Waits for the dashboard to be drawn, then puts the second measuring script into it.
    fn install(&mut self) -> Result<(), Failure> {
        if !self.holds_within(BROWSER_WAIT, "!!document.querySelector('.dash')")? {
            return Err(format!("a page was not drawn within {BROWSER_WAIT:?}"));
        }
        self.devtools.evaluate(SWEEP_SCRIPT).map(|_| ())
    }

    /// What the script saw, once every line has a translation drawn or `within` has passed.
    fn record(&mut self, within: Duration) -> Result<PageRecord, Failure> {
        // What the page shows by then is judged, whether or not it is all there.
        self.holds_within(within, "window.__br_done()")?;
        let report = self.devtools.evaluate("window.__br_report()")?;
        let text = report.as_str().ok_or("the page report is not text")?;
        page_record_of(text)
    }
}

/// The dashboard with the relay in its address, and, when asked, one without.
struct Pages {
    with_relay: Page,
    without_relay: Option<Page>,
}

impl Pages {
    fn open(tools: &Tools, ports: &Ports, work: &Path, control: bool) -> Result<Pages, Failure> {
        let page = format!("http://127.0.0.1:{}/dashboard", ports.events);
        let relay = format!("?relay=http://127.0.0.1:{}/events", ports.relay);
        let [first, second] = ports.browsers;
        let start = start_script_for(ports.events, ports.relay);
        let with_relay = Page::open(
            &tools.shell,
            &format!("{page}{relay}"),
            &start,
            first,
            work,
            "with-relay",
        )?;
        let without_relay = if control {
            Some(Page::open(
                &tools.shell,
                &page,
                &start,
                second,
                work,
                "without-relay",
            )?)
        } else {
            None
        };
        Ok(Pages {
            with_relay,
            without_relay,
        })
    }

    /// Puts the second script into every page, then lets their streams connect.
    fn install(&mut self) -> Result<(), Failure> {
        self.with_relay.install()?;
        if let Some(page) = self.without_relay.as_mut() {
            page.install()?;
        }
        std::thread::sleep(CONNECT_GRACE);
        Ok(())
    }

    /// The report of each page once the page with the relay has drawn everything.
    fn records(mut self) -> Result<(PageRecord, Option<PageRecord>), Failure> {
        let with_relay = self.with_relay.record(DRAW_WAIT)?;
        std::thread::sleep(SHOW_GRACE);
        let without_relay = match self.without_relay.as_mut() {
            Some(page) => Some(page.record(Duration::ZERO)?),
            None => None,
        };
        Ok((with_relay, without_relay))
    }
}

/// How long a run waits for the machine to be quiet enough for `stream` to
/// start: it refuses a run while the load is over its lines, and the models
/// the run has just read keep the load up for a while.
const QUIET_WAIT: Duration = Duration::from_secs(420);

/// A load average of `/proc/loadavg` in hundredths, rounded down.
fn hundredths(field: Option<&str>) -> Result<u32, Failure> {
    let text = field.ok_or("/proc/loadavg has too few fields")?;
    let value: f64 = text
        .parse()
        .map_err(|error| format!("/proc/loadavg `{text}`: {error}"))?;
    Ok((value * 100.0).floor() as u32)
}

/// The first and the third load average, in hundredths.
fn loads() -> Result<(u32, u32), Failure> {
    let text = std::fs::read_to_string("/proc/loadavg")
        .map_err(|error| format!("cannot read /proc/loadavg: {error}"))?;
    let mut fields = text.split_whitespace();
    let one = hundredths(fields.next())?;
    fields.next();
    Ok((one, hundredths(fields.next())?))
}

/// Whether a `cargo` or `rustc` process runs: `stream` refuses a run while one does.
fn build_stands() -> Result<bool, Failure> {
    let entries =
        std::fs::read_dir("/proc").map_err(|error| format!("cannot read /proc: {error}"))?;
    Ok(entries.flatten().any(|entry| {
        std::fs::read_to_string(entry.path().join("comm"))
            .is_ok_and(|name| matches!(name.trim(), "cargo" | "rustc"))
    }))
}

/// Waits until the loads are under the lines `stream` refuses a run at and no
/// build stands.
fn wait_for_quiet_machine() -> Result<(), Failure> {
    let deadline = Instant::now() + QUIET_WAIT;
    loop {
        let (one, fifteen) = loads()?;
        if one < hayamimi_eval::machine::LOAD1_LIMIT
            && fifteen < hayamimi_eval::machine::LOAD15_LIMIT
            && !build_stands()?
        {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "the machine stayed busy for {QUIET_WAIT:?}: load1 {one}, load15 {fifteen} (hundredths)"
            ));
        }
        std::thread::sleep(Duration::from_secs(5));
    }
}

/// Plays into `/ingest` with `hayamimi-eval stream`; returns the samples file
/// of the run, which has its events file beside it.
fn play_into(
    tools: &Tools,
    playback: &Playback<'_>,
    ports: &Ports,
    work: &Path,
) -> Result<PathBuf, Failure> {
    let out = work.join("played.jsonl");
    let mut command = Command::new(&tools.eval);
    command
        .args(["stream", "--wav", &path_text(playback.wav)?])
        .args(["--repeat", &playback.repeat.to_string()])
        .args(["--port", &ports.ingest.to_string()])
        .args(["--relay-port", &ports.relay.to_string()])
        .args(["--out", &path_text(&out)?]);
    if let Some(manifest) = playback.manifest {
        command.args(["--manifest", &path_text(manifest)?]);
    }
    let status = command
        .status()
        .map_err(|error| format!("cannot run hayamimi-eval stream: {error}"))?;
    if status.success() {
        Ok(out)
    } else {
        Err(format!("hayamimi-eval stream ended with {status}"))
    }
}

/// Plays `playback` through a stack with the dashboard open and timed by the
/// script in the page. With `control`, a second page without `?relay=` is
/// open beside it. Every process the run starts is stopped when it ends,
/// whether it came out or not; a `Translator::Real` server is left alone.
pub fn run_paged(
    translator: &Translator,
    playback: &Playback<'_>,
    work: &Path,
    control: bool,
) -> Result<PagedRun, Failure> {
    check_translator(translator)?;
    let tools = Tools::find()?;
    let ports = free_ports(translator)?;
    let _stack = PagedStack::start(translator, &tools, &ports, work)?;
    let mut pages = Pages::open(&tools, &ports, work, control)?;
    pages.install()?;
    wait_for_quiet_machine()?;
    let played = play_into(&tools, playback, &ports, work)?;
    let (with_relay, without_relay) = pages.records()?;
    Ok(PagedRun {
        played,
        with_relay,
        without_relay,
    })
}

/// The `key` of a manifest row as a sample count.
fn sample_of(row: &Row, key: &str) -> Result<usize, Failure> {
    let at = row
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("a row has no integer `{key}`"))?;
    usize::try_from(at).map_err(|error| error.to_string())
}

/// `row` with its samples counted from sample `from` of the recording.
fn rebased(row: &Row, from: usize) -> Result<Row, Failure> {
    let mut moved = row.clone();
    for key in ["start_sample", "end_sample"] {
        let at = sample_of(row, key)?
            .checked_sub(from)
            .ok_or_else(|| format!("`{key}` of a row lies before the cut"))?;
        moved.insert(String::from(key), Value::from(at));
    }
    Ok(moved)
}

/// Rows `first_row` to `first_row + rows - 1` (counted from 1) of `lat-dialog`
/// as a wav that starts at the end of the row before them, and their manifest
/// with the samples counted from the start of that wav. Returns `(wav, manifest)`.
pub fn dialog_window(
    work: &Path,
    first_row: usize,
    rows: usize,
) -> Result<(PathBuf, PathBuf), Failure> {
    let all = read_rows(&find_above("testdata/manifests/lat-dialog.json")?)
        .map_err(|error| error.to_string())?;
    let first = first_row.checked_sub(1).ok_or("rows are counted from 1")?;
    let window = all
        .get(first..first + rows)
        .ok_or_else(|| format!("lat-dialog has fewer than {} utterances", first + rows))?;
    let before = first
        .checked_sub(1)
        .and_then(|at| all.get(at))
        .ok_or("the window has no row before it")?;
    let from = sample_of(before, "end_sample")?;
    let to = sample_of(window.last().ok_or("the window has no row")?, "end_sample")?;
    let whole =
        read_wav(&find_above("testdata/lat-dialog.wav")?).map_err(|error| error.to_string())?;
    let samples = whole
        .samples
        .get(from..to)
        .ok_or("lat-dialog.wav is shorter than its manifest")?;
    let moved = window
        .iter()
        .map(|row| rebased(row, from))
        .collect::<Result<Vec<_>, _>>()?;
    let (wav, manifest) = (work.join("window.wav"), work.join("window.json"));
    write_wav(&wav, SAMPLE_RATE, samples).map_err(|error| error.to_string())?;
    write_rows(&manifest, &moved).map_err(|error| error.to_string())?;
    Ok((wav, manifest))
}

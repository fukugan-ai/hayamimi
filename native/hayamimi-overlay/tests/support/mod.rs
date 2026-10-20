//! Shared pieces of the acceptance tests of the overlay window: starting the
//! real `hayamimi-overlay` process, reading its JSONL log, fake `/events`
//! servers it can follow, and the band of design 11.4 worked out on the test's
//! side from the numbers of that section.
//!
//! Only Windows builds this: every test file that includes it starts with
//! `#![cfg(windows)]`.
//!
//! # What the tests read from the log
//!
//! Event names and the keys of design 11.3 are fixed there; the shape of the
//! two nested events is not, so the tests read them as follows and the window
//! has to write them so:
//!
//! - `overlay.line.rendered`: flat keys `id`, `lane` (`upper` | `lower`),
//!   `state`, `translated` (bool).
//! - `overlay.layout.measured`: `inner_height`, `zoom`, `probe_shrink`,
//!   `missing_glyphs` (numbers), `fonts` (array of `{family, status}`), `rows`
//!   (array of `{id, scroll_height, client_height, bottom, font_family}`),
//!   `lanes` (array of `{lane, bottom}`). The probe's rows carry the `id`
//!   prefix of their lane (`loopback-` in the upper lane, `mic-` in the lower),
//!   which is also how the lanes are chosen.
//! - `table.load.failed`: `table` (string).

#![allow(dead_code)]

pub mod win;

use std::ffi::OsStr;
use std::fs;
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use hayamimi_overlay::WINDOW_CLASS;
use serde_json::{Value, json};

use win::{Pixels, Rect, Win};

pub type TestResult = Result<(), Box<dyn std::error::Error>>;

/// The built overlay executable.
pub const OVERLAY_EXE: &str = env!("CARGO_BIN_EXE_hayamimi-overlay");

/// How long a started overlay may take to show its window. The first start
/// also pays for the WebView2 profile directory.
pub const OPEN_TIMEOUT: Duration = Duration::from_secs(60);

/// How long a closed overlay may linger after `WM_CLOSE` (design 11.9).
const CLOSE_TIMEOUT: Duration = Duration::from_millis(5000);

const POLL: Duration = Duration::from_millis(50);

pub fn sleep_ms(milliseconds: u64) {
    std::thread::sleep(Duration::from_millis(milliseconds));
}

/// A directory for one test's files, under the build's `target` and never `/tmp`.
pub fn scratch() -> io::Result<tempfile::TempDir> {
    tempfile::tempdir_in(concat!(env!("CARGO_MANIFEST_DIR"), "/../target"))
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

// ---- reading the log ---------------------------------------------------

pub fn text<'a>(event: &'a Value, key: &str) -> Option<&'a str> {
    event.get(key).and_then(Value::as_str)
}

pub fn flag(event: &Value, key: &str) -> Option<bool> {
    event.get(key).and_then(Value::as_bool)
}

pub fn is_event(event: &Value, name: &str) -> bool {
    text(event, "event") == Some(name)
}

/// A line of the overlay was drawn, for the row `id`.
pub fn rendered(event: &Value, id: &str) -> bool {
    is_event(event, "overlay.line.rendered") && text(event, "id") == Some(id)
}

/// A line was drawn with its translation, for the row `id`.
pub fn rendered_translated(event: &Value, id: &str) -> bool {
    rendered(event, id) && flag(event, "translated") == Some(true)
}

/// The JSONL file the overlay writes (`--log`).
pub struct Log {
    path: PathBuf,
}

impl Log {
    pub fn at(path: PathBuf) -> Log {
        Log { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Every complete event written so far, oldest first.
    pub fn events(&self) -> Vec<Value> {
        // DEFAULT: the overlay creates its log lazily, so a missing file means no event has been written yet.
        let Ok(bytes) = fs::read(&self.path) else {
            return Vec::new();
        };
        String::from_utf8_lossy(&bytes)
            .lines()
            // A line cut by a write in progress does not parse; the next poll reads it whole.
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .collect()
    }

    pub fn named(&self, name: &str) -> Vec<Value> {
        self.events()
            .into_iter()
            .filter(|event| is_event(event, name))
            .collect()
    }

    pub fn count(&self, matching: impl Fn(&Value) -> bool) -> usize {
        self.events().iter().filter(|event| matching(event)).count()
    }

    fn find(&self, matching: &dyn Fn(&Value) -> bool) -> Option<Value> {
        self.events().into_iter().find(|event| matching(event))
    }

    /// A one-line digest of the events so far, for a failure message.
    pub fn digest(&self) -> String {
        let events = self.events();
        let names: Vec<String> = events
            .iter()
            .map(|event| {
                let name = text(event, "event").map_or_else(|| event.to_string(), str::to_owned);
                match text(event, "id") {
                    Some(id) => format!("{name}[{id}]"),
                    None => name,
                }
            })
            .collect();
        format!("{} events: {}", events.len(), names.join(", "))
    }
}

// ---- the overlay process -----------------------------------------------

/// A running `hayamimi-overlay`. Dropping it kills a process that is still there.
pub struct Overlay {
    child: Child,
    log: Log,
    started: Instant,
}

impl Overlay {
    /// Starts the overlay with `args` and a log file in `log`. Refuses to start
    /// while a window of the overlay's class exists, since that is a real overlay
    /// and the test would read it instead.
    pub fn launch<I, S>(args: I, log: Log) -> Result<Overlay, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        win::per_monitor_aware()?;
        if win::class_exists(WINDOW_CLASS)? {
            return Err(format!(
                "a window of class {WINDOW_CLASS} already exists; close the running overlay before the test"
            ));
        }
        let child = Command::new(OVERLAY_EXE)
            .args(args)
            .arg("--log")
            .arg(log.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("could not start {OVERLAY_EXE}: {error}"))?;
        Ok(Overlay {
            child,
            log,
            started: Instant::now(),
        })
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    pub fn log(&self) -> &Log {
        &self.log
    }

    /// Milliseconds since the process was started.
    pub fn age_ms(&self) -> u128 {
        self.started.elapsed().as_millis()
    }

    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// The exit code once the process has ended; `None` while it runs.
    /// A process ended by a signal has no code and is an error.
    pub fn try_exit_code(&mut self) -> Result<Option<i32>, String> {
        match self.child.try_wait() {
            Ok(None) => Ok(None),
            Ok(Some(status)) => status
                .code()
                .map(Some)
                .ok_or_else(|| format!("the overlay ended without an exit code ({status})")),
            Err(error) => Err(format!("could not poll the overlay: {error}")),
        }
    }

    /// Waits for the first event `matching` the predicate, and fails early if the process exits first.
    pub fn wait(
        &mut self,
        what: &str,
        timeout: Duration,
        matching: impl Fn(&Value) -> bool,
    ) -> Result<Value, String> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(found) = self.log.find(&matching) {
                return Ok(found);
            }
            if let Ok(Some(status)) = self.child.try_wait() {
                return Err(format!(
                    "the overlay exited ({status}) before {what}; {}",
                    self.log.digest()
                ));
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "{what} did not happen within {timeout:?}; {}",
                    self.log.digest()
                ));
            }
            std::thread::sleep(POLL);
        }
    }

    /// Waits for `overlay.window.opened`, the line after which the window may be read.
    pub fn wait_opened(&mut self) -> Result<Value, String> {
        self.wait("overlay.window.opened", OPEN_TIMEOUT, |event| {
            is_event(event, "overlay.window.opened")
        })
    }

    /// The overlay's own top-level window, found through the OS by class and process id.
    pub fn window(&self) -> Result<Win, String> {
        let mut found = win::windows_of(WINDOW_CLASS, self.pid())?;
        if found.is_empty() {
            return Err(format!(
                "process {} owns no window of class {WINDOW_CLASS}",
                self.pid()
            ));
        }
        Ok(found.remove(0))
    }

    /// Closes the overlay with `WM_CLOSE`. A process that is still there after
    /// 5000 ms is killed and the test fails, naming its pid.
    pub fn close(mut self) -> Result<(), String> {
        for window in win::windows_of(WINDOW_CLASS, self.pid())? {
            win::post_close(window)?;
        }
        let deadline = Instant::now() + CLOSE_TIMEOUT;
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => return Ok(()),
                Ok(None) => {}
                Err(error) => return Err(format!("could not poll the overlay: {error}")),
            }
            if Instant::now() >= deadline {
                let pid = self.pid();
                self.kill();
                return Err(format!(
                    "overlay pid {pid} was still running {CLOSE_TIMEOUT:?} after WM_CLOSE; killed"
                ));
            }
            std::thread::sleep(POLL);
        }
    }

    fn kill(&mut self) {
        // A process that exited in the meantime has nothing left to kill, and `wait` collects it either way.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Overlay {
    fn drop(&mut self) {
        if self.is_running() {
            self.kill();
        }
    }
}

// ---- fake /events servers ----------------------------------------------

const SSE_HEAD: &[u8] =
    b"HTTP/1.0 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\n\r\n";

/// An `/events` server the overlay can follow: close-delimited `HTTP/1.0`, one
/// `data:` line per event, as the recognizers and the relay serve it. It sends
/// to the clients connected now and keeps nothing for later ones. Stopping it
/// closes every connection and frees the port.
pub struct Sse {
    address: SocketAddr,
    clients: Arc<Mutex<Vec<TcpStream>>>,
    connections: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

/// Reads the request head the client sent, up to the blank line.
fn read_head(stream: &mut TcpStream) -> Option<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    let mut raw = Vec::new();
    let mut chunk = [0_u8; 512];
    while !raw.windows(4).any(|window| window == b"\r\n\r\n") {
        let count = stream.read(&mut chunk).ok().filter(|count| *count > 0)?;
        raw.extend_from_slice(chunk.get(..count)?);
    }
    Some(())
}

fn admit(mut stream: TcpStream, clients: &Mutex<Vec<TcpStream>>, connections: &AtomicUsize) {
    let ready = stream.set_nonblocking(false).is_ok()
        && read_head(&mut stream).is_some()
        && stream.write_all(SSE_HEAD).is_ok();
    if ready {
        locked(clients).push(stream);
        connections.fetch_add(1, Ordering::SeqCst);
    }
}

fn serve(
    listener: TcpListener,
    clients: Arc<Mutex<Vec<TcpStream>>>,
    connections: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
) {
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => admit(stream, &clients, &connections),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => sleep_ms(10),
            Err(_) => break,
        }
    }
    for client in locked(&clients).drain(..) {
        // A client that hung up first has nothing left to close.
        let _ = client.shutdown(Shutdown::Both);
    }
}

impl Sse {
    /// A server on a free port of the loopback address.
    pub fn start() -> io::Result<Sse> {
        Sse::on(TcpListener::bind("127.0.0.1:0")?)
    }

    /// A server on `port`, to bring back a server that was stopped.
    pub fn start_on(port: u16) -> io::Result<Sse> {
        Sse::on(TcpListener::bind(("127.0.0.1", port))?)
    }

    fn on(listener: TcpListener) -> io::Result<Sse> {
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?;
        let clients = Arc::new(Mutex::new(Vec::new()));
        let connections = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = std::thread::spawn({
            let (clients, connections, stop) = (
                Arc::clone(&clients),
                Arc::clone(&connections),
                Arc::clone(&stop),
            );
            move || serve(listener, clients, connections, stop)
        });
        Ok(Sse {
            address,
            clients,
            connections,
            stop,
            thread: Some(thread),
        })
    }

    pub fn url(&self) -> String {
        format!("http://{}/events", self.address)
    }

    pub fn port(&self) -> u16 {
        self.address.port()
    }

    /// How many clients have connected so far.
    pub fn connections(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }

    /// Waits until `count` clients have connected.
    pub fn wait_connections(&self, count: usize, timeout: Duration) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        while self.connections() < count {
            if Instant::now() >= deadline {
                return Err(format!(
                    "{} saw {} of {count} connections within {timeout:?}",
                    self.url(),
                    self.connections()
                ));
            }
            std::thread::sleep(POLL);
        }
        Ok(())
    }

    /// Sends one event, a JSON text, to every connected client.
    pub fn send(&self, json: &str) {
        let line = format!("data: {json}\n\n");
        locked(&self.clients).retain_mut(|client| {
            client
                .write_all(line.as_bytes())
                .and_then(|()| client.flush())
                .is_ok()
        });
    }

    /// Stops serving, closes every connection and frees the port.
    pub fn stop(mut self) {
        self.shut_down();
    }

    fn shut_down(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            // A serving thread that panicked has already dropped its listener.
            let _ = thread.join();
        }
    }
}

impl Drop for Sse {
    fn drop(&mut self) {
        self.shut_down();
    }
}

/// The three streams the overlay follows: the microphone's recognizer, the
/// loopback's recognizer, and the relay.
pub struct Streams {
    pub mic: Sse,
    pub loopback: Sse,
    pub relay: Sse,
}

impl Streams {
    pub fn start() -> io::Result<Streams> {
        Ok(Streams {
            mic: Sse::start()?,
            loopback: Sse::start()?,
            relay: Sse::start()?,
        })
    }

    /// The command line that points the overlay at the three servers.
    pub fn args(&self) -> Vec<String> {
        vec![
            String::from("--events"),
            self.mic.url(),
            String::from("--events"),
            self.loopback.url(),
            String::from("--relay"),
            self.relay.url(),
        ]
    }

    /// Waits until the overlay follows all three.
    pub fn wait_followed(&self) -> Result<(), String> {
        for server in [&self.mic, &self.loopback, &self.relay] {
            server.wait_connections(1, OPEN_TIMEOUT)?;
        }
        Ok(())
    }
}

/// A confirmed line as a recognizer sends it.
pub fn final_line(id: &str, text: &str, lang: &str) -> String {
    json!({"type": "final", "id": id, "text": text, "lang": lang, "speaker": "", "latency_ms": 300.0})
        .to_string()
}

/// The deciding translation of the line `id`, as the relay sends it.
pub fn translation_line(id: &str, lang: &str, text: &str) -> String {
    json!({"type": "translation", "of": "final", "id": id, "lang": lang, "text": text, "status": "ok", "settled": true})
        .to_string()
}

// ---- the band of design 11.4, worked out here ---------------------------

/// The settings that decide the band (the keys of `overlay.toml`).
#[derive(Clone, Copy, Debug)]
pub struct BandSpec {
    pub width_ratio: f64,
    pub bottom_gap_px: f64,
    pub upper_rows: u32,
    pub lower_rows: u32,
}

/// The built-in settings of design 11.3.
pub const DEFAULT_BAND: BandSpec = BandSpec {
    width_ratio: 0.8,
    bottom_gap_px: 0.0,
    upper_rows: 2,
    lower_rows: 1,
};

/// A row is 1.5 times its font size high: 36 px in the upper lane, 26 px in the lower.
const UPPER_ROW_CSS: f64 = 54.0;
const LOWER_ROW_CSS: f64 = 39.0;
/// The margin under the band: 0.3 times the upper font size.
const FOOT_CSS: f64 = 10.8;

/// The height of the band in CSS px: rows times row height, plus the margin.
pub fn band_height_css(spec: &BandSpec) -> f64 {
    f64::from(spec.upper_rows) * UPPER_ROW_CSS
        + f64::from(spec.lower_rows) * LOWER_ROW_CSS
        + FOOT_CSS
}

fn round_px(value: f64) -> i32 {
    value.round() as i32
}

/// The band on `area` (the work area, physical px) at `scale`: `width_ratio`
/// of the width in CSS px, centred, its bottom edge `bottom_gap_px` above the
/// bottom of the area.
pub fn expected_band(area: Rect, scale: f64, spec: &BandSpec) -> Rect {
    let width_css = spec.width_ratio * (f64::from(area.width()) / scale);
    let width = width_css * scale;
    let height = band_height_css(spec) * scale;
    let left = f64::from(area.left) + (f64::from(area.width()) - width) / 2.0;
    let bottom = f64::from(area.bottom) - spec.bottom_gap_px * scale;
    Rect {
        left: round_px(left),
        top: round_px(bottom - height),
        right: round_px(left + width),
        bottom: round_px(bottom),
    }
}

/// Whether every edge of `a` is within `tolerance` physical px of `b`.
pub fn within(a: Rect, b: Rect, tolerance: i32) -> bool {
    (a.left - b.left).abs() <= tolerance
        && (a.top - b.top).abs() <= tolerance
        && (a.right - b.right).abs() <= tolerance
        && (a.bottom - b.bottom).abs() <= tolerance
}

// ---- the layout probe ---------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct RowBox {
    pub id: String,
    pub scroll_height: f64,
    pub client_height: f64,
    pub bottom: f64,
    pub font_family: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LaneBox {
    pub lane: String,
    pub bottom: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FontFace {
    pub family: String,
    pub status: String,
}

/// What `overlay.layout.measured` carries, in CSS px.
#[derive(Clone, Debug, PartialEq)]
pub struct Measured {
    pub inner_height: f64,
    pub zoom: f64,
    pub probe_shrink: f64,
    pub missing_glyphs: u64,
    pub rows: Vec<RowBox>,
    pub lanes: Vec<LaneBox>,
    pub fonts: Vec<FontFace>,
}

fn number(from: &Value, key: &str) -> Result<f64, String> {
    from.get(key)
        .and_then(Value::as_f64)
        .ok_or_else(|| format!("`{key}` is missing or not a number in {from}"))
}

fn string(from: &Value, key: &str) -> Result<String, String> {
    text(from, key)
        .map(str::to_owned)
        .ok_or_else(|| format!("`{key}` is missing or not a string in {from}"))
}

fn list<'a>(from: &'a Value, key: &str) -> Result<&'a Vec<Value>, String> {
    from.get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("`{key}` is missing or not an array in {from}"))
}

fn row_box(row: &Value) -> Result<RowBox, String> {
    Ok(RowBox {
        id: string(row, "id")?,
        scroll_height: number(row, "scroll_height")?,
        client_height: number(row, "client_height")?,
        bottom: number(row, "bottom")?,
        font_family: string(row, "font_family")?,
    })
}

fn lane_box(lane: &Value) -> Result<LaneBox, String> {
    Ok(LaneBox {
        lane: string(lane, "lane")?,
        bottom: number(lane, "bottom")?,
    })
}

fn font_face(font: &Value) -> Result<FontFace, String> {
    Ok(FontFace {
        family: string(font, "family")?,
        status: string(font, "status")?,
    })
}

/// Reads one `overlay.layout.measured` event.
pub fn parse_measured(event: &Value) -> Result<Measured, String> {
    let missing = event
        .get("missing_glyphs")
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("`missing_glyphs` is missing or not a count in {event}"))?;
    Ok(Measured {
        inner_height: number(event, "inner_height")?,
        zoom: number(event, "zoom")?,
        probe_shrink: number(event, "probe_shrink")?,
        missing_glyphs: missing,
        rows: list(event, "rows")?
            .iter()
            .map(row_box)
            .collect::<Result<_, _>>()?,
        lanes: list(event, "lanes")?
            .iter()
            .map(lane_box)
            .collect::<Result<_, _>>()?,
        fonts: list(event, "fonts")?
            .iter()
            .map(font_face)
            .collect::<Result<_, _>>()?,
    })
}

/// The first family of a CSS `font-family` value, without quotes: what the
/// browser tries first for the text.
pub fn first_family(css: &str) -> &str {
    css.split(',')
        .next()
        .map_or(css, |first| first.trim().trim_matches(['"', '\'']))
}

/// The ways a probe breaks design 3.12: a row whose text overflows its box, a
/// row or a lane whose bottom is below the bottom of the window.
pub fn clipping(measured: &Measured) -> Vec<String> {
    let mut found = Vec::new();
    for row in &measured.rows {
        if row.scroll_height > row.client_height {
            found.push(format!(
                "row {}: scrollHeight {} > clientHeight {}",
                row.id, row.scroll_height, row.client_height
            ));
        }
        if row.bottom > measured.inner_height {
            found.push(format!(
                "row {}: bottom {} > innerHeight {}",
                row.id, row.bottom, measured.inner_height
            ));
        }
    }
    for lane in &measured.lanes {
        if lane.bottom > measured.inner_height {
            found.push(format!(
                "lane {}: bottom {} > innerHeight {}",
                lane.lane, lane.bottom, measured.inner_height
            ));
        }
    }
    found
}

// ---- comparing the screen before and after the window --------------------

/// What comparing the ring around the band found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RingReport {
    /// Ring pixels that were steady between the two grabs before the window.
    pub compared: usize,
    /// Pixels (ring or not) that changed between the two grabs before the window.
    pub noisy: usize,
    /// Steady ring pixels that differ after the window is up.
    pub mismatched: usize,
    /// Steady pixels inside the ring that differ after the window is up.
    pub interior_changed: usize,
}

fn same_colour(a: &[u8], b: &[u8]) -> bool {
    a.get(..3) == b.get(..3)
}

/// Whether the pixel `(x, y)` of a grab `margin` px wider on every side than a
/// `band_width` x `band_height` band is within `margin` px of the band's edge,
/// inside it or outside it.
fn on_ring(x: usize, y: usize, band_width: usize, band_height: usize, margin: usize) -> bool {
    x < 2 * margin || y < 2 * margin || x >= band_width || y >= band_height
}

/// Compares three grabs of a region `margin` px wider on every side than a
/// `band_width` x `band_height` band: two before the window, one after. The
/// ring is the `margin` px at the edge of the band, inside it and outside it
/// (where a frame, a border or a shadow would be). A pixel that changed
/// between the two grabs before the window is noise and is left out.
pub fn compare_ring(
    first: &Pixels,
    second: &Pixels,
    after: &Pixels,
    band_width: usize,
    band_height: usize,
    margin: usize,
) -> Result<RingReport, String> {
    let (width, height) = (band_width + 2 * margin, band_height + 2 * margin);
    for grab in [first, second, after] {
        if (grab.width, grab.height) != (width, height) {
            return Err(format!(
                "a grab is {}x{}, not {width}x{height}",
                grab.width, grab.height
            ));
        }
    }
    let mut report = RingReport {
        compared: 0,
        noisy: 0,
        mismatched: 0,
        interior_changed: 0,
    };
    for y in 0..height {
        for x in 0..width {
            let at = (y * width + x) * 4;
            let (Some(a), Some(b), Some(c)) = (
                first.bgra.get(at..at + 4),
                second.bgra.get(at..at + 4),
                after.bgra.get(at..at + 4),
            ) else {
                return Err(String::from("a grab is shorter than its size says"));
            };
            if !same_colour(a, b) {
                report.noisy += 1;
                continue;
            }
            let changed = !same_colour(a, c);
            if on_ring(x, y, band_width, band_height, margin) {
                report.compared += 1;
                report.mismatched += usize::from(changed);
            } else {
                report.interior_changed += usize::from(changed);
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(width: usize, height: usize, colour: u8) -> Pixels {
        Pixels {
            width,
            height,
            bgra: vec![colour; width * height * 4],
        }
    }

    fn paint(pixels: &mut Pixels, x: usize, y: usize, colour: u8) {
        let at = (y * pixels.width + x) * 4;
        pixels.bgra[at..at + 3].fill(colour);
    }

    #[test]
    fn the_default_band_on_the_work_area_of_the_design_is_3072_by_237() {
        let area = Rect {
            left: 0,
            top: 0,
            right: 3840,
            bottom: 2088,
        };
        let band = expected_band(area, 1.5, &DEFAULT_BAND);
        assert_eq!((band.width(), band.height()), (3072, 237));
        assert_eq!((band.left, band.bottom), (384, 2088));
        assert!((band_height_css(&DEFAULT_BAND) - 157.8).abs() < 1e-9);
    }

    #[test]
    fn a_gap_above_the_bottom_is_counted_in_css_px() {
        let area = Rect {
            left: 0,
            top: 0,
            right: 3840,
            bottom: 2088,
        };
        let spec = BandSpec {
            bottom_gap_px: 20.0,
            ..DEFAULT_BAND
        };
        assert_eq!(expected_band(area, 1.5, &spec).bottom, 2088 - 30);
    }

    #[test]
    fn a_narrower_ratio_narrows_the_band_around_the_same_centre() {
        let area = Rect {
            left: 0,
            top: 0,
            right: 3840,
            bottom: 2088,
        };
        let spec = BandSpec {
            width_ratio: 0.6,
            ..DEFAULT_BAND
        };
        let band = expected_band(area, 1.5, &spec);
        assert_eq!(band.width(), 2304);
        assert_eq!(band.center().0, 1920);
    }

    #[test]
    fn two_rectangles_one_pixel_apart_are_within_one_and_not_within_zero() {
        let a = Rect {
            left: 0,
            top: 0,
            right: 10,
            bottom: 10,
        };
        let b = Rect {
            left: 1,
            top: 0,
            right: 10,
            bottom: 11,
        };
        assert!(within(a, b, 1));
        assert!(!within(a, b, 0));
    }

    fn measured_json(lower_bottom: f64) -> Value {
        json!({
            "inner_height": 158.0, "zoom": 100.0, "probe_shrink": 0.0, "missing_glyphs": 3,
            "rows": [{"id": "mic-1", "scroll_height": 39.0, "client_height": 39.0, "bottom": lower_bottom, "font_family": "UDEV Gothic 35"}],
            "lanes": [{"lane": "lower", "bottom": lower_bottom}],
            "fonts": [{"family": "UDEV Gothic 35", "status": "loaded"}],
        })
    }

    #[test]
    fn the_first_family_is_the_one_the_browser_tries_first() {
        assert_eq!(
            first_family("cinecaption, \"Yu Gothic UI\", sans-serif"),
            "cinecaption"
        );
        assert_eq!(
            first_family("\"UDEV Gothic 35\", monospace"),
            "UDEV Gothic 35"
        );
        assert_eq!(first_family("'cinecaption'"), "cinecaption");
        assert_eq!(first_family("monospace"), "monospace");
    }

    #[test]
    fn a_probe_that_fits_has_no_clipping() -> Result<(), String> {
        let measured = parse_measured(&measured_json(147.0))?;
        assert_eq!(measured.missing_glyphs, 3);
        assert!(clipping(&measured).is_empty());
        Ok(())
    }

    #[test]
    fn a_lane_below_the_window_edge_is_clipping() -> Result<(), String> {
        let measured = parse_measured(&measured_json(159.0))?;
        assert_eq!(clipping(&measured).len(), 2);
        Ok(())
    }

    #[test]
    fn a_row_taller_than_its_box_is_clipping() -> Result<(), String> {
        let mut event = measured_json(100.0);
        event["rows"][0]["scroll_height"] = json!(41.0);
        assert_eq!(clipping(&parse_measured(&event)?).len(), 1);
        Ok(())
    }

    #[test]
    fn an_event_without_its_rows_is_refused_by_name() {
        let mut event = measured_json(100.0);
        if let Some(object) = event.as_object_mut() {
            object.remove("rows");
        }
        let said = parse_measured(&event).err();
        assert!(
            said.as_deref().is_some_and(|text| text.contains("`rows`")),
            "{said:?}"
        );
    }

    #[test]
    fn a_change_inside_the_band_is_not_a_change_of_the_ring() -> Result<(), String> {
        let (band_w, band_h, margin) = (40, 30, 4);
        let first = flat(48, 38, 7);
        let mut after = first.clone();
        paint(&mut after, 24, 19, 200);
        let report = compare_ring(&first, &first, &after, band_w, band_h, margin)?;
        assert_eq!((report.mismatched, report.interior_changed), (0, 1));
        assert!(report.compared > 0);
        Ok(())
    }

    #[test]
    fn a_change_on_the_ring_is_found_inside_and_outside_the_band() -> Result<(), String> {
        let (band_w, band_h, margin) = (40, 30, 4);
        let first = flat(48, 38, 7);
        let mut after = first.clone();
        paint(&mut after, 1, 1, 200);
        paint(&mut after, 5, 19, 200);
        let report = compare_ring(&first, &first, &after, band_w, band_h, margin)?;
        assert_eq!(report.mismatched, 2);
        Ok(())
    }

    #[test]
    fn a_pixel_that_moved_before_the_window_is_noise_and_is_left_out() -> Result<(), String> {
        let (band_w, band_h, margin) = (40, 30, 4);
        let first = flat(48, 38, 7);
        let mut second = first.clone();
        paint(&mut second, 1, 1, 99);
        let mut after = first.clone();
        paint(&mut after, 1, 1, 200);
        let report = compare_ring(&first, &second, &after, band_w, band_h, margin)?;
        assert_eq!((report.noisy, report.mismatched), (1, 0));
        Ok(())
    }

    #[test]
    fn grabs_of_different_sizes_are_refused() {
        let small = flat(10, 10, 0);
        let big = flat(48, 38, 0);
        assert!(compare_ring(&big, &big, &small, 40, 30, 4).is_err());
    }
}

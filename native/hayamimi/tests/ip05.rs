//! IP-5: one page shows both directions of translation.
//!
//! The real run (T-IP-05-001 to 004) starts `hayamimi` on a websocket input,
//! the relay, a stand-in translation server and `chrome-headless-shell`, opens
//! the dashboard with `?relay=` before the first line is played, plays the
//! first four utterances of `lat-dialog` (ja, en, ja, en) and reads the html of
//! the page over the DevTools protocol once the relay has settled the fourth
//! translation. It is run once and the four rows judge what it saw. They are
//! `#[ignore]`d because they need the models, `testdata/` (not in git), the
//! page bundle next to the `hayamimi` binary and a browser.
//!
//! The rows 005, 006 and 017 to 019 test the tools that read a dump of the
//! page, on html written by hand. The rows 007 to 016 read the `?relay=` of an
//! address and are in `hayamimi-ui/src/route.rs`. The record of the run is
//! `docs/results/in-person/ip05.md`.

mod support;

use std::fs::File;
use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use hayamimi_eval::manifest::{read_rows, write_rows};
use hayamimi_eval::wav::{SAMPLE_RATE, read_wav, write_wav};
use serde_json::Value;
use support::dialog::append_line;
use support::dialog_run::sibling_binary;
use support::{BINARY, Failure, TestResult, data_events, find_above, models_dir, path_text};

/// What `fake_llama_server -m translates.gguf` says for a translation into
/// English and into Japanese.
const ENGLISH: &str = "That is a very good question.";
const JAPANESE: &str = "それはとても良い質問です。";

const FAKE_SERVER: &str = env!("CARGO_BIN_EXE_fake_llama_server");
const RECORD: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/results/in-person/ip05.md"
);
const UTTERANCES: usize = 4;
/// How long the stack is given to open its ports: the models are read first.
const START_WAIT: Duration = Duration::from_secs(300);
/// How long a browser is given to start and to draw the page.
const BROWSER_WAIT: Duration = Duration::from_secs(60);
/// How long a page that is drawn is given to connect its streams, before the
/// first line is played.
const CONNECT_GRACE: Duration = Duration::from_secs(3);
/// How long the last translation is given after the stream has ended.
const TRANSLATE_WAIT: Duration = Duration::from_secs(90);
/// How long a page is given after its last translation, for the pages that
/// should show none to show one.
const SHOW_GRACE: Duration = Duration::from_secs(2);

// ---- a dump of the page ---------------------------------------------------

/// One element of a dumped page: its tag, its class tokens, its parent, and
/// the text inside it, its descendants' included.
struct Element {
    tag: String,
    classes: Vec<String>,
    parent: Option<usize>,
    text: String,
}

/// Elements whose content is not markup.
const RAW_TEXT: [&str; 2] = ["script", "style"];
/// Elements that have no end tag.
const VOID: [&str; 14] = [
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// The end of `from`: what is left once all of it has been read.
fn consumed(from: &str) -> &str {
    &from[from.len()..]
}

/// The index of the `>` that ends the tag at the start of `from`, skipping a
/// `>` inside a quoted value.
fn tag_end(from: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (at, c) in from.char_indices() {
        match (quote, c) {
            (Some(open), c) if c == open => quote = None,
            (None, '"' | '\'') => quote = Some(c),
            (None, '>') => return Some(at),
            _ => {}
        }
    }
    None
}

/// The value of the `class` attribute in the text after a tag's name.
fn class_attribute(attributes: &str) -> Option<&str> {
    let (at, _) = attributes
        .match_indices("class=")
        .find(|(at, _)| attributes[..*at].ends_with(char::is_whitespace))?;
    let value = &attributes[at + "class=".len()..];
    match value.chars().next()? {
        quote @ ('"' | '\'') => value[1..].split(quote).next(),
        _ => value.split(char::is_whitespace).next(),
    }
}

fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// Adds `text` to every element that is open.
fn add_text(found: &mut [Element], open: &[usize], text: &str) {
    if text.is_empty() {
        return;
    }
    let text = unescape(text);
    for at in open {
        found[*at].text.push_str(&text);
    }
}

/// Closes the innermost open element called `name`, and what is open inside it.
fn close(found: &[Element], open: &mut Vec<usize>, name: &str) {
    if let Some(depth) = open.iter().rposition(|at| found[*at].tag == name) {
        open.truncate(depth);
    }
}

/// Opens the element a start tag makes; `inner` is the tag without `<` and `>`.
fn open_element(inner: &str, found: &mut Vec<Element>, open: &mut Vec<usize>) -> String {
    let tag: String = inner
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect::<String>()
        .to_ascii_lowercase();
    let classes = class_attribute(&inner[tag.len()..])
        .into_iter()
        .flat_map(str::split_whitespace)
        .map(str::to_owned)
        .collect();
    found.push(Element {
        tag: tag.clone(),
        classes,
        parent: open.last().copied(),
        text: String::new(),
    });
    if !inner.ends_with('/') && !VOID.contains(&tag.as_str()) {
        open.push(found.len() - 1);
    }
    tag
}

/// Reads the markup at the start of `from` (it begins with `<`) and returns
/// what follows it.
fn read_markup<'a>(from: &'a str, found: &mut Vec<Element>, open: &mut Vec<usize>) -> &'a str {
    if let Some(rest) = from.strip_prefix("<!--") {
        return match rest.find("-->") {
            Some(end) => &rest[end + 3..],
            None => consumed(rest),
        };
    }
    let Some(end) = tag_end(from) else {
        return consumed(from);
    };
    let (inner, after) = (&from[1..end], &from[end + 1..]);
    if inner.starts_with(['!', '?']) {
        return after;
    }
    if let Some(name) = inner.strip_prefix('/') {
        close(found, open, name.trim().to_ascii_lowercase().as_str());
        return after;
    }
    let tag = open_element(inner, found, open);
    if !RAW_TEXT.contains(&tag.as_str()) {
        return after;
    }
    let closing = format!("</{tag}");
    match after.to_ascii_lowercase().find(&closing) {
        Some(at) => &after[at..],
        None => consumed(after),
    }
}

fn parse_elements(html: &str) -> Vec<Element> {
    let mut found = Vec::new();
    let mut open = Vec::new();
    let mut rest = html;
    while !rest.is_empty() {
        let Some(at) = rest.find('<') else {
            add_text(&mut found, &open, rest);
            break;
        };
        add_text(&mut found, &open, &rest[..at]);
        rest = read_markup(&rest[at..], &mut found, &mut open);
    }
    found
}

/// What the page drew for one translation.
#[derive(Debug)]
struct Tr {
    arrow: Option<String>,
    text: Option<String>,
    mark: Option<String>,
    reason: Option<String>,
}

/// What the page drew for one confirmed line.
#[derive(Debug)]
struct Row {
    badge: Option<String>,
    trs: Vec<Tr>,
}

/// The rows of a dumped dashboard, and how many `tr` elements the page holds
/// anywhere.
#[derive(Debug)]
struct Dump {
    rows: Vec<Row>,
    trs_total: usize,
}

fn has_class(element: &Element, class: &str) -> bool {
    element.classes.iter().any(|token| token == class)
}

/// Whether element `at` is `ancestor` or inside it.
fn inside(found: &[Element], at: usize, ancestor: usize) -> bool {
    std::iter::successors(Some(at), |current| found[*current].parent)
        .any(|current| current == ancestor)
}

/// The text of the first element of class `class` inside element `within`.
fn text_of(found: &[Element], within: usize, class: &str) -> Option<String> {
    found
        .iter()
        .enumerate()
        .find(|(at, element)| has_class(element, class) && inside(found, *at, within))
        .map(|(_, element)| element.text.clone())
}

fn tr_at(found: &[Element], at: usize) -> Tr {
    Tr {
        arrow: text_of(found, at, "tr-arrow"),
        text: text_of(found, at, "tr-text"),
        mark: text_of(found, at, "tr-mark"),
        reason: text_of(found, at, "tr-reason"),
    }
}

fn row_at(found: &[Element], at: usize) -> Row {
    let trs = found
        .iter()
        .enumerate()
        .filter(|(inner, element)| has_class(element, "tr") && inside(found, *inner, at))
        .map(|(inner, _)| tr_at(found, inner))
        .collect();
    Row {
        badge: text_of(found, at, "lang-badge"),
        trs,
    }
}

/// Reads a dump of the dashboard. A dump with no element of class `dash` is
/// of a page that was not drawn, and is an error rather than zero rows.
fn read_dump(html: &str) -> Result<Dump, Failure> {
    let found = parse_elements(html);
    if !found.iter().any(|element| has_class(element, "dash")) {
        return Err(String::from(
            "the dump has no element of class `dash`: the page was not drawn",
        ));
    }
    let rows = found
        .iter()
        .enumerate()
        .filter(|(_, element)| has_class(element, "row"))
        .map(|(at, _)| row_at(&found, at))
        .collect();
    let trs_total = found
        .iter()
        .filter(|element| has_class(element, "tr"))
        .count();
    Ok(Dump { rows, trs_total })
}

/// The dump has `rows` rows and `per_row` translations in each, and none
/// outside a row.
fn check_shape(dump: &Dump, rows: usize, per_row: usize) -> Result<(), Failure> {
    if dump.rows.len() != rows {
        return Err(format!("{} rows, not {rows}", dump.rows.len()));
    }
    for (at, row) in dump.rows.iter().enumerate() {
        if row.trs.len() != per_row {
            return Err(format!("row {at} has {} tr, not {per_row}", row.trs.len()));
        }
    }
    if dump.trs_total != rows * per_row {
        return Err(format!(
            "the page has {} tr elements, {} of them in rows",
            dump.trs_total,
            rows * per_row
        ));
    }
    Ok(())
}

// ---- what the translations should look like -------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Ok,
    Held,
}

/// How many translations of each status were judged, and whether each judgement
/// had anything to judge.
#[derive(Debug, PartialEq, Eq)]
struct Judgement {
    ok: usize,
    held: usize,
    ok_judged: bool,
    held_judged: bool,
}

/// The words a translation row is judged against.
struct Words<'a> {
    english: &'a str,
    japanese: &'a str,
    /// The mark of a withheld translation in each language of the page.
    held_marks: &'a [String],
}

fn check_ok_tr(at: usize, tr: &Tr, target: &str, words: &Words<'_>) -> Result<(), Failure> {
    let expected = match target {
        "en" => words.english,
        "ja" => words.japanese,
        other => return Err(format!("row {at}: no sentence is fixed for `{other}`")),
    };
    if tr.text.as_deref() == Some(expected) {
        Ok(())
    } else {
        Err(format!(
            "row {at}: `tr-text` is {:?}, not {expected:?}",
            tr.text
        ))
    }
}

fn check_held_tr(at: usize, tr: &Tr, held_marks: &[String]) -> Result<(), Failure> {
    if tr.text.is_some() {
        return Err(format!(
            "row {at}: a withheld translation shows `tr-text` {:?}",
            tr.text
        ));
    }
    match tr.mark.as_deref() {
        None => return Err(format!("row {at}: a withheld translation has no `tr-mark`")),
        Some(mark) if !held_marks.iter().any(|held| held == mark) => {
            return Err(format!(
                "row {at}: `tr-mark` is {mark:?}, not one of {held_marks:?}"
            ));
        }
        Some(_) => {}
    }
    if tr
        .reason
        .as_deref()
        .is_some_and(|reason| reason.trim().is_empty())
    {
        return Err(format!("row {at}: `tr-reason` is empty"));
    }
    Ok(())
}

/// Judges the translation of each row of the dump against the status and the
/// target language the relay settled it with. A judgement with no row of its
/// status to judge is marked as not made, and is not a pass.
fn judge(dump: &Dump, lines: &[(Status, &str)], words: &Words<'_>) -> Result<Judgement, Failure> {
    check_shape(dump, lines.len(), 1)?;
    let (mut ok, mut held) = (0, 0);
    for (at, (row, (status, target))) in dump.rows.iter().zip(lines).enumerate() {
        let [tr] = row.trs.as_slice() else {
            return Err(format!("row {at} has {} tr", row.trs.len()));
        };
        if tr.arrow.as_deref() != Some(format!("→{target}").as_str()) {
            return Err(format!(
                "row {at}: `tr-arrow` is {:?}, not →{target}",
                tr.arrow
            ));
        }
        match status {
            Status::Ok => {
                check_ok_tr(at, tr, target, words)?;
                ok += 1;
            }
            Status::Held => {
                check_held_tr(at, tr, words.held_marks)?;
                held += 1;
            }
        }
    }
    Ok(Judgement {
        ok,
        held,
        ok_judged: ok > 0,
        held_judged: held > 0,
    })
}

/// The mark of a withheld translation in the strings of each language.
fn held_marks() -> Result<Vec<String>, Failure> {
    ["en", "ja"]
        .into_iter()
        .map(|lang| {
            let path = format!(
                "{}/../hayamimi-ui/data/strings.{lang}.toml",
                env!("CARGO_MANIFEST_DIR")
            );
            let table = toml::from_str::<toml::Table>(&support::read(Path::new(&path))?)
                .map_err(|error| format!("{path}: {error}"))?;
            table
                .get("ui")
                .and_then(|ui| ui.get("tr"))
                .and_then(|tr| tr.get("held"))
                .and_then(toml::Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| format!("{path} has no ui.tr.held"))
        })
        .collect()
}

// ---- the events a run produced --------------------------------------------

fn of_type<'a>(events: &'a [Value], kind: &str) -> Vec<&'a Value> {
    events
        .iter()
        .filter(|event| event.get("type").and_then(Value::as_str) == Some(kind))
        .collect()
}

fn is_settled(event: &Value) -> bool {
    event.get("settled").and_then(Value::as_bool) == Some(true)
}

fn settled_translations(events: &[Value]) -> Vec<&Value> {
    of_type(events, "translation")
        .into_iter()
        .filter(|event| is_settled(event))
        .collect()
}

fn text_field<'a>(event: &'a Value, key: &str) -> Result<&'a str, Failure> {
    event
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("an event has no string `{key}`: {event}"))
}

/// The status a settled translation was published with. A translation with no
/// `status` is `ok`, as the page reads it.
fn status_of(translation: &Value) -> Result<Status, Failure> {
    match translation.get("status").and_then(Value::as_str) {
        Some("ok") => Ok(Status::Ok),
        Some("held") => Ok(Status::Held),
        // DEFAULT: the wire of the page (`hayamimi-ui/src/wire.rs`, `Status`) reads a translation with no `status` as `ok`.
        None => Ok(Status::Ok),
        Some(other) => Err(format!("a translation has the status `{other}`")),
    }
}

/// The finals are four lines that alternate ja, en, ja, en.
fn check_finals_alternate(finals: &[&Value]) -> Result<(), Failure> {
    let langs = finals
        .iter()
        .map(|event| text_field(event, "lang"))
        .collect::<Result<Vec<_>, _>>()?;
    if langs == ["ja", "en", "ja", "en"] {
        Ok(())
    } else {
        Err(format!(
            "the finals are {langs:?}, not ja, en, ja, en: IP-5 is not judged"
        ))
    }
}

/// The settled translations in the order of their lines (`file-1` first).
fn in_line_order<'a>(translations: &[&'a Value]) -> Result<Vec<&'a Value>, Failure> {
    let mut ordered = translations.to_vec();
    for event in &ordered {
        text_field(event, "id")?;
    }
    ordered.sort_by_key(|event| event["id"].as_str());
    Ok(ordered)
}

/// The settled translations are four, for the lines `file-1` to `file-4`, into
/// en, ja, en, ja.
fn check_settled(translations: &[&Value]) -> Result<(), Failure> {
    let mut seen = Vec::new();
    for event in translations {
        seen.push((text_field(event, "id")?, text_field(event, "lang")?));
    }
    let expected = [
        ("file-1", "en"),
        ("file-2", "ja"),
        ("file-3", "en"),
        ("file-4", "ja"),
    ];
    if seen == expected {
        Ok(())
    } else {
        Err(format!(
            "the settled translations are {seen:?}, not {expected:?}"
        ))
    }
}

// ---- the stack and the pages ----------------------------------------------

/// A process that is stopped and waited for when it goes out of scope, its
/// output in a log file.
struct Proc(Child);

impl Proc {
    fn start(binary: &str, args: &[&str], log: &Path) -> Result<Proc, Failure> {
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
            .map(Proc)
            .map_err(|error| format!("cannot start {binary}: {error}"))
    }
}

impl Drop for Proc {
    fn drop(&mut self) {
        // A process that already ended has nothing to kill; the wait reaps it either way.
        let _ = self.0.kill();
        let _ = self.0.wait();
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

/// The ports of the run, which the system chooses and which differ.
struct Ports {
    events: u16,
    ingest: u16,
    llm: u16,
    relay: u16,
    /// The debugging port of each of the three browsers.
    browsers: [u16; 3],
}

fn free_ports() -> Result<Ports, Failure> {
    // All are held at once, so that no two are the same port.
    let held = (0..7)
        .map(|_| TcpListener::bind((Ipv4Addr::LOCALHOST, 0)))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let numbers = held
        .iter()
        .map(|listener| listener.local_addr().map(|address| address.port()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let [events, ingest, llm, relay, first, second, third] = numbers[..] else {
        return Err(String::from("seven ports were not made"));
    };
    Ok(Ports {
        events,
        ingest,
        llm,
        relay,
        browsers: [first, second, third],
    })
}

/// The programs and files of the run, found before anything is started.
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

/// `hayamimi`, the stand-in translation server and the relay in front of it.
struct Stack {
    _fake: Proc,
    _hayamimi: Proc,
    _relay: Proc,
}

impl Stack {
    fn start(tools: &Tools, ports: &Ports, work: &Path) -> Result<Stack, Failure> {
        let llm = ports.llm.to_string();
        let fake = Proc::start(
            FAKE_SERVER,
            &["-m", "translates.gguf", "--port", &llm],
            &work.join("fake.log"),
        )?;
        wait_for_port(ports.llm, START_WAIT)?;
        let (events, ingest) = (ports.events.to_string(), ports.ingest.to_string());
        let hayamimi = Proc::start(
            BINARY,
            &[
                "--input",
                "ws",
                "--ws-port",
                &ingest,
                "--mode",
                "pair",
                "--source",
                "file",
                "--models",
                &tools.models,
                "--serve",
                &events,
            ],
            &work.join("hayamimi.log"),
        )?;
        wait_for_port(ports.events, START_WAIT)?;
        wait_for_port(ports.ingest, START_WAIT)?;
        let source = format!("http://127.0.0.1:{}/events", ports.events);
        let translator = format!("http://127.0.0.1:{}", ports.llm);
        let serve = ports.relay.to_string();
        let relay = Proc::start(
            &tools.relay,
            &[
                "live",
                "--events",
                &source,
                "--llm",
                &translator,
                "--serve",
                &serve,
            ],
            &work.join("relay.log"),
        )?;
        wait_for_port(ports.relay, START_WAIT)?;
        Ok(Stack {
            _fake: fake,
            _hayamimi: hayamimi,
            _relay: relay,
        })
    }
}

/// A client of an `/events` stream. It is connected before the lines are
/// played in, because a client that connects later is not sent what came
/// before.
struct Events {
    stream: TcpStream,
    received: Vec<u8>,
}

impl Events {
    fn open(port: u16) -> Result<Events, Failure> {
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port))
            .map_err(|error| format!("cannot connect to port {port}: {error}"))?;
        stream
            .write_all(b"GET /events HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .map_err(|error| error.to_string())?;
        stream
            .set_read_timeout(Some(Duration::from_millis(500)))
            .map_err(|error| error.to_string())?;
        Ok(Events {
            stream,
            received: Vec::new(),
        })
    }

    /// Reads until `done` holds of the events so far, or `within` passes.
    fn read_until(
        &mut self,
        within: Duration,
        done: impl Fn(&[Value]) -> bool,
    ) -> Result<Vec<Value>, Failure> {
        let deadline = Instant::now() + within;
        let mut chunk = [0_u8; 8192];
        loop {
            let events = data_events(&String::from_utf8_lossy(&self.received));
            if done(&events) {
                return Ok(events);
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "only {} events came within {within:?}",
                    events.len()
                ));
            }
            match self.stream.read(&mut chunk) {
                Ok(0) => return Err(String::from("the stream closed")),
                Ok(got) => self.received.extend_from_slice(&chunk[..got]),
                Err(error)
                    if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                Err(error) => return Err(error.to_string()),
            }
        }
    }
}

/// The first four utterances of `lat-dialog`: a wav cut at the end of the
/// fourth, and its four-row manifest.
struct Clip {
    wav: PathBuf,
    manifest: PathBuf,
}

fn first_four_utterances(work: &Path) -> Result<Clip, Failure> {
    let rows = read_rows(&find_above("testdata/manifests/lat-dialog.json")?)
        .map_err(|error| error.to_string())?;
    let first = rows
        .get(..UTTERANCES)
        .ok_or("lat-dialog has fewer than four utterances")?;
    let end = first
        .last()
        .and_then(|row| row.get("end_sample"))
        .and_then(Value::as_u64)
        .ok_or("the fourth row has no end_sample")?;
    let end = usize::try_from(end).map_err(|error| error.to_string())?;
    let whole =
        read_wav(&find_above("testdata/lat-dialog.wav")?).map_err(|error| error.to_string())?;
    let samples = whole
        .samples
        .get(..end)
        .ok_or("lat-dialog.wav is shorter than its manifest")?;
    let clip = Clip {
        wav: work.join("first-four.wav"),
        manifest: work.join("first-four.json"),
    };
    write_wav(&clip.wav, SAMPLE_RATE, samples).map_err(|error| error.to_string())?;
    write_rows(&clip.manifest, first).map_err(|error| error.to_string())?;
    Ok(clip)
}

/// Plays the clip into `/ingest` with `hayamimi-eval stream`.
fn play(tools: &Tools, clip: &Clip, ingest: u16, work: &Path) -> Result<(), Failure> {
    let status = Command::new(&tools.eval)
        .args(["stream", "--wav", &path_text(&clip.wav)?])
        .args(["--manifest", &path_text(&clip.manifest)?])
        .args(["--repeat", "1", "--port", &ingest.to_string()])
        .args(["--out", &path_text(&work.join("played.jsonl"))?])
        .status()
        .map_err(|error| format!("cannot run hayamimi-eval stream: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("hayamimi-eval stream ended with {status}"))
    }
}

// The browser is driven over the DevTools protocol and not asked for a dump
// of the page as it starts (`--dump-dom`): that dump is taken when the page
// has loaded, before the app has drawn anything, and with a virtual-time
// budget it is never taken while the page holds an `/events` stream open.

/// The key of the websocket handshake; the browser does not look at its value.
const WS_KEY: &str = "dGhlIHNhbXBsZSBub25jZQ==";
/// The mask of the frames this client sends; any four bytes will do.
const WS_MASK: [u8; 4] = [0x12, 0x34, 0x56, 0x78];
/// The longest frame this client reads: a dump of the page is far below it.
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

/// One frame from the server: whether it ends a message, its opcode and its payload.
fn read_frame(reader: &mut impl Read) -> Result<(bool, u8, Vec<u8>), Failure> {
    let mut head = [0_u8; 2];
    reader
        .read_exact(&mut head)
        .map_err(|error| format!("websocket: {error}"))?;
    let length = match head[1] & 0x7f {
        126 => {
            let mut size = [0_u8; 2];
            reader
                .read_exact(&mut size)
                .map_err(|error| format!("websocket: {error}"))?;
            u64::from(u16::from_be_bytes(size))
        }
        127 => {
            let mut size = [0_u8; 8];
            reader
                .read_exact(&mut size)
                .map_err(|error| format!("websocket: {error}"))?;
            u64::from_be_bytes(size)
        }
        small => u64::from(small),
    };
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
    let length = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())?
        })
        .ok_or_else(|| format!("{path}: the answer has no Content-Length"))?;
    let mut body = vec![0_u8; length];
    stream
        .read_exact(&mut body)
        .map_err(|error| format!("{path}: {error}"))?;
    String::from_utf8(body).map_err(|error| error.to_string())
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
        let request = serde_json::json!({"id": id, "method": method, "params": params});
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

    /// The html of the page as it stands now.
    fn html(&mut self) -> Result<String, Failure> {
        let result = self.call(
            "Runtime.evaluate",
            serde_json::json!({
                "expression": "document.documentElement.outerHTML",
                "returnByValue": true
            }),
        )?;
        result
            .get("result")
            .and_then(|value| value.get("value"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| format!("the page has no html: {result}"))
    }
}

/// A browser with one page open, which is closed when this goes out of scope.
struct Page {
    _browser: Proc,
    devtools: DevTools,
}

impl Page {
    fn open(shell: &Path, url: &str, port: u16, work: &Path, name: &str) -> Result<Page, Failure> {
        let debugging = format!("--remote-debugging-port={port}");
        let profile = format!(
            "--user-data-dir={}",
            path_text(&work.join(format!("{name}-profile")))?
        );
        let browser = Proc::start(
            &path_text(shell)?,
            &["--disable-gpu", &debugging, &profile, url],
            &work.join(format!("{name}.log")),
        )?;
        wait_for_port(port, BROWSER_WAIT)?;
        let path = page_socket_path(port)?;
        Ok(Page {
            _browser: browser,
            devtools: DevTools::connect(port, &path)?,
        })
    }

    /// The html of the page once `ready` holds of it, or as it stands when
    /// `within` has passed.
    fn html_when(
        &mut self,
        within: Duration,
        ready: impl Fn(&str) -> bool,
    ) -> Result<String, Failure> {
        let deadline = Instant::now() + within;
        loop {
            let html = self.devtools.html()?;
            if ready(&html) || Instant::now() >= deadline {
                return Ok(html);
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }
}

/// Whether the dashboard has been drawn.
fn is_drawn(html: &str) -> bool {
    read_dump(html).is_ok()
}

/// Whether the dashboard shows a translation under each of its confirmed lines.
fn shows_all_translations(html: &str) -> bool {
    read_dump(html).is_ok_and(|dump| dump.trs_total >= UTTERANCES)
}

/// Three dashboards opened before the first line: with the relay, with none,
/// and with a `javascript:` value that is no relay.
struct Pages {
    with_relay: Page,
    without_relay: Page,
    bad_relay: Page,
}

impl Pages {
    fn open(tools: &Tools, ports: &Ports, work: &Path) -> Result<Pages, Failure> {
        let page = format!("http://127.0.0.1:{}/dashboard", ports.events);
        let relay = format!("?relay=http://127.0.0.1:{}/events", ports.relay);
        let [first, second, third] = ports.browsers;
        let open = |query: &str, port: u16, name: &str| {
            Page::open(&tools.shell, &format!("{page}{query}"), port, work, name)
        };
        Ok(Pages {
            with_relay: open(&relay, first, "with-relay")?,
            without_relay: open("", second, "without-relay")?,
            bad_relay: open("?relay=javascript:x", third, "bad-relay")?,
        })
    }

    /// Waits until every page is drawn, which is after it opened its streams,
    /// and a little longer for them to connect.
    fn wait_until_drawn(&mut self) -> Result<(), Failure> {
        for page in [
            &mut self.with_relay,
            &mut self.without_relay,
            &mut self.bad_relay,
        ] {
            if !is_drawn(&page.html_when(BROWSER_WAIT, is_drawn)?) {
                return Err(format!("a page was not drawn within {BROWSER_WAIT:?}"));
            }
        }
        std::thread::sleep(CONNECT_GRACE);
        Ok(())
    }

    /// The html of the three pages: with the relay once it shows a translation
    /// under every line, then the others a little later.
    fn dumps(mut self) -> Result<[String; 3], Failure> {
        // What the page shows by then is judged, whether or not it is all there.
        self.with_relay
            .html_when(TRANSLATE_WAIT, shows_all_translations)?;
        std::thread::sleep(SHOW_GRACE);
        Ok([
            self.with_relay.devtools.html()?,
            self.without_relay.devtools.html()?,
            self.bad_relay.devtools.html()?,
        ])
    }
}

/// What the run saw.
struct Observed {
    finals: Vec<Value>,
    translations: Vec<Value>,
    with_relay: String,
    without_relay: String,
    bad_relay: String,
}

fn scratch() -> Result<PathBuf, Failure> {
    let work = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("ip05");
    std::fs::create_dir_all(&work).map_err(|error| format!("{}: {error}", work.display()))?;
    Ok(work)
}

fn run_dialog_page() -> Result<Observed, Failure> {
    let tools = Tools::find()?;
    let work = scratch()?;
    std::fs::write(RECORD, "# IP-5 record\n").map_err(|error| format!("{RECORD}: {error}"))?;
    let clip = first_four_utterances(&work)?;
    let ports = free_ports()?;
    let _stack = Stack::start(&tools, &ports, &work)?;
    let mut own = Events::open(ports.events)?;
    let mut relay = Events::open(ports.relay)?;
    let mut pages = Pages::open(&tools, &ports, &work)?;
    pages.wait_until_drawn()?;
    play(&tools, &clip, ports.ingest, &work)?;
    let finals = own.read_until(TRANSLATE_WAIT, |seen| {
        of_type(seen, "final").len() >= UTTERANCES
    })?;
    let translations = relay.read_until(TRANSLATE_WAIT, |seen| {
        settled_translations(seen).len() >= UTTERANCES
    })?;
    let [with_relay, without_relay, bad_relay] = pages.dumps()?;
    Ok(Observed {
        finals: of_type(&finals, "final").into_iter().cloned().collect(),
        translations: settled_translations(&translations)
            .into_iter()
            .cloned()
            .collect(),
        with_relay,
        without_relay,
        bad_relay,
    })
}

/// The run is made once; the four rows that judge it share what it saw.
fn observed() -> Result<&'static Observed, Failure> {
    static RUN: OnceLock<Result<Observed, Failure>> = OnceLock::new();
    RUN.get_or_init(run_dialog_page)
        .as_ref()
        .map_err(Clone::clone)
}

fn yes_no(flag: bool) -> &'static str {
    if flag { "yes" } else { "no (nothing to judge)" }
}

/// Writes the breakdown by status to the record, and which judgements had
/// something to judge.
fn record_judgement(judgement: &Judgement) -> Result<(), Failure> {
    let record = Path::new(RECORD);
    append_line(
        record,
        &format!(
            "status of the four translations: ok {}, held {}",
            judgement.ok, judgement.held
        ),
    )?;
    append_line(
        record,
        &format!(
            "sentences of ok translations judged: {}",
            yes_no(judgement.ok_judged)
        ),
    )?;
    append_line(
        record,
        &format!(
            "marks of held translations judged: {}",
            yes_no(judgement.held_judged)
        ),
    )
}

/// The status and the target language of each settled translation, in the
/// order of the lines.
fn lines_of<'a>(ordered: &[&'a Value]) -> Result<Vec<(Status, &'a str)>, Failure> {
    ordered
        .iter()
        .map(|event| Ok((status_of(event)?, text_field(event, "lang")?)))
        .collect()
}

/// T-IP-05-001
#[test]
#[ignore = "starts the hayamimi binary, the relay, a stand-in translation server and chrome-headless-shell"]
fn ip05_one_page_shows_both_directions() -> TestResult {
    let seen = observed()?;
    check_finals_alternate(&of_type(&seen.finals, "final"))?;
    let dump = read_dump(&seen.with_relay)?;
    check_shape(&dump, UTTERANCES, 1)?;
    Ok(())
}

/// T-IP-05-002
#[test]
#[ignore = "starts the hayamimi binary, the relay, a stand-in translation server and chrome-headless-shell"]
fn each_direction_shows_its_arrow_and_the_translation_the_server_made() -> TestResult {
    let seen = observed()?;
    let ordered = in_line_order(&settled_translations(&seen.translations))?;
    let marks = held_marks()?;
    let words = Words {
        english: ENGLISH,
        japanese: JAPANESE,
        held_marks: &marks,
    };
    let dump = read_dump(&seen.with_relay)?;
    let judgement = judge(&dump, &lines_of(&ordered)?, &words)?;
    record_judgement(&judgement)?;
    let badges: Vec<_> = dump.rows.iter().map(|row| row.badge.as_deref()).collect();
    assert_eq!(badges, [Some("ja"), Some("en"), Some("ja"), Some("en")]);
    Ok(())
}

/// T-IP-05-003
#[test]
#[ignore = "starts the hayamimi binary, the relay, a stand-in translation server and chrome-headless-shell"]
fn a_page_without_relay_in_its_address_shows_no_translation() -> TestResult {
    let seen = observed()?;
    for html in [&seen.without_relay, &seen.bad_relay] {
        check_shape(&read_dump(html)?, UTTERANCES, 0)?;
    }
    Ok(())
}

/// T-IP-05-004
#[test]
#[ignore = "starts the hayamimi binary, the relay, a stand-in translation server and chrome-headless-shell"]
fn the_relay_publishes_four_settled_translations_for_the_four_lines() -> TestResult {
    let seen = observed()?;
    let ordered = in_line_order(&settled_translations(&seen.translations))?;
    check_settled(&ordered)?;
    Ok(())
}

const OK_ROW: &str = r#"<div class="row"><div class="row-text">x</div><div class="tr lang-en"><span class="tr-arrow">→en</span><span class="tr-text">y</span></div></div>"#;
const HELD_ROW: &str = r#"<div class="row"><div class="row-text">z</div><div class="tr lang-ja"><span class="tr-arrow">→ja</span><span class="tr-mark">訳を控えました</span><span class="tr-reason">w</span></div></div>"#;

fn page(rows: &[&str]) -> String {
    format!(r#"<div class="dash">{}</div>"#, rows.concat())
}

fn marks() -> Vec<String> {
    vec![
        String::from("Translation withheld"),
        String::from("訳を控えました"),
    ]
}

/// Judges `rows` of hand-written html, whose ok rows say "y" into English and
/// "j" into Japanese.
fn judge_rows(rows: &[&str], lines: &[(Status, &str)]) -> Result<Judgement, Failure> {
    let marks = marks();
    let words = Words {
        english: "y",
        japanese: "j",
        held_marks: &marks,
    };
    judge(&read_dump(&page(rows))?, lines, &words)
}

/// T-IP-05-005
#[test]
fn the_page_counter_counts_whole_class_names_only() -> TestResult {
    let dump = read_dump(&page(&[
        r#"<div class="row"><div class="row-text">x</div><div class="tr lang-ja"><span class="tr-arrow">→ja</span></div></div>"#,
    ]))?;
    assert_eq!((dump.rows.len(), dump.trs_total), (1, 1));
    let row = dump.rows.first().ok_or("no row")?;
    assert_eq!(row.trs.len(), 1);
    assert_eq!(
        row.trs.first().ok_or("no tr")?.arrow.as_deref(),
        Some("→ja")
    );
    let without = read_dump(r#"<div class="dash"><div class="row-text">x</div></div>"#)?;
    assert_eq!((without.rows.len(), without.trs_total), (0, 0));
    Ok(())
}

/// T-IP-05-006
#[test]
fn a_dump_of_an_unrendered_page_is_an_error_and_not_zero_counts() -> TestResult {
    for html in [
        r#"<div class="x"></div>"#,
        "<html><head></head><body></body></html>",
        "",
    ] {
        let said = read_dump(html);
        assert!(
            matches!(&said, Err(message) if message.contains("dash")),
            "{html:?}: {said:?}"
        );
    }
    read_dump(&page(&[]))?;
    Ok(())
}

/// The dump of a real page: the shell around the app, a comment, a script with
/// a `<` in it, void elements, an attribute with a `>` in it and an entity.
#[test]
fn a_dump_with_the_shell_around_the_page_is_read() -> TestResult {
    let html = format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>hayamimi</title></head><body><!-- x --><div id=\"main\" title=\"a>b\">{}</div><script type=\"module\">if (a < b) {{ init(); }}</script></body></html>",
        page(&[
            r#"<div class="row"><span class="lang-badge ja">ja</span><div class="row-text">a &amp; b<br></div></div>"#
        ])
    );
    let dump = read_dump(&html)?;
    assert_eq!(dump.rows.len(), 1);
    let row = dump.rows.first().ok_or("no row")?;
    assert_eq!(row.badge.as_deref(), Some("ja"));
    Ok(())
}

/// The marks of a withheld translation are the strings of the two languages.
#[test]
fn the_held_marks_are_read_from_both_string_tables() -> TestResult {
    assert_eq!(held_marks()?, marks());
    Ok(())
}

/// A client frame is masked, and its length field has the size the payload needs.
#[test]
fn a_client_frame_is_masked_and_its_length_has_the_size_the_payload_needs() -> TestResult {
    let short = text_frame(b"hi")?;
    assert_eq!(&short[..2], [0x81, 0x82]);
    assert_eq!(&short[2..6], WS_MASK);
    assert_eq!(&short[6..], [b'h' ^ WS_MASK[0], b'i' ^ WS_MASK[1]]);
    let medium = text_frame(&[b'a'; 126])?;
    assert_eq!(&medium[..4], [0x81, 0x80 | 126, 0, 126]);
    assert_eq!(medium.len(), 4 + 4 + 126);
    let long = text_frame(&vec![b'a'; 65_536])?;
    assert_eq!(&long[..10], [0x81, 0x80 | 127, 0, 0, 0, 0, 0, 1, 0, 0]);
    Ok(())
}

/// A server frame is not masked; a payload of 126 bytes or more has a longer
/// length field, and a frame beyond the limit is refused before it is read.
#[test]
fn a_server_frame_is_read_with_its_length_field_and_its_end_flag() -> TestResult {
    let mut small: &[u8] = &[0x81, 0x02, b'o', b'k'];
    assert_eq!(read_frame(&mut small)?, (true, 0x1, b"ok".to_vec()));
    let mut medium = vec![0x01, 126, 0, 200];
    medium.extend([b'x'; 200]);
    let (last, opcode, payload) = read_frame(&mut medium.as_slice())?;
    assert_eq!((last, opcode, payload.len()), (false, 0x1, 200));
    let mut huge: &[u8] = &[0x81, 127, 0xff, 0, 0, 0, 0, 0, 0, 0];
    let said = read_frame(&mut huge);
    assert!(
        matches!(&said, Err(message) if message.contains("bytes")),
        "{said:?}"
    );
    Ok(())
}

/// T-IP-05-017
#[test]
fn a_held_translation_with_a_mark_and_no_text_passes_the_page_check() -> TestResult {
    let judgement = judge_rows(
        &[OK_ROW, HELD_ROW],
        &[(Status::Ok, "en"), (Status::Held, "ja")],
    )?;
    assert_eq!(
        judgement,
        Judgement {
            ok: 1,
            held: 1,
            ok_judged: true,
            held_judged: true
        }
    );
    Ok(())
}

/// T-IP-05-018
#[test]
fn a_held_translation_that_shows_text_or_has_no_mark_fails_the_page_check() {
    let with_text = HELD_ROW.replacen(
        "</div></div>",
        r#"<span class="tr-text">y</span></div></div>"#,
        1,
    );
    let no_mark = HELD_ROW.replace(r#"<span class="tr-mark">訳を控えました</span>"#, "");
    let wrong_mark = HELD_ROW.replace("訳を控えました", "x");
    let cases = [
        (with_text, "tr-text"),
        (no_mark, "no `tr-mark`"),
        (wrong_mark, "`tr-mark` is \"x\""),
    ];
    for (held, reason) in cases {
        let said = judge_rows(
            &[OK_ROW, &held],
            &[(Status::Ok, "en"), (Status::Held, "ja")],
        );
        assert!(
            matches!(&said, Err(message) if message.contains("row 1") && message.contains(reason)),
            "{reason}: {said:?}"
        );
    }
}

/// T-IP-05-019
#[test]
fn a_judgement_with_no_row_to_judge_is_not_issued_and_the_counts_are_recorded() -> TestResult {
    let all_ok = judge_rows(&[OK_ROW, OK_ROW], &[(Status::Ok, "en"), (Status::Ok, "en")])?;
    assert_eq!(
        all_ok,
        Judgement {
            ok: 2,
            held: 0,
            ok_judged: true,
            held_judged: false
        }
    );
    let all_held = judge_rows(
        &[HELD_ROW, HELD_ROW],
        &[(Status::Held, "ja"), (Status::Held, "ja")],
    )?;
    assert_eq!(
        all_held,
        Judgement {
            ok: 0,
            held: 2,
            ok_judged: false,
            held_judged: true
        }
    );
    Ok(())
}

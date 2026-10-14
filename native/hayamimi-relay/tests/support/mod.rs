//! Shared pieces of the acceptance tests: temporary directories, a loopback
//! HTTP server, rate-file builders, a scripted translator, and the tuning
//! sentences.
//!
//! Nothing here reaches the network: every server is bound to 127.0.0.1.

#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use hayamimi_relay::calendar;
use hayamimi_relay::lang::Lang;
use hayamimi_relay::llm::{Completion, Llm, LlmError};
use hayamimi_relay::logging::MemorySink;
use hayamimi_relay::pipeline::{
    LineKind, LineRequest, NoObserver, Observer, Pipeline, PipelineConfig, Stage, Translation,
};

pub mod events;

pub type TestResult = Result<(), Box<dyn std::error::Error>>;

/// The tuning sentences: id, src_lang, src, ref.
pub const CONV20: &str = include_str!("../fixtures/conv20.tsv");
/// The output of the translation model on seven tuning sentences, with the
/// end marker kept: id, three timings, output.
pub const CAT_LOG: &str = include_str!("../fixtures/cat_q8_rows.tsv");

pub const DEFAULT_NOW: &str = "2026-10-13T09:00:00+09:00";

/// Unix seconds of an RFC 3339 timestamp.
pub fn epoch(text: &str) -> Result<i64, String> {
    calendar::parse_rfc3339(text).ok_or_else(|| format!("not an RFC 3339 timestamp: {text}"))
}

// ---- temporary directories -------------------------------------------------

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn base_dir() -> PathBuf {
    if cfg!(windows) {
        std::env::temp_dir().join("hayamimi-tests")
    } else {
        PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
    }
}

/// A directory that is removed when dropped.
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub fn new(label: &str) -> io::Result<TempDir> {
        let number = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = base_dir().join(format!("{label}-{}-{number}", std::process::id()));
        fs::create_dir_all(&path)?;
        Ok(TempDir { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn join(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        // Leftover scratch files are harmless; there is no one to tell.
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Replaces a file by writing a sibling and renaming it over the target.
pub fn write_by_rename(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no file name"))?;
    name.push(".swap");
    let swap = path.with_file_name(name);
    fs::write(&swap, bytes)?;
    fs::rename(&swap, path)
}

/// Overwrites a file where it stands.
pub fn write_in_place(path: &Path, bytes: &[u8]) -> io::Result<()> {
    fs::write(path, bytes)
}

// ---- rate files ------------------------------------------------------------

pub const ECB_URL: &str = "https://www.ecb.europa.eu/stats/eurofxref/eurofxref-daily.xml";

/// A rate file with the given date and two rates.
pub fn rates_json(rate_date: &str, fetched_at: &str, usd: &str, jpy: &str) -> String {
    format!(
        "{{\"source_url\":\"{ECB_URL}\",\"rate_date\":\"{rate_date}\",\"fetched_at\":\"{fetched_at}\",\
         \"base\":\"EUR\",\"rates\":{{\"USD\":{usd},\"JPY\":{jpy}}}}}"
    )
}

/// The ECB values of 2026-10-12: USD 1.1181, JPY 177.33.
pub fn rates_a() -> String {
    rates_json(
        "2026-10-12",
        "2026-10-13T07:30:00+09:00",
        "1.1181",
        "177.33",
    )
}

/// A temporary directory holding `fx_rates.json` with the given content,
/// and the path of that file.
pub fn dir_with_rates(content: &str) -> io::Result<(TempDir, PathBuf)> {
    let dir = TempDir::new("rates")?;
    let path = dir.join("fx_rates.json");
    fs::write(&path, content)?;
    Ok((dir, path))
}

/// Marks a file as last written `secs` seconds ago, as a file fetched
/// earlier in the day would be.
pub fn backdate(path: &Path, secs: u64) -> io::Result<()> {
    let then = std::time::SystemTime::now()
        .checked_sub(Duration::from_secs(secs))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "time before the epoch"))?;
    fs::File::options()
        .write(true)
        .open(path)?
        .set_modified(then)
}

/// An ECB daily document in the published shape.
pub fn ecb_xml(date: &str, usd: &str, jpy: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<gesmes:Envelope xmlns:gesmes=\"http://www.gesmes.org/xml/2002-08-01\" xmlns=\"http://www.ecb.int/vocabulary/2002-08-01/eurofxref\">\n\
    <gesmes:subject>Reference rates</gesmes:subject>\n\
    <gesmes:Sender>\n\
        <gesmes:name>European Central Bank</gesmes:name>\n\
    </gesmes:Sender>\n\
    <Cube>\n\
        <Cube time='{date}'>\n\
            <Cube currency='USD' rate='{usd}'/>\n\
            <Cube currency='JPY' rate='{jpy}'/>\n\
            <Cube currency='GBP' rate='0.87000'/>\n\
            <Cube currency='CAD' rate='1.5600'/>\n\
            <Cube currency='AUD' rate='1.7000'/>\n\
        </Cube>\n\
    </Cube>\n\
</gesmes:Envelope>\n"
    )
}

// ---- a loopback HTTP server --------------------------------------------------

/// What the fake server answers.
pub struct Reply {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn json(body: &str) -> Reply {
        Reply {
            status: 200,
            content_type: "application/json",
            body: body.as_bytes().to_vec(),
        }
    }

    pub fn xml(body: &str) -> Reply {
        Reply {
            status: 200,
            content_type: "application/xml",
            body: body.as_bytes().to_vec(),
        }
    }

    pub fn status(code: u16) -> Reply {
        Reply {
            status: code,
            content_type: "text/plain",
            body: Vec::new(),
        }
    }
}

type Responder = Box<dyn FnMut(usize, &[u8]) -> Reply + Send>;

/// An HTTP server on 127.0.0.1 that records every request it receives.
pub struct FakeServer {
    addr: SocketAddr,
    requests: Arc<Mutex<Vec<Vec<u8>>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn content_length(head: &[u8]) -> usize {
    let text = String::from_utf8_lossy(head).to_lowercase();
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("content-length:")
            && let Ok(length) = value.trim().parse::<usize>()
        {
            return length;
        }
    }
    0
}

/// Reads one request (head and body) from the stream.
fn read_request(stream: &mut TcpStream) -> Vec<u8> {
    let mut raw = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        if let Some(end) = find(&raw, b"\r\n\r\n") {
            let wanted = end + 4 + content_length(&raw[..end]);
            if raw.len() >= wanted {
                return raw;
            }
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return raw,
            Ok(count) => raw.extend_from_slice(&chunk[..count]),
        }
    }
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Status",
    }
}

fn serve_one(stream: &mut TcpStream, requests: &Mutex<Vec<Vec<u8>>>, responder: &mut Responder) {
    if stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .is_err()
    {
        return;
    }
    let raw = read_request(stream);
    let index = match requests.lock() {
        Ok(mut guard) => {
            guard.push(raw.clone());
            guard.len() - 1
        }
        Err(poisoned) => poisoned.into_inner().len(),
    };
    let reply = responder(index, &raw);
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        reply.status,
        reason_phrase(reply.status),
        reply.content_type,
        reply.body.len()
    );
    // A client that hung up has nothing left to read.
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&reply.body);
    let _ = stream.flush();
}

impl FakeServer {
    /// Starts a server whose answer to the n-th request (counting from 0) is
    /// `responder(n, raw_request)`.
    pub fn start(
        responder: impl FnMut(usize, &[u8]) -> Reply + Send + 'static,
    ) -> io::Result<FakeServer> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let addr = listener.local_addr()?;
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (thread_requests, thread_stop) = (Arc::clone(&requests), Arc::clone(&stop));
        let mut responder: Responder = Box::new(responder);
        let thread = std::thread::spawn(move || {
            for stream in listener.incoming() {
                if thread_stop.load(Ordering::SeqCst) {
                    break;
                }
                if let Ok(mut stream) = stream {
                    serve_one(&mut stream, &thread_requests, &mut responder);
                }
            }
        });
        Ok(FakeServer {
            addr,
            requests,
            stop,
            thread: Some(thread),
        })
    }

    /// A server that always answers with the same reply body and status.
    pub fn fixed(status: u16, body: &str) -> io::Result<FakeServer> {
        let body = body.to_owned();
        FakeServer::start(move |_, _| Reply {
            status,
            content_type: "application/xml",
            body: body.as_bytes().to_vec(),
        })
    }

    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }

    /// Every request received so far, as raw bytes.
    pub fn requests(&self) -> Vec<Vec<u8>> {
        match self.requests.lock() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }
}

impl Drop for FakeServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wake the accept loop so that it sees the flag, then wait for it.
        let _ = TcpStream::connect(self.addr);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// A port with nothing listening on it.
pub fn closed_port_url() -> io::Result<String> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    drop(listener);
    Ok(format!("http://{addr}/stats/eurofxref/eurofxref-daily.xml"))
}

/// The bytes after the blank line that ends the request head.
pub fn request_body(raw: &[u8]) -> &[u8] {
    match find(raw, b"\r\n\r\n") {
        Some(end) => &raw[end + 4..],
        None => &[],
    }
}

// ---- a scripted translator and the pipeline around it -----------------------

/// Answers each request with the next queued reply.
pub struct ScriptedLlm {
    replies: Rc<RefCell<VecDeque<String>>>,
}

impl Llm for ScriptedLlm {
    fn complete(&self, _request: &Completion<'_>) -> Result<String, LlmError> {
        match self.replies.borrow_mut().pop_front() {
            Some(reply) => Ok(reply),
            None => Err(LlmError::Transport(String::from("the script is empty"))),
        }
    }
}

/// Remembers what the pipeline did.
#[derive(Default)]
pub struct Recorder {
    pub stages: Vec<Stage>,
    pub verify_inputs: Vec<String>,
}

impl Observer for Recorder {
    fn stage(&mut self, stage: Stage) {
        self.stages.push(stage);
    }

    fn verify_input(&mut self, translation: &str) {
        self.verify_inputs.push(translation.to_owned());
    }
}

/// A pipeline fed by a script: each line is given the edited text directly.
pub struct Harness {
    pipeline: Pipeline<ScriptedLlm>,
    replies: Rc<RefCell<VecDeque<String>>>,
    pub sink: MemorySink,
    next: u32,
}

impl Harness {
    pub fn open(
        rates_path: Option<PathBuf>,
        tables_dir: Option<PathBuf>,
    ) -> Result<Harness, String> {
        let replies = Rc::new(RefCell::new(VecDeque::new()));
        let mut sink = MemorySink::default();
        let config = PipelineConfig {
            rates_path,
            tables_dir,
        };
        let pipeline = Pipeline::open(
            ScriptedLlm {
                replies: Rc::clone(&replies),
            },
            &config,
            &mut sink,
        )?;
        Ok(Harness {
            pipeline,
            replies,
            sink,
            next: 0,
        })
    }

    /// One line whose edit step answers `edited`.
    pub fn line(&mut self, src: &str, lang: Lang, edited: &str, now: i64) -> Translation {
        self.line_observed(src, lang, edited, now, &mut NoObserver)
    }

    pub fn line_observed(
        &mut self,
        src: &str,
        lang: Lang,
        edited: &str,
        now: i64,
        observer: &mut dyn Observer,
    ) -> Translation {
        self.next += 1;
        let id = format!("test-{}", self.next);
        {
            let mut replies = self.replies.borrow_mut();
            replies.push_back(String::from("draft"));
            replies.push_back(edited.to_owned());
        }
        let request = LineRequest {
            id: &id,
            of: LineKind::Final,
            source: "file",
            src_lang: lang,
            text: src,
            now_epoch: now,
        };
        self.pipeline.run(&request, observer, &mut self.sink)
    }

    /// How many `warn` events have been logged.
    pub fn warn_count(&self) -> usize {
        self.sink
            .events
            .iter()
            .filter(|event| event.level == hayamimi_relay::logging::Level::Warn)
            .count()
    }
}

// ---- the tuning sentences ---------------------------------------------------

pub struct Pair {
    pub id: String,
    pub lang: Lang,
    pub src: String,
    pub reference: String,
}

/// The 40 tuning pairs of `fixtures/conv20.tsv`.
pub fn conv20() -> Result<Vec<Pair>, String> {
    let mut pairs = Vec::new();
    for line in CONV20.lines().skip(1) {
        let fields: Vec<&str> = line.trim_end_matches('\r').split('\t').collect();
        let [id, code, src, reference] = fields[..] else {
            return Err(format!("conv20 line has {} fields", fields.len()));
        };
        let lang = match code {
            "ja" => Lang::Ja,
            "en" => Lang::En,
            other => return Err(format!("unknown language {other}")),
        };
        pairs.push(Pair {
            id: id.to_owned(),
            lang,
            src: src.to_owned(),
            reference: reference.to_owned(),
        });
    }
    Ok(pairs)
}

fn is_line_id(field: &str) -> bool {
    let bytes = field.as_bytes();
    bytes.len() == 3
        && (bytes[0] == b'j' || bytes[0] == b'e')
        && bytes[1].is_ascii_digit()
        && bytes[2].is_ascii_digit()
}

/// The model's output per line id, without the trailing end marker.
pub fn cat_outputs() -> BTreeMap<String, String> {
    let mut outputs = BTreeMap::new();
    for line in CAT_LOG.lines() {
        let fields: Vec<&str> = line.trim_end_matches('\r').split('\t').collect();
        if fields.len() >= 5 && is_line_id(fields[0]) {
            let text = fields[4]
                .strip_suffix("</s>")
                .map_or(fields[4], |head| head);
            outputs.insert(fields[0].to_owned(), text.to_owned());
        }
    }
    outputs
}

/// A model output looked up by id.
pub fn cat_output(id: &str) -> Result<String, String> {
    cat_outputs()
        .remove(id)
        .ok_or_else(|| format!("the model log has no line {id}"))
}

//! Pieces for the tests of the relay's event streams: a recognizer that serves
//! `/events`, a translator that answers from the text of the prompt, and a way
//! to run the relay against them and read what it publishes.
//!
//! The recognizer serves the wire form of the recorded events
//! (a close-delimited `HTTP/1.0` response whose body is `data: <json>` and a
//! blank line per event) and the
//! JSON of the recorded events (`testdata/baseline/wire/events.jsonl`, a
//! `final` on line 2 and a `refine` on line 5) with the five items added.
//! Nothing here leaves the loopback address.

#![allow(dead_code)]

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use hayamimi_relay::deliver::{Caption, Destination, Outputs, SendError, Words};
use hayamimi_relay::follow::{Backoff, follow};
use hayamimi_relay::hub;
use hayamimi_relay::lang::Lang;
use hayamimi_relay::llm::{Completion, Llm, LlmError};
use hayamimi_relay::logging::{LogEvent, LogSink, SharedSink};
use hayamimi_relay::partial::PartialTranslation;
use hayamimi_relay::pipeline::{Pipeline, PipelineConfig, sha256_hex};
use hayamimi_relay::relay::{self, Lanes, MakePipeline, RelayConfig};
use hayamimi_relay::strings::Strings;
use hayamimi_relay::verify_dump::VerifyDump;
use serde_json::{Value, json};

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

// ---- a recognizer that serves /events ---------------------------------------

enum Command {
    Event(String),
    Close,
}

/// What happened to a client of the recognizer.
enum Served {
    /// The client is gone; the next one can come.
    ClientDone,
    /// The test has finished with the recognizer.
    Finished,
}

/// A recognizer's `/events`. Events sent while no client is connected wait
/// and go to the next client that connects.
pub struct FakeRecognizer {
    address: SocketAddr,
    commands: Sender<Command>,
    connections: Arc<AtomicUsize>,
}

fn read_request_head(stream: &mut TcpStream) -> Option<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let mut raw = Vec::new();
    let mut chunk = [0_u8; 512];
    while !raw.windows(4).any(|window| window == b"\r\n\r\n") {
        let count = stream.read(&mut chunk).ok().filter(|count| *count > 0)?;
        raw.extend_from_slice(&chunk[..count]);
    }
    Some(())
}

const HEAD: &[u8] = b"HTTP/1.0 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nAccess-Control-Allow-Origin: *\r\n\r\n";

fn serve_client(mut stream: TcpStream, commands: &Receiver<Command>) -> Served {
    if read_request_head(&mut stream).is_none() || stream.write_all(HEAD).is_err() {
        return Served::ClientDone;
    }
    loop {
        match commands.recv() {
            Ok(Command::Event(json)) => {
                let sent = write!(stream, "data: {json}\n\n").and_then(|()| stream.flush());
                if sent.is_err() {
                    return Served::ClientDone;
                }
            }
            Ok(Command::Close) => return Served::ClientDone,
            Err(_) => return Served::Finished,
        }
    }
}

impl FakeRecognizer {
    pub fn start() -> io::Result<FakeRecognizer> {
        FakeRecognizer::on(TcpListener::bind("127.0.0.1:0")?)
    }

    pub fn start_on(port: u16) -> io::Result<FakeRecognizer> {
        FakeRecognizer::on(TcpListener::bind(("127.0.0.1", port))?)
    }

    fn on(listener: TcpListener) -> io::Result<FakeRecognizer> {
        let address = listener.local_addr()?;
        let (commands, queue) = mpsc::channel();
        let connections = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&connections);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                counter.fetch_add(1, Ordering::SeqCst);
                if let Served::Finished = serve_client(stream, &queue) {
                    return;
                }
            }
        });
        Ok(FakeRecognizer {
            address,
            commands,
            connections,
        })
    }

    pub fn url(&self) -> String {
        format!("http://{}/events", self.address)
    }

    /// Sends one event, a JSON text.
    pub fn send(&self, json: &str) {
        // The serving thread ends only when this value is dropped.
        let _ = self.commands.send(Command::Event(json.to_owned()));
    }

    /// Hangs up on the client; the events sent after this go to the next one.
    pub fn hang_up(&self) {
        let _ = self.commands.send(Command::Close);
    }

    /// How many clients have connected so far.
    pub fn connections(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }
}

// ---- lines ------------------------------------------------------------------

/// A line the test sends, and what its translation must be.
#[derive(Clone, Debug)]
pub struct Sent {
    pub source: &'static str,
    pub id: String,
    /// `final` or `refine`.
    pub of: &'static str,
    pub lang: &'static str,
    pub text: String,
    pub at: Instant,
}

const KANA: [char; 5] = ['あ', 'い', 'う', 'え', 'お'];
const VOWELS: [char; 5] = ['a', 'i', 'u', 'e', 'o'];

/// Four digits in base five, written with the five letters of `alphabet`.
/// Every number below 625 gives a different word, and none is a word the
/// verifier reads.
fn word_of(number: usize, alphabet: &[char; 5]) -> String {
    (0..4)
        .rev()
        .map(|place| alphabet[(number / 5_usize.pow(place)) % 5])
        .collect()
}

/// The Japanese the test microphone says: unique for each `number`.
pub fn japanese_text(number: usize) -> String {
    format!("{}。", word_of(number, &KANA))
}

/// The English the test loopback says: unique for each `number`.
pub fn english_text(number: usize) -> String {
    format!("{}.", word_of(number, &VOWELS))
}

/// `text` with each letter of `from` replaced by the letter of `to` at the
/// same place, and the full stop of one language by the other's.
fn transliterate(text: &str, from: &[char; 5], to: &[char; 5], stop: (char, char)) -> String {
    text.chars()
        .map(|c| match from.iter().position(|letter| *letter == c) {
            Some(at) => to[at],
            None if c == stop.0 => stop.1,
            None => c,
        })
        .collect()
}

/// What the test translator answers for Japanese text.
pub fn english_of(japanese: &str) -> String {
    transliterate(japanese, &KANA, &VOWELS, ('。', '.'))
}

/// What the test translator answers for English text.
pub fn japanese_of(english: &str) -> String {
    transliterate(english, &VOWELS, &KANA, ('.', '。'))
}

impl Sent {
    fn new(
        source: &'static str,
        seq: usize,
        of: &'static str,
        lang: &'static str,
        text: String,
    ) -> Sent {
        Sent {
            source,
            id: format!("{source}-{seq}"),
            of,
            lang,
            text,
            at: Instant::now(),
        }
    }

    /// A microphone line; `seq` numbers it and picks its text.
    pub fn mic(seq: usize, of: &'static str) -> Sent {
        Sent::new("mic", seq, of, "ja", japanese_text(seq))
    }

    /// A loopback line; `seq` numbers it and picks its text.
    pub fn loopback(seq: usize, of: &'static str) -> Sent {
        Sent::new("loopback", seq, of, "en", english_text(seq))
    }

    /// The event as the recognizer sends it: the items of a `final` and the
    /// five the relay needs.
    pub fn event(&self) -> String {
        json!({
            "type": self.of,
            "id": self.id,
            "source": self.source,
            "lang": self.lang,
            "text": self.text,
            "speaker": "",
            "audio_s": 1.5,
            "t0_s": 0.5,
            "t1_s": 2.0,
            "end_wall_ms": 1_790_000_000_000_u64,
        })
        .to_string()
    }

    /// The translation the test translator makes of this line.
    pub fn translated(&self) -> String {
        if self.lang == "ja" {
            english_of(&self.text)
        } else {
            japanese_of(&self.text)
        }
    }

    pub fn digest(&self) -> String {
        sha256_hex(&self.text)
    }

    /// Sends this line through `recognizer` and stamps the time.
    pub fn send_through(mut self, recognizer: &FakeRecognizer) -> Sent {
        self.at = Instant::now();
        recognizer.send(&self.event());
        self
    }
}

// ---- a translator -----------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Translate,
    Edit,
}

/// One request to the translator.
pub struct Call<'a> {
    pub stage: Stage,
    /// The source text of the line.
    pub text: &'a str,
}

/// Reads the stage and the source text off a request of the pipeline, and for
/// an edit the draft too. Both stages carry T's prompt as the user message;
/// an edit also has the system message that holds the draft.
pub fn read_prompt<'a>(
    prompt: &'a str,
    system: Option<&'a str>,
) -> Option<(Stage, &'a str, &'a str)> {
    let rest = prompt.strip_prefix("Translate the following ")?;
    let (_, text) = rest.split_once("\n\n ")?;
    let Some(system) = system else {
        return Some((Stage::Translate, text, ""));
    };
    let (_, tail) = system.split_once(" text is: ")?;
    let (draft, _) = tail.split_once("\nIt may contain errors.")?;
    Some((Stage::Edit, text, draft))
}

/// Two requests must be in flight at once before the first of them is
/// answered, as a translator with several slots serving two sources does. The
/// first request gives up after `wait`.
pub struct Meeting {
    state: Mutex<(usize, bool)>,
    changed: Condvar,
    wait: Duration,
}

impl Meeting {
    pub fn new(wait: Duration) -> Meeting {
        Meeting {
            state: Mutex::new((0, false)),
            changed: Condvar::new(),
            wait,
        }
    }

    /// True when a second request arrived in time, or the meeting already took place.
    fn meet(&self) -> bool {
        let mut state = locked(&self.state);
        if state.1 {
            return true;
        }
        state.0 += 1;
        if state.0 >= 2 {
            state.1 = true;
            self.changed.notify_all();
            return true;
        }
        let waited = self.changed.wait_timeout_while(state, self.wait, |s| !s.1);
        let (state, _) = match waited {
            Ok(pair) => pair,
            Err(poisoned) => poisoned.into_inner(),
        };
        state.1
    }
}

type Delay = Box<dyn Fn(&Call<'_>) -> Duration + Send + Sync>;

/// Answers a translation with the text spelled in the other language's
/// letters, and an edit with its draft.
pub struct FakeLlm {
    delay: Delay,
    meeting: Option<Meeting>,
    pub calls: AtomicUsize,
}

impl FakeLlm {
    pub fn new(delay: impl Fn(&Call<'_>) -> Duration + Send + Sync + 'static) -> FakeLlm {
        FakeLlm {
            delay: Box::new(delay),
            meeting: None,
            calls: AtomicUsize::new(0),
        }
    }

    pub fn meeting(mut self, meeting: Meeting) -> FakeLlm {
        self.meeting = Some(meeting);
        self
    }

    fn answer(&self, prompt: &str, system: Option<&str>) -> Result<String, LlmError> {
        let Some((stage, text, draft)) = read_prompt(prompt, system) else {
            return Err(LlmError::Body(String::from(
                "a prompt the test does not know",
            )));
        };
        self.calls.fetch_add(1, Ordering::SeqCst);
        if stage == Stage::Translate
            && let Some(meeting) = &self.meeting
            && !meeting.meet()
        {
            return Err(LlmError::Transport(String::from(
                "no second request arrived",
            )));
        }
        std::thread::sleep((self.delay)(&Call { stage, text }));
        Ok(match stage {
            Stage::Translate if text.chars().any(|c| KANA.contains(&c)) => english_of(text),
            Stage::Translate => japanese_of(text),
            Stage::Edit => draft.to_owned(),
        })
    }
}

/// The translator every lane of a test shares.
pub struct SharedLlm(pub Arc<FakeLlm>);

impl Llm for SharedLlm {
    fn complete(&self, request: &Completion<'_>) -> Result<String, LlmError> {
        self.0.answer(request.prompt, request.system)
    }
}

// ---- running the relay -------------------------------------------------------

/// Events logged into memory, readable from the test.
#[derive(Clone, Default)]
pub struct Logs(Arc<Mutex<Vec<LogEvent>>>);

impl LogSink for Logs {
    fn emit(&mut self, event: LogEvent) {
        locked(&self.0).push(event);
    }
}

impl Logs {
    pub fn named(&self, name: &str) -> Vec<LogEvent> {
        locked(&self.0)
            .iter()
            .filter(|event| event.event == name)
            .cloned()
            .collect()
    }

    /// Every event as the line the log would hold.
    pub fn lines(&self) -> Vec<String> {
        locked(&self.0).iter().map(LogEvent::to_json_line).collect()
    }
}

/// Whether `value` is a translation event, and whether it is the settled one
/// of its line (the other is a draft).
fn is_translation(value: &Value, settled: bool) -> bool {
    value["type"] == "translation" && value["settled"] == settled
}

/// What the relay published, with the time each event arrived.
#[derive(Clone, Default)]
pub struct Collected(Arc<Mutex<Vec<(Instant, Value)>>>);

impl Collected {
    pub fn snapshot(&self) -> Vec<(Instant, Value)> {
        locked(&self.0).clone()
    }

    fn settled_count(&self) -> usize {
        locked(&self.0)
            .iter()
            .filter(|(_, value)| is_translation(value, true))
            .count()
    }

    /// Waits for `count` settled translations, or for `timeout`, and returns
    /// everything that arrived, drafts included.
    pub fn wait_for(&self, count: usize, timeout: Duration) -> Vec<(Instant, Value)> {
        let deadline = Instant::now() + timeout;
        while self.settled_count() < count && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        self.snapshot()
    }

    /// Waits until `done` is true of what arrived, or for `timeout`.
    pub fn wait_until(
        &self,
        done: impl Fn(&[(Instant, Value)]) -> bool,
        timeout: Duration,
    ) -> Vec<(Instant, Value)> {
        let deadline = Instant::now() + timeout;
        while !done(&self.snapshot()) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        self.snapshot()
    }
}

/// Sets a flag when the follower has opened its stream.
struct OpenedFlag(Arc<AtomicBool>);

impl LogSink for OpenedFlag {
    fn emit(&mut self, event: LogEvent) {
        if event.event == "events.subscribe.opened" {
            self.0.store(true, Ordering::SeqCst);
        }
    }
}

/// Follows the relay's own stream from a thread of its own. Returns once the
/// stream is open, so that nothing published afterwards is missed.
pub fn collect_from(address: SocketAddr) -> Result<Collected, io::Error> {
    let collected = Collected::default();
    let (items, opened) = (collected.clone(), Arc::new(AtomicBool::new(false)));
    let flag = Arc::clone(&opened);
    let url = format!("http://{address}/events");
    std::thread::spawn(move || {
        let backoff = Backoff {
            initial: Duration::from_millis(10),
            max: Duration::from_millis(50),
        };
        let mut on_event = |data: &str, _: &mut dyn LogSink| {
            if let Ok(value) = serde_json::from_str::<Value>(data) {
                locked(&items.0).push((Instant::now(), value));
            }
        };
        match follow((0, &url), backoff, &mut OpenedFlag(flag), &mut on_event) {}
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while !opened.load(Ordering::SeqCst) {
        if Instant::now() > deadline {
            return Err(io::Error::other("the relay's stream did not open"));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Ok(collected)
}

/// A relay at work, and the two ways to see what it does.
pub struct Running {
    pub translations: Collected,
    pub logs: Logs,
}

/// Starts a relay that follows `events`, translates with `llm` in `lanes`, and
/// whose stream is already being collected. Nothing leaves it but the stream.
pub fn run_relay(
    events: Vec<String>,
    lanes: Lanes,
    llm: Arc<FakeLlm>,
) -> Result<Running, io::Error> {
    let make: MakePipeline<SharedLlm> = Arc::new(move |sink: &mut dyn LogSink| {
        Pipeline::open(
            SharedLlm(Arc::clone(&llm)),
            &PipelineConfig::default(),
            sink,
        )
    });
    run_relay_with(events, lanes, make, outputs(Vec::new(), None)?)
}

/// Starts a relay like [`run_relay`] with the translator and the outputs the
/// test gives.
pub fn run_relay_with<L: Llm + 'static>(
    events: Vec<String>,
    lanes: Lanes,
    make: MakePipeline<L>,
    outputs: Outputs,
) -> Result<Running, io::Error> {
    run_relay_partials(events, lanes, make, outputs, PartialTranslation::Off)
}

/// Starts a relay like [`run_relay_with`], translating the text in progress
/// or not as `partials` says.
pub fn run_relay_partials<L: Llm + 'static>(
    events: Vec<String>,
    lanes: Lanes,
    make: MakePipeline<L>,
    outputs: Outputs,
    partials: PartialTranslation,
) -> Result<Running, io::Error> {
    run_relay_logging(Logs::default(), events, lanes, make, outputs, partials)
}

/// Starts a relay like [`run_relay_partials`] whose events go to `logs`, so
/// that a destination built with a clone of it logs to the same place.
pub fn run_relay_logging<L: Llm + 'static>(
    logs: Logs,
    events: Vec<String>,
    lanes: Lanes,
    make: MakePipeline<L>,
    outputs: Outputs,
    partials: PartialTranslation,
) -> Result<Running, io::Error> {
    run_relay_dumping(logs, events, lanes, make, outputs, partials, None)
}

/// Starts a relay like [`run_relay_logging`] that also writes the developer's
/// record of what V is handed to `verify_dump`, when one is given.
pub fn run_relay_dumping<L: Llm + 'static>(
    logs: Logs,
    events: Vec<String>,
    lanes: Lanes,
    make: MakePipeline<L>,
    outputs: Outputs,
    partials: PartialTranslation,
    verify_dump: Option<VerifyDump>,
) -> Result<Running, io::Error> {
    let config = RelayConfig {
        events,
        lanes,
        backoff: Backoff {
            initial: Duration::from_millis(20),
            max: Duration::from_millis(100),
        },
        started_epoch: 1_790_000_000,
        translate: None,
        verify_dump,
    };
    let sink = SharedSink::new(logs.clone());
    let address = relay::start_partials(hub::bind(0)?, config, make, sink, outputs, partials)?;
    Ok(Running {
        translations: collect_from(address)?,
        logs,
    })
}

/// A port nothing listens on.
pub fn unused_port() -> io::Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

/// The settled translations among the events of a stream, with their arrival
/// times.
pub fn translations_of(got: &[(Instant, Value)]) -> Vec<&(Instant, Value)> {
    got.iter()
        .filter(|(_, value)| is_translation(value, true))
        .collect()
}

/// The drafts among the events of a stream: translations not yet settled.
pub fn drafts_of(got: &[(Instant, Value)]) -> Vec<&(Instant, Value)> {
    got.iter()
        .filter(|(_, value)| is_translation(value, false))
        .collect()
}

// ---- outputs -----------------------------------------------------------------

/// A written-to buffer the test can read back.
#[derive(Clone, Default)]
pub struct Buffer(Arc<Mutex<Vec<u8>>>);

impl Write for Buffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        locked(&self.0).extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Buffer {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&locked(&self.0)).into_owned()
    }
}

/// A destination that records every caption it is sent, and can be told to be
/// down.
#[derive(Clone)]
pub struct Recording {
    sent: Arc<Mutex<Vec<Caption>>>,
    up: Arc<AtomicBool>,
}

impl Recording {
    pub fn new(up: bool) -> Recording {
        Recording {
            sent: Arc::default(),
            up: Arc::new(AtomicBool::new(up)),
        }
    }

    pub fn set_up(&self, up: bool) {
        self.up.store(up, Ordering::SeqCst);
    }

    /// The captions it took.
    pub fn sent(&self) -> Vec<Caption> {
        locked(&self.sent).clone()
    }
}

impl Destination for Recording {
    fn name(&self) -> &'static str {
        "recording"
    }

    fn send(&mut self, caption: &Caption) -> Result<(), SendError> {
        if !self.up.load(Ordering::SeqCst) {
            return Err(SendError::Unreachable);
        }
        locked(&self.sent).push(caption.clone());
        Ok(())
    }
}

/// The outputs of a run: the destinations, the subtitle file when there is
/// one, and the English words.
pub fn outputs(
    destinations: Vec<Box<dyn Destination>>,
    srt: Option<Box<dyn Write + Send>>,
) -> Result<Outputs, io::Error> {
    let strings = Strings::embedded(Lang::En).map_err(io::Error::other)?;
    Ok(Outputs {
        destinations,
        srt,
        words: Words::from_strings(&strings),
    })
}

/// The one translation of `line` among `translations`.
pub fn translation_of<'a>(line: &Sent, translations: &[&'a Value]) -> Result<&'a Value, String> {
    let own: Vec<&Value> = translations
        .iter()
        .copied()
        .filter(|found| found["id"] == line.id.as_str() && found["of"] == line.of)
        .collect();
    match own[..] {
        [one] => Ok(one),
        _ => Err(format!(
            "{} ({}) has {} translations",
            line.id,
            line.of,
            own.len()
        )),
    }
}

/// A translation that was made from `line`, and says so.
pub fn check_made_from(line: &Sent, translation: &Value) -> Result<(), String> {
    let wanted = [
        ("status", String::from("ok")),
        ("source", line.source.to_owned()),
        ("src_sha256", line.digest()),
        ("text", line.translated()),
    ];
    for (key, want) in wanted {
        if translation[key] != want.as_str() {
            return Err(format!(
                "{} ({}): {key} is {}",
                line.id, line.of, translation[key]
            ));
        }
    }
    Ok(())
}

/// Every line sent has exactly one translation, made from it, and nothing
/// else was published.
pub fn check_delivery(sent: &[Sent], arrived: &[(Instant, Value)]) -> Result<(), String> {
    let translations: Vec<&Value> = translations_of(arrived)
        .into_iter()
        .map(|(_, value)| value)
        .collect();
    for line in sent {
        check_made_from(line, translation_of(line, &translations)?)?;
    }
    if translations.len() != sent.len() {
        return Err(format!(
            "{} translations for {} lines",
            translations.len(),
            sent.len()
        ));
    }
    Ok(())
}

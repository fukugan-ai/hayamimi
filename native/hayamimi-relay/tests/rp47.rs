//! RP-47: the T of a new `final` line is not queued behind the E of the line
//! before it, and no more requests are made at once than the server has
//! slots.
//!
//! A fake llama-server answers each request on a thread of its own, E a full
//! second late, and records when each request arrived and when it was
//! answered. The relay runs against it with real HTTP, so the times are those
//! of the requests the server sees.

mod support;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{JoinHandle, sleep};
use std::time::{Duration, Instant};

use hayamimi_relay::follow::Backoff;
use hayamimi_relay::hub;
use hayamimi_relay::llm::{Completion, HttpLlm, Llm};
use hayamimi_relay::logging::{LogSink, SharedSink};
use hayamimi_relay::pipeline::{Pipeline, PipelineConfig};
use hayamimi_relay::relay::{self, Lanes, MakePipeline, RelayConfig};
use hayamimi_relay::slots::LLAMA_SLOTS;
use serde_json::Value;
use support::TestResult;
use support::events::{
    FakeLlm, FakeRecognizer, Logs, Sent, SharedLlm, Stage, check_delivery, collect_from, drafts_of,
    japanese_text, outputs, run_relay_with, translations_of,
};

/// How late the fake server answers an edit.
const E_TAKES: Duration = Duration::from_millis(1000);
/// How late it answers a translation, where the test needs the translations of
/// different lines to be out together.
const T_TAKES: Duration = Duration::from_millis(300);
/// How late it answers a request that names its languages (a line into zh).
const CODED_TAKES: Duration = Duration::from_millis(300);
/// How long after the first line the second one arrives.
const SECOND_LINE_AFTER: Duration = Duration::from_millis(200);
/// The second line's T must be asked at least this long before the first
/// line's E is answered.
const AHEAD_BY: Duration = Duration::from_millis(400);
const ALL_ARRIVE_WITHIN: Duration = Duration::from_secs(20);
/// A request is not given up on while the machine is busy.
const REQUEST_LIMIT: Duration = Duration::from_secs(10);
/// What the fake server says for a line into zh.
const ZH: &str = "明天会下雨。";

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

// ---- a llama-server that serves requests at the same time -------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Translate,
    Edit,
    /// A request that names the languages (a line into zh).
    Coded,
}

/// One request the server took.
#[derive(Clone, Debug)]
struct Seen {
    kind: Kind,
    prompt: String,
    arrived: Instant,
    answered: Instant,
}

struct Shared {
    llm: SharedLlm,
    seen: Mutex<Vec<Seen>>,
    in_flight: AtomicUsize,
    peak: AtomicUsize,
}

struct SlowServer {
    address: SocketAddr,
    shared: Arc<Shared>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

/// The body of the request on `stream`, read to its end.
fn read_body(stream: &mut TcpStream) -> Option<Vec<u8>> {
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let mut reader = BufReader::new(stream);
    let mut length = 0_usize;
    loop {
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .ok()
            .filter(|count| *count > 0)?;
        let lowered = line.trim().to_lowercase();
        if lowered.is_empty() {
            break;
        }
        if let Some(value) = lowered.strip_prefix("content-length:") {
            length = value.trim().parse().ok()?;
        }
    }
    let mut body = vec![0_u8; length];
    reader.read_exact(&mut body).ok()?;
    Some(body)
}

fn kind_of(prompt: &str, coded: bool) -> Kind {
    if coded {
        Kind::Coded
    } else if prompt.starts_with("Translate the following ") {
        Kind::Translate
    } else {
        Kind::Edit
    }
}

/// What the server answers to the request, after the time that kind of
/// request takes.
fn answer(shared: &Shared, kind: Kind, prompt: &str) -> Option<String> {
    if kind == Kind::Coded {
        sleep(CODED_TAKES);
        return Some(ZH.to_owned());
    }
    shared.llm.complete(&Completion::new(prompt, 256)).ok()
}

fn serve(mut stream: TcpStream, shared: &Shared) {
    let Some(body) = read_body(&mut stream) else {
        return;
    };
    let Ok(request) = serde_json::from_slice::<Value>(&body) else {
        return;
    };
    let Some(prompt) = request["messages"][0]["content"].as_str() else {
        return;
    };
    let kind = kind_of(prompt, request.get("chat_template_kwargs").is_some());
    let arrived = Instant::now();
    let now = shared.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
    shared.peak.fetch_max(now, Ordering::SeqCst);
    let said = answer(shared, kind, prompt);
    shared.in_flight.fetch_sub(1, Ordering::SeqCst);
    locked(&shared.seen).push(Seen {
        kind,
        prompt: prompt.to_owned(),
        arrived,
        answered: Instant::now(),
    });
    let (status, text) = match said {
        Some(text) => ("200 OK", support::chat_reply(&text)),
        None => ("500 Internal Server Error", String::new()),
    };
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        text.len()
    );
    // A client that hung up has nothing left to read.
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(text.as_bytes());
    let _ = stream.flush();
}

impl SlowServer {
    /// A server whose edits take [`E_TAKES`] and whose translations take
    /// `t_takes`.
    fn start(t_takes: Duration) -> std::io::Result<SlowServer> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let llm = Arc::new(FakeLlm::new(move |call| match call.stage {
            Stage::Edit => E_TAKES,
            Stage::Translate => t_takes,
        }));
        let shared = Arc::new(Shared {
            llm: SharedLlm(llm),
            seen: Mutex::new(Vec::new()),
            in_flight: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
        });
        let stop = Arc::new(AtomicBool::new(false));
        let (thread_shared, thread_stop) = (Arc::clone(&shared), Arc::clone(&stop));
        let thread = std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                if thread_stop.load(Ordering::SeqCst) {
                    break;
                }
                let shared = Arc::clone(&thread_shared);
                std::thread::spawn(move || serve(stream, &shared));
            }
        });
        Ok(SlowServer {
            address,
            shared,
            stop,
            thread: Some(thread),
        })
    }

    fn base_url(&self) -> String {
        format!("http://{}", self.address)
    }

    fn seen(&self) -> Vec<Seen> {
        locked(&self.shared.seen).clone()
    }

    /// The most requests that were being answered at the same time.
    fn peak(&self) -> usize {
        self.shared.peak.load(Ordering::SeqCst)
    }
}

impl Drop for SlowServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wake the accepting thread so that it sees the flag, then wait for it.
        let _ = TcpStream::connect(self.address);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

// ---- running the relay against it -----------------------------------------------

fn make_over(base: String) -> MakePipeline<HttpLlm> {
    Arc::new(move |sink: &mut dyn LogSink| {
        Pipeline::open(
            HttpLlm::with_timeout(&base, REQUEST_LIMIT),
            &PipelineConfig::default(),
            sink,
        )
    })
}

/// The request of `kind` that carried the text of `line`.
fn request_of<'a>(seen: &'a [Seen], kind: Kind, line: &Sent) -> Result<&'a Seen, String> {
    seen.iter()
        .find(|request| request.kind == kind && request.prompt.contains(&line.text))
        .ok_or_else(|| format!("{kind:?} of {} was never asked", line.id))
}

type Arrived = Vec<(Instant, Value)>;

/// The events of a stream that `pick` keeps (`drafts_of` or `translations_of`).
type Pick = for<'a> fn(&'a [(Instant, Value)]) -> Vec<&'a (Instant, Value)>;

/// When the event of `line` that `pick` keeps arrived.
fn arrival(got: &Arrived, pick: Pick, line: &Sent) -> Result<Instant, String> {
    pick(got)
        .into_iter()
        .find(|(_, value)| value["id"] == line.id.as_str())
        .map(|(at, _)| *at)
        .ok_or_else(|| format!("{} never arrived", line.id))
}

/// A line of the file source, which the microphone's and the loopback's
/// texts do not collide with.
fn file_line(number: usize) -> Sent {
    Sent {
        source: "file",
        id: format!("file-{number}"),
        of: "final",
        lang: "ja",
        text: japanese_text(number),
        at: Instant::now(),
    }
}

// ---- the acceptance ---------------------------------------------------------------

/// The acceptance test of the row: while the first line's E is out, the
/// second line's T goes out, and the second line's draft is on the screen
/// before the first line is settled.
#[test]
fn rp47_next_draft_is_not_queued_behind_the_previous_edit() -> TestResult {
    let server = SlowServer::start(Duration::ZERO)?;
    let mic = FakeRecognizer::start()?;
    let running = run_relay_with(
        vec![mic.url()],
        Lanes::PerSource,
        make_over(server.base_url()),
        outputs(Vec::new(), None)?,
    )?;
    let first = Sent::mic(1, "final").send_through(&mic);
    sleep(SECOND_LINE_AFTER);
    let second = Sent::mic(2, "final").send_through(&mic);
    let arrived = running.translations.wait_for(2, ALL_ARRIVE_WITHIN);

    let seen = server.seen();
    let first_edit = request_of(&seen, Kind::Edit, &first)?;
    let second_translate = request_of(&seen, Kind::Translate, &second)?;
    assert!(
        first_edit.arrived < second_translate.arrived,
        "the first edit was not out when the second line came"
    );
    assert!(
        second_translate.arrived + AHEAD_BY <= first_edit.answered,
        "the second T was asked {:?} before the first E was answered",
        first_edit
            .answered
            .saturating_duration_since(second_translate.arrived)
    );
    let second_draft = arrival(&arrived, drafts_of, &second)?;
    let first_settled = arrival(&arrived, translations_of, &first)?;
    assert!(
        second_draft < first_settled,
        "the draft waited for the edit"
    );
    assert!(
        (2..=LLAMA_SLOTS).contains(&server.peak()),
        "peak of requests at once: {}",
        server.peak()
    );
    Ok(())
}

/// The lines still come back one translation each, made from their own text,
/// and the first line is settled before the second.
#[test]
fn rp47_every_line_is_settled_once_and_in_the_order_it_came() -> TestResult {
    let server = SlowServer::start(Duration::ZERO)?;
    let mic = FakeRecognizer::start()?;
    let running = run_relay_with(
        vec![mic.url()],
        Lanes::PerSource,
        make_over(server.base_url()),
        outputs(Vec::new(), None)?,
    )?;
    let first = Sent::mic(1, "final").send_through(&mic);
    sleep(SECOND_LINE_AFTER);
    let second = Sent::mic(2, "final").send_through(&mic);
    let arrived = running.translations.wait_for(2, ALL_ARRIVE_WITHIN);

    check_delivery(&[first.clone(), second.clone()], &arrived)?;
    let first_settled = arrival(&arrived, translations_of, &first)?;
    let second_settled = arrival(&arrived, translations_of, &second)?;
    assert!(first_settled < second_settled);
    Ok(())
}

/// No more requests are made at once than the server has slots: three
/// sources, each with the T of its second line and the E of its first line
/// out together, would otherwise ask for six at once.
#[test]
fn rp47_no_more_requests_are_made_at_once_than_the_server_has_slots() -> TestResult {
    let server = SlowServer::start(T_TAKES)?;
    let (mic, loopback, file) = (
        FakeRecognizer::start()?,
        FakeRecognizer::start()?,
        FakeRecognizer::start()?,
    );
    let running = run_relay_with(
        vec![mic.url(), loopback.url(), file.url()],
        Lanes::PerSource,
        make_over(server.base_url()),
        outputs(Vec::new(), None)?,
    )?;
    let mut sent = vec![
        Sent::mic(1, "final").send_through(&mic),
        Sent::loopback(1, "final").send_through(&loopback),
        file_line(11).send_through(&file),
    ];
    sleep(Duration::from_millis(100));
    sent.extend([
        Sent::mic(2, "final").send_through(&mic),
        Sent::loopback(2, "final").send_through(&loopback),
        file_line(12).send_through(&file),
    ]);
    let arrived = running.translations.wait_for(sent.len(), ALL_ARRIVE_WITHIN);

    check_delivery(&sent, &arrived)?;
    // Six are wanted at once: the T of the second line and the E of the first,
    // for each of the three sources.
    assert_eq!(
        server.peak(),
        LLAMA_SLOTS.min(6),
        "peak of requests at once"
    );
    Ok(())
}

/// The control: a line into zh has no E, so its lane stays one thread, and
/// the second line's T is asked after the first line's T was answered.
#[test]
fn rp47_a_language_without_an_edit_asks_its_translations_one_after_the_other() -> TestResult {
    let server = SlowServer::start(Duration::ZERO)?;
    let mic = FakeRecognizer::start()?;
    let config = RelayConfig {
        events: vec![mic.url()],
        lanes: Lanes::PerSource,
        backoff: Backoff {
            initial: Duration::from_millis(20),
            max: Duration::from_millis(100),
        },
        started_epoch: 1_790_000_000,
        translate: Some(vec![String::from("zh")]),
        verify_dump: None,
    };
    let address = relay::start_with(
        hub::bind(0)?,
        config,
        make_over(server.base_url()),
        SharedSink::new(Logs::default()),
        outputs(Vec::new(), None)?,
    )?;
    let published = collect_from(address)?;
    let first = Sent::mic(1, "final").send_through(&mic);
    sleep(Duration::from_millis(100));
    let second = Sent::mic(2, "final").send_through(&mic);
    let arrived = published.wait_for(2, ALL_ARRIVE_WITHIN);

    let seen = server.seen();
    assert_eq!(seen.len(), 2, "{seen:?}");
    assert!(seen.iter().all(|request| request.kind == Kind::Coded));
    let [one, two] = &seen[..] else {
        return Err("not two requests".into());
    };
    assert!(
        one.answered <= two.arrived,
        "the second T was asked while the first was out"
    );
    for line in [&first, &second] {
        let found = translations_of(&arrived)
            .into_iter()
            .map(|(_, value)| value)
            .find(|value| value["id"] == line.id.as_str());
        let Some(value) = found else {
            return Err(format!("{} was never translated", line.id).into());
        };
        assert_eq!(value["status"], "ok", "{value}");
        assert_eq!(value["lang"], "zh");
    }
    Ok(())
}

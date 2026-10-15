//! RP-69: a `final` that only closes the last text in progress takes over the
//! translation of that text.
//!
//! When the text of a `final` is the last `partial` of its utterance, or that
//! text with one `。` added, the answer T gave (or is still giving) for the
//! `partial` is the answer T would give the line. The line asks T nothing,
//! V judges the answer against the `final`'s own text, and E follows at once.
//! A `final` of another text, another `id` or another source asks T as before.
//!
//! The recognizer and the translator are fakes (`tests/support/events.rs`).
//! The translator answers a T request after 300 ms and an E request after
//! 100 ms, records every request, and gives a request up at once when its
//! answer stops being wanted.

mod support;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::sleep;
use std::time::{Duration, Instant};

use hayamimi_relay::llm::{Completion, Llm, LlmError};
use hayamimi_relay::logging::{LogEvent, LogSink};
use hayamimi_relay::partial::PartialTranslation;
use hayamimi_relay::pipeline::{Pipeline, PipelineConfig, sha256_hex};
use hayamimi_relay::relay::{Lanes, MakePipeline};
use hayamimi_relay::verify_dump::{SourceLog, VerifyDump};
use serde_json::{Value, json};
use support::events::{
    FakeRecognizer, Logs, Running, Sent, Stage, english_of, japanese_of, japanese_text, outputs,
    read_prompt, run_relay_dumping,
};
use support::{TempDir, TestResult};

const WAIT: Duration = Duration::from_secs(10);
const T_DELAY: Duration = Duration::from_millis(300);
const E_DELAY: Duration = Duration::from_millis(100);
/// How soon after the `final` the reused answer is shown, or E is asked.
const PROMPT: Duration = Duration::from_millis(50);

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Polls `done` until it is true or `timeout` passes; whether it became true.
fn until(timeout: Duration, mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while !done() {
        if Instant::now() >= deadline {
            return false;
        }
        sleep(Duration::from_millis(5));
    }
    true
}

// ---- the translator ------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    T,
    E,
}

/// One request the translator received.
#[derive(Clone, Debug)]
struct Asked {
    kind: Kind,
    /// The source text of the line the request is for.
    text: String,
    /// For E, T's translation it is handed; empty for T.
    draft: String,
    at: Instant,
    done: Option<Instant>,
    aborted: bool,
}

/// A translator that records what it is asked.
struct Translator {
    asked: Mutex<Vec<Asked>>,
    running: AtomicUsize,
    peak: AtomicUsize,
    t_delay: Box<dyn Fn(&str) -> Duration + Send + Sync>,
    /// T's reply for a source text: the translation, or an HTTP status.
    t_reply: Box<TReply>,
    /// E's reply for a source text and the draft.
    e_reply: Box<EReply>,
}

type TReply = dyn Fn(&str) -> Result<String, u16> + Send + Sync;
type EReply = dyn Fn(&str, &str) -> String + Send + Sync;

/// What the plain test translator answers: the text spelled in the other
/// language's letters.
fn plain_translation(text: &str) -> String {
    if text
        .chars()
        .any(|c| matches!(c, 'あ' | 'い' | 'う' | 'え' | 'お'))
    {
        english_of(text)
    } else {
        japanese_of(text)
    }
}

impl Translator {
    fn new() -> Translator {
        Translator {
            asked: Mutex::new(Vec::new()),
            running: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
            t_delay: Box::new(|_| T_DELAY),
            t_reply: Box::new(|text| Ok(plain_translation(text))),
            e_reply: Box::new(|_, draft| draft.to_owned()),
        }
    }

    fn t_reply(
        mut self,
        reply: impl Fn(&str) -> Result<String, u16> + Send + Sync + 'static,
    ) -> Self {
        self.t_reply = Box::new(reply);
        self
    }

    fn e_reply(mut self, reply: impl Fn(&str, &str) -> String + Send + Sync + 'static) -> Self {
        self.e_reply = Box::new(reply);
        self
    }

    fn begin(&self, kind: Kind, text: &str, draft: &str) -> usize {
        let now_running = self.running.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(now_running, Ordering::SeqCst);
        let mut asked = locked(&self.asked);
        asked.push(Asked {
            kind,
            text: text.to_owned(),
            draft: draft.to_owned(),
            at: Instant::now(),
            done: None,
            aborted: false,
        });
        asked.len() - 1
    }

    fn end(&self, index: usize, aborted: bool) {
        self.running.fetch_sub(1, Ordering::SeqCst);
        if let Some(row) = locked(&self.asked).get_mut(index) {
            row.done = Some(Instant::now());
            row.aborted = aborted;
        }
    }

    /// Sleeps `delay`, or returns `Aborted` at once when the answer stops being wanted.
    fn wait(request: &Completion<'_>, delay: Duration) -> Result<(), LlmError> {
        let until = Instant::now() + delay;
        while Instant::now() < until {
            if request.wanted.is_some_and(|wanted| !wanted.still()) {
                return Err(LlmError::Aborted);
            }
            sleep(Duration::from_millis(5));
        }
        Ok(())
    }

    fn reply(&self, kind: Kind, text: &str, draft: &str) -> Result<String, LlmError> {
        match kind {
            Kind::T => (self.t_reply)(text).map_err(LlmError::Status),
            Kind::E => Ok((self.e_reply)(text, draft)),
        }
    }

    fn serve(&self, request: &Completion<'_>) -> Result<String, LlmError> {
        let Some((stage, text, draft)) = read_prompt(request.prompt) else {
            return Err(LlmError::Body(String::from(
                "a prompt the test does not know",
            )));
        };
        let kind = match stage {
            Stage::Translate => Kind::T,
            Stage::Edit => Kind::E,
        };
        let index = self.begin(kind, text, draft);
        let delay = match kind {
            Kind::T => (self.t_delay)(text),
            Kind::E => E_DELAY,
        };
        let answered =
            Translator::wait(request, delay).and_then(|()| self.reply(kind, text, draft));
        self.end(index, matches!(answered, Err(LlmError::Aborted)));
        answered
    }

    fn rows(&self) -> Vec<Asked> {
        locked(&self.asked).clone()
    }

    /// How many requests of `kind` were made for the source text `text`.
    fn count(&self, kind: Kind, text: &str) -> usize {
        self.rows()
            .iter()
            .filter(|row| row.kind == kind && row.text == text)
            .count()
    }

    /// The first request of `kind` for `text`.
    fn first(&self, kind: Kind, text: &str) -> Option<Asked> {
        self.rows()
            .into_iter()
            .find(|row| row.kind == kind && row.text == text)
    }

    /// How many requests were given up before they were answered.
    fn aborted(&self) -> usize {
        self.rows().iter().filter(|row| row.aborted).count()
    }

    fn peak(&self) -> usize {
        self.peak.load(Ordering::SeqCst)
    }
}

/// The translator every lane and the worker of the text in progress share.
struct Shared(Arc<Translator>);

impl Llm for Shared {
    fn complete(&self, request: &Completion<'_>) -> Result<String, LlmError> {
        self.0.serve(request)
    }
}

// ---- texts and events -------------------------------------------------------------

/// The text in progress of utterance `number`: `japanese_text(number)` without its `。`.
fn body(number: usize) -> String {
    japanese_text(number).trim_end_matches('。').to_owned()
}

/// `body(number)` with the `。` the recognizer closes it with.
fn closed(number: usize) -> String {
    japanese_text(number)
}

fn partial_event(text: &str) -> String {
    json!({"type": "partial", "text": text}).to_string()
}

/// The microphone `final` with id `mic-<seq>` and the given text.
fn final_of(seq: usize, text: &str) -> Sent {
    Sent {
        text: text.to_owned(),
        ..Sent::mic(seq, "final")
    }
}

fn is_final_of(value: &Value, id: &str, settled: bool) -> bool {
    value["type"] == "translation"
        && value["of"] == "final"
        && value["id"] == id
        && value["settled"] == settled
}

/// The first translation of the `final` `id` among what arrived, settled or a draft.
fn translation_of<'a>(
    arrived: &'a [(Instant, Value)],
    id: &str,
    settled: bool,
) -> Option<&'a (Instant, Value)> {
    arrived
        .iter()
        .find(|(_, value)| is_final_of(value, id, settled))
}

/// The translations of the text in progress among what arrived.
fn in_progress(arrived: &[(Instant, Value)]) -> Vec<&Value> {
    arrived
        .iter()
        .map(|(_, value)| value)
        .filter(|value| value["type"] == "translation" && value["of"] == "partial")
        .collect()
}

fn partial_shown(arrived: &[(Instant, Value)], text: &str) -> bool {
    let digest = sha256_hex(text);
    in_progress(arrived)
        .iter()
        .any(|value| value["src_sha256"] == digest.as_str())
}

// ---- the relay -----------------------------------------------------------------

/// A relay that translates the text in progress, its recognizer, its
/// translator and the record of what V was run against.
struct Rig {
    mic: FakeRecognizer,
    running: Running,
    llm: Arc<Translator>,
    sources: SourceLog,
    _dir: TempDir,
}

/// What came of one utterance: the `final` as sent, and what arrived by the time it settled.
struct Seen {
    sent: Sent,
    arrived: Vec<(Instant, Value)>,
}

impl Seen {
    fn draft(&self) -> Option<&(Instant, Value)> {
        translation_of(&self.arrived, &self.sent.id, false)
    }

    fn settled(&self) -> Option<&(Instant, Value)> {
        translation_of(&self.arrived, &self.sent.id, true)
    }

    /// How long after the `final` was pressed the draft was shown.
    fn draft_after(&self) -> Option<Duration> {
        self.draft()
            .map(|(at, _)| at.saturating_duration_since(self.sent.at))
    }
}

impl Rig {
    fn start(llm: Translator) -> Result<Rig, Box<dyn std::error::Error>> {
        let llm = Arc::new(llm);
        let dir = TempDir::new("rp69")?;
        let sources = SourceLog::default();
        let Some(dump) = VerifyDump::open(&dir.join("verify-text.jsonl"), &mut Logs::default())
        else {
            return Err("the dump file was not opened".into());
        };
        let dump = dump.keeping_sources(sources.clone());
        let mic = FakeRecognizer::start()?;
        let made = Arc::clone(&llm);
        let make: MakePipeline<Shared> = Arc::new(move |sink: &mut dyn LogSink| {
            Pipeline::open(Shared(Arc::clone(&made)), &PipelineConfig::default(), sink)
        });
        let running = run_relay_dumping(
            Logs::default(),
            vec![mic.url()],
            Lanes::PerSource,
            make,
            outputs(Vec::new(), None)?,
            PartialTranslation::On,
            Some(dump),
        )?;
        let rig = Rig {
            mic,
            running,
            llm,
            sources,
            _dir: dir,
        };
        rig.first_line();
        Ok(rig)
    }

    /// L0: a `final` (`mic-1`) before the utterance under test, so that the
    /// relay knows the next `id`. Returns once its translation is settled.
    fn first_line(&self) {
        final_of(1, &japanese_text(9)).send_through(&self.mic);
        self.running.translations.wait_for(1, WAIT);
    }

    fn partial(&self, text: &str) {
        self.mic.send(&partial_event(text));
    }

    /// Sends the `final` and returns once its line is settled.
    fn finish(&self, sent: Sent) -> Seen {
        let sent = sent.send_through(&self.mic);
        let id = sent.id.clone();
        let arrived = self
            .running
            .translations
            .wait_until(|got| translation_of(got, &id, true).is_some(), WAIT);
        Seen { sent, arrived }
    }

    /// Utterance `seq` whose last text in progress, `text`, is translated
    /// before the `final` `closing` comes.
    fn answered_then_final(&self, seq: usize, text: &str, closing: &str) -> Seen {
        self.partial(text);
        self.running
            .translations
            .wait_until(|got| partial_shown(got, text), WAIT);
        self.finish(final_of(seq, closing))
    }

    /// Utterance `seq` whose request for `text` is running when the `final`
    /// `closing` comes, 100 ms after the translator received it.
    fn running_then_final(&self, seq: usize, text: &str, closing: &str) -> Seen {
        self.partial(text);
        until(WAIT, || self.llm.count(Kind::T, text) > 0);
        sleep(Duration::from_millis(100));
        self.finish(final_of(seq, closing))
    }

    fn reused(&self) -> Vec<LogEvent> {
        self.running.logs.named("translate.draft.reused")
    }

    /// The `translate.line.ok` event of the line `id`.
    fn line_ok(&self, id: &str) -> Option<LogEvent> {
        self.running
            .logs
            .named("translate.line.ok")
            .into_iter()
            .find(|event| event.fields.get("id") == Some(&json!(id)))
    }
}

fn field<'a>(event: &'a LogEvent, key: &str) -> Option<&'a Value> {
    event.fields.get(key)
}

/// A reuse event of the line `id`.
fn reused_for(rig: &Rig, id: &str) -> Vec<LogEvent> {
    rig.reused()
        .into_iter()
        .filter(|event| field(event, "id") == Some(&json!(id)))
        .collect()
}

// ---- (a) the answer is in -------------------------------------------------------

/// T-RP-69-001: the answer of the text in progress is in; the `final` that
/// adds `。` takes it, T is not asked again, and the draft is shown at once.
#[test]
fn a_done_partial_answer_is_reused_without_asking_t_again() -> TestResult {
    let rig = Rig::start(Translator::new())?;
    let seen = rig.answered_then_final(2, &body(1), &closed(1));
    assert_eq!(rig.llm.count(Kind::T, &closed(1)), 0);
    assert_eq!(rig.llm.count(Kind::T, &body(1)), 1);
    let Some((_, draft)) = seen.draft() else {
        return Err("no draft for the final".into());
    };
    assert!(
        seen.draft_after().is_some_and(|after| after <= PROMPT),
        "{:?}",
        seen.draft_after()
    );
    assert_eq!(draft["of"], "final");
    assert_eq!(draft["stage"], "t");
    assert_eq!(draft["settled"], false);
    assert_eq!(draft["src_sha256"], sha256_hex(&closed(1)));
    assert_eq!(draft["text"], english_of(&body(1)));
    let Some((_, settled)) = seen.settled() else {
        return Err("the line was not settled".into());
    };
    assert_eq!(settled["stage"], "e");
    assert_eq!(rig.llm.count(Kind::E, &closed(1)), 1);
    Ok(())
}

/// T-RP-69-002: one log event, six items, no text; the line's `t_ms` is the wait.
#[test]
fn the_reuse_is_logged_once_with_its_six_items_and_no_text() -> TestResult {
    let rig = Rig::start(Translator::new())?;
    rig.answered_then_final(2, &body(1), &closed(1));
    let reused = rig.reused();
    let [event] = reused.as_slice() else {
        return Err(format!("{} reuse events, not 1", reused.len()).into());
    };
    let mut keys: Vec<&str> = event.fields.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        ["id", "match", "source", "state", "verdict", "waited_ms"]
    );
    assert_eq!(field(event, "id"), Some(&json!("mic-2")));
    assert_eq!(field(event, "source"), Some(&json!("mic")));
    assert_eq!(field(event, "match"), Some(&json!("closing_mark")));
    assert_eq!(field(event, "state"), Some(&json!("done")));
    assert_eq!(field(event, "verdict"), Some(&json!("ok")));
    let line = event.to_json_line();
    for text in [body(1), closed(1), english_of(&body(1))] {
        assert!(!line.contains(&text), "the event carries {text:?}: {line}");
    }
    let Some(ok) = rig.line_ok("mic-2") else {
        return Err("no translate.line.ok for mic-2".into());
    };
    assert_eq!(field(&ok, "t_ms"), field(event, "waited_ms"));
    Ok(())
}

/// T-RP-69-003: a `final` of the same text takes the answer too.
#[test]
fn a_final_with_the_same_text_reuses_the_answer_and_logs_same() -> TestResult {
    let rig = Rig::start(Translator::new())?;
    let seen = rig.answered_then_final(2, &body(1), &body(1));
    assert_eq!(rig.llm.count(Kind::T, &body(1)), 1);
    let reused = reused_for(&rig, "mic-2");
    let [event] = reused.as_slice() else {
        return Err(format!("{} reuse events, not 1", reused.len()).into());
    };
    assert_eq!(field(event, "match"), Some(&json!("same")));
    let Some((_, draft)) = seen.draft() else {
        return Err("no draft".into());
    };
    assert_eq!(draft["src_sha256"], sha256_hex(&body(1)));
    Ok(())
}

// ---- keys that do not match ----------------------------------------------------------

/// Runs an utterance whose `final` text is `closing` and returns the rig and
/// the line, for the cases where the answer must not be taken over.
fn not_reused(closing: &str) -> Result<(Rig, Seen), Box<dyn std::error::Error>> {
    let rig = Rig::start(Translator::new())?;
    let seen = rig.answered_then_final(2, &body(1), closing);
    Ok((rig, seen))
}

fn assert_asked_again(rig: &Rig, closing: &str) {
    assert_eq!(rig.llm.count(Kind::T, closing), 1, "T for {closing:?}");
    assert!(rig.reused().is_empty(), "{:?}", rig.reused());
}

/// T-RP-69-004: a question mark changes the sentence; T is asked again, and a
/// request still running is given up.
#[test]
fn a_final_that_adds_a_question_mark_asks_t_again() -> TestResult {
    let rig = Rig::start(Translator::new())?;
    let closing = format!("{}？", body(1));
    let seen = rig.running_then_final(2, &body(1), &closing);
    assert_asked_again(&rig, &closing);
    assert!(rig.llm.aborted() >= 1);
    assert!(seen.settled().is_some());
    Ok(())
}

/// T-RP-69-005: an exclamation mark likewise.
#[test]
fn a_final_that_adds_an_exclamation_mark_asks_t_again() -> TestResult {
    let rig = Rig::start(Translator::new())?;
    let closing = format!("{}！", body(1));
    let seen = rig.running_then_final(2, &body(1), &closing);
    assert_asked_again(&rig, &closing);
    assert!(rig.llm.aborted() >= 1);
    assert!(seen.settled().is_some());
    Ok(())
}

/// T-RP-69-006: not one `。`: two of them, another stop, a full stop of another script.
#[test]
fn a_final_that_adds_anything_but_one_closing_mark_asks_t_again() -> TestResult {
    let rig = Rig::start(Translator::new())?;
    for (index, suffix) in ["。。", "、", "."].into_iter().enumerate() {
        let closing = format!("{}{suffix}", body(1));
        let seen = rig.answered_then_final(2 + index, &body(1), &closing);
        assert!(seen.settled().is_some(), "{closing:?} was not settled");
        assert_eq!(rig.llm.count(Kind::T, &closing), 1, "T for {closing:?}");
        assert!(rig.reused().is_empty(), "{closing:?}: {:?}", rig.reused());
    }
    Ok(())
}

/// T-RP-69-007: another text (what ITN rewrote, say).
#[test]
fn a_final_with_other_text_asks_t_again() -> TestResult {
    let (rig, seen) = not_reused(&closed(2))?;
    assert_asked_again(&rig, &closed(2));
    assert!(seen.settled().is_some());
    Ok(())
}

/// T-RP-69-008: another `id`.
#[test]
fn a_final_with_another_id_asks_t_again() -> TestResult {
    let rig = Rig::start(Translator::new())?;
    rig.partial(&body(1));
    rig.running
        .translations
        .wait_until(|got| partial_shown(got, &body(1)), WAIT);
    let seen = rig.finish(final_of(4, &closed(1)));
    assert!(seen.settled().is_some());
    assert_asked_again(&rig, &closed(1));
    Ok(())
}

/// T-RP-69-009: another source; the `id` and the text are the same.
#[test]
fn a_final_from_another_source_asks_t_again() -> TestResult {
    let rig = Rig::start(Translator::new())?;
    rig.partial(&body(1));
    rig.running
        .translations
        .wait_until(|got| partial_shown(got, &body(1)), WAIT);
    let other = Sent {
        source: "loopback",
        id: String::from("mic-2"),
        lang: "en",
        text: closed(1),
        ..Sent::loopback(2, "final")
    };
    other.send_through(&rig.mic);
    assert!(until(WAIT, || rig.llm.count(Kind::T, &closed(1)) > 0));
    assert_asked_again(&rig, &closed(1));
    Ok(())
}

/// T-RP-69-010: the `final` closes the first of two texts, not the last.
#[test]
fn only_the_last_partial_is_compared_with_the_final() -> TestResult {
    let rig = Rig::start(Translator::new())?;
    for text in [body(1), body(2)] {
        rig.partial(&text);
        rig.running
            .translations
            .wait_until(|got| partial_shown(got, &text), WAIT);
    }
    let seen = rig.finish(final_of(2, &closed(1)));
    assert!(seen.settled().is_some());
    assert_asked_again(&rig, &closed(1));
    Ok(())
}

/// T-RP-69-011: the order reversed; the `final` closes the last text.
#[test]
fn the_last_partial_is_the_one_that_is_reused() -> TestResult {
    let rig = Rig::start(Translator::new())?;
    for text in [body(2), body(1)] {
        rig.partial(&text);
        rig.running
            .translations
            .wait_until(|got| partial_shown(got, &text), WAIT);
    }
    let seen = rig.finish(final_of(2, &closed(1)));
    assert!(seen.settled().is_some());
    assert_eq!(rig.llm.count(Kind::T, &closed(1)), 0);
    assert_eq!(rig.reused().len(), 1);
    Ok(())
}

/// T-RP-69-012: no text in progress; the base of the cases above.
#[test]
fn a_final_without_a_partial_asks_t_as_before() -> TestResult {
    let rig = Rig::start(Translator::new())?;
    let seen = rig.finish(final_of(2, &closed(1)));
    assert!(seen.settled().is_some());
    assert_asked_again(&rig, &closed(1));
    Ok(())
}

// ---- (b) the request is running ----------------------------------------------------------

/// T-RP-69-013: the request of the text in progress is running when the
/// `final` comes; it is not given up, the line waits for it, and T is asked
/// once in all.
#[test]
fn a_running_partial_is_awaited_not_aborted() -> TestResult {
    let rig = Rig::start(Translator::new())?;
    let seen = rig.running_then_final(2, &body(1), &closed(1));
    assert_eq!(rig.llm.aborted(), 0);
    assert_eq!(rig.llm.count(Kind::T, &body(1)), 1);
    assert_eq!(rig.llm.count(Kind::T, &closed(1)), 0);
    let Some(asked) = rig.llm.first(Kind::T, &body(1)) else {
        return Err("T was never asked".into());
    };
    let Some((drafted, draft)) = seen.draft() else {
        return Err("no draft for the final".into());
    };
    assert_eq!(draft["stage"], "t");
    let after = drafted.saturating_duration_since(asked.at);
    assert!(
        after >= T_DELAY,
        "the draft came {after:?} after the request"
    );
    let reused = reused_for(&rig, "mic-2");
    let [event] = reused.as_slice() else {
        return Err(format!("{} reuse events, not 1", reused.len()).into());
    };
    assert_eq!(field(event, "state"), Some(&json!("running")));
    let Some(waited) = field(event, "waited_ms").and_then(Value::as_u64) else {
        return Err("no waited_ms".into());
    };
    assert!((100..=400).contains(&waited), "waited {waited} ms");
    let Some(ok) = rig.line_ok("mic-2") else {
        return Err("no translate.line.ok".into());
    };
    assert_eq!(field(&ok, "t_ms"), Some(&json!(waited)));
    // The answer is the line's now: it is not shown as a text in progress.
    assert!(
        in_progress(&seen.arrived).is_empty(),
        "{:?}",
        in_progress(&seen.arrived)
    );
    Ok(())
}

/// T-RP-69-014: the next utterance's text in progress comes while the line
/// waits; the request the line waits for is not given up.
#[test]
fn the_next_partial_does_not_abort_the_request_a_line_awaits() -> TestResult {
    let rig = Rig::start(Translator::new())?;
    rig.partial(&body(1));
    until(WAIT, || rig.llm.count(Kind::T, &body(1)) > 0);
    sleep(Duration::from_millis(100));
    let sent = final_of(2, &closed(1)).send_through(&rig.mic);
    rig.partial(&body(2));
    let id = sent.id.clone();
    let arrived = rig
        .running
        .translations
        .wait_until(|got| translation_of(got, &id, true).is_some(), WAIT);
    assert_eq!(rig.llm.aborted(), 0);
    assert!(translation_of(&arrived, &id, false).is_some());
    assert!(until(WAIT, || rig.llm.count(Kind::T, &body(2)) == 1));
    Ok(())
}

/// T-RP-69-015: the worker is still busy with the request a line awaits when
/// the next utterance's text in progress and its `final` come. That text was
/// never taken, so it is dropped and the lane asks T itself.
#[test]
fn a_queued_partial_is_dropped_and_the_lane_asks_t_itself() -> TestResult {
    let rig = Rig::start(Translator::new())?;
    rig.partial(&body(1));
    until(WAIT, || rig.llm.count(Kind::T, &body(1)) > 0);
    sleep(Duration::from_millis(50));
    final_of(2, &closed(1)).send_through(&rig.mic);
    rig.partial(&body(2));
    let seen = rig.finish(final_of(3, &closed(2)));
    assert!(seen.settled().is_some());
    assert_eq!(rig.llm.count(Kind::T, &body(2)), 0);
    assert_eq!(rig.llm.count(Kind::T, &closed(2)), 1);
    assert!(reused_for(&rig, "mic-3").is_empty());
    assert_eq!(reused_for(&rig, "mic-2").len(), 1);
    Ok(())
}

/// T-RP-69-016: the request of the text in progress fails; the line asks T.
#[test]
fn a_failed_partial_request_makes_the_lane_ask_t() -> TestResult {
    let body_text = body(1);
    let rig = Rig::start(Translator::new().t_reply(move |text| {
        if text == body_text {
            Err(500)
        } else {
            Ok(plain_translation(text))
        }
    }))?;
    let seen = rig.running_then_final(2, &body(1), &closed(1));
    assert_eq!(rig.llm.count(Kind::T, &closed(1)), 1);
    assert!(rig.reused().is_empty());
    let Some((_, draft)) = seen.draft() else {
        return Err("the line got no draft".into());
    };
    assert_eq!(draft["stage"], "t");
    assert!(seen.settled().is_some());
    Ok(())
}

/// T-RP-69-017: six utterances in a row, each `final` 30 ms after its text in
/// progress. A lane that waits holds no slot of the server, so all six are
/// translated in time and the server never has more than four at once.
#[test]
fn lanes_waiting_for_partials_do_not_hold_a_slot() -> TestResult {
    let rig = Rig::start(Translator::new())?;
    let started = Instant::now();
    for seq in 2..8 {
        rig.partial(&body(seq));
        sleep(Duration::from_millis(30));
        final_of(seq, &closed(seq)).send_through(&rig.mic);
    }
    let arrived = rig.running.translations.wait_until(
        |got| (2..8).all(|seq| translation_of(got, &format!("mic-{seq}"), true).is_some()),
        Duration::from_secs(3),
    );
    for seq in 2..8 {
        let id = format!("mic-{seq}");
        let Some((at, _)) = translation_of(&arrived, &id, true) else {
            return Err(format!("{id} was not settled in 3 s").into());
        };
        assert!(
            at.saturating_duration_since(started) <= Duration::from_secs(3),
            "{id}"
        );
        let asks = rig.llm.count(Kind::T, &body(seq)) + rig.llm.count(Kind::T, &closed(seq));
        assert!(asks <= 1, "{id}: T asked {asks} times");
    }
    assert!(rig.llm.peak() <= 4, "{} requests at once", rig.llm.peak());
    Ok(())
}

// ---- (c) the answer fails V ---------------------------------------------------------------

/// A translator whose answer for the text in progress of utterance 1 carries a
/// number the text does not have, so that V holds it. E answers cleanly.
fn translator_with_a_bad_draft() -> Translator {
    let bad_for = body(1);
    Translator::new()
        .t_reply(move |text| {
            if text == bad_for {
                Ok(format!("{} 7", english_of(text)))
            } else {
                Ok(plain_translation(text))
            }
        })
        .e_reply(|source, _| plain_translation(source))
}

/// T-RP-69-018: the answer of the text in progress fails V against the line's
/// text; T is not asked again and E is asked at once.
#[test]
fn a_reused_answer_that_fails_v_sends_e_at_once() -> TestResult {
    let rig = Rig::start(translator_with_a_bad_draft())?;
    rig.partial(&body(1));
    assert!(until(WAIT, || !rig
        .running
        .logs
        .named("translate.partial.held")
        .is_empty()));
    let seen = rig.finish(final_of(2, &closed(1)));
    assert_eq!(rig.llm.count(Kind::T, &closed(1)), 0);
    let Some(asked) = rig.llm.first(Kind::E, &closed(1)) else {
        return Err("E was never asked".into());
    };
    let after = asked.at.saturating_duration_since(seen.sent.at);
    assert!(after <= PROMPT, "E was asked {after:?} after the final");
    assert!(
        seen.draft().is_none(),
        "a draft was shown: {:?}",
        seen.draft()
    );
    let reused = reused_for(&rig, "mic-2");
    let [event] = reused.as_slice() else {
        return Err(format!("{} reuse events, not 1", reused.len()).into());
    };
    assert_eq!(field(event, "verdict"), Some(&json!("held")));
    assert_eq!(field(event, "state"), Some(&json!("done")));
    let Some((_, settled)) = seen.settled() else {
        return Err("the line was not settled".into());
    };
    assert_eq!(
        (settled["stage"].as_str(), settled["status"].as_str()),
        (Some("e"), Some("ok"))
    );
    Ok(())
}

/// T-RP-69-019: the same while the request is still running: E is asked as
/// soon as the answer comes.
#[test]
fn a_running_answer_that_fails_v_sends_e_as_soon_as_it_arrives() -> TestResult {
    let rig = Rig::start(translator_with_a_bad_draft())?;
    let seen = rig.running_then_final(2, &body(1), &closed(1));
    assert_eq!(rig.llm.aborted(), 0);
    assert_eq!(rig.llm.count(Kind::T, &closed(1)), 0);
    let (Some(t), Some(e)) = (
        rig.llm.first(Kind::T, &body(1)),
        rig.llm.first(Kind::E, &closed(1)),
    ) else {
        return Err("T or E was not asked".into());
    };
    let Some(answered) = t.done else {
        return Err("the request has no end".into());
    };
    let after = e.at.saturating_duration_since(answered);
    assert!(
        after <= PROMPT,
        "E was asked {after:?} after the answer came"
    );
    assert!(seen.draft().is_none());
    let reused = reused_for(&rig, "mic-2");
    let [event] = reused.as_slice() else {
        return Err(format!("{} reuse events, not 1", reused.len()).into());
    };
    assert_eq!(field(event, "state"), Some(&json!("running")));
    assert_eq!(field(event, "verdict"), Some(&json!("held")));
    Ok(())
}

// ---- (d) V against the final, (e) V at most twice --------------------------------------------

/// T-RP-69-020: V judges the reused answer against the `final`'s text, never
/// the text in progress.
#[test]
fn the_reused_answer_is_verified_against_the_final_text() -> TestResult {
    let rig = Rig::start(Translator::new())?;
    rig.answered_then_final(2, &body(1), &closed(1));
    let sources = rig.sources.sources_of("mic-2");
    assert!(!sources.is_empty());
    assert!(
        sources.iter().all(|source| *source == closed(1)),
        "{sources:?}"
    );
    assert!(!sources.contains(&body(1)));
    Ok(())
}

/// T-RP-69-021: V runs at most twice for a line: on T's answer and on E's.
#[test]
fn a_reused_line_runs_v_at_most_twice() -> TestResult {
    let rig = Rig::start(Translator::new())?;
    rig.answered_then_final(2, &body(1), &closed(1));
    assert_eq!(rig.sources.sources_of("mic-2"), [closed(1), closed(1)]);
    let held = Rig::start(translator_with_a_bad_draft())?;
    held.partial(&body(1));
    until(WAIT, || {
        !held.running.logs.named("translate.partial.held").is_empty()
    });
    held.finish(final_of(2, &closed(1)));
    assert_eq!(held.sources.sources_of("mic-2"), [closed(1), closed(1)]);
    Ok(())
}

/// T-RP-69-022: E starts from the reused answer.
#[test]
fn e_is_asked_with_the_reused_answer_as_its_draft() -> TestResult {
    let rig = Rig::start(Translator::new())?;
    rig.answered_then_final(2, &body(1), &closed(1));
    let Some(asked) = rig.llm.first(Kind::E, &closed(1)) else {
        return Err("E was never asked".into());
    };
    assert_eq!(asked.draft, english_of(&body(1)));
    Ok(())
}

/// T-RP-69-023: a reused translation has the keys of a fresh one.
#[test]
fn a_reused_translation_has_the_same_wire_keys_as_a_fresh_one() -> TestResult {
    let rig = Rig::start(Translator::new())?;
    let reused = rig.answered_then_final(2, &body(1), &closed(1));
    let fresh_text = format!("{}？", body(2));
    let fresh = rig.answered_then_final(3, &body(2), &fresh_text);
    let keys = |seen: &Seen| -> Option<Vec<String>> {
        seen.draft()?
            .1
            .as_object()
            .map(|map| map.keys().cloned().collect())
    };
    let (Some(reused_keys), Some(fresh_keys)) = (keys(&reused), keys(&fresh)) else {
        return Err("a line has no draft".into());
    };
    assert_eq!(reused_keys, fresh_keys);
    for seen in [&reused, &fresh] {
        let Some((_, draft)) = seen.draft() else {
            return Err("no draft".into());
        };
        assert_eq!(
            (
                draft["status"].as_str(),
                draft["settled"].as_bool(),
                draft["of"].as_str()
            ),
            (Some("ok"), Some(false), Some("final"))
        );
    }
    Ok(())
}

/// T-RP-69-025: the acceptance. A line that takes the answer when it is in,
/// a line that waits for it, a line whose answer fails V, and the checks on V.
#[test]
fn rp69_a_final_that_only_closes_the_last_partial_reuses_its_translation() -> TestResult {
    let rig = Rig::start(
        Translator::new()
            .t_reply(|text| {
                if text == body(3) {
                    Ok(format!("{} 7", english_of(text)))
                } else {
                    Ok(plain_translation(text))
                }
            })
            .e_reply(|source, _| plain_translation(source)),
    )?;
    // (a) the answer is in, and the `final` adds a `。`: T is not asked again
    let done = rig.answered_then_final(2, &body(1), &closed(1));
    assert_eq!(rig.llm.count(Kind::T, &closed(1)), 0);
    assert!(done.draft_after().is_some_and(|after| after <= PROMPT));
    assert_eq!(
        field_of(&reused_for(&rig, "mic-2"), "match"),
        Some(json!("closing_mark"))
    );
    // (b) the request is running: it is awaited and not given up, and T is asked once in all
    rig.running_then_final(3, &body(2), &closed(2));
    assert_eq!(
        rig.llm.count(Kind::T, &body(2)) + rig.llm.count(Kind::T, &closed(2)),
        1
    );
    assert_eq!(rig.llm.aborted(), 0);
    assert_eq!(
        field_of(&reused_for(&rig, "mic-3"), "state"),
        Some(json!("running"))
    );
    // (c) the answer fails V: no draft, E at once
    rig.partial(&body(3));
    assert!(until(WAIT, || !rig
        .running
        .logs
        .named("translate.partial.held")
        .is_empty()));
    let held = rig.finish(final_of(4, &closed(3)));
    assert!(held.draft().is_none());
    let Some(asked) = rig.llm.first(Kind::E, &closed(3)) else {
        return Err("E was never asked for the held line".into());
    };
    assert!(asked.at.saturating_duration_since(held.sent.at) <= PROMPT);
    assert_eq!(
        field_of(&reused_for(&rig, "mic-4"), "verdict"),
        Some(json!("held"))
    );
    // (d) V judged the answers against the `final`, and (e) at most twice for a line
    for (id, number) in [("mic-2", 1), ("mic-3", 2), ("mic-4", 3)] {
        assert_eq!(
            rig.sources.sources_of(id),
            [closed(number), closed(number)],
            "{id}"
        );
    }
    Ok(())
}

/// The value of `key` in the only event of `events`.
fn field_of(events: &[LogEvent], key: &str) -> Option<Value> {
    let [event] = events else {
        return None;
    };
    field(event, key).cloned()
}

//! RP-85: a Japanese `final` that differs from the text in progress only in how
//! its numerals are written and in the `。` `、` inside it takes over the
//! translation T gave that text. V judges the answer against the `final`'s own
//! text, so a number that changed is held there and E is asked at once.
//!
//! The rows of 4.77 of the test design are the functions of this file, in
//! their order:
//!
//! - 001 to 017 run the relay against a fake recognizer and a scripted
//!   translator that answers a T request after 300 ms and an E request after
//!   100 ms and records every request.
//! - 018 to 021 read the pairs file the quality probe asks about
//!   (`docs/results/r8x/rp85_pairs.jsonl`).
//! - 022 to 025 compare how often V stops the answer of the text in progress
//!   with how often it stops the answer of the `final`. Only 025 asks a
//!   llama-server (port 8080) and runs with `--ignored`.
//! - 026 to 031 judge the records of the runs of window E3'. Only 031 reads
//!   them and runs with `--ignored`.
//! - 032 calls V directly on the scripted answer for utterance 7.
//! - 033 is the rule that a run whose log dropped lines is not read.

mod support;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::sleep;
use std::time::{Duration, Instant};

use hayamimi_relay::lang::Lang;
use hayamimi_relay::llm::{Completion, HttpLlm, Llm, LlmError};
use hayamimi_relay::logging::{LogEvent, LogSink, NullSink};
use hayamimi_relay::partial::{PartialTranslation, reuse_match};
use hayamimi_relay::pipeline::{
    MAX_TOKENS, Pipeline, PipelineConfig, ReuseMatch, sha256_hex, translate_prompt,
};
use hayamimi_relay::relay::{Lanes, MakePipeline};
use hayamimi_relay::verify::{Verdict, Verifier};
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
/// How soon after the `final` E is asked when the reused answer is held.
const PROMPT: Duration = Duration::from_millis(50);

// ---- the pairs of the recorded run (F85) -----------------------------------------

const P7_DRAFT: &str =
    "千九百四十年八月十五日連合国は南フランスに侵攻しその侵攻はドラグーン作戦と呼ばれました";
const P7_FINAL: &str =
    "1940年八月15日連合国は南フランスに侵攻しその侵攻はドラグーン作戦と呼ばれました。";
const P7_ANSWER: &str = "On August 15, 1940, the Allies invaded southern France, and the invasion was called Operation Dragoon.";
const P11_DRAFT: &str = "そこの上流から丸太を組んでいかだにしてどんどん流したんですこれを";
const P11_FINAL: &str = "そこの上流から丸太を組んでいかだにしてどんどん流したんです。これを。";
const P20_DRAFT: &str = "言ったろヒーローよりもてめぇを信じる";
const P20_FINAL: &str = "言ったろ。ヒーローよりもてめぇを信じる。";
const DAMAGE_DRAFT: &str = "被害は三件でした";
const DAMAGE_ANSWER: &str = "There were three cases of damage";

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

// ---- the translator --------------------------------------------------------------

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
    at: Instant,
}

type TReply = dyn Fn(&str) -> String + Send + Sync;
type EReply = dyn Fn(&str, &str) -> String + Send + Sync;

/// A translator that records what it is asked and answers from a script.
struct Translator {
    asked: Mutex<Vec<Asked>>,
    /// T's reply for a source text.
    t_reply: Box<TReply>,
    /// E's reply for a source text and the draft.
    e_reply: Box<EReply>,
}

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

/// `answers` as owned pairs, to move into a reply.
fn owned(answers: &[(&str, &str)]) -> Vec<(String, String)> {
    answers
        .iter()
        .map(|(text, answer)| ((*text).to_owned(), (*answer).to_owned()))
        .collect()
}

impl Translator {
    fn new() -> Translator {
        Translator {
            asked: Mutex::new(Vec::new()),
            t_reply: Box::new(plain_translation),
            e_reply: Box::new(|_, draft| draft.to_owned()),
        }
    }

    /// T answers `answers` (source text, translation) and spells any other text.
    fn t_answers(mut self, answers: &[(&str, &str)]) -> Translator {
        let script = owned(answers);
        self.t_reply = Box::new(
            move |text| match script.iter().find(|(key, _)| key == text) {
                Some((_, answer)) => answer.clone(),
                None => plain_translation(text),
            },
        );
        self
    }

    /// E answers `answers` (source text, edited translation) and returns the draft otherwise.
    fn e_answers(mut self, answers: &[(&str, &str)]) -> Translator {
        let script = owned(answers);
        self.e_reply =
            Box::new(
                move |text, draft| match script.iter().find(|(key, _)| key == text) {
                    Some((_, answer)) => answer.clone(),
                    None => draft.to_owned(),
                },
            );
        self
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

    fn serve(&self, request: &Completion<'_>) -> Result<String, LlmError> {
        let Some((stage, text, draft)) = read_prompt(request.prompt, request.system) else {
            return Err(LlmError::Body(String::from(
                "a prompt the test does not know",
            )));
        };
        let (kind, delay) = match stage {
            Stage::Translate => (Kind::T, T_DELAY),
            Stage::Edit => (Kind::E, E_DELAY),
        };
        locked(&self.asked).push(Asked {
            kind,
            text: text.to_owned(),
            at: Instant::now(),
        });
        Translator::wait(request, delay)?;
        Ok(match kind {
            Kind::T => (self.t_reply)(text),
            Kind::E => (self.e_reply)(text, draft),
        })
    }

    /// How many requests of `kind` were made for the source text `text`.
    fn count(&self, kind: Kind, text: &str) -> usize {
        locked(&self.asked)
            .iter()
            .filter(|row| row.kind == kind && row.text == text)
            .count()
    }

    /// The first request of `kind` for `text`.
    fn first(&self, kind: Kind, text: &str) -> Option<Asked> {
        locked(&self.asked)
            .iter()
            .find(|row| row.kind == kind && row.text == text)
            .cloned()
    }
}

/// The translator every lane and the worker of the text in progress share.
struct Shared(Arc<Translator>);

impl Llm for Shared {
    fn complete(&self, request: &Completion<'_>) -> Result<String, LlmError> {
        self.0.serve(request)
    }
}

// ---- events and the relay --------------------------------------------------------

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

/// The first translation of the `final` `id` among what arrived, settled or a draft.
fn translation_of<'a>(
    arrived: &'a [(Instant, Value)],
    id: &str,
    settled: bool,
) -> Option<&'a (Instant, Value)> {
    arrived.iter().find(|(_, value)| {
        value["type"] == "translation"
            && value["of"] == "final"
            && value["id"] == id
            && value["settled"] == settled
    })
}

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
}

impl Rig {
    fn start(llm: Translator) -> Result<Rig, Box<dyn std::error::Error>> {
        let llm = Arc::new(llm);
        let dir = TempDir::new("rp85")?;
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
        final_of(1, &japanese_text(9)).send_through(&rig.mic);
        rig.running.translations.wait_for(1, WAIT);
        Ok(rig)
    }

    /// A line of the loopback before the utterance under test, so that the
    /// relay reads the text in progress after it as English.
    fn switch_to_the_loopback(&self) {
        Sent::loopback(1, "final").send_through(&self.mic);
        self.running.translations.wait_for(2, WAIT);
    }

    /// Whether T's answer for the text in progress has been judged by V.
    fn partial_judged(&self) -> bool {
        ["translate.partial.ok", "translate.partial.held"]
            .iter()
            .any(|name| !self.running.logs.named(name).is_empty())
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

    /// `draft` is the text in progress, its translation is in, and then `closing` comes.
    fn answered_then_final(&self, draft: &str, closing: Sent) -> Seen {
        self.mic.send(&partial_event(draft));
        until(WAIT, || self.partial_judged());
        self.finish(closing)
    }

    fn reused(&self) -> Vec<LogEvent> {
        self.running.logs.named("translate.draft.reused")
    }
}

fn field<'a>(event: &'a LogEvent, key: &str) -> Option<&'a Value> {
    event.fields.get(key)
}

/// The one reuse event of the line `mic-2`.
fn only_reuse(rig: &Rig) -> Result<LogEvent, Box<dyn std::error::Error>> {
    let reused = rig.reused();
    match reused.as_slice() {
        [event] if field(event, "id") == Some(&json!("mic-2")) => Ok(event.clone()),
        _ => Err(format!("not one reuse event of mic-2: {reused:?}").into()),
    }
}

/// Utterance 2 of the microphone: `draft` is in progress and answered, then
/// `closing` ends it.
fn utterance(
    translator: Translator,
    draft: &str,
    closing: &str,
) -> Result<(Rig, Seen), Box<dyn std::error::Error>> {
    let rig = Rig::start(translator)?;
    let seen = rig.answered_then_final(draft, final_of(2, closing));
    Ok((rig, seen))
}

/// The answer of the text in progress was taken over under `matched`, and T
/// was not asked for the `final`'s own text.
fn assert_taken_over(
    rig: &Rig,
    draft: &str,
    closing: &str,
    matched: &str,
) -> Result<LogEvent, Box<dyn std::error::Error>> {
    assert_eq!(rig.llm.count(Kind::T, draft), 1, "T for the draft");
    assert_eq!(
        rig.llm.count(Kind::T, closing),
        usize::from(draft == closing),
        "T for the final"
    );
    let event = only_reuse(rig)?;
    assert_eq!(field(&event, "match"), Some(&json!(matched)));
    Ok(event)
}

/// The answer was not taken over: T is asked for the `final`'s own text.
fn assert_asked_again(rig: &Rig, closing: &str) {
    assert_eq!(rig.llm.count(Kind::T, closing), 1, "T for {closing:?}");
    assert!(rig.reused().is_empty(), "{:?}", rig.reused());
}

/// The draft of the `final` was shown from T's reused answer, V having passed it.
fn assert_draft_from_t(seen: &Seen, closing: &str) -> TestResult {
    let Some((_, draft)) = seen.draft() else {
        return Err("no draft for the final".into());
    };
    assert_eq!(draft["of"], "final");
    assert_eq!(draft["stage"], "t");
    assert_eq!(draft["settled"], false);
    assert_eq!(draft["src_sha256"], sha256_hex(closing));
    Ok(())
}

// ---- (1) the key: pairs the widened key takes -----------------------------------

/// T-RP-85-001: numerals written differently, and the `final` ends in `。`.
#[test]
fn a_draft_with_other_numerals_is_reused_under_the_widened_key() -> TestResult {
    let script = Translator::new().t_answers(&[(P7_DRAFT, P7_ANSWER)]);
    let (rig, seen) = utterance(script, P7_DRAFT, P7_FINAL)?;
    let event = assert_taken_over(&rig, P7_DRAFT, P7_FINAL, "normalized")?;
    assert_eq!(field(&event, "verdict"), Some(&json!("ok")));
    assert_draft_from_t(&seen, P7_FINAL)
}

/// T-RP-85-002: a `。` inside the `final` that the text in progress lacks.
#[test]
fn a_draft_without_inner_marks_is_reused_under_the_widened_key() -> TestResult {
    let answer = "They tied logs together upstream and floated them down as a raft, this one.";
    let script = Translator::new().t_answers(&[(P11_DRAFT, answer)]);
    let (rig, seen) = utterance(script, P11_DRAFT, P11_FINAL)?;
    let event = assert_taken_over(&rig, P11_DRAFT, P11_FINAL, "normalized")?;
    assert_eq!(field(&event, "verdict"), Some(&json!("ok")));
    assert_draft_from_t(&seen, P11_FINAL)
}

/// T-RP-85-003: a `。` after the first clause as well as at the end.
#[test]
fn a_draft_without_the_inner_mark_after_the_first_clause_is_reused() -> TestResult {
    let (rig, _) = utterance(Translator::new(), P20_DRAFT, P20_FINAL)?;
    assert_taken_over(&rig, P20_DRAFT, P20_FINAL, "normalized")?;
    Ok(())
}

/// T-RP-85-004: a `、` in the text in progress that the `final` lacks.
#[test]
fn a_draft_with_a_comma_the_final_lacks_is_reused() -> TestResult {
    let (draft, closing) = ("ところで、昨日は雨でした", "ところで昨日は雨でした。");
    let (rig, _) = utterance(Translator::new(), draft, closing)?;
    assert_taken_over(&rig, draft, closing, "normalized")?;
    Ok(())
}

/// T-RP-85-005: only the numerals differ, and no mark is added.
#[test]
fn a_draft_differing_only_in_numerals_is_reused() -> TestResult {
    let (draft, closing) = (DAMAGE_DRAFT, "被害は3件でした");
    let (rig, _) = utterance(Translator::new(), draft, closing)?;
    assert_taken_over(&rig, draft, closing, "normalized")?;
    Ok(())
}

// ---- the keys that were there before win --------------------------------------------

/// T-RP-85-006: the same text is still `same`.
#[test]
fn a_final_with_the_same_text_still_logs_same() -> TestResult {
    let (rig, _) = utterance(Translator::new(), "今日は晴れです", "今日は晴れです")?;
    assert_taken_over(&rig, "今日は晴れです", "今日は晴れです", "same")?;
    Ok(())
}

/// T-RP-85-007: the text with `。` added is still `closing_mark`.
#[test]
fn a_final_that_only_adds_the_closing_mark_still_logs_closing_mark() -> TestResult {
    let (rig, _) = utterance(Translator::new(), "今日は晴れです", "今日は晴れです。")?;
    assert_taken_over(&rig, "今日は晴れです", "今日は晴れです。", "closing_mark")?;
    Ok(())
}

/// T-RP-85-008: a pair both keys take keeps the earlier key's value.
#[test]
fn a_pair_both_keys_match_logs_the_existing_value() -> TestResult {
    let (draft, closing) = ("ところで、昨日は雨でした", "ところで、昨日は雨でした。");
    let (rig, _) = utterance(Translator::new(), draft, closing)?;
    assert_taken_over(&rig, draft, closing, "closing_mark")?;
    Ok(())
}

// ---- pairs the widened key leaves apart ---------------------------------------------

/// A pair the key must not take: T is asked for the `final` again.
fn assert_kept_apart(draft: &str, closing: &str) -> TestResult {
    let (rig, seen) = utterance(Translator::new(), draft, closing)?;
    assert_asked_again(&rig, closing);
    assert!(seen.settled().is_some(), "{closing:?} was not settled");
    Ok(())
}

/// T-RP-85-009: a `？` changes a statement into a question.
#[test]
fn a_question_mark_added_by_the_final_keeps_the_pair_apart() -> TestResult {
    assert_kept_apart("これは本当ですか", "これは本当ですか？")
}

/// T-RP-85-010: a `！` is not left out either.
#[test]
fn an_exclamation_mark_added_by_the_final_keeps_the_pair_apart() -> TestResult {
    assert_kept_apart("すごいですね", "すごいですね！")
}

/// T-RP-85-011: numerals and marks agree but a letter differs.
#[test]
fn a_draft_with_a_different_letter_is_not_reused() -> TestResult {
    let draft = P7_DRAFT.replace("連合国は", "連合国が");
    assert_kept_apart(&draft, P7_FINAL)
}

/// T-RP-85-012: two runs of numerals against three are not one run each.
#[test]
fn a_pair_with_a_different_number_of_numerals_is_not_reused() -> TestResult {
    assert_kept_apart("三件と五件です", "3件と5件と7件です。")
}

/// T-RP-85-013: an English line is compared as it stands. The English the
/// loopback says is read after a line of the loopback has come.
#[test]
fn an_english_line_is_never_matched_by_the_widened_key() -> TestResult {
    for (draft, closing) in [
        ("In 1944 the allies landed", "In 1940 the allies landed"),
        ("well we left", "well, we left."),
    ] {
        let rig = Rig::start(Translator::new())?;
        rig.switch_to_the_loopback();
        let ending = Sent {
            text: closing.to_owned(),
            ..Sent::loopback(2, "final")
        };
        let seen = rig.answered_then_final(draft, ending);
        assert_asked_again(&rig, closing);
        assert!(seen.settled().is_some(), "{closing:?} was not settled");
    }
    Ok(())
}

/// T-RP-85-014: a `final` of another `id` does not close the text in progress.
#[test]
fn the_widened_key_still_needs_the_same_id() -> TestResult {
    let script = Translator::new().t_answers(&[(P7_DRAFT, P7_ANSWER)]);
    let rig = Rig::start(script)?;
    let seen = rig.answered_then_final(P7_DRAFT, final_of(3, P7_FINAL));
    assert_asked_again(&rig, P7_FINAL);
    assert!(seen.settled().is_some());
    Ok(())
}

/// T-RP-85-015: the key follows the `lang` of the `final`, not its letters.
#[test]
fn the_widened_key_follows_the_lang_of_the_final() -> TestResult {
    let script = Translator::new().t_answers(&[(P7_DRAFT, P7_ANSWER)]);
    let rig = Rig::start(script)?;
    let english = Sent {
        lang: "en",
        ..final_of(2, P7_FINAL)
    };
    rig.mic.send(&partial_event(P7_DRAFT));
    until(WAIT, || rig.partial_judged());
    english.send_through(&rig.mic);
    // The relay does not translate a line of English: it notes the line and
    // never asks for a reuse.
    let skipped = until(WAIT, || {
        !rig.running.logs.named("translate.line.skipped").is_empty()
    });
    assert!(skipped, "the English line was not noted as skipped");
    assert!(rig.reused().is_empty(), "{:?}", rig.reused());
    Ok(())
}

// ---- (2) V judges the taken answer against the final -------------------------------

/// T-RP-85-016: the number changed, so V stops the answer and E is asked at once.
#[test]
fn a_reused_draft_with_another_number_is_stopped_by_v_and_e_is_asked() -> TestResult {
    let closing = "被害は4件でした。";
    let script = Translator::new()
        .t_answers(&[(DAMAGE_DRAFT, DAMAGE_ANSWER)])
        .e_answers(&[(closing, "There were 4 cases of damage")]);
    let (rig, seen) = utterance(script, DAMAGE_DRAFT, closing)?;
    let event = assert_taken_over(&rig, DAMAGE_DRAFT, closing, "normalized")?;
    assert_eq!(field(&event, "state"), Some(&json!("done")));
    assert_eq!(field(&event, "verdict"), Some(&json!("held")));
    assert_eq!(rig.running.logs.named("translate.draft.held").len(), 1);
    assert_eq!(rig.llm.count(Kind::E, closing), 1);
    let Some(asked) = rig.llm.first(Kind::E, closing) else {
        return Err("E was never asked".into());
    };
    let after = asked.at.saturating_duration_since(seen.sent.at);
    assert!(after <= PROMPT, "E was asked {after:?} after the final");
    assert!(
        seen.draft().is_none(),
        "a draft was shown: {:?}",
        seen.draft()
    );
    let Some((_, settled)) = seen.settled() else {
        return Err("the line was not settled".into());
    };
    assert_eq!(settled["stage"], "e");
    assert_eq!(rig.sources.sources_of("mic-2"), [closing, closing]);
    Ok(())
}

/// T-RP-85-017: the same number in another script is not a reason to stop.
#[test]
fn a_reused_draft_with_the_same_number_passes_v() -> TestResult {
    let closing = "被害は3件でした。";
    let script = Translator::new().t_answers(&[(DAMAGE_DRAFT, DAMAGE_ANSWER)]);
    let (rig, seen) = utterance(script, DAMAGE_DRAFT, closing)?;
    let event = assert_taken_over(&rig, DAMAGE_DRAFT, closing, "normalized")?;
    assert_eq!(field(&event, "verdict"), Some(&json!("ok")));
    assert_draft_from_t(&seen, closing)?;
    assert_eq!(rig.llm.count(Kind::E, closing), 1);
    Ok(())
}

// ---- (3) the pairs file --------------------------------------------------------------

/// A pair of the quality probe: the text in progress and the `final` that closed it.
#[derive(Debug, PartialEq)]
struct Pair {
    utt: u64,
    draft: String,
    closing: String,
}

/// Why a pairs file was not accepted, and the line when it is one line's fault.
#[derive(Debug, PartialEq)]
struct PairsFault {
    line: Option<usize>,
    what: String,
}

impl std::fmt::Display for PairsFault {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.line {
            Some(line) => write!(out, "pairs line {line}: {}", self.what),
            None => write!(out, "pairs: {}", self.what),
        }
    }
}

impl std::error::Error for PairsFault {}

fn pair_of(line: &str) -> Result<Pair, String> {
    let value: Value = serde_json::from_str(line).map_err(|error| error.to_string())?;
    let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
    let (Some(draft), Some(closing)) = (text("draft"), text("final")) else {
        return Err(String::from("`draft` and `final` must be strings"));
    };
    let Some(utt) = value.get("utt").and_then(Value::as_u64) else {
        return Err(String::from("`utt` must be an unsigned integer"));
    };
    Ok(Pair {
        utt,
        draft,
        closing,
    })
}

/// Only the widened key may take the pair: a pair the earlier keys take does not
/// show what the widening adds.
fn widened_only(pair: &Pair) -> Result<(), String> {
    match reuse_match(&pair.closing, &pair.draft, true) {
        Some(ReuseMatch::Normalized) => Ok(()),
        Some(earlier) => Err(format!(
            "the existing key `{}` takes this pair already",
            earlier.as_str()
        )),
        None => Err(String::from("no key takes this pair")),
    }
}

/// The pairs of `text`, one JSON object per line; a line the widened key alone
/// takes, and no pair twice (same utterance, `final` and text in progress).
fn parse_pairs(text: &str) -> Result<Vec<Pair>, PairsFault> {
    let mut pairs: Vec<Pair> = Vec::new();
    let lines = text
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty());
    for (index, line) in lines {
        let fault = |what: String| PairsFault {
            line: Some(index + 1),
            what,
        };
        let pair = pair_of(line).map_err(fault)?;
        widened_only(&pair).map_err(fault)?;
        if pairs.contains(&pair) {
            return Err(fault(String::from("this pair is listed twice")));
        }
        pairs.push(pair);
    }
    Ok(pairs)
}

fn read_pairs(path: &Path) -> Result<Vec<Pair>, PairsFault> {
    let text = std::fs::read_to_string(path).map_err(|error| PairsFault {
        line: None,
        what: format!("{} was not read: {error}", path.display()),
    })?;
    parse_pairs(&text)
}

/// The repository's root: the crate sits two directories below it, in the
/// main tree and in a worktree alike.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn pairs_path() -> PathBuf {
    repo_root().join("docs/results/r8x/rp85_pairs.jsonl")
}

/// T-RP-85-018: the file holds the five pairs only the widened key takes.
#[test]
fn the_pairs_file_holds_only_pairs_the_widened_key_alone_matches() -> TestResult {
    let pairs = read_pairs(&pairs_path())?;
    let utterances: Vec<u64> = pairs.iter().map(|pair| pair.utt).collect();
    assert_eq!(utterances, [2, 5, 7, 11, 20]);
    for (draft, closing) in [
        (P7_DRAFT, P7_FINAL),
        (P11_DRAFT, P11_FINAL),
        (P20_DRAFT, P20_FINAL),
    ] {
        assert!(
            pairs
                .iter()
                .any(|pair| pair.draft == draft && pair.closing == closing),
            "{closing}"
        );
    }
    Ok(())
}

/// T-RP-85-019: a pair the earlier key takes is named by its line.
#[test]
fn a_pair_the_existing_key_matches_is_rejected_from_the_pairs_file() -> TestResult {
    let extra = json!({"utt": 3, "draft": "これは雨です", "final": "これは雨です。"});
    let fixture = format!("{}{extra}\n", std::fs::read_to_string(pairs_path())?);
    let Some(fault) = parse_pairs(&fixture).err() else {
        return Err("a pair of the existing key was accepted".into());
    };
    assert_eq!(fault.line, Some(6), "{fault}");
    assert!(fault.what.contains("closing_mark"), "{fault}");
    Ok(())
}

/// T-RP-85-020: the same pair twice is named by the line of the second.
#[test]
fn a_pair_listed_twice_is_rejected_from_the_pairs_file() -> TestResult {
    let text = std::fs::read_to_string(pairs_path())?;
    let first = text.lines().next().ok_or("the pairs file is empty")?;
    let fixture = format!("{text}{first}\n");
    let Some(fault) = parse_pairs(&fixture).err() else {
        return Err("a pair listed twice was accepted".into());
    };
    assert_eq!(fault.line, Some(6), "{fault}");
    assert!(fault.what.contains("twice"), "{fault}");
    Ok(())
}

/// T-RP-85-021: a file that is not there is named by its path.
#[test]
fn a_missing_pairs_file_is_rejected_by_path() {
    let path = repo_root().join("docs/results/r8x/no_such_pairs.jsonl");
    let fault = read_pairs(&path).err();
    assert!(
        fault.is_some_and(|fault| fault.line.is_none()
            && fault.what.contains("no_such_pairs.jsonl"))
    );
}

// ---- (4) how often V stops the answers -----------------------------------------------

/// One pair with the answers T gave for the text in progress and for the `final`.
struct Answered {
    closing: String,
    draft_answer: String,
    closing_answer: String,
}

/// How many pairs V stops, with the answer of the text in progress and with
/// the answer of the `final`; both judged against the `final`.
#[derive(Debug, PartialEq)]
struct Stops {
    draft_side: usize,
    closing_side: usize,
}

#[derive(Debug, PartialEq)]
enum ProbeFault {
    NoPairs,
    MoreStops(Stops),
}

impl std::fmt::Display for ProbeFault {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProbeFault::NoPairs => write!(out, "the probe was asked about no pair"),
            ProbeFault::MoreStops(stops) => write!(
                out,
                "V stops {} answers of the texts in progress and {} answers of the finals",
                stops.draft_side, stops.closing_side
            ),
        }
    }
}

impl std::error::Error for ProbeFault {}

fn stopped(verifier: &mut Verifier, closing: &str, answer: &str) -> bool {
    matches!(
        verifier.verify(closing, answer, Lang::Ja, &mut NullSink),
        Verdict::Held(_)
    )
}

fn stop_counts(verifier: &mut Verifier, answered: &[Answered]) -> Stops {
    let mut stops = Stops {
        draft_side: 0,
        closing_side: 0,
    };
    for pair in answered {
        stops.draft_side += usize::from(stopped(verifier, &pair.closing, &pair.draft_answer));
        stops.closing_side += usize::from(stopped(verifier, &pair.closing, &pair.closing_answer));
    }
    stops
}

/// The answers of the texts in progress may be stopped as often as the answers
/// of the finals, and no more often.
fn judge_stops(stops: Stops) -> Result<Stops, ProbeFault> {
    if stops.draft_side > stops.closing_side {
        Err(ProbeFault::MoreStops(stops))
    } else {
        Ok(stops)
    }
}

fn judge_probe(verifier: &mut Verifier, answered: &[Answered]) -> Result<Stops, ProbeFault> {
    if answered.is_empty() {
        return Err(ProbeFault::NoPairs);
    }
    judge_stops(stop_counts(verifier, answered))
}

fn damage_pair(draft_answer: &str) -> Answered {
    Answered {
        closing: String::from("被害は4件でした。"),
        draft_answer: draft_answer.to_owned(),
        closing_answer: String::from("There were 4 cases of damage"),
    }
}

/// T-RP-85-022: equal counts pass, more stops on the draft side do not.
#[test]
fn the_draft_stop_count_may_equal_but_not_exceed_the_final_stop_count() {
    for (draft_side, closing_side) in [(0, 0), (2, 2), (1, 2)] {
        let stops = Stops {
            draft_side,
            closing_side,
        };
        assert!(judge_stops(stops).is_ok(), "{draft_side}, {closing_side}");
    }
    for (draft_side, closing_side) in [(3, 2), (1, 0)] {
        let fault = judge_stops(Stops {
            draft_side,
            closing_side,
        })
        .err();
        let said = fault.map(|fault| fault.to_string());
        assert!(
            said.as_deref()
                .is_some_and(|text| text.contains(&draft_side.to_string())
                    && text.contains(&closing_side.to_string())),
            "{said:?}"
        );
    }
}

/// T-RP-85-023: the counter sees the answer of the draft that has another number.
#[test]
fn the_stop_counter_counts_a_draft_answer_with_another_number() -> TestResult {
    let mut verifier = Verifier::open(None, &mut NullSink)?;
    let other = judge_probe(&mut verifier, &[damage_pair(DAMAGE_ANSWER)]);
    assert_eq!(
        other,
        Err(ProbeFault::MoreStops(Stops {
            draft_side: 1,
            closing_side: 0
        }))
    );
    let same = judge_probe(
        &mut verifier,
        &[damage_pair("There were 4 cases of damage")],
    );
    assert_eq!(
        same,
        Ok(Stops {
            draft_side: 0,
            closing_side: 0
        })
    );
    Ok(())
}

/// T-RP-85-024: no pair is not "no more stops than".
#[test]
fn a_probe_over_no_pairs_is_rejected() -> TestResult {
    let mut verifier = Verifier::open(None, &mut NullSink)?;
    assert_eq!(judge_probe(&mut verifier, &[]), Err(ProbeFault::NoPairs));
    Ok(())
}

const LLAMA_SERVER: &str = "http://127.0.0.1:8080";

/// T's answer for `text`, asked of the llama-server like the relay asks it.
fn t_answer(llm: &HttpLlm, text: &str) -> Result<String, Box<dyn std::error::Error>> {
    let prompt = translate_prompt(Lang::Ja, text);
    match llm.complete(&Completion::new(&prompt, MAX_TOKENS)) {
        Ok(answer) => Ok(answer.trim().to_owned()),
        Err(error) => {
            Err(format!("the llama-server at {LLAMA_SERVER} did not answer: {error}").into())
        }
    }
}

/// T-RP-85-025: the probe over the pairs file, against the llama-server on 8080.
#[test]
#[ignore = "needs the llama-server with the translation model on port 8080"]
fn the_draft_answers_stop_no_more_often_than_the_final_answers() -> TestResult {
    let llm = HttpLlm::with_timeout(LLAMA_SERVER, Duration::from_secs(60));
    let mut answered = Vec::new();
    for pair in read_pairs(&pairs_path())? {
        answered.push(Answered {
            draft_answer: t_answer(&llm, &pair.draft)?,
            closing_answer: t_answer(&llm, &pair.closing)?,
            closing: pair.closing,
        });
    }
    let mut verifier = Verifier::open(None, &mut NullSink)?;
    let stops = judge_probe(&mut verifier, &answered)?;
    eprintln!(
        "pairs {}: V stops {} answers of the texts in progress (a), {} answers of the finals (b)",
        answered.len(),
        stops.draft_side,
        stops.closing_side
    );
    Ok(())
}

// ---- (5) the records of the runs of window E3' --------------------------------------------

const UTTERANCE: u64 = 7;
/// The translation stage of utterance 7 may not exceed this at its 95th percentile.
const STAGE_LIMIT_US: i64 = 182_000;

/// One run: its name, the samples (`<run>.e2e1.jsonl`) and the relay's stdout.
struct XRun<'a> {
    name: &'a str,
    samples: &'a str,
    relay_log: &'a str,
}

#[derive(Debug, PartialEq)]
enum Fault {
    NoNormalizedReuse { run: String },
    DroppedLines { run: String },
    NoUtterance7Sample,
    SlowUtterance7 { p95_us: i64 },
    Unreadable { run: String, what: String },
}

/// What a run that passed shows: how many samples of utterance 7 it holds and
/// their 95th percentile.
#[derive(Debug, PartialEq)]
struct XReport {
    samples: usize,
    p95_us: i64,
}

/// Microseconds as `183.456 ms`.
fn millis(us: i64) -> String {
    format!("{}.{:03} ms", us / 1000, (us % 1000).abs())
}

impl std::fmt::Display for Fault {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Fault::NoNormalizedReuse { run } => write!(
                out,
                "run {run} has no translate.draft.reused with match `normalized` (0 of them)"
            ),
            Fault::DroppedLines { run } => write!(
                out,
                "run {run} dropped log lines (log.queue.dropped) and is not read"
            ),
            Fault::NoUtterance7Sample => write!(
                out,
                "no sample of utterance {UTTERANCE} has a translation stage"
            ),
            Fault::SlowUtterance7 { p95_us } => write!(
                out,
                "the translation stage of utterance {UTTERANCE} is {p95_us} us ({}) at p95, over {} ms",
                millis(*p95_us),
                STAGE_LIMIT_US / 1000
            ),
            Fault::Unreadable { run, what } => write!(out, "run {run} was not read: {what}"),
        }
    }
}

fn describe(faults: &[Fault]) -> String {
    faults
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

/// What the relay's stdout holds that RP-85 reads.
struct LogCounts {
    normalized: usize,
    dropped: bool,
}

fn log_counts(relay_log: &str) -> Result<LogCounts, String> {
    let mut counts = LogCounts {
        normalized: 0,
        dropped: false,
    };
    let lines = relay_log
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty());
    for (index, line) in lines {
        let value: Value =
            serde_json::from_str(line).map_err(|error| format!("line {}: {error}", index + 1))?;
        match value.get("event").and_then(Value::as_str) {
            Some("log.queue.dropped") => counts.dropped = true,
            Some("translate.draft.reused")
                if value.get("match").and_then(Value::as_str) == Some("normalized") =>
            {
                counts.normalized += 1;
            }
            _ => {}
        }
    }
    Ok(counts)
}

/// The translation stage (`t_tr_us - t_final_us`) of each sample of
/// utterance 7 that has both times. The validity line `d >= 120 ms` is not
/// applied: the stage does not include `d`.
fn utterance_7_stages(samples: &str) -> Result<Vec<i64>, String> {
    let mut stages = Vec::new();
    let lines = samples
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty());
    for (index, line) in lines {
        let row: Value =
            serde_json::from_str(line).map_err(|error| format!("line {}: {error}", index + 1))?;
        let time = |key: &str| row.get(key).and_then(Value::as_i64);
        if row.get("utt").and_then(Value::as_u64) != Some(UTTERANCE) {
            continue;
        }
        if let (Some(final_us), Some(tr_us)) = (time("t_final_us"), time("t_tr_us")) {
            stages.push(tr_us - final_us);
        }
    }
    Ok(stages)
}

/// The nearest-rank 95th percentile; `None` for no sample.
fn p95(stages: &mut [i64]) -> Option<i64> {
    stages.sort_unstable();
    let rank = (95 * stages.len()).div_ceil(100);
    rank.checked_sub(1).and_then(|at| stages.get(at)).copied()
}

fn judge_log(run: &XRun<'_>, faults: &mut Vec<Fault>) {
    let name = run.name.to_owned();
    match log_counts(run.relay_log) {
        Ok(counts) => {
            if counts.dropped {
                faults.push(Fault::DroppedLines { run: name.clone() });
            }
            if counts.normalized == 0 {
                faults.push(Fault::NoNormalizedReuse { run: name });
            }
        }
        Err(what) => faults.push(Fault::Unreadable { run: name, what }),
    }
}

/// Every run reuses an answer under the widened key, and the translation stage
/// of utterance 7 over all runs stays within [`STAGE_LIMIT_US`] at p95.
fn check_x_runs(runs: &[XRun<'_>]) -> Result<XReport, Vec<Fault>> {
    let mut faults = Vec::new();
    let mut stages = Vec::new();
    for run in runs {
        judge_log(run, &mut faults);
        match utterance_7_stages(run.samples) {
            Ok(mut found) => stages.append(&mut found),
            Err(what) => faults.push(Fault::Unreadable {
                run: run.name.to_owned(),
                what,
            }),
        }
    }
    let samples = stages.len();
    let p95_us = p95(&mut stages);
    match p95_us {
        None => faults.push(Fault::NoUtterance7Sample),
        Some(p95_us) if p95_us > STAGE_LIMIT_US => faults.push(Fault::SlowUtterance7 { p95_us }),
        Some(_) => {}
    }
    match (p95_us, faults.is_empty()) {
        (Some(p95_us), true) => Ok(XReport { samples, p95_us }),
        _ => Err(faults),
    }
}

/// The text of a record file; the path when it cannot be read.
fn read_record(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))
}

/// The three files of a run, as `(name, samples, relay log)`.
type RunText = (String, String, String);

fn run_text(name: &str, samples: &Path, relay_log: &Path) -> Result<RunText, String> {
    Ok((
        name.to_owned(),
        read_record(samples)?,
        read_record(relay_log)?,
    ))
}

/// A run known only by its samples: the recorded run has no relay log, and
/// the check then finds no reuse in it, which is what 029 shows.
fn samples_only(name: &str, samples: &Path) -> Result<RunText, String> {
    Ok((name.to_owned(), read_record(samples)?, String::new()))
}

fn borrowed(texts: &[RunText]) -> Vec<XRun<'_>> {
    texts
        .iter()
        .map(|(name, samples, relay_log)| XRun {
            name,
            samples,
            relay_log,
        })
        .collect()
}

// ---- fixtures of the runs ----

/// A sample row of the run's `<run>.e2e1.jsonl`.
fn sample_row(utt: u64, final_us: Option<i64>, tr_us: Option<i64>) -> String {
    json!({"repeat": 1, "utt": utt, "t_final_us": final_us, "t_tr_us": tr_us}).to_string()
}

/// The samples of a run: one row of utterance 7 for each stage, and two rows
/// the check has to leave out (another utterance, and one with no translation).
fn samples_with_stages(stages_us: &[i64]) -> String {
    let mut rows: Vec<String> = stages_us
        .iter()
        .map(|stage| sample_row(UTTERANCE, Some(1_000_000), Some(1_000_000 + stage)))
        .collect();
    rows.push(sample_row(8, Some(1_000_000), Some(1_900_000)));
    rows.push(sample_row(UTTERANCE, Some(1_000_000), None));
    rows.join("\n")
}

/// The relay's stdout of a run: one `translate.draft.reused` for each `match`.
fn relay_log_with(matches: &[&str]) -> String {
    matches
        .iter()
        .enumerate()
        .map(|(index, matched)| {
            json!({
                "event": "translate.draft.reused", "id": format!("mic-{index}"),
                "match": matched, "state": "done", "waited_ms": 0, "verdict": "ok",
            })
            .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Two runs of five samples of utterance 7: the slowest stage is `slowest_us`.
fn two_runs(slowest_us: i64) -> (String, String) {
    let first = samples_with_stages(&[120_000; 5]);
    let second = samples_with_stages(&[120_000, 120_000, 120_000, 120_000, slowest_us]);
    (first, second)
}

fn pair_of_runs<'a>(samples: &'a (String, String), logs: &'a (String, String)) -> [XRun<'a>; 2] {
    [
        XRun {
            name: "e3p1",
            samples: &samples.0,
            relay_log: &logs.0,
        },
        XRun {
            name: "e3p2",
            samples: &samples.1,
            relay_log: &logs.1,
        },
    ]
}

fn reusing_logs() -> (String, String) {
    (
        relay_log_with(&["normalized", "normalized", "normalized", "same"]),
        relay_log_with(&["normalized"]),
    )
}

/// T-RP-85-026: utterance 7 at the line itself, and one reuse in each run.
#[test]
fn x_runs_with_a_reuse_each_and_utterance_7_at_182_ms_pass() {
    let samples = two_runs(182_000);
    let logs = reusing_logs();
    let report = check_x_runs(&pair_of_runs(&samples, &logs));
    assert_eq!(
        report,
        Ok(XReport {
            samples: 10,
            p95_us: 182_000
        })
    );
}

/// T-RP-85-027: one microsecond over the line.
#[test]
fn utterance_7_one_microsecond_over_182_ms_is_rejected() {
    let samples = two_runs(182_001);
    let logs = reusing_logs();
    let faults = check_x_runs(&pair_of_runs(&samples, &logs)).err();
    assert_eq!(
        faults,
        Some(vec![Fault::SlowUtterance7 { p95_us: 182_001 }])
    );
    let said = faults.map(|faults| describe(&faults));
    assert!(
        said.as_deref()
            .is_some_and(|text| text.contains("182001") && text.contains("182 ms")),
        "{said:?}"
    );
}

/// T-RP-85-028: a run whose reuses are all of the earlier keys.
#[test]
fn a_run_without_a_normalized_reuse_is_rejected() {
    let samples = two_runs(182_000);
    let logs = (
        relay_log_with(&["normalized"]),
        relay_log_with(&["same", "closing_mark", "same", "closing_mark"]),
    );
    let faults = check_x_runs(&pair_of_runs(&samples, &logs)).err();
    assert_eq!(
        faults,
        Some(vec![Fault::NoNormalizedReuse {
            run: String::from("e3p2")
        }])
    );
}

/// T-RP-85-029: the recorded run, with no reuse at all, is over the line.
#[test]
fn the_recorded_utterance_7_is_over_182_ms_without_a_reuse() -> TestResult {
    let dir = repo_root().join("docs/results/rp32");
    let mut texts = Vec::new();
    for name in ["rs1", "rs2", "rs3"] {
        texts.push(samples_only(name, &dir.join(format!("{name}.jsonl")))?);
    }
    let Some(faults) = check_x_runs(&borrowed(&texts)).err() else {
        return Err("the recorded run passed".into());
    };
    let said = describe(&faults);
    assert!(
        faults.contains(&Fault::SlowUtterance7 { p95_us: 372_760 }),
        "{said}"
    );
    assert!(said.contains("372.760 ms"), "{said}");
    assert!(said.contains("`normalized` (0 of them)"), "{said}");
    Ok(())
}

/// T-RP-85-030: a run whose samples of utterance 7 have no translation stage.
#[test]
fn an_x_run_without_utterance_7_samples_is_rejected() {
    let samples = (samples_with_stages(&[]), samples_with_stages(&[]));
    let logs = reusing_logs();
    let faults = check_x_runs(&pair_of_runs(&samples, &logs)).err();
    assert_eq!(faults, Some(vec![Fault::NoUtterance7Sample]));
}

/// T-RP-85-031: the two Japanese runs of window E3', read from `docs/results/r8x/`.
#[test]
#[ignore = "reads the records of window E3', which are put in docs/results/r8x/ after the window"]
fn rp85_a_draft_differing_only_in_numerals_or_inner_marks_is_reused_and_checked_by_v() -> TestResult
{
    let dir = repo_root().join("docs/results/r8x");
    let mut texts = Vec::new();
    for name in ["e3p1", "e3p2"] {
        let samples = dir.join(format!("{name}.e2e1.jsonl"));
        let relay_log = dir.join(format!("{name}.e2e1.relay.stdout.log"));
        texts.push(run_text(name, &samples, &relay_log)?);
    }
    let report = check_x_runs(&borrowed(&texts)).map_err(|faults| describe(&faults))?;
    eprintln!(
        "utterance {UTTERANCE}: {} samples, translation stage {} at p95",
        report.samples,
        millis(report.p95_us)
    );
    Ok(())
}

/// T-RP-85-032 (0): V passes the scripted answer of utterance 7 against the
/// `final`'s own text. No recognizer and no translator are involved.
#[test]
fn v_passes_the_scripted_answer_for_utterance_7() -> TestResult {
    let mut verifier = Verifier::open(None, &mut NullSink)?;
    let verdict = verifier.verify(P7_FINAL, P7_ANSWER, Lang::Ja, &mut NullSink);
    assert_eq!(verdict, Verdict::Ok, "{verdict:?}");
    Ok(())
}

/// T-RP-85-033: a run whose log dropped lines cannot be tied to its samples.
#[test]
fn a_run_whose_log_dropped_lines_is_rejected() {
    let samples = two_runs(182_000);
    let dropped = json!({"event": "log.queue.dropped", "level": "warn", "dropped": 3});
    let logs = (
        format!("{}\n{dropped}", relay_log_with(&["normalized"])),
        relay_log_with(&["normalized"]),
    );
    let faults = check_x_runs(&pair_of_runs(&samples, &logs)).err();
    assert_eq!(
        faults,
        Some(vec![Fault::DroppedLines {
            run: String::from("e3p1")
        }])
    );
}

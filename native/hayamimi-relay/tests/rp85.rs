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
//!   llama-server (port 18084) and runs with `--ignored`.
//! - 026 to 031 and 034 to 051 judge the records of the runs of window E3''
//!   (utterance 7). The samples are split by the product's own key
//!   (`reuse_match`) into those it matches and those it does not; only the
//!   matched ones are held to the translation-stage line. The records are made
//!   from formulas in this file; only 031 reads the recorded runs and runs
//!   with `--ignored`.
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

const LLAMA_SERVER: &str = "http://127.0.0.1:18084";

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

/// T-RP-85-025: the probe over the pairs file, against the llama-server on 18084.
#[test]
#[ignore = "needs the llama-server with the translation model on port 18084"]
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

// ---- (5) the records of the runs of window E3'' -------------------------------------------

const UTTERANCE: u64 = 7;
/// The translation stage of a sample whose pair the product key matches may not exceed this.
const STAGE_LIMIT_US: i64 = 182_000;
/// Each run needs at least this many such samples: a majority of the five takes.
const MIN_KEYED: usize = 3;
/// The runs of the window are named `<RUN_PREFIX><N>`; a record of any other name is not read.
const RUN_PREFIX: &str = "e3q";
/// The runs the recorded-run test reads.
const RECORDED_RUNS: [&str; 2] = ["e3q1", "e3q2"];

/// One run: its name and the text of its three records.
struct XRun {
    name: String,
    /// `<run>.e2e1.jsonl`
    samples: String,
    /// `<run>.e2e1.events.jsonl`
    events: String,
    /// `<run>.e2e1.relay.stdout.log`
    relay_log: String,
}

/// Whether `name` is `<RUN_PREFIX>` followed by digits only.
fn is_window_run(name: &str) -> bool {
    name.strip_prefix(RUN_PREFIX)
        .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_digit()))
}

#[derive(Debug, PartialEq)]
enum Fault {
    NoWindowRun,
    NoNormalizedReuse {
        run: String,
    },
    DroppedLines {
        run: String,
    },
    NoUtterance7Sample {
        run: String,
    },
    UnjoinedSample {
        run: String,
        repeat: u64,
        final_us: i64,
    },
    KeyedWithoutReuse {
        run: String,
        repeat: u64,
        id: String,
        stage_us: i64,
    },
    SlowKeyed {
        run: String,
        repeat: u64,
        id: String,
        stage_us: i64,
    },
    TooFewKeyed {
        run: String,
        keyed: usize,
    },
    Unreadable {
        run: String,
        what: String,
    },
}

impl Fault {
    /// The run the fault is about; none for a fault about the set of runs.
    fn run(&self) -> Option<&str> {
        match self {
            Fault::NoWindowRun => None,
            Fault::NoNormalizedReuse { run }
            | Fault::DroppedLines { run }
            | Fault::NoUtterance7Sample { run }
            | Fault::UnjoinedSample { run, .. }
            | Fault::KeyedWithoutReuse { run, .. }
            | Fault::SlowKeyed { run, .. }
            | Fault::TooFewKeyed { run, .. }
            | Fault::Unreadable { run, .. } => Some(run),
        }
    }
}

/// Microseconds as `183.456 ms`.
fn millis(us: i64) -> String {
    format!("{}.{:03} ms", us / 1000, (us % 1000).abs())
}

impl std::fmt::Display for Fault {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Fault::NoWindowRun => write!(out, "no run named {RUN_PREFIX}<N> was given"),
            Fault::NoNormalizedReuse { run } => write!(
                out,
                "run {run} has no translate.draft.reused with match `normalized` (0 of them)"
            ),
            Fault::DroppedLines { run } => write!(
                out,
                "run {run} dropped log lines (log.queue.dropped) and is not read"
            ),
            Fault::NoUtterance7Sample { run } => write!(
                out,
                "run {run} has no sample of utterance {UTTERANCE} with a translation stage"
            ),
            Fault::UnjoinedSample {
                run,
                repeat,
                final_us,
            } => write!(
                out,
                "run {run}: sample {} t_final_us {final_us} has no final in the events at that time",
                sample_name(*repeat)
            ),
            Fault::KeyedWithoutReuse {
                run,
                repeat,
                id,
                stage_us,
            } => write!(
                out,
                "run {run}: sample {} {id} (stage {}) is matched by the product key but has no translate.draft.reused of that id",
                sample_name(*repeat),
                millis(*stage_us)
            ),
            Fault::SlowKeyed {
                run,
                repeat,
                id,
                stage_us,
            } => write!(
                out,
                "run {run}: the translation stage of the matched sample {} {id} is {stage_us} us ({}), over {} ms",
                sample_name(*repeat),
                millis(*stage_us),
                STAGE_LIMIT_US / 1000
            ),
            Fault::TooFewKeyed { run, keyed } => write!(
                out,
                "run {run} has {keyed} keyed samples of utterance {UTTERANCE}, fewer than {MIN_KEYED}"
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

// ---- reading the three records of a run ----

/// The non-blank lines of a JSON-lines record as values. The first line that
/// does not parse is named by its number.
fn json_lines(text: &str) -> Result<Vec<Value>, String> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| {
            serde_json::from_str(line).map_err(|error| format!("line {}: {error}", index + 1))
        })
        .collect()
}

/// What the relay's stdout holds that RP-85 reads.
struct LogCounts {
    /// How many `translate.draft.reused` lines have the match `normalized`.
    normalized: usize,
    dropped: bool,
    /// The `id` of every `translate.draft.reused` line.
    reused: Vec<String>,
}

fn note_reuse(row: &Value, counts: &mut LogCounts) {
    if row.get("match").and_then(Value::as_str) == Some("normalized") {
        counts.normalized += 1;
    }
    if let Some(id) = row.get("id").and_then(Value::as_str) {
        counts.reused.push(id.to_owned());
    }
}

fn log_counts(relay_log: &str) -> Result<LogCounts, String> {
    let mut counts = LogCounts {
        normalized: 0,
        dropped: false,
        reused: Vec::new(),
    };
    for row in json_lines(relay_log)? {
        match row.get("event").and_then(Value::as_str) {
            Some("log.queue.dropped") => counts.dropped = true,
            Some("translate.draft.reused") => note_reuse(&row, &mut counts),
            _ => {}
        }
    }
    Ok(counts)
}

/// How every fault and the listing name a sample: by its repeat number.
fn sample_name(repeat: u64) -> String {
    format!("utt {UTTERANCE} repeat {repeat}")
}

/// A sample of utterance 7 that has both times. The validity line `d >= 120 ms`
/// is not applied: the stage does not include `d`.
struct Sample {
    /// The row's `repeat`. A sample is named by it; the row's `id`, if it has
    /// one, is not read.
    repeat: u64,
    final_us: i64,
    /// `t_tr_us - t_final_us`
    stage_us: i64,
}

fn sample_of(row: &Value, record: usize) -> Result<Option<Sample>, String> {
    if row.get("utt").and_then(Value::as_u64) != Some(UTTERANCE) {
        return Ok(None);
    }
    let time = |key: &str| row.get(key).and_then(Value::as_i64);
    let (Some(final_us), Some(tr_us)) = (time("t_final_us"), time("t_tr_us")) else {
        return Ok(None);
    };
    let Some(repeat) = row.get("repeat").and_then(Value::as_u64) else {
        return Err(format!(
            "record {record}: the sample with t_final_us {final_us} has no repeat that is a JSON integer"
        ));
    };
    Ok(Some(Sample {
        repeat,
        final_us,
        stage_us: tr_us - final_us,
    }))
}

fn utterance_7_samples(samples: &str) -> Result<Vec<Sample>, String> {
    let mut found = Vec::new();
    for (index, row) in json_lines(samples)?.iter().enumerate() {
        found.extend(sample_of(row, index + 1)?);
    }
    Ok(found)
}

/// Whether a recognizer event is a text in progress or a `final` with its id.
enum Said {
    Partial,
    Final(String),
}

/// A `partial` or `final` event of the recognizer's record.
struct Heard {
    t_us: i64,
    text: String,
    said: Said,
}

impl Heard {
    fn is_final(&self) -> bool {
        matches!(self.said, Said::Final(_))
    }
}

fn heard_of(row: &Value, record: usize) -> Result<Option<Heard>, String> {
    let Some(event) = row.get("event") else {
        return Ok(None);
    };
    let kind = match event.get("type").and_then(Value::as_str) {
        Some(kind @ ("partial" | "final")) => kind,
        _ => return Ok(None),
    };
    let broken = |what: &str| format!("record {record}: a {kind} event has no {what}");
    let Some(t_us) = row.get("t_us").and_then(Value::as_i64) else {
        return Err(broken("t_us"));
    };
    let Some(text) = event.get("text").and_then(Value::as_str) else {
        return Err(broken("text"));
    };
    let said = if kind == "final" {
        let Some(id) = event.get("id").and_then(Value::as_str) else {
            return Err(broken("id"));
        };
        Said::Final(id.to_owned())
    } else {
        Said::Partial
    };
    Ok(Some(Heard {
        t_us,
        text: text.to_owned(),
        said,
    }))
}

fn heard_events(events: &str) -> Result<Vec<Heard>, String> {
    let mut found = Vec::new();
    for (index, row) in json_lines(events)?.iter().enumerate() {
        found.extend(heard_of(row, index + 1)?);
    }
    Ok(found)
}

// ---- splitting the samples by the product's key ----

/// Why the product key does not match a sample.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reason {
    /// There was a text in progress and `reuse_match` returned `None` for it.
    Key,
    /// No `partial` came between the previous `final` and this one.
    NoDraft,
}

impl std::fmt::Display for Reason {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str(match self {
            Reason::Key => "key",
            Reason::NoDraft => "no_draft",
        })
    }
}

#[derive(Debug, PartialEq)]
struct Keyed {
    repeat: u64,
    /// The `id` of the `final` the sample is joined to.
    id: String,
    stage_us: i64,
}

#[derive(Debug, PartialEq)]
struct Unmatched {
    repeat: u64,
    id: String,
    stage_us: i64,
    closing: String,
    draft: Option<String>,
    reason: Reason,
}

#[derive(Debug, PartialEq)]
enum Split {
    Keyed(Keyed),
    Unmatched(Unmatched),
}

/// The `id` and text of the `final` stamped `final_us`.
fn final_at(heard: &[Heard], final_us: i64) -> Option<(&str, &str)> {
    heard.iter().find_map(|event| match &event.said {
        Said::Final(id) if event.t_us == final_us => Some((id.as_str(), event.text.as_str())),
        _ => None,
    })
}

/// The text in progress when the `final` at `final_us` came: the last
/// `partial` after the previous `final` and before this one.
fn last_draft(heard: &[Heard], final_us: i64) -> Option<&str> {
    let previous = heard
        .iter()
        .filter(|event| event.is_final() && event.t_us < final_us)
        .map(|event| event.t_us)
        .max();
    heard
        .iter()
        .filter(|event| !event.is_final() && event.t_us < final_us)
        .filter(|event| previous.is_none_or(|stamp| event.t_us > stamp))
        .max_by_key(|event| event.t_us)
        .map(|event| event.text.as_str())
}

/// Splits one sample by the product's own key. `None` when the events hold no
/// `final` stamped with the sample's `t_final_us`.
fn classify(sample: &Sample, heard: &[Heard]) -> Option<Split> {
    let (id, closing) = final_at(heard, sample.final_us)?;
    let draft = last_draft(heard, sample.final_us);
    let id = id.to_owned();
    let stage_us = sample.stage_us;
    let repeat = sample.repeat;
    if draft.is_some_and(|draft| reuse_match(closing, draft, true).is_some()) {
        return Some(Split::Keyed(Keyed {
            repeat,
            id,
            stage_us,
        }));
    }
    let reason = if draft.is_some() {
        Reason::Key
    } else {
        Reason::NoDraft
    };
    Some(Split::Unmatched(Unmatched {
        repeat,
        id,
        stage_us,
        closing: closing.to_owned(),
        draft: draft.map(str::to_owned),
        reason,
    }))
}

// ---- judging a run ----

/// How one run's samples split.
struct RunReport {
    run: String,
    keyed: Vec<Keyed>,
    unmatched: Vec<Unmatched>,
}

/// The faults of the set of runs, and how each run's samples split.
struct Judgement {
    faults: Vec<Fault>,
    runs: Vec<RunReport>,
}

/// What a run's three records hold once parsed.
struct Read {
    counts: LogCounts,
    samples: Vec<Sample>,
    heard: Vec<Heard>,
}

fn read_run(run: &XRun) -> Result<Read, String> {
    let named = |file: &'static str| move |what: String| format!("{file}: {what}");
    Ok(Read {
        counts: log_counts(&run.relay_log).map_err(named("relay log"))?,
        samples: utterance_7_samples(&run.samples).map_err(named("samples"))?,
        heard: heard_events(&run.events).map_err(named("events"))?,
    })
}

fn judge_log(run: &str, counts: &LogCounts, faults: &mut Vec<Fault>) {
    if counts.dropped {
        faults.push(Fault::DroppedLines {
            run: run.to_owned(),
        });
    }
    if counts.normalized == 0 {
        faults.push(Fault::NoNormalizedReuse {
            run: run.to_owned(),
        });
    }
}

/// Splits every sample of the run. A sample with no `final` at its time
/// and a matched sample with no reuse of its id are faults.
fn split_samples(run: &str, read: &Read, faults: &mut Vec<Fault>) -> RunReport {
    let mut report = RunReport {
        run: run.to_owned(),
        keyed: Vec::new(),
        unmatched: Vec::new(),
    };
    for sample in &read.samples {
        match classify(sample, &read.heard) {
            None => faults.push(Fault::UnjoinedSample {
                run: run.to_owned(),
                repeat: sample.repeat,
                final_us: sample.final_us,
            }),
            Some(Split::Unmatched(unmatched)) => report.unmatched.push(unmatched),
            Some(Split::Keyed(keyed)) => {
                if !read.counts.reused.contains(&keyed.id) {
                    faults.push(Fault::KeyedWithoutReuse {
                        run: run.to_owned(),
                        repeat: keyed.repeat,
                        id: keyed.id.clone(),
                        stage_us: keyed.stage_us,
                    });
                }
                report.keyed.push(keyed);
            }
        }
    }
    report
}

/// The matched samples are enough in number and none is over the stage line.
/// The line is held to the slowest one: with at most ten samples the
/// nearest-rank 95th percentile is the maximum.
fn judge_keyed(report: &RunReport, faults: &mut Vec<Fault>) {
    let over = report
        .keyed
        .iter()
        .max_by_key(|keyed| keyed.stage_us)
        .filter(|slowest| slowest.stage_us > STAGE_LIMIT_US);
    if let Some(slowest) = over {
        faults.push(Fault::SlowKeyed {
            run: report.run.clone(),
            repeat: slowest.repeat,
            id: slowest.id.clone(),
            stage_us: slowest.stage_us,
        });
    }
    if report.keyed.len() < MIN_KEYED {
        faults.push(Fault::TooFewKeyed {
            run: report.run.clone(),
            keyed: report.keyed.len(),
        });
    }
}

fn judge_run(run: &XRun, judgement: &mut Judgement) {
    let name = run.name.as_str();
    let read = match read_run(run) {
        Ok(read) => read,
        Err(what) => {
            judgement.faults.push(Fault::Unreadable {
                run: name.to_owned(),
                what,
            });
            return;
        }
    };
    judge_log(name, &read.counts, &mut judgement.faults);
    if read.samples.is_empty() {
        judgement.faults.push(Fault::NoUtterance7Sample {
            run: name.to_owned(),
        });
        return;
    }
    let report = split_samples(name, &read, &mut judgement.faults);
    judge_keyed(&report, &mut judgement.faults);
    judgement.runs.push(report);
}

/// Judges the runs of the window; a run of any other name is left unread.
fn check_x_runs(runs: &[XRun]) -> Judgement {
    let mut judgement = Judgement {
        faults: Vec::new(),
        runs: Vec::new(),
    };
    let window: Vec<&XRun> = runs.iter().filter(|run| is_window_run(&run.name)).collect();
    if window.is_empty() {
        judgement.faults.push(Fault::NoWindowRun);
    }
    for run in window {
        judge_run(run, &mut judgement);
    }
    judgement
}

// ---- the output of the judgement ----

/// One line for each run and one for each sample the key does not match.
fn listing(reports: &[&RunReport]) -> String {
    let mut lines = Vec::new();
    for report in reports {
        let slowest = report.keyed.iter().map(|keyed| keyed.stage_us).max();
        lines.push(format!(
            "{} keyed {} slowest {} unmatched {}",
            report.run,
            report.keyed.len(),
            slowest.map_or_else(|| String::from("-"), millis),
            report.unmatched.len()
        ));
        for one in &report.unmatched {
            lines.push(format!(
                "{} unmatched {} {} stage {} reason {} final {:?} draft {:?}",
                report.run,
                sample_name(one.repeat),
                one.id,
                millis(one.stage_us),
                one.reason,
                one.closing,
                one.draft
            ));
        }
    }
    lines.join("\n")
}

/// The listing of every run when the judgement passed. When it did not, the
/// faults, then the listing of the runs they name.
fn verdict(judgement: &Judgement) -> Result<String, String> {
    if judgement.faults.is_empty() {
        let all: Vec<&RunReport> = judgement.runs.iter().collect();
        return Ok(listing(&all));
    }
    let named: Vec<&RunReport> = judgement
        .runs
        .iter()
        .filter(|report| {
            judgement
                .faults
                .iter()
                .any(|fault| fault.run() == Some(report.run.as_str()))
        })
        .collect();
    Err(format!(
        "{}\n{}",
        describe(&judgement.faults),
        listing(&named)
    ))
}

/// The text of a record file; the path when it cannot be read.
fn read_record(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))
}

/// The path of one record of a run: `<dir>/<run>.e2e1.<suffix>`.
fn record_path(dir: &Path, run: &str, suffix: &str) -> PathBuf {
    dir.join(format!("{run}.e2e1.{suffix}"))
}

/// The three records of a run from `dir`. A record that cannot be read is
/// named by its path.
fn load_run(dir: &Path, name: &str) -> Result<XRun, String> {
    Ok(XRun {
        name: name.to_owned(),
        samples: read_record(&record_path(dir, name, "jsonl"))?,
        events: read_record(&record_path(dir, name, "events.jsonl"))?,
        relay_log: read_record(&record_path(dir, name, "relay.stdout.log"))?,
    })
}

// ---- fixtures of the runs (F85q) ----
//
// A made run holds five takes of utterance 7. Takes 1, 2, 3 and 5 close with
// the recorded final, which the product key matches against the recorded
// draft; take 4 closes with the final whose `連合国は` is `連合国が`, which it
// does not. The records are written from these values, so a test changes a
// value and the three records follow.

const TAKE_GAP_US: i64 = 10_000_000;
const KEYED_STAGE_US: i64 = 120_000;
const UNKEYED_STAGE_US: i64 = 418_540;
/// How long before its `final` the last text in progress of a take came.
const DRAFT_LEAD_US: i64 = 500_000;
/// How long before its `final` an earlier, shorter text in progress came.
const EARLY_LEAD_US: i64 = 2_000_000;
/// The earlier text in progress: a prefix of the recorded draft.
const EARLY_DRAFT: &str = "千九百四十年";

/// The recorded final with `連合国は` written `連合国が`: the product key does
/// not match it against the recorded draft.
fn unkeyed_final() -> String {
    P7_FINAL.replace("連合国は", "連合国が")
}

struct Take {
    id: String,
    final_us: i64,
    /// The last text in progress before the final; none when no `partial` came.
    draft: Option<String>,
    closing: String,
    /// `t_tr_us - t_final_us`; none for a take that was not translated.
    stage_us: Option<i64>,
    /// `d` in ms: the sample's `(t_final_us - t_end_us) / 1000 - final_latency_ms`.
    d_ms: i64,
    /// Whether the events hold the `final` of the take.
    final_kept: bool,
    /// Another `final` of the take's id with its own time and text, written
    /// after the real one.
    other_final: Option<(i64, String)>,
}

fn take(n: i64, keyed: bool) -> Take {
    Take {
        id: format!("mic-7{n}"),
        final_us: n * TAKE_GAP_US,
        draft: Some(P7_DRAFT.to_owned()),
        closing: if keyed {
            P7_FINAL.to_owned()
        } else {
            unkeyed_final()
        },
        stage_us: Some(if keyed {
            KEYED_STAGE_US
        } else {
            UNKEYED_STAGE_US
        }),
        d_ms: 500,
        final_kept: true,
        other_final: None,
    }
}

struct MadeRun {
    name: String,
    takes: Vec<Take>,
    /// `(id, match)` of each `translate.draft.reused` of the relay's stdout.
    reuses: Vec<(String, String)>,
    /// Further lines of the relay's stdout.
    extra_log: Vec<String>,
}

/// The made run of window E3'' with the given name. Besides the reuses of the
/// matched takes, the relay's stdout holds one of another utterance (`mic-31`).
fn made_run(name: &str) -> MadeRun {
    let keyed = |n: i64| n != 4;
    let mut reuses: Vec<(String, String)> = (1..=5)
        .filter(|n| keyed(*n))
        .map(|n| (format!("mic-7{n}"), String::from("normalized")))
        .collect();
    reuses.push((String::from("mic-31"), String::from("normalized")));
    MadeRun {
        name: name.to_owned(),
        takes: (1..=5).map(|n| take(n, keyed(n))).collect(),
        reuses,
        extra_log: Vec::new(),
    }
}

/// The made runs `e3q1` and `e3q2`.
fn made_pair() -> (MadeRun, MadeRun) {
    (made_run("e3q1"), made_run("e3q2"))
}

/// One `partial` or `final` row of a recognizer's events.
fn event_line(kind: &str, id: Option<&str>, text: &str, t_us: i64) -> String {
    let mut event = json!({"type": kind, "text": text});
    if let Some(id) = id {
        event["id"] = json!(id);
    }
    json!({"event": event, "t_us": t_us}).to_string()
}

impl MadeRun {
    fn take_mut(&mut self, n: usize) -> Result<&mut Take, String> {
        self.takes
            .get_mut(n.wrapping_sub(1))
            .ok_or_else(|| format!("{} has no take {n}", self.name))
    }

    /// Removes the reuse of the take with this `id`.
    fn drop_reuse(&mut self, id: &str) {
        self.reuses.retain(|(reused, _)| reused != id);
    }

    /// Takes `n` close with a final the key does not match, at `stage_us`, and
    /// the relay does not reuse its draft.
    fn unmatch(&mut self, n: usize, stage_us: i64) -> Result<(), String> {
        let take = self.take_mut(n)?;
        take.closing = unkeyed_final();
        take.stage_us = Some(stage_us);
        let id = take.id.clone();
        self.drop_reuse(&id);
        Ok(())
    }

    fn sample_rows(&self) -> String {
        let rows: Vec<String> = self
            .takes
            .iter()
            .zip(1..)
            .map(|(take, repeat)| {
                json!({
                    "repeat": repeat, "utt": UTTERANCE,
                    "t_end_us": take.final_us - (take.d_ms + 100) * 1000,
                    "t_final_us": take.final_us,
                    "t_tr_us": take.stage_us.map(|stage| take.final_us + stage),
                    "final_latency_ms": 100.0,
                })
                .to_string()
            })
            .collect();
        rows.join("\n")
    }

    /// The events of one take: the earlier and the last text in progress, the
    /// final, another final, and a row of a kind the check leaves out.
    fn take_events(take: &Take, rows: &mut Vec<String>) {
        if let Some(draft) = &take.draft {
            let early = take.final_us - EARLY_LEAD_US;
            rows.push(event_line("partial", None, EARLY_DRAFT, early));
            let last = take.final_us - DRAFT_LEAD_US;
            rows.push(event_line("partial", None, draft, last));
        }
        if take.final_kept {
            let closing = event_line("final", Some(&take.id), &take.closing, take.final_us);
            rows.push(closing);
        }
        if let Some((t_us, text)) = &take.other_final {
            rows.push(event_line("final", Some(&take.id), text, *t_us));
        }
        let speaker =
            json!({"event": {"type": "speaker", "id": take.id}, "t_us": take.final_us + 1000});
        rows.push(speaker.to_string());
    }

    fn event_rows(&self) -> String {
        let ready = json!({"event": {"sr": 16000, "type": "ready"}, "t_us": 16_093});
        let mut rows = vec![ready.to_string()];
        for take in &self.takes {
            MadeRun::take_events(take, &mut rows);
        }
        rows.join("\n")
    }

    fn log_rows(&self) -> String {
        let mut rows: Vec<String> = self
            .reuses
            .iter()
            .map(|(id, matched)| {
                json!({
                    "event": "translate.draft.reused", "id": id, "match": matched,
                    "source": "mic", "state": "done", "waited_ms": 0, "verdict": "ok",
                })
                .to_string()
            })
            .collect();
        rows.extend(self.extra_log.iter().cloned());
        rows.join("\n")
    }

    fn texts(&self) -> XRun {
        XRun {
            name: self.name.clone(),
            samples: self.sample_rows(),
            events: self.event_rows(),
            relay_log: self.log_rows(),
        }
    }

    /// Writes the three records into `dir`, leaving out the events when asked.
    fn write_to(&self, dir: &Path, with_events: bool) -> std::io::Result<()> {
        let texts = self.texts();
        std::fs::write(record_path(dir, &self.name, "jsonl"), texts.samples)?;
        if with_events {
            std::fs::write(record_path(dir, &self.name, "events.jsonl"), texts.events)?;
        }
        std::fs::write(
            record_path(dir, &self.name, "relay.stdout.log"),
            texts.relay_log,
        )
    }
}

/// Judges two made runs. The same runs with an `id` of `mic-99` written into
/// every sample row must be judged the same, and `mic-99` must not appear.
fn judged(first: &MadeRun, second: &MadeRun) -> Judgement {
    let runs = [first.texts(), second.texts()];
    let judgement = check_x_runs(&runs);
    let marked: Result<Vec<XRun>, String> = runs
        .iter()
        .map(|run| changed_sample_rows(run, |row| row["id"] = json!("mic-99")))
        .collect();
    let Ok(marked) = marked else {
        panic!("the sample rows were not rewritten");
    };
    let with_id = check_x_runs(&marked);
    assert_eq!(with_id.faults, judgement.faults, "an id in a sample row");
    let (plain, said) = (verdict(&judgement), verdict(&with_id));
    assert_eq!(plain, said, "an id in a sample row");
    assert!(!format!("{said:?}").contains("mic-99"), "{said:?}");
    judgement
}

/// The text of a judgement that did not pass; an error when it passed.
fn refused(judgement: &Judgement) -> Result<String, Box<dyn std::error::Error>> {
    match verdict(judgement) {
        Ok(listing) => Err(format!("the judgement passed:\n{listing}").into()),
        Err(said) => Ok(said),
    }
}

/// Whether one line of `text` holds every one of `parts`.
fn has_line_with(text: &str, parts: &[&str]) -> bool {
    text.lines()
        .any(|line| parts.iter().all(|part| line.contains(part)))
}

fn assert_passes(judgement: &Judgement) {
    assert_eq!(judgement.faults, Vec::<Fault>::new());
}

fn fault_keyed_without_reuse(run: &str, repeat: u64, id: &str, stage_us: i64) -> Fault {
    Fault::KeyedWithoutReuse {
        run: run.to_owned(),
        repeat,
        id: id.to_owned(),
        stage_us,
    }
}

fn run_report<'a>(judgement: &'a Judgement, run: &str) -> Result<&'a RunReport, String> {
    judgement
        .runs
        .iter()
        .find(|report| report.run == run)
        .ok_or_else(|| format!("no report for {run}"))
}

fn unmatched_ids(report: &RunReport) -> Vec<&str> {
    report.unmatched.iter().map(|one| one.id.as_str()).collect()
}

/// T-RP-85-026: the slowest matched sample is at the line itself, one sample
/// is not matched, and each run holds a reuse of the earlier kind and of the
/// new kind.
#[test]
fn x_runs_with_a_reuse_each_and_utterance_7_at_182_ms_pass() -> TestResult {
    let (mut first, mut second) = made_pair();
    first.take_mut(3)?.stage_us = Some(STAGE_LIMIT_US);
    second.take_mut(3)?.stage_us = Some(STAGE_LIMIT_US);
    let judgement = judged(&first, &second);
    assert_passes(&judgement);
    for name in ["e3q1", "e3q2"] {
        let report = run_report(&judgement, name)?;
        assert_eq!(report.keyed.len(), 4, "{name}");
        let unmatched = Unmatched {
            repeat: 4,
            id: String::from("mic-74"),
            stage_us: 418_540,
            closing: unkeyed_final(),
            draft: Some(P7_DRAFT.to_owned()),
            reason: Reason::Key,
        };
        assert_eq!(report.unmatched, [unmatched], "{name}");
    }
    let listing = verdict(&judgement)?;
    for name in ["e3q1", "e3q2"] {
        let parts = [name, "unmatched", "mic-74", "418.540 ms", "key"];
        assert!(has_line_with(&listing, &parts), "{listing}");
    }
    Ok(())
}

/// T-RP-85-027: one microsecond over the line.
#[test]
fn utterance_7_one_microsecond_over_182_ms_is_rejected() -> TestResult {
    let (mut first, mut second) = made_pair();
    first.take_mut(3)?.stage_us = Some(STAGE_LIMIT_US + 1);
    second.take_mut(3)?.stage_us = Some(STAGE_LIMIT_US);
    let judgement = judged(&first, &second);
    let slow = Fault::SlowKeyed {
        run: String::from("e3q1"),
        repeat: 3,
        id: String::from("mic-73"),
        stage_us: 182_001,
    };
    assert_eq!(judgement.faults, [slow]);
    let said = refused(&judgement)?;
    for part in ["e3q1", "mic-73", "182001", "182 ms"] {
        assert!(said.contains(part), "{part}: {said}");
    }
    assert!(!said.contains("e3q2"), "{said}");
    Ok(())
}

/// T-RP-85-028: a run whose reuses are all of the earlier keys.
#[test]
fn a_run_without_a_normalized_reuse_is_rejected() -> TestResult {
    let (first, mut second) = made_pair();
    for (_, matched) in &mut second.reuses {
        *matched = String::from("same");
    }
    let judgement = judged(&first, &second);
    let none = Fault::NoNormalizedReuse {
        run: String::from("e3q2"),
    };
    assert_eq!(judgement.faults, [none]);
    let said = refused(&judgement)?;
    assert!(said.contains("e3q2") && said.contains("`normalized` (0 of them)"));
    assert!(!said.contains("e3q1"), "{said}");
    for id in ["mic-71", "mic-72", "mic-73", "mic-75"] {
        assert!(!said.contains(id), "{id}: {said}");
    }
    Ok(())
}

/// T-RP-85-029: no reuse at all in either run. The matched takes are named;
/// the take the key does not match is not.
#[test]
fn the_recorded_utterance_7_is_over_182_ms_without_a_reuse() -> TestResult {
    let (mut first, mut second) = made_pair();
    first.reuses.clear();
    second.reuses.clear();
    let judgement = judged(&first, &second);
    let mut expected = Vec::new();
    for run in ["e3q1", "e3q2"] {
        expected.push(Fault::NoNormalizedReuse {
            run: run.to_owned(),
        });
        for (repeat, id) in [(1, "mic-71"), (2, "mic-72"), (3, "mic-73"), (5, "mic-75")] {
            expected.push(fault_keyed_without_reuse(run, repeat, id, KEYED_STAGE_US));
        }
    }
    assert_eq!(judgement.faults, expected);
    assert!(!describe(&judgement.faults).contains("mic-74"));
    Ok(())
}

/// T-RP-85-030: a run whose samples of utterance 7 have no translation stage.
#[test]
fn an_x_run_without_utterance_7_samples_is_rejected() -> TestResult {
    let (mut first, second) = made_pair();
    for take in &mut first.takes {
        take.stage_us = None;
    }
    let judgement = judged(&first, &second);
    let none = Fault::NoUtterance7Sample {
        run: String::from("e3q1"),
    };
    assert_eq!(judgement.faults, [none]);
    let said = refused(&judgement)?;
    assert!(said.contains("e3q1") && !said.contains("e3q2"), "{said}");
    Ok(())
}

/// T-RP-85-031: the two Japanese runs of window E3'', read from `docs/results/r8x/`.
#[test]
#[ignore = "reads the records of window E3'', which are put in docs/results/r8x/ after the window"]
fn rp85_a_draft_differing_only_in_numerals_or_inner_marks_is_reused_and_checked_by_v() -> TestResult
{
    let dir = repo_root().join("docs/results/r8x");
    let mut runs = Vec::new();
    for name in RECORDED_RUNS {
        runs.push(load_run(&dir, name)?);
    }
    let listing = verdict(&check_x_runs(&runs))?;
    eprintln!("utterance {UTTERANCE}:\n{listing}");
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
fn a_run_whose_log_dropped_lines_is_rejected() -> TestResult {
    let (mut first, second) = made_pair();
    let dropped = json!({"event": "log.queue.dropped", "level": "warn", "dropped": 3});
    first.extra_log.push(dropped.to_string());
    let judgement = judged(&first, &second);
    let none = Fault::DroppedLines {
        run: String::from("e3q1"),
    };
    assert_eq!(judgement.faults, [none]);
    let said = refused(&judgement)?;
    assert!(
        said.contains("e3q1") && said.contains("log.queue.dropped"),
        "{said}"
    );
    Ok(())
}

/// T-RP-85-034: three matched samples are enough, and the unmatched ones are
/// listed without being held to the line.
#[test]
fn three_keyed_samples_are_enough_and_the_unmatched_ones_are_listed() -> TestResult {
    let (mut first, second) = made_pair();
    first.unmatch(2, 300_000)?;
    let judgement = judged(&first, &second);
    assert_passes(&judgement);
    assert_eq!(run_report(&judgement, "e3q1")?.keyed.len(), 3);
    assert_eq!(
        unmatched_ids(run_report(&judgement, "e3q1")?),
        ["mic-72", "mic-74"]
    );
    assert_eq!(unmatched_ids(run_report(&judgement, "e3q2")?), ["mic-74"]);
    let listing = verdict(&judgement)?;
    for (run, id, stage) in [
        ("e3q1", "mic-72", "300.000 ms"),
        ("e3q1", "mic-74", "418.540 ms"),
        ("e3q2", "mic-74", "418.540 ms"),
    ] {
        assert!(
            has_line_with(&listing, &[run, id, stage, "key"]),
            "{listing}"
        );
    }
    Ok(())
}

/// T-RP-85-035: two matched samples are one short.
#[test]
fn two_keyed_samples_are_too_few_and_the_unmatched_ones_are_listed() -> TestResult {
    let (mut first, second) = made_pair();
    first.unmatch(2, 300_000)?;
    first.unmatch(3, 300_000)?;
    let judgement = judged(&first, &second);
    let few = Fault::TooFewKeyed {
        run: String::from("e3q1"),
        keyed: 2,
    };
    assert_eq!(judgement.faults, [few]);
    let said = refused(&judgement)?;
    for part in ["e3q1", "2 keyed", "fewer than 3", P7_DRAFT] {
        assert!(said.contains(part), "{part}: {said}");
    }
    let unkeyed = unkeyed_final();
    for id in ["mic-72", "mic-73", "mic-74"] {
        let parts = ["unmatched", id, unkeyed.as_str()];
        assert!(has_line_with(&said, &parts), "{id}: {said}");
    }
    assert!(!said.contains("e3q2"), "{said}");
    Ok(())
}

/// T-RP-85-036: all five takes are matched; nothing is left unmatched.
#[test]
fn five_keyed_samples_pass_and_none_is_listed_as_unmatched() -> TestResult {
    let (mut first, second) = made_pair();
    let fourth = first.take_mut(4)?;
    fourth.closing = P7_FINAL.to_owned();
    fourth.stage_us = Some(KEYED_STAGE_US);
    first
        .reuses
        .push((String::from("mic-74"), String::from("normalized")));
    let judgement = judged(&first, &second);
    assert_passes(&judgement);
    assert_eq!(run_report(&judgement, "e3q1")?.keyed.len(), 5);
    assert!(run_report(&judgement, "e3q1")?.unmatched.is_empty());
    assert_eq!(run_report(&judgement, "e3q2")?.unmatched.len(), 1);
    let listing = verdict(&judgement)?;
    assert!(
        has_line_with(&listing, &["e3q1 keyed 5", "unmatched 0"]),
        "{listing}"
    );
    Ok(())
}

/// T-RP-85-037: no sample is matched; the reuse of another utterance does not
/// stand for one.
#[test]
fn no_keyed_sample_is_rejected_with_all_five_listed() -> TestResult {
    let (mut first, second) = made_pair();
    for n in [1, 2, 3, 5] {
        first.unmatch(n, KEYED_STAGE_US)?;
    }
    let judgement = judged(&first, &second);
    let few = Fault::TooFewKeyed {
        run: String::from("e3q1"),
        keyed: 0,
    };
    assert_eq!(judgement.faults, [few]);
    let said = refused(&judgement)?;
    assert!(said.contains("0 keyed") && !said.contains("e3q2"), "{said}");
    let unkeyed = unkeyed_final();
    for id in ["mic-71", "mic-72", "mic-73", "mic-74", "mic-75"] {
        let parts = ["unmatched", id, unkeyed.as_str(), P7_DRAFT];
        assert!(has_line_with(&said, &parts), "{id}: {said}");
    }
    Ok(())
}

/// T-RP-85-038: a matched sample whose reuse is missing is named.
#[test]
fn a_keyed_sample_without_a_reuse_is_named_and_rejected() -> TestResult {
    let (mut first, second) = made_pair();
    first.drop_reuse("mic-71");
    let judgement = judged(&first, &second);
    let missing = fault_keyed_without_reuse("e3q1", 1, "mic-71", KEYED_STAGE_US);
    assert_eq!(judgement.faults, [missing]);
    let said = refused(&judgement)?;
    assert!(
        said.contains("mic-71") && said.contains("translate.draft.reused"),
        "{said}"
    );
    for id in ["mic-72", "mic-73", "mic-75"] {
        assert!(!said.contains(id), "{id}: {said}");
    }
    Ok(())
}

/// T-RP-85-039: a reuse line of another id is not the sample's reuse, though
/// the count of reuse lines is unchanged.
#[test]
fn a_reuse_line_under_another_id_does_not_stand_for_the_keyed_sample() -> TestResult {
    let (mut first, second) = made_pair();
    let before = first.reuses.len();
    for (id, _) in &mut first.reuses {
        if id == "mic-71" {
            *id = String::from("mic-91");
        }
    }
    assert_eq!(first.reuses.len(), before);
    let judgement = judged(&first, &second);
    let missing = fault_keyed_without_reuse("e3q1", 1, "mic-71", KEYED_STAGE_US);
    assert_eq!(judgement.faults, [missing]);
    Ok(())
}

/// T-RP-85-040: an unmatched sample, however slow, is not held to the line.
#[test]
fn a_slow_unmatched_sample_is_not_judged_against_the_182_ms_line() -> TestResult {
    let (mut first, second) = made_pair();
    first.take_mut(4)?.stage_us = Some(1_000_000);
    let judgement = judged(&first, &second);
    assert_passes(&judgement);
    let listing = verdict(&judgement)?;
    let parts = ["e3q1", "unmatched", "mic-74", "1000.000 ms", "key"];
    assert!(has_line_with(&listing, &parts), "{listing}");
    // The run left as made: four matched samples, the slowest at 120 ms.
    assert!(has_line_with(
        &listing,
        &["e3q2 keyed 4 slowest 120.000 ms"]
    ));
    eprintln!("{listing}");
    Ok(())
}

/// T-RP-85-041: the product key matches this slow sample, so it is judged,
/// and it has no reuse.
#[test]
fn a_slow_sample_whose_pair_the_product_key_matches_is_judged() -> TestResult {
    let (mut first, second) = made_pair();
    first.take_mut(4)?.closing = P7_FINAL.to_owned();
    let judgement = judged(&first, &second);
    let slow = Fault::SlowKeyed {
        run: String::from("e3q1"),
        repeat: 4,
        id: String::from("mic-74"),
        stage_us: 418_540,
    };
    let missing = fault_keyed_without_reuse("e3q1", 4, "mic-74", 418_540);
    assert_eq!(judgement.faults, [missing, slow]);
    let said = refused(&judgement)?;
    for part in ["e3q1", "mic-74", "translate.draft.reused", "418.540 ms"] {
        assert!(said.contains(part), "{part}: {said}");
    }
    Ok(())
}

/// The sample split of one pair: a made run of one take whose draft is
/// `draft` and whose final is `closing`.
fn split_of(draft: &str, closing: &str) -> Result<Split, String> {
    let rows = [
        event_line("partial", None, draft, 1_000_000),
        event_line("final", Some("mic-1"), closing, 2_000_000),
    ];
    let heard = heard_events(&rows.join("\n"))?;
    let sample = Sample {
        repeat: 1,
        final_us: 2_000_000,
        stage_us: KEYED_STAGE_US,
    };
    classify(&sample, &heard).ok_or_else(|| String::from("the final was not found"))
}

/// T-RP-85-042: the split follows the product's key on the nine pairs of the
/// key rows (001 to 005 match; 009 to 012 do not).
#[test]
fn the_sample_split_agrees_with_the_products_key_on_the_nine_pairs() -> TestResult {
    let other_letter = P7_DRAFT.replace("連合国は", "連合国が");
    let matched = [
        (P7_DRAFT, P7_FINAL),
        (P11_DRAFT, P11_FINAL),
        (P20_DRAFT, P20_FINAL),
        ("ところで、昨日は雨でした", "ところで昨日は雨でした。"),
        (DAMAGE_DRAFT, "被害は3件でした"),
    ];
    let apart = [
        ("これは本当ですか", "これは本当ですか？"),
        ("すごいですね", "すごいですね！"),
        (other_letter.as_str(), P7_FINAL),
        ("三件と五件です", "3件と5件と7件です。"),
    ];
    for (draft, closing) in matched {
        let split = split_of(draft, closing)?;
        assert!(
            matches!(split, Split::Keyed(_)),
            "{draft} / {closing}: {split:?}"
        );
    }
    for (draft, closing) in apart {
        let split = split_of(draft, closing)?;
        assert!(
            matches!(&split, Split::Unmatched(one) if one.reason == Reason::Key),
            "{draft} / {closing}: {split:?}"
        );
    }
    Ok(())
}

/// T-RP-85-043: runs named for the earlier window are not read, however good
/// their records are.
#[test]
fn a_set_of_runs_all_named_for_the_old_window_is_rejected_for_utterance_7() {
    let judgement = judged(&made_run("e3p1"), &made_run("e3p2"));
    assert_eq!(judgement.faults, [Fault::NoWindowRun]);
    assert!(judgement.runs.is_empty());
}

/// T-RP-85-044: a run of the earlier window beside valid runs is not read.
#[test]
fn a_run_named_for_the_old_window_is_not_read_beside_valid_runs_for_utterance_7() -> TestResult {
    let (mut first, second) = made_pair();
    first.take_mut(3)?.stage_us = Some(STAGE_LIMIT_US);
    let mut old = made_run("e3p1");
    old.drop_reuse("mic-71");
    old.take_mut(3)?.stage_us = Some(STAGE_LIMIT_US + 1);
    let runs = [old.texts(), first.texts(), second.texts()];
    let judgement = check_x_runs(&runs);
    assert_passes(&judgement);
    assert!(!verdict(&judgement)?.contains("e3p1"));
    Ok(())
}

/// T-RP-85-045: a run without its events file is named by that file's path.
#[test]
fn a_missing_events_file_is_rejected_by_path() -> TestResult {
    let (first, second) = made_pair();
    let dir = TempDir::new("rp85-records")?;
    first.write_to(dir.path(), false)?;
    second.write_to(dir.path(), true)?;
    let Err(said) = load_run(dir.path(), "e3q1") else {
        return Err("a run without its events file was read".into());
    };
    assert!(said.contains("e3q1.e2e1.events.jsonl"), "{said}");
    assert!(!said.contains("e3q2"), "{said}");
    let loaded = load_run(dir.path(), "e3q2")?;
    assert_passes(&check_x_runs(&[loaded]));
    Ok(())
}

/// T-RP-85-046: the validity line `d >= 120 ms` is not applied to a matched
/// sample.
#[test]
fn a_keyed_sample_below_the_validity_line_is_still_judged() -> TestResult {
    let (mut first, second) = made_pair();
    let third = first.take_mut(3)?;
    third.d_ms = 100;
    third.stage_us = Some(STAGE_LIMIT_US + 1);
    let judgement = judged(&first, &second);
    let slow = Fault::SlowKeyed {
        run: String::from("e3q1"),
        repeat: 3,
        id: String::from("mic-73"),
        stage_us: 182_001,
    };
    assert_eq!(judgement.faults, [slow]);
    Ok(())
}

/// T-RP-85-047: a sample is joined to its `final` by time, not by id. Another
/// `final` of the same id, a microsecond later, does not take its place.
#[test]
fn a_sample_is_joined_to_its_final_by_time_not_by_id() -> TestResult {
    let (mut first, second) = made_pair();
    first.drop_reuse("mic-73");
    let third = first.take_mut(3)?;
    third.other_final = Some((third.final_us + 1, unkeyed_final()));
    let judgement = judged(&first, &second);
    let missing = fault_keyed_without_reuse("e3q1", 3, "mic-73", KEYED_STAGE_US);
    assert_eq!(judgement.faults, [missing]);
    let said = refused(&judgement)?;
    assert!(
        said.contains("mic-73") && said.contains("translate.draft.reused"),
        "{said}"
    );
    Ok(())
}

/// T-RP-85-048: only `<prefix>` and digits is a run of the window.
#[test]
fn names_other_than_e3q_and_digits_are_not_read_for_utterance_7() {
    let longer = format!("{RUN_PREFIX}1x");
    let before = format!("x{RUN_PREFIX}1");
    let runs: Vec<XRun> = [longer.as_str(), "E3Q1", before.as_str()]
        .iter()
        .map(|name| made_run(name).texts())
        .collect();
    let judgement = check_x_runs(&runs);
    assert_eq!(judgement.faults, [Fault::NoWindowRun]);
}

/// T-RP-85-049: a sample that no `final` of the events is stamped for fails
/// the run by name; it is not put with the unmatched ones.
#[test]
fn a_sample_whose_final_is_missing_from_the_events_fails_the_run_by_name() -> TestResult {
    for other in [false, true] {
        let (mut first, second) = made_pair();
        let third = first.take_mut(3)?;
        third.stage_us = Some(UNKEYED_STAGE_US);
        third.final_kept = false;
        if other {
            third.other_final = Some((third.final_us + 1_000_000, unkeyed_final()));
        }
        let judgement = judged(&first, &second);
        let unjoined = Fault::UnjoinedSample {
            run: String::from("e3q1"),
            repeat: 3,
            final_us: 30_000_000,
        };
        assert_eq!(judgement.faults, [unjoined], "other final: {other}");
        let said = refused(&judgement)?;
        let parts = ["utt 7 repeat 3", "t_final_us 30000000"];
        assert!(has_line_with(&said, &parts), "{said}");
        assert!(!said.contains("e3q2"), "{said}");
        assert!(!has_line_with(&said, &["unmatched", "mic-73"]), "{said}");
    }
    Ok(())
}

/// A made run whose take 3 has no text in progress in its span (take 2's text
/// in progress stands before take 2's `final`).
fn run_with_no_draft_for_take_3() -> Result<MadeRun, String> {
    let mut run = made_run("e3q1");
    run.take_mut(3)?.draft = None;
    run.drop_reuse("mic-73");
    Ok(run)
}

/// T-RP-85-050: a take with no text in progress in its span is listed as
/// `no_draft`, and the text in progress of the take before is not taken for it.
#[test]
fn a_sample_with_no_draft_in_its_span_is_listed_as_no_draft_and_not_matched() -> TestResult {
    let first = run_with_no_draft_for_take_3()?;
    let judgement = judged(&first, &made_run("e3q2"));
    assert_passes(&judgement);
    let report = run_report(&judgement, "e3q1")?;
    assert_eq!(report.keyed.len(), 3);
    let reasons: Vec<(&str, Reason)> = report
        .unmatched
        .iter()
        .map(|one| (one.id.as_str(), one.reason))
        .collect();
    assert_eq!(
        reasons,
        [("mic-73", Reason::NoDraft), ("mic-74", Reason::Key)]
    );
    let listing = verdict(&judgement)?;
    assert!(
        has_line_with(&listing, &["e3q1", "mic-73", "no_draft"]),
        "{listing}"
    );
    Ok(())
}

/// The sample rows of `texts` with `change` applied to each row.
fn changed_sample_rows(texts: &XRun, change: impl Fn(&mut Value)) -> Result<XRun, String> {
    let mut rows = json_lines(&texts.samples)?;
    rows.iter_mut().for_each(change);
    let lines: Vec<String> = rows.iter().map(ToString::to_string).collect();
    Ok(XRun {
        name: texts.name.clone(),
        samples: lines.join("\n"),
        events: texts.events.clone(),
        relay_log: texts.relay_log.clone(),
    })
}

/// A made run with take 3 unjoined and the reuse of take 1 missing.
fn run_with_both_name_forms() -> Result<MadeRun, String> {
    let mut run = made_run("e3q1");
    run.take_mut(3)?.final_kept = false;
    run.drop_reuse("mic-71");
    Ok(run)
}

/// A sample is named by its repeat number and the id of its joined final (or
/// its `t_final_us`); an `id` written into the sample row changes nothing.
#[test]
fn a_sample_row_id_is_not_read_when_a_sample_is_named() -> TestResult {
    let texts = run_with_both_name_forms()?.texts();
    let plain = check_x_runs(std::slice::from_ref(&texts));
    let marked = changed_sample_rows(&texts, |row| row["id"] = json!("mic-99"))?;
    let with_id = check_x_runs(&[marked]);
    let expected = [
        fault_keyed_without_reuse("e3q1", 1, "mic-71", KEYED_STAGE_US),
        Fault::UnjoinedSample {
            run: String::from("e3q1"),
            repeat: 3,
            final_us: 30_000_000,
        },
    ];
    assert_eq!(plain.faults, expected);
    assert_eq!(with_id.faults, expected);
    let said = describe(&with_id.faults);
    assert!(said.contains("utt 7 repeat 1 mic-71"), "{said}");
    assert!(!said.contains("mic-99"), "{said}");
    Ok(())
}

/// A sample row whose `repeat` is missing or is not a JSON integer is a broken
/// record: the run fails naming the row's `t_final_us`.
#[test]
fn a_sample_row_without_an_integer_repeat_fails_the_run_by_its_final_time() -> TestResult {
    let texts = made_run("e3q1").texts();
    let broken: [fn(&mut Value); 2] = [
        |row| {
            let third = row["t_final_us"] == json!(30_000_000);
            if let Some(fields) = row.as_object_mut().filter(|_| third) {
                fields.remove("repeat");
            }
        },
        |row| {
            if row["t_final_us"] == json!(30_000_000) {
                row["repeat"] = json!("3");
            }
        },
    ];
    for change in broken {
        let run = changed_sample_rows(&texts, change)?;
        let judgement = check_x_runs(&[run]);
        let [Fault::Unreadable { run, what }] = judgement.faults.as_slice() else {
            return Err(format!("not one unreadable run: {:?}", judgement.faults).into());
        };
        assert_eq!(run, "e3q1");
        assert!(what.contains("t_final_us 30000000"), "{what}");
    }
    Ok(())
}

/// T-RP-85-051: a `no_draft` sample is not counted among the matched ones.
#[test]
fn a_no_draft_sample_is_not_counted_among_the_keyed_samples() -> TestResult {
    let mut first = run_with_no_draft_for_take_3()?;
    first.unmatch(5, KEYED_STAGE_US)?;
    let judgement = judged(&first, &made_run("e3q2"));
    let few = Fault::TooFewKeyed {
        run: String::from("e3q1"),
        keyed: 2,
    };
    assert_eq!(judgement.faults, [few]);
    let said = refused(&judgement)?;
    assert!(
        said.contains("2 keyed") && said.contains("fewer than 3"),
        "{said}"
    );
    for (id, reason) in [("mic-73", "no_draft"), ("mic-74", "key"), ("mic-75", "key")] {
        assert!(
            has_line_with(&said, &["unmatched", id, reason]),
            "{id}: {said}"
        );
    }
    assert!(!said.contains("e3q2"), "{said}");
    Ok(())
}

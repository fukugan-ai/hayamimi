//! RP-42: a line whose translations T and E were both withheld by V logs the
//! reason of each in `translate.line.held` (`hold_reason` is E's,
//! `t_hold_reason` is T's), and no log event carries the text of a
//! translation. The two translations V was handed go to the developer's file
//! (`VerifyDump`) one row each, only when the caller names the file.
//!
//! A line whose T was withheld and whose E passed goes out, and logs the
//! reason of T once in `translate.draft.held`.
//!
//! A fake llama-server answers T with the first scripted text and E with the
//! second, so each text goes through V exactly as the model's answer would.

mod support;

use std::fs;
use std::sync::Arc;
use std::time::Duration;

use hayamimi_relay::cli::{Command, Live, parse_args, start_live};
use hayamimi_relay::follow::Backoff;
use hayamimi_relay::hub;
use hayamimi_relay::lang::Lang;
use hayamimi_relay::llm::HttpLlm;
use hayamimi_relay::logging::{Level, LogEvent, MemorySink, SharedSink};
use hayamimi_relay::origin::Guard;
use hayamimi_relay::pipeline::{
    LineKind, LineRequest, NoObserver, Observer, Pipeline, PipelineConfig, Status, Translation,
};
use hayamimi_relay::relay::{self, Lanes, MakePipeline, RelayConfig};
use hayamimi_relay::strings::Strings;
use hayamimi_relay::verify::HoldReason;
use hayamimi_relay::verify_dump::VerifyDump;
use serde_json::{Value, json};
use support::events::{FakeRecognizer, Logs, collect_from, outputs};
use support::{FakeServer, Reply, TempDir, TestResult};

const ID: &str = "rp42-1";
/// Five people, and not there: a number and a negation for V to keep.
const SOURCE: &str = "五人はそこにいませんでした。";
/// T keeps the number and loses the negation.
const T_NO_NEGATION: &str = "Five were there.";
/// E keeps the negation and loses the number.
const E_NO_NUMBER: &str = "They were not there.";

/// A source V has nothing against, and a translation that passes it.
const PLAIN_SOURCE: &str = "そこに五人います。";
const PLAIN_TRANSLATION: &str = "Five people are there.";

/// A server that answers the first request with `first` and every later one
/// with `second`: T is asked first, E second.
fn server_saying(first: &str, second: &str) -> std::io::Result<FakeServer> {
    let (first, second) = (support::chat_reply(first), support::chat_reply(second));
    FakeServer::start(move |n, _| Reply::json(if n == 0 { &first } else { &second }))
}

fn request(text: &str) -> Result<LineRequest<'_>, String> {
    Ok(LineRequest {
        id: ID,
        of: LineKind::Final,
        source: "file",
        src_lang: Lang::Ja,
        text,
        now_epoch: support::epoch(support::DEFAULT_NOW)?,
    })
}

/// One line of the live pair through the pipeline, watched by `observer`.
fn run_line(
    src: &str,
    first: &str,
    second: &str,
    observer: &mut dyn Observer,
) -> Result<(Translation, Vec<LogEvent>), Box<dyn std::error::Error>> {
    let server = server_saying(first, second)?;
    let mut sink = MemorySink::default();
    let mut pipeline = Pipeline::open(
        HttpLlm::new(&server.base_url()),
        &PipelineConfig::default(),
        &mut sink,
    )?;
    let line = pipeline.run(&request(src)?, observer, &mut sink);
    Ok((line, sink.events))
}

/// The one `translate.line.held` event of `events`.
fn the_held_event(events: &[LogEvent]) -> Result<&LogEvent, String> {
    let held: Vec<&LogEvent> = events
        .iter()
        .filter(|event| event.event == "translate.line.held")
        .collect();
    match held.as_slice() {
        [event] => Ok(event),
        other => Err(format!("{} held events, not 1", other.len())),
    }
}

fn field<'a>(event: &'a LogEvent, key: &str) -> Option<&'a Value> {
    event.fields.get(key)
}

/// No event of the run, in its JSON form, carries any of `texts`.
fn assert_no_text_in_the_log(events: &[LogEvent], texts: &[&str]) {
    for event in events {
        let json = event.to_json_line();
        for text in texts {
            assert!(!json.contains(text), "{} carries {text:?}", event.event);
        }
    }
}

/// The rows of the developer's file, parsed.
fn rows_of(path: &std::path::Path) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    let text = fs::read_to_string(path)?;
    let mut rows = Vec::new();
    for line in text.lines() {
        rows.push(serde_json::from_str(line)?);
    }
    Ok(rows)
}

/// The acceptance test of the row.
#[test]
fn rp42_held_line_logs_both_reasons_without_the_text() -> TestResult {
    let dir = TempDir::new("rp42")?;
    let path = dir.join("verify-text.jsonl");
    let mut sink = MemorySink::default();
    let Some(dump) = VerifyDump::open(&path, &mut sink) else {
        return Err("the dump file was not opened".into());
    };
    let mut observer = dump.observer(ID);

    let (line, events) = run_line(SOURCE, T_NO_NEGATION, E_NO_NUMBER, &mut observer)?;

    assert_eq!(line.status, Status::Held);
    assert_eq!(line.text, "");
    assert_eq!(line.hold_reason, Some(HoldReason::Number));
    let event = the_held_event(&events)?;
    assert_eq!(field(event, "hold_reason"), Some(&json!("number")));
    assert_eq!(field(event, "t_hold_reason"), Some(&json!("negation")));
    assert_no_text_in_the_log(&events, &[T_NO_NEGATION, E_NO_NUMBER]);
    assert_eq!(
        rows_of(&path)?,
        [
            json!({ "id": ID, "pass": "t", "text": T_NO_NEGATION }),
            json!({ "id": ID, "pass": "e", "text": E_NO_NUMBER }),
        ]
    );
    Ok(())
}

/// The negative control: when T passed V, a held line logs no
/// reason for T. Here E's text carries the end marker, so the line is held
/// although T's translation stood.
#[test]
fn rp42_a_line_whose_t_passed_logs_no_reason_for_t() -> TestResult {
    let marked = format!("{PLAIN_TRANSLATION}</s>");
    let (line, events) = run_line(PLAIN_SOURCE, PLAIN_TRANSLATION, &marked, &mut NoObserver)?;
    assert_eq!(line.status, Status::Held);
    let event = the_held_event(&events)?;
    assert_eq!(field(event, "hold_reason"), Some(&json!("form")));
    assert_eq!(field(event, "t_hold_reason"), None);
    Ok(())
}

/// Both reasons are logged when they are the same.
#[test]
fn rp42_both_reasons_are_logged_when_they_agree() -> TestResult {
    let (_, events) = run_line(SOURCE, T_NO_NEGATION, T_NO_NEGATION, &mut NoObserver)?;
    let event = the_held_event(&events)?;
    assert_eq!(field(event, "hold_reason"), Some(&json!("negation")));
    assert_eq!(field(event, "t_hold_reason"), Some(&json!("negation")));
    Ok(())
}

/// Without a file named, the pipeline writes none; a file that
/// cannot be opened is said so in the log and the run goes on without it.
#[test]
fn rp42_the_dump_exists_only_when_a_file_is_named_and_opens() -> TestResult {
    let dir = TempDir::new("rp42-open")?;
    let untouched = dir.join("never-named.jsonl");
    run_line(SOURCE, T_NO_NEGATION, E_NO_NUMBER, &mut NoObserver)?;
    assert!(!untouched.exists());

    let mut sink = MemorySink::default();
    assert!(VerifyDump::open(dir.path(), &mut sink).is_none());
    let [event] = sink.events.as_slice() else {
        return Err(format!("{} events, not 1", sink.events.len()).into());
    };
    assert_eq!(event.event, "translate.dump.failed");
    assert_eq!(event.error_code.as_deref(), Some("HYM_STORAGE_DUMP_OPEN"));
    Ok(())
}

/// The recognizer's `final` event for a Japanese line of the microphone.
fn final_event(id: &str, text: &str) -> String {
    json!({
        "type": "final", "id": id, "source": "mic", "lang": "ja", "text": text,
        "speaker": "", "audio_s": 1.5, "t0_s": 0.5, "t1_s": 2.0,
        "end_wall_ms": 1_790_000_000_000_u64,
    })
    .to_string()
}

/// A relay with `dump` (or none) that translates the one line `SOURCE`
/// through the recognizer's stream and has published it. Its events go to
/// `logs`.
fn relay_translating_source(dump: Option<VerifyDump>, logs: &Logs) -> TestResult {
    let server = server_saying(T_NO_NEGATION, E_NO_NUMBER)?;
    let base = server.base_url();
    let make: MakePipeline<HttpLlm> =
        Arc::new(move |sink| Pipeline::open(HttpLlm::new(&base), &PipelineConfig::default(), sink));
    let recognizer = FakeRecognizer::start()?;
    let config = RelayConfig {
        events: vec![recognizer.url()],
        lanes: Lanes::One,
        backoff: Backoff {
            initial: Duration::from_millis(20),
            max: Duration::from_millis(100),
        },
        started_epoch: 1_790_000_000,
        translate: None,
        verify_dump: dump,
        guard: Guard::default(),
    };
    let address = relay::start_with(
        hub::bind(0)?,
        config,
        make,
        SharedSink::new(logs.clone()),
        outputs(Vec::new(), None)?,
    )?;
    let published = collect_from(address)?;
    recognizer.send(&final_event("mic-1", SOURCE));
    published.wait_for(1, Duration::from_secs(10));
    Ok(())
}

/// The relay's lane hands the observer of its line to the pipeline: a line
/// that arrives through the recognizer's stream leaves its two rows, and the
/// relay's log keeps the text out.
#[test]
fn rp42_the_relay_writes_the_rows_of_the_lines_it_translates() -> TestResult {
    let dir = TempDir::new("rp42-relay")?;
    let path = dir.join("verify-text.jsonl");
    let logs = Logs::default();
    let Some(dump) = VerifyDump::open(&path, &mut logs.clone()) else {
        return Err("the dump file was not opened".into());
    };
    relay_translating_source(Some(dump), &logs)?;

    assert_eq!(
        rows_of(&path)?,
        [
            json!({ "id": "mic-1", "pass": "t", "text": T_NO_NEGATION }),
            json!({ "id": "mic-1", "pass": "e", "text": E_NO_NUMBER }),
        ]
    );
    assert_eq!(logs.named("translate.line.held").len(), 1);
    for line in logs.lines() {
        assert!(
            !line.contains(T_NO_NEGATION) && !line.contains(E_NO_NUMBER),
            "{line}"
        );
    }
    Ok(())
}

/// T-RP-42-006: every reason that holds a ja to en line is a possible reason
/// for T. E always answers with the same text, so only T's side changes.
/// `uncertainty` is not one of them: a hedge that only one side carries does
/// not hold a line into English (RP-48), so the hedged T below passes V and
/// the line goes out with no held event.
#[test]
fn rp42_every_reason_that_holds_a_ja_to_en_line_can_be_the_reason_of_t() -> TestResult {
    const WITH_NAME: &str = "五人はAcmeにいませんでした。";
    let hedged = "Maybe five were not there.";
    let (line, events) = run_line(SOURCE, hedged, E_NO_NUMBER, &mut NoObserver)?;
    assert_eq!(line.status, Status::Ok);
    assert!(
        events
            .iter()
            .all(|event| event.event != "translate.line.held")
    );
    let cases = [
        (SOURCE, "Six were not there.", "number"),
        (SOURCE, "Sure, five were not there.", "form"),
        (WITH_NAME, "Five were not there.", "proper_noun"),
    ];
    for (src, t_text, reason) in cases {
        let (_, events) = run_line(src, t_text, E_NO_NUMBER, &mut NoObserver)?;
        let event = the_held_event(&events)?;
        assert_eq!(
            field(event, "t_hold_reason"),
            Some(&json!(reason)),
            "{t_text}"
        );
    }
    Ok(())
}

/// T-RP-42-007: a line that goes out logs no held event, and the text of its
/// translations stays out of the log although the dump file is open.
#[test]
fn rp42_a_line_that_goes_out_logs_no_held_event() -> TestResult {
    let dir = TempDir::new("rp42-out")?;
    let mut sink = MemorySink::default();
    let Some(dump) = VerifyDump::open(&dir.join("verify-text.jsonl"), &mut sink) else {
        return Err("the dump file was not opened".into());
    };
    let mut observer = dump.observer(ID);
    let (line, events) = run_line(
        PLAIN_SOURCE,
        PLAIN_TRANSLATION,
        PLAIN_TRANSLATION,
        &mut observer,
    )?;
    assert_eq!(line.status, Status::Ok);
    assert!(
        events
            .iter()
            .all(|event| event.event != "translate.line.held")
    );
    assert_no_text_in_the_log(&events, &[PLAIN_TRANSLATION]);
    Ok(())
}

/// T-RP-42-008: a relay whose config names no file writes none, and the line
/// that V withholds still logs both reasons.
#[test]
fn rp42_a_relay_without_a_named_file_writes_none() -> TestResult {
    let dir = TempDir::new("rp42-none")?;
    let logs = Logs::default();
    relay_translating_source(None, &logs)?;

    assert_eq!(fs::read_dir(dir.path())?.count(), 0);
    let held = logs.named("translate.line.held");
    let [event] = held.as_slice() else {
        return Err(format!("{} held events, not 1", held.len()).into());
    };
    assert_eq!(field(event, "hold_reason"), Some(&json!("number")));
    assert_eq!(field(event, "t_hold_reason"), Some(&json!("negation")));
    Ok(())
}

/// The settings `live` is given with `extra` after the stream and the server.
fn live_settings(
    recognizer: &FakeRecognizer,
    server: &FakeServer,
    extra: &[&str],
) -> Result<Live, Box<dyn std::error::Error>> {
    let mut args: Vec<String> = ["live", "--events", &recognizer.url(), "--llm"]
        .map(str::to_owned)
        .to_vec();
    args.push(server.base_url());
    args.push(String::from("--llm-other"));
    args.push(server.base_url());
    args.extend(["--serve", "0"].map(str::to_owned));
    args.extend(extra.iter().map(|word| (*word).to_owned()));
    match parse_args(&args) {
        Ok(Command::Run(live)) => Ok(live),
        other => Err(format!("live was not parsed: {other:?}").into()),
    }
}

/// Starts the relay the way `cli::run` does and stops it again.
fn start_and_stop(live: &Live) -> TestResult {
    let strings = Strings::embedded(Lang::En)?;
    let sink = SharedSink::new(Logs::default());
    let relay = start_live(live, 1_790_000_000, &strings, &sink)
        .map_err(|error| format!("live did not start: {error:?}"))?;
    relay.stop();
    Ok(())
}

/// T-RP-42-009: from the arguments to the relay's config, the developer's file
/// is named only by `--verify-text`.
#[test]
fn rp42_live_without_verify_text_names_no_file() -> TestResult {
    let dir = TempDir::new("rp42-live")?;
    let named = dir.join("verify-text.jsonl");
    let named_arg = named.to_str().ok_or("non-utf8 path")?;
    let (recognizer, server) = (
        FakeRecognizer::start()?,
        server_saying(PLAIN_TRANSLATION, PLAIN_TRANSLATION)?,
    );

    let without = live_settings(&recognizer, &server, &[])?;
    assert_eq!(without.verify_text, None);
    start_and_stop(&without)?;
    assert_eq!(fs::read_dir(dir.path())?.count(), 0);

    let with = live_settings(&recognizer, &server, &["--verify-text", named_arg])?;
    assert_eq!(with.verify_text.as_deref(), Some(named.as_path()));
    start_and_stop(&with)?;
    assert!(named.exists());
    Ok(())
}

/// The `translate.draft.held` events of `events`.
fn draft_held(events: &[LogEvent]) -> Vec<&LogEvent> {
    events
        .iter()
        .filter(|event| event.event == "translate.draft.held")
        .collect()
}

/// E's translation that keeps the number and the negation of `SOURCE`.
const E_PASSES: &str = "Five were not there.";

/// T-RP-42-010: V withheld T's translation and E's passed. The line goes out,
/// and one `translate.draft.held` says why T fell, without any text.
#[test]
fn rp42_a_line_whose_t_was_held_and_e_passed_logs_the_draft_held() -> TestResult {
    let (line, events) = run_line(SOURCE, T_NO_NEGATION, E_PASSES, &mut NoObserver)?;
    assert_eq!(line.status, Status::Ok);
    assert_eq!(line.text, E_PASSES);
    let held = draft_held(&events);
    let [event] = held.as_slice() else {
        return Err(format!("{} draft.held events, not 1", held.len()).into());
    };
    assert_eq!(event.level, Level::Info);
    assert_eq!(field(event, "id"), Some(&json!(ID)));
    assert_eq!(field(event, "source"), Some(&json!("file")));
    assert_eq!(field(event, "hold_reason"), Some(&json!("negation")));
    assert_no_text_in_the_log(&events, &[T_NO_NEGATION, E_PASSES]);
    Ok(())
}

/// T-RP-42-011: the control. When T passed V too, nothing is said about T.
#[test]
fn rp42_a_line_whose_t_passed_logs_no_draft_held() -> TestResult {
    let (line, events) = run_line(
        PLAIN_SOURCE,
        PLAIN_TRANSLATION,
        PLAIN_TRANSLATION,
        &mut NoObserver,
    )?;
    assert_eq!(line.status, Status::Ok);
    assert!(draft_held(&events).is_empty());
    Ok(())
}

/// T-RP-42-012: a line whose T and E were both held says it in
/// `translate.line.held` (`t_hold_reason`); the draft is not held twice.
#[test]
fn rp42_a_line_held_on_both_passes_logs_no_draft_held() -> TestResult {
    let (line, events) = run_line(SOURCE, T_NO_NEGATION, E_NO_NUMBER, &mut NoObserver)?;
    assert_eq!(line.status, Status::Held);
    assert!(draft_held(&events).is_empty());
    the_held_event(&events)?;
    Ok(())
}

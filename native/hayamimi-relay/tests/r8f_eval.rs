//! R8f: `eval` translates the Japanese side of sentence pairs into the
//! language each file names, through the chat endpoint, and writes one line
//! per sentence for a scorer.
//!
//! A scripted server stands in for llama-server: it reads the one user message
//! of each request and answers the translation or the edit of the sentence
//! the message holds.

mod support;

use std::fs;
use std::time::Duration;

use hayamimi_relay::eval::{self, EvalError, EvalOptions, Report, Written};
use hayamimi_relay::llm::HttpLlm;
use hayamimi_relay::logging::{Level, MemorySink};
use serde_json::Value;
use support::{FakeServer, Reply, TempDir, TestResult, chat_reply, closed_port_url, request_body};

/// One sentence and what the server answers to its T and to its E.
struct Row {
    source: &'static str,
    translation: &'static str,
    edit: &'static str,
}

/// The user message of a chat request.
fn user_message(raw: &[u8]) -> Option<String> {
    let body: Value = serde_json::from_slice(request_body(raw)).ok()?;
    body["messages"][0]["content"].as_str().map(str::to_owned)
}

/// What the scripted server says to `message`: the translation when it is a
/// T prompt, the edit when it is an E prompt.
fn scripted_answer(rows: &[Row], message: &str) -> Option<&'static str> {
    let row = rows.iter().find(|row| message.contains(row.source))?;
    if message.starts_with("Translate the following") {
        Some(row.translation)
    } else {
        Some(row.edit)
    }
}

fn scripted_server(rows: Vec<Row>) -> std::io::Result<FakeServer> {
    FakeServer::start(move |_, raw| {
        match user_message(raw).and_then(|message| scripted_answer(&rows, &message)) {
            Some(text) => Reply::json(&chat_reply(text)),
            None => Reply::status(400),
        }
    })
}

fn pairs_dir(files: &[(&str, &str)]) -> std::io::Result<TempDir> {
    let dir = TempDir::new("r8f-pairs")?;
    for (name, text) in files {
        fs::write(dir.join(name), text)?;
    }
    Ok(dir)
}

fn run_eval(
    server: &FakeServer,
    pairs: &TempDir,
    out: &TempDir,
    sink: &mut MemorySink,
) -> Result<Report, EvalError> {
    let llm = HttpLlm::with_timeout(&server.base_url(), Duration::from_secs(5));
    eval::run(
        &llm,
        &EvalOptions {
            pairs: pairs.path(),
            out: out.path(),
        },
        sink,
    )
}

/// One line per sentence, in order; the edit stands when it passed, the first
/// translation when only the edit failed, and a sentence withheld in both is
/// an empty line.
#[test]
fn each_sentence_settles_on_the_edit_then_the_draft_then_an_empty_line() -> TestResult {
    let server = scripted_server(vec![
        Row {
            source: "猫が寝ています。",
            translation: "El gato duerme.",
            edit: "El gato está durmiendo.",
        },
        Row {
            source: "予算は五百万円です。",
            translation: "El presupuesto es de 7 millones de yenes.",
            edit: "El presupuesto es de 7 millones de yenes.",
        },
        Row {
            source: "犬が走ります。",
            translation: "El perro corre.",
            edit: "  ",
        },
    ])?;
    let pairs = pairs_dir(&[
        (
            "es.ja.txt",
            "猫が寝ています。\n予算は五百万円です。\n犬が走ります。\n",
        ),
        // The reference and the earlier hypotheses of the same directory are
        // not sources.
        ("es.ref.txt", "ignored\n"),
        ("es.hyp.txt", "ignored\n"),
        ("es.ids.txt", "1\n2\n3\n"),
    ])?;
    let out = TempDir::new("r8f-out")?;
    let mut sink = MemorySink::default();
    let report = run_eval(&server, &pairs, &out, &mut sink)?;
    assert_eq!(
        fs::read_to_string(out.join("es.hyp.txt"))?,
        "El gato está durmiendo.\n\nEl perro corre.\n"
    );
    assert_eq!(
        report.written,
        [Written {
            code: "es",
            lines: 3,
            held: 1
        }]
    );
    assert!(report.skipped.is_empty());
    // T and E for each of the three sentences.
    assert_eq!(server.requests().len(), 6);
    Ok(())
}

/// The request is the chat form of the design: one user message that names
/// the language, `stop`, a greedy temperature, and the prompt cache.
#[test]
fn a_request_names_the_language_in_one_user_message() -> TestResult {
    let server = scripted_server(vec![Row {
        source: "猫が寝ています。",
        translation: "猫在睡觉。",
        edit: "猫在睡觉。",
    }])?;
    let pairs = pairs_dir(&[("zh.ja.txt", "猫が寝ています。\n")])?;
    let out = TempDir::new("r8f-out")?;
    run_eval(&server, &pairs, &out, &mut MemorySink::default())?;
    let requests = server.requests();
    let first: Value = serde_json::from_slice(request_body(&requests[0]))?;
    assert_eq!(
        first["messages"][0]["content"],
        "Translate the following Japanese text into Chinese.\n\n猫が寝ています。"
    );
    assert_eq!(first["messages"].as_array().map(Vec::len), Some(1));
    assert_eq!(first["stop"], serde_json::json!(["</s>"]));
    assert_eq!(first["temperature"], 0.0);
    assert_eq!(first["cache_prompt"], true);
    let second: Value = serde_json::from_slice(request_body(&requests[1]))?;
    let Some(edit) = second["messages"][0]["content"].as_str() else {
        panic!("the second request has no user message: {second}");
    };
    assert!(edit.contains("Rewritten Chinese:"), "{edit}");
    Ok(())
}

/// A language the translator does not write is left out and said so; the
/// others, including English through the live pair's own verification, go on.
#[test]
fn an_unlisted_language_is_left_out_with_an_event_and_the_others_go_on() -> TestResult {
    let server = scripted_server(vec![
        Row {
            source: "こんにちは。",
            translation: "Hello.",
            edit: "Hello there.",
        },
        Row {
            source: "猫が寝ています。",
            translation: "猫在睡觉。",
            edit: "猫在睡觉。",
        },
    ])?;
    let pairs = pairs_dir(&[
        ("en.ja.txt", "こんにちは。\n"),
        ("xx.ja.txt", "こんにちは。\n"),
        ("zh.ja.txt", "猫が寝ています。\n"),
    ])?;
    let out = TempDir::new("r8f-out")?;
    let mut sink = MemorySink::default();
    let report = run_eval(&server, &pairs, &out, &mut sink)?;
    assert_eq!(report.skipped, ["xx"]);
    assert_eq!(
        report
            .written
            .iter()
            .map(|one| one.code)
            .collect::<Vec<_>>(),
        ["en", "zh"]
    );
    assert_eq!(
        fs::read_to_string(out.join("en.hyp.txt"))?,
        "Hello there.\n"
    );
    assert_eq!(fs::read_to_string(out.join("zh.hyp.txt"))?, "猫在睡觉。\n");
    assert!(!out.join("xx.hyp.txt").exists());
    let skipped: Vec<_> = sink
        .events
        .iter()
        .filter(|event| event.event == eval::SKIPPED_EVENT)
        .collect();
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0].level, Level::Warn);
    assert_eq!(
        skipped[0].fields.get("target"),
        Some(&serde_json::json!("xx"))
    );
    assert_eq!(
        skipped[0].error_code.as_deref(),
        Some("HAYAMIMI_TRANSLATE_FALLBACK")
    );
    // T and E for the two languages that were written; none for the other.
    assert_eq!(server.requests().len(), 4);
    Ok(())
}

/// A translator that does not answer stops the run: no file is written, so a
/// score cannot be taken over empty answers.
#[test]
fn an_unreachable_translator_stops_the_run_without_writing() -> TestResult {
    let pairs = pairs_dir(&[("es.ja.txt", "猫が寝ています。\n")])?;
    let out = TempDir::new("r8f-out")?;
    let llm = HttpLlm::with_timeout(&closed_port_url()?, Duration::from_secs(2));
    let said = eval::run(
        &llm,
        &EvalOptions {
            pairs: pairs.path(),
            out: out.path(),
        },
        &mut MemorySink::default(),
    );
    assert!(matches!(
        said,
        Err(EvalError::Llm {
            code: "es",
            line: 1,
            ..
        })
    ));
    assert!(!out.join("es.hyp.txt").exists());
    Ok(())
}

#[test]
fn a_directory_without_sources_is_refused() -> TestResult {
    let server = scripted_server(Vec::new())?;
    let pairs = pairs_dir(&[("es.ref.txt", "x\n")])?;
    let out = TempDir::new("r8f-out")?;
    let said = run_eval(&server, &pairs, &out, &mut MemorySink::default());
    assert!(matches!(said, Err(EvalError::NoPairs(_))));
    assert_eq!(server.requests().len(), 0);
    Ok(())
}

/// A request the server refuses (a 4xx) stops the run on that line and says
/// which language and line.
#[test]
fn a_refused_request_names_the_language_and_the_line() -> TestResult {
    let server = scripted_server(vec![Row {
        source: "猫が寝ています。",
        translation: "El gato duerme.",
        edit: "El gato duerme mucho.",
    }])?;
    let pairs = pairs_dir(&[("es.ja.txt", "猫が寝ています。\n犬が走ります。\n")])?;
    let out = TempDir::new("r8f-out")?;
    let said = run_eval(&server, &pairs, &out, &mut MemorySink::default());
    assert!(matches!(
        said,
        Err(EvalError::Llm {
            code: "es",
            line: 2,
            ..
        })
    ));
    assert!(!out.join("es.hyp.txt").exists());
    Ok(())
}

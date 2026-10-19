//! R8f: `eval` translates the Japanese side of sentence pairs into the
//! language each file names, through the chat endpoint, and writes one line
//! per sentence for a scorer.
//!
//! A scripted server stands in for llama-server: it reads the user message of
//! each request (and the system message of an edit) and answers the
//! translation or the edit of the sentence the user message holds.

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

/// What a chat request holds: the text of its user message, whether a system
/// message stands before it (the draft of an edit), and whether it carries the
/// language codes as template variables (the form of the model that writes
/// the languages outside the live pair).
struct Chat {
    user: String,
    has_system: bool,
    coded: bool,
}

fn chat_of(raw: &[u8]) -> Option<Chat> {
    let body: Value = serde_json::from_slice(request_body(raw)).ok()?;
    let messages = body["messages"].as_array()?;
    let user = messages.last()?["content"].as_str()?.to_owned();
    Some(Chat {
        user,
        has_system: messages.len() == 2,
        coded: body.get("chat_template_kwargs").is_some(),
    })
}

/// What the scripted server says to `chat`: the translation when it is a T
/// request or a coded request, the edit when a system message holds a draft.
fn scripted_answer(rows: &[Row], chat: &Chat) -> Option<&'static str> {
    let row = rows.iter().find(|row| chat.user.contains(row.source))?;
    if chat.coded || !chat.has_system {
        Some(row.translation)
    } else {
        Some(row.edit)
    }
}

fn scripted_server(rows: Vec<Row>) -> std::io::Result<FakeServer> {
    FakeServer::start(move |_, raw| {
        match chat_of(raw).and_then(|chat| scripted_answer(&rows, &chat)) {
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
            translation: "The cat sleeps.",
            edit: "The cat is sleeping.",
        },
        Row {
            source: "予算は五百万円です。",
            translation: "The budget is 7 million yen.",
            edit: "The budget is 7 million yen.",
        },
        Row {
            source: "犬が走ります。",
            translation: "The dog runs.",
            edit: "  ",
        },
    ])?;
    let pairs = pairs_dir(&[
        (
            "en.ja.txt",
            "猫が寝ています。\n予算は五百万円です。\n犬が走ります。\n",
        ),
        // The reference and the earlier hypotheses of the same directory are
        // not sources.
        ("en.ref.txt", "ignored\n"),
        ("en.hyp.txt", "ignored\n"),
        ("en.ids.txt", "1\n2\n3\n"),
    ])?;
    let out = TempDir::new("r8f-out")?;
    let mut sink = MemorySink::default();
    let report = run_eval(&server, &pairs, &out, &mut sink)?;
    assert_eq!(
        fs::read_to_string(out.join("en.hyp.txt"))?,
        "The cat is sleeping.\n\nThe dog runs.\n"
    );
    assert_eq!(
        report.written,
        [Written {
            code: "en",
            lines: 3,
            held: 1
        }]
    );
    assert!(report.skipped.is_empty());
    // T and E for each of the three sentences.
    assert_eq!(server.requests().len(), 6);
    Ok(())
}

/// A language outside the live pair is written by T alone: one request per
/// sentence, no edit, and a translation that failed V is an empty line.
#[test]
fn a_language_outside_the_pair_settles_on_t_alone_or_an_empty_line() -> TestResult {
    let server = scripted_server(vec![
        Row {
            source: "猫が寝ています。",
            translation: "El gato duerme.",
            edit: "unused",
        },
        Row {
            source: "予算は五百万円です。",
            translation: "El presupuesto es de 7 millones de yenes.",
            edit: "unused",
        },
    ])?;
    let pairs = pairs_dir(&[("es.ja.txt", "猫が寝ています。\n予算は五百万円です。\n")])?;
    let out = TempDir::new("r8f-out")?;
    let mut sink = MemorySink::default();
    let report = run_eval(&server, &pairs, &out, &mut sink)?;
    assert_eq!(
        fs::read_to_string(out.join("es.hyp.txt"))?,
        "El gato duerme.\n\n"
    );
    assert_eq!(
        report.written,
        [Written {
            code: "es",
            lines: 2,
            held: 1
        }]
    );
    assert_eq!(server.requests().len(), 2);
    Ok(())
}

/// The request is the chat form of the design: one user message that names
/// the language, `stop`, a greedy temperature, and the prompt cache.
#[test]
fn a_request_names_the_language_in_one_user_message() -> TestResult {
    let server = scripted_server(vec![Row {
        source: "猫が寝ています。",
        translation: "The cat sleeps.",
        edit: "The cat is sleeping.",
    }])?;
    let pairs = pairs_dir(&[("en.ja.txt", "猫が寝ています。\n")])?;
    let out = TempDir::new("r8f-out")?;
    run_eval(&server, &pairs, &out, &mut MemorySink::default())?;
    let requests = server.requests();
    let first: Value = serde_json::from_slice(request_body(&requests[0]))?;
    assert_eq!(
        first["messages"][0]["content"],
        "Translate the following Japanese text into English.\n\n 猫が寝ています。"
    );
    assert_eq!(first["messages"].as_array().map(Vec::len), Some(1));
    assert_eq!(first["stop"], serde_json::json!(["</s>"]));
    assert_eq!(first["temperature"], 0.0);
    assert_eq!(first["cache_prompt"], true);
    let second: Value = serde_json::from_slice(request_body(&requests[1]))?;
    assert_eq!(second["messages"].as_array().map(Vec::len), Some(2));
    assert_eq!(second["messages"][0]["role"], "system");
    assert_eq!(second["messages"][1]["role"], "user");
    Ok(())
}

/// E's request is two messages: T's own prompt as the user message, space
/// included, and the draft with what to keep in a system message before it.
#[test]
fn eval_asks_e_with_ts_prompt_and_the_draft_in_a_system_message() -> TestResult {
    let server = scripted_server(vec![Row {
        source: "猫が寝ています。",
        translation: "The cat sleeps.",
        edit: "The cat is sleeping.",
    }])?;
    let pairs = pairs_dir(&[("en.ja.txt", "猫が寝ています。\n")])?;
    let out = TempDir::new("r8f-out")?;
    run_eval(&server, &pairs, &out, &mut MemorySink::default())?;
    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    let first: Value = serde_json::from_slice(request_body(&requests[0]))?;
    let Some(translate) = first["messages"][0]["content"].as_str() else {
        panic!("the first request has no user message: {first}");
    };
    assert!(translate.ends_with("\n\n 猫が寝ています。"), "{translate}");
    let second: Value = serde_json::from_slice(request_body(&requests[1]))?;
    assert_eq!(second["messages"].as_array().map(Vec::len), Some(2));
    assert_eq!(second["messages"][1]["content"], translate);
    let Some(system) = second["messages"][0]["content"].as_str() else {
        panic!("the second request has no system message: {second}");
    };
    assert!(
        system.starts_with(
            "A draft English translation of the user's Japanese text is: The cat sleeps.\nIt may contain errors. Keep the facts, numbers, proper names, negation and uncertainty of the Japanese text."
        ),
        "{system}"
    );
    assert!(
        system.contains(
            "\nAvoid \"please take care of me\"; write \"I look forward to working with you\"."
        ),
        "{system}"
    );
    assert!(!system.contains("Rewritten"), "{system}");
    Ok(())
}

/// A language outside the pair is asked the way the chat template of its model
/// reads: one user message with the bare text, and the source and target codes
/// as template variables.
#[test]
fn a_request_outside_the_pair_carries_the_language_codes() -> TestResult {
    let server = scripted_server(vec![Row {
        source: "猫が寝ています。",
        translation: "猫在睡觉。",
        edit: "unused",
    }])?;
    let pairs = pairs_dir(&[("zh.ja.txt", "猫が寝ています。\n")])?;
    let out = TempDir::new("r8f-out")?;
    run_eval(&server, &pairs, &out, &mut MemorySink::default())?;
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    let body: Value = serde_json::from_slice(request_body(&requests[0]))?;
    assert_eq!(
        body["messages"],
        serde_json::json!([{"role": "user", "content": "猫が寝ています。"}])
    );
    assert_eq!(
        body["chat_template_kwargs"],
        serde_json::json!({"source_lang_code": "ja", "target_lang_code": "zh"})
    );
    assert_eq!(body["stop"], serde_json::json!(["</s>"]));
    assert_eq!(body["temperature"], 0.0);
    assert_eq!(body["cache_prompt"], true);
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
    // T and E for English, T alone for Chinese; none for the other.
    assert_eq!(server.requests().len(), 3);
    Ok(())
}

/// The pair of the test above: when every language of the directory is in the
/// table, nothing is said to be left out. The written event is counted too,
/// so that a count of none is not a misspelt event name.
#[test]
fn pairs_of_listed_languages_only_emit_no_skipped_event() -> TestResult {
    let rows = [
        ("en", "こんにちは。", "Hello.", "Hello there."),
        ("zh", "猫が寝ています。", "猫在睡觉。", "猫在睡觉。"),
        ("ko", "犬が走ります。", "개가 달려요.", "개가 달려요."),
        (
            "es",
            "鳥が飛びます。",
            "El pájaro vuela.",
            "El pájaro vuela.",
        ),
    ];
    let server = scripted_server(
        rows.iter()
            .map(|(_, source, translation, edit)| Row {
                source,
                translation,
                edit,
            })
            .collect(),
    )?;
    let files: Vec<(String, String)> = rows
        .iter()
        .map(|(code, source, _, _)| (format!("{code}.ja.txt"), format!("{source}\n")))
        .collect();
    let named: Vec<(&str, &str)> = files
        .iter()
        .map(|(name, text)| (name.as_str(), text.as_str()))
        .collect();
    let pairs = pairs_dir(&named)?;
    let out = TempDir::new("r8f-out")?;
    let mut sink = MemorySink::default();
    let report = run_eval(&server, &pairs, &out, &mut sink)?;
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    let count = |name: &str| {
        sink.events
            .iter()
            .filter(|event| event.event == name)
            .count()
    };
    assert_eq!(count(eval::SKIPPED_EVENT), 0);
    assert_eq!(count(eval::WRITTEN_EVENT), 4);
    for (code, ..) in &rows {
        assert!(out.join(&format!("{code}.hyp.txt")).exists(), "{code}");
    }
    // T and E for English, T alone for the three others.
    assert_eq!(server.requests().len(), 5);
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

//! R8f: `live --translate` translates a Japanese line into the languages it
//! names. The relay is started the way `cli::run` starts it (`start_live`
//! with the settings `parse_args` returned), its own `/events` is read, and
//! the handle stops it.
//!
//! A scripted server stands in for llama-server: it answers a request for
//! Chinese with a Chinese sentence and any other with an English one.

mod support;

use std::time::{Duration, Instant};

use hayamimi_relay::cli::{Command, Live, parse_args, start_live};
use hayamimi_relay::lang::Lang;
use hayamimi_relay::logging::SharedSink;
use hayamimi_relay::pipeline::sha256_hex;
use hayamimi_relay::relay::Relay;
use hayamimi_relay::strings::Strings;
use serde_json::Value;
use support::events::{Collected, FakeRecognizer, Logs, collect_from, translations_of};
use support::{FakeServer, Reply, TestResult, chat_reply, request_body};

const SOURCE: &str = "猫が寝ています。";
const CHINESE: &str = "猫在睡觉。";
const ENGLISH: &str = "The cat is sleeping.";
const FALLBACK_SKIPPED: &str =
    r#"{"type":"fallback","stage":"translate","reason":"unsupported_target","to":"skipped"}"#;
const WAIT: Duration = Duration::from_secs(10);

/// The user message of a chat request.
fn user_message(raw: &[u8]) -> Option<String> {
    let body: Value = serde_json::from_slice(request_body(raw)).ok()?;
    body["messages"][0]["content"].as_str().map(str::to_owned)
}

/// The target language code a request hands to the chat template, when it
/// does (the form of the model that writes the languages outside the pair).
fn target_code(raw: &[u8]) -> Option<String> {
    let body: Value = serde_json::from_slice(request_body(raw)).ok()?;
    body["chat_template_kwargs"]["target_lang_code"]
        .as_str()
        .map(str::to_owned)
}

fn scripted_server() -> std::io::Result<FakeServer> {
    FakeServer::start(|_, raw| match (user_message(raw), target_code(raw)) {
        (Some(_), Some(code)) if code == "zh" => Reply::json(&chat_reply(CHINESE)),
        (Some(_), _) => Reply::json(&chat_reply(ENGLISH)),
        (None, _) => Reply::status(400),
    })
}

fn final_line() -> String {
    serde_json::json!({
        "type": "final", "id": "mic-1", "source": "mic", "lang": "ja", "text": SOURCE,
        "t0_s": 0.5, "t1_s": 1.5, "end_wall_ms": 1_790_000_000_000_i64,
    })
    .to_string()
}

/// The settings `live` is given with `extra` after the streams and the server.
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

/// A relay started from the settings, and the two ways to see it.
struct Running {
    relay: Relay,
    collected: Collected,
    logs: Logs,
}

fn start(live: &Live) -> Result<Running, Box<dyn std::error::Error>> {
    let strings = Strings::embedded(Lang::En)?;
    let logs = Logs::default();
    let sink = SharedSink::new(logs.clone());
    let relay = start_live(live, 1_790_000_000, &strings, &sink)
        .map_err(|error| format!("live did not start: {error:?}"))?;
    let collected = collect_from(relay.address())?;
    Ok(Running {
        relay,
        collected,
        logs,
    })
}

/// The settled translations of `lang` among what arrived.
fn settled_in<'a>(got: &'a [(Instant, Value)], lang: &str) -> Vec<&'a Value> {
    translations_of(got)
        .into_iter()
        .map(|(_, value)| value)
        .filter(|value| value["lang"] == lang)
        .collect()
}

fn translations_in(got: &[(Instant, Value)], lang: &str) -> usize {
    got.iter()
        .filter(|(_, value)| value["type"] == "translation" && value["lang"] == lang)
        .count()
}

fn fallbacks(got: &[(Instant, Value)]) -> Vec<&Value> {
    got.iter()
        .map(|(_, value)| value)
        .filter(|value| value["type"] == "fallback")
        .collect()
}

#[test]
fn a_translate_option_outside_the_live_pair_publishes_a_translation_in_that_language() -> TestResult
{
    let (recognizer, server) = (FakeRecognizer::start()?, scripted_server()?);
    let running = start(&live_settings(
        &recognizer,
        &server,
        &["--translate", "zh"],
    )?)?;
    recognizer.send(&final_line());
    let got = running.collected.wait_for(1, WAIT);
    let chinese = settled_in(&got, "zh");
    assert_eq!(chinese.len(), 1, "{got:?}");
    let event = chinese[0];
    assert_eq!(event["id"], "mic-1");
    assert_eq!(event["src_sha256"], sha256_hex(SOURCE));
    assert_eq!(event["status"], "ok");
    assert_eq!(event["text"], CHINESE);
    // `--translate` replaces the set: no English draft or translation, and
    // no request for English reached the server.
    assert_eq!(translations_in(&got, "en"), 0, "{got:?}");
    let requests = server.requests();
    let first = requests.first().and_then(|raw| user_message(raw));
    assert_eq!(first.as_deref(), Some(SOURCE));
    assert_eq!(
        requests.first().and_then(|raw| target_code(raw)).as_deref(),
        Some("zh")
    );
    assert_eq!(requests.len(), 1, "one T and no E");
    running.relay.stop();
    Ok(())
}

#[test]
fn without_the_translate_option_no_translation_in_that_language_is_published() -> TestResult {
    let (recognizer, server) = (FakeRecognizer::start()?, scripted_server()?);
    let running = start(&live_settings(&recognizer, &server, &[])?)?;
    recognizer.send(&final_line());
    let got = running.collected.wait_for(1, WAIT);
    assert_eq!(settled_in(&got, "en").len(), 1, "{got:?}");
    assert_eq!(translations_in(&got, "zh"), 0, "{got:?}");
    running.relay.stop();
    Ok(())
}

#[test]
fn translate_en_and_zh_publishes_both() -> TestResult {
    let (recognizer, server) = (FakeRecognizer::start()?, scripted_server()?);
    let running = start(&live_settings(
        &recognizer,
        &server,
        &["--translate", "en,zh"],
    )?)?;
    recognizer.send(&final_line());
    let got = running.collected.wait_for(2, WAIT);
    assert_eq!(settled_in(&got, "en").len(), 1, "{got:?}");
    assert_eq!(settled_in(&got, "zh").len(), 1, "{got:?}");
    running.relay.stop();
    Ok(())
}

#[test]
fn an_unlisted_translate_language_is_skipped_with_a_kept_fallback_event_and_zh_goes_on()
-> TestResult {
    let (recognizer, server) = (FakeRecognizer::start()?, scripted_server()?);
    let running = start(&live_settings(
        &recognizer,
        &server,
        &["--translate", "xx,zh"],
    )?)?;
    assert_eq!(running.relay.skipped(), ["xx"]);
    recognizer.send(&final_line());
    let got = running.collected.wait_for(1, WAIT);
    assert_eq!(settled_in(&got, "zh").len(), 1, "{got:?}");

    // The log says it once, for `xx` and for no other language.
    let said = running.logs.named("translate.fallback.skipped");
    assert_eq!(said.len(), 1);
    assert_eq!(said[0].fields.get("target"), Some(&Value::from("xx")));
    assert_eq!(
        said[0].fields.get("reason"),
        Some(&Value::from("unsupported_target"))
    );
    assert_eq!(
        said[0].error_code.as_deref(),
        Some("HAYAMIMI_TRANSLATE_FALLBACK")
    );

    // A client that was there before the line and one that came after it both
    // get the one `fallback`, the second as a kept event.
    let wanted: Value = serde_json::from_str(FALLBACK_SKIPPED)?;
    let early = running.collected.snapshot();
    assert_eq!(fallbacks(&early), [&wanted]);
    let late = collect_from(running.relay.address())?;
    let after = late.wait_until(|got| !fallbacks(got).is_empty(), WAIT);
    assert_eq!(fallbacks(&after), [&wanted]);

    // Nothing was asked for `xx`: every request names Chinese.
    let asked: Vec<Option<String>> = server
        .requests()
        .iter()
        .map(|raw| target_code(raw))
        .collect();
    assert_eq!(asked.len(), 1);
    assert!(
        asked.iter().all(|code| code.as_deref() == Some("zh")),
        "{asked:?}"
    );
    running.relay.stop();
    Ok(())
}

#[test]
fn a_run_that_names_only_zh_has_no_fallback_event() -> TestResult {
    let (recognizer, server) = (FakeRecognizer::start()?, scripted_server()?);
    let running = start(&live_settings(
        &recognizer,
        &server,
        &["--translate", "zh"],
    )?)?;
    recognizer.send(&final_line());
    let got = running.collected.wait_for(1, WAIT);
    assert_eq!(settled_in(&got, "zh").len(), 1, "{got:?}");
    assert!(fallbacks(&got).is_empty());
    assert!(running.logs.named("translate.fallback.skipped").is_empty());
    running.relay.stop();
    Ok(())
}

/// Whether a text is Chinese: it has Han characters and no kana.
fn is_chinese(text: &str) -> bool {
    let han = text.chars().any(|c| ('\u{4E00}'..='\u{9FFF}').contains(&c));
    let kana = text.chars().any(|c| ('\u{3040}'..='\u{30FF}').contains(&c));
    han && !kana
}

/// The second llama-server at its default address, with TranslateGemma on it
/// (`units/hayamimi-llama-other.service`): one Japanese line, translated by
/// `live --translate zh`, comes out as one settled Chinese translation.
#[test]
#[ignore = "needs hayamimi-llama-other.service on 127.0.0.1:18081"]
fn the_real_second_server_translates_a_japanese_line_into_chinese() -> TestResult {
    let (recognizer, server) = (FakeRecognizer::start()?, scripted_server()?);
    let running = start(&live_settings(
        &recognizer,
        &server,
        &[
            "--translate",
            "zh",
            "--llm-other",
            hayamimi_relay::cli::DEFAULT_LLM_OTHER,
        ],
    )?)?;
    recognizer.send(&final_line());
    let got = running.collected.wait_for(1, Duration::from_secs(60));
    let chinese = settled_in(&got, "zh");
    assert_eq!(chinese.len(), 1, "{got:?}");
    let Some(text) = chinese[0]["text"].as_str() else {
        return Err(format!("the translation has no text: {got:?}").into());
    };
    assert_eq!(chinese[0]["status"], "ok", "{got:?}");
    assert!(is_chinese(text), "{text}");
    println!("settled zh translation: {text}");
    assert!(
        server.requests().is_empty(),
        "the live pair's server was asked"
    );
    running.relay.stop();
    Ok(())
}

/// No server at the second address: the run goes on, the line of the language
/// outside the pair is held (its translation is not shown), and the run says
/// that the translator could not be reached, once for the line.
#[test]
fn an_absent_second_server_holds_the_line_and_says_the_translator_was_unreachable() -> TestResult {
    let (recognizer, server) = (FakeRecognizer::start()?, scripted_server()?);
    let running = start(&live_settings(
        &recognizer,
        &server,
        &["--translate", "zh", "--llm-other", "http://127.0.0.1:1"],
    )?)?;
    recognizer.send(&final_line());
    let got = running.collected.wait_for(1, WAIT);
    let shown = settled_in(&got, "zh");
    assert_eq!(shown.len(), 1, "{got:?}");
    assert_eq!(shown[0]["status"], "held", "{got:?}");
    assert_eq!(running.logs.named("translate.request.failed").len(), 1);
    running.relay.stop();
    Ok(())
}

fn held_wire() -> Value {
    serde_json::json!({
        "type": "fallback", "stage": "translate", "reason": "unreachable", "to": "held",
    })
}

fn cleared_wire() -> Value {
    serde_json::json!({"type": "fallback", "stage": "translate", "to": null})
}

/// A `final` line of the microphone with the given number in its `id`.
fn final_numbered(number: usize) -> String {
    serde_json::json!({
        "type": "final", "id": format!("mic-{number}"), "source": "mic", "lang": "ja",
        "text": SOURCE, "t0_s": 0.5, "t1_s": 1.5, "end_wall_ms": 1_790_000_000_000_i64,
    })
    .to_string()
}

/// The `fallback` messages among what arrived, as they were sent.
fn fallback_texts(got: &[(Instant, Value)]) -> Vec<Value> {
    fallbacks(got).into_iter().cloned().collect()
}

/// H45: no translator at the live pair's address. Every line is held and
/// fails on its own, and the run says once that the translator cannot be
/// reached, in the log and on the stream, for the screens that come later too.
#[test]
fn an_unreachable_translator_is_held_once_while_each_line_fails_on_its_own() -> TestResult {
    let (recognizer, server) = (FakeRecognizer::start()?, scripted_server()?);
    let running = start(&live_settings(
        &recognizer,
        &server,
        &["--llm", "http://127.0.0.1:1"],
    )?)?;
    recognizer.send(&final_numbered(1));
    recognizer.send(&final_numbered(2));
    let got = running.collected.wait_for(2, WAIT);
    let shown = settled_in(&got, "en");
    assert_eq!(shown.len(), 2, "the original lines go on: {got:?}");
    assert!(
        shown.iter().all(|event| event["status"] == "held"),
        "{got:?}"
    );
    let got = running
        .collected
        .wait_until(|got| !fallbacks(got).is_empty(), WAIT);
    assert_eq!(fallback_texts(&got), [held_wire()], "{got:?}");

    assert_eq!(running.logs.named("translate.request.failed").len(), 2);
    let held = running.logs.named("translate.fallback.held");
    assert_eq!(held.len(), 1);
    assert_eq!(
        held[0].error_code.as_deref(),
        Some("HAYAMIMI_TRANSLATE_FALLBACK")
    );
    assert_eq!(
        held[0].fields.get("reason"),
        Some(&Value::from("unreachable"))
    );

    let late = collect_from(running.relay.address())?;
    let after = late.wait_until(|got| !fallbacks(got).is_empty(), WAIT);
    assert_eq!(fallback_texts(&after), [held_wire()]);
    running.relay.stop();
    Ok(())
}

/// A translator that answers is not announced as anything.
#[test]
fn a_reachable_translator_says_nothing_about_a_fallback() -> TestResult {
    let (recognizer, server) = (FakeRecognizer::start()?, scripted_server()?);
    let running = start(&live_settings(&recognizer, &server, &[])?)?;
    recognizer.send(&final_numbered(1));
    recognizer.send(&final_numbered(2));
    let got = running.collected.wait_for(2, WAIT);
    assert_eq!(settled_in(&got, "en").len(), 2, "{got:?}");
    std::thread::sleep(Duration::from_millis(300));
    let got = running.collected.snapshot();
    assert!(fallbacks(&got).is_empty(), "{got:?}");
    assert!(running.logs.named("translate.fallback.held").is_empty());
    assert!(running.logs.named("translate.request.failed").is_empty());
    running.relay.stop();
    Ok(())
}

/// The second translator is watched on its own: with the live pair's server
/// answering, a missing second one is still said, once.
#[test]
fn an_unreachable_second_translator_is_held_once() -> TestResult {
    let (recognizer, server) = (FakeRecognizer::start()?, scripted_server()?);
    let running = start(&live_settings(
        &recognizer,
        &server,
        &["--translate", "en,zh", "--llm-other", "http://127.0.0.1:1"],
    )?)?;
    recognizer.send(&final_numbered(1));
    recognizer.send(&final_numbered(2));
    let got = running.collected.wait_for(4, WAIT);
    assert_eq!(settled_in(&got, "en").len(), 2, "{got:?}");
    assert!(
        settled_in(&got, "zh")
            .iter()
            .all(|event| event["status"] == "held")
    );
    let got = running
        .collected
        .wait_until(|got| !fallbacks(got).is_empty(), WAIT);
    assert_eq!(fallback_texts(&got), [held_wire()], "{got:?}");
    assert_eq!(running.logs.named("translate.fallback.held").len(), 1);
    assert_eq!(running.logs.named("translate.request.failed").len(), 2);
    running.relay.stop();
    Ok(())
}

/// A translator that comes back is said once, and the kept `fallback` ends.
#[test]
fn a_translator_that_comes_back_clears_the_fallback_once() -> TestResult {
    let recognizer = FakeRecognizer::start()?;
    let server = FakeServer::start(|index, raw| match (index, user_message(raw)) {
        (0, _) => Reply::status(503),
        (_, Some(_)) => Reply::json(&chat_reply(ENGLISH)),
        (_, None) => Reply::status(400),
    })?;
    let running = start(&live_settings(&recognizer, &server, &[])?)?;
    recognizer.send(&final_numbered(1));
    let got = running
        .collected
        .wait_until(|got| fallback_texts(got) == [held_wire()], WAIT);
    assert_eq!(fallback_texts(&got), [held_wire()], "{got:?}");
    recognizer.send(&final_numbered(2));
    let got = running
        .collected
        .wait_until(|got| fallbacks(got).len() == 2, WAIT);
    assert_eq!(
        fallback_texts(&got),
        [held_wire(), cleared_wire()],
        "{got:?}"
    );
    assert_eq!(running.logs.named("translate.fallback.held").len(), 1);
    assert_eq!(running.logs.named("translate.fallback.cleared").len(), 1);

    let late = collect_from(running.relay.address())?;
    std::thread::sleep(Duration::from_millis(200));
    assert!(
        fallbacks(&late.snapshot()).is_empty(),
        "nothing is kept in effect"
    );
    running.relay.stop();
    Ok(())
}

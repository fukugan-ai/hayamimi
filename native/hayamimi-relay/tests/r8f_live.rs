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
    let relay = start_live(live, 1_790_000_000, &strings, &sink)?;
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

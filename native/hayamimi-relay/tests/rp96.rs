//! RP-96: a translation request that went out on a connection the server had
//! closed is sent again once, so the line is translated and the translator is
//! not reported unreachable.
//!
//! The relay is started the way `cli::run` starts it (`start_live` with the
//! settings `parse_args` returned), its own `/events` is read, and the handle
//! stops it. A scripted server stands in for llama-server: it hangs up on the
//! requests a test names, after reading the whole request, and answers every
//! other with a Chinese sentence.

mod support;

use std::time::{Duration, Instant};

use hayamimi_relay::cli::{Command, Live, parse_args, start_live};
use hayamimi_relay::lang::Lang;
use hayamimi_relay::logging::SharedSink;
use hayamimi_relay::relay::Relay;
use hayamimi_relay::strings::Strings;
use serde_json::Value;
use support::events::{
    Collected, FakeRecognizer, Logs, collect_from, translations_of, unused_port,
};
use support::{FakeServer, Reply, TestResult, chat_reply};

const SOURCE: &str = "猫が寝ています。";
const CHINESE: &str = "猫在睡觉。";
const WAIT: Duration = Duration::from_secs(10);

/// A server that hangs up on the requests `hang_up` is true for (counting
/// from 0) and answers every other with the Chinese sentence.
fn server_hanging_up(hang_up: fn(usize) -> bool) -> std::io::Result<FakeServer> {
    FakeServer::start(move |index, _| {
        if hang_up(index) {
            Reply::hang_up()
        } else {
            Reply::json(&chat_reply(CHINESE))
        }
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

/// T-RP-96-001: a `final` line of the microphone, translated into Chinese by a
/// server whose first request is hung up on, comes out as an `ok` translation: the request
/// is sent again once, the log says so, and the translator is not held.
#[test]
fn rp96_a_closed_connection_is_asked_again_and_the_line_is_translated() -> TestResult {
    let (recognizer, server) = (
        FakeRecognizer::start()?,
        server_hanging_up(|index| index == 0)?,
    );
    let running = start(&live_settings(
        &recognizer,
        &server,
        &["--translate", "zh"],
    )?)?;
    recognizer.send(&final_line());
    let got = running.collected.wait_for(1, WAIT);
    let chinese = settled_in(&got, "zh");
    assert_eq!(chinese.len(), 1, "{got:?}");
    assert_eq!(chinese[0]["status"], "ok", "{got:?}");
    assert_eq!(chinese[0]["text"], CHINESE);
    assert_eq!(
        server.requests().len(),
        2,
        "the try that was hung up on and the one sent again"
    );
    let retried = running.logs.named("translate.request.retried");
    assert_eq!(retried.len(), 1, "{retried:?}");
    assert_eq!(
        retried[0].fields.get("cause"),
        Some(&Value::from("unexpected_eof"))
    );
    assert!(running.logs.named("translate.request.failed").is_empty());
    assert!(running.logs.named("translate.fallback.held").is_empty());
    running.relay.stop();
    Ok(())
}

/// T-RP-96-002: a server that hangs up on every request. The request is sent
/// again once and no more, then the line is held and the failure is said.
#[test]
fn a_server_that_always_hangs_up_holds_the_line_after_one_more_try() -> TestResult {
    let (recognizer, server) = (FakeRecognizer::start()?, server_hanging_up(|_| true)?);
    let running = start(&live_settings(
        &recognizer,
        &server,
        &["--translate", "zh"],
    )?)?;
    recognizer.send(&final_line());
    let got = running.collected.wait_for(1, WAIT);
    let chinese = settled_in(&got, "zh");
    assert_eq!(chinese.len(), 1, "{got:?}");
    assert_eq!(chinese[0]["status"], "held", "{got:?}");
    assert_eq!(server.requests().len(), 2, "one try and one more");
    assert_eq!(running.logs.named("translate.request.retried").len(), 1);
    let failed = running.logs.named("translate.request.failed");
    assert_eq!(failed.len(), 1, "{failed:?}");
    assert_eq!(
        failed[0].fields.get("cause"),
        Some(&Value::from("unexpected_eof"))
    );
    running.relay.stop();
    Ok(())
}

/// T-RP-96-003: nothing listens at the second address (a port that was bound
/// and let go). Most machines refuse the connection; a machine whose firewall drops it lets
/// the request time out instead. Neither is a closed connection, so the
/// request is not sent again.
#[test]
fn a_refused_connection_is_not_asked_again() -> TestResult {
    let (recognizer, server) = (FakeRecognizer::start()?, server_hanging_up(|_| false)?);
    let closed = format!("http://127.0.0.1:{}", unused_port()?);
    let running = start(&live_settings(
        &recognizer,
        &server,
        &["--translate", "zh", "--llm-other", &closed],
    )?)?;
    recognizer.send(&final_line());
    let got = running.collected.wait_for(1, WAIT);
    let chinese = settled_in(&got, "zh");
    assert_eq!(chinese.len(), 1, "{got:?}");
    assert_eq!(chinese[0]["status"], "held", "{got:?}");
    let failed = running.logs.named("translate.request.failed");
    assert_eq!(failed.len(), 1, "{failed:?}");
    let cause = failed[0].fields.get("cause").and_then(Value::as_str);
    assert!(
        matches!(cause, Some("connection_refused" | "timeout")),
        "{failed:?}"
    );
    assert!(running.logs.named("translate.request.retried").is_empty());
    running.relay.stop();
    Ok(())
}

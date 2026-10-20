//! The first requests of a run go through the start-up path of `live`
//! (`announce_after_first_request`): the form of T, then the form of E, with
//! the time limit of the first requests and not the 1.5 s limit of a live
//! subtitle.
//!
//! A fake translator listens on a port the system picks. In one test it
//! answers its first request after 3 s, later than `REQUEST_TIMEOUT` and
//! sooner than `warmup::TIMEOUT`.

mod support;

use std::error::Error;
use std::net::SocketAddr;
use std::time::Duration;

use hayamimi_relay::cli::{Command, Live, announce_after_first_request, parse_args};
use hayamimi_relay::lang::Lang;
use hayamimi_relay::llm::REQUEST_TIMEOUT;
use hayamimi_relay::logging::MemorySink;
use hayamimi_relay::strings::Strings;
use hayamimi_relay::warmup;
use support::{FakeServer, Reply, TestResult, chat_reply, request_body};

/// Later than a live subtitle waits, sooner than the first request gives up.
const SLOW_ANSWER: Duration = Duration::from_secs(3);

fn live_against(url: &str) -> Result<Live, String> {
    let args = ["live", "--llm", url].map(str::to_owned);
    match parse_args(&args) {
        Ok(Command::Run(live)) => Ok(live),
        other => Err(format!("live was not parsed: {other:?}")),
    }
}

/// The names of the events one start logs, in order.
fn names_logged_at_start(server: &FakeServer) -> Result<Vec<String>, String> {
    let live = live_against(&server.base_url())?;
    let mut sink = MemorySink::default();
    // The address is only announced; nothing listens on it.
    let address = SocketAddr::from(([127, 0, 0, 1], live.serve));
    announce_after_first_request(&live, address, &mut sink, &Strings::embedded(Lang::En)?);
    Ok(sink.events.into_iter().map(|event| event.event).collect())
}

/// The `(role, content)` of every message of a request, in order.
fn messages_of(raw: &[u8]) -> Result<Vec<(String, String)>, Box<dyn Error>> {
    let body: serde_json::Value = serde_json::from_slice(request_body(raw))?;
    let messages = body["messages"]
        .as_array()
        .ok_or("the body has no messages")?;
    messages
        .iter()
        .map(|message| {
            let text = |key: &str| message[key].as_str().map(str::to_owned);
            text("role")
                .zip(text("content"))
                .ok_or_else(|| Box::<dyn Error>::from("a message lacks its role or content"))
        })
        .collect()
}

#[test]
fn a_translator_that_answers_in_three_seconds_still_warms_up_at_start() -> TestResult {
    assert!(REQUEST_TIMEOUT < SLOW_ANSWER && SLOW_ANSWER < warmup::TIMEOUT);
    let server = FakeServer::start(|nth, _| {
        if nth == 0 {
            std::thread::sleep(SLOW_ANSWER);
        }
        Reply::json(&chat_reply("Hello."))
    })?;
    let names = names_logged_at_start(&server)?;
    assert_eq!(
        names,
        [
            warmup::SENT_EVENT,
            warmup::SENT_EVENT,
            "relay.serve.started"
        ]
    );
    assert_eq!(server.requests().len(), 2);
    Ok(())
}

#[test]
fn the_start_sends_t_as_one_user_message_then_e_as_a_system_and_a_user_message() -> TestResult {
    let server = FakeServer::start(|_, _| Reply::json(&chat_reply("Hello.")))?;
    names_logged_at_start(&server)?;
    let requests = server.requests();
    let [first, second] = requests.as_slice() else {
        return Err(format!("{} requests, not 2", requests.len()).into());
    };
    let user = ("user".to_owned(), warmup::prompt());
    assert_eq!(messages_of(first)?, std::slice::from_ref(&user));
    let system = ("system".to_owned(), warmup::system()?);
    assert_eq!(messages_of(second)?, [system, user]);
    Ok(())
}

#[test]
fn a_failed_e_request_still_lets_the_relay_say_it_serves() -> TestResult {
    let server = FakeServer::start(|nth, _| match nth {
        0 => Reply::json(&chat_reply("Hello.")),
        _ => Reply::status(500),
    })?;
    let names = names_logged_at_start(&server)?;
    assert_eq!(
        names,
        [
            warmup::SENT_EVENT,
            warmup::FAILED_EVENT,
            "relay.serve.started"
        ]
    );
    assert_eq!(server.requests().len(), 2);
    Ok(())
}

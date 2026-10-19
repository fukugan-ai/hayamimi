//! The first request of a run goes through the start-up path of `live`
//! (`announce_after_first_request`), with the time limit of the first request
//! and not the 1.5 s limit of a live subtitle.
//!
//! A fake translator answers after 3 s, later than `REQUEST_TIMEOUT` and
//! sooner than `warmup::TIMEOUT`. It listens on a port the system picks.

mod support;

use std::net::SocketAddr;
use std::time::Duration;

use hayamimi_relay::cli::{Command, Live, announce_after_first_request, parse_args};
use hayamimi_relay::lang::Lang;
use hayamimi_relay::llm::REQUEST_TIMEOUT;
use hayamimi_relay::logging::MemorySink;
use hayamimi_relay::strings::Strings;
use hayamimi_relay::warmup;
use support::{FakeServer, Reply, TestResult, chat_reply};

/// Later than a live subtitle waits, sooner than the first request gives up.
const SLOW_ANSWER: Duration = Duration::from_secs(3);

fn live_against(url: &str) -> Result<Live, String> {
    let args = ["live", "--llm", url].map(str::to_owned);
    match parse_args(&args) {
        Ok(Command::Run(live)) => Ok(live),
        other => Err(format!("live was not parsed: {other:?}")),
    }
}

#[test]
fn a_translator_that_answers_in_three_seconds_still_warms_up_at_start() -> TestResult {
    assert!(REQUEST_TIMEOUT < SLOW_ANSWER && SLOW_ANSWER < warmup::TIMEOUT);
    let server = FakeServer::start(|_, _| {
        std::thread::sleep(SLOW_ANSWER);
        Reply::json(&chat_reply("Hello."))
    })?;
    let live = live_against(&server.base_url())?;
    let mut sink = MemorySink::default();
    // The address is only announced; nothing listens on it.
    let address = SocketAddr::from(([127, 0, 0, 1], live.serve));
    announce_after_first_request(&live, address, &mut sink, &Strings::embedded(Lang::En)?);
    let names: Vec<&str> = sink
        .events
        .iter()
        .map(|event| event.event.as_str())
        .collect();
    assert_eq!(names, [warmup::SENT_EVENT, "relay.serve.started"]);
    assert_eq!(server.requests().len(), 1);
    Ok(())
}

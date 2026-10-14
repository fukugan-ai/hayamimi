//! CS-21: two sources are translated at the same time and no line is lost.
//!
//! The relay follows two recognizers (the microphone and the loopback) and
//! translates through one translator that serves two requests at once. Every
//! `final` and every `refine` the recognizers sent must come back as exactly
//! one translation that was made, not withheld.
//!
//! The recognizers are fake: they serve `/events` in the wire form of the
//! Python server (`support/events.rs`). The run is a few seconds long and has
//! no audio; the ten-minute run with real recognizers belongs to the end to
//! end acceptance (E2E-1, E2E-2).

mod support;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use hayamimi_relay::relay::Lanes;
use serde_json::Value;
use support::TestResult;
use support::events::{FakeLlm, FakeRecognizer, Meeting, Sent, check_delivery, run_relay};

const FINALS_PER_SOURCE: usize = 40;
const REFINES_PER_SOURCE: usize = 10;
/// How long the first request waits for a second one to be in flight.
const MEETING_WAIT: Duration = Duration::from_secs(2);
const ALL_ARRIVE_WITHIN: Duration = Duration::from_secs(30);

type Arrived = Vec<(Instant, Value)>;

/// Both recognizers send their finals and then their refines, one source
/// after the other for each number, as two live recognizers would interleave.
fn send_all(mic: &FakeRecognizer, loopback: &FakeRecognizer) -> Vec<Sent> {
    let mut sent = Vec::new();
    for (of, count) in [("final", FINALS_PER_SOURCE), ("refine", REFINES_PER_SOURCE)] {
        for number in 1..=count {
            sent.push(Sent::mic(number, of).send_through(mic));
            sent.push(Sent::loopback(number, of).send_through(loopback));
        }
    }
    sent
}

/// Runs the relay in `lanes` against two recognizers and returns what was
/// sent, what arrived, and how many requests the translator took.
fn run(lanes: Lanes) -> Result<(Vec<Sent>, Arrived, usize), Box<dyn std::error::Error>> {
    let (mic, loopback) = (FakeRecognizer::start()?, FakeRecognizer::start()?);
    let llm =
        Arc::new(FakeLlm::new(|_| Duration::from_millis(5)).meeting(Meeting::new(MEETING_WAIT)));
    let running = run_relay(vec![mic.url(), loopback.url()], lanes, Arc::clone(&llm))?;
    let sent = send_all(&mic, &loopback);
    let arrived = running.translations.wait_for(sent.len(), ALL_ARRIVE_WITHIN);
    Ok((sent, arrived, llm.calls.load(Ordering::SeqCst)))
}

/// One lane for each source: every line comes back translated.
#[test]
fn cs21_every_line_gets_exactly_one_translation() -> TestResult {
    let (sent, arrived, requests) = run(Lanes::PerSource)?;
    assert_eq!(sent.len(), 2 * (FINALS_PER_SOURCE + REFINES_PER_SOURCE));
    check_delivery(&sent, &arrived)?;
    // Each line went through the translator and the editor once.
    assert_eq!(requests, 2 * sent.len());
    Ok(())
}

/// The negative control. With one lane for both sources the translator is
/// never asked twice at once, its first request goes unanswered, and that line
/// is withheld instead of translated.
#[test]
fn cs21_one_lane_for_two_sources_loses_a_line() -> TestResult {
    let (sent, arrived, _) = run(Lanes::One)?;
    let fault = check_delivery(&sent, &arrived).err();
    assert!(
        fault
            .as_deref()
            .is_some_and(|text| text.contains("status is \"held\"")),
        "{fault:?}"
    );
    Ok(())
}

//! CS-22: a translation that is late is still attached to its own line.
//!
//! The translator answers the first line of the microphone 1200 ms late. The
//! second line of the microphone arrives 100 ms after the first, and a line of
//! the loopback arrives with the first. Each translation must name the line it
//! was made from, by `id` and by the digest of its source text, however late
//! it comes.

mod support;

use std::sync::Arc;
use std::thread::sleep;
use std::time::{Duration, Instant};

use hayamimi_relay::relay::Lanes;
use serde_json::Value;
use support::TestResult;
use support::events::{
    FakeLlm, FakeRecognizer, Sent, Stage, check_delivery, run_relay, translations_of,
};

const LATE_BY: Duration = Duration::from_millis(1200);
const SECOND_LINE_AFTER: Duration = Duration::from_millis(100);
const ALL_ARRIVE_WITHIN: Duration = Duration::from_secs(10);

type Arrived = Vec<(Instant, Value)>;

/// The three lines in the order they were sent, and what arrived.
struct Run {
    first: Sent,
    loopback: Sent,
    second: Sent,
    arrived: Arrived,
}

/// Runs the relay with a translator that is late for the first line only.
fn run() -> Result<Run, Box<dyn std::error::Error>> {
    let (mic, loopback) = (FakeRecognizer::start()?, FakeRecognizer::start()?);
    let late_text = Sent::mic(1, "final").text;
    let llm = Arc::new(FakeLlm::new(move |call| {
        if call.stage == Stage::Translate && call.text == late_text {
            LATE_BY
        } else {
            Duration::ZERO
        }
    }));
    let running = run_relay(vec![mic.url(), loopback.url()], Lanes::PerSource, llm)?;
    let first = Sent::mic(1, "final").send_through(&mic);
    let loopback_line = Sent::loopback(1, "final").send_through(&loopback);
    sleep(SECOND_LINE_AFTER);
    let second = Sent::mic(2, "final").send_through(&mic);
    let arrived = running.translations.wait_for(3, ALL_ARRIVE_WITHIN);
    Ok(Run {
        first,
        loopback: loopback_line,
        second,
        arrived,
    })
}

/// When the translation of `line` arrived.
fn arrival_of(line: &Sent, arrived: &Arrived) -> Result<Instant, String> {
    translations_of(arrived)
        .into_iter()
        .find(|(_, value)| value["id"] == line.id.as_str())
        .map(|(at, _)| *at)
        .ok_or_else(|| format!("{} was never translated", line.id))
}

/// The line a reader would attach each translation to if it attached it to
/// the latest line of the same source received before the translation came:
/// the translation's `id` and the `id` of that line.
fn attached_to_the_latest_line(sent: &[&Sent], arrived: &Arrived) -> Vec<(String, String)> {
    translations_of(arrived)
        .into_iter()
        .filter_map(|(at, value)| {
            let id = value["id"].as_str()?;
            let source = value["source"].as_str()?;
            let latest = sent
                .iter()
                .filter(|line| line.source == source && line.at <= *at)
                .max_by_key(|line| line.at)?;
            Some((id.to_owned(), latest.id.clone()))
        })
        .collect()
}

/// The late translation of the first line carries the first line's id and
/// text, the second line's translation carries its own, and the loopback is
/// not held up by the microphone.
#[test]
fn cs22_late_translation_attaches_to_its_own_line() -> TestResult {
    let run = run()?;
    let sent = [run.first.clone(), run.loopback.clone(), run.second.clone()];
    check_delivery(&sent, &run.arrived)?;
    let first_at = arrival_of(&run.first, &run.arrived)?;
    assert!(
        first_at.duration_since(run.first.at) >= LATE_BY,
        "the first line came back in {:?}, so the translator was not late",
        first_at.duration_since(run.first.at)
    );
    let loopback_at = arrival_of(&run.loopback, &run.arrived)?;
    assert!(
        loopback_at < first_at,
        "the loopback waited for the microphone"
    );
    Ok(())
}

/// Lines of one source are translated in the order they arrived, so the
/// second line of the microphone comes after the late first one.
#[test]
fn cs22_lines_of_one_source_come_back_in_order() -> TestResult {
    let run = run()?;
    let first_at = arrival_of(&run.first, &run.arrived)?;
    let second_at = arrival_of(&run.second, &run.arrived)?;
    assert!(first_at < second_at);
    Ok(())
}

/// The negative control: a build that attached a translation to the latest
/// line of its source would put the late translation of the first line on the
/// second line. The run above is one that tells the two apart.
#[test]
fn cs22_attaching_to_the_latest_line_would_be_caught() -> TestResult {
    let run = run()?;
    let sent = [&run.first, &run.loopback, &run.second];
    let attached = attached_to_the_latest_line(&sent, &run.arrived);
    let wrong: Vec<&(String, String)> = attached
        .iter()
        .filter(|(own, latest)| own != latest)
        .collect();
    assert_eq!(
        wrong,
        [&(String::from("mic-1"), String::from("mic-2"))],
        "{attached:?}"
    );
    Ok(())
}

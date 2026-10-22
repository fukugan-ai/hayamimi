//! IP-1: one source whose lines are in two languages is translated both ways,
//! each line into the other language.
//!
//! The relay follows one fake recognizer and translates through the fake
//! translator of `support/events.rs`; there is no audio and no model. A
//! recording played through the recognizer has the source `file`, which the
//! relay translates whichever of the two languages a line is in. The
//! microphone translates only Japanese and the loopback only English, and the
//! cases for them are the negative controls.
//!
//!     cd native && cargo test --release --jobs 2 -p hayamimi-relay --test ip01

mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use hayamimi_relay::logging::LogEvent;
use hayamimi_relay::relay::Lanes;
use serde_json::Value;
use support::TestResult;
use support::events::{
    FakeLlm, FakeRecognizer, Logs, Sent, check_delivery, run_relay, translation_of, translations_of,
};

const LINES: usize = 10;
const SPACING: Duration = Duration::from_millis(300);
const ALL_ARRIVE_WITHIN: Duration = Duration::from_secs(30);
const SKIPPED: &str = "translate.line.skipped";

type Arrived = Vec<(Instant, Value)>;

/// Japanese on the odd numbers, English on the even ones.
fn alternating(seq: usize) -> &'static str {
    if seq % 2 == 1 { "ja" } else { "en" }
}

/// What a run sent, and what came back: the translations that arrived and the
/// `translate.line.skipped` events the relay logged.
struct Run {
    sent: Vec<Sent>,
    arrived: Arrived,
    skipped: Vec<LogEvent>,
}

/// The `translate.line.skipped` events once `logs` holds `count` of them, or
/// all it holds when `ALL_ARRIVE_WITHIN` has passed.
fn skipped_after_waiting(logs: &Logs, count: usize) -> Vec<LogEvent> {
    let began = Instant::now();
    loop {
        let found = logs.named(SKIPPED);
        if found.len() >= count || began.elapsed() > ALL_ARRIVE_WITHIN {
            return found;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Sends `lines` through one fake recognizer, `spacing` apart, to a relay with
/// one lane for each source, and waits for `translations` of them to arrive
/// and for `skipped` lines to be noted.
fn play(
    lines: Vec<Sent>,
    spacing: Duration,
    translations: usize,
    skipped: usize,
) -> Result<Run, Box<dyn std::error::Error>> {
    let recognizer = FakeRecognizer::start()?;
    let llm = Arc::new(FakeLlm::new(|_| Duration::ZERO));
    let running = run_relay(vec![recognizer.url()], Lanes::PerSource, llm)?;
    let mut sent = Vec::with_capacity(lines.len());
    for line in lines {
        sent.push(line.send_through(&recognizer));
        std::thread::sleep(spacing);
    }
    let arrived = running
        .translations
        .wait_for(translations, ALL_ARRIVE_WITHIN);
    let skipped = skipped_after_waiting(&running.logs, skipped);
    Ok(Run {
        sent,
        arrived,
        skipped,
    })
}

/// Ten lines of `source`, Japanese and English in turn, the first in Japanese.
fn ten_in_turn(source: &'static str) -> Vec<Sent> {
    (1..=LINES)
        .map(|seq| Sent::spoken(source, seq, "final", alternating(seq)))
        .collect()
}

fn other_language(lang: &str) -> &'static str {
    if lang == "ja" { "en" } else { "ja" }
}

/// Every translation says which language it came from and which it was made in.
fn check_directions(sent: &[Sent], arrived: &Arrived) -> Result<(), String> {
    let translations: Vec<&Value> = translations_of(arrived)
        .into_iter()
        .map(|(_, value)| value)
        .collect();
    for line in sent {
        let found = translation_of(line, &translations)?;
        let (from, into) = (line.lang, other_language(line.lang));
        if found["src_lang"] != from || found["lang"] != into || found["settled"] != true {
            return Err(format!(
                "{}: translated in the wrong direction: {found}",
                line.id
            ));
        }
    }
    Ok(())
}

fn all_skipped_for_language(skipped: &[LogEvent]) -> bool {
    skipped.iter().all(|event| {
        event
            .fields
            .get("reason")
            .is_some_and(|reason| *reason == "language")
    })
}

fn of_language(sent: &[Sent], lang: &str) -> Vec<Sent> {
    sent.iter()
        .filter(|line| line.lang == lang)
        .cloned()
        .collect()
}

#[test]
fn ip01_one_source_is_translated_both_ways_by_line_language() -> TestResult {
    let run = play(ten_in_turn("file"), SPACING, LINES, 0)?;
    check_delivery(&run.sent, &run.arrived)?;
    check_directions(&run.sent, &run.arrived)?;
    assert_eq!(translations_of(&run.arrived).len(), LINES);
    assert!(
        run.skipped.is_empty(),
        "{} lines were skipped",
        run.skipped.len()
    );
    Ok(())
}

#[test]
fn ten_alternating_file_lines_sent_back_to_back_each_get_one_settled_translation() -> TestResult {
    let run = play(ten_in_turn("file"), Duration::ZERO, LINES, 0)?;
    check_delivery(&run.sent, &run.arrived)?;
    check_directions(&run.sent, &run.arrived)?;
    assert!(
        run.skipped.is_empty(),
        "{} lines were skipped",
        run.skipped.len()
    );
    Ok(())
}

#[test]
fn a_microphone_line_in_english_is_noted_and_not_translated() -> TestResult {
    let run = play(ten_in_turn("mic"), SPACING, LINES / 2, LINES / 2)?;
    let japanese = of_language(&run.sent, "ja");
    check_delivery(&japanese, &run.arrived)?;
    check_directions(&japanese, &run.arrived)?;
    assert_eq!(run.skipped.len(), LINES / 2);
    assert!(all_skipped_for_language(&run.skipped));
    Ok(())
}

#[test]
fn a_loopback_line_in_japanese_is_noted_and_not_translated() -> TestResult {
    let run = play(ten_in_turn("loopback"), SPACING, LINES / 2, LINES / 2)?;
    let english = of_language(&run.sent, "en");
    check_delivery(&english, &run.arrived)?;
    check_directions(&english, &run.arrived)?;
    assert_eq!(run.skipped.len(), LINES / 2);
    assert!(all_skipped_for_language(&run.skipped));
    Ok(())
}

#[test]
fn a_file_line_in_a_third_language_is_noted_and_the_next_line_is_translated() -> TestResult {
    let lines = vec![Sent::file(1, "final", "zh"), Sent::file(2, "final", "ja")];
    let run = play(lines, Duration::ZERO, 1, 1)?;
    let translated = of_language(&run.sent, "ja");
    check_delivery(&translated, &run.arrived)?;
    check_directions(&translated, &run.arrived)?;
    assert_eq!(translations_of(&run.arrived).len(), 1);
    assert_eq!(run.skipped.len(), 1);
    assert!(all_skipped_for_language(&run.skipped));
    Ok(())
}

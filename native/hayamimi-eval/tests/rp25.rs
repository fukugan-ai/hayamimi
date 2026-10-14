//! RP-25: a translation moves at most once in front of the reader (draft to
//! settled), never after it settled, a prefix drawn in the settled form never
//! vanishes, and each destination outside gets one caption per `id`, equal to
//! the settled text.

mod support;

use hayamimi_eval::flicker::{
    Event, Fault, Sent, Shown, check_flicker, count_replaced, count_vanished_partial_prefixes,
    shown_from_wire,
};
use support::TestResult;

fn shown(id: &str, text: &str, settled: bool, committed: usize) -> Event {
    Event::Shown(Shown {
        id: id.to_owned(),
        text: text.to_owned(),
        settled,
        committed,
    })
}

fn draft(id: &str, text: &str) -> Event {
    shown(id, text, false, 0)
}

fn settled(id: &str, text: &str) -> Event {
    shown(id, text, true, 0)
}

fn sent(id: &str, to: &str, text: &str) -> Event {
    Event::Sent(Sent {
        id: id.to_owned(),
        to: to.to_owned(),
        text: text.to_owned(),
    })
}

fn named(id: &str) -> String {
    id.to_owned()
}

#[test]
fn one_replacement_from_early_to_final_passes() {
    let run = [draft("a", "T text"), settled("a", "E text")];
    assert_eq!(check_flicker(&run), Ok(()));
}

#[test]
fn two_replacements_before_final_are_rejected() {
    let run = [
        draft("a", "first"),
        draft("a", "second"),
        settled("a", "third"),
    ];
    assert_eq!(
        check_flicker(&run),
        Err(Fault::TooManyReplacements(named("a")))
    );
}

#[test]
fn change_after_final_is_rejected() {
    let run = [draft("a", "T"), settled("a", "E"), settled("a", "E2")];
    assert_eq!(
        check_flicker(&run),
        Err(Fault::ChangedAfterSettled(named("a")))
    );
}

#[test]
fn vanished_committed_prefix_is_rejected() {
    let run = [shown("a", "Hello wor", false, 5), draft("a", "Hi there")];
    assert_eq!(check_flicker(&run), Err(Fault::VanishedPrefix(named("a"))));
    let kept = [shown("a", "Hello wor", false, 5), draft("a", "Hello world")];
    assert_eq!(check_flicker(&kept), Ok(()));
}

#[test]
fn one_send_per_id_equal_to_final_passes() {
    let run = [
        draft("a", "T"),
        settled("a", "E"),
        sent("a", "zoom", "E"),
        sent("a", "teams", "E"),
        sent("a", "srt", "E"),
    ];
    assert_eq!(check_flicker(&run), Ok(()));
}

#[test]
fn two_sends_for_one_id_are_rejected() {
    let run = [
        settled("a", "E"),
        sent("a", "zoom", "E"),
        sent("a", "zoom", "E"),
    ];
    assert_eq!(check_flicker(&run), Err(Fault::SentTwice(named("a"))));
}

#[test]
fn early_translation_sent_outside_is_rejected() {
    let run = [draft("a", "T"), sent("a", "zoom", "T"), settled("a", "E")];
    assert_eq!(check_flicker(&run), Err(Fault::SentNotSettled(named("a"))));
}

#[test]
fn counter_reads_two_utterances_with_different_early_and_final() {
    let run = [
        draft("a", "T1"),
        settled("a", "E1"),
        draft("b", "T2"),
        settled("b", "E2"),
        draft("c", "same"),
        settled("c", "same"),
    ];
    assert_eq!(count_replaced(&run), 2);
}

#[test]
fn held_e_keeps_the_early_translation_as_final() {
    let run = [draft("a", "T"), settled("a", "T"), sent("a", "zoom", "T")];
    assert_eq!(check_flicker(&run), Ok(()));
    assert_eq!(count_replaced(&run), 0);
}

/// The wire events of the recorded RP-3 run, each as the JSON text the relay sent.
fn wire_events_of_the_run() -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let text = support::read(&support::rp3_run("events.jsonl"))?;
    let mut wire = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let row: serde_json::Value = serde_json::from_str(line)?;
        let Some(event) = row.get("event") else {
            return Err("a line of the events file has no `event`".into());
        };
        wire.push(event.to_string());
    }
    Ok(wire)
}

/// T-RP-25-010: the real run passes the judgements of 001 to 009, and no
/// prefix drawn in the settled form vanished from a translation in progress.
/// The destinations outside (Zoom, Teams, SRT) are not part of this run, so
/// the `Sent` events are none.
#[test]
#[ignore = "reads the recorded RP-3 run of the Rust relay"]
fn rp25_translation_flicker_is_bounded() -> TestResult {
    let wire = wire_events_of_the_run()?;
    let mut run = Vec::new();
    for json in &wire {
        if let Some(item) = shown_from_wire(json).map_err(|fault| format!("{fault:?}"))? {
            run.push(Event::Shown(item));
        }
    }
    // The controls: the run did show lines, and it did translate text in progress.
    assert!(!run.is_empty(), "no translation of a line in the run");
    let vanished = count_vanished_partial_prefixes(wire.iter().map(String::as_str))
        .map_err(|f| format!("{f:?}"))?;
    let partials = wire
        .iter()
        .filter(|json| json.contains(r#""of":"partial""#))
        .count();
    assert!(
        partials > 0,
        "no translation of the text in progress in the run"
    );
    eprintln!(
        "rp25: {} lines shown, {} replaced draft to settled, {partials} partial translations, {vanished} vanished prefixes",
        run.len(),
        count_replaced(&run)
    );
    assert_eq!(check_flicker(&run), Ok(()));
    assert_eq!(vanished, 0);
    Ok(())
}

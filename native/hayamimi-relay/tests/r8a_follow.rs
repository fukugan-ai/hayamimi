//! R8a: the relay follows a recognizer's `/events` across starts and
//! disconnections, uses only the events that are lines, and says what it
//! leaves out.
//!
//! The recognizer is fake (`support/events.rs`).

mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use hayamimi_relay::relay::Lanes;
use serde_json::{Value, json};
use support::TestResult;
use support::events::{
    FakeLlm, FakeRecognizer, Logs, Running, Sent, check_delivery, run_relay, translations_of,
    unused_port,
};

const SOON: Duration = Duration::from_secs(10);

/// A relay that follows one stream and translates without delay.
fn relay_following(url: String) -> Result<Running, std::io::Error> {
    let llm = Arc::new(FakeLlm::new(|_| Duration::ZERO));
    run_relay(vec![url], Lanes::PerSource, llm)
}

/// Waits until `count` events named `name` have been logged.
fn wait_for_logged(logs: &Logs, name: &str, count: usize) -> Result<(), String> {
    let deadline = Instant::now() + SOON;
    while logs.named(name).len() < count {
        if Instant::now() > deadline {
            return Err(format!(
                "{name} was logged {} times, not {count}",
                logs.named(name).len()
            ));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}

fn reasons(logs: &Logs, name: &str) -> Vec<String> {
    logs.named(name)
        .iter()
        .filter_map(|event| event.fields.get("reason")?.as_str().map(str::to_owned))
        .collect()
}

/// A recognizer that is not running when the relay starts is waited for, and
/// followed once it runs.
#[test]
fn r8a_a_recognizer_that_starts_after_the_relay_is_followed() -> TestResult {
    let port = unused_port()?;
    let running = relay_following(format!("http://127.0.0.1:{port}/events"))?;
    wait_for_logged(&running.logs, "events.subscribe.lost", 1)?;
    assert_eq!(
        reasons(&running.logs, "events.subscribe.lost")[0],
        "connect"
    );
    assert!(running.logs.named("events.subscribe.opened").is_empty());
    let recognizer = FakeRecognizer::start_on(port)?;
    let line = Sent::mic(1, "final").send_through(&recognizer);
    let arrived = running.translations.wait_for(1, SOON);
    check_delivery(&[line], &arrived)?;
    Ok(())
}

/// A recognizer that hangs up is followed again, and the line it sends next
/// is translated.
#[test]
fn r8a_a_recognizer_that_hangs_up_is_followed_again() -> TestResult {
    let recognizer = FakeRecognizer::start()?;
    let running = relay_following(recognizer.url())?;
    let before = Sent::mic(1, "final").send_through(&recognizer);
    running.translations.wait_for(1, SOON);
    recognizer.hang_up();
    let after = Sent::mic(2, "final").send_through(&recognizer);
    let arrived = running.translations.wait_for(2, SOON);
    check_delivery(&[before, after], &arrived)?;
    assert_eq!(recognizer.connections(), 2);
    assert_eq!(running.logs.named("events.subscribe.opened").len(), 2);
    assert_eq!(reasons(&running.logs, "events.subscribe.lost")[0], "closed");
    Ok(())
}

/// Events that are not lines make no translation. A line the relay cannot use
/// is named with the reason, and the lines after it are still translated.
#[test]
fn r8a_only_lines_are_translated_and_a_line_that_cannot_be_used_is_named() -> TestResult {
    let recognizer = FakeRecognizer::start()?;
    let running = relay_following(recognizer.url())?;
    for event in [
        json!({"type": "partial", "text": "あ"}),
        json!({"type": "fallback", "stage": "punct", "reason": "no_device", "to": "cpu"}),
        json!({"type": "session_start"}),
        json!({"type": "final", "source": "mic", "lang": "ja", "text": "ああああ。"}),
        json!({"type": "final", "id": "radio-1", "source": "radio", "lang": "ja", "text": "ああああ。"}),
        json!({"type": "final", "id": "mic-7", "source": "mic", "lang": "ja"}),
    ] {
        recognizer.send(&event.to_string());
    }
    recognizer.send("this is not json");
    let line = Sent::mic(1, "final").send_through(&recognizer);
    let arrived = running.translations.wait_for(1, SOON);
    check_delivery(&[line], &arrived)?;
    let mut named = reasons(&running.logs, "events.line.skipped");
    named.sort();
    assert_eq!(named, ["no_id", "no_source", "no_text", "not_json"]);
    Ok(())
}

/// A line in a language the relay does not translate, or one the source does
/// not translate, is noted and makes no translation; the next line is
/// translated as usual.
#[test]
fn r8a_a_line_that_is_not_translated_is_only_noted() -> TestResult {
    let recognizer = FakeRecognizer::start()?;
    let running = relay_following(recognizer.url())?;
    for (id, lang) in [("loopback-8", "zh"), ("loopback-9", "ja")] {
        let event = json!({"type": "final", "id": id, "source": "loopback", "lang": lang, "text": "あいう"});
        recognizer.send(&event.to_string());
    }
    let line = Sent::loopback(1, "final").send_through(&recognizer);
    let arrived = running.translations.wait_for(1, SOON);
    check_delivery(&[line], &arrived)?;
    assert_eq!(
        reasons(&running.logs, "translate.line.skipped"),
        ["language", "language"]
    );
    assert_eq!(translations_of(&arrived).len(), 1);
    Ok(())
}

/// What the relay publishes is JSON with the type `translation`, and it
/// carries the digest of the text it translated.
#[test]
fn r8a_the_published_translation_has_the_wire_form_of_the_design() -> TestResult {
    let recognizer = FakeRecognizer::start()?;
    let running = relay_following(recognizer.url())?;
    let line = Sent::mic(3, "refine").send_through(&recognizer);
    let arrived = running.translations.wait_for(1, SOON);
    let Some((_, event)) = arrived.first() else {
        return Err("no translation arrived".into());
    };
    let expected = json!({
        "type": "translation",
        "id": "mic-3",
        "of": "refine",
        "source": "mic",
        "src_lang": "ja",
        "lang": "en",
        "src_sha256": line.digest(),
        "status": "ok",
        "hold_reason": null,
        "text": line.translated(),
        "annotations": [],
        "skipped": [],
    });
    assert_eq!(event, &expected as &Value);
    Ok(())
}

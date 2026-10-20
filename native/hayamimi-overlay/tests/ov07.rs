//! OV-7: a stream that drops comes back without a restart, and the overlay
//! keeps drawing while it is gone.
//!
//! The test serves the two recognizers' `/events` and the relay's `/events`
//! itself, on free ports, and starts the real overlay against them.
//!
//! Run A: lines are drawn with their translations while everything is up (the
//! positive control). The relay is stopped; a line that arrives while it is
//! down has no translation and is drawn as its source marked "untranslated"
//! after about 3000 ms. The relay is brought back 1000 ms after it went down,
//! and a line published after the overlay has reconnected is drawn with its
//! translation within 6000 ms of the relay coming back. The process never
//! exits.
//!
//! Run B (negative control): the relay is stopped and never brought back. The
//! check of run A, a translated line, then fails, and the line is drawn
//! untranslated instead, so the check is not satisfied by accident and the
//! overlay did not die.
//!
//! Opens a window: run it with `--ignored` when nobody is using the screen.

#![cfg(windows)]

mod support;

use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::Value;
use support::{
    Log, Overlay, Sse, Streams, TestResult, final_line, rendered, rendered_translated, scratch,
    sleep_ms, text, translation_line,
};

/// How long after the relay is back a translated line may take (design 11.8).
const TRANSLATED_WITHIN_MS: u128 = 6000;
/// The relay is brought back this long after it went down (design 11.8).
const RELAY_BACK_AFTER_MS: u128 = 1000;
/// A line without a translation is drawn as its source after this long (design 11.3,
/// `TR_WAIT_MS`), allowing the overlay's own tick on top.
const UNTRANSLATED_AFTER_MS: std::ops::RangeInclusive<u128> = 2500..=6000;
/// The longest run A waits for both outcomes.
const GIVE_UP: Duration = Duration::from_millis(15000);

fn untranslated(event: &Value, id: &str) -> bool {
    rendered(event, id) && text(event, "state") == Some("untranslated")
}

/// A running overlay with the two recognizers it follows. The relay is kept
/// apart, since the scenarios stop it.
struct Running {
    overlay: Overlay,
    mic: Sse,
    loopback: Sse,
}

/// Starts the overlay against three fake streams and waits until it follows all of them.
fn bring_up(dir: &Path, name: &str) -> Result<(Running, Sse), String> {
    let streams = Streams::start().map_err(|error| error.to_string())?;
    let mut overlay = Overlay::launch(streams.args(), Log::at(dir.join(name)))?;
    overlay.wait_opened()?;
    streams.wait_followed()?;
    let Streams {
        mic,
        loopback,
        relay,
    } = streams;
    Ok((
        Running {
            overlay,
            mic,
            loopback,
        },
        relay,
    ))
}

/// Publishes a line as the recognizer of its source and the relay would.
fn publish(running: &Running, relay: &Sse, id: &str) {
    if id.starts_with("loopback-") {
        running
            .loopback
            .send(&final_line(id, "Thank you for joining us today", "en"));
        relay.send(&translation_line(
            id,
            "ja",
            "本日はご参加ありがとうございます",
        ));
    } else {
        running
            .mic
            .send(&final_line(id, "それでは始めましょう", "ja"));
        relay.send(&translation_line(id, "en", "Let us begin."));
    }
}

/// Waits until `id` is drawn with its translation.
fn wait_translated(overlay: &mut Overlay, id: &str, within: Duration) -> Result<(), String> {
    let wanted = id.to_owned();
    overlay
        .wait(
            &format!("{id} drawn with its translation"),
            within,
            move |event| rendered_translated(event, &wanted),
        )
        .map(|_| ())
}

/// When, in milliseconds, each outcome of run A was first seen.
struct Outcome {
    /// Since the line arrived: `loopback-2` drawn untranslated.
    untranslated_after: Option<u128>,
    /// Since the relay came back: `loopback-3` drawn with its translation.
    translated_after: Option<u128>,
}

/// Stops the relay, sends a line nobody translates, brings the relay back
/// 1000 ms after it went down, and watches for both outcomes in one loop so
/// that neither is timed from the wrong moment.
fn drop_and_restore_the_relay(running: &mut Running, relay: Sse) -> Result<Outcome, String> {
    let port = relay.port();
    relay.stop();
    let down_at = Instant::now();
    sleep_ms(100);
    running.loopback.send(&final_line(
        "loopback-2",
        "Can everyone see my screen",
        "en",
    ));
    let sent_at = Instant::now();

    let mut back: Option<(Sse, Instant)> = None;
    let mut published_3 = false;
    let mut outcome = Outcome {
        untranslated_after: None,
        translated_after: None,
    };
    while (outcome.untranslated_after.is_none() || outcome.translated_after.is_none())
        && sent_at.elapsed() < GIVE_UP
    {
        if back.is_none() && down_at.elapsed().as_millis() >= RELAY_BACK_AFTER_MS {
            let server = Sse::start_on(port).map_err(|error| {
                format!("could not bring the relay back on port {port}: {error}")
            })?;
            back = Some((server, Instant::now()));
        }
        let followed = back
            .as_ref()
            .filter(|(server, _)| !published_3 && server.connections() >= 1);
        if let Some((server, _)) = followed {
            publish(running, server, "loopback-3");
            published_3 = true;
        }
        let events = running.overlay.log().events();
        if outcome.untranslated_after.is_none()
            && events.iter().any(|event| untranslated(event, "loopback-2"))
        {
            outcome.untranslated_after = Some(sent_at.elapsed().as_millis());
        }
        if outcome.translated_after.is_none()
            && events
                .iter()
                .any(|event| rendered_translated(event, "loopback-3"))
        {
            outcome.translated_after = back.as_ref().map(|(_, at)| at.elapsed().as_millis());
        }
        sleep_ms(25);
    }
    Ok(outcome)
}

fn check_outcome(outcome: &Outcome, digest: &str) -> Result<(), String> {
    let Some(untranslated_after) = outcome.untranslated_after else {
        return Err(format!("loopback-2 was never drawn untranslated; {digest}"));
    };
    if !UNTRANSLATED_AFTER_MS.contains(&untranslated_after) {
        return Err(format!(
            "loopback-2 was drawn untranslated {untranslated_after} ms after it arrived, not within {UNTRANSLATED_AFTER_MS:?}"
        ));
    }
    let Some(translated_after) = outcome.translated_after else {
        return Err(format!(
            "loopback-3 was never drawn with its translation after the relay came back; {digest}"
        ));
    };
    if translated_after > TRANSLATED_WITHIN_MS {
        return Err(format!(
            "loopback-3 was drawn with its translation {translated_after} ms after the relay came back, not within {TRANSLATED_WITHIN_MS} ms"
        ));
    }
    Ok(())
}

/// Run A.
fn run_with_the_relay_coming_back(dir: &Path) -> Result<(), String> {
    let (mut running, relay) = bring_up(dir, "a.jsonl")?;

    // Positive control: nothing is stopped, and every line is drawn with its translation.
    publish(&running, &relay, "loopback-1");
    publish(&running, &relay, "mic-1");
    wait_translated(
        &mut running.overlay,
        "loopback-1",
        Duration::from_millis(6000),
    )?;
    wait_translated(&mut running.overlay, "mic-1", Duration::from_millis(6000))?;

    let outcome = drop_and_restore_the_relay(&mut running, relay)?;
    let alive = running.overlay.is_running();
    let digest = running.overlay.log().digest();
    running.overlay.close()?;
    if !alive {
        return Err(format!(
            "the overlay exited while the relay was down; {digest}"
        ));
    }
    check_outcome(&outcome, &digest)
}

/// Run B: the relay never comes back.
fn run_without_the_relay_coming_back(dir: &Path) -> Result<(), String> {
    let (mut running, relay) = bring_up(dir, "b.jsonl")?;
    relay.stop();
    sleep_ms(100);
    running.loopback.send(&final_line(
        "loopback-9",
        "Can everyone see my screen",
        "en",
    ));

    // The check of run A, for a line that cannot be translated, must fail.
    let translated = wait_translated(
        &mut running.overlay,
        "loopback-9",
        Duration::from_millis(6000),
    );
    // The overlay is alive and drew the line, as its source.
    let drawn = running.overlay.wait(
        "loopback-9 drawn untranslated",
        Duration::from_millis(6000),
        |event| untranslated(event, "loopback-9"),
    );
    running.overlay.close()?;
    if translated.is_ok() {
        return Err(String::from(
            "a line with no relay was drawn translated; the check passes by itself",
        ));
    }
    drawn.map(|_| ())
}

#[test]
#[ignore = "opens a window; run it when nobody is using the screen"]
fn ov07_a_dropped_stream_comes_back_without_a_restart() -> TestResult {
    let dir = scratch()?;
    run_with_the_relay_coming_back(dir.path())?;
    run_without_the_relay_coming_back(dir.path())?;
    Ok(())
}

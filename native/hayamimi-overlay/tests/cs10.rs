//! CS-10: both audio sources share one overlay, loopback above and the
//! microphone below.
//!
//! This test covers the overlay's part of CS-10. The recognizers and the relay
//! are fake `/events` servers of the test, so a run needs no GPU, no
//! translator and no audio; the lines are those that CS-1 and CS-2 produce, one
//! `mic-` and one `loopback-` row, each with its deciding translation. What it
//! shows is the overlay's: it follows both recognizers and the relay, joins
//! every row to its translation by `id`, and draws loopback rows in the upper
//! lane and microphone rows in the lower one. Playing the two wavs through the
//! real recognizers and the real relay is the end-to-end run of the same ID.
//!
//! Positive control: with only one source sending, only that source appears.
//! Negative control: with the loopback recognizer stopped, no `loopback-` row
//! is drawn, although the relay still sends a translation for one.
//!
//! Opens a window: run it with `--ignored` when nobody is using the screen.

#![cfg(windows)]

mod support;

use std::path::Path;
use std::time::Duration;

use support::{
    Log, Overlay, Streams, TestResult, final_line, is_event, rendered, rendered_translated,
    scratch, sleep_ms, text, translation_line,
};

const DRAWN_WITHIN: Duration = Duration::from_millis(6000);
/// How long a source that should not appear is watched.
const QUIET_MS: u64 = 1500;

fn lane_of_translated_row(overlay: &Overlay, id: &str) -> Option<String> {
    overlay
        .log()
        .events()
        .iter()
        .find(|event| rendered_translated(event, id))
        .and_then(|event| text(event, "lane").map(str::to_owned))
}

fn wait_translated(overlay: &mut Overlay, id: &str) -> Result<(), String> {
    let wanted = id.to_owned();
    overlay
        .wait(
            &format!("{id} drawn with its translation"),
            DRAWN_WITHIN,
            move |event| rendered_translated(event, &wanted),
        )
        .map(|_| ())
}

fn start(streams: &Streams, dir: &Path, name: &str) -> Result<Overlay, String> {
    let mut overlay = Overlay::launch(streams.args(), Log::at(dir.join(name)))?;
    overlay.wait_opened()?;
    streams.wait_followed()?;
    Ok(overlay)
}

/// Both sources speak; then only the loopback speaks.
fn run_with_both_sources(dir: &Path) -> Result<(), String> {
    let streams = Streams::start().map_err(|error| error.to_string())?;
    let mut overlay = start(&streams, dir, "both.jsonl")?;

    streams
        .mic
        .send(&final_line("mic-1", "こんにちは、会議を始めます", "ja"));
    streams.relay.send(&translation_line(
        "mic-1",
        "en",
        "Hello, let us start the meeting.",
    ));
    streams
        .loopback
        .send(&final_line("loopback-1", "Thank you for joining us", "en"));
    streams.relay.send(&translation_line(
        "loopback-1",
        "ja",
        "ご参加ありがとうございます",
    ));
    wait_translated(&mut overlay, "mic-1")?;
    wait_translated(&mut overlay, "loopback-1")?;
    let (mic_lane, loopback_lane) = (
        lane_of_translated_row(&overlay, "mic-1"),
        lane_of_translated_row(&overlay, "loopback-1"),
    );

    // Positive control: one source alone, and only that source appears.
    streams
        .loopback
        .send(&final_line("loopback-2", "Let me share my screen", "en"));
    streams
        .relay
        .send(&translation_line("loopback-2", "ja", "画面を共有します"));
    wait_translated(&mut overlay, "loopback-2")?;
    sleep_ms(QUIET_MS);
    let mic_rows_after = overlay.log().count(|event| rendered(event, "mic-2"));
    overlay.close()?;

    if mic_lane.as_deref() != Some("lower") || loopback_lane.as_deref() != Some("upper") {
        return Err(format!(
            "the lanes are wrong: mic-1 in {mic_lane:?}, loopback-1 in {loopback_lane:?}"
        ));
    }
    if mic_rows_after != 0 {
        return Err(format!(
            "a microphone row was drawn although only the loopback spoke ({mic_rows_after} events)"
        ));
    }
    Ok(())
}

/// The loopback recognizer is stopped; the relay still sends a translation for a loopback row.
fn run_without_the_loopback(dir: &Path) -> Result<(), String> {
    let streams = Streams::start().map_err(|error| error.to_string())?;
    let mut overlay = start(&streams, dir, "no-loopback.jsonl")?;
    let Streams {
        mic,
        loopback,
        relay,
    } = streams;
    loopback.stop();

    mic.send(&final_line("mic-1", "こんにちは、会議を始めます", "ja"));
    relay.send(&translation_line(
        "mic-1",
        "en",
        "Hello, let us start the meeting.",
    ));
    relay.send(&translation_line(
        "loopback-1",
        "ja",
        "ご参加ありがとうございます",
    ));
    wait_translated(&mut overlay, "mic-1")?;
    sleep_ms(QUIET_MS);
    let loopback_rows = overlay.log().count(|event| {
        is_event(event, "overlay.line.rendered")
            && text(event, "id").is_some_and(|id| id.starts_with("loopback-"))
    });
    overlay.close()?;
    if loopback_rows != 0 {
        return Err(format!(
            "{loopback_rows} loopback drawings with the loopback recognizer stopped"
        ));
    }
    Ok(())
}

#[test]
#[ignore = "opens a window; run it when nobody is using the screen"]
fn cs10_both_sources_share_one_overlay() -> TestResult {
    let dir = scratch()?;
    run_with_both_sources(dir.path())?;
    run_without_the_loopback(dir.path())?;
    Ok(())
}

//! OV-2: a click on the band reaches the window below it.
//!
//! A plain window of the test is put under the band and counts the left
//! clicks it receives. The click is a real `SendInput` at the centre of the
//! band. Positive control: a click on the same window outside the band is
//! counted. Negative control: with a second plain window covering the band,
//! the same click is counted by the second window and not by the first.
//!
//! Opens windows and moves the pointer: run it with `--ignored` when nobody is
//! using the screen.

#![cfg(windows)]

mod support;

use std::time::{Duration, Instant};

use support::win::{self, Naive, Rect, Win};
use support::{Log, Overlay, TestResult, scratch, sleep_ms};

/// How long a click may take to be counted by the window it lands on.
const COUNTED_WITHIN: Duration = Duration::from_millis(1000);
/// How long the foreground window is watched after the click, and how often.
const WATCH: Duration = Duration::from_millis(1000);
const WATCH_EVERY_MS: u64 = 50;

fn wait_for_clicks(window: &Naive, wanted: u32) -> bool {
    let deadline = Instant::now() + COUNTED_WITHIN;
    while window.clicks() < wanted {
        if Instant::now() >= deadline {
            return false;
        }
        sleep_ms(10);
    }
    true
}

/// Whether `overlay` was the foreground window at any of the reads over `WATCH`.
fn ever_in_front(overlay: Win) -> bool {
    let end = Instant::now() + WATCH;
    let mut seen = false;
    while Instant::now() < end {
        seen = seen || win::foreground() == Some(overlay);
        sleep_ms(WATCH_EVERY_MS);
    }
    seen
}

/// The rectangle of a plain window that starts 200 px above the band and ends at its bottom.
fn under(band: Rect) -> Rect {
    Rect {
        left: band.left,
        top: band.top - 200,
        right: band.right,
        bottom: band.bottom,
    }
}

#[test]
#[ignore = "opens windows and moves the pointer; run it when nobody is using the screen"]
fn ov02_a_click_on_the_band_reaches_the_window_below() -> TestResult {
    win::per_monitor_aware()?;
    let dir = scratch()?;
    let mut overlay = Overlay::launch(
        ["--layout-probe"],
        Log::at(dir.path().join("overlay.jsonl")),
    )?;
    overlay.wait_opened()?;
    let overlay_win = overlay.window()?;
    let band = win::window_rect(overlay_win)?;
    let (centre_x, centre_y) = band.center();

    let below = Naive::open(under(band))?;

    // Positive control: a click on the plain window outside the band is counted.
    let outside = (band.left + 100, band.top - 100);
    win::click(outside.0, outside.1)?;
    assert!(
        wait_for_clicks(&below, 1),
        "a click outside the band was not counted by the plain window"
    );

    // The click on the band reaches the same window, once, and the overlay never takes the front.
    win::click(centre_x, centre_y)?;
    assert!(
        wait_for_clicks(&below, 2),
        "a click on the band did not reach the window below"
    );
    let overlay_took_the_front = ever_in_front(overlay_win);
    assert_eq!(
        below.clicks(),
        2,
        "one click on the band was counted more than once"
    );
    assert!(
        !overlay_took_the_front,
        "the overlay was in front after the click"
    );

    // Negative control: a second plain window covering the band takes the same click.
    let cover = Naive::open(band)?;
    win::make_topmost(cover.win())?;
    let before = below.clicks();
    win::click(centre_x, centre_y)?;
    assert!(
        wait_for_clicks(&cover, 1),
        "the window covering the band did not count the click"
    );
    assert_eq!(
        below.clicks(),
        before,
        "the window under the cover counted a click aimed at the cover"
    );

    drop(cover);
    drop(below);
    overlay.close()?;
    Ok(())
}

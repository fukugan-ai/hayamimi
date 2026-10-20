//! OV-2: a click on the band reaches the window below it.
//!
//! A plain window of the test is put under the band, in the topmost band, and
//! counts the left clicks it receives. The click is a real `SendInput` at the
//! centre of the band. Positive control: a click on the same window outside the
//! band is counted. Negative control: with a second plain window covering the
//! band, the same click is counted by the second window and not by the first.
//!
//! The plain window is topmost because a window that is not would be hidden by
//! whatever the user has maximised, and the click would land there. The band
//! climbs back above it (the overlay lifts itself every second), so that the
//! click still has to pass through the band to reach it.
//!
//! No input goes to a window the test did not make. `Pointer::click` looks at
//! the window under the pixel with `WindowFromPoint` right before the one
//! `SendInput` that moves the pointer and presses the button, and sends nothing
//! unless that window is the band or one of the test's plain windows. It also
//! sends nothing unless the user has been away 300 s (first click), and stops
//! when the pointer or the input clock shows that the user came back. The window
//! that was in front is put back in front when the test ends. Before any input is sent, the
//! test shows that the check works (the measure control below): it accepts a
//! pixel on the plain window and refuses a pixel on no window of the test.
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
/// How long the band is given to climb above a window that was just made topmost.
const CLIMB_WITHIN: Duration = Duration::from_millis(3000);

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

/// Whether `upper` stands above `lower` in the z-order.
fn is_above(upper: Win, lower: Win) -> Result<bool, String> {
    let order = win::z_order()?;
    let place = |window: Win| order.iter().position(|found| *found == window);
    match (place(upper), place(lower)) {
        (Some(up), Some(down)) => Ok(up < down),
        _ => Err(format!(
            "{upper:?} or {lower:?} is not among the top-level windows"
        )),
    }
}

/// Waits until the band stands above the plain window. If it never does, a
/// click at the band would reach the plain window without passing through the
/// band, and the test would show nothing.
fn wait_until_above(overlay: Win, plain: Win) -> Result<(), String> {
    let deadline = Instant::now() + CLIMB_WITHIN;
    while !is_above(overlay, plain)? {
        if Instant::now() >= deadline {
            return Err(String::from(
                "the measure does not work: the band did not climb above the topmost plain window, so a click would not have to pass through it",
            ));
        }
        sleep_ms(WATCH_EVERY_MS);
    }
    Ok(())
}

/// The click guard must look at the window under the pixel: it passes a pixel
/// of the plain window and refuses one that is on no window of the test, and
/// refuses the same pixel when the plain window is not named. Nothing is sent.
fn check_the_guard_sees(
    own: &[Win],
    on_plain: (i32, i32),
    elsewhere: (i32, i32),
) -> Result<(), String> {
    win::guard_click(on_plain.0, on_plain.1, own)?;
    if win::guard_click(elsewhere.0, elsewhere.1, own).is_ok() {
        return Err(format!(
            "the measure does not work: the click guard let {elsewhere:?} through although no window of the test is there; it checks nothing"
        ));
    }
    if win::guard_click(on_plain.0, on_plain.1, &[]).is_ok() {
        return Err(format!(
            "the measure does not work: the click guard let {on_plain:?} through with no window of the test named; it checks nothing"
        ));
    }
    Ok(())
}

/// The measure control of the click guard on its own: one plain window, and
/// no input at all (this test never calls `win::click`).
#[test]
#[ignore = "opens a window; run it when nobody is using the screen"]
fn ov02_the_click_guard_passes_only_the_tests_own_windows() -> TestResult {
    win::per_monitor_aware()?;
    let (_, work) = win::primary_monitor()?;
    let at = Rect {
        left: work.left + 200,
        top: work.top + 200,
        right: work.left + 700,
        bottom: work.top + 500,
    };
    let plain = Naive::open(at)?;
    win::make_topmost(plain.win())?;
    check_the_guard_sees(&[plain.win()], at.center(), (work.left + 2, work.top + 2))?;
    Ok(())
}

#[test]
#[ignore = "opens windows and moves the pointer; run it when nobody is using the screen"]
fn ov02_a_click_on_the_band_reaches_the_window_below() -> TestResult {
    win::per_monitor_aware()?;
    // Dropped last: the window that was in front goes back in front, also when the test fails.
    let _front = win::keep_foreground();
    let mut pointer = win::Pointer::new();
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
    win::make_topmost(below.win())?;
    wait_until_above(overlay_win, below.win())?;
    let own = [overlay_win, below.win()];

    // Measure control: the guard that stands between the test and the user's windows works.
    let outside = (band.left + 100, band.top - 100);
    let (_, work) = win::primary_monitor()?;
    check_the_guard_sees(&own, outside, (work.left + 2, work.top + 2))?;

    // Positive control: a click on the plain window outside the band is counted.
    pointer.click(outside.0, outside.1, &own)?;
    assert!(
        wait_for_clicks(&below, 1),
        "a click outside the band was not counted by the plain window"
    );

    // The click on the band reaches the same window, once, and the overlay never takes the front.
    pointer.click(centre_x, centre_y, &own)?;
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
    let covered = [overlay_win, below.win(), cover.win()];
    let before = below.clicks();
    pointer.click(centre_x, centre_y, &covered)?;
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

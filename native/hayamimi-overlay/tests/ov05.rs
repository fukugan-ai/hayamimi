//! OV-5: starting the overlay does not take the foreground.
//!
//! A plain window of the test is put in front, the overlay is started, and
//! 2000 ms after its window is up the foreground window must still be the plain
//! one.
//!
//! Bringing a window to the front can be refused by the system (the foreground
//! lock). The test therefore first shows that it can change the foreground at
//! all: a second plain window, shown and brought forward, must take the front.
//! If it does not, the reading proves nothing and the test fails as "the
//! measure does not work" with the class of the window that is in front.
//!
//! Opens windows: run it with `--ignored` when nobody is using the screen.

#![cfg(windows)]

mod support;

use support::win::{self, Naive, Rect, Win};
use support::{Log, Overlay, TestResult, scratch, sleep_ms};

/// How long after the window is up the foreground is read (design 11.8).
const READ_AFTER_MS: u64 = 2000;

const FIRST_AT: Rect = Rect {
    left: 200,
    top: 200,
    right: 700,
    bottom: 500,
};
const SECOND_AT: Rect = Rect {
    left: 800,
    top: 200,
    right: 1300,
    bottom: 500,
};

/// Shows `window`, clicks it (a click the test sends on its own window gives
/// it the right to take the front), asks for the front, and checks that it got it.
fn bring_forward(pointer: &mut win::Pointer, window: Win, what: &str) -> Result<(), String> {
    win::show(window);
    // A window a background process shows can land behind the user's window; the click must find it.
    win::make_topmost(window)?;
    let (centre_x, centre_y) = win::window_rect(window)?.center();
    pointer.click(centre_x, centre_y, &[window])?;
    win::set_foreground(window);
    sleep_ms(200);
    let front = win::foreground();
    if front == Some(window) {
        return Ok(());
    }
    let class = front.and_then(win::class_name);
    Err(format!(
        "the measure does not work: {what} did not take the front; the front window is {front:?} (class {class:?})"
    ))
}

#[test]
#[ignore = "opens windows; run it when nobody is using the screen"]
fn ov05_opening_the_overlay_keeps_the_foreground() -> TestResult {
    win::per_monitor_aware()?;
    // Dropped last: the window that was in front goes back in front, also when the test fails.
    let _front = win::keep_foreground();
    let mut pointer = win::Pointer::new();
    let first = Naive::open(FIRST_AT)?;

    // Positive control: before the overlay starts, the plain window is in front.
    bring_forward(&mut pointer, first.win(), "the first plain window")?;

    // Negative control: a second plain window shown the same way takes the front.
    let second = Naive::open(SECOND_AT)?;
    bring_forward(&mut pointer, second.win(), "the second plain window")?;
    drop(second);
    bring_forward(&mut pointer, first.win(), "the first plain window, again")?;

    let dir = scratch()?;
    let mut overlay = Overlay::launch(
        ["--layout-probe"],
        Log::at(dir.path().join("overlay.jsonl")),
    )?;
    overlay.wait_opened()?;
    sleep_ms(READ_AFTER_MS);
    let after = win::foreground();
    let class = after.and_then(win::class_name);
    let overlay_window = overlay.window()?;
    overlay.close()?;
    assert_eq!(
        after,
        Some(first.win()),
        "the front window changed while the overlay started; it is {after:?} (class {class:?}; the overlay is {overlay_window:?})"
    );
    Ok(())
}

//! OV-3: the overlay climbs back above a window that becomes topmost after it.
//!
//! The z-order is read with `EnumWindows` (top first). A topmost window made
//! later sits above the overlay until the overlay raises itself again; the test
//! reads right after the change and 2000 ms later.
//!
//! The reading right after must show the plain window in front. If the overlay
//! is already in front then, the reading cannot tell the overlay's raising from
//! no change at all, and the test fails as "the measure does not work" rather
//! than pass or fail the overlay.
//!
//! Opens windows: run it with `--ignored` when nobody is using the screen.

#![cfg(windows)]

mod support;

use support::win::{self, Naive, Rect, Win};
use support::{Log, Overlay, TestResult, scratch, sleep_ms};

/// How long the overlay is given to raise itself again (design 11.3: once a second).
const CLIMB_WITHIN_MS: u64 = 2000;

const PLAIN_AT: Rect = Rect {
    left: 200,
    top: 200,
    right: 700,
    bottom: 500,
};

/// Whether `upper` is above `lower` in the z-order, read now.
fn above(upper: Win, lower: Win) -> Result<bool, String> {
    let order = win::z_order()?;
    let place = |wanted: Win| {
        order
            .iter()
            .position(|found| *found == wanted)
            .ok_or_else(|| format!("window {wanted:?} is not among the top-level windows"))
    };
    Ok(place(upper)? < place(lower)?)
}

#[test]
#[ignore = "opens windows; run it when nobody is using the screen"]
fn ov03_overlay_climbs_back_above_a_new_topmost_window() -> TestResult {
    win::per_monitor_aware()?;
    let dir = scratch()?;
    let mut overlay = Overlay::launch(
        ["--layout-probe"],
        Log::at(dir.path().join("overlay.jsonl")),
    )?;
    overlay.wait_opened()?;
    let overlay_win = overlay.window()?;
    let plain = Naive::open(PLAIN_AT)?;

    // Positive control: a plain window brought to the front of the ordinary
    // windows is below the overlay at once.
    win::bring_to_top(plain.win())?;
    assert!(
        above(overlay_win, plain.win())?,
        "a window that is only in front of the ordinary windows got above the overlay"
    );

    // Negative control: right after the plain window is made topmost, the reading sees it in front.
    win::make_topmost(plain.win())?;
    if above(overlay_win, plain.win())? {
        return Err("the measure does not work: the overlay was already above the new topmost window right after SetWindowPos".into());
    }

    // The overlay raises itself again.
    sleep_ms(CLIMB_WITHIN_MS);
    let climbed = above(overlay_win, plain.win())?;
    drop(plain);
    overlay.close()?;
    assert!(
        climbed,
        "{CLIMB_WITHIN_MS} ms after a new topmost window appeared, the overlay was still below it"
    );
    Ok(())
}

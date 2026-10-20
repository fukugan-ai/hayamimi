//! OV-9: a window whose clicks cannot be passed through is never shown.
//!
//! The overlay is started with `--layout-probe --probe-click-through-unset`,
//! which makes the read-back of the click-through setting report "not set".
//! The window of such a run must never become visible, the process must end
//! with exit code 1 within 10000 ms, and the log must hold one
//! `overlay.window.click_through_failed` event with the error code
//! `HYM_BROWSER_CLICK_THROUGH_FAILED` and no `overlay.window.opened`. The
//! window of the overlay's class is looked for every 50 ms while the process
//! lives, so a window that flashes up before the exit is seen.
//!
//! Positive control: the same start without the flag opens the window, keeps
//! running, and its window carries `WS_EX_TRANSPARENT` and `WS_EX_LAYERED`.
//! The expectation of the main run, applied to that control, must fail.
//!
//! Opens a window (the control run does): run it with `--ignored` when nobody
//! is using the screen.

#![cfg(windows)]

mod support;

use std::time::{Duration, Instant};

use hayamimi_overlay::WINDOW_CLASS;
use serde_json::Value;
use support::win;
use support::{Log, Overlay, TestResult, is_event, scratch, sleep_ms, text};

/// How long the process may take to end (design 11.8).
const EXIT_WITHIN: Duration = Duration::from_millis(10_000);
/// How often the window is looked for (design 11.8).
const POLL_MS: u64 = 50;
const FAILED_EVENT: &str = "overlay.window.click_through_failed";
const FAILED_CODE: &str = "HYM_BROWSER_CLICK_THROUGH_FAILED";

/// What one run of the overlay left behind.
struct Outcome {
    /// `None` while the process was still running when the run was cut off.
    exit_code: Option<i32>,
    elapsed: Duration,
    /// A window of the overlay's class was visible at some poll.
    ever_visible: bool,
    events: Vec<Value>,
}

fn count(outcome: &Outcome, name: &str) -> usize {
    outcome
        .events
        .iter()
        .filter(|event| is_event(event, name))
        .count()
}

/// Whether a window of the overlay's class owned by `pid` is visible now.
fn visible_now(pid: u32) -> Result<bool, String> {
    Ok(win::windows_of(WINDOW_CLASS, pid)?
        .into_iter()
        .any(win::is_visible))
}

/// Starts the overlay and watches it until it ends or `limit` has passed.
fn run(args: &[&str], log: Log, limit: Duration) -> Result<Outcome, String> {
    let mut overlay = Overlay::launch(args, log)?;
    let started = Instant::now();
    let mut ever_visible = false;
    let exit_code = loop {
        ever_visible |= visible_now(overlay.pid())?;
        if let Some(code) = overlay.try_exit_code()? {
            break Some(code);
        }
        if started.elapsed() >= limit {
            break None;
        }
        sleep_ms(POLL_MS);
    };
    let outcome = Outcome {
        exit_code,
        elapsed: started.elapsed(),
        ever_visible,
        events: overlay.log().events(),
    };
    // A process still alive here is killed when `overlay` drops.
    Ok(outcome)
}

/// The expectation of the main run: ended with 1 in time, said why, never opened,
/// never visible.
fn judge(outcome: &Outcome) -> Result<(), String> {
    let mut found = Vec::new();
    match outcome.exit_code {
        Some(1) => {}
        Some(other) => found.push(format!("the exit code is {other}, not 1")),
        None => found.push(format!(
            "the process was still running after {:?}",
            outcome.elapsed
        )),
    }
    if outcome.elapsed > EXIT_WITHIN {
        found.push(format!(
            "it took {:?}, not {EXIT_WITHIN:?}",
            outcome.elapsed
        ));
    }
    let failed: Vec<&Value> = outcome
        .events
        .iter()
        .filter(|event| is_event(event, FAILED_EVENT))
        .collect();
    if failed.len() != 1 {
        found.push(format!("{} {FAILED_EVENT} events, not 1", failed.len()));
    }
    if !failed
        .iter()
        .all(|event| text(event, "error_code") == Some(FAILED_CODE))
    {
        found.push(format!("{FAILED_EVENT} lacks error_code {FAILED_CODE}"));
    }
    let opened = count(outcome, "overlay.window.opened");
    if opened != 0 {
        found.push(format!("{opened} overlay.window.opened events, not 0"));
    }
    if outcome.ever_visible {
        found.push(String::from("a window of the overlay was visible"));
    }
    if found.is_empty() {
        Ok(())
    } else {
        Err(found.join("; "))
    }
}

/// The control: opened, still running, and the window is layered and transparent.
fn run_the_control(dir: &std::path::Path) -> Result<Outcome, String> {
    let mut overlay = Overlay::launch(["--layout-probe"], Log::at(dir.join("control.jsonl")))?;
    overlay.wait_opened()?;
    let facts = win::facts(overlay.window()?)?;
    let wanted = win::EX_TRANSPARENT | win::EX_LAYERED;
    if facts.ex_style & wanted != wanted {
        return Err(format!(
            "without the flag the window should be layered and transparent: {facts:?}"
        ));
    }
    if !facts.visible {
        return Err(format!(
            "without the flag the window is not visible: {facts:?}"
        ));
    }
    let outcome = Outcome {
        exit_code: overlay.try_exit_code()?,
        elapsed: Duration::from_millis(
            u64::try_from(overlay.age_ms()).map_err(|error| error.to_string())?,
        ),
        ever_visible: facts.visible,
        events: overlay.log().events(),
    };
    overlay.close()?;
    Ok(outcome)
}

#[test]
#[ignore = "opens a window; run it when nobody is using the screen"]
fn ov09_click_through_unset_exits_without_showing_the_window() -> TestResult {
    win::per_monitor_aware()?;
    let dir = scratch()?;

    // Positive control: the window of a normal start is shown and keeps running,
    // and the expectation of the main run rejects it.
    let control = run_the_control(dir.path())?;
    assert!(
        control.exit_code.is_none(),
        "the control process should keep running: {:?}",
        control.exit_code
    );
    assert!(
        judge(&control).is_err(),
        "the expectation accepted a window that was shown and kept running"
    );

    // The run with the click-through read back as unset.
    let outcome = run(
        &["--layout-probe", "--probe-click-through-unset"],
        Log::at(dir.path().join("unset.jsonl")),
        EXIT_WITHIN + Duration::from_millis(2000),
    )?;
    judge(&outcome)?;
    Ok(())
}

#[cfg(test)]
mod expectation {
    use super::*;
    use serde_json::json;

    fn ended() -> Outcome {
        Outcome {
            exit_code: Some(1),
            elapsed: Duration::from_millis(900),
            ever_visible: false,
            events: vec![json!({"event": FAILED_EVENT, "error_code": FAILED_CODE})],
        }
    }

    #[test]
    fn the_expected_run_is_accepted() {
        assert_eq!(judge(&ended()), Ok(()));
    }

    #[test]
    fn a_visible_window_an_opened_event_or_another_code_is_rejected() {
        let mut shown = ended();
        shown.ever_visible = true;
        assert!(judge(&shown).is_err());
        let mut opened = ended();
        opened
            .events
            .push(json!({"event": "overlay.window.opened"}));
        assert!(judge(&opened).is_err());
        let mut other = ended();
        other.events = vec![json!({"event": FAILED_EVENT, "error_code": "PLACE_FAILED"})];
        assert!(judge(&other).is_err());
        let mut code = ended();
        code.exit_code = Some(0);
        assert!(judge(&code).is_err());
    }

    #[test]
    fn a_late_or_missing_exit_is_rejected() {
        let mut late = ended();
        late.elapsed = EXIT_WITHIN + Duration::from_millis(1);
        assert!(judge(&late).is_err());
        let mut running = ended();
        running.exit_code = None;
        assert!(judge(&running).is_err());
    }
}

//! OV-1: the attributes of the overlay's window read back from the OS.
//!
//! The window is found by class and process id and read with the Win32 calls
//! of `support::win`, never from what the overlay says about itself. A plain
//! `WS_OVERLAPPEDWINDOW` window of the test is read the same way as the
//! positive control, and the same expectation applied to it must fail.
//!
//! Opens a window: run it with `--ignored` when nobody is using the screen.

#![cfg(windows)]

mod support;

use support::win::{self, Facts, Rect};
use support::{Log, Overlay, TestResult, scratch};

/// Where the plain window of the controls goes (physical px; any free place will do).
const PLAIN_AT: Rect = Rect {
    left: 200,
    top: 200,
    right: 700,
    bottom: 500,
};

/// How `facts` differ from what design 11.8 expects of the overlay's window:
/// topmost, layered and transparent, not a tool window, no resize edge, no
/// frame (the client area is the whole window), visible.
fn violations(facts: &Facts) -> Vec<String> {
    let mut found = Vec::new();
    let bits = [
        (win::EX_TOPMOST, "WS_EX_TOPMOST", true),
        (win::EX_LAYERED, "WS_EX_LAYERED", true),
        (win::EX_TRANSPARENT, "WS_EX_TRANSPARENT", true),
        (win::EX_TOOLWINDOW, "WS_EX_TOOLWINDOW", false),
    ];
    for (bit, name, wanted) in bits {
        if (facts.ex_style & bit != 0) != wanted {
            found.push(format!(
                "{name} should be {}",
                if wanted { "set" } else { "clear" }
            ));
        }
    }
    if facts.style & win::STYLE_SIZEBOX != 0 {
        found.push(String::from("WS_SIZEBOX is set"));
    }
    if !facts.frameless() {
        found.push(format!(
            "client {:?} is not the window {}x{}",
            facts.client,
            facts.window.width(),
            facts.window.height()
        ));
    }
    if !facts.visible {
        found.push(String::from("the window is not visible"));
    }
    found
}

#[test]
#[ignore = "opens a window; run it when nobody is using the screen"]
fn ov01_window_styles_read_back_from_the_os() -> TestResult {
    win::per_monitor_aware()?;

    // Positive control: the reader sees a framed window as framed.
    let plain = win::Naive::open(PLAIN_AT)?;
    let plain_facts = win::facts(plain.win())?;
    assert!(
        plain_facts.client.0 < plain_facts.window.width()
            && plain_facts.client.1 < plain_facts.window.height(),
        "a titled window should have a client area smaller than its rectangle: {plain_facts:?}"
    );
    assert!(
        plain_facts.style & win::STYLE_SIZEBOX != 0,
        "a WS_OVERLAPPEDWINDOW window should carry WS_SIZEBOX: {plain_facts:?}"
    );

    // Negative control: the overlay's expectation fails for that plain window.
    let against_plain = violations(&plain_facts);
    assert!(
        against_plain.iter().any(|said| said.contains("WS_SIZEBOX"))
            && against_plain
                .iter()
                .any(|said| said.contains("WS_EX_TOPMOST")),
        "the expectation did not reject a plain window: {against_plain:?}"
    );
    drop(plain);

    // The overlay itself.
    let dir = scratch()?;
    let mut overlay = Overlay::launch(
        ["--layout-probe"],
        Log::at(dir.path().join("overlay.jsonl")),
    )?;
    overlay.wait_opened()?;
    let facts = win::facts(overlay.window()?)?;
    overlay.close()?;
    let found = violations(&facts);
    assert!(
        found.is_empty(),
        "the overlay's window is wrong: {found:?}; read {facts:?}"
    );
    Ok(())
}

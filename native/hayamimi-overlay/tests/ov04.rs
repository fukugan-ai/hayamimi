//! OV-4: the band sits on the work area and follows the settings file.
//!
//! The expected rectangle is worked out here from the work area (read with
//! `GetMonitorInfoW` by a per-monitor-aware process) and the numbers of design
//! 11.4, and compared to `GetWindowRect` within one physical pixel. Then the
//! settings file is rewritten while the overlay runs: a narrower band, a broken
//! file, and (in a second run) a monitor that does not exist.
//!
//! Opens a window: run it with `--ignored` when nobody is using the screen.

#![cfg(windows)]

mod support;

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use support::win::{self, Rect, Win};
use support::{
    BandSpec, DEFAULT_BAND, Log, Overlay, Streams, TestResult, expected_band, is_event, scratch,
    sleep_ms, text, within,
};

/// How long the overlay is given to follow a rewrite of the settings file (design 11.8).
const FOLLOWS_WITHIN: Duration = Duration::from_millis(3000);
const NARROW: BandSpec = BandSpec {
    width_ratio: 0.6,
    ..DEFAULT_BAND
};

/// The settings file with `width_ratio` and `monitor` set (everything else is the built-in).
fn settings(ratio: f64, monitor: &str) -> String {
    format!("monitor = '{monitor}'\nwidth_ratio = {ratio:?}\n")
}

fn wait_for_rect(window: Win, wanted: Rect) -> Result<(), String> {
    let deadline = Instant::now() + FOLLOWS_WITHIN;
    loop {
        let found = win::window_rect(window)?;
        if within(found, wanted, 1) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "the band is {found:?}, not {wanted:?}, {FOLLOWS_WITHIN:?} after the file changed"
            ));
        }
        sleep_ms(50);
    }
}

fn start(streams: &Streams, config: &Path, dir: &Path, name: &str) -> Result<Overlay, String> {
    let mut args = streams.args();
    args.push(String::from("--config"));
    args.push(config.to_string_lossy().into_owned());
    Overlay::launch(&args, Log::at(dir.join(name)))
}

#[test]
#[ignore = "opens a window; run it when nobody is using the screen"]
fn ov04_band_sits_on_the_work_area_and_follows_the_config() -> TestResult {
    win::per_monitor_aware()?;
    let (monitor, area) = win::primary_monitor()?;
    let dir = scratch()?;
    let config = dir.path().join("overlay.toml");
    fs::write(&config, settings(0.8, ""))?;
    let streams = Streams::start()?;

    let mut overlay = start(&streams, &config, dir.path(), "first.jsonl")?;
    overlay.wait_opened()?;
    let window = overlay.window()?;
    let scale = win::scale_of(window)?;
    let default_band = expected_band(area, scale, &DEFAULT_BAND);

    // Positive control: before any rewrite the band is where the default formula puts it.
    let before = win::window_rect(window)?;
    assert!(
        within(before, default_band, 1),
        "the band is {before:?}, the formula says {default_band:?}"
    );

    // Negative control: the same formula on the whole monitor gives another rectangle,
    // so the comparison can tell the work area from the screen.
    let from_screen = expected_band(monitor, scale, &DEFAULT_BAND);
    if within(from_screen, default_band, 1) {
        return Err("the measure does not work: the work area is the whole monitor (is the taskbar hidden?)".into());
    }
    assert!(
        !within(before, from_screen, 1),
        "the band was placed from the whole monitor, not the work area"
    );

    // A narrower band, followed within 3000 ms, announced once.
    fs::write(&config, settings(0.6, ""))?;
    wait_for_rect(window, expected_band(area, scale, &NARROW))?;
    sleep_ms(1500);
    let reloaded = overlay.log().named("overlay.config.reloaded").len();
    assert_eq!(
        reloaded, 1,
        "overlay.config.reloaded was written {reloaded} times for one rewrite"
    );

    // A broken file changes nothing and is reported once, by the table loader.
    fs::write(&config, "width_ratio = [\n")?;
    sleep_ms(3000);
    let kept = win::window_rect(window)?;
    let failed = overlay.log().count(|event| {
        is_event(event, "table.load.failed") && text(event, "table") == Some("overlay")
    });
    overlay.close()?;
    assert!(
        within(kept, expected_band(area, scale, &NARROW), 1),
        "a broken file moved the band to {kept:?}"
    );
    assert_eq!(
        failed, 1,
        "table.load.failed (table: overlay) was written {failed} times for one broken file"
    );

    // A monitor that does not exist puts the band on the primary monitor, with one warning.
    fs::write(&config, settings(0.8, "no-such-monitor-for-the-test"))?;
    let mut second = start(&streams, &config, dir.path(), "second.jsonl")?;
    second.wait_opened()?;
    let on_primary = win::window_rect(second.window()?)?;
    sleep_ms(1500);
    let missing = second.log().named("overlay.monitor.missing").len();
    second.close()?;
    assert!(
        within(on_primary, default_band, 1),
        "with a missing monitor the band is {on_primary:?}, not {default_band:?}"
    );
    assert_eq!(
        missing, 1,
        "overlay.monitor.missing was written {missing} times"
    );
    Ok(())
}

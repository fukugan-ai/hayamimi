//! CS-11: no text is cut off, and no frame or shadow shows around the band, at
//! 100%, 125%, 150% and 200%.
//!
//! For each scale the overlay is started with `--layout-probe --zoom <n>`: it
//! draws the three probe rows that reach lowest and reports the boxes it
//! measured. The test checks that
//!
//! - every row has `scrollHeight <= clientHeight`, and every row and lane has
//!   `bottom <= innerHeight`;
//! - the band's outer ring (8 px, inside and outside the band's edge) is the
//!   same on the screen before the window is shown and while it is up. The
//!   screen is grabbed three times before and three times while the window is up, so that
//!   a pixel that moves by itself is left out (a frame stays put while the
//!   window is up), and once more after the window is closed: a pixel that differs
//!   while the window is up and differs still after it is closed is the
//!   screen's own change (`drifted`), not a frame, a border or a shadow, which
//!   go with the window. The rows under the work area are the taskbar's, which
//!   draws a button for the window; they are counted apart (`taskbar_changed`)
//!   and not judged;
//!   A live screen can change and change back by itself at a pixel or two, so
//!   the ring of one scale is compared again, up to 3 times, until it is clean;
//!   a window that draws a frame, a border or a shadow draws it every time, and
//!   a text that is cut is never tried again;
//! - `innerHeight` is the height of design 11.4 (1 CSS px tolerance), and the
//!   probe drew something inside the band (positive controls).
//!
//! The band is expected where design 11.4 puts it at the scale the probe lays
//! it out at: the machine's scale times the zoom. Its physical height grows
//! with the zoom, its bottom edge stays on the bottom of the work area.
//!
//! Controls:
//!
//! - The ring is grabbed seven times with no window started in between, as a
//!   real run grabs it. The screen may change by itself (`drifted`), but a
//!   pixel that changes and then goes back would be taken for the window's
//!   doing: if there is one, or one under the taskbar rows, the test fails as
//!   "the measure does not work".
//! - The same probe with `--probe-shrink 12` shrinks the window by 12 CSS px,
//!   more than the 10.8 px margin under the band, so the lower lane must end
//!   below the window and the window must be 12 CSS px lower than the same
//!   probe without the shrink. If not, the measure cannot see a cut and the
//!   test fails as "the measure does not work".
//!
//! The scale of the machine is real; 100%, 125%, 150% and 200% are written
//! with `--zoom`. The probe at the machine's own scale without `--zoom` is
//! printed next to the `--zoom` probe of the same percentage, so that whether
//! the two agree is on record (design H29); it is not a pass or fail.
//!
//! Opens windows: run it with `--ignored` when nobody is using the screen.

#![cfg(windows)]

mod support;

use std::time::Duration;

use support::win::{self, Pixels, Rect};
use support::{
    DEFAULT_BAND, Grabs, Log, Measured, Overlay, RingGeometry, RingReport, TestResult,
    band_height_css, clipping, compare_ring, expected_band, is_event, parse_measured, scratch,
    shrunk_band, sleep_ms, within,
};

const ZOOMS: [u32; 4] = [100, 125, 150, 200];
/// The ring around the band's edge, in physical px (design 11.8).
const RING: usize = 8;
/// The shrink of the negative control, in CSS px.
const SHRINK: u32 = 12;
/// How often a failed screen grab is tried again.
const GRAB_TRIES: usize = 5;
/// How many grabs make up one state of the screen (before the window, with it up), and the gap between them.
const STEADY_GRABS: usize = 3;
const STEADY_GAP_MS: u64 = 100;
/// How often the ring comparison of one scale is made again when it is not clean.
const RING_ATTEMPTS: usize = 3;
/// The time the painting is given before the screen is grabbed after the window is up.
const SETTLE_MS: u64 = 700;
/// How long the screen is left alone between the grabs of the control with no window.
const CONTROL_WAIT_MS: u64 = 3000;
/// The time the system is given to take a closed window off the screen before it is grabbed again.
const CLOSE_SETTLE_MS: u64 = 500;

/// What one probe run measured.
struct Probe {
    measured: Measured,
    ring: Option<RingReport>,
}

/// The primary monitor's work area and scale, from which every expected band is worked out.
#[derive(Clone, Copy)]
struct Screen {
    work: Rect,
    scale: f64,
}

/// Where a probe's window is expected, and the work area it stands in.
#[derive(Clone, Copy)]
struct Placed {
    band: Rect,
    work: Rect,
}

impl Screen {
    /// The band of a probe started with `--zoom zoom` and `--probe-shrink shrink`.
    fn placed(&self, zoom: u32, shrink: u32) -> Placed {
        let scale = self.scale * f64::from(zoom) / 100.0;
        let band = expected_band(self.work, scale, &DEFAULT_BAND);
        Placed {
            band: shrunk_band(band, shrink, scale),
            work: self.work,
        }
    }
}

fn grab(region: Rect, retried: &mut usize) -> Result<Pixels, String> {
    let mut last = String::new();
    for _ in 0..GRAB_TRIES {
        match win::capture(region) {
            Ok(pixels) => return Ok(pixels),
            Err(error) => {
                *retried += 1;
                last = error;
                sleep_ms(100);
            }
        }
    }
    Err(format!(
        "the screen could not be grabbed {GRAB_TRIES} times: {last}"
    ))
}

/// `STEADY_GRABS` grabs of the region, `STEADY_GAP_MS` apart: a pixel that is
/// not the same in all of them moves by itself.
fn grab_steady(region: Rect, retried: &mut usize) -> Result<Vec<Pixels>, String> {
    let mut grabs = Vec::with_capacity(STEADY_GRABS);
    for index in 0..STEADY_GRABS {
        if index > 0 {
            sleep_ms(STEADY_GAP_MS);
        }
        grabs.push(grab(region, retried)?);
    }
    Ok(grabs)
}

fn probe_args(zoom: Option<u32>, shrink: Option<u32>) -> Vec<String> {
    let mut args = vec![String::from("--layout-probe")];
    if let Some(percent) = zoom {
        args.push(String::from("--zoom"));
        args.push(percent.to_string());
    }
    if let Some(pixels) = shrink {
        args.push(String::from("--probe-shrink"));
        args.push(pixels.to_string());
    }
    args
}

/// The screen around the band: the band grown by `RING` px on every side.
fn ring_region(band: Rect) -> Result<Rect, String> {
    let ring = i32::try_from(RING).map_err(|error| error.to_string())?;
    Ok(band.inflate(ring))
}

/// The grabs of one run: `STEADY_GRABS` before the window, as many with it up, one after it is closed.
struct Frames {
    before: Vec<Pixels>,
    up: Vec<Pixels>,
    closed: Pixels,
}

fn ring_report(frames: &Frames, placed: Placed) -> Result<RingReport, String> {
    let geometry = RingGeometry::around(placed.band, placed.work, RING)?;
    let before: Vec<&Pixels> = frames.before.iter().collect();
    let up: Vec<&Pixels> = frames.up.iter().collect();
    let grabs = Grabs {
        before: &before,
        up: &up,
        closed: &frames.closed,
    };
    compare_ring(&grabs, geometry)
}

/// Starts the overlay as a probe and reads what it measured. With `watch_ring`
/// the screen around the expected band is grabbed before the window and after it.
fn run_probe(
    dir: &std::path::Path,
    name: &str,
    args: &[String],
    placed: Placed,
    watch_ring: bool,
    retried: &mut usize,
) -> Result<Probe, String> {
    let region = ring_region(placed.band)?;
    let before = if watch_ring {
        Some(grab_steady(region, retried)?)
    } else {
        None
    };
    let mut overlay = Overlay::launch(args, Log::at(dir.join(name)))?;
    overlay.wait_opened()?;
    let event = overlay.wait(
        "overlay.layout.measured",
        Duration::from_secs(30),
        |event| is_event(event, "overlay.layout.measured"),
    )?;
    sleep_ms(SETTLE_MS);
    let rect = win::window_rect(overlay.window()?)?;
    let up = if watch_ring {
        Some(grab_steady(region, retried)?)
    } else {
        None
    };
    overlay.close()?;
    if !within(rect, placed.band, 1) {
        return Err(format!(
            "the band is {rect:?}, not {:?}; the screen regions do not line up",
            placed.band
        ));
    }
    let ring = match (before, up) {
        (Some(before), Some(up)) => {
            sleep_ms(CLOSE_SETTLE_MS);
            let closed = grab(region, retried)?;
            let frames = Frames { before, up, closed };
            Some(ring_report(&frames, placed)?)
        }
        _ => None,
    };
    Ok(Probe {
        measured: parse_measured(&event)?,
        ring,
    })
}

/// The ring grabbed seven times with the overlay not started, at the pace a
/// real run grabs it: what the screen changes by itself.
fn ring_with_no_window(placed: Placed, retried: &mut usize) -> Result<RingReport, String> {
    let region = ring_region(placed.band)?;
    let before = grab_steady(region, retried)?;
    sleep_ms(CONTROL_WAIT_MS);
    let up = grab_steady(region, retried)?;
    sleep_ms(CLOSE_SETTLE_MS);
    let closed = grab(region, retried)?;
    ring_report(&Frames { before, up, closed }, placed)
}

/// What the page measured: no text is cut and the window has the height of design 11.4.
/// This does not depend on the screen around the band, so it is never tried again.
fn check_the_page(zoom: u32, probe: &Probe) -> Result<(), String> {
    let measured = &probe.measured;
    let cut = clipping(measured);
    if !cut.is_empty() {
        return Err(format!("at {zoom}%: text is cut: {cut:?}"));
    }
    let wanted = band_height_css(&DEFAULT_BAND);
    if (measured.inner_height - wanted).abs() > 1.0 {
        return Err(format!(
            "at {zoom}%: innerHeight is {}, design 11.4 says {wanted}",
            measured.inner_height
        ));
    }
    Ok(())
}

/// What the screen showed around the band.
fn check_the_ring(zoom: u32, probe: &Probe) -> Result<(), String> {
    let measured = &probe.measured;
    let Some(ring) = probe.ring else {
        return Err(format!("at {zoom}%: the ring was not compared"));
    };
    eprintln!(
        "CS-11 {zoom}%: {ring:?}, missing_glyphs {}",
        measured.missing_glyphs
    );
    if ring.compared == 0 {
        return Err(format!(
            "at {zoom}%: no steady pixel was left to compare on the ring"
        ));
    }
    if ring.interior_changed == 0 {
        return Err(format!(
            "at {zoom}%: nothing was drawn inside the band, so the ring comparison shows nothing"
        ));
    }
    if ring.mismatched != 0 {
        return Err(format!(
            "at {zoom}%: {} ring pixels differ after the window is up (a frame, a border or a shadow)",
            ring.mismatched
        ));
    }
    Ok(())
}

/// The shrunk probe must show the window `SHRINK` CSS px lower than the plain
/// probe of the same zoom, and the lower lane below it; otherwise the measure
/// cannot see a cut.
fn check_the_cut_is_seen(zoom: u32, probe: &Probe, plain: &Probe) -> Result<(), String> {
    let measured = &probe.measured;
    let lowered = plain.measured.inner_height - measured.inner_height;
    eprintln!(
        "CS-11 {zoom}%: inner_height {} with --probe-shrink 0 and {} with --probe-shrink {SHRINK}",
        plain.measured.inner_height, measured.inner_height
    );
    if (lowered - f64::from(SHRINK)).abs() > 1.0 {
        return Err(format!(
            "the measure does not work: at {zoom}% --probe-shrink {SHRINK} made the window {lowered} CSS px shorter ({} against {})",
            measured.inner_height, plain.measured.inner_height
        ));
    }
    let Some(lower) = measured.lanes.iter().find(|lane| lane.lane == "lower") else {
        return Err(format!(
            "at {zoom}% with --probe-shrink {SHRINK}: the probe reported no lower lane"
        ));
    };
    if lower.bottom > measured.inner_height && !clipping(measured).is_empty() {
        return Ok(());
    }
    Err(format!(
        "the measure does not work: at {zoom}% with --probe-shrink {SHRINK} the lower lane ends at {} inside a window {} high",
        lower.bottom, measured.inner_height
    ))
}

/// Runs one scale until the ring comparison is clean, at most `RING_ATTEMPTS`
/// times. A live screen can change and change back by itself at a pixel or two
/// of a ring that is watched for a second, and a window that draws a frame, a
/// border or a shadow draws it in every attempt, so a ring that is clean once
/// shows there is none. A text that is cut is never tried again.
fn run_scale(
    dir: &std::path::Path,
    screen: Screen,
    zoom: u32,
    retried: &mut usize,
) -> Result<Probe, String> {
    let mut failures = Vec::new();
    for attempt in 1..=RING_ATTEMPTS {
        let name = format!("zoom-{zoom}-{attempt}.jsonl");
        let args = probe_args(Some(zoom), None);
        let placed = screen.placed(zoom, 0);
        let probe = run_probe(dir, &name, &args, placed, true, retried)?;
        check_the_page(zoom, &probe)?;
        match check_the_ring(zoom, &probe) {
            Ok(()) => return Ok(probe),
            Err(error) => {
                eprintln!("CS-11 {zoom}%: attempt {attempt} of {RING_ATTEMPTS}: {error}");
                failures.push(error);
            }
        }
    }
    Err(format!(
        "the ring was not clean in any of {RING_ATTEMPTS} attempts: {failures:?}"
    ))
}

/// With no window started, nothing may be taken for the window's doing: the
/// screen may change by itself, but not change and change back. Tried again
/// for the reason `run_scale` gives.
fn check_nothing_is_blamed_on_a_window_that_is_not_there(
    placed: Placed,
    retried: &mut usize,
) -> Result<(), String> {
    let mut failures = Vec::new();
    for attempt in 1..=RING_ATTEMPTS {
        match blame_with_no_window(placed, retried) {
            Ok(()) => return Ok(()),
            Err(error) => {
                eprintln!("CS-11 control: attempt {attempt} of {RING_ATTEMPTS}: {error}");
                failures.push(error);
            }
        }
    }
    Err(format!(
        "the measure does not work: no attempt of {RING_ATTEMPTS} was free of blame: {failures:?}"
    ))
}

fn blame_with_no_window(placed: Placed, retried: &mut usize) -> Result<(), String> {
    let none = ring_with_no_window(placed, retried)?;
    eprintln!("CS-11 control, no window: {none:?}");
    if none.compared == 0 {
        return Err(String::from(
            "the measure does not work: no steady pixel was left on the ring with no window",
        ));
    }
    if none.mismatched != 0 || none.interior_changed != 0 || none.taskbar_changed != 0 {
        return Err(format!(
            "the measure does not work: the screen changed and changed back with no window started: {none:?}"
        ));
    }
    Ok(())
}

/// Prints the machine's own scale next to the `--zoom` probe of the same percentage.
fn record_the_real_scale(real: &Probe, zoomed: &[(u32, Probe)], percent: u32) {
    let Some((_, same)) = zoomed.iter().find(|(zoom, _)| *zoom == percent) else {
        eprintln!(
            "CS-11: the machine is at {percent}%, which is not one of {ZOOMS:?}; no --zoom run to compare with"
        );
        return;
    };
    let agree = (real.measured.inner_height - same.measured.inner_height).abs() < 0.5
        && real
            .measured
            .lanes
            .iter()
            .zip(&same.measured.lanes)
            .all(|(a, b)| (a.bottom - b.bottom).abs() < 0.5);
    eprintln!(
        "CS-11 (design H29): the real {percent}% and --zoom {percent} agree on innerHeight and the lanes: {agree}"
    );
}

#[test]
#[ignore = "opens windows; run it when nobody is using the screen"]
fn cs11_no_clipping_and_no_frame_at_any_scale() -> TestResult {
    win::per_monitor_aware()?;
    let (_, work) = win::primary_monitor()?;
    let screen = Screen {
        work,
        scale: win::system_scale()?,
    };
    let dir = scratch()?;
    let mut retried = 0;

    check_nothing_is_blamed_on_a_window_that_is_not_there(screen.placed(100, 0), &mut retried)?;
    let mut zoomed = Vec::new();
    for zoom in ZOOMS {
        let probe = run_scale(dir.path(), screen, zoom, &mut retried)?;
        zoomed.push((zoom, probe));
    }
    for (zoom, plain) in &zoomed {
        let name = format!("shrink-{zoom}.jsonl");
        let args = probe_args(Some(*zoom), Some(SHRINK));
        let placed = screen.placed(*zoom, SHRINK);
        let probe = run_probe(dir.path(), &name, &args, placed, false, &mut retried)?;
        check_the_cut_is_seen(*zoom, &probe, plain)?;
    }
    let real = run_probe(
        dir.path(),
        "real.jsonl",
        &probe_args(None, None),
        screen.placed(100, 0),
        false,
        &mut retried,
    )?;
    let percent = (screen.scale * 100.0).round() as u32;
    record_the_real_scale(&real, &zoomed, percent);
    eprintln!("CS-11: {retried} screen grabs were tried again (design H12)");
    Ok(())
}

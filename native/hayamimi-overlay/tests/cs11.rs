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
//!   same on the screen before the window is shown and after. The screen is
//!   grabbed twice before, so that a pixel that moves by itself is left out;
//! - `innerHeight` is the height of design 11.4 (1 CSS px tolerance), and the
//!   probe drew something inside the band (positive controls).
//!
//! Negative control: the same probe with `--probe-shrink 12` shrinks the window
//! by 12 CSS px, more than the 10.8 px margin under the band, so the lower
//! lane must end below the window. If it does not, the measure cannot see a cut
//! and the test fails as "the measure does not work".
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
    DEFAULT_BAND, Log, Measured, Overlay, RingReport, TestResult, band_height_css, clipping,
    compare_ring, expected_band, is_event, parse_measured, scratch, sleep_ms, within,
};

const ZOOMS: [u32; 4] = [100, 125, 150, 200];
/// The ring around the band's edge, in physical px (design 11.8).
const RING: usize = 8;
/// The shrink of the negative control, in CSS px.
const SHRINK: u32 = 12;
/// How often a failed screen grab is tried again.
const GRAB_TRIES: usize = 5;
/// The time the painting is given before the screen is grabbed after the window is up.
const SETTLE_MS: u64 = 700;

/// What one probe run measured.
struct Probe {
    measured: Measured,
    ring: Option<RingReport>,
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

fn ring_report(
    before: &(Pixels, Pixels),
    after: &Pixels,
    band: Rect,
) -> Result<RingReport, String> {
    let width = usize::try_from(band.width()).map_err(|error| error.to_string())?;
    let height = usize::try_from(band.height()).map_err(|error| error.to_string())?;
    compare_ring(&before.0, &before.1, after, width, height, RING)
}

/// Starts the overlay as a probe and reads what it measured. With `watch_ring`
/// the screen around `expected` is grabbed before the window and after it.
fn run_probe(
    dir: &std::path::Path,
    name: &str,
    args: &[String],
    expected: Rect,
    watch_ring: bool,
    retried: &mut usize,
) -> Result<Probe, String> {
    let region = expected.inflate(i32::try_from(RING).map_err(|error| error.to_string())?);
    let before = if watch_ring {
        let first = grab(region, retried)?;
        sleep_ms(100);
        Some((first, grab(region, retried)?))
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
    let after = if watch_ring {
        Some(grab(region, retried)?)
    } else {
        None
    };
    overlay.close()?;
    if !within(rect, expected, 1) {
        return Err(format!(
            "the band is {rect:?}, not {expected:?}; the screen regions do not line up"
        ));
    }
    let ring = match (&before, &after) {
        (Some(before), Some(after)) => Some(ring_report(before, after, expected)?),
        _ => None,
    };
    Ok(Probe {
        measured: parse_measured(&event)?,
        ring,
    })
}

fn check_at_scale(zoom: u32, probe: &Probe) -> Result<(), String> {
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

/// The shrunk probe must show the lower lane below the window; otherwise the measure cannot see a cut.
fn check_the_cut_is_seen(zoom: u32, probe: &Probe) -> Result<(), String> {
    let measured = &probe.measured;
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
    let (_, area) = win::primary_monitor()?;
    let scale = win::system_scale()?;
    let expected = expected_band(area, scale, &DEFAULT_BAND);
    let dir = scratch()?;
    let mut retried = 0;

    let mut zoomed = Vec::new();
    for zoom in ZOOMS {
        let name = format!("zoom-{zoom}.jsonl");
        let probe = run_probe(
            dir.path(),
            &name,
            &probe_args(Some(zoom), None),
            expected,
            true,
            &mut retried,
        )?;
        check_at_scale(zoom, &probe)?;
        zoomed.push((zoom, probe));
    }
    for zoom in ZOOMS {
        let name = format!("shrink-{zoom}.jsonl");
        let args = probe_args(Some(zoom), Some(SHRINK));
        let probe = run_probe(dir.path(), &name, &args, expected, false, &mut retried)?;
        check_the_cut_is_seen(zoom, &probe)?;
    }
    let real = run_probe(
        dir.path(),
        "real.jsonl",
        &probe_args(None, None),
        expected,
        false,
        &mut retried,
    )?;
    let percent = (scale * 100.0).round() as u32;
    record_the_real_scale(&real, &zoomed, percent);
    eprintln!("CS-11: {retried} screen grabs were tried again (design H12)");
    Ok(())
}

//! The window itself: its attributes, which monitor it is on and where on it
//! the band sits.
//!
//! The attributes tao has a door for are set here (no frame, transparent,
//! always on top, no shadow, not focused, hidden until placed); the three it
//! has none for are `win`'s. The band is worked out in `place` from the work
//! area of the monitor, read after the event loop has made the process DPI
//! aware, and handed to tao in physical px.

use std::collections::HashSet;

use dioxus::desktop::Config;
use dioxus::desktop::tao::dpi::{PhysicalPosition, PhysicalSize};
use dioxus::desktop::tao::monitor::MonitorHandle;
use dioxus::desktop::tao::platform::windows::{MonitorHandleExtWindows, WindowBuilderExtWindows};
use dioxus::desktop::tao::window::{Window, WindowBuilder};
use hayamimi_relay::logging::{Level, LogEvent, LogSink};

use crate::WINDOW_CLASS;
use crate::config::OverlayConfig;
use crate::place::{self, PhysicalRect, PlaceError};
use crate::win;

/// The attributes of the window. It is made hidden, unfocused and not
/// focusable (`WS_EX_NOACTIVATE`, so that showing it never activates it);
/// `open` shows it once it stands where it belongs.
fn builder() -> WindowBuilder {
    WindowBuilder::new()
        .with_title("hayamimi")
        .with_decorations(false)
        .with_resizable(false)
        .with_transparent(true)
        .with_always_on_top(true)
        .with_focused(false)
        .with_focusable(false)
        .with_visible(false)
        .with_undecorated_shadow(false)
        .with_window_classname(WINDOW_CLASS)
}

/// The desktop configuration: the window above, a transparent webview, and
/// `head` (the `@font-face` rules) in the page.
pub fn launch_config(head: String) -> Config {
    Config::new()
        .with_window(builder())
        .with_background_color((0, 0, 0, 0))
        .with_custom_head(head)
        .with_disable_context_menu(true)
        .with_exits_when_last_window_closes(true)
}

/// Where the window is to stand.
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub rect: PhysicalRect,
    /// The scale the band was laid out at: the monitor's, times the probe's zoom.
    pub scale: f64,
    pub monitor: Option<String>,
    /// The band was laid on the whole monitor because its work area could not be read.
    pub full_screen: bool,
}

/// Why no place could be worked out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    NoMonitor,
    NoWorkArea,
    Place(PlaceError),
}

impl Failure {
    pub fn reason(self) -> &'static str {
        match self {
            Failure::NoMonitor => "no_monitor",
            Failure::NoWorkArea => "no_work_area",
            Failure::Place(PlaceError::ZeroScale) => "zero_scale",
            Failure::Place(PlaceError::EmptyWorkArea) => "empty_work_area",
            Failure::Place(PlaceError::RatioOutOfRange) => "ratio_out_of_range",
            Failure::Place(PlaceError::RowsOutOfRange) => "rows_out_of_range",
        }
    }
}

/// The monitor the settings name; the primary one when the name is empty or
/// no monitor has it, which is said once for each name.
fn monitor_for(
    window: &Window,
    name: &str,
    said: &mut HashSet<String>,
    sink: &mut dyn LogSink,
) -> Option<MonitorHandle> {
    if !name.is_empty() {
        let named = window
            .available_monitors()
            .find(|monitor| monitor.name().as_deref() == Some(name));
        if named.is_some() {
            return named;
        }
        if said.insert(name.to_owned()) {
            sink.emit(monitor_missing(name));
        }
    }
    window
        .primary_monitor()
        .or_else(|| window.current_monitor())
}

/// Works out where the band stands on the monitor the settings name, made
/// `shrink_px` CSS px shorter (only a probe asks for that).
pub fn target(
    window: &Window,
    config: &OverlayConfig,
    zoom_percent: u32,
    shrink_px: u32,
    said: &mut HashSet<String>,
    sink: &mut dyn LogSink,
) -> Result<Target, Failure> {
    let monitor = monitor_for(window, &config.monitor, said, sink).ok_or(Failure::NoMonitor)?;
    let (area, full_screen) =
        choose_area(win::work_area(monitor.hmonitor()), || screen_rect(&monitor))?;
    let scale = place::effective_scale(monitor.scale_factor(), zoom_percent);
    let band = place::band(area, scale, config).map_err(Failure::Place)?;
    Ok(Target {
        rect: place::to_physical(place::shrunk(band, shrink_px), scale),
        scale,
        monitor: monitor.name(),
        full_screen,
    })
}

/// The area the band is laid in: the work area when it was read, else the
/// whole monitor (`screen`, asked only then). The flag says the latter.
fn choose_area(
    work: Option<PhysicalRect>,
    screen: impl FnOnce() -> Option<PhysicalRect>,
) -> Result<(PhysicalRect, bool), Failure> {
    if let Some(area) = work {
        return Ok((area, false));
    }
    screen().map(|area| (area, true)).ok_or(Failure::NoWorkArea)
}

/// The whole of a monitor in physical px; `None` when it has no area.
fn screen_rect(monitor: &MonitorHandle) -> Option<PhysicalRect> {
    let (at, size) = (monitor.position(), monitor.size());
    let right = at.x.checked_add_unsigned(size.width)?;
    let bottom = at.y.checked_add_unsigned(size.height)?;
    place::rect_from_edges(at.x, at.y, right, bottom)
}

/// Whether the window already stands on `rect`.
pub fn stands_on(window: &Window, rect: PhysicalRect) -> bool {
    let size = window.inner_size();
    window
        .outer_position()
        .is_ok_and(|at| at.x == rect.x && at.y == rect.y)
        && size.width == rect.width
        && size.height == rect.height
}

/// Moves and sizes the window to `rect`, in physical px.
pub fn place_on(window: &Window, rect: PhysicalRect) {
    window.set_outer_position(PhysicalPosition::new(rect.x, rect.y));
    window.set_inner_size(PhysicalSize::new(rect.width, rect.height));
}

fn monitor_missing(name: &str) -> LogEvent {
    LogEvent::new(Level::Warn, "overlay.monitor.missing", "browser")
        .with("monitor", name)
        .with_error("MONITOR_MISSING")
}

/// The window was shown for the first time, on `target`.
pub fn opened(target: &Target) -> LogEvent {
    LogEvent::new(Level::Info, "overlay.window.opened", "browser")
        .with(
            "rect",
            serde_json::json!({
                "x": target.rect.x,
                "y": target.rect.y,
                "width": target.rect.width,
                "height": target.rect.height,
            }),
        )
        .with("scale", target.scale)
        .with("monitor", target.monitor.clone())
}

/// DWM did not take the border off, so the window keeps its border.
pub fn border_kept(build: Option<u32>) -> LogEvent {
    LogEvent::new(Level::Warn, "overlay.window.border_kept", "browser")
        .with("windows_build", build)
        .with_error("BORDER_KEPT")
}

/// The lift to the front of the topmost band failed; `code` is the system's.
pub fn topmost_failed(code: i32) -> LogEvent {
    LogEvent::new(Level::Warn, "overlay.topmost.failed", "browser")
        .with("os_error", code)
        .with_error("TOPMOST_FAILED")
}

/// No place could be worked out for the window. `fatal` is the first showing,
/// which ends the process; later the window keeps the rectangle it has.
pub fn place_failed(failure: Failure, fatal: bool) -> LogEvent {
    let level = if fatal { Level::Error } else { Level::Warn };
    LogEvent::new(level, "overlay.window.place_failed", "browser")
        .with("reason", failure.reason())
        .with_error("PLACE_FAILED")
}

/// The work area could not be read, so the band stands on the whole monitor.
pub fn work_area_unread() -> LogEvent {
    LogEvent::new(Level::Warn, "overlay.window.place_failed", "browser")
        .with("reason", Failure::NoWorkArea.reason())
        .with("placed_on", "full_screen")
        .with_error("PLACE_FAILED")
}

/// Whether the read-back of the extended style says the window is click-through.
pub fn click_through_read_ok(read: Result<bool, u32>) -> bool {
    read == Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> Target {
        Target {
            rect: PhysicalRect {
                x: 384,
                y: 1851,
                width: 3072,
                height: 237,
            },
            scale: 1.5,
            monitor: Some(r"\\.\DISPLAY2".to_owned()),
            full_screen: false,
        }
    }

    const WORK: PhysicalRect = PhysicalRect {
        x: 0,
        y: 0,
        width: 3840,
        height: 2088,
    };
    const SCREEN: PhysicalRect = PhysicalRect {
        x: 0,
        y: 0,
        width: 3840,
        height: 2160,
    };

    #[test]
    fn the_work_area_is_used_when_it_was_read_and_the_screen_is_not_asked() {
        let chosen = choose_area(Some(WORK), || None);
        assert_eq!(chosen, Ok((WORK, false)));
    }

    #[test]
    fn the_whole_screen_stands_in_when_the_work_area_was_not_read() {
        let chosen = choose_area(None, || Some(SCREEN));
        assert_eq!(chosen, Ok((SCREEN, true)));
    }

    #[test]
    fn no_area_at_all_is_the_no_work_area_failure() {
        assert_eq!(choose_area(None, || None), Err(Failure::NoWorkArea));
    }

    #[test]
    fn a_failure_is_an_error_at_the_first_showing_and_a_warning_after()
    -> Result<(), serde_json::Error> {
        let first = value_of(&place_failed(Failure::NoMonitor, true))?;
        let later = value_of(&place_failed(Failure::NoMonitor, false))?;
        assert_eq!(first["level"], "error");
        assert_eq!(later["level"], "warn");
        assert_eq!(first["reason"], "no_monitor");
        assert_eq!(later["error_code"], "HYM_BROWSER_PLACE_FAILED");
        Ok(())
    }

    #[test]
    fn the_unread_work_area_is_a_warning_that_says_where_the_band_went()
    -> Result<(), serde_json::Error> {
        let said = value_of(&work_area_unread())?;
        assert_eq!(said["level"], "warn");
        assert_eq!(said["reason"], "no_work_area");
        assert_eq!(said["placed_on"], "full_screen");
        Ok(())
    }

    #[test]
    fn only_a_style_read_with_both_bits_is_click_through() {
        assert!(click_through_read_ok(Ok(true)));
        assert!(!click_through_read_ok(Ok(false)));
        assert!(!click_through_read_ok(Err(5)));
    }

    fn value_of(event: &LogEvent) -> Result<serde_json::Value, serde_json::Error> {
        serde_json::from_str(&event.to_json_line())
    }

    #[test]
    fn the_opened_event_names_the_rectangle_the_scale_and_the_monitor()
    -> Result<(), serde_json::Error> {
        let value = value_of(&opened(&target()))?;
        assert_eq!(value["event"], "overlay.window.opened");
        assert_eq!(value["rect"]["width"], 3072);
        assert_eq!(value["rect"]["height"], 237);
        assert_eq!(value["rect"]["x"], 384);
        assert_eq!(value["scale"], 1.5);
        assert_eq!(value["monitor"], r"\\.\DISPLAY2");
        Ok(())
    }

    #[test]
    fn a_monitor_with_no_name_is_a_null_in_the_event() -> Result<(), serde_json::Error> {
        let nameless = Target {
            monitor: None,
            ..target()
        };
        assert!(!nameless.full_screen);
        assert!(value_of(&opened(&nameless))?["monitor"].is_null());
        Ok(())
    }

    #[test]
    fn the_warnings_carry_their_error_codes() -> Result<(), serde_json::Error> {
        let missing = value_of(&monitor_missing("X"))?;
        let border = value_of(&border_kept(Some(19045)))?;
        let topmost = value_of(&topmost_failed(5))?;
        assert_eq!(missing["error_code"], "HYM_BROWSER_MONITOR_MISSING");
        assert_eq!(border["error_code"], "HYM_BROWSER_BORDER_KEPT");
        assert_eq!(border["windows_build"], 19045);
        assert_eq!(topmost["error_code"], "HYM_BROWSER_TOPMOST_FAILED");
        assert_eq!(topmost["os_error"], 5);
        Ok(())
    }

    #[test]
    fn every_failure_has_its_own_reason() {
        let all = [
            Failure::NoMonitor,
            Failure::NoWorkArea,
            Failure::Place(PlaceError::ZeroScale),
            Failure::Place(PlaceError::EmptyWorkArea),
            Failure::Place(PlaceError::RatioOutOfRange),
            Failure::Place(PlaceError::RowsOutOfRange),
        ];
        let reasons: HashSet<&str> = all.iter().map(|failure| failure.reason()).collect();
        assert_eq!(reasons.len(), all.len());
    }
}

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

/// The attributes of the window. It is made hidden and unfocused; `open`
/// shows it once it stands where it belongs.
fn builder() -> WindowBuilder {
    WindowBuilder::new()
        .with_title("hayamimi")
        .with_decorations(false)
        .with_resizable(false)
        .with_transparent(true)
        .with_always_on_top(true)
        .with_focused(false)
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

/// Works out where the band stands on the monitor the settings name.
pub fn target(
    window: &Window,
    config: &OverlayConfig,
    zoom_percent: u32,
    said: &mut HashSet<String>,
    sink: &mut dyn LogSink,
) -> Result<Target, Failure> {
    let monitor = monitor_for(window, &config.monitor, said, sink).ok_or(Failure::NoMonitor)?;
    let work_area = win::work_area(monitor.hmonitor()).ok_or(Failure::NoWorkArea)?;
    let scale = place::effective_scale(monitor.scale_factor(), zoom_percent);
    let band = place::band(work_area, scale, config).map_err(Failure::Place)?;
    Ok(Target {
        rect: place::to_physical(band, scale),
        scale,
        monitor: monitor.name(),
    })
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

/// No place could be worked out for the window.
pub fn place_failed(failure: Failure) -> LogEvent {
    LogEvent::new(Level::Error, "overlay.window.place_failed", "browser")
        .with("reason", failure.reason())
        .with_error("PLACE_FAILED")
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
        }
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

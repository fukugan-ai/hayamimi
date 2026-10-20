//! Where the band sits: the one formula that gives the window its size and
//! position.
//!
//! The band is sized in CSS px, the unit the page lays out in, and the
//! operating system multiplies by the scale of the monitor. A band sized in
//! physical px at a fixed value lets the text run out of its window at 150%.

use crate::config::OverlayConfig;

/// The most rows a lane may be given.
pub const MAX_ROWS: usize = 4;
/// The height of a row of the upper lane: 1.5 times its 36 px text.
pub const UPPER_ROW_PX: f64 = 54.0;
/// The height of a row of the lower lane: 1.5 times its 26 px text.
pub const LOWER_ROW_PX: f64 = 39.0;
/// The space under the band: 0.3 times the 36 px text.
pub const MARGIN_PX: f64 = 10.8;

/// A rectangle in physical px, as the operating system reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhysicalRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// A rectangle in CSS px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LogicalRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaceError {
    ZeroScale,
    EmptyWorkArea,
    RatioOutOfRange,
    RowsOutOfRange,
}

/// The height of a band that shows the given rows, in CSS px.
pub fn band_height(upper_rows: usize, lower_rows: usize) -> f64 {
    upper_rows as f64 * UPPER_ROW_PX + lower_rows as f64 * LOWER_ROW_PX + MARGIN_PX
}

fn rows_in_range(rows: usize) -> bool {
    (1..=MAX_ROWS).contains(&rows)
}

/// The band on `work_area` (physical px of a per-monitor-aware read) at `scale`.
///
/// It is as wide as `width_ratio` of the work area, centred across it, and its
/// bottom edge stands `bottom_gap_px` above the bottom of the work area.
pub fn band(
    work_area: PhysicalRect,
    scale: f64,
    config: &OverlayConfig,
) -> Result<LogicalRect, PlaceError> {
    if !(scale.is_finite() && scale > 0.0) {
        return Err(PlaceError::ZeroScale);
    }
    if work_area.width == 0 || work_area.height == 0 {
        return Err(PlaceError::EmptyWorkArea);
    }
    if !(config.width_ratio > 0.0 && config.width_ratio <= 1.0) {
        return Err(PlaceError::RatioOutOfRange);
    }
    if !(rows_in_range(config.upper_rows) && rows_in_range(config.lower_rows)) {
        return Err(PlaceError::RowsOutOfRange);
    }
    let work_x = f64::from(work_area.x) / scale;
    let work_width = f64::from(work_area.width) / scale;
    let work_bottom = (f64::from(work_area.y) + f64::from(work_area.height)) / scale;
    let width = config.width_ratio * work_width;
    let height = band_height(config.upper_rows, config.lower_rows);
    Ok(LogicalRect {
        x: work_x + (work_width - width) / 2.0,
        y: work_bottom - config.bottom_gap_px - height,
        width,
        height,
    })
}

/// The rectangle in physical px that a logical one occupies at `scale`, with
/// each edge rounded to the nearest px.
pub fn to_physical(rect: LogicalRect, scale: f64) -> PhysicalRect {
    let edge = |logical: f64| (logical * scale).round();
    let left = edge(rect.x);
    let top = edge(rect.y);
    PhysicalRect {
        x: left as i32,
        y: top as i32,
        width: (edge(rect.x + rect.width) - left).max(0.0) as u32,
        height: (edge(rect.y + rect.height) - top).max(0.0) as u32,
    }
}

/// The scale the band is laid out at: the monitor's own scale times the
/// webview's zoom (`--zoom`), which stands in for a monitor at that scale.
/// The window is made as large in physical px as the page then takes its
/// CSS px to be, so the page measures the same band it would on that monitor.
pub fn effective_scale(monitor_scale: f64, zoom_percent: u32) -> f64 {
    monitor_scale * f64::from(zoom_percent) / 100.0
}

/// A rectangle given by its four edges, as `RECT` of the operating system
/// gives it; `None` when it has no area.
pub fn rect_from_edges(left: i32, top: i32, right: i32, bottom: i32) -> Option<PhysicalRect> {
    let width = u32::try_from(i64::from(right) - i64::from(left)).ok()?;
    let height = u32::try_from(i64::from(bottom) - i64::from(top)).ok()?;
    (width > 0 && height > 0).then_some(PhysicalRect {
        x: left,
        y: top,
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;

    const WORK_3840: PhysicalRect = PhysicalRect {
        x: 0,
        y: 0,
        width: 3840,
        height: 2088,
    };

    fn defaults() -> Result<OverlayConfig, String> {
        config::builtin()
    }

    #[test]
    fn the_default_band_on_this_machines_work_area_is_2048_by_157_8_css_px()
    -> Result<(), Box<dyn std::error::Error>> {
        let rect = band(WORK_3840, 1.5, &defaults()?).map_err(|e| format!("{e:?}"))?;
        assert!((rect.width - 2048.0).abs() < 1e-9, "{rect:?}");
        assert!((rect.height - 157.8).abs() < 1e-9, "{rect:?}");
        assert!((rect.x - 256.0).abs() < 1e-9, "{rect:?}");
        assert!((rect.y - (1392.0 - 157.8)).abs() < 1e-9, "{rect:?}");
        Ok(())
    }

    #[test]
    fn the_default_band_is_3072_by_237_physical_px_at_150_percent()
    -> Result<(), Box<dyn std::error::Error>> {
        let rect = band(WORK_3840, 1.5, &defaults()?).map_err(|e| format!("{e:?}"))?;
        let physical = to_physical(rect, 1.5);
        let bottom_edge = physical.y + i32::try_from(physical.height)?;
        assert_eq!(physical.width, 3072);
        assert_eq!(physical.height, 237);
        assert_eq!(physical.x, 384);
        assert_eq!(bottom_edge, 2088);
        Ok(())
    }

    #[test]
    fn a_work_area_that_starts_off_the_origin_moves_the_band_with_it()
    -> Result<(), Box<dyn std::error::Error>> {
        let area = PhysicalRect {
            x: -1920,
            y: 40,
            width: 1920,
            height: 1000,
        };
        let rect = band(area, 1.0, &defaults()?).map_err(|e| format!("{e:?}"))?;
        assert!((rect.x - (-1920.0 + 192.0)).abs() < 1e-9, "{rect:?}");
        assert!((rect.y + rect.height - 1040.0).abs() < 1e-9, "{rect:?}");
        Ok(())
    }

    #[test]
    fn the_bottom_gap_lifts_the_band() -> Result<(), Box<dyn std::error::Error>> {
        let mut config = defaults()?;
        let flat = band(WORK_3840, 1.5, &config).map_err(|e| format!("{e:?}"))?;
        config.bottom_gap_px = 20.0;
        let lifted = band(WORK_3840, 1.5, &config).map_err(|e| format!("{e:?}"))?;
        assert!((flat.y - lifted.y - 20.0).abs() < 1e-9);
        Ok(())
    }

    #[test]
    fn more_rows_make_a_taller_band() {
        assert!((band_height(2, 1) - 157.8).abs() < 1e-9);
        assert!((band_height(4, 4) - (216.0 + 156.0 + 10.8)).abs() < 1e-9);
        assert!((band_height(1, 1) - (54.0 + 39.0 + 10.8)).abs() < 1e-9);
    }

    #[test]
    fn a_scale_that_is_zero_negative_or_not_a_number_is_refused()
    -> Result<(), Box<dyn std::error::Error>> {
        for scale in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(
                band(WORK_3840, scale, &defaults()?),
                Err(PlaceError::ZeroScale),
                "{scale}"
            );
        }
        Ok(())
    }

    #[test]
    fn a_work_area_with_no_width_or_no_height_is_refused() -> Result<(), Box<dyn std::error::Error>>
    {
        for area in [
            PhysicalRect {
                width: 0,
                ..WORK_3840
            },
            PhysicalRect {
                height: 0,
                ..WORK_3840
            },
        ] {
            assert_eq!(
                band(area, 1.0, &defaults()?),
                Err(PlaceError::EmptyWorkArea)
            );
        }
        Ok(())
    }

    #[test]
    fn a_ratio_outside_zero_to_one_is_refused() -> Result<(), Box<dyn std::error::Error>> {
        for ratio in [0.0, -0.1, 1.0001, f64::NAN] {
            let config = OverlayConfig {
                width_ratio: ratio,
                ..defaults()?
            };
            assert_eq!(
                band(WORK_3840, 1.0, &config),
                Err(PlaceError::RatioOutOfRange),
                "{ratio}"
            );
        }
        let whole = OverlayConfig {
            width_ratio: 1.0,
            ..defaults()?
        };
        assert!(band(WORK_3840, 1.0, &whole).is_ok());
        Ok(())
    }

    #[test]
    fn rows_of_zero_and_five_are_refused_and_one_and_four_pass()
    -> Result<(), Box<dyn std::error::Error>> {
        for (upper, lower) in [(0, 1), (1, 0), (5, 1), (1, 5)] {
            let config = OverlayConfig {
                upper_rows: upper,
                lower_rows: lower,
                ..defaults()?
            };
            assert_eq!(
                band(WORK_3840, 1.0, &config),
                Err(PlaceError::RowsOutOfRange),
                "{upper} {lower}"
            );
        }
        for (upper, lower) in [(1, 1), (4, 4)] {
            let config = OverlayConfig {
                upper_rows: upper,
                lower_rows: lower,
                ..defaults()?
            };
            assert!(band(WORK_3840, 1.0, &config).is_ok(), "{upper} {lower}");
        }
        Ok(())
    }

    #[test]
    fn the_band_is_the_same_size_in_css_px_at_every_scale() -> Result<(), Box<dyn std::error::Error>>
    {
        // The same screen: its physical size grows with the scale.
        let at_100 = PhysicalRect {
            width: 2560,
            height: 1392,
            ..WORK_3840
        };
        let a = band(at_100, 1.0, &defaults()?).map_err(|e| format!("{e:?}"))?;
        let b = band(WORK_3840, 1.5, &defaults()?).map_err(|e| format!("{e:?}"))?;
        assert!((a.width - b.width).abs() < 1e-9);
        assert!((a.height - b.height).abs() < 1e-9);
        Ok(())
    }

    #[test]
    fn rounding_to_physical_rounds_each_edge_not_the_width() {
        let rect = LogicalRect {
            x: 0.4,
            y: 0.0,
            width: 10.2,
            height: 1.0,
        };
        let physical = to_physical(rect, 1.0);
        assert_eq!((physical.x, physical.width), (0, 11));
    }

    #[test]
    fn a_rectangle_of_edges_has_its_width_and_height() {
        assert_eq!(
            rect_from_edges(10, 20, 110, 70),
            Some(PhysicalRect {
                x: 10,
                y: 20,
                width: 100,
                height: 50
            })
        );
    }

    #[test]
    fn a_rectangle_with_no_area_or_reversed_edges_is_none() {
        assert_eq!(rect_from_edges(0, 0, 0, 10), None);
        assert_eq!(rect_from_edges(0, 0, 10, 0), None);
        assert_eq!(rect_from_edges(10, 0, 0, 10), None);
    }

    #[test]
    fn the_zoom_multiplies_the_scale_of_the_monitor() {
        assert!((effective_scale(1.5, 100) - 1.5).abs() < 1e-12);
        assert!((effective_scale(1.5, 200) - 3.0).abs() < 1e-12);
        assert!((effective_scale(1.0, 125) - 1.25).abs() < 1e-12);
    }

    /// A zoomed probe keeps the band over the same stretch of the screen and
    /// makes it as tall as its CSS px take at the zoomed scale.
    #[test]
    fn a_zoom_keeps_the_physical_width_and_bottom_and_grows_the_height()
    -> Result<(), Box<dyn std::error::Error>> {
        let config = defaults()?;
        let plain = band(WORK_3840, 1.5, &config).map_err(|e| format!("{e:?}"))?;
        let zoomed_scale = effective_scale(1.5, 200);
        let zoomed = band(WORK_3840, zoomed_scale, &config).map_err(|e| format!("{e:?}"))?;
        let before = to_physical(plain, 1.5);
        let after = to_physical(zoomed, zoomed_scale);
        let bottom_before = before.y + i32::try_from(before.height)?;
        let bottom_after = after.y + i32::try_from(after.height)?;
        assert_eq!((after.x, after.width), (before.x, before.width));
        assert_eq!(bottom_after, bottom_before);
        assert_eq!(after.height, 473);
        Ok(())
    }

    #[test]
    fn a_rectangle_as_wide_as_the_integer_range_keeps_its_width() {
        let wide = rect_from_edges(i32::MIN, 0, i32::MAX, 1);
        assert_eq!(wide.map(|rect| rect.width), Some(u32::MAX));
    }
}

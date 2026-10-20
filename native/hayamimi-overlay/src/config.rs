//! What the overlay is told: the command line and `overlay.toml`.
//!
//! The file holds where the band sits and which fonts it uses. It is read
//! again while the window runs (`Watched`), so a change reaches the screen
//! without a restart, and a file that cannot be read leaves the settings in
//! use as they were. Every key may be left out; the defaults are in
//! [`builtin`] and nowhere else.

use std::path::PathBuf;

use hayamimi_relay::cli::{DEFAULT_EVENTS, DEFAULT_SERVE};
use hayamimi_relay::logging::{Level, LogEvent, LogSink};
use hayamimi_relay::watched::Watched;
use serde::Deserialize;

use crate::place::MAX_ROWS;

/// The table name `Watched` puts in its log events.
pub const TABLE: &str = "overlay";

/// The shape of the file the window is placed by.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OverlayConfig {
    /// The monitor's name; empty means the primary monitor.
    pub monitor: String,
    /// The width of the band as a share of the work area: `0 < width_ratio <= 1`.
    pub width_ratio: f64,
    /// CSS px from the bottom of the work area to the bottom of the band: `>= 0`.
    pub bottom_gap_px: f64,
    /// Rows of the upper lane: `1..=4`.
    pub upper_rows: usize,
    /// Rows of the lower lane: `1..=4`.
    pub lower_rows: usize,
    pub cinecaption: Option<PathBuf>,
    pub udev_gothic: Option<PathBuf>,
    pub udev_gothic_bold: Option<PathBuf>,
}

/// The file as it is read: every key optional, so that a key left out is
/// told from a key set to the default.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PartialConfig {
    monitor: Option<String>,
    width_ratio: Option<f64>,
    bottom_gap_px: Option<f64>,
    upper_rows: Option<usize>,
    lower_rows: Option<usize>,
    cinecaption: Option<PathBuf>,
    udev_gothic: Option<PathBuf>,
    udev_gothic_bold: Option<PathBuf>,
}

/// The settings of a machine with no `overlay.toml`.
pub fn builtin() -> Result<OverlayConfig, String> {
    Ok(OverlayConfig {
        monitor: String::new(),
        width_ratio: 0.8,
        bottom_gap_px: 0.0,
        upper_rows: 2,
        lower_rows: 1,
        cinecaption: None,
        udev_gothic: None,
        udev_gothic_bold: None,
    })
}

/// Reads the bytes of an `overlay.toml`. A key left out takes its default; a
/// key that is unknown, or a value out of range, is an error.
pub fn parse(bytes: &[u8]) -> Result<OverlayConfig, String> {
    let text = std::str::from_utf8(bytes).map_err(|error| error.to_string())?;
    let partial: PartialConfig = toml::from_str(text).map_err(|error| error.to_string())?;
    let config = fill(partial, builtin()?);
    check(&config)?;
    Ok(config)
}

fn fill(partial: PartialConfig, base: OverlayConfig) -> OverlayConfig {
    OverlayConfig {
        // DEFAULT: an absent key takes the value of the defaults table in 11.3 of the design
        monitor: partial.monitor.unwrap_or(base.monitor),
        // DEFAULT: an absent key takes the value of the defaults table in 11.3 of the design
        width_ratio: partial.width_ratio.unwrap_or(base.width_ratio),
        // DEFAULT: an absent key takes the value of the defaults table in 11.3 of the design
        bottom_gap_px: partial.bottom_gap_px.unwrap_or(base.bottom_gap_px),
        // DEFAULT: an absent key takes the value of the defaults table in 11.3 of the design
        upper_rows: partial.upper_rows.unwrap_or(base.upper_rows),
        // DEFAULT: an absent key takes the value of the defaults table in 11.3 of the design
        lower_rows: partial.lower_rows.unwrap_or(base.lower_rows),
        // DEFAULT: an absent key takes the value of the defaults table in 11.3 of the design
        cinecaption: partial.cinecaption.or(base.cinecaption),
        // DEFAULT: an absent key takes the value of the defaults table in 11.3 of the design
        udev_gothic: partial.udev_gothic.or(base.udev_gothic),
        // DEFAULT: an absent key takes the value of the defaults table in 11.3 of the design
        udev_gothic_bold: partial.udev_gothic_bold.or(base.udev_gothic_bold),
    }
}

fn check(config: &OverlayConfig) -> Result<(), String> {
    if !(config.width_ratio > 0.0 && config.width_ratio <= 1.0) {
        return Err(String::from("width_ratio must be above 0 and at most 1"));
    }
    if !(config.bottom_gap_px.is_finite() && config.bottom_gap_px >= 0.0) {
        return Err(String::from("bottom_gap_px must be a number of at least 0"));
    }
    for (key, rows) in [
        ("upper_rows", config.upper_rows),
        ("lower_rows", config.lower_rows),
    ] {
        if !(1..=MAX_ROWS).contains(&rows) {
            return Err(format!("{key} must be from 1 to {MAX_ROWS}"));
        }
    }
    Ok(())
}

/// The names of the keys whose values differ.
pub fn changed_keys(old: &OverlayConfig, new: &OverlayConfig) -> Vec<&'static str> {
    [
        ("monitor", old.monitor != new.monitor),
        ("width_ratio", old.width_ratio != new.width_ratio),
        ("bottom_gap_px", old.bottom_gap_px != new.bottom_gap_px),
        ("upper_rows", old.upper_rows != new.upper_rows),
        ("lower_rows", old.lower_rows != new.lower_rows),
        ("cinecaption", old.cinecaption != new.cinecaption),
        ("udev_gothic", old.udev_gothic != new.udev_gothic),
        (
            "udev_gothic_bold",
            old.udev_gothic_bold != new.udev_gothic_bold,
        ),
    ]
    .into_iter()
    .filter_map(|(key, differs)| differs.then_some(key))
    .collect()
}

/// `%APPDATA%\hayamimi\overlay.toml`; `None` when the machine names no
/// per-user configuration directory.
pub fn default_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("hayamimi").join("overlay.toml"))
}

/// Opens the settings: the defaults first, then the file at `path` if it exists.
pub fn open(
    path: Option<PathBuf>,
    sink: &mut dyn LogSink,
) -> Result<Watched<OverlayConfig>, String> {
    Watched::open(TABLE, path, builtin, parse, sink)
}

/// Reads the file again if it changed. When the settings in use differ
/// afterwards, one `overlay.config.reloaded` event names the keys, and the
/// same names are returned. A file that cannot be read is reported by
/// `Watched` as `table.load.failed` and changes nothing here.
pub fn refresh(watched: &mut Watched<OverlayConfig>, sink: &mut dyn LogSink) -> Vec<&'static str> {
    let before = watched.current().clone();
    watched.refresh(sink);
    let keys = changed_keys(&before, watched.current());
    if !keys.is_empty() {
        sink.emit(
            LogEvent::new(Level::Info, "overlay.config.reloaded", "browser")
                .with("keys", keys.clone()),
        );
    }
    keys
}

/// What `hayamimi-overlay` was asked to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Args {
    /// The streams to follow, in the order the log names them by number: the
    /// recognizers' `/events`, then the relay's. Empty in a layout probe.
    pub streams: Vec<String>,
    pub config: Option<PathBuf>,
    pub log: Option<PathBuf>,
    pub probe: Option<Probe>,
}

/// A layout probe: draws three fixed rows, measures them and reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Probe {
    /// The webview's zoom in percent, standing in for a screen at that scale.
    pub zoom_percent: u32,
    /// CSS px the window is made shorter than the band; a negative control.
    pub shrink_px: u32,
    /// The read-back of the click-through styles is made to find them unset; a
    /// negative control for the exit taken when the window cannot be clicked through.
    pub click_through_unset: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArgError {
    Unknown(String),
    MissingValue(&'static str),
    BadValue(&'static str),
    Twice(&'static str),
    OnlyWithProbe(&'static str),
    ProbeReadsNoStream(&'static str),
}

impl std::fmt::Display for ArgError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArgError::Unknown(flag) => write!(out, "unknown argument: {flag}"),
            ArgError::MissingValue(flag) => write!(out, "{flag} needs a value"),
            ArgError::BadValue(flag) => write!(out, "{flag} has a value that is not allowed"),
            ArgError::Twice(flag) => write!(out, "{flag} was given twice"),
            ArgError::OnlyWithProbe(flag) => write!(out, "{flag} needs --layout-probe"),
            ArgError::ProbeReadsNoStream(flag) => {
                write!(out, "{flag} cannot be used with --layout-probe")
            }
        }
    }
}

impl std::error::Error for ArgError {}

/// The zoom of a probe that is not told one: the screen's own scale.
const ZOOM_PERCENT_NONE: u32 = 100;
/// The range of the webview's zoom, in percent.
const ZOOM_PERCENT_RANGE: std::ops::RangeInclusive<u32> = 25..=500;

#[derive(Default)]
struct Raw {
    events: Vec<String>,
    relay: Option<String>,
    config: Option<PathBuf>,
    log: Option<PathBuf>,
    probe: bool,
    zoom: Option<u32>,
    shrink: Option<u32>,
    click_through_unset: bool,
}

fn value_of(
    flag: &'static str,
    rest: &mut impl Iterator<Item = String>,
) -> Result<String, ArgError> {
    rest.next().ok_or(ArgError::MissingValue(flag))
}

fn number_of(flag: &'static str, rest: &mut impl Iterator<Item = String>) -> Result<u32, ArgError> {
    value_of(flag, rest)?
        .parse()
        .map_err(|_| ArgError::BadValue(flag))
}

fn once<T>(slot: &mut Option<T>, value: T, flag: &'static str) -> Result<(), ArgError> {
    if slot.is_some() {
        return Err(ArgError::Twice(flag));
    }
    *slot = Some(value);
    Ok(())
}

/// The relay's `/events` on the port `hayamimi-relay live` serves on.
fn default_relay() -> String {
    format!("http://127.0.0.1:{DEFAULT_SERVE}/events")
}

impl Raw {
    fn take(
        &mut self,
        flag: &str,
        rest: &mut impl Iterator<Item = String>,
    ) -> Result<(), ArgError> {
        match flag {
            "--layout-probe" => self.probe = true,
            "--events" => self.events.push(value_of("--events", rest)?),
            "--relay" => once(&mut self.relay, value_of("--relay", rest)?, "--relay")?,
            "--config" => {
                let path = PathBuf::from(value_of("--config", rest)?);
                once(&mut self.config, path, "--config")?;
            }
            "--log" => {
                let path = PathBuf::from(value_of("--log", rest)?);
                once(&mut self.log, path, "--log")?;
            }
            "--zoom" => once(&mut self.zoom, number_of("--zoom", rest)?, "--zoom")?,
            "--probe-shrink" => once(
                &mut self.shrink,
                number_of("--probe-shrink", rest)?,
                "--probe-shrink",
            )?,
            "--probe-click-through-unset" => self.click_through_unset = true,
            other => return Err(ArgError::Unknown(other.to_owned())),
        }
        Ok(())
    }

    fn finish(self) -> Result<Args, ArgError> {
        if !self.probe {
            return self.finish_live();
        }
        if !self.events.is_empty() {
            return Err(ArgError::ProbeReadsNoStream("--events"));
        }
        if self.relay.is_some() {
            return Err(ArgError::ProbeReadsNoStream("--relay"));
        }
        // DEFAULT: a probe that is not told a zoom runs at the screen's own scale, 100 percent
        let zoom_percent = self.zoom.unwrap_or(ZOOM_PERCENT_NONE);
        if !ZOOM_PERCENT_RANGE.contains(&zoom_percent) {
            return Err(ArgError::BadValue("--zoom"));
        }
        Ok(Args {
            streams: Vec::new(),
            config: self.config,
            log: self.log,
            probe: Some(Probe {
                zoom_percent,
                // DEFAULT: a probe that is not told to shrink the window leaves it at the height of the band
                shrink_px: self.shrink.unwrap_or(0),
                click_through_unset: self.click_through_unset,
            }),
        })
    }

    fn finish_live(self) -> Result<Args, ArgError> {
        if self.zoom.is_some() {
            return Err(ArgError::OnlyWithProbe("--zoom"));
        }
        if self.shrink.is_some() {
            return Err(ArgError::OnlyWithProbe("--probe-shrink"));
        }
        if self.click_through_unset {
            return Err(ArgError::OnlyWithProbe("--probe-click-through-unset"));
        }
        let mut streams = self.events;
        if streams.is_empty() {
            streams = DEFAULT_EVENTS.map(str::to_owned).to_vec();
        }
        // DEFAULT: a relay that is not named is the one `hayamimi-relay live` serves on its default port
        let relay = self.relay.unwrap_or_else(default_relay);
        streams.push(relay);
        Ok(Args {
            streams,
            config: self.config,
            log: self.log,
            probe: None,
        })
    }
}

/// Reads the command line (without the program name).
pub fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Args, ArgError> {
    let mut rest = args.into_iter();
    let mut raw = Raw::default();
    while let Some(flag) = rest.next() {
        raw.take(&flag, &mut rest)?;
    }
    raw.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hayamimi_relay::logging::MemorySink;
    use std::fs;

    fn words(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_owned).collect()
    }

    fn scratch(name: &str) -> Result<tempfile::TempDir, std::io::Error> {
        tempfile::Builder::new()
            .prefix(name)
            .tempdir_in(concat!(env!("CARGO_MANIFEST_DIR"), "/../target"))
    }

    #[test]
    fn an_empty_file_gives_the_defaults() -> Result<(), String> {
        assert_eq!(parse(b"")?, builtin()?);
        Ok(())
    }

    #[test]
    fn the_defaults_are_the_band_of_the_design() -> Result<(), String> {
        let config = builtin()?;
        assert_eq!(config.monitor, "");
        assert_eq!(config.width_ratio, 0.8);
        assert_eq!(config.bottom_gap_px, 0.0);
        assert_eq!((config.upper_rows, config.lower_rows), (2, 1));
        assert_eq!(config.cinecaption, None);
        Ok(())
    }

    #[test]
    fn a_key_that_is_set_replaces_its_default_and_the_others_stay() -> Result<(), String> {
        let config = parse(b"width_ratio = 0.6\nlower_rows = 2\n")?;
        assert_eq!(config.width_ratio, 0.6);
        assert_eq!(config.lower_rows, 2);
        assert_eq!(config.upper_rows, 2);
        assert_eq!(config.monitor, "");
        Ok(())
    }

    #[test]
    fn the_font_paths_are_read_as_given() -> Result<(), String> {
        let config = parse(
            br"cinecaption = 'C:\fonts\a.ttf'
udev_gothic = 'C:\fonts\b.ttf'
udev_gothic_bold = 'C:\fonts\c.ttf'
",
        )?;
        assert_eq!(config.cinecaption, Some(PathBuf::from(r"C:\fonts\a.ttf")));
        assert_eq!(config.udev_gothic, Some(PathBuf::from(r"C:\fonts\b.ttf")));
        assert_eq!(
            config.udev_gothic_bold,
            Some(PathBuf::from(r"C:\fonts\c.ttf"))
        );
        Ok(())
    }

    #[test]
    fn a_key_the_overlay_does_not_know_is_refused() {
        assert!(parse(b"width_ratios = 0.6\n").is_err());
    }

    #[test]
    fn a_ratio_of_zero_below_zero_above_one_or_not_a_number_is_refused() {
        for value in ["0.0", "-0.5", "1.01", "nan", "inf"] {
            let text = format!("width_ratio = {value}\n");
            assert!(parse(text.as_bytes()).is_err(), "{value}");
        }
        assert!(parse(b"width_ratio = 1.0\n").is_ok());
    }

    #[test]
    fn a_gap_below_zero_or_infinite_is_refused() {
        for value in ["-1.0", "inf", "nan"] {
            let text = format!("bottom_gap_px = {value}\n");
            assert!(parse(text.as_bytes()).is_err(), "{value}");
        }
        assert!(parse(b"bottom_gap_px = 0.0\n").is_ok());
        assert!(parse(b"bottom_gap_px = 24.5\n").is_ok());
    }

    #[test]
    fn rows_of_zero_and_five_are_refused_and_one_and_four_pass() {
        for key in ["upper_rows", "lower_rows"] {
            for rows in [0, 5] {
                let text = format!("{key} = {rows}\n");
                assert!(parse(text.as_bytes()).is_err(), "{text}");
            }
            for rows in [1, 4] {
                let text = format!("{key} = {rows}\n");
                assert!(parse(text.as_bytes()).is_ok(), "{text}");
            }
        }
        assert!(parse(b"upper_rows = -1\n").is_err());
    }

    #[test]
    fn bytes_that_are_not_utf8_or_not_toml_are_refused() {
        assert!(parse(&[0xff, 0xfe]).is_err());
        assert!(parse(b"width_ratio = \n").is_err());
        assert!(parse(b"[monitor]\n").is_err());
    }

    #[test]
    fn the_changed_keys_are_named_and_an_unchanged_config_names_none() -> Result<(), String> {
        let base = builtin()?;
        assert!(changed_keys(&base, &base).is_empty());
        let moved = OverlayConfig {
            width_ratio: 0.6,
            monitor: String::from("DISPLAY2"),
            udev_gothic_bold: Some(PathBuf::from("b.ttf")),
            ..builtin()?
        };
        assert_eq!(
            changed_keys(&base, &moved),
            ["monitor", "width_ratio", "udev_gothic_bold"]
        );
        Ok(())
    }

    #[test]
    fn a_rewritten_file_changes_the_settings_and_logs_the_key_names_once()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = scratch("reload")?;
        let path = dir.path().join("overlay.toml");
        fs::write(&path, "width_ratio = 0.8\n")?;
        let mut sink = MemorySink::default();
        let mut watched = open(Some(path.clone()), &mut sink)?;
        fs::write(&path, "width_ratio = 0.6\n")?;
        let keys = refresh(&mut watched, &mut sink);
        assert_eq!(keys, ["width_ratio"]);
        assert_eq!(watched.current().width_ratio, 0.6);
        let reloaded: Vec<_> = sink
            .events
            .iter()
            .filter(|event| event.event == "overlay.config.reloaded")
            .collect();
        assert_eq!(reloaded.len(), 1);
        assert_eq!(
            reloaded[0].fields.get("keys"),
            Some(&serde_json::json!(["width_ratio"]))
        );
        assert!(refresh(&mut watched, &mut sink).is_empty());
        Ok(())
    }

    #[test]
    fn a_broken_file_keeps_the_settings_and_is_reported_by_the_table_event()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = scratch("broken")?;
        let path = dir.path().join("overlay.toml");
        fs::write(&path, "width_ratio = 0.7\n")?;
        let mut sink = MemorySink::default();
        let mut watched = open(Some(path.clone()), &mut sink)?;
        fs::write(&path, "width_ratio = 7\n")?;
        assert!(refresh(&mut watched, &mut sink).is_empty());
        assert_eq!(watched.current().width_ratio, 0.7);
        let failures: Vec<_> = sink
            .events
            .iter()
            .filter(|event| event.event == "table.load.failed")
            .collect();
        assert_eq!(failures.len(), 1);
        assert_eq!(
            failures[0].fields.get("table"),
            Some(&serde_json::json!("overlay"))
        );
        assert!(
            sink.events
                .iter()
                .all(|event| event.event != "overlay.config.reloaded")
        );
        Ok(())
    }

    #[test]
    fn no_file_leaves_the_defaults() -> Result<(), Box<dyn std::error::Error>> {
        let dir = scratch("absent")?;
        let mut sink = MemorySink::default();
        let watched = open(Some(dir.path().join("none.toml")), &mut sink)?;
        assert_eq!(watched.current(), &builtin()?);
        assert!(sink.events.is_empty());
        Ok(())
    }

    #[test]
    fn the_default_path_ends_in_hayamimi_overlay_toml() {
        if let Some(path) = default_path() {
            assert!(path.ends_with("hayamimi/overlay.toml"), "{path:?}");
        }
    }

    #[test]
    fn no_arguments_follow_the_two_recognizers_and_the_relay() -> Result<(), ArgError> {
        let args = parse_args(words(""))?;
        assert_eq!(
            args.streams,
            [
                "http://127.0.0.1:8833/events",
                "http://127.0.0.1:8834/events",
                "http://127.0.0.1:8835/events"
            ]
        );
        assert_eq!((args.config, args.log, args.probe), (None, None, None));
        Ok(())
    }

    #[test]
    fn named_streams_replace_the_defaults_and_the_relay_comes_last() -> Result<(), ArgError> {
        let args = parse_args(words(
            "--events http://a/events --relay http://r/events --events http://b/events",
        ))?;
        assert_eq!(
            args.streams,
            ["http://a/events", "http://b/events", "http://r/events"]
        );
        Ok(())
    }

    #[test]
    fn a_config_and_a_log_path_are_taken() -> Result<(), ArgError> {
        let args = parse_args(words("--config c.toml --log l.jsonl"))?;
        assert_eq!(args.config, Some(PathBuf::from("c.toml")));
        assert_eq!(args.log, Some(PathBuf::from("l.jsonl")));
        Ok(())
    }

    #[test]
    fn a_probe_reads_no_stream_and_defaults_to_100_percent_and_no_shrink() -> Result<(), ArgError> {
        let args = parse_args(words("--layout-probe"))?;
        assert!(args.streams.is_empty());
        assert_eq!(
            args.probe,
            Some(Probe {
                zoom_percent: 100,
                shrink_px: 0,
                click_through_unset: false
            })
        );
        Ok(())
    }

    #[test]
    fn the_unset_click_through_probe_is_taken_with_a_probe_only() -> Result<(), ArgError> {
        let args = parse_args(words("--layout-probe --probe-click-through-unset"))?;
        assert!(args.probe.is_some_and(|probe| probe.click_through_unset));
        assert_eq!(
            parse_args(words("--probe-click-through-unset")),
            Err(ArgError::OnlyWithProbe("--probe-click-through-unset"))
        );
        Ok(())
    }

    #[test]
    fn a_probe_takes_a_zoom_and_a_shrink_and_a_config() -> Result<(), ArgError> {
        let args = parse_args(words(
            "--layout-probe --zoom 150 --probe-shrink 12 --config c.toml",
        ))?;
        assert_eq!(
            args.probe,
            Some(Probe {
                zoom_percent: 150,
                shrink_px: 12,
                click_through_unset: false
            })
        );
        assert_eq!(args.config, Some(PathBuf::from("c.toml")));
        Ok(())
    }

    #[test]
    fn a_zoom_or_a_shrink_without_a_probe_is_an_argument_error() {
        assert_eq!(
            parse_args(words("--zoom 150")),
            Err(ArgError::OnlyWithProbe("--zoom"))
        );
        assert_eq!(
            parse_args(words("--probe-shrink 12")),
            Err(ArgError::OnlyWithProbe("--probe-shrink"))
        );
    }

    #[test]
    fn a_probe_with_a_stream_is_an_argument_error() {
        assert_eq!(
            parse_args(words("--layout-probe --events http://a/events")),
            Err(ArgError::ProbeReadsNoStream("--events"))
        );
        assert_eq!(
            parse_args(words("--layout-probe --relay http://a/events")),
            Err(ArgError::ProbeReadsNoStream("--relay"))
        );
    }

    #[test]
    fn a_zoom_outside_25_to_500_or_not_a_number_is_refused() {
        for bad in ["24", "501", "x", "-100", "1.5"] {
            let line = format!("--layout-probe --zoom {bad}");
            assert!(parse_args(words(&line)).is_err(), "{bad}");
        }
        for good in ["25", "100", "500"] {
            let line = format!("--layout-probe --zoom {good}");
            assert!(parse_args(words(&line)).is_ok(), "{good}");
        }
    }

    #[test]
    fn an_unknown_flag_a_missing_value_and_a_repeated_flag_are_refused() {
        assert_eq!(
            parse_args(words("--nope")),
            Err(ArgError::Unknown(String::from("--nope")))
        );
        assert_eq!(
            parse_args(words("--config")),
            Err(ArgError::MissingValue("--config"))
        );
        assert_eq!(
            parse_args(words("--log a --log b")),
            Err(ArgError::Twice("--log"))
        );
        assert_eq!(
            parse_args(words("--relay a --relay b")),
            Err(ArgError::Twice("--relay"))
        );
    }

    #[test]
    fn the_error_text_names_the_flag() {
        let said = ArgError::OnlyWithProbe("--zoom").to_string();
        assert!(
            said.contains("--zoom") && said.contains("--layout-probe"),
            "{said}"
        );
    }
}

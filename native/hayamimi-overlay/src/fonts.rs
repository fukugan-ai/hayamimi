//! The fonts of the two lanes: which file each one is read from, the
//! `@font-face` rules that declare them to the page, and the answer to the
//! page's request for a file.
//!
//! A font file is neither kept in the repository nor installed with the
//! program; the settings file names where each one is. The page asks for a
//! file by URL (`/hymfont/<route>`) and the window answers with the bytes of
//! the file the settings in use name at that moment. A font that is named but
//! cannot be read is still declared: the page then reports the face as
//! `error` and draws in the next family of its stack. A font that is not named
//! is not declared, and the page draws in what the machine has under that
//! family name or in the next family of the stack.

use std::collections::HashSet;
use std::fs::File;
use std::path::{Path, PathBuf};

use hayamimi_relay::logging::{Level, LogEvent, LogSink};
use hayamimi_ui::css::{LANES_FONT_EN, LANES_FONT_JA};

use crate::config::OverlayConfig;

/// The first segment of the URL a font is asked for by. The window registers
/// its answer under this name.
pub const ASSET_NAME: &str = "hymfont";

/// One font file the settings can name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Slot {
    Cinecaption,
    UdevGothic,
    UdevGothicBold,
}

impl Slot {
    pub const ALL: [Slot; 3] = [Slot::Cinecaption, Slot::UdevGothic, Slot::UdevGothicBold];

    /// The family the lanes' stylesheet names.
    pub fn family(self) -> &'static str {
        match self {
            Slot::Cinecaption => LANES_FONT_JA,
            Slot::UdevGothic | Slot::UdevGothicBold => LANES_FONT_EN,
        }
    }

    /// The key of the settings file that names the file.
    pub fn key(self) -> &'static str {
        match self {
            Slot::Cinecaption => "cinecaption",
            Slot::UdevGothic => "udev_gothic",
            Slot::UdevGothicBold => "udev_gothic_bold",
        }
    }

    /// The file the settings name for this font, if they name one.
    pub fn path(self, config: &OverlayConfig) -> Option<&Path> {
        match self {
            Slot::Cinecaption => config.cinecaption.as_deref(),
            Slot::UdevGothic => config.udev_gothic.as_deref(),
            Slot::UdevGothicBold => config.udev_gothic_bold.as_deref(),
        }
    }

    fn weight(self) -> u16 {
        match self {
            Slot::Cinecaption | Slot::UdevGothic => 400,
            Slot::UdevGothicBold => 700,
        }
    }

    /// The last segment of the URL the page asks this font by.
    fn route(self) -> &'static str {
        match self {
            Slot::Cinecaption => "ja",
            Slot::UdevGothic => "en",
            Slot::UdevGothicBold => "en-bold",
        }
    }

    fn url(self) -> String {
        format!("/{ASSET_NAME}/{}", self.route())
    }

    /// The font a URL path asks for: exactly `/hymfont/<route>`.
    fn of_path(uri_path: &str) -> Option<Slot> {
        let mut segments = uri_path.split('/').filter(|segment| !segment.is_empty());
        let (name, route) = (segments.next()?, segments.next()?);
        if name != ASSET_NAME || segments.next().is_some() {
            return None;
        }
        Slot::ALL.into_iter().find(|slot| slot.route() == route)
    }
}

/// The `@font-face` rules of the fonts the settings name.
pub fn font_face_css(config: &OverlayConfig) -> String {
    Slot::ALL
        .into_iter()
        .filter(|slot| slot.path(config).is_some())
        .map(|slot| {
            format!(
                "@font-face {{ font-family: \"{}\"; font-weight: {}; src: url(\"{}\"); }}\n",
                slot.family(),
                slot.weight(),
                slot.url()
            )
        })
        .collect()
}

/// What goes into the `<head>` of the page: the rules, in a `<style>`.
pub fn head(config: &OverlayConfig) -> String {
    format!("<style>\n{}</style>", font_face_css(config))
}

/// The script that makes the page ask again for the fonts the settings name,
/// after the settings changed. `version` is added to each URL so that the page
/// does not answer from what it already fetched. A face added later wins over
/// one declared earlier under the same family. Empty when no font is named.
pub fn reload_script(config: &OverlayConfig, version: u32) -> String {
    Slot::ALL
        .into_iter()
        .filter(|slot| slot.path(config).is_some())
        .map(|slot| {
            format!(
                "try {{ const face = new FontFace(\"{}\", 'url(\"{}?v={version}\")', {{ weight: '{}' }}); await face.load(); document.fonts.add(face); }} catch (error) {{}}\n",
                slot.family(),
                slot.url(),
                slot.weight()
            )
        })
        .collect()
}

/// The answer to one request of the page for a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answer {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

impl Answer {
    fn not_found() -> Answer {
        Answer {
            status: 404,
            content_type: "text/plain",
            body: Vec::new(),
        }
    }
}

fn content_type(path: &Path) -> &'static str {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        Some("ttc") => "font/collection",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

/// The answer for a request of `uri_path`, from the files `config` names.
/// A path that is no font's, a font no file is named for and a file that
/// cannot be read are all 404, which the page reads as a face in `error`.
pub fn answer(config: &OverlayConfig, uri_path: &str) -> Answer {
    let Some(slot) = Slot::of_path(uri_path) else {
        return Answer::not_found();
    };
    let Some(path) = slot.path(config) else {
        return Answer::not_found();
    };
    match std::fs::read(path) {
        Ok(body) => Answer {
            status: 200,
            content_type: content_type(path),
            body,
        },
        // The page reads this as a face in `error`; `Reported` says it in the log.
        Err(_) => Answer::not_found(),
    }
}

/// The fonts already reported as missing, so that each file is reported once.
#[derive(Default)]
pub struct Reported {
    seen: HashSet<(Slot, PathBuf)>,
}

impl Reported {
    /// Emits `overlay.font.missing` for each file the settings name that
    /// cannot be opened and has not been reported before. The path is not
    /// written: the font and the key that names it are.
    pub fn check(&mut self, config: &OverlayConfig, sink: &mut dyn LogSink) {
        for slot in Slot::ALL {
            let Some(path) = slot.path(config) else {
                continue;
            };
            if File::open(path).is_ok() {
                continue;
            }
            if self.seen.insert((slot, path.to_owned())) {
                sink.emit(missing(slot));
            }
        }
    }
}

fn missing(slot: Slot) -> LogEvent {
    LogEvent::new(Level::Warn, "overlay.font.missing", "browser")
        .with("font", slot.family())
        .with("key", slot.key())
        .with_error("FONT_MISSING")
}

#[cfg(test)]
mod tests {
    use hayamimi_relay::logging::MemorySink;

    use super::*;
    use crate::config;

    type Failure = Box<dyn std::error::Error>;

    fn scratch() -> Result<tempfile::TempDir, std::io::Error> {
        tempfile::Builder::new()
            .prefix("fonts")
            .tempdir_in(concat!(env!("CARGO_MANIFEST_DIR"), "/../target"))
    }

    fn named(config: &mut OverlayConfig, slot: Slot, path: &Path) {
        let path = Some(path.to_owned());
        match slot {
            Slot::Cinecaption => config.cinecaption = path,
            Slot::UdevGothic => config.udev_gothic = path,
            Slot::UdevGothicBold => config.udev_gothic_bold = path,
        }
    }

    #[test]
    fn no_rule_is_declared_for_a_font_the_settings_do_not_name() -> Result<(), Failure> {
        let config = config::builtin()?;
        assert_eq!(font_face_css(&config), "");
        Ok(())
    }

    #[test]
    fn a_rule_is_declared_for_each_named_font_under_the_family_of_the_stylesheet()
    -> Result<(), Failure> {
        let mut config = config::builtin()?;
        named(&mut config, Slot::Cinecaption, Path::new("a.ttf"));
        named(&mut config, Slot::UdevGothicBold, Path::new("b.ttf"));
        let css = font_face_css(&config);
        assert!(
            css.contains(
                r#"font-family: "cinecaption"; font-weight: 400; src: url("/hymfont/ja")"#
            ),
            "{css}"
        );
        assert!(
            css.contains(
                r#"font-family: "UDEV Gothic 35"; font-weight: 700; src: url("/hymfont/en-bold")"#
            ),
            "{css}"
        );
        assert_eq!(css.matches("@font-face").count(), 2, "{css}");
        Ok(())
    }

    #[test]
    fn the_reload_script_asks_again_for_each_named_font_under_a_new_version() -> Result<(), Failure>
    {
        let mut config = config::builtin()?;
        assert_eq!(reload_script(&config, 3), "");
        named(&mut config, Slot::Cinecaption, Path::new("a.ttf"));
        let script = reload_script(&config, 3);
        assert!(
            script.contains(r#"new FontFace("cinecaption", 'url("/hymfont/ja?v=3")'"#),
            "{script}"
        );
        assert!(script.contains("document.fonts.add(face)"), "{script}");
        assert_eq!(script.matches("new FontFace").count(), 1);
        Ok(())
    }

    #[test]
    fn the_head_wraps_the_rules_in_a_style_element() -> Result<(), Failure> {
        let mut config = config::builtin()?;
        named(&mut config, Slot::UdevGothic, Path::new("c.ttf"));
        let head = head(&config);
        assert!(
            head.starts_with("<style>") && head.ends_with("</style>"),
            "{head}"
        );
        Ok(())
    }

    #[test]
    fn a_path_is_a_font_only_as_slash_name_slash_route() {
        assert_eq!(Slot::of_path("/hymfont/ja"), Some(Slot::Cinecaption));
        assert_eq!(Slot::of_path("/hymfont/en"), Some(Slot::UdevGothic));
        assert_eq!(
            Slot::of_path("/hymfont/en-bold"),
            Some(Slot::UdevGothicBold)
        );
        for other in [
            "/",
            "/hymfont",
            "/hymfont/",
            "/hymfont/fr",
            "/hymfont/ja/more",
            "/other/ja",
            "/__events",
        ] {
            assert_eq!(Slot::of_path(other), None, "{other}");
        }
    }

    #[test]
    fn a_named_file_is_answered_with_its_bytes_and_a_type_from_its_extension() -> Result<(), Failure>
    {
        let dir = scratch()?;
        let file = dir.path().join("face.TTF");
        std::fs::write(&file, b"font bytes")?;
        let mut config = config::builtin()?;
        named(&mut config, Slot::Cinecaption, &file);
        let reply = answer(&config, "/hymfont/ja");
        assert_eq!(
            reply,
            Answer {
                status: 200,
                content_type: "font/ttf",
                body: b"font bytes".to_vec()
            }
        );
        Ok(())
    }

    #[test]
    fn the_file_is_read_when_it_is_asked_for_not_when_the_settings_were_read() -> Result<(), Failure>
    {
        let dir = scratch()?;
        let file = dir.path().join("face.otf");
        let mut config = config::builtin()?;
        named(&mut config, Slot::UdevGothic, &file);
        assert_eq!(answer(&config, "/hymfont/en").status, 404);
        std::fs::write(&file, b"later")?;
        assert_eq!(answer(&config, "/hymfont/en").body, b"later".to_vec());
        Ok(())
    }

    #[test]
    fn a_font_no_file_is_named_for_and_a_path_that_is_not_a_font_are_404() -> Result<(), Failure> {
        let config = config::builtin()?;
        assert_eq!(answer(&config, "/hymfont/ja").status, 404);
        assert_eq!(answer(&config, "/nothing").status, 404);
        Ok(())
    }

    #[test]
    fn a_named_file_that_cannot_be_read_is_reported_once_without_its_path() -> Result<(), Failure> {
        let dir = scratch()?;
        let gone = dir.path().join("gone.ttf");
        let mut config = config::builtin()?;
        named(&mut config, Slot::Cinecaption, &gone);
        let mut sink = MemorySink::default();
        let mut reported = Reported::default();
        reported.check(&config, &mut sink);
        reported.check(&config, &mut sink);
        assert_eq!(sink.events.len(), 1);
        let line = sink.events[0].to_json_line();
        assert!(line.contains(r#""event":"overlay.font.missing""#), "{line}");
        assert!(
            line.contains(r#""error_code":"HYM_BROWSER_FONT_MISSING""#),
            "{line}"
        );
        assert!(line.contains(r#""key":"cinecaption""#), "{line}");
        assert!(!line.contains("gone.ttf"), "{line}");
        Ok(())
    }

    #[test]
    fn a_file_that_can_be_read_and_a_font_not_named_are_not_reported() -> Result<(), Failure> {
        let dir = scratch()?;
        let file = dir.path().join("here.ttf");
        std::fs::write(&file, b"x")?;
        let mut config = config::builtin()?;
        named(&mut config, Slot::UdevGothic, &file);
        let mut sink = MemorySink::default();
        Reported::default().check(&config, &mut sink);
        assert!(sink.events.is_empty());
        Ok(())
    }

    #[test]
    fn a_file_named_again_under_another_path_is_reported_again() -> Result<(), Failure> {
        let dir = scratch()?;
        let mut config = config::builtin()?;
        let mut sink = MemorySink::default();
        let mut reported = Reported::default();
        named(&mut config, Slot::Cinecaption, &dir.path().join("one.ttf"));
        reported.check(&config, &mut sink);
        named(&mut config, Slot::Cinecaption, &dir.path().join("two.ttf"));
        reported.check(&config, &mut sink);
        assert_eq!(sink.events.len(), 2);
        Ok(())
    }
}

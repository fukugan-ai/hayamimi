//! The file that names where captions go besides the screens: the Zoom
//! caption API token and the Teams CART link.
//!
//! Both change with every meeting, so the file is read again whenever it
//! changes, without a restart. Its content is a credential: the file is read
//! only while no one but its owner can read it, a value never appears in a
//! log event or in an error text, and a file that cannot be parsed leaves the
//! previous content in use.
//!
//! ```toml
//! [zoom]
//! token = "https://…"    # the API token URL the meeting's host hands over
//!
//! [teams]
//! link = "https://…"     # the CART link the meeting's organizer creates
//! ```

use std::fmt;
use std::io;
use std::path::PathBuf;

use serde::Deserialize;

use crate::access;
use crate::logging::{Level, LogEvent, LogSink, SharedSink};
use crate::watched::Watched;

/// The name the file goes by in the log.
const TABLE: &str = "send";

/// A value that must not be written anywhere but the request it is for.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    /// The value itself, for the one place that sends it.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str("Secret(..)")
    }
}

/// What the file names. A destination that is not named has no value.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SendConfig {
    pub zoom: Option<Secret>,
    pub teams: Option<Secret>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ZoomPart {
    token: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TeamsPart {
    link: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FilePart {
    zoom: Option<ZoomPart>,
    teams: Option<TeamsPart>,
}

/// What a refused file says. It names no line and no value: the parser's own
/// message quotes the line it stopped at.
const INVALID: &str = "send.toml is not valid";

fn secret_of(value: String) -> Result<Secret, String> {
    let url = value.trim();
    let is_url = url.starts_with("https://") || url.starts_with("http://");
    if is_url {
        Ok(Secret(url.to_owned()))
    } else {
        Err(String::from(INVALID))
    }
}

/// Parses the bytes of the file.
pub fn parse(bytes: &[u8]) -> Result<SendConfig, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| String::from(INVALID))?;
    let file: FilePart = toml::from_str(text).map_err(|_| String::from(INVALID))?;
    Ok(SendConfig {
        zoom: file.zoom.map(|part| secret_of(part.token)).transpose()?,
        teams: file.teams.map(|part| secret_of(part.link)).transpose()?,
    })
}

fn no_destination() -> Result<SendConfig, String> {
    Ok(SendConfig::default())
}

/// What the file gives for one destination right now.
#[derive(Debug, PartialEq, Eq)]
pub enum Credential {
    /// The file is not there, or does not name this destination.
    Absent,
    /// Someone besides the owner can read the file, or that could not be
    /// checked. Nothing is read from it.
    TooOpen,
    Present(Secret),
}

/// The file, read again when it changes.
pub struct SendFile {
    path: PathBuf,
    config: Watched<SendConfig>,
    sink: SharedSink,
}

impl SendFile {
    pub fn open(path: PathBuf, mut sink: SharedSink) -> Result<SendFile, String> {
        let config = Watched::open(TABLE, Some(path.clone()), no_destination, parse, &mut sink)?;
        Ok(SendFile { path, config, sink })
    }

    /// Whether the file may be read. A file that is not there has nothing to
    /// protect; one whose access cannot be checked is treated as open.
    fn readable(&mut self) -> bool {
        match access::is_private(&self.path) {
            Ok(true) => true,
            Ok(false) => self.refuse(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => true,
            Err(_) => self.refuse(false),
        }
    }

    /// Says the file is too open, and answers that it is not to be read.
    fn refuse(&mut self, checked: bool) -> bool {
        self.sink.emit(
            LogEvent::new(Level::Error, "send.config.too_open", "security")
                .with("checked", checked)
                .with_error("SEND_CONFIG_TOO_OPEN"),
        );
        false
    }

    /// The credential `pick` takes from the file as it stands now.
    pub fn credential(&mut self, pick: fn(&SendConfig) -> Option<&Secret>) -> Credential {
        if !self.readable() {
            return Credential::TooOpen;
        }
        self.config.refresh(&mut self.sink);
        match pick(self.config.current()) {
            Some(secret) => Credential::Present(secret.clone()),
            None => Credential::Absent,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ZOOM: &str = "https://zoom.example/closedcaption?id=SECRET1&sig=SECRET2";
    const TEAMS: &str = "https://teams.example/cart/SECRET3";

    fn both() -> String {
        format!("[zoom]\ntoken = \"{ZOOM}\"\n\n[teams]\nlink = \"{TEAMS}\"\n")
    }

    #[test]
    fn a_file_with_both_destinations_parses() {
        let parsed = parse(both().as_bytes());
        assert_eq!(
            parsed,
            Ok(SendConfig {
                zoom: Some(Secret(ZOOM.to_owned())),
                teams: Some(Secret(TEAMS.to_owned())),
            })
        );
    }

    #[test]
    fn a_file_may_name_one_destination_or_none() {
        let zoom = parse(format!("[zoom]\ntoken = \"{ZOOM}\"\n").as_bytes());
        assert!(matches!(
            zoom,
            Ok(SendConfig {
                zoom: Some(_),
                teams: None
            })
        ));
        assert_eq!(parse(b""), Ok(SendConfig::default()));
    }

    #[test]
    fn a_value_that_is_not_a_url_is_refused() {
        for text in [
            "[zoom]\ntoken = \"abc\"\n",
            "[teams]\nlink = \"\"\n",
            "[teams]\nlink = \"ftp://x\"\n",
        ] {
            assert_eq!(parse(text.as_bytes()), Err(String::from(INVALID)), "{text}");
        }
    }

    #[test]
    fn an_unknown_key_is_refused_so_that_a_typo_is_not_silent() {
        let text = format!("[zoom]\ntokn = \"{ZOOM}\"\n");
        assert_eq!(parse(text.as_bytes()), Err(String::from(INVALID)));
    }

    #[test]
    fn a_refusal_never_quotes_the_file() {
        let broken = format!("[zoom]\ntoken = \"{ZOOM}\" oops\n");
        for bytes in [broken.as_bytes(), &[0xff, 0xfe][..]] {
            let Err(said) = parse(bytes) else {
                panic!("a broken file was accepted");
            };
            assert!(!said.contains("SECRET"), "{said}");
        }
    }

    #[test]
    fn a_secret_does_not_print() {
        let config = SendConfig {
            zoom: Some(Secret(ZOOM.to_owned())),
            teams: None,
        };
        let shown = format!("{config:?}");
        assert!(!shown.contains("SECRET"), "{shown}");
    }
}

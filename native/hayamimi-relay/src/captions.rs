//! The outside destinations of captions: the Zoom caption API and the Teams
//! CART link.
//!
//! Both take one caption per `POST` of plain text. The URL comes from the
//! send file (`send_config`), read again for every caption, because a
//! meeting hands over a new one each time. An answer that is not a success,
//! an address that does not answer, and a file that is too open are all a
//! destination that did not take the caption; the courier says so and keeps
//! the line in the subtitle file and on the screens.
//!
//! The wire form is that of the Zoom third-party caption API as the users of
//! it describe it (`seq` and `lang` in the query, a plain-text body). The
//! Teams CART link is posted the same way. Neither has been checked against a
//! live meeting.

use std::time::Duration;

use crate::deliver::{Caption, Destination, SendError};
use crate::send_config::{Credential, Secret, SendConfig, SendFile};

/// How long a destination is given to answer. The courier waits for it, and
/// the translations do not.
const SEND_TIMEOUT: Duration = Duration::from_secs(3);

/// The language Zoom is told the captions are in. Only translations into
/// English are sent outside.
const ZOOM_LANG: &str = "en-US";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Zoom,
    Teams,
}

fn zoom_of(config: &SendConfig) -> Option<&Secret> {
    config.zoom.as_ref()
}

fn teams_of(config: &SendConfig) -> Option<&Secret> {
    config.teams.as_ref()
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Zoom => "zoom",
            Kind::Teams => "teams",
        }
    }

    fn pick(self) -> fn(&SendConfig) -> Option<&Secret> {
        match self {
            Kind::Zoom => zoom_of,
            Kind::Teams => teams_of,
        }
    }

    /// The address of caption number `seq`.
    fn address(self, secret: &Secret, seq: u64) -> String {
        match self {
            Kind::Zoom => zoom_address(secret.expose(), seq),
            Kind::Teams => secret.expose().to_owned(),
        }
    }
}

/// The Zoom API token URL with the number of the caption and its language
/// added to the query it already has.
pub fn zoom_address(token: &str, seq: u64) -> String {
    let joiner = if token.contains('?') { '&' } else { '?' };
    format!("{token}{joiner}seq={seq}&lang={ZOOM_LANG}")
}

/// A destination that takes captions over HTTP.
pub struct HttpCaptions {
    kind: Kind,
    file: SendFile,
    agent: ureq::Agent,
    /// The number of the last caption sent, which Zoom wants to grow.
    seq: u64,
    /// What the file gave at the last [`Destination::ready`].
    credential: Credential,
}

impl HttpCaptions {
    pub fn new(kind: Kind, file: SendFile) -> HttpCaptions {
        // A redirect would carry the credential to an address it was not
        // given for.
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(SEND_TIMEOUT))
            .max_redirects(0)
            .build();
        HttpCaptions {
            kind,
            file,
            agent: ureq::Agent::new_with_config(config),
            seq: 0,
            credential: Credential::Absent,
        }
    }
}

/// Posts `text` to `address`. A success status is a caption taken. Any other
/// is not, a redirect included, because none is followed.
fn post(agent: &ureq::Agent, address: &str, text: &str) -> Result<(), SendError> {
    let sent = agent
        .post(address)
        .header("Content-Type", "text/plain; charset=utf-8")
        .send(text);
    match sent {
        Ok(response) if response.status().is_success() => Ok(()),
        Ok(response) => Err(SendError::Refused(response.status().as_u16())),
        Err(ureq::Error::StatusCode(code)) => Err(SendError::Refused(code)),
        // The error text may quote the address, which carries the credential.
        Err(_) => Err(SendError::Unreachable),
    }
}

impl Destination for HttpCaptions {
    fn name(&self) -> &'static str {
        self.kind.name()
    }

    fn ready(&mut self) -> bool {
        self.credential = self.file.credential(self.kind.pick());
        self.credential != Credential::Absent
    }

    fn send(&mut self, caption: &Caption) -> Result<(), SendError> {
        let Credential::Present(secret) = &self.credential else {
            return Err(SendError::Unreachable);
        };
        self.seq += 1;
        post(
            &self.agent,
            &self.kind.address(secret, self.seq),
            &caption.text,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_zoom_token_gets_the_number_and_the_language_in_its_query() {
        assert_eq!(
            zoom_address("https://z.example/closedcaption?id=1&sig=2", 7),
            "https://z.example/closedcaption?id=1&sig=2&seq=7&lang=en-US"
        );
    }

    #[test]
    fn a_token_without_a_query_gets_one() {
        assert_eq!(
            zoom_address("https://z.example/closedcaption", 1),
            "https://z.example/closedcaption?seq=1&lang=en-US"
        );
    }

    #[test]
    fn the_teams_link_is_posted_as_it_is() -> Result<(), String> {
        let text = "[teams]\nlink = \"https://t.example/cart/abc\"\n";
        let config = crate::send_config::parse(text.as_bytes())?;
        let Some(link) = config.teams else {
            return Err(String::from("the link was not read"));
        };
        assert_eq!(Kind::Teams.address(&link, 5), "https://t.example/cart/abc");
        Ok(())
    }

    #[test]
    fn the_two_kinds_have_their_own_names() {
        assert_eq!(Kind::Zoom.name(), "zoom");
        assert_eq!(Kind::Teams.name(), "teams");
    }
}

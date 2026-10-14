//! The one place that talks HTTP. The rest of the command reads an address
//! through `Net`, so that its tests can give it a stand-in that records the
//! addresses asked for.

use std::io::Read;
use std::time::Duration;

use super::error::FetchError;

/// Opens an address as a stream of its body.
pub trait Net {
    fn open(&self, url: &str) -> Result<Box<dyn Read>, FetchError>;
}

/// How long a connection may take to be made.
const CONNECT: Duration = Duration::from_secs(10);
/// How long the server may take to start answering.
const RESPONSE: Duration = Duration::from_secs(60);
/// How long a whole body may take: a few hundred megabytes on a slow line.
const BODY: Duration = Duration::from_secs(2 * 60 * 60);

pub struct Http {
    agent: ureq::Agent,
}

impl Http {
    pub fn new() -> Http {
        let config = ureq::Agent::config_builder()
            .timeout_connect(Some(CONNECT))
            .timeout_recv_response(Some(RESPONSE))
            .timeout_recv_body(Some(BODY))
            .build();
        Http {
            agent: ureq::Agent::new_with_config(config),
        }
    }
}

impl Net for Http {
    fn open(&self, url: &str) -> Result<Box<dyn Read>, FetchError> {
        match self.agent.get(url).call() {
            Ok(response) => Ok(Box::new(response.into_body().into_reader())),
            Err(ureq::Error::StatusCode(code)) => Err(FetchError::Status {
                url: url.to_owned(),
                code,
            }),
            Err(other) => Err(FetchError::Net {
                url: url.to_owned(),
                message: other.to_string(),
            }),
        }
    }
}

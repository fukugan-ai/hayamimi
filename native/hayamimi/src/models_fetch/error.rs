//! What can go wrong while a model group is fetched. Every variant names the
//! address or the file it concerns, and `reason` gives the short token that the
//! log event carries instead of text.

use std::fmt;
use std::path::PathBuf;

#[derive(Debug)]
pub enum FetchError {
    /// The address could not be reached, or the connection broke.
    Net { url: String, message: String },
    /// The server answered with a status that is not a success.
    Status { url: String, code: u16 },
    /// A file or directory under the models directory could not be written.
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    /// The archive is not a readable `.tar.bz2`.
    Archive { url: String, message: String },
    /// The archive does not hold some of the files asked for.
    MissingMember { names: Vec<String> },
    /// The file list of a repository is not the JSON it must be.
    BadListing { url: String },
    /// A file name from a repository listing would leave the group's directory.
    BadName { name: String },
}

impl FetchError {
    /// A failed write of `path`.
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> FetchError {
        FetchError::Io {
            path: path.into(),
            source,
        }
    }

    /// The token the log event carries as `reason`.
    pub fn reason(&self) -> &'static str {
        match self {
            FetchError::Net { .. } => "unreachable",
            FetchError::Status { .. } => "http_status",
            FetchError::Io { .. } => "io",
            FetchError::Archive { .. } => "bad_archive",
            FetchError::MissingMember { .. } => "missing_member",
            FetchError::BadListing { .. } | FetchError::BadName { .. } => "bad_listing",
        }
    }
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FetchError::Net { url, message } => write!(f, "{url}: {message}"),
            FetchError::Status { url, code } => write!(f, "{url}: HTTP {code}"),
            FetchError::Io { path, source } => write!(f, "{}: {source}", path.display()),
            FetchError::Archive { url, message } => write!(f, "{url}: {message}"),
            FetchError::MissingMember { names } => {
                write!(f, "the archive has no {}", names.join(", "))
            }
            FetchError::BadListing { url } => write!(f, "{url}: not a file list"),
            FetchError::BadName { name } => write!(f, "{name}: not a name inside the group"),
        }
    }
}

impl std::error::Error for FetchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            FetchError::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_error_has_a_reason_token_and_a_text() {
        let errors = [
            FetchError::Net {
                url: String::from("u"),
                message: String::from("m"),
            },
            FetchError::Status {
                url: String::from("u"),
                code: 404,
            },
            FetchError::io("p", std::io::Error::from(std::io::ErrorKind::NotFound)),
            FetchError::Archive {
                url: String::from("u"),
                message: String::from("m"),
            },
            FetchError::MissingMember {
                names: vec![String::from("a"), String::from("b")],
            },
            FetchError::BadListing {
                url: String::from("u"),
            },
            FetchError::BadName {
                name: String::from("../x"),
            },
        ];
        let reasons: Vec<&str> = errors.iter().map(FetchError::reason).collect();
        assert_eq!(
            reasons,
            [
                "unreachable",
                "http_status",
                "io",
                "bad_archive",
                "missing_member",
                "bad_listing",
                "bad_listing"
            ]
        );
        assert!(errors.iter().all(|error| !error.to_string().is_empty()));
    }

    #[test]
    fn a_missing_member_error_names_every_file() {
        let error = FetchError::MissingMember {
            names: vec![String::from("a"), String::from("b")],
        };
        assert_eq!(error.to_string(), "the archive has no a, b");
    }
}

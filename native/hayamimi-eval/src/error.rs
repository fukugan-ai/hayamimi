//! The one error type of the tool. Every variant names what failed, so a
//! failed run says which file, address or row it stopped on.

use std::fmt;

#[derive(Debug)]
pub enum EvalError {
    /// A file could not be read or written.
    Io {
        path: String,
        source: std::io::Error,
    },
    /// A file is not the JSON it must be.
    Json {
        path: String,
        source: serde_json::Error,
    },
    /// A wav file is not 16-bit PCM mono, or cannot be decoded.
    Wav { path: String, message: String },
    /// The TCP connection to the server could not be opened.
    Connect {
        address: String,
        source: std::io::Error,
    },
    /// The WebSocket upgrade was refused or malformed.
    Handshake(String),
    /// The peer sent bytes that are not a frame this tool understands.
    Protocol(String),
    /// An HTTP request failed before a status was read.
    Http { url: String, message: String },
    /// A manifest or an input list is not usable as it stands.
    Manifest(String),
    /// A value on the command line or in a result file is out of range.
    Invalid(String),
}

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EvalError::Io { path, source } => write!(f, "{path}: {source}"),
            EvalError::Json { path, source } => write!(f, "{path}: not valid JSON: {source}"),
            EvalError::Wav { path, message } => write!(f, "{path}: {message}"),
            EvalError::Connect { address, source } => {
                write!(f, "cannot connect to {address}: {source}")
            }
            EvalError::Handshake(message) => write!(f, "WebSocket handshake failed: {message}"),
            EvalError::Protocol(message) => write!(f, "protocol error: {message}"),
            EvalError::Http { url, message } => write!(f, "{url}: {message}"),
            EvalError::Manifest(message) => write!(f, "manifest: {message}"),
            EvalError::Invalid(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for EvalError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            EvalError::Io { source, .. } | EvalError::Connect { source, .. } => Some(source),
            EvalError::Json { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl EvalError {
    /// An I/O failure on `path`.
    pub fn io(path: &std::path::Path, source: std::io::Error) -> EvalError {
        EvalError::Io {
            path: path.display().to_string(),
            source,
        }
    }

    /// A JSON failure in `path`.
    pub fn json(path: &std::path::Path, source: serde_json::Error) -> EvalError {
        EvalError::Json {
            path: path.display().to_string(),
            source,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn an_io_error_names_its_path() {
        let source = std::io::Error::from(std::io::ErrorKind::NotFound);
        let error = EvalError::io(Path::new("a/b.json"), source);
        assert!(error.to_string().starts_with("a/b.json: "));
    }

    #[test]
    fn a_json_error_names_its_path() {
        let Err(source) = serde_json::from_str::<serde_json::Value>("{") else {
            panic!("unterminated JSON parsed");
        };
        let error = EvalError::json(Path::new("m.json"), source);
        assert!(error.to_string().starts_with("m.json: not valid JSON"));
    }

    #[test]
    fn the_connect_error_keeps_the_address() {
        let source = std::io::Error::from(std::io::ErrorKind::ConnectionRefused);
        let error = EvalError::Connect {
            address: String::from("127.0.0.1:9"),
            source,
        };
        assert!(error.to_string().contains("127.0.0.1:9"));
    }
}

//! Errors of the developer tasks. Each variant names what was being done, so
//! that a failed build step reads without a backtrace.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum Error {
    /// No task name was given.
    Usage,
    UnknownTask(String),
    UnknownFlag(String),
    DuplicateFlag(&'static str),
    MissingValue(&'static str),
    MissingFlag(&'static str),
    Io {
        action: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    Spawn {
        program: String,
        source: io::Error,
    },
    Failed {
        program: String,
        code: Option<i32>,
    },
    /// A tool printed something this task cannot read.
    Unreadable {
        what: &'static str,
    },
    /// A file that must exist after a step is missing.
    Missing {
        what: &'static str,
        path: PathBuf,
    },
    /// No runtime library with this name prefix was found to stage.
    NotStaged(&'static str),
    Json(serde_json::Error),
}

impl Error {
    /// An adapter for `map_err` that records which action on which path failed.
    pub fn io(action: &'static str, path: &Path) -> impl FnOnce(io::Error) -> Error {
        let path = path.to_path_buf();
        move |source| Error::Io {
            action,
            path,
            source,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Usage => f.write_str(
                "usage: xtask sherpa-gpu --cuda-dir <dir> --cudnn-dir <dir> [--sherpa-tag <tag>] \
                 [--ort-version <version>] [--ort-dir <dir>] [--out-dir <dir>]",
            ),
            Error::UnknownTask(task) => write!(f, "unknown task `{task}`"),
            Error::UnknownFlag(flag) => write!(f, "unknown flag `{flag}`"),
            Error::DuplicateFlag(flag) => write!(f, "flag `{flag}` was given twice"),
            Error::MissingValue(flag) => write!(f, "flag `{flag}` needs a value"),
            Error::MissingFlag(flag) => write!(f, "flag `{flag}` is required"),
            Error::Io {
                action,
                path,
                source,
            } => write!(f, "cannot {action} `{}`: {source}", path.display()),
            Error::Spawn { program, source } => write!(f, "cannot start `{program}`: {source}"),
            Error::Failed { program, code } => match code {
                Some(code) => write!(f, "`{program}` exited with status {code}"),
                None => write!(f, "`{program}` was terminated by a signal"),
            },
            Error::Unreadable { what } => write!(f, "cannot read {what}"),
            Error::Missing { what, path } => write!(f, "{what} is missing: `{}`", path.display()),
            Error::NotStaged(prefix) => {
                write!(f, "no runtime library named `{prefix}*` was found to stage")
            }
            Error::Json(source) => write!(f, "cannot read or write a JSON file: {source}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io { source, .. } | Error::Spawn { source, .. } => Some(source),
            Error::Json(source) => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_program_is_named_with_its_status() {
        let said = Error::Failed {
            program: String::from("cmake"),
            code: Some(2),
        }
        .to_string();
        assert!(said.contains("cmake") && said.contains('2'), "{said}");
    }

    #[test]
    fn an_io_error_names_the_action_and_the_path() {
        let made = Error::io("read", Path::new("a/b"))(io::Error::other("boom"));
        let said = made.to_string();
        assert!(
            said.contains("read") && said.contains("a/b") && said.contains("boom"),
            "{said}"
        );
    }

    #[test]
    fn a_prefix_that_found_nothing_is_named() {
        let said = Error::NotStaged("cudart64_").to_string();
        assert!(said.contains("cudart64_"), "{said}");
    }
}

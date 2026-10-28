//! Errors of the GPU probe. Each variant names the step that failed.

use std::fmt;
use std::io;

#[derive(Debug)]
pub enum GpuError {
    /// A command-line flag is unknown, repeated, valueless or missing.
    Usage(String),
    /// The NVIDIA driver library could not be opened.
    DriverLibrary(libloading::Error),
    /// A symbol of an opened library could not be resolved.
    Symbol {
        name: &'static str,
        source: libloading::Error,
    },
    /// A CUDA driver call returned a non-zero status.
    Cuda {
        call: &'static str,
        status: i32,
    },
    /// The ONNX Runtime library could not be opened.
    OnnxRuntime(libloading::Error),
    /// The ONNX Runtime library returned a version that is not text.
    VersionString,
    /// The running executable has no parent directory.
    NoExecutableDirectory,
    /// The process module list could not be read.
    Modules(String),
    /// A path cannot be handed to the C library as UTF-8 text.
    PathNotUtf8,
    /// The wave file could not be read.
    Wave,
    /// The wave file declares a sample rate that is not positive.
    SampleRate(i32),
    /// The recognizer could not be created.
    Recognizer,
    /// The recognizer produced no result.
    NoResult,
    /// A number does not fit the width it is reported in.
    Overflow(&'static str),
    Json(serde_json::Error),
    Io(io::Error),
    /// The result of a recognition is not JSON.
    ResultNotJson(serde_json::Error),
    /// The sherpa-onnx C library that was loaded is not the release these crates were written against.
    Library {
        found: String,
        expected: &'static str,
    },
}

impl fmt::Display for GpuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GpuError::Usage(why) => write!(f, "usage: {why}"),
            GpuError::DriverLibrary(source) => {
                write!(f, "cannot open the NVIDIA driver library: {source}")
            }
            GpuError::Symbol { name, source } => write!(f, "cannot resolve `{name}`: {source}"),
            GpuError::Cuda { call, status } => write!(f, "`{call}` returned CUDA status {status}"),
            GpuError::OnnxRuntime(source) => {
                write!(f, "cannot open the ONNX Runtime library: {source}")
            }
            GpuError::VersionString => f.write_str("the ONNX Runtime version string is not text"),
            GpuError::NoExecutableDirectory => {
                f.write_str("the running executable has no parent directory")
            }
            GpuError::Modules(why) => write!(f, "cannot list the process modules: {why}"),
            GpuError::PathNotUtf8 => f.write_str("a path is not UTF-8"),
            GpuError::Wave => f.write_str("cannot read the wave file"),
            GpuError::SampleRate(rate) => write!(f, "the wave file declares sample rate {rate}"),
            GpuError::Recognizer => f.write_str("cannot create the recognizer"),
            GpuError::NoResult => f.write_str("the recognizer produced no result"),
            GpuError::Overflow(what) => write!(f, "{what} does not fit its reported width"),
            GpuError::Json(source) => write!(f, "cannot write the probe output: {source}"),
            GpuError::Io(source) => write!(f, "cannot write the probe output: {source}"),
            GpuError::ResultNotJson(source) => {
                write!(f, "the recognition result is not JSON: {source}")
            }
            GpuError::Library { found, expected } => write!(
                f,
                "the sherpa-onnx C library is release {found}, but this build was written for {expected}; \
                 point SHERPA_ONNX_LIB_DIR (and LD_LIBRARY_PATH or PATH) at a library built by \
                 `cargo run -p xtask -- sherpa-gpu` (docs/guide/build_gpu.ja.md)"
            ),
        }
    }
}

impl std::error::Error for GpuError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            GpuError::DriverLibrary(source)
            | GpuError::OnnxRuntime(source)
            | GpuError::Symbol { source, .. } => Some(source),
            GpuError::Json(source) | GpuError::ResultNotJson(source) => Some(source),
            GpuError::Io(source) => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cuda_failure_names_the_call_and_the_status() {
        let said = GpuError::Cuda {
            call: "cuInit",
            status: 100,
        }
        .to_string();
        assert!(said.contains("cuInit") && said.contains("100"), "{said}");
    }

    #[test]
    fn a_bad_sample_rate_is_shown() {
        assert!(GpuError::SampleRate(0).to_string().contains('0'));
    }

    #[test]
    fn a_library_of_another_release_names_both_releases_and_the_remedy() {
        let said = GpuError::Library {
            found: String::from("1.13.8"),
            expected: "2.0.1",
        }
        .to_string();
        assert!(said.contains("1.13.8") && said.contains("2.0.1"), "{said}");
        assert!(
            said.contains("SHERPA_ONNX_LIB_DIR") && said.contains("sherpa-gpu"),
            "{said}"
        );
    }
}

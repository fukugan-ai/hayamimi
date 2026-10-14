//! The ONNX Runtime the sherpa-onnx build ships, opened through `ort` for the
//! one model sherpa-onnx cannot run: the punctuation BERT.
//!
//! A second ONNX Runtime in the same process makes it fail, so this opens the
//! library that is already loaded for sherpa-onnx, by its name: the file name
//! of the same module, not a second copy. `OrtRuntime::take_node_counts`
//! returns the node placement ONNX Runtime logs when a session is built.

use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use ort::environment::Environment;
use ort::ep;
use ort::logging::LogLevel;
use ort::session::Session;

use crate::error::AsrError;

/// The library as sherpa-onnx's C API names it: its SONAME on Linux, so the
/// module that is already mapped answers; on Windows the module preloaded
/// from the directory of the executable answers to the bare name.
const LIBRARY: &str = if cfg!(windows) {
    "onnxruntime.dll"
} else {
    "libonnxruntime.so.1"
};

fn ort_error(error: impl std::fmt::Display) -> AsrError {
    AsrError::Ort(error.to_string())
}

/// The provider a session is built for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Device {
    Cuda,
    Cpu,
}

impl Device {
    /// The provider name used in log events.
    pub fn name(self) -> &'static str {
        match self {
            Device::Cuda => "cuda",
            Device::Cpu => "cpu",
        }
    }
}

/// How many nodes ONNX Runtime put on one execution provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placement {
    pub provider: String,
    pub nodes: u64,
}

/// The nodes of one session on the CUDA provider and on the CPU provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeCounts {
    pub cuda: u64,
    pub cpu: u64,
}

/// The nodes-per-provider figures in one ONNX Runtime log line, if it has any.
///
/// The line reads `Node(s) placed on [CPUExecutionProvider]. Number of nodes: 12`.
pub fn parse_placement(message: &str) -> Option<Placement> {
    let (_, after) = message.split_once("placed on [")?;
    let (provider, rest) = after.split_once(']')?;
    let (_, count) = rest.split_once("Number of nodes:")?;
    let nodes = count.split_whitespace().next()?.parse().ok()?;
    Some(Placement {
        provider: provider.to_owned(),
        nodes,
    })
}

/// Sums placements per provider. A session that logged none, or one that
/// names a provider other than CUDA and CPU, is an error: its nodes would be
/// counted nowhere.
pub fn count_nodes(placements: &[Placement]) -> Result<NodeCounts, AsrError> {
    if placements.is_empty() {
        return Err(AsrError::Ort(String::from(
            "ONNX Runtime logged no node placement; the session was not built at verbose level",
        )));
    }
    let mut counts = NodeCounts { cuda: 0, cpu: 0 };
    for placement in placements {
        if placement.provider.starts_with("CUDA") {
            counts.cuda += placement.nodes;
        } else if placement.provider.starts_with("CPU") {
            counts.cpu += placement.nodes;
        } else {
            return Err(AsrError::Ort(format!(
                "nodes were placed on the unexpected provider `{}`",
                placement.provider
            )));
        }
    }
    Ok(counts)
}

/// Whether the inference libraries (sherpa-onnx and the ONNX Runtime) can be
/// opened. Asked before the first sherpa-onnx call; the answer is kept.
pub fn check_library() -> Result<(), AsrError> {
    OrtRuntime::shared().map(|_| ())
}

/// An ONNX Runtime opened by name, with the lines it logs about node placement kept.
pub struct OrtRuntime {
    environment: Environment,
    placements: Arc<Mutex<Vec<String>>>,
}

/// The one runtime of the process, opened on first use.
static SHARED: OnceLock<Result<OrtRuntime, String>> = OnceLock::new();

impl OrtRuntime {
    /// The runtime of this process. A second ONNX Runtime in the same process
    /// makes it fail, so every session is built on this one; a failure to open
    /// it is kept and reported again to every caller.
    pub fn shared() -> Result<&'static OrtRuntime, AsrError> {
        match SHARED.get_or_init(|| OrtRuntime::open().map_err(|error| error.to_string())) {
            Ok(runtime) => Ok(runtime),
            Err(message) => Err(AsrError::Ort(message.clone())),
        }
    }

    /// Opens the runtime that sherpa-onnx uses.
    fn open() -> Result<Self, AsrError> {
        #[cfg(windows)]
        {
            // The binary delay-loads sherpa-onnx; an absent library would end the
            // process at the first call, so it is asked for here, before any call.
            hayamimi_gpu::runtime::require_near_executable("sherpa-onnx-c-api.dll")?;
            hayamimi_gpu::runtime::preload_near_executable(LIBRARY)?;
        }
        let placements: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&placements);
        let environment = ort::init_from(Path::new(LIBRARY))
            .map_err(ort_error)?
            .with_logger(Arc::new(
                move |_: LogLevel, _: &str, _: &str, _: &str, message: &str| {
                    if message.contains("placed on")
                        && let Ok(mut lines) = sink.lock()
                    {
                        lines.push(message.to_owned());
                    }
                },
            ))
            .build()
            .map_err(ort_error)?;
        environment.set_log_level(LogLevel::Warning);
        Ok(Self {
            environment,
            placements,
        })
    }

    /// A session on `model`. A CUDA session fails to build when the CUDA
    /// provider cannot be used, so the session that comes back runs there.
    /// `verbose` makes ONNX Runtime log the node placement.
    pub fn session(
        &self,
        model: &Path,
        device: Device,
        threads: usize,
        verbose: bool,
    ) -> Result<Session, AsrError> {
        let mut builder = Session::builder(&self.environment).map_err(ort_error)?;
        if device == Device::Cuda {
            builder = builder
                .with_execution_providers([ep::CUDA::default().build().error_on_failure()])
                .map_err(ort_error)?;
        }
        if verbose {
            self.environment.set_log_level(LogLevel::Verbose);
            builder = builder
                .with_log_level(LogLevel::Verbose)
                .map_err(ort_error)?;
        }
        let session = builder
            .with_intra_threads(threads)
            .map_err(ort_error)?
            .commit_from_file(model)
            .map_err(ort_error);
        self.environment.set_log_level(LogLevel::Warning);
        session
    }

    /// The node counts of the session built since the last call, from the
    /// placement lines ONNX Runtime logged.
    pub fn take_node_counts(&self) -> Result<NodeCounts, AsrError> {
        let mut lines = self
            .placements
            .lock()
            .map_err(|_| AsrError::Ort(String::from("the placement log is poisoned")))?;
        let found: Vec<Placement> = lines.iter().filter_map(|l| parse_placement(l)).collect();
        lines.clear();
        count_nodes(&found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on(provider: &str, nodes: u64) -> Placement {
        Placement {
            provider: provider.to_owned(),
            nodes,
        }
    }

    #[test]
    fn a_placement_line_gives_the_provider_and_the_count() {
        let found =
            parse_placement(" Node(s) placed on [CPUExecutionProvider]. Number of nodes: 12");
        assert_eq!(found, Some(on("CPUExecutionProvider", 12)));
    }

    #[test]
    fn a_line_without_a_count_gives_nothing() {
        assert_eq!(
            parse_placement("All nodes placed on [CUDAExecutionProvider]."),
            None
        );
        assert_eq!(parse_placement("Node placements"), None);
    }

    #[test]
    fn a_count_that_is_not_a_number_gives_nothing() {
        assert_eq!(
            parse_placement("Node(s) placed on [CPUExecutionProvider]. Number of nodes: many"),
            None
        );
    }

    #[test]
    fn placements_are_summed_per_provider() -> Result<(), AsrError> {
        let counts = count_nodes(&[
            on("CUDAExecutionProvider", 900),
            on("CPUExecutionProvider", 40),
            on("CPUExecutionProvider", 2),
        ])?;
        assert_eq!(counts, NodeCounts { cuda: 900, cpu: 42 });
        Ok(())
    }

    #[test]
    fn no_placement_line_is_an_error_not_zero_cpu_nodes() {
        assert!(count_nodes(&[]).is_err());
    }

    #[test]
    fn an_unexpected_provider_is_an_error() {
        assert!(count_nodes(&[on("TensorrtExecutionProvider", 3)]).is_err());
    }

    #[test]
    fn devices_name_the_provider_of_the_log_events() {
        assert_eq!(Device::Cuda.name(), "cuda");
        assert_eq!(Device::Cpu.name(), "cpu");
    }
}

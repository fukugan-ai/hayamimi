//! What to start: which build of the server, how many layers on the GPU, and
//! the order of attempts. Everything here is a pure function of the GPU check,
//! so the choice can be tested without starting a process.

use std::ffi::OsString;
use std::path::Path;

use hayamimi_asr::{Gpu, Reason};

/// Parallel requests the server takes: the microphone's final line, the
/// loopback's final line, the partial line and the edit.
pub const SLOTS: u32 = 4;
/// The context shared by the slots. Each slot gets `CONTEXT_TOKENS / SLOTS`,
/// which holds the longest edit prompt and its answer; the server's default
/// (the model's whole training context) would reserve far more memory.
pub const CONTEXT_TOKENS: u32 = 8192;
/// More layers than the model has, so that every layer is offloaded.
const ALL_LAYERS: u32 = 99;
/// The address the server listens on; the relay reaches it on the same machine.
pub const HOST: &str = "127.0.0.1";

/// Which build of the server runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Build {
    /// The CUDA build.
    Cuda,
    /// The build with no GPU backend.
    Cpu,
}

impl Build {
    /// The name used in events.
    pub fn name(self) -> &'static str {
        match self {
            Build::Cuda => "cuda",
            Build::Cpu => "cpu",
        }
    }
}

/// One way of starting the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    pub build: Build,
    /// Layers placed on the GPU; 0 keeps the whole model on the CPU.
    pub gpu_layers: u32,
}

/// The ways to start the server, best first. With a device the CUDA build
/// runs every layer on the GPU and the CPU build is the last resort. Without
/// one the CUDA build is still tried first with no layers on the GPU (it needs
/// no device for that), then the CPU build.
pub fn attempts(gpu: Gpu) -> [Plan; 2] {
    let first = match gpu {
        Gpu::Try => Plan {
            build: Build::Cuda,
            gpu_layers: ALL_LAYERS,
        },
        Gpu::Absent(_) => Plan {
            build: Build::Cuda,
            gpu_layers: 0,
        },
    };
    let last = Plan {
        build: Build::Cpu,
        gpu_layers: 0,
    };
    [first, last]
}

/// Why the server runs on the CPU, or `None` when it runs where the GPU check
/// put it: on the GPU.
pub fn fallback_reason(gpu: Gpu, used: Plan) -> Option<Reason> {
    match gpu {
        Gpu::Absent(why) => Some(why),
        Gpu::Try if used.gpu_layers == 0 => Some(Reason::SessionFailed),
        Gpu::Try => None,
    }
}

/// The command-line arguments of the server for `plan`, with `slots`
/// parallel requests.
pub fn server_args(plan: Plan, model: &Path, port: u16, slots: u32) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec![OsString::from("-m"), model.as_os_str().to_owned()];
    for (name, value) in [
        ("--host", HOST.to_owned()),
        ("--port", port.to_string()),
        ("-np", slots.to_string()),
        ("-c", CONTEXT_TOKENS.to_string()),
        ("-ngl", plan.gpu_layers.to_string()),
    ] {
        args.push(OsString::from(name));
        args.push(OsString::from(value));
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    const CUDA_ALL: Plan = Plan {
        build: Build::Cuda,
        gpu_layers: ALL_LAYERS,
    };
    const CUDA_NONE: Plan = Plan {
        build: Build::Cuda,
        gpu_layers: 0,
    };
    const CPU: Plan = Plan {
        build: Build::Cpu,
        gpu_layers: 0,
    };

    fn text(args: &[OsString]) -> Vec<String> {
        args.iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn a_machine_with_a_device_starts_on_the_gpu_and_ends_on_the_cpu_build() {
        assert_eq!(attempts(Gpu::Try), [CUDA_ALL, CPU]);
    }

    #[test]
    fn a_machine_without_a_device_never_asks_for_gpu_layers() {
        for why in [Reason::NoDevice, Reason::DriverUnreadable] {
            let plans = attempts(Gpu::Absent(why));
            assert_eq!(plans, [CUDA_NONE, CPU]);
            assert!(plans.iter().all(|plan| plan.gpu_layers == 0));
        }
    }

    #[test]
    fn a_start_on_the_gpu_is_not_a_fallback() {
        assert_eq!(fallback_reason(Gpu::Try, CUDA_ALL), None);
    }

    #[test]
    fn a_device_that_could_not_be_used_is_a_session_failure() {
        assert_eq!(
            fallback_reason(Gpu::Try, CUDA_NONE),
            Some(Reason::SessionFailed)
        );
        assert_eq!(fallback_reason(Gpu::Try, CPU), Some(Reason::SessionFailed));
    }

    #[test]
    fn a_missing_device_is_named_whichever_build_ran() {
        let gone = Gpu::Absent(Reason::NoDevice);
        assert_eq!(fallback_reason(gone, CUDA_NONE), Some(Reason::NoDevice));
        assert_eq!(fallback_reason(gone, CPU), Some(Reason::NoDevice));
        let driver = Gpu::Absent(Reason::DriverUnreadable);
        assert_eq!(fallback_reason(driver, CPU), Some(Reason::DriverUnreadable));
    }

    #[test]
    fn the_arguments_carry_the_model_the_port_four_slots_and_the_gpu_layers() {
        let args = server_args(CUDA_ALL, Path::new("m.gguf"), 8099, SLOTS);
        assert_eq!(
            text(&args),
            [
                "-m",
                "m.gguf",
                "--host",
                "127.0.0.1",
                "--port",
                "8099",
                "-np",
                "4",
                "-c",
                "8192",
                "-ngl",
                "99"
            ]
        );
    }

    #[test]
    fn the_cpu_arguments_keep_every_layer_off_the_gpu() {
        let args = text(&server_args(CPU, Path::new("m.gguf"), 8099, SLOTS));
        let at = args.iter().position(|arg| arg == "-ngl");
        assert_eq!(at.map(|index| args[index + 1].as_str()), Some("0"));
    }

    #[test]
    fn the_slot_count_is_the_one_asked_for() {
        let args = text(&server_args(CUDA_ALL, Path::new("m.gguf"), 8081, 1));
        let at = args.iter().position(|arg| arg == "-np");
        assert_eq!(at.map(|index| args[index + 1].as_str()), Some("1"));
    }

    #[test]
    fn the_slots_fit_the_context() {
        const { assert!(CONTEXT_TOKENS / SLOTS >= 2048) };
    }
}

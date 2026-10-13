//! The RP-17 acceptance rule over what one probe run observed.
//!
//! A run passes when the self-built CUDA library opened one ONNX Runtime on
//! the CUDA provider, and the CUDA libraries the process loaded are the CUDA 13
//! ones. The file name of the provider library cannot tell the CUDA version
//! (it names `cudart` of both majors as plain text), so the rule reads the
//! process module list instead: CUDA 13 cuBLAS must be loaded once, the CUDA 12
//! runtime and cuBLAS must not be loaded at all. Other CUDA 12 libraries are
//! allowed, because CUDA 13.4 itself ships `cufft64_12.dll`. Names are
//! compared ignoring ASCII case.
//!
//! A run that passes is recorded in `rp17_passed.json` beside the
//! `build.json` it ran against, with the SHA-256 of that `build.json`.

use std::fmt;
use std::io;
use std::path::Path;

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::modules::{LoadedModule, Origin, is_named};

/// The library names that tell the CUDA major version, per operating system.
#[derive(Debug, Clone, Copy)]
pub struct RuntimeNames {
    /// Each of these must be loaded exactly once.
    pub required: [&'static str; 2],
    /// Each of these must not be loaded.
    pub forbidden: [&'static str; 3],
    /// The ONNX Runtime library; it must come from the executable directory.
    pub onnxruntime: &'static str,
}

pub const WINDOWS: RuntimeNames = RuntimeNames {
    required: ["cublas64_13.dll", "cublaslt64_13.dll"],
    forbidden: ["cudart64_12.dll", "cublas64_12.dll", "cublaslt64_12.dll"],
    onnxruntime: "onnxruntime.dll",
};

pub const LINUX: RuntimeNames = RuntimeNames {
    required: ["libcublas.so.13", "libcublaslt.so.13"],
    forbidden: ["libcudart.so.12", "libcublas.so.12", "libcublaslt.so.12"],
    onnxruntime: "libonnxruntime.so",
};

/// What one probe run reported.
#[derive(Debug, Clone, Copy)]
pub struct Observed<'a> {
    pub cuda_devices: u32,
    pub loaded_provider: &'a str,
    pub ort_version: &'a str,
    pub onnxruntime_libraries: u32,
    pub modules: &'a [LoadedModule],
}

/// The first rule a run broke.
#[derive(Debug, PartialEq, Eq)]
pub enum Rp17Error {
    NoCudaDevice,
    ProviderNotCuda(String),
    OrtVersion {
        built: String,
        opened: String,
    },
    OnnxruntimeCount(u32),
    /// A required library is loaded `times` times instead of once.
    RequiredNotOnce {
        name: &'static str,
        times: usize,
    },
    /// A library of the wrong CUDA major version is loaded.
    ForbiddenLoaded(&'static str),
    OnnxruntimeOrigin(Origin),
}

impl fmt::Display for Rp17Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Rp17Error::NoCudaDevice => f.write_str("no CUDA device"),
            Rp17Error::ProviderNotCuda(provider) => {
                write!(f, "the provider is `{provider}`, not `cuda`")
            }
            Rp17Error::OrtVersion { built, opened } => {
                write!(f, "ONNX Runtime {opened} is open, the build used {built}")
            }
            Rp17Error::OnnxruntimeCount(count) => {
                write!(f, "{count} ONNX Runtime libraries are loaded, not 1")
            }
            Rp17Error::RequiredNotOnce { name, times } => {
                write!(f, "`{name}` is loaded {times} times, not once")
            }
            Rp17Error::ForbiddenLoaded(name) => write!(f, "`{name}` is loaded"),
            Rp17Error::OnnxruntimeOrigin(origin) => {
                write!(
                    f,
                    "ONNX Runtime was loaded from {origin:?}, not the executable directory"
                )
            }
        }
    }
}

impl std::error::Error for Rp17Error {}

fn times_loaded(modules: &[LoadedModule], library: &str) -> usize {
    modules
        .iter()
        .filter(|row| is_named(&row.name, library))
        .count()
}

fn check_runtime(modules: &[LoadedModule], names: &RuntimeNames) -> Result<(), Rp17Error> {
    for name in names.required {
        let times = times_loaded(modules, name);
        if times != 1 {
            return Err(Rp17Error::RequiredNotOnce { name, times });
        }
    }
    for name in names.forbidden {
        if times_loaded(modules, name) != 0 {
            return Err(Rp17Error::ForbiddenLoaded(name));
        }
    }
    for row in modules {
        if is_named(&row.name, names.onnxruntime) && row.origin != Origin::ExeDirectory {
            return Err(Rp17Error::OnnxruntimeOrigin(row.origin));
        }
    }
    Ok(())
}

/// Checks one probe run against the ONNX Runtime version the build recorded.
pub fn check(
    observed: &Observed,
    built_ort_version: &str,
    names: &RuntimeNames,
) -> Result<(), Rp17Error> {
    if observed.cuda_devices == 0 {
        return Err(Rp17Error::NoCudaDevice);
    }
    if observed.loaded_provider != "cuda" {
        return Err(Rp17Error::ProviderNotCuda(
            observed.loaded_provider.to_owned(),
        ));
    }
    if observed.ort_version != built_ort_version {
        return Err(Rp17Error::OrtVersion {
            built: built_ort_version.to_owned(),
            opened: observed.ort_version.to_owned(),
        });
    }
    if observed.onnxruntime_libraries != 1 {
        return Err(Rp17Error::OnnxruntimeCount(observed.onnxruntime_libraries));
    }
    check_runtime(observed.modules, names)
}

/// The fields of the probe output that RP-17 reads.
#[derive(Debug, Deserialize)]
struct Report {
    cuda_devices: u32,
    loaded_provider: String,
    ort_version: String,
    onnxruntime_libraries: u32,
    modules: Vec<LoadedModule>,
}

/// Why a run could not be judged, or was judged and failed.
#[derive(Debug)]
pub enum JudgeError {
    Io(io::Error),
    Json(serde_json::Error),
    /// `build.json` has no `ort_version` text.
    NoBuiltVersion,
    Rejected(Rp17Error),
}

impl fmt::Display for JudgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JudgeError::Io(error) => write!(f, "cannot read or write a record: {error}"),
            JudgeError::Json(error) => write!(f, "cannot read a JSON file: {error}"),
            JudgeError::NoBuiltVersion => f.write_str("build.json has no `ort_version`"),
            JudgeError::Rejected(error) => write!(f, "RP-17 failed: {error}"),
        }
    }
}

impl std::error::Error for JudgeError {}

impl From<io::Error> for JudgeError {
    fn from(error: io::Error) -> JudgeError {
        JudgeError::Io(error)
    }
}

impl From<serde_json::Error> for JudgeError {
    fn from(error: serde_json::Error) -> JudgeError {
        JudgeError::Json(error)
    }
}

impl From<Rp17Error> for JudgeError {
    fn from(error: Rp17Error) -> JudgeError {
        JudgeError::Rejected(error)
    }
}

/// The lower-case hexadecimal SHA-256 of `bytes`.
fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Judges one probe output against the `build.json` in `platform_dir`. When
/// the run passes, writes `rp17_passed.json` there; when it fails, writes
/// nothing and leaves any earlier record as it was.
pub fn judge_and_record(
    platform_dir: &Path,
    probe_output: &str,
    names: &RuntimeNames,
) -> Result<(), JudgeError> {
    let build_bytes = std::fs::read(platform_dir.join("build.json"))?;
    let build: serde_json::Value = serde_json::from_slice(&build_bytes)?;
    let Some(built_ort_version) = build.get("ort_version").and_then(|value| value.as_str()) else {
        return Err(JudgeError::NoBuiltVersion);
    };
    let report: Report = serde_json::from_str(probe_output)?;
    let observed = Observed {
        cuda_devices: report.cuda_devices,
        loaded_provider: &report.loaded_provider,
        ort_version: &report.ort_version,
        onnxruntime_libraries: report.onnxruntime_libraries,
        modules: &report.modules,
    };
    check(&observed, built_ort_version, names)?;
    let record = serde_json::json!({
        "schema_version": 1,
        "build_json_sha256": sha256_hex(&build_bytes),
        "probe": serde_json::from_str::<serde_json::Value>(probe_output)?,
    });
    let target = platform_dir.join("rp17_passed.json");
    let part = platform_dir.join("rp17_passed.json.part");
    std::fs::write(&part, serde_json::to_string_pretty(&record)?)?;
    std::fs::rename(&part, &target)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUILT: &str = "1.28.2";

    fn row(name: &str, origin: Origin) -> LoadedModule {
        LoadedModule {
            name: name.to_owned(),
            origin,
        }
    }

    /// The modules a good Windows run loads, with the executable directory as origin.
    fn windows() -> Vec<LoadedModule> {
        [
            "cublas64_13.dll",
            "cublaslt64_13.dll",
            "cudnn64_9.dll",
            "onnxruntime.dll",
            "onnxruntime_providers_cuda.dll",
        ]
        .map(|name| row(name, Origin::ExeDirectory))
        .to_vec()
    }

    /// The modules a good WSL run maps; the file names carry the full version.
    fn wsl() -> Vec<LoadedModule> {
        [
            "libcublas.so.13.4.1.1",
            "libcublaslt.so.13.4.1.1",
            "libonnxruntime.so.1.28.2",
        ]
        .map(|name| row(name, Origin::ExeDirectory))
        .to_vec()
    }

    fn observe(modules: &[LoadedModule]) -> Observed<'_> {
        Observed {
            cuda_devices: 1,
            loaded_provider: "cuda",
            ort_version: BUILT,
            onnxruntime_libraries: 1,
            modules,
        }
    }

    fn windows_check(modules: &[LoadedModule]) -> Result<(), Rp17Error> {
        check(&observe(modules), BUILT, &WINDOWS)
    }

    fn wsl_check(modules: &[LoadedModule]) -> Result<(), Rp17Error> {
        check(&observe(modules), BUILT, &LINUX)
    }

    fn with(mut modules: Vec<LoadedModule>, extra: &str) -> Vec<LoadedModule> {
        modules.push(row(extra, Origin::ExeDirectory));
        modules
    }

    fn without(modules: Vec<LoadedModule>, name: &str) -> Vec<LoadedModule> {
        modules
            .into_iter()
            .filter(|module| module.name != name)
            .collect()
    }

    #[test]
    fn probe_output_with_all_expected_values_passes() {
        assert_eq!(windows_check(&windows()), Ok(()));
    }

    #[test]
    fn zero_cuda_devices_is_rejected() {
        let modules = windows();
        let observed = Observed {
            cuda_devices: 0,
            ..observe(&modules)
        };
        assert_eq!(
            check(&observed, BUILT, &WINDOWS),
            Err(Rp17Error::NoCudaDevice)
        );
    }

    #[test]
    fn cpu_provider_is_rejected() {
        let modules = windows();
        let observed = Observed {
            loaded_provider: "cpu",
            ..observe(&modules)
        };
        assert!(matches!(
            check(&observed, BUILT, &WINDOWS),
            Err(Rp17Error::ProviderNotCuda(provider)) if provider == "cpu"
        ));
    }

    #[test]
    fn ort_version_differing_from_build_json_is_rejected() {
        let modules = windows();
        let observed = Observed {
            ort_version: "1.28.1",
            ..observe(&modules)
        };
        assert!(matches!(
            check(&observed, BUILT, &WINDOWS),
            Err(Rp17Error::OrtVersion { opened, .. }) if opened == "1.28.1"
        ));
    }

    #[test]
    fn two_onnxruntime_libraries_are_rejected() {
        let modules = windows();
        let observed = Observed {
            onnxruntime_libraries: 2,
            ..observe(&modules)
        };
        assert_eq!(
            check(&observed, BUILT, &WINDOWS),
            Err(Rp17Error::OnnxruntimeCount(2))
        );
    }

    #[test]
    fn zero_onnxruntime_libraries_is_rejected() {
        let modules = windows();
        let observed = Observed {
            onnxruntime_libraries: 0,
            ..observe(&modules)
        };
        assert_eq!(
            check(&observed, BUILT, &WINDOWS),
            Err(Rp17Error::OnnxruntimeCount(0))
        );
    }

    #[test]
    fn windows_and_wsl_outputs_have_the_same_keys() -> Result<(), Box<dyn std::error::Error>> {
        let output = |os: &'static str, modules: Vec<LoadedModule>| crate::probe::Probe {
            os,
            requested_provider: "cuda",
            cuda_devices: 1,
            loaded_provider: "cuda",
            ort_version: String::from(BUILT),
            onnxruntime_libraries: 1,
            audio_ms: 9600,
            decode_ms: 650,
            decoded_chars: 44,
            modules,
        };
        let keys =
            |probe: &crate::probe::Probe| -> Result<Vec<String>, Box<dyn std::error::Error>> {
                let value = serde_json::to_value(probe)?;
                let object = value
                    .as_object()
                    .ok_or("the probe output is not an object")?;
                let mut keys: Vec<String> = object.keys().cloned().collect();
                keys.sort();
                Ok(keys)
            };
        let windows_keys = keys(&output("windows", windows()))?;
        assert!(windows_keys.len() > 5, "{windows_keys:?}");
        assert_eq!(windows_keys, keys(&output("linux", wsl()))?);
        Ok(())
    }

    #[test]
    fn cudart64_12_loaded_is_rejected() {
        let modules = with(windows(), "cudart64_12.dll");
        assert_eq!(
            windows_check(&modules),
            Err(Rp17Error::ForbiddenLoaded("cudart64_12.dll"))
        );
    }

    #[test]
    fn missing_cublas64_13_is_rejected() {
        let modules = without(windows(), "cublas64_13.dll");
        assert_eq!(
            windows_check(&modules),
            Err(Rp17Error::RequiredNotOnce {
                name: "cublas64_13.dll",
                times: 0
            })
        );
    }

    #[test]
    fn two_cublas64_13_modules_are_rejected() {
        let modules = with(windows(), "cublas64_13.dll");
        assert_eq!(
            windows_check(&modules),
            Err(Rp17Error::RequiredNotOnce {
                name: "cublas64_13.dll",
                times: 2
            })
        );
    }

    #[test]
    fn missing_cublaslt64_13_is_rejected() {
        let modules = without(windows(), "cublaslt64_13.dll");
        assert_eq!(
            windows_check(&modules),
            Err(Rp17Error::RequiredNotOnce {
                name: "cublaslt64_13.dll",
                times: 0
            })
        );
    }

    #[test]
    fn cublas64_12_loaded_is_rejected() {
        let modules = with(windows(), "cublas64_12.dll");
        assert_eq!(
            windows_check(&modules),
            Err(Rp17Error::ForbiddenLoaded("cublas64_12.dll"))
        );
    }

    #[test]
    fn cublaslt64_12_loaded_is_rejected() {
        let modules = with(windows(), "cublaslt64_12.dll");
        assert_eq!(
            windows_check(&modules),
            Err(Rp17Error::ForbiddenLoaded("cublaslt64_12.dll"))
        );
    }

    #[test]
    fn cufft64_12_alone_is_accepted() {
        let modules = with(windows(), "cufft64_12.dll");
        assert_eq!(windows_check(&modules), Ok(()));
    }

    #[test]
    fn onnxruntime_loaded_from_system_is_rejected() {
        let modules: Vec<LoadedModule> = windows()
            .into_iter()
            .map(|module| {
                if module.name == "onnxruntime.dll" {
                    LoadedModule {
                        origin: Origin::System,
                        ..module
                    }
                } else {
                    module
                }
            })
            .collect();
        assert_eq!(
            windows_check(&modules),
            Err(Rp17Error::OnnxruntimeOrigin(Origin::System))
        );
    }

    #[test]
    fn uppercase_cublas_names_are_counted() {
        let modules: Vec<LoadedModule> = windows()
            .into_iter()
            .map(|module| LoadedModule {
                name: module.name.to_ascii_uppercase(),
                ..module
            })
            .collect();
        assert_eq!(windows_check(&modules), Ok(()));
    }

    #[test]
    fn uppercase_cudart64_12_is_rejected() {
        let modules = with(windows(), "CUDART64_12.DLL");
        assert_eq!(
            windows_check(&modules),
            Err(Rp17Error::ForbiddenLoaded("cudart64_12.dll"))
        );
    }

    #[test]
    fn wsl_modules_with_cublas_13_and_without_cudart_12_pass() {
        assert_eq!(wsl_check(&wsl()), Ok(()));
    }

    #[test]
    fn wsl_libcudart_so_12_loaded_is_rejected() {
        let modules = with(wsl(), "libcudart.so.12.9.79");
        assert_eq!(
            wsl_check(&modules),
            Err(Rp17Error::ForbiddenLoaded("libcudart.so.12"))
        );
    }

    #[test]
    fn wsl_missing_libcublas_so_13_is_rejected() {
        let modules = without(wsl(), "libcublas.so.13.4.1.1");
        assert_eq!(
            wsl_check(&modules),
            Err(Rp17Error::RequiredNotOnce {
                name: "libcublas.so.13",
                times: 0
            })
        );
    }

    #[test]
    fn a_provider_library_is_not_counted_as_onnxruntime_itself() {
        let mut modules = windows();
        modules.push(row("onnxruntime_providers_shared.dll", Origin::Elsewhere));
        assert_eq!(windows_check(&modules), Ok(()));
    }
}

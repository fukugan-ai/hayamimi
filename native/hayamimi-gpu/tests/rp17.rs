//! RP-17: the self-built CUDA library opens one ONNX Runtime on the CUDA
//! provider, and the process loads the CUDA 13 libraries.
//!
//! The real-run test starts the `hayamimi-gpu-probe` binary on one wave file,
//! judges its JSON against the `build.json` beside the library directory, and
//! (on a pass) writes `rp17_passed.json` there. It needs a CUDA device, the
//! ReazonSpeech model under a `models` directory above this crate, and the
//! `build.json` that `cargo run -p xtask -- sherpa-gpu` wrote.

use std::collections::BTreeSet;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;

use hayamimi_gpu::link_args::{self, LIBRARY_CRATE, Metadata};
use hayamimi_gpu::rp17::{self, RuntimeNames};
use serde_json::Value;

/// The library directory the crate was linked against (a compile-time constant).
const LIB_DIR: &str = env!("SHERPA_ONNX_LIB_DIR");
const PROBE: &str = env!("CARGO_BIN_EXE_hayamimi-gpu-probe");
const MANIFEST_DIR: &str = env!("CARGO_MANIFEST_DIR");
const CARGO: &str = env!("CARGO");
const SCRATCH_DIR: &str = env!("CARGO_TARGET_TMPDIR");
const MODEL: &str = "sherpa-onnx-zipformer-ja-en-reazonspeech-2025-01-17";
const WAVE: &str = "tests/golden/ja/ja_014.wav";

#[cfg(windows)]
const NAMES: RuntimeNames = rp17::WINDOWS;
#[cfg(not(windows))]
const NAMES: RuntimeNames = rp17::LINUX;

/// `<out>/<os>`, which holds `build.json`; the library directory is `<out>/<os>/install/lib`.
fn platform_dir() -> Result<&'static Path, Box<dyn Error>> {
    Path::new(LIB_DIR)
        .ancestors()
        .nth(2)
        .ok_or_else(|| "the library directory is too shallow to hold a build.json beside it".into())
}

/// The nearest `models/<MODEL>` directory above this crate.
fn model_dir() -> Result<PathBuf, Box<dyn Error>> {
    for ancestor in Path::new(MANIFEST_DIR).ancestors() {
        let candidate = ancestor.join("models").join(MODEL);
        if candidate.is_dir() {
            return Ok(candidate);
        }
    }
    Err(format!("no `models/{MODEL}` directory above the crate").into())
}

fn run_probe() -> Result<String, Box<dyn Error>> {
    let model = model_dir()?;
    let wave = Path::new(MANIFEST_DIR).join("../..").join(WAVE);
    let output = Command::new(PROBE)
        .arg("--encoder")
        .arg(model.join("encoder-epoch-35-avg-1.onnx"))
        .arg("--decoder")
        .arg(model.join("decoder-epoch-35-avg-1.onnx"))
        .arg("--joiner")
        .arg(model.join("joiner-epoch-35-avg-1.onnx"))
        .arg("--tokens")
        .arg(model.join("tokens.txt"))
        .arg("--wav")
        .arg(wave)
        .output()?;
    if !output.status.success() {
        return Err(format!("the probe exited with {}", output.status).into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

fn workspace_metadata() -> Result<Metadata, Box<dyn Error>> {
    let output = Command::new(CARGO)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
        ])
        .current_dir(MANIFEST_DIR)
        .output()?;
    if !output.status.success() {
        return Err(format!("cargo metadata exited with {}", output.status).into());
    }
    Ok(Metadata::parse(&String::from_utf8(output.stdout)?)?)
}

#[test]
fn rp17_self_built_cuda_library_opens_one_onnxruntime() -> Result<(), Box<dyn Error>> {
    rp17::judge_and_record(platform_dir()?, &run_probe()?, &NAMES)?;
    if cfg!(windows) {
        link_args::check_workspace(&workspace_metadata()?)?;
    }
    Ok(())
}

#[test]
fn dependent_crates_from_metadata_equal_those_from_manifest_search() -> Result<(), Box<dyn Error>> {
    let from_metadata: BTreeSet<String> = workspace_metadata()?.dependents().into_iter().collect();
    let native = Path::new(MANIFEST_DIR).join("..");
    let mut from_manifests = BTreeSet::new();
    for entry in std::fs::read_dir(&native)? {
        let directory = entry?.path();
        let Ok(manifest) = std::fs::read_to_string(directory.join("Cargo.toml")) else {
            continue;
        };
        let name = directory.file_name().and_then(|name| name.to_str());
        if let Some(name) =
            name.filter(|name| *name != LIBRARY_CRATE && manifest.contains(LIBRARY_CRATE))
        {
            from_manifests.insert(name.to_owned());
        }
    }
    assert_eq!(from_metadata, from_manifests);
    Ok(())
}

/// The provider library names both CUDA majors of `cudart` as plain text, so
/// the text cannot decide the version; the rule reads the loaded modules.
#[cfg(windows)]
#[test]
fn strings_inside_the_provider_library_do_not_decide_the_cuda_version() -> Result<(), Box<dyn Error>>
{
    let bytes = std::fs::read(Path::new(LIB_DIR).join("onnxruntime_providers_cuda.dll"))?;
    let holds = |text: &[u8]| bytes.windows(text.len()).any(|window| window == text);
    assert!(holds(b"cudart64_12.dll"), "no cudart64_12.dll string");
    assert!(holds(b"cudart64_13.dll"), "no cudart64_13.dll string");
    let modules = ["cublas64_13.dll", "cublaslt64_13.dll", "onnxruntime.dll"].map(|name| {
        hayamimi_gpu::modules::LoadedModule {
            name: name.to_owned(),
            origin: hayamimi_gpu::modules::Origin::ExeDirectory,
        }
    });
    let observed = rp17::Observed {
        cuda_devices: 1,
        loaded_provider: "cuda",
        ort_version: "1.31.0",
        onnxruntime_libraries: 1,
        modules: &modules,
    };
    rp17::check(&observed, "1.31.0", &NAMES)?;
    Ok(())
}

/// A scratch directory under `target/tmp` holding a fixture `build.json`.
fn fixture_dir(name: &str) -> Result<PathBuf, Box<dyn Error>> {
    let directory = Path::new(SCRATCH_DIR).join(name);
    if directory.exists() {
        std::fs::remove_dir_all(&directory)?;
    }
    std::fs::create_dir_all(&directory)?;
    std::fs::write(directory.join("build.json"), r#"{"ort_version":"1.28.2"}"#)?;
    Ok(directory)
}

fn fixture_probe(extra_module: Option<&str>) -> String {
    let mut modules = vec!["cublas64_13.dll", "cublaslt64_13.dll", "onnxruntime.dll"];
    modules.extend(extra_module);
    let rows: Vec<Value> = modules
        .into_iter()
        .map(|name| serde_json::json!({"name": name, "origin": "exe_directory"}))
        .collect();
    serde_json::json!({
        "os": "windows", "requested_provider": "cuda", "cuda_devices": 1,
        "loaded_provider": "cuda", "ort_version": "1.28.2", "onnxruntime_libraries": 1,
        "audio_ms": 9600, "decode_ms": 650, "decoded_chars": 44, "modules": rows,
    })
    .to_string()
}

#[test]
fn passing_probe_writes_rp17_passed_json_with_the_build_json_sha256() -> Result<(), Box<dyn Error>>
{
    let directory = fixture_dir("rp17_pass")?;
    rp17::judge_and_record(&directory, &fixture_probe(None), &rp17::WINDOWS)?;
    let record: Value =
        serde_json::from_slice(&std::fs::read(directory.join("rp17_passed.json"))?)?;
    // The digest of the 24-byte fixture `{"ort_version":"1.28.2"}`, taken with `sha256sum`.
    assert_eq!(
        record.get("build_json_sha256").and_then(Value::as_str),
        Some("ad42b37ce87832cf383680198e5fbd0f0d80a46cf520bc92cfb20cbdbbbc5d2d")
    );
    assert_eq!(
        record.pointer("/probe/ort_version").and_then(Value::as_str),
        Some("1.28.2")
    );
    Ok(())
}

#[test]
fn failing_probe_does_not_write_rp17_passed_json() -> Result<(), Box<dyn Error>> {
    let directory = fixture_dir("rp17_fail")?;
    let said = rp17::judge_and_record(
        &directory,
        &fixture_probe(Some("cudart64_12.dll")),
        &rp17::WINDOWS,
    );
    assert!(
        matches!(said, Err(rp17::JudgeError::Rejected(_))),
        "{said:?}"
    );
    assert!(!directory.join("rp17_passed.json").exists());
    Ok(())
}

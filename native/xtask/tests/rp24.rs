//! RP-24: two builds are the same build when five items of their
//! `build.json` agree. The fixture cases run anywhere; the two real-run cases
//! are `#[ignore]` and read the `build.json` files that `cargo run -p xtask --
//! sherpa-gpu` wrote (run them with `cargo test --release --test rp24 -- --ignored`).

use std::error::Error;
use std::path::Path;

use serde_json::{Value, json};
use xtask::build_compare::{BuildFacts, Mismatch, compare_build_json, sm_120_family};

/// A build with `sm_120a` machine code, so that its verdict is true on its own.
fn base() -> BuildFacts {
    BuildFacts {
        build: json!({
            "sherpa_onnx_commit": "11afbd009a7f8c08f4bcf2fc1b265d0df4670fbf",
            "ort_version": "1.31.0",
            "cuda_version": "13.4.59",
            "cuda_major": 13,
            "cudnn_major": 9,
            "provider_cubin": {"sm_90a": 290, "sm_120a": 279},
            "provider_ptx": {"sm_120": 38},
        }),
        build_json_sha256: String::from("aaaa"),
        rp17_passed: None,
    }
}

/// A build with no `sm_120` family machine code and `sm_120` PTX, as the verdict cases need.
fn ptx_only() -> BuildFacts {
    let mut facts = base();
    facts.build["provider_cubin"] = json!({"sm_90a": 290});
    facts
}

fn recorded(sha256: &str) -> Option<Value> {
    Some(json!({"schema_version": 1, "build_json_sha256": sha256, "probe": {}}))
}

fn set(mut facts: BuildFacts, field: &str, value: Value) -> BuildFacts {
    facts.build[field] = value;
    facts
}

fn named(said: Result<(), Mismatch>, wanted: &'static str) -> bool {
    matches!(said, Err(Mismatch::Field { field, .. }) if field == wanted)
}

#[test]
fn identical_five_fields_pass() {
    assert_eq!(compare_build_json(&base(), &base()), Ok(()));
}

#[test]
fn ort_version_difference_is_named() {
    let other = set(base(), "ort_version", json!("1.28.3"));
    assert!(named(compare_build_json(&base(), &other), "ort_version"));
}

#[test]
fn sherpa_commit_difference_is_named() {
    let other = set(base(), "sherpa_onnx_commit", json!("0000000"));
    assert!(named(
        compare_build_json(&base(), &other),
        "sherpa_onnx_commit"
    ));
}

#[test]
fn cuda_major_difference_is_named() {
    let other = set(base(), "cuda_major", json!(12));
    assert!(named(compare_build_json(&base(), &other), "cuda_major"));
}

#[test]
fn cudnn_major_difference_is_named() {
    let other = set(base(), "cudnn_major", json!(8));
    assert!(named(compare_build_json(&base(), &other), "cudnn_major"));
}

#[test]
fn sm_120_family_verdict_difference_is_named() {
    let mut other = ptx_only();
    other.build["provider_ptx"] = json!({});
    assert!(named(compare_build_json(&base(), &other), "sm_120_family"));
}

#[test]
fn cuda_minor_difference_is_accepted() {
    let other = set(base(), "cuda_version", json!("13.5.1"));
    assert_eq!(compare_build_json(&base(), &other), Ok(()));
}

#[test]
fn zero_sm_120_family_elf_without_rp17_record_is_rejected() {
    assert_eq!(sm_120_family(&ptx_only()), Err(Mismatch::NoRp17Record));
}

#[test]
fn zero_sm_120_family_elf_with_ptx_and_rp17_record_is_accepted() {
    let mut facts = ptx_only();
    facts.rp17_passed = recorded("aaaa");
    assert_eq!(sm_120_family(&facts), Ok(()));
}

#[test]
fn one_sm_120_elf_is_accepted() {
    let mut facts = ptx_only();
    facts.build["provider_cubin"] = json!({"sm_120": 1});
    assert_eq!(sm_120_family(&facts), Ok(()));
}

#[test]
fn sm_120a_only_elf_is_accepted() {
    let mut facts = ptx_only();
    facts.build["provider_cubin"] = json!({"sm_120a": 1});
    facts.build["provider_ptx"] = json!({});
    assert_eq!(sm_120_family(&facts), Ok(()));
}

#[test]
fn rp17_record_for_another_build_is_rejected() {
    let mut facts = ptx_only();
    facts.rp17_passed = recorded("bbbb");
    assert_eq!(
        sm_120_family(&facts),
        Err(Mismatch::Rp17RecordForAnotherBuild {
            recorded: Some(String::from("bbbb")),
            build: String::from("aaaa")
        })
    );
}

#[test]
fn rp17_record_without_sm_120_ptx_is_rejected() {
    let mut facts = ptx_only();
    facts.build["provider_ptx"] = json!({});
    facts.rp17_passed = recorded("aaaa");
    assert_eq!(sm_120_family(&facts), Err(Mismatch::NoSm120Ptx));
}

#[test]
fn other_arch_elf_does_not_count_as_sm_120_family() {
    let mut facts = ptx_only();
    facts.build["provider_cubin"] = json!({"sm_90a": 1, "sm_100a": 1});
    facts.build["provider_ptx"] = json!({});
    assert_eq!(sm_120_family(&facts), Err(Mismatch::NoSm120Ptx));
}

/// `native/target`, next to this crate.
fn target_dir() -> Result<&'static Path, Box<dyn Error>> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| "xtask has no parent directory".into())
}

fn read_build(directory: &str, os: &str) -> Result<BuildFacts, Box<dyn Error>> {
    let native = target_dir()?;
    Ok(BuildFacts::read(
        &native.join("target").join(directory).join(os),
    )?)
}

/// Two builds of the same sources on this operating system: `sherpa-gpu` and `sherpa-gpu-repeat`.
#[test]
#[ignore = "needs two real builds on this operating system"]
fn two_builds_on_one_os_match() -> Result<(), Box<dyn Error>> {
    let os = std::env::consts::OS;
    let (first, second) = (
        read_build("sherpa-gpu", os)?,
        read_build("sherpa-gpu-repeat", os)?,
    );
    compare_build_json(&first, &second)?;
    Ok(())
}

/// The Windows build and the WSL build under `sherpa-gpu`.
#[test]
#[ignore = "needs a Windows build and a WSL build"]
fn windows_and_wsl_builds_match() -> Result<(), Box<dyn Error>> {
    let (windows, linux) = (
        read_build("sherpa-gpu", "windows")?,
        read_build("sherpa-gpu", "linux")?,
    );
    compare_build_json(&windows, &linux)?;
    Ok(())
}

//! RP-21: the manifest of `models fetch` lists the four groups with the version their stage
//! is placed on. The cases here read fixtures. The cases that run the `models fetch` command
//! (T-RP-21-001, -005, -006 and -007) wait for the slice that builds it.

use hayamimi_eval::models_manifest::{Entry, ManifestError, check_entries, check_versions};
use hayamimi_eval::stage_table::StageRow;

fn stage(name: &str, placement: &str) -> StageRow {
    StageRow {
        name: name.to_owned(),
        placement: placement.to_owned(),
        evidence: String::new(),
    }
}

/// The table: the recognition on the CPU (int8), the language ID and the BERT on the GPU.
fn table() -> Vec<StageRow> {
    vec![
        stage("VAD（Silero、sherpa-onnx の `provider`）", "CPU"),
        stage("言語判定（whisper-tiny）", "GPU"),
        stage("認識の encoder（全 model）", "CPU（int8）"),
        stage("句読点（BERT、`ort` の CUDA）", "GPU"),
    ]
}

fn entry(stage: &str, file: &str) -> Entry {
    Entry {
        stage: stage.to_owned(),
        file: file.to_owned(),
        sha256: "0123456789abcdef".repeat(4),
        bytes: 1024,
    }
}

/// The four groups: the recognition, the language ID (two files), the BERT and the Silero VAD.
fn manifest() -> Vec<Entry> {
    vec![
        entry("認識の encoder", "recognizer-encoder.int8.onnx"),
        entry("言語判定", "tiny-encoder.onnx"),
        entry("言語判定", "tiny-decoder.onnx"),
        entry("句読点", "punct_bert.onnx"),
        entry("VAD", "silero_vad.onnx"),
    ]
}

/// T-RP-21-002: every file of the manifest has a digest of 64 hexadecimal digits and a size above 0.
#[test]
fn manifest_entries_have_sha256_and_bytes() {
    assert_eq!(check_entries(&manifest()), Ok(()));
}

/// T-RP-21-003
#[test]
fn sha256_of_63_hex_digits_is_rejected() {
    let mut files = manifest();
    files[1].sha256 = "a".repeat(63);
    assert_eq!(
        check_entries(&files),
        Err(ManifestError::BadSha256 {
            file: String::from("tiny-encoder.onnx")
        })
    );
}

/// T-RP-21-004
#[test]
fn zero_bytes_entry_is_rejected() {
    let mut files = manifest();
    files[3].bytes = 0;
    assert_eq!(
        check_entries(&files),
        Err(ManifestError::NoBytes {
            file: String::from("punct_bert.onnx")
        })
    );
}

/// T-RP-21-008: the recognition is int8, the GPU stages are not.
#[test]
fn manifest_versions_match_the_stage_placements() {
    assert_eq!(check_versions(&table(), &manifest()), Ok(()));
}

/// T-RP-21-009
#[test]
fn fp32_recognizer_in_the_manifest_is_rejected() {
    let mut files = manifest();
    files[0].file = String::from("recognizer-encoder.onnx");
    let said = check_versions(&table(), &files);
    assert!(matches!(
        said,
        Err(ManifestError::WrongPrecision { stage, file, .. })
            if stage == "認識の encoder" && file == "recognizer-encoder.onnx"
    ));
}

/// T-RP-21-010: CUDA has no op for an int8 model.
#[test]
fn int8_model_for_a_gpu_stage_is_rejected() {
    let mut files = manifest();
    files[1].file = String::from("tiny-encoder.int8.onnx");
    let said = check_versions(&table(), &files);
    assert!(matches!(
        said,
        Err(ManifestError::WrongPrecision { stage, file, .. })
            if stage == "言語判定" && file == "tiny-encoder.int8.onnx"
    ));
}

/// T-RP-21-011: fp16 is a precision CUDA has ops for.
#[test]
fn fp16_model_for_a_gpu_stage_passes() {
    let mut files = manifest();
    files[3].file = String::from("punct_bert.fp16.onnx");
    assert_eq!(check_versions(&table(), &files), Ok(()));
}

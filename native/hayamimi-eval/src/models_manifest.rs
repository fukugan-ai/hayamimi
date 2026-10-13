//! RP-21: the manifest that `models fetch` writes, and whether the precision of each
//! file it lists is the one the stage table places the stage on.
//!
//! A precision shows in the file name: `.int8.onnx` is int8, `.fp16.onnx` is fp16 and a
//! name with neither is fp32. A stage the table places on the CPU as `CPU（int8）` reads
//! the int8 file, and a stage placed on the GPU never reads an int8 file (CUDA has no op
//! for it). Any other stage has no precision to match.

use std::fmt;

use crate::stage_table::StageRow;

/// The file-name ending of an int8 model.
const INT8_ENDING: &str = ".int8.onnx";

/// The length of a SHA-256 digest written in hexadecimal.
const SHA256_HEX_LEN: usize = 64;

/// One file of the manifest, with the stage that reads it (the start of the stage name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub stage: String,
    pub file: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ManifestError {
    /// The digest of the file is not 64 hexadecimal digits.
    BadSha256 { file: String },
    /// The file has no bytes.
    NoBytes { file: String },
    /// The precision of the file is not the one the placement of its stage asks for.
    WrongPrecision {
        stage: String,
        file: String,
        wanted: &'static str,
    },
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ManifestError::BadSha256 { file } => {
                write!(f, "`{file}`: the sha256 is not 64 hexadecimal digits")
            }
            ManifestError::NoBytes { file } => write!(f, "`{file}`: the size is not above 0 bytes"),
            ManifestError::WrongPrecision {
                stage,
                file,
                wanted,
            } => {
                write!(f, "stage `{stage}`: `{file}` is not {wanted}")
            }
        }
    }
}

impl std::error::Error for ManifestError {}

/// Every file has a 64-digit hexadecimal sha256 and a size above 0.
pub fn check_entries(entries: &[Entry]) -> Result<(), ManifestError> {
    for entry in entries {
        let hex = entry.sha256.len() == SHA256_HEX_LEN
            && entry.sha256.chars().all(|c| c.is_ascii_hexdigit());
        if !hex {
            return Err(ManifestError::BadSha256 {
                file: entry.file.clone(),
            });
        }
        if entry.bytes == 0 {
            return Err(ManifestError::NoBytes {
                file: entry.file.clone(),
            });
        }
    }
    Ok(())
}

/// The precision the placement of a stage asks for, if it asks for one: the words that
/// name it and whether the file must be int8.
fn wanted(placement: &str) -> Option<(&'static str, bool)> {
    if placement.contains("int8") {
        Some(("an int8 file", true))
    } else if placement.starts_with("GPU") {
        Some(("a file of a precision CUDA has ops for (not int8)", false))
    } else {
        None
    }
}

/// Every file is of the precision the table places its stage on.
pub fn check_versions(table: &[StageRow], entries: &[Entry]) -> Result<(), ManifestError> {
    for entry in entries {
        let Some(row) = table.iter().find(|row| row.name.starts_with(&entry.stage)) else {
            continue;
        };
        let Some((words, int8)) = wanted(&row.placement) else {
            continue;
        };
        if entry.file.ends_with(INT8_ENDING) != int8 {
            return Err(ManifestError::WrongPrecision {
                stage: entry.stage.clone(),
                file: entry.file.clone(),
                wanted: words,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(stage: &str, file: &str) -> Entry {
        Entry {
            stage: stage.to_owned(),
            file: file.to_owned(),
            sha256: "a".repeat(SHA256_HEX_LEN),
            bytes: 1,
        }
    }

    fn row(name: &str, placement: &str) -> StageRow {
        StageRow {
            name: name.to_owned(),
            placement: placement.to_owned(),
            evidence: String::new(),
        }
    }

    #[test]
    fn an_empty_manifest_has_no_bad_entry() {
        assert_eq!(check_entries(&[]), Ok(()));
        assert_eq!(check_versions(&[], &[]), Ok(()));
    }

    #[test]
    fn an_upper_case_digest_is_a_digest() {
        let mut upper = entry("VAD", "silero_vad.onnx");
        upper.sha256 = "A".repeat(SHA256_HEX_LEN);
        assert_eq!(check_entries(&[upper]), Ok(()));
    }

    #[test]
    fn a_digest_of_65_digits_or_a_non_hex_digit_is_refused() {
        let mut long = entry("VAD", "silero_vad.onnx");
        long.sha256 = "a".repeat(SHA256_HEX_LEN + 1);
        let mut odd = entry("VAD", "silero_vad.onnx");
        odd.sha256 = format!("g{}", "a".repeat(SHA256_HEX_LEN - 1));
        for bad in [long, odd] {
            assert!(matches!(
                check_entries(&[bad]),
                Err(ManifestError::BadSha256 { .. })
            ));
        }
    }

    #[test]
    fn a_stage_that_asks_for_no_precision_takes_any_file() {
        let table = [row("VAD（Silero）", "CPU")];
        let int8 = [entry("VAD", "silero_vad.int8.onnx")];
        assert_eq!(check_versions(&table, &int8), Ok(()));
    }

    #[test]
    fn a_file_whose_stage_is_not_in_the_table_is_not_judged() {
        let table = [row("VAD（Silero）", "CPU")];
        let other = [entry("話者", "campplus.int8.onnx")];
        assert_eq!(check_versions(&table, &other), Ok(()));
    }
}

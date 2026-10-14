//! `manifest.json`: the record of what is in the models directory, one entry
//! for each file with the stage that reads it, its sha256 and its size. It is
//! the record, not a gate: a file that is there but not listed is hashed when
//! the next run looks at it, and a changed record is the only thing written.

use std::fs;
use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::error::FetchError;
use super::items::Item;

/// The name of the record inside the models directory.
pub const FILE_NAME: &str = "manifest.json";

/// Block size of a read for hashing.
const HASH_BLOCK: usize = 1 << 20;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub stage: String,
    /// The path under the models directory, with `/`.
    pub file: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Serialize, Deserialize)]
struct Record {
    files: Vec<Entry>,
}

/// The entries of the record in `models`; none when it is not there or cannot be read.
pub fn read(models: &Path) -> Vec<Entry> {
    let parsed = fs::read(models.join(FILE_NAME))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Record>(&bytes).ok());
    match parsed {
        Some(record) => record.files,
        // DEFAULT: a record that is absent or damaged lists no file; every file on disk is hashed again and the record is rewritten whole.
        None => Vec::new(),
    }
}

/// Lower-case hexadecimal digits of `bytes`.
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        // Writing to a String cannot fail.
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// The sha256 of a file as 64 lower-case hexadecimal digits, and its size.
pub fn hash_file(path: &Path) -> Result<(String, u64), FetchError> {
    let mut file = fs::File::open(path).map_err(|e| FetchError::io(path, e))?;
    let mut hasher = Sha256::new();
    let mut block = vec![0_u8; HASH_BLOCK];
    let mut total = 0_u64;
    loop {
        let read = file.read(&mut block).map_err(|e| FetchError::io(path, e))?;
        if read == 0 {
            return Ok((hex(&hasher.finalize()), total));
        }
        hasher.update(&block[..read]);
        // A read count is at most the block size, which fits in 64 bits on every target.
        total += read as u64;
    }
}

/// The entry of `name` in `item`'s directory: the old one when its size still
/// matches the file, otherwise the file hashed again.
fn entry_for(models: &Path, item: &Item, name: &str, old: &[Entry]) -> Result<Entry, FetchError> {
    let relative = item.relative(name);
    let path = models.join(&relative);
    let size = fs::metadata(&path)
        .map_err(|e| FetchError::io(&path, e))?
        .len();
    if let Some(known) = old
        .iter()
        .find(|entry| entry.file == relative && entry.bytes == size)
    {
        return Ok(known.clone());
    }
    let (sha256, bytes) = hash_file(&path)?;
    Ok(Entry {
        stage: item.stage.to_owned(),
        file: relative,
        sha256,
        bytes,
    })
}

/// The entries of every file of a group that is in `models`.
pub fn entries_of(
    models: &Path,
    item: &Item,
    names: &[String],
    old: &[Entry],
) -> Result<Vec<Entry>, FetchError> {
    names
        .iter()
        .map(|name| entry_for(models, item, name, old))
        .collect()
}

/// `entries` with the files of `stage` replaced by `group`, in a fixed order.
pub fn replace_group(mut entries: Vec<Entry>, stage: &str, group: Vec<Entry>) -> Vec<Entry> {
    entries.retain(|entry| entry.stage != stage);
    entries.extend(group);
    entries.sort_by(|a, b| (&a.stage, &a.file).cmp(&(&b.stage, &b.file)));
    entries
}

/// The text of the record.
fn render(entries: Vec<Entry>) -> Result<String, FetchError> {
    let mut text = serde_json::to_string_pretty(&Record { files: entries }).map_err(|e| {
        FetchError::Archive {
            url: String::from(FILE_NAME),
            message: e.to_string(),
        }
    })?;
    text.push('\n');
    Ok(text)
}

/// Writes the record when it differs from the one on disk; returns whether it did.
/// The new text goes to a file beside it and is renamed into place.
pub fn write_if_changed(models: &Path, entries: Vec<Entry>) -> Result<bool, FetchError> {
    let path = models.join(FILE_NAME);
    if entries.is_empty() && !path.exists() {
        // A record of no file is not made: a directory that got nothing stays as it was.
        return Ok(false);
    }
    let text = render(entries)?;
    if fs::read(&path).is_ok_and(|current| current == text.as_bytes()) {
        return Ok(false);
    }
    let beside = models.join(format!(".{FILE_NAME}.part"));
    fs::write(&beside, text).map_err(|e| FetchError::io(&beside, e))?;
    fs::rename(&beside, &path).map_err(|e| FetchError::io(&path, e))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models_fetch::items::MINIMAL;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn scratch(name: &str) -> Result<std::path::PathBuf, std::io::Error> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../target/unit-tmp")
            .join(format!("manifest-{name}-{}", std::process::id()));
        if root.exists() {
            fs::remove_dir_all(&root)?;
        }
        fs::create_dir_all(&root)?;
        Ok(root)
    }

    fn entry(stage: &str, file: &str) -> Entry {
        Entry {
            stage: stage.to_owned(),
            file: file.to_owned(),
            sha256: "a".repeat(64),
            bytes: 1,
        }
    }

    #[test]
    fn the_hash_of_abc_is_the_known_digest() -> TestResult {
        let root = scratch("abc")?;
        fs::write(root.join("f"), b"abc")?;
        let (sha256, bytes) = hash_file(&root.join("f"))?;
        assert_eq!(
            sha256,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(bytes, 3);
        fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn an_empty_file_hashes_to_the_digest_of_nothing_with_zero_bytes() -> TestResult {
        let root = scratch("empty")?;
        fs::write(root.join("f"), b"")?;
        let (sha256, bytes) = hash_file(&root.join("f"))?;
        assert_eq!(
            sha256,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(bytes, 0);
        fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn a_missing_or_damaged_record_lists_nothing() -> TestResult {
        let root = scratch("damaged")?;
        assert!(read(&root).is_empty());
        fs::write(root.join(FILE_NAME), b"{not json")?;
        assert!(read(&root).is_empty());
        fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn replacing_a_group_keeps_the_others_in_a_fixed_order() {
        let before = vec![entry("vad", "silero_vad.onnx"), entry("recognizer", "b")];
        let after = replace_group(before, "recognizer", vec![entry("recognizer", "a")]);
        let order: Vec<(&str, &str)> = after
            .iter()
            .map(|e| (e.stage.as_str(), e.file.as_str()))
            .collect();
        assert_eq!(order, [("recognizer", "a"), ("vad", "silero_vad.onnx")]);
    }

    #[test]
    fn the_record_is_written_only_when_it_changes() -> TestResult {
        let root = scratch("write")?;
        let entries = vec![entry("vad", "silero_vad.onnx")];
        assert!(write_if_changed(&root, entries.clone())?);
        let first = fs::metadata(root.join(FILE_NAME))?.modified()?;
        assert!(!write_if_changed(&root, entries.clone())?);
        assert_eq!(fs::metadata(root.join(FILE_NAME))?.modified()?, first);
        assert_eq!(read(&root), entries);
        assert!(write_if_changed(&root, vec![entry("vad", "other")])?);
        assert!(!root.join(format!(".{FILE_NAME}.part")).exists());
        fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn an_old_entry_of_the_same_size_is_reused_and_a_changed_size_is_hashed_again() -> TestResult {
        let root = scratch("reuse")?;
        let item = &MINIMAL[2];
        fs::write(root.join("silero_vad.onnx"), b"abc")?;
        let mut known = entry("vad", "silero_vad.onnx");
        known.bytes = 3;
        let names = [String::from("silero_vad.onnx")];
        let reused = entries_of(&root, item, &names, std::slice::from_ref(&known))?;
        assert_eq!(reused, [known.clone()]);
        known.bytes = 4;
        let fresh = entries_of(&root, item, &names, &[known])?;
        assert_eq!(fresh[0].bytes, 3);
        assert_eq!(fresh[0].sha256.len(), 64);
        fs::remove_dir_all(&root)?;
        Ok(())
    }
}

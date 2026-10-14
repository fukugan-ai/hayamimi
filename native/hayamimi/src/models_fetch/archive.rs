//! Reading the files a group needs out of a release `.tar.bz2` while it is
//! still downloading. A release archive holds a whole model directory (test
//! wavs, other precisions); only the named files are written to the stage, by
//! their base name, and the read stops as soon as the last one has been seen.

use std::io::Read;

use bzip2::read::BzDecoder;

use super::error::FetchError;
use super::stage::Stage;

fn archive_error(url: &str, error: std::io::Error) -> FetchError {
    FetchError::Archive {
        url: url.to_owned(),
        message: error.to_string(),
    }
}

/// The wanted name that `entry` is a regular file of, by base name.
fn wanted_name<'a, R: Read>(entry: &tar::Entry<'_, R>, wanted: &[&'a str]) -> Option<&'a str> {
    if !entry.header().entry_type().is_file() {
        return None;
    }
    let path = entry.path().ok()?;
    let base = path.file_name()?.to_str()?;
    wanted.iter().copied().find(|name| *name == base)
}

/// The wanted names that were not found.
fn missing(wanted: &[&str], found: &[&str]) -> Vec<String> {
    wanted
        .iter()
        .filter(|name| !found.contains(name))
        .map(|name| (*name).to_owned())
        .collect()
}

/// Writes the `wanted` files of the `.tar.bz2` in `stream` into `stage`; the
/// first file of a name wins. Fails with the names that the archive lacks.
pub fn extract(
    stream: impl Read,
    url: &str,
    wanted: &[&str],
    stage: &Stage,
) -> Result<(), FetchError> {
    let mut archive = tar::Archive::new(BzDecoder::new(stream));
    let entries = archive.entries().map_err(|e| archive_error(url, e))?;
    let mut found: Vec<&str> = Vec::with_capacity(wanted.len());
    for entry in entries {
        let mut entry = entry.map_err(|e| archive_error(url, e))?;
        let Some(name) = wanted_name(&entry, wanted) else {
            continue;
        };
        if found.contains(&name) {
            continue;
        }
        stage.write(name, &mut entry)?;
        found.push(name);
        if found.len() == wanted.len() {
            return Ok(());
        }
    }
    Err(FetchError::MissingMember {
        names: missing(wanted, &found),
    })
}

/// A `.tar.bz2` of `files` (path inside the archive, content), for the tests.
#[cfg(test)]
pub(super) fn tar_bz2(files: &[(&str, &[u8])]) -> Result<Vec<u8>, std::io::Error> {
    let encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::fast());
    let mut builder = tar::Builder::new(encoder);
    for (path, content) in files {
        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        builder.append_data(&mut header, path, *content)?;
    }
    builder.into_inner()?.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn scratch(name: &str) -> Result<PathBuf, std::io::Error> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../target/unit-tmp")
            .join(format!("archive-{name}-{}", std::process::id()));
        if root.exists() {
            fs::remove_dir_all(&root)?;
        }
        fs::create_dir_all(&root)?;
        Ok(root)
    }

    #[test]
    fn only_the_wanted_files_are_written_by_their_base_name() -> TestResult {
        let root = scratch("wanted")?;
        let bytes = tar_bz2(&[
            ("model/a.onnx", b"AA"),
            ("model/test_wavs/0.wav", b"WW"),
            ("model/b.txt", b"B"),
        ])?;
        let stage = Stage::create(&root, "g")?;
        extract(&bytes[..], "u", &["a.onnx", "b.txt"], &stage)?;
        let dest = root.join("g");
        stage.install(&dest, &[String::from("a.onnx"), String::from("b.txt")])?;
        assert_eq!(fs::read(dest.join("a.onnx"))?, b"AA");
        assert_eq!(fs::read(dest.join("b.txt"))?, b"B");
        assert_eq!(fs::read_dir(&dest)?.count(), 2);
        fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn a_file_the_archive_lacks_is_named() -> TestResult {
        let root = scratch("missing")?;
        let bytes = tar_bz2(&[("m/a.onnx", b"AA")])?;
        let stage = Stage::create(&root, "g")?;
        let result = extract(&bytes[..], "u", &["a.onnx", "tokens.txt"], &stage);
        assert!(
            matches!(&result, Err(FetchError::MissingMember { names }) if names == &["tokens.txt"])
        );
        fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn a_name_that_is_only_the_end_of_another_name_is_not_taken() -> TestResult {
        let root = scratch("suffix")?;
        let bytes = tar_bz2(&[("m/encoder.int8.onnx", b"I"), ("m/encoder.onnx", b"F")])?;
        let stage = Stage::create(&root, "g")?;
        extract(&bytes[..], "u", &["encoder.onnx"], &stage)?;
        let dest = root.join("g");
        stage.install(&dest, &[String::from("encoder.onnx")])?;
        assert_eq!(fs::read(dest.join("encoder.onnx"))?, b"F");
        fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn the_first_of_two_files_with_one_name_wins() -> TestResult {
        let root = scratch("first")?;
        let bytes = tar_bz2(&[("m/t.txt", b"first"), ("n/t.txt", b"second")])?;
        let stage = Stage::create(&root, "g")?;
        extract(&bytes[..], "u", &["t.txt"], &stage)?;
        let dest = root.join("g");
        stage.install(&dest, &[String::from("t.txt")])?;
        assert_eq!(fs::read(dest.join("t.txt"))?, b"first");
        fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn bytes_that_are_not_an_archive_are_a_bad_archive() -> TestResult {
        let root = scratch("garbage")?;
        let stage = Stage::create(&root, "g")?;
        let result = extract(&b"not bzip2 at all"[..], "u", &["a"], &stage);
        assert!(matches!(result, Err(FetchError::Archive { .. })));
        fs::remove_dir_all(&root)?;
        Ok(())
    }
}

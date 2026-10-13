//! SHA-256 of bytes and of files, as lowercase hex.

use std::fmt::Write as _;
use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::error::EvalError;

/// Block size of the streaming file read.
const READ_BLOCK: usize = 1 << 20;

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        // Writing to a String cannot fail.
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// The SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// The SHA-256 of the file at `path`, read in blocks.
pub fn sha256_file(path: &Path) -> Result<String, EvalError> {
    let mut file = std::fs::File::open(path).map_err(|e| EvalError::io(path, e))?;
    let mut hasher = Sha256::new();
    let mut block = vec![0_u8; READ_BLOCK];
    loop {
        let read = file.read(&mut block).map_err(|e| EvalError::io(path, e))?;
        if read == 0 {
            break;
        }
        hasher.update(&block[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_standard_vector_for_abc() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn the_empty_input_has_the_standard_digest() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn a_missing_file_names_its_path() {
        let missing = Path::new("no/such/file.wav");
        let said = sha256_file(missing).err().map(|e| e.to_string());
        assert!(
            said.as_deref()
                .is_some_and(|t| t.starts_with("no/such/file.wav")),
            "{said:?}"
        );
    }
}

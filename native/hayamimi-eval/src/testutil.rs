//! Helpers of the unit tests. Scratch files live under the build directory,
//! never under the system temporary directory.

use std::path::PathBuf;

/// A path for one scratch file or directory, unique to this process.
pub(crate) fn scratch(name: &str) -> PathBuf {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("target")
        .join("eval-test-tmp");
    // A directory that cannot be made surfaces as the error of the first write.
    let _ = std::fs::create_dir_all(&dir);
    dir.join(format!("{}-{name}", std::process::id()))
}

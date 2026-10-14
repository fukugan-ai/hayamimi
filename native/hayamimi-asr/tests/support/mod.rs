//! Helpers shared by the acceptance tests of this package.
//!
//! Each test binary uses its own subset.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

/// What a `check_*` function returns for a wrong input: a message that
/// names what is wrong.
pub type Failure = String;

/// The return type of every test.
pub type TestResult = Result<(), Box<dyn std::error::Error>>;

pub const REPO: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
pub const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/golden/ja");
pub const PARITY: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../mobile/hayamimi_core/test/fixtures/punct_ja_parity.json"
);
/// The workspace's own `target` directory; fixture trees are made under it.
pub const TARGET: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../target");

/// The directory above the repository checkout that holds `models/`: the
/// checkout itself, or the checkout a worktree hangs from. Model files are
/// not in git, so a worktree reads them from the main checkout.
pub fn models_dir() -> Result<PathBuf, Failure> {
    let start = Path::new(REPO)
        .canonicalize()
        .map_err(|error| format!("cannot resolve {REPO}: {error}"))?;
    start
        .ancestors()
        .map(|directory| directory.join("models"))
        .find(|models| models.join("silero_vad.onnx").is_file())
        .ok_or_else(|| format!("no models/silero_vad.onnx at or above {}", start.display()))
}

pub fn read(path: &Path) -> Result<String, Failure> {
    std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))
}

pub fn read_json(path: &Path) -> Result<serde_json::Value, Failure> {
    let text = read(path)?;
    serde_json::from_str(&text).map_err(|error| format!("{} is not JSON: {error}", path.display()))
}

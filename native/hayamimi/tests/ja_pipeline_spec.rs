//! `docs/spec/ja_pipeline_spec.json` is made by `hayamimi config dump` and has
//! to keep describing what the program says.
//!
//! Two halves, because they need different things on disk. The configuration
//! half loads no model and reads no file under `models/`, so it runs on a
//! checkout that has neither `models/` nor `testdata/`, CI included. The model
//! half (size and sha256 of each file) can be checked only where `models/` is
//! populated, so no test here compares it.
//!
//! After an intentional change, regenerate the document from `native/`:
//!
//! ```text
//! cargo run --release -p hayamimi -- config dump --with-models --models ../models \
//!     --out ../docs/spec/ja_pipeline_spec.json
//! ```

use std::error::Error;
use std::process::Command;

use serde_json::{Value, json};

type TestResult = Result<(), Box<dyn Error>>;

const SPEC: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/spec/ja_pipeline_spec.json"
);
const NO_MODELS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/no_such_models_directory");
const BINARY: &str = env!("CARGO_BIN_EXE_hayamimi");

/// The committed document.
fn committed() -> Result<Value, Box<dyn Error>> {
    Ok(serde_json::from_str(&std::fs::read_to_string(SPEC)?)?)
}

/// The document `hayamimi` prints for these arguments.
fn dump(args: &[&str]) -> Result<Value, Box<dyn Error>> {
    let output = Command::new(BINARY).args(args).output()?;
    if !output.status.success() {
        return Err(format!("hayamimi {args:?} exited with {:?}", output.status.code()).into());
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

#[test]
fn spec_file_exists_and_declares_its_generator() -> TestResult {
    let spec = committed()?;
    assert_eq!(spec["generated_by"], "hayamimi config dump");
    assert_eq!(spec["schema_version"], 1);
    assert_eq!(spec["describes"], "docs/spec/ja_pipeline.md");
    assert!(
        spec["generated_at"]
            .as_str()
            .is_some_and(|stamp| stamp.ends_with('Z')),
        "generated_at is not a UTC time: {}",
        spec["generated_at"]
    );
    Ok(())
}

/// The whole point: the `config` block of the committed document is what the
/// program says now, not what it said when someone wrote it down.
#[test]
fn config_matches_the_live_dump() -> TestResult {
    let live = dump(&["config", "dump"])?;
    assert_eq!(
        live["config"],
        committed()?["config"],
        "docs/spec/ja_pipeline_spec.json is stale; regenerate it (see the head of this file) \
         and update docs/spec/ja_pipeline.ja.md and docs/spec/ja_pipeline.md to match"
    );
    Ok(())
}

/// The dump has to work on a checkout with no `models/`: that is what makes
/// the check above runnable in CI at all.
#[test]
fn dump_needs_no_models_directory() -> TestResult {
    let live = dump(&["config", "dump", "--with-models", "--models", NO_MODELS])?;
    assert_eq!(live["models"], json!({}));
    assert_eq!(live["config"], committed()?["config"]);
    Ok(())
}

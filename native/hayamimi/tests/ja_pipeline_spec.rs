//! `docs/spec/ja_pipeline_spec.json` is the machine-readable form of the
//! Japanese route's configuration. These tests keep it a document that
//! declares where it came from and what it holds.

use serde_json::Value;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const SPEC: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/spec/ja_pipeline_spec.json"
);

fn spec() -> Result<Value, Box<dyn std::error::Error>> {
    Ok(serde_json::from_str(&std::fs::read_to_string(SPEC)?)?)
}

fn text_of<'a>(spec: &'a Value, key: &str) -> Result<&'a str, Box<dyn std::error::Error>> {
    spec.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("the spec has no text `{key}`").into())
}

#[test]
fn spec_file_exists_and_declares_its_generator() -> TestResult {
    let spec = spec()?;
    assert!(!text_of(&spec, "generated_by")?.is_empty());
    assert_eq!(spec.get("schema_version").and_then(Value::as_u64), Some(1));
    assert!(text_of(&spec, "generated_at")?.ends_with('Z'));
    assert!(spec.get("config").is_some_and(Value::is_object));
    Ok(())
}

//! The English route of the binary: an English clip is read by the English
//! recognizer, with the language chosen by the route or forced by `--lang en`,
//! and every `final` on `/events` says so (`lang`, `tier`, a Latin text).

mod support;

use serde_json::Value;
use support::{TestResult, find_above, models_dir, serve};

const CLIP: &str = "testdata/eval_real/en_01.wav";

/// The `final` messages a run served.
fn finals(wire: &[Value]) -> Vec<&Value> {
    wire.iter()
        .filter(|event| event.get("type").and_then(Value::as_str) == Some("final"))
        .collect()
}

fn text_of<'a>(event: &'a Value, key: &str) -> Option<&'a str> {
    event.get(key).and_then(Value::as_str)
}

/// Plays the English clip through the binary with `extra` arguments.
fn english_run(extra: &[&str]) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    let clip = find_above(CLIP)?;
    let models = models_dir()?;
    let clip = clip.to_str().ok_or("the clip path is not UTF-8")?;
    let models = models.to_str().ok_or("the models path is not UTF-8")?;
    let mut args = vec!["--wav", clip, "--no-realtime", "--models", models];
    args.extend(extra);
    let served = serve(&args, None, &[])?;
    assert_eq!(served.output.code, Some(0), "{}", served.output.stderr);
    Ok(served.wire)
}

/// Every final names English, comes from the English tier and reads as words.
fn assert_english(wire: &[Value]) {
    let all = finals(wire);
    assert!(!all.is_empty(), "no final came out");
    for event in all {
        assert_eq!(text_of(event, "lang"), Some("en"), "{event}");
        assert_eq!(text_of(event, "tier"), Some("v3"), "{event}");
        let words = text_of(event, "text").map(|text| text.split_whitespace().count());
        assert!(words.is_some_and(|count| count >= 2), "{event}");
    }
}

/// The route detects English and reads it in the English tier.
#[test]
#[ignore = "runs the hayamimi binary on an English clip"]
fn english_clip_is_routed_to_the_english_recognizer() -> TestResult {
    assert_english(&english_run(&[])?);
    Ok(())
}

/// `--lang en` reads the clip in English with no detection.
#[test]
#[ignore = "runs the hayamimi binary on an English clip"]
fn lang_en_reads_the_clip_in_english() -> TestResult {
    let wire = english_run(&["--lang", "en"])?;
    assert_english(&wire);
    let detected = finals(&wire)
        .iter()
        .all(|event| event.get("lid_ms").and_then(Value::as_f64) == Some(0.0));
    assert!(detected, "a forced language still ran the identifier");
    Ok(())
}

/// The negative control: a Japanese clip is not read as English.
#[test]
#[ignore = "runs the hayamimi binary on a Japanese clip"]
fn japanese_clip_is_not_read_in_english() -> TestResult {
    let clip = find_above("testdata/eval_real/ja_01.wav")?;
    let models = models_dir()?;
    let clip = clip.to_str().ok_or("the clip path is not UTF-8")?;
    let models = models.to_str().ok_or("the models path is not UTF-8")?;
    let args = ["--wav", clip, "--no-realtime", "--models", models];
    let wire = serve(&args, None, &[])?.wire;
    let all = finals(&wire);
    assert!(!all.is_empty(), "no final came out");
    assert!(all.iter().all(|event| text_of(event, "lang") != Some("en")));
    Ok(())
}

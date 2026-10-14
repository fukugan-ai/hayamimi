//! `--source` in a real run: a value that is not a source does not stop the
//! run. The run says `config.fallback.default` (stage `source`, reason
//! `bad_value`) once in the log and once on `/events`, and goes on with the
//! source of its input. A valid value reaches the `source` of every `final`.
#![allow(dead_code)]

mod support;

use serde_json::Value;
use support::{Served, TestResult, clip, models_dir, path_text, serve};

fn run_with_source(value: &str) -> Result<Served, support::Failure> {
    let wav = path_text(&clip())?;
    let models = path_text(&models_dir()?)?;
    let args = [
        "--wav",
        wav.as_str(),
        "--no-realtime",
        "--models",
        models.as_str(),
        "--source",
        value,
    ];
    serve(&args, Some("C.UTF-8"), &[])
}

fn text<'a>(event: &'a Value, key: &str) -> Option<&'a str> {
    event.get(key).and_then(Value::as_str)
}

/// The `/events` fallbacks that name the source stage.
fn source_fallbacks(served: &Served) -> usize {
    served
        .wire
        .iter()
        .filter(|event| {
            text(event, "type") == Some("fallback")
                && text(event, "stage") == Some("source")
                && text(event, "reason") == Some("bad_value")
        })
        .count()
}

/// The log lines of the same fallback on stdout.
fn source_log_lines(served: &Served) -> usize {
    served
        .output
        .stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|line| {
            text(line, "event") == Some("config.fallback.default")
                && text(line, "stage") == Some("source")
                && text(line, "reason") == Some("bad_value")
        })
        .count()
}

fn final_sources(served: &Served) -> Vec<&str> {
    served
        .wire
        .iter()
        .filter(|event| text(event, "type") == Some("final"))
        .filter_map(|event| text(event, "source"))
        .collect()
}

/// A value that is not a source: the run finishes, the fallback is said once
/// on each channel, and the finals carry the source of the wav input.
#[test]
#[ignore = "runs the hayamimi binary"]
fn source_line_run_says_the_fallback_once_and_goes_on() -> TestResult {
    let served = run_with_source("line")?;
    assert_eq!(served.output.code, Some(0));
    assert_eq!(source_fallbacks(&served), 1, "events: {:?}", served.wire);
    assert_eq!(source_log_lines(&served), 1, "{}", served.output.stdout);
    let sources = final_sources(&served);
    assert!(!sources.is_empty(), "no final came out");
    assert!(
        sources.iter().all(|source| *source == "file"),
        "{sources:?}"
    );
    Ok(())
}

/// A source: no fallback is said, and the finals carry the value given.
#[test]
#[ignore = "runs the hayamimi binary"]
fn source_loopback_run_names_it_on_every_final_and_says_no_fallback() -> TestResult {
    let served = run_with_source("loopback")?;
    assert_eq!(served.output.code, Some(0));
    assert_eq!(source_fallbacks(&served), 0);
    assert_eq!(source_log_lines(&served), 0);
    let sources = final_sources(&served);
    assert!(!sources.is_empty(), "no final came out");
    assert!(
        sources.iter().all(|source| *source == "loopback"),
        "{sources:?}"
    );
    Ok(())
}

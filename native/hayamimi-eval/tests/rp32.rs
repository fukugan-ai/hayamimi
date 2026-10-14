//! RP-32: the share of valid samples that got a translation.
//!
//! A run whose translations the checker stops leaves those samples out of
//! `L`, so a small `L` over the rest proves nothing. The share is counted over
//! the valid samples (3.1: a `final` is there and `L_asr - L_dec` reaches the
//! run's `--min-silence`), with the same validity function RP-3 uses, and it
//! must reach 20 / 23.

use hayamimi_eval::compare::{check_translated_share, translated_share};
use hayamimi_eval::latency::{Sample, Window, build_samples, parse_event};
use serde_json::Value;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const R8E: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/results/r8e");
/// The `--min-silence` of the recorded run, in milliseconds.
const MIN_SILENCE_MS: i64 = 300;
const REFERENCES: usize = 64;

fn read_lines(name: &str) -> Result<Vec<Value>, String> {
    let path = std::path::Path::new(R8E).join(name);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).map_err(|e| format!("{}: {e}", path.display())))
        .collect()
}

fn int_of(row: &Value, key: &str) -> Result<i64, String> {
    row.get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("a row has no integer `{key}`: {row}"))
}

/// The recorded Rust run counted the way RP-31 says.
fn recorded_samples() -> Result<Vec<Sample>, String> {
    let mut windows = Vec::new();
    for row in read_lines("rp3_rust.jsonl")? {
        let narrow =
            |key: &str| u32::try_from(int_of(&row, key)?).map_err(|e| format!("{key}: {e}"));
        windows.push(Window {
            repeat: narrow("repeat")?,
            utt: narrow("utt")?,
            t_start_us: int_of(&row, "t_start_us")?,
            t_end_us: int_of(&row, "t_end_us")?,
        });
    }
    let mut events = Vec::new();
    for row in read_lines("rp3_rust.events.jsonl")? {
        let Some(event) = row.get("event") else {
            return Err(format!("a row has no `event`: {row}"));
        };
        events.push(
            parse_event(int_of(&row, "t_us")?, &event.to_string()).map_err(|e| e.to_string())?,
        );
    }
    let references = vec![String::new(); REFERENCES];
    build_samples(&windows, &references, &events).map_err(|e| e.to_string())
}

#[test]
fn rp32_the_translated_share_of_valid_samples_reaches_20_of_23() -> TestResult {
    let samples = recorded_samples()?;
    // A run that translated every valid sample passes.
    let all: Vec<Sample> = samples
        .iter()
        .cloned()
        .map(|sample| Sample {
            t_tr_us: sample.t_final_us.map(|t| t + 100_000),
            ..sample
        })
        .collect();
    assert!(check_translated_share(&all, MIN_SILENCE_MS).is_ok());
    Ok(())
}

#[test]
fn the_recorded_r8e_run_is_below_20_of_23() -> TestResult {
    let samples = recorded_samples()?;
    let share = translated_share(&samples, MIN_SILENCE_MS);
    // 100 valid samples of the 115 (one of the 101 with a `final` has a wrong end), 66 translated.
    assert_eq!((share.valid, share.translated), (100, 66));
    let said = check_translated_share(&samples, MIN_SILENCE_MS);
    assert!(
        matches!(&said, Err(m) if m.to_string().contains("66 of 100")),
        "{said:?}"
    );
    Ok(())
}

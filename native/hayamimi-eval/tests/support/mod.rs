//! Helpers shared by the R0 acceptance tests.
//!
//! The RB tests read result files that the R0 runs left behind. Each test
//! binary uses its own subset of these helpers.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use sha2::{Digest, Sha256};

pub mod der_logs;
pub mod e2e;
pub mod rp32;
pub mod rp33;
pub mod stages;

/// What a `check_*` function returns for a wrong input: a message that
/// names what is wrong.
pub type Failure = String;

/// The return type of every test.
pub type TestResult = Result<(), Box<dyn std::error::Error>>;

pub const REPO: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
pub const RESULTS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/results");
pub const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/golden/ja");
pub const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

/// The `testdata/` of this checkout, which `git` does not carry.
const OWN_TESTDATA: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata");

/// The `testdata/` of the main checkout: the parent of git's common
/// directory, which a worktree shares with the checkout it hangs from.
fn main_checkout_testdata() -> Result<PathBuf, Failure> {
    let output = Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .map_err(|error| format!("cannot run git: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git rev-parse failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let printed = String::from_utf8_lossy(&output.stdout);
    let common = PathBuf::from(printed.trim());
    match common.parent() {
        Some(root) => Ok(root.join("testdata")),
        None => Err(format!(
            "git's common directory {} has no parent",
            common.display()
        )),
    }
}

/// The first of the two places that is a directory; neither is an error that
/// names both. The second is asked for only when the first is missing. No
/// symlink stands in for either: Windows cannot follow one made in WSL.
pub fn find_testdata(
    own: &Path,
    main: impl FnOnce() -> Result<PathBuf, Failure>,
) -> Result<PathBuf, Failure> {
    if own.is_dir() {
        return Ok(own.to_path_buf());
    }
    match main() {
        Ok(path) if path.is_dir() => Ok(path),
        Ok(path) => Err(format!(
            "no testdata/ at {} or at {}",
            own.display(),
            path.display()
        )),
        Err(why) => Err(format!(
            "no testdata/ at {}, and the main checkout's could not be found: {why}",
            own.display()
        )),
    }
}

/// The `testdata/` directory the tests read.
pub fn testdata() -> Result<PathBuf, Failure> {
    find_testdata(Path::new(OWN_TESTDATA), main_checkout_testdata)
}

/// The recorded baseline under `testdata/`.
pub fn base() -> Result<PathBuf, Failure> {
    Ok(testdata()?.join("baseline"))
}

/// Where the one place of the sample-file key spellings lives (the keys the
/// `stream` sub command writes into each line of its samples file).
pub const KEY_REPEAT: &str = "repeat";
pub const KEY_UTT: &str = "utt";
pub const KEY_T_END: &str = "t_end_us";
pub const KEY_T_FINAL: &str = "t_final_us";
pub const KEY_T_TR: &str = "t_tr_us";
pub const KEY_FINAL_LATENCY: &str = "final_latency_ms";
pub const KEY_CER_DIST: &str = "cer_dist";
pub const KEY_CER_DENOM: &str = "cer_denom";

/// `relative` under the nearest directory at or above the repository checkout
/// that holds it: untracked data is not in a worktree, so it is read from the
/// main checkout.
pub fn find_up(relative: &str) -> Result<PathBuf, Failure> {
    let start = Path::new(REPO)
        .canonicalize()
        .map_err(|error| format!("cannot resolve {REPO}: {error}"))?;
    start
        .ancestors()
        .map(|directory| directory.join(relative))
        .find(|path| path.exists())
        .ok_or_else(|| format!("no {relative} at or above {}", start.display()))
}

/// The text of a result file. Output saved from a Windows console can hold
/// bytes that are not UTF-8, so they are replaced rather than refused.
pub fn read(path: &Path) -> Result<String, Failure> {
    let bytes = read_bytes(path)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

pub fn read_bytes(path: &Path) -> Result<Vec<u8>, Failure> {
    fs::read(path).map_err(|_| format!("cannot read {}", path.display()))
}

pub fn read_json(path: &Path) -> Result<Value, Failure> {
    let text = read(path)?;
    serde_json::from_str(&text).map_err(|e| format!("{} is not JSON: {e}", path.display()))
}

/// A decimal such as `0.038` or `-47.5` in units of 10^-6. A number without
/// a decimal point, or with more than six decimals, is refused.
pub fn micros(text: &str) -> Result<i64, Failure> {
    let trimmed = text.trim();
    let negative = trimmed.starts_with('-');
    let digits = trimmed.trim_start_matches('-');
    let Some((whole, fraction)) = digits.split_once('.') else {
        return Err(format!("{text:?} has no decimal point"));
    };
    if fraction.len() > 6 {
        return Err(format!("{text:?} has more than six decimals"));
    }
    let Ok(whole) = whole.parse::<i64>() else {
        return Err(format!("{text:?} is not a decimal"));
    };
    let Ok(fraction) = format!("{fraction:0<6}").parse::<i64>() else {
        return Err(format!("{text:?} is not a decimal"));
    };
    let value = whole * 1_000_000 + fraction;
    Ok(if negative { -value } else { value })
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The text of the `## ` section whose heading contains `id`, up to the
/// next `## ` heading.
pub fn section<'a>(markdown: &'a str, id: &str) -> Result<&'a str, Failure> {
    let mut begin = None;
    let mut offset = 0;
    for line in markdown.split_inclusive('\n') {
        if line.starts_with("## ") {
            if let Some(start) = begin {
                return Ok(&markdown[start..offset]);
            }
            if line.contains(id) {
                begin = Some(offset + line.len());
            }
        }
        offset += line.len();
    }
    match begin {
        Some(start) => Ok(&markdown[start..]),
        None => Err(format!("no section for {id}")),
    }
}

/// The cells of the table rows in `text`, header included, separator rows
/// left out.
pub fn table_rows(text: &str) -> Vec<Vec<&str>> {
    text.lines()
        .map(str::trim)
        .filter(|line| line.starts_with('|'))
        .map(|line| {
            line.trim_matches('|')
                .split('|')
                .map(str::trim)
                .collect::<Vec<_>>()
        })
        .filter(|cells| {
            !cells
                .iter()
                .all(|cell| cell.chars().all(|c| c == '-' || c == ':'))
        })
        .collect()
}

/// The `p`-th percentile by nearest rank: the `ceil(p * n / 100)`-th
/// smallest of `n` values, counted from 1. An empty input is refused.
pub fn nearest_rank(values: &[i64], p: usize) -> Result<i64, Failure> {
    if values.is_empty() {
        return Err(String::from("percentile of no samples"));
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let rank = (p * sorted.len()).div_ceil(100);
    match rank.checked_sub(1).and_then(|index| sorted.get(index)) {
        Some(value) => Ok(*value),
        None => Err(format!("percentile {p} is out of range")),
    }
}

/// Whole milliseconds from microseconds, halves rounded up.
pub fn ms(micros: i64) -> i64 {
    (micros + 500).div_euclid(1000)
}

/// One line of a samples file written by `hayamimi-eval stream`.
#[derive(Clone, Debug)]
pub struct Row {
    pub repeat: i64,
    pub utt: i64,
    pub t_end: i64,
    pub t_final: Option<i64>,
    pub t_tr: Option<i64>,
    pub final_latency_ms: Option<f64>,
    pub cer_dist: i64,
    pub cer_denom: i64,
}

impl Row {
    /// Recognition delay `t_final - t_end`, in whole milliseconds.
    pub fn l_asr_ms(&self) -> Option<i64> {
        self.t_final.map(|t| ms(t - self.t_end))
    }

    /// Delay to the translation `t_tr - t_end`, in whole milliseconds.
    pub fn l_ms(&self) -> Option<i64> {
        self.t_tr.map(|t| ms(t - self.t_end))
    }

    /// Translation delay after the final `t_tr - t_final`, in milliseconds.
    pub fn l_tr_ms(&self) -> Option<i64> {
        match (self.t_final, self.t_tr) {
            (Some(f), Some(t)) => Some(ms(t - f)),
            _ => None,
        }
    }
}

fn required(value: &Value, key: &str) -> Result<i64, Failure> {
    match value.get(key).and_then(Value::as_i64) {
        Some(number) => Ok(number),
        None => Err(format!("a sample has no integer {key}")),
    }
}

/// Reads a samples file (one JSON object per line).
pub fn parse_rows(text: &str) -> Result<Vec<Row>, Failure> {
    let mut rows = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let value: Value =
            serde_json::from_str(line).map_err(|e| format!("a sample line is not JSON: {e}"))?;
        rows.push(Row {
            repeat: required(&value, KEY_REPEAT)?,
            utt: required(&value, KEY_UTT)?,
            t_end: required(&value, KEY_T_END)?,
            t_final: value.get(KEY_T_FINAL).and_then(Value::as_i64),
            t_tr: value.get(KEY_T_TR).and_then(Value::as_i64),
            final_latency_ms: value.get(KEY_FINAL_LATENCY).and_then(Value::as_f64),
            cer_dist: required(&value, KEY_CER_DIST)?,
            cer_denom: required(&value, KEY_CER_DENOM)?,
        });
    }
    Ok(rows)
}

pub fn load_rows(path: &Path) -> Result<Vec<Row>, Failure> {
    parse_rows(&read(path)?)
}

/// The samples files `lat_ja_<s>_<n>.jsonl` of one `--min-silence` value,
/// ordered by `n`.
pub fn run_files(silence: &str) -> Result<Vec<(u32, PathBuf)>, Failure> {
    Ok(run_files_in(&base()?, silence))
}

/// A file of the recorded RP-3 run of the Rust relay (R8e), `docs/results/r8e/rp3_rust.<suffix>`:
/// `jsonl` holds the samples, `events.jsonl` every event received.
pub fn rp3_run(suffix: &str) -> PathBuf {
    Path::new(RESULTS)
        .join("r8e")
        .join(format!("rp3_rust.{suffix}"))
}

/// Like [`run_files`], for the samples files in `dir`.
pub fn run_files_in(dir: &Path, silence: &str) -> Vec<(u32, PathBuf)> {
    let prefix = format!("lat_ja_{silence}_");
    let mut found: Vec<(u32, PathBuf)> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let number = name
                .strip_prefix(&prefix)?
                .strip_suffix(".jsonl")?
                .parse::<u32>()
                .ok()?;
            Some((number, entry.path()))
        })
        .collect();
    found.sort();
    found
}

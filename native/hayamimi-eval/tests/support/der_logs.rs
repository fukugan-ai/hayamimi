//! The DER logs of the evaluation runs: reading the mean DER lines, the list
//! of Python runs under the baseline directory, and the mean and the spread
//! of the runs. RB-7 reads them to record the Python baseline; RP-15 reads
//! the same logs to hold the Rust route to it.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use super::{Failure, REPO, micros, read};

/// How many times the DER is measured before a mean and a spread are taken.
pub const MIN_DER_RUNS: usize = 3;
pub const MEETINGS: usize = 5;
/// One hundredth of a point, in units of 10^-6 points.
pub const HUNDREDTH: i64 = 10_000;

/// The directory of the Python baseline. Result files are not in git, so a
/// worktree reads them from the main checkout: the first `testdata/baseline`
/// at or above the repository checkout.
pub fn baseline_dir() -> Result<PathBuf, Failure> {
    let start = Path::new(REPO)
        .canonicalize()
        .map_err(|error| format!("cannot resolve {REPO}: {error}"))?;
    start
        .ancestors()
        .map(|directory| directory.join("testdata").join("baseline"))
        .find(|baseline| baseline.is_dir())
        .ok_or_else(|| format!("no testdata/baseline at or above {}", start.display()))
}

/// Each `mean DER over N meeting(s): V%` line as `(N, V)`, V in units of 10^-6.
pub fn mean_der_lines(log: &str) -> Result<Vec<(usize, i64)>, Failure> {
    let mut found = Vec::new();
    for line in log.lines() {
        let Some((_, rest)) = line.split_once("mean DER over ") else {
            continue;
        };
        let Some((count, tail)) = rest.split_once(" meeting(s): ") else {
            return Err(format!("cannot read the mean DER line {line:?}"));
        };
        let Ok(count) = count.parse::<usize>() else {
            return Err(format!("{count:?} is not a meeting count"));
        };
        let Some((value, _)) = tail.split_once('%') else {
            return Err(format!("the mean DER line {line:?} has no percent sign"));
        };
        found.push((count, micros(value)?));
    }
    Ok(found)
}

/// A run's mean DER, in units of 10^-6: the log has a mean line, over five
/// meetings.
pub fn check_der(log: &str) -> Result<i64, Failure> {
    let lines = mean_der_lines(log)?;
    let Some((_, first)) = lines.first() else {
        return Err(String::from("no mean DER line"));
    };
    for (meetings, _) in &lines {
        if *meetings != MEETINGS {
            return Err(format!("{meetings} meetings, want {MEETINGS}"));
        }
    }
    Ok(*first)
}

/// Enough runs to take a mean and a spread: at least [`MIN_DER_RUNS`] logs
/// carry a mean DER line.
pub fn check_der_run_count(logs: &[&str]) -> Result<(), Failure> {
    let with_mean = logs
        .iter()
        .filter(|log| mean_der_lines(log).is_ok_and(|lines| !lines.is_empty()))
        .count();
    if with_mean >= MIN_DER_RUNS {
        Ok(())
    } else {
        Err(format!(
            "{with_mean} DER runs, want at least {MIN_DER_RUNS}"
        ))
    }
}

/// The DER logs of runs in `directory`, in the order of their numbers: a file
/// named `first` (when given) is run 1, and `<prefix><n>.log` is run `n`.
pub fn numbered_logs(
    directory: &Path,
    first: Option<&str>,
    prefix: &str,
) -> Result<Vec<PathBuf>, Failure> {
    let entries = fs::read_dir(directory)
        .map_err(|error| format!("cannot list {}: {error}", directory.display()))?;
    let mut found: Vec<(u32, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let number = if first == Some(name.as_str()) {
                1
            } else {
                name.strip_prefix(prefix)?
                    .strip_suffix(".log")?
                    .parse::<u32>()
                    .ok()?
            };
            Some((number, entry.path()))
        })
        .collect();
    found.sort();
    Ok(found.into_iter().map(|(_, path)| path).collect())
}

/// The DER logs of the Python `--method refine_diarize` runs: `der_eval.log`
/// is the first, `der_eval_run<n>.log` the others. A log of another method
/// has another name and is not listed.
pub fn der_run_files() -> Result<Vec<PathBuf>, Failure> {
    numbered_logs(&baseline_dir()?, Some("der_eval.log"), "der_eval_run")
}

pub fn der_run_logs() -> Result<Vec<String>, Failure> {
    der_run_files()?.iter().map(|path| read(path)).collect()
}

/// Every meeting row of a `--method refine_diarize` run carries `diar_rtf=`;
/// a run of the default method prints no such field.
pub fn check_refine_diarize(log: &str) -> Result<(), Failure> {
    let rows: Vec<&str> = log.lines().filter(|line| line.contains("] DER=")).collect();
    if rows.len() != MEETINGS {
        return Err(format!("{} meeting rows, want {MEETINGS}", rows.len()));
    }
    if rows.iter().any(|row| !row.contains("diar_rtf=")) {
        return Err(String::from(
            "the meeting rows carry no diar_rtf=: not a --method refine_diarize run",
        ));
    }
    Ok(())
}

/// The arithmetic mean of the runs' mean DER, rounded half up to two
/// decimals, in units of 10^-6.
pub fn mean_of_runs(values: &[i64]) -> Result<i64, Failure> {
    let Ok(count) = i64::try_from(values.len()) else {
        return Err(String::from("too many runs"));
    };
    let sum: i64 = values.iter().sum();
    match (2 * sum + count * HUNDREDTH).checked_div(2 * count * HUNDREDTH) {
        Some(steps) => Ok(steps * HUNDREDTH),
        None => Err(String::from("the mean of no runs")),
    }
}

/// The spread of the runs' mean DER: the largest minus the smallest.
pub fn spread_of_runs(values: &[i64]) -> Result<i64, Failure> {
    match (values.iter().max(), values.iter().min()) {
        (Some(high), Some(low)) => Ok(high - low),
        _ => Err(String::from("the spread of no runs")),
    }
}

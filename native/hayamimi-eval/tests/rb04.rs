//! RB-4: the Python golden check, run again, finds the eight recorded
//! Japanese clips unchanged. The inputs are its saved output and exit code.

mod support;

use std::path::Path;

use support::{FIXTURES, Failure, GOLDEN, TestResult, base, read};

const CLIPS: [&str; 8] = [
    "ja_014", "ja_024", "ja_033", "ja_036", "ja_044", "ja_079", "ja_085", "ja_090",
];

/// The clip named on each `  <wav>  finals=` line, without `.wav`.
fn clip_names(log: &str) -> Vec<String> {
    log.lines()
        .filter_map(|line| {
            let (name, _) = line.strip_prefix("  ")?.split_once("  finals=")?;
            Some(name.strip_suffix(".wav")?.to_owned())
        })
        .collect()
}

/// The clip named on each `DIFF <wav>` line, without `.wav`.
fn diff_names(log: &str) -> Vec<String> {
    log.lines()
        .filter_map(|line| Some(line.strip_prefix("DIFF ")?.strip_suffix(".wav")?.to_owned()))
        .collect()
}

/// `(differ, total)` from the `N of M clips differ` line.
fn statistic(log: &str) -> Option<(usize, usize)> {
    log.lines().find_map(|line| {
        let mut words = line.split_whitespace();
        let differ = words.next()?.parse().ok()?;
        let (of, total) = (words.next()?, words.next()?.parse().ok()?);
        let rest: Vec<&str> = words.collect();
        (of == "of" && rest == ["clips", "differ"]).then_some((differ, total))
    })
}

/// Eight clips, none differing, exit code 0. The error names every clip that differs.
fn check_golden_log(log: &str, exit_code: &str) -> Result<(), Failure> {
    let mut names = clip_names(log);
    if names.len() != CLIPS.len() {
        return Err(format!(
            "{} clips in the log, want {}",
            names.len(),
            CLIPS.len()
        ));
    }
    names.sort();
    if names != CLIPS {
        return Err(format!("the clips are {names:?}"));
    }
    let differing = diff_names(log);
    if !differing.is_empty() {
        return Err(format!("clips differ: {}", differing.join(", ")));
    }
    if statistic(log) != Some((0, CLIPS.len())) {
        return Err(String::from("no `0 of 8 clips differ` line"));
    }
    if exit_code.trim() != "0" {
        return Err(format!("exit code {}", exit_code.trim()));
    }
    Ok(())
}

fn log() -> Result<String, Failure> {
    read(&base()?.join("golden_check.log"))
}

#[test]
fn rb04_python_golden_reproduces_record_log_names_all_eight_golden_clips() -> TestResult {
    let mut named = clip_names(&log()?);
    named.sort();
    assert_eq!(named, CLIPS);
    let mut on_disk: Vec<String> = std::fs::read_dir(GOLDEN)?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let stem = path.file_stem()?.to_string_lossy().into_owned();
            (path.extension()? == "wav").then_some(stem)
        })
        .collect();
    on_disk.sort();
    assert_eq!(named, on_disk);
    Ok(())
}

#[test]
fn rb04_python_golden_reproduces_record_log_reports_zero_of_eight_clips_differ() -> TestResult {
    assert_eq!(statistic(&log()?), Some((0, 8)));
    Ok(())
}

#[test]
fn rb04_python_golden_reproduces_record_log_has_no_diff_lines() -> TestResult {
    let text = log()?;
    assert!(statistic(&text).is_some(), "the log has no statistic line");
    assert!(diff_names(&text).is_empty());
    Ok(())
}

#[test]
fn rb04_python_golden_reproduces_record_check_exit_code_is_zero() -> TestResult {
    let code = read(&base()?.join("golden_check.rc"))?;
    check_golden_log(&log()?, &code)?;
    assert_eq!(code.trim(), "0");
    Ok(())
}

#[test]
fn rb04_python_golden_reproduces_record_itn_off_log_names_ja_085_and_ja_090() {
    let said = check_golden_log(
        include_str!("fixtures/rb04_two_diffs.log"),
        include_str!("fixtures/rb04_two_diffs.rc"),
    );
    assert!(matches!(&said, Err(text) if text.contains("ja_085") && text.contains("ja_090")));
}

#[test]
fn rb04_python_golden_reproduces_record_log_with_seven_clips_is_rejected() {
    let said = check_golden_log(include_str!("fixtures/rb04_seven_clips.log"), "0\n");
    assert!(said.is_err());
}

#[test]
fn rb04_python_golden_reproduces_record_log_with_nine_clips_is_rejected() {
    let said = check_golden_log(include_str!("fixtures/rb04_nine_clips.log"), "0\n");
    assert!(said.is_err());
}

#[test]
fn rb04_python_golden_reproduces_record_empty_golden_log_is_an_error() {
    let said = check_golden_log(include_str!("fixtures/rb04_empty.log"), "0\n");
    assert!(said.is_err());
}

#[test]
fn rb04_python_golden_reproduces_record_missing_golden_log_names_its_path() {
    let absent = Path::new(FIXTURES).join("absent").join("golden_check.log");
    assert!(matches!(read(&absent), Err(path) if path.ends_with("golden_check.log")));
}

//! RB-7: the translation chrF and the diarization DER of the Python
//! evaluation, reproduced and summarised, and the chrF fixture the Rust
//! scorer will later be held to.
//!
//! The inputs are `chrf_eval.log`, the DER logs `der_eval.log` and
//! `der_eval_run<n>.log`, `chrf_fixture.tsv` and `chrf_control.tsv` under the
//! baseline directory, and the RB-7 section of
//! `docs/results/baseline_python.md`. The DER has no recorded value to match:
//! it is measured at least three times, and the mean and the spread of the
//! runs are what the Rust implementation is later held to.

mod support;

use std::fs;
use std::path::{Path, PathBuf};

use hayamimi_eval::fleurs::degrade;
use support::{BASE, FIXTURES, Failure, RESULTS, TestResult, micros, read, section};

/// The recorded corpus chrF per target language, in units of 10^-6.
const CHRF_RECORD: [(&str, i64); 4] = [
    ("en", 47_570_000),
    ("zh", 20_190_000),
    ("ko", 25_200_000),
    ("es", 42_090_000),
];
const CHRF_TOLERANCE: i64 = 10_000;

/// How many times the DER is measured before a mean and a spread are taken.
const MIN_DER_RUNS: usize = 3;
const MEETINGS: usize = 5;
const SENTENCES: usize = 50;
/// One hundredth of a point, in units of 10^-6 points.
const HUNDREDTH: i64 = 10_000;

/// A log with one mean DER line, as a run prints it.
const ONE_RUN: &str = "=== mean DER over 5 meeting(s): 13.3% (collar=0.25s) ===";

fn base(name: &str) -> PathBuf {
    Path::new(BASE).join(name)
}

/// Every `chrF=` value logged for `lang`, in units of 10^-6.
fn chrf_values(log: &str, lang: &str) -> Result<Vec<i64>, Failure> {
    let mut found = Vec::new();
    for line in log.lines() {
        let Some((_, rest)) = line.split_once("] ") else {
            continue;
        };
        let Some((name, tail)) = rest.split_once(": chrF=") else {
            continue;
        };
        let Some(value) = tail.split_whitespace().next() else {
            continue;
        };
        if name == lang {
            found.push(micros(value)?);
        }
    }
    Ok(found)
}

fn check_chrf_value(lang: &str, got: i64) -> Result<(), Failure> {
    let Some((_, want)) = CHRF_RECORD.iter().find(|(name, _)| *name == lang) else {
        return Err(format!("{lang} has no record"));
    };
    if (got - want).abs() > CHRF_TOLERANCE {
        return Err(format!(
            "{lang}: chrF {got} is not within {CHRF_TOLERANCE} of {want}"
        ));
    }
    Ok(())
}

/// Every language is logged and every logged value is within the tolerance.
fn check_chrf(log: &str) -> Result<(), Failure> {
    for (lang, _) in CHRF_RECORD {
        let values = chrf_values(log, lang)?;
        if values.is_empty() {
            return Err(format!("{lang} has no chrF line"));
        }
        for value in values {
            check_chrf_value(lang, value)?;
        }
    }
    Ok(())
}

/// Each `mean DER over N meeting(s): V%` line as `(N, V)`, V in units of 10^-6.
fn mean_der_lines(log: &str) -> Result<Vec<(usize, i64)>, Failure> {
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
fn check_der(log: &str) -> Result<i64, Failure> {
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
fn check_der_run_count(logs: &[&str]) -> Result<(), Failure> {
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

/// The DER logs of `--method refine_diarize` runs: `der_eval.log` is the
/// first, `der_eval_run<n>.log` the others. A log of another method has
/// another name and is not listed.
fn der_run_files() -> Vec<PathBuf> {
    let mut found: Vec<(u32, PathBuf)> = fs::read_dir(BASE)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let number = if name == "der_eval.log" {
                1
            } else {
                name.strip_prefix("der_eval_run")?
                    .strip_suffix(".log")?
                    .parse::<u32>()
                    .ok()?
            };
            Some((number, entry.path()))
        })
        .collect();
    found.sort();
    found.into_iter().map(|(_, path)| path).collect()
}

fn der_run_logs() -> Result<Vec<String>, Failure> {
    der_run_files().iter().map(|path| read(path)).collect()
}

/// Every meeting row of a `--method refine_diarize` run carries `diar_rtf=`;
/// a run of the default method prints no such field.
fn check_refine_diarize(log: &str) -> Result<(), Failure> {
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

/// The per-meeting `DER=V%` values in units of 10^-6.
fn meeting_ders(log: &str) -> Result<Vec<i64>, Failure> {
    let mut found = Vec::new();
    for line in log.lines() {
        let Some((_, rest)) = line.split_once("] DER=") else {
            continue;
        };
        let Some((value, _)) = rest.split_once('%') else {
            return Err(format!("the DER line {line:?} has no percent sign"));
        };
        found.push(micros(value)?);
    }
    Ok(found)
}

/// The five per-meeting values average to the printed mean within 0.1 point.
fn check_meeting_average(log: &str) -> Result<(), Failure> {
    let ders = meeting_ders(log)?;
    if ders.len() != MEETINGS {
        return Err(format!("{} meeting lines, want {MEETINGS}", ders.len()));
    }
    let lines = mean_der_lines(log)?;
    let Some((_, printed)) = lines.first() else {
        return Err(String::from("no mean DER line"));
    };
    let total: i64 = ders.iter().sum();
    let average = total / MEETINGS as i64;
    if (average - printed).abs() > 100_000 {
        return Err(format!(
            "the meetings average {average}, the log prints {printed}"
        ));
    }
    Ok(())
}

/// The arithmetic mean of the runs' mean DER, rounded half up to two
/// decimals, in units of 10^-6.
fn mean_of_runs(values: &[i64]) -> Result<i64, Failure> {
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
fn spread_of_runs(values: &[i64]) -> Result<i64, Failure> {
    match (values.iter().max(), values.iter().min()) {
        (Some(high), Some(low)) => Ok(high - low),
        _ => Err(String::from("the spread of no runs")),
    }
}

/// The value after `marker` on its line of the summary, in units of 10^-6.
fn recorded_value(section_text: &str, marker: &str) -> Result<i64, Failure> {
    let Some(line) = section_text.lines().find(|line| line.starts_with(marker)) else {
        return Err(format!("no line starts with {marker:?}"));
    };
    micros(line[marker.len()..].trim())
}

/// The summary records the mean and the spread of the runs' mean DER.
fn check_der_summary(run_means: &[i64], section_text: &str) -> Result<(), Failure> {
    let mean = mean_of_runs(run_means)?;
    let said = recorded_value(section_text, "DER 平均：")?;
    if said != mean {
        return Err(format!("DER 平均 is {said}, recomputed {mean}"));
    }
    let spread = spread_of_runs(run_means)?;
    let said = recorded_value(section_text, "DER 幅：")?;
    if said != spread {
        return Err(format!("DER 幅 is {said}, recomputed {spread}"));
    }
    Ok(())
}

/// Each target language has 50 sentence rows and one corpus row; the result
/// is the number of rows.
fn check_fixture_rows(tsv: &str) -> Result<usize, Failure> {
    let rows: Vec<Vec<&str>> = tsv.lines().map(|line| line.split('\t').collect()).collect();
    for (lang, _) in CHRF_RECORD {
        let mine: Vec<&Vec<&str>> = rows
            .iter()
            .filter(|cells| cells.first() == Some(&lang))
            .collect();
        let corpus = mine
            .iter()
            .filter(|cells| cells.get(1) == Some(&"corpus"))
            .count();
        if corpus != 1 {
            return Err(format!("{lang} has {corpus} corpus rows, want 1"));
        }
        if mine.len() - corpus != SENTENCES {
            return Err(format!(
                "{lang} has {} sentence rows, want {SENTENCES}",
                mine.len() - corpus
            ));
        }
        for cells in mine {
            let Some(score) = cells.get(2) else {
                return Err(format!("a {lang} row has no score"));
            };
            micros(score)?;
        }
    }
    Ok(rows.len())
}

/// `(lang, reference as hypothesis, source as hypothesis)` in units of 10^-6.
fn control_rows(tsv: &str) -> Result<Vec<(String, i64, i64)>, Failure> {
    let mut found = Vec::new();
    for line in tsv.lines() {
        let cells: Vec<&str> = line.split('\t').collect();
        let [lang, reference, source] = cells.as_slice() else {
            return Err(format!(
                "the control row {line:?} does not have three columns"
            ));
        };
        found.push(((*lang).to_owned(), micros(reference)?, micros(source)?));
    }
    Ok(found)
}

fn check_reference_scores_100(tsv: &str) -> Result<(), Failure> {
    let rows = control_rows(tsv)?;
    for (lang, _) in CHRF_RECORD {
        let Some((_, reference, _)) = rows.iter().find(|(name, _, _)| name == lang) else {
            return Err(format!("{lang} has no control row"));
        };
        if *reference != 100_000_000 {
            return Err(format!(
                "{lang}: the reference scores {reference} against itself"
            ));
        }
    }
    Ok(())
}

fn check_source_below_record(tsv: &str) -> Result<(), Failure> {
    let rows = control_rows(tsv)?;
    for (lang, record) in CHRF_RECORD {
        let Some((_, _, source)) = rows.iter().find(|(name, _, _)| name == lang) else {
            return Err(format!("{lang} has no control row"));
        };
        if *source >= record {
            return Err(format!(
                "{lang}: the source scores {source}, not below the record {record}"
            ));
        }
    }
    Ok(())
}

fn summary_section() -> Result<String, Failure> {
    let summary = read(&Path::new(RESULTS).join("baseline_python.md"))?;
    Ok(section(&summary, "RB-7")?.to_owned())
}

#[test]
fn rb07_python_chrf_and_der_reproduce_record_chrf_within_0_01_of_record() -> TestResult {
    check_chrf(&read(&base("chrf_eval.log"))?)?;
    Ok(())
}

#[test]
fn rb07_python_chrf_and_der_reproduce_record_chrf_upper_edge_is_record_plus_0_01() -> TestResult {
    let values = chrf_values(include_str!("fixtures/rb07_chrf_en_plus.log"), "en")?;
    assert_eq!(values, vec![47_580_000, 47_590_000]);
    assert!(check_chrf_value("en", values[0]).is_ok());
    assert!(check_chrf_value("en", values[1]).is_err());
    Ok(())
}

#[test]
fn rb07_python_chrf_and_der_reproduce_record_chrf_lower_edge_is_record_minus_0_01() -> TestResult {
    let values = chrf_values(include_str!("fixtures/rb07_chrf_en_minus.log"), "en")?;
    assert_eq!(values, vec![47_560_000, 47_550_000]);
    assert!(check_chrf_value("en", values[0]).is_ok());
    assert!(check_chrf_value("en", values[1]).is_err());
    Ok(())
}

#[test]
fn rb07_python_chrf_and_der_reproduce_record_chrf_log_missing_a_language_is_named() {
    let said = check_chrf(include_str!("fixtures/rb07_chrf_no_es.log"));
    assert!(matches!(&said, Err(text) if text.starts_with("es")));
}

#[test]
fn rb07_python_chrf_and_der_reproduce_record_at_least_three_der_runs_exist() -> TestResult {
    let texts = der_run_logs()?;
    for text in &texts {
        check_der(text)?;
    }
    let logs: Vec<&str> = texts.iter().map(String::as_str).collect();
    check_der_run_count(&logs)?;
    Ok(())
}

#[test]
fn rb07_python_chrf_and_der_reproduce_record_two_der_runs_are_rejected() {
    let said = check_der_run_count(&[ONE_RUN, ONE_RUN]);
    assert!(matches!(&said, Err(text) if text.contains('2') && text.contains('3')));
}

#[test]
fn rb07_python_chrf_and_der_reproduce_record_three_der_runs_are_accepted() {
    assert!(check_der_run_count(&[ONE_RUN, ONE_RUN, ONE_RUN]).is_ok());
}

#[test]
fn rb07_python_chrf_and_der_reproduce_record_four_meetings_are_rejected() {
    let said = check_der(include_str!("fixtures/rb07_der_four.log"));
    assert!(matches!(&said, Err(text) if text.contains("4 meetings")));
}

#[test]
fn rb07_python_chrf_and_der_reproduce_record_six_meetings_are_rejected() {
    let said = check_der(include_str!("fixtures/rb07_der_six.log"));
    assert!(matches!(&said, Err(text) if text.contains("6 meetings")));
}

#[test]
fn rb07_python_chrf_and_der_reproduce_record_meeting_ders_average_to_the_printed_mean() -> TestResult
{
    for text in der_run_logs()? {
        check_meeting_average(&text)?;
    }
    Ok(())
}

#[test]
fn rb07_python_chrf_and_der_reproduce_record_fixture_has_50_sentences_and_one_corpus_row_per_language()
-> TestResult {
    let lines = check_fixture_rows(&read(&base("chrf_fixture.tsv"))?)?;
    assert_eq!(lines, 204);
    Ok(())
}

#[test]
fn rb07_python_chrf_and_der_reproduce_record_fixture_missing_a_corpus_row_is_named() {
    let said = check_fixture_rows(include_str!("fixtures/rb07_fixture_no_corpus_row.tsv"));
    assert!(matches!(&said, Err(text) if text.starts_with("zh")));
}

#[test]
fn rb07_python_chrf_and_der_reproduce_record_degrade_drops_the_first_word_of_space_separated_text()
{
    assert_eq!(
        degrade("The cat sat on the mat.", "en"),
        "cat sat on the mat."
    );
    assert_eq!(degrade("Hola mundo cruel.", "es"), "mundo cruel.");
}

#[test]
fn rb07_python_chrf_and_der_reproduce_record_degrade_drops_the_first_character_of_zh() {
    assert_eq!(degrade("你好世界。", "zh"), "好世界。");
}

#[test]
fn rb07_python_chrf_and_der_reproduce_record_degrade_of_a_one_word_sentence_is_empty() {
    assert_eq!(degrade("Yes", "en"), "");
}

#[test]
fn rb07_python_chrf_and_der_reproduce_record_reference_as_hypothesis_scores_100() -> TestResult {
    check_reference_scores_100(&read(&base("chrf_control.tsv"))?)?;
    Ok(())
}

#[test]
fn rb07_python_chrf_and_der_reproduce_record_source_as_hypothesis_scores_below_record() -> TestResult
{
    check_source_below_record(&read(&base("chrf_control.tsv"))?)?;
    Ok(())
}

#[test]
fn rb07_python_chrf_and_der_reproduce_record_source_as_hypothesis_above_record_is_flagged() {
    let said = check_source_below_record(include_str!("fixtures/rb07_control_above_record.tsv"));
    assert!(matches!(&said, Err(text) if text.starts_with("zh")));
}

#[test]
fn rb07_python_chrf_and_der_reproduce_record_missing_chrf_log_names_its_path() {
    let absent = Path::new(FIXTURES).join("absent").join("chrf_eval.log");
    assert!(matches!(read(&absent), Err(path) if path.ends_with("chrf_eval.log")));
}

#[test]
fn rb07_python_chrf_and_der_reproduce_record_der_run_without_diar_rtf_rows_is_not_a_refine_diarize_run()
-> TestResult {
    for text in der_run_logs()? {
        check_refine_diarize(&text)?;
    }
    let said = check_refine_diarize(include_str!("fixtures/rb07_der_default_method.log"));
    assert!(matches!(&said, Err(text) if text.contains("--method")));
    Ok(())
}

#[test]
fn rb07_python_chrf_and_der_reproduce_record_baseline_summary_records_der_mean_and_spread_of_runs()
-> TestResult {
    let means = der_run_logs()?
        .iter()
        .map(|text| check_der(text))
        .collect::<Result<Vec<_>, Failure>>()?;
    check_der_summary(&means, &summary_section()?)?;
    Ok(())
}

#[test]
fn rb07_python_chrf_and_der_reproduce_record_mean_and_spread_of_13_3_13_1_13_5_are_13_30_and_0_4()
-> TestResult {
    let runs = [13_300_000, 13_100_000, 13_500_000];
    assert_eq!(mean_of_runs(&runs)?, 13_300_000);
    assert_eq!(spread_of_runs(&runs)?, 400_000);
    Ok(())
}

#[test]
fn rb07_python_chrf_and_der_reproduce_record_summary_without_der_spread_is_rejected() {
    let said = check_der_summary(
        &[13_300_000, 13_100_000, 13_500_000],
        include_str!("fixtures/rb07_summary_without_der_spread.md"),
    );
    assert!(matches!(&said, Err(text) if text.contains("DER 幅")));
}

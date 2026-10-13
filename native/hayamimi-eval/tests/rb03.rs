//! RB-3: the Python scorecard, run again twice, reproduces the recorded
//! per-language mean error, and the tracked scorecard was put back.

mod support;

use std::path::Path;

use support::{
    BASE, FIXTURES, Failure, RESULTS, TestResult, micros, read, read_bytes, section, table_rows,
};

const LANGS: [&str; 5] = ["ja", "en", "zh", "ko", "yue"];

/// The recorded mean error per language, in units of 10^-6.
const RECORD: [(&str, i64); 5] = [
    ("ja", 38_000),
    ("en", 23_000),
    ("zh", 66_000),
    ("ko", 81_000),
    ("yue", 61_000),
];

/// The allowed distance from the record, in units of 10^-6.
const TOLERANCE: i64 = 5_000;

/// The `mean err` of each language row of the scorecard table.
fn means(text: &str) -> Result<Vec<(String, i64)>, Failure> {
    let rows = table_rows(text);
    let Some(head) = rows
        .iter()
        .position(|cells| cells.first() == Some(&"lang") && cells.contains(&"mean err"))
    else {
        return Err(String::from("no table with a `mean err` column"));
    };
    let header = &rows[head];
    let Some(column) = header.iter().position(|cell| *cell == "mean err") else {
        return Err(String::from("no `mean err` column"));
    };
    let mut found = Vec::new();
    for cells in rows[head + 1..].iter().take_while(|cells| {
        cells.len() == header.len() && cells.first().is_some_and(|lang| LANGS.contains(lang))
    }) {
        found.push((cells[0].to_owned(), micros(cells[column])?));
    }
    Ok(found)
}

fn mean_of(list: &[(String, i64)], lang: &str) -> Result<i64, Failure> {
    match list.iter().find(|(name, _)| name == lang) {
        Some((_, mean)) => Ok(*mean),
        None => Err(format!("{lang} has no row")),
    }
}

/// Each language's mean error is within `TOLERANCE` of the record.
fn check_means_against_record(text: &str) -> Result<(), Failure> {
    let found = means(text)?;
    for (lang, want) in RECORD {
        let got = mean_of(&found, lang)?;
        if (got - want).abs() > TOLERANCE {
            return Err(format!("{lang}: {got} is not within {TOLERANCE} of {want}"));
        }
    }
    Ok(())
}

/// A run made without OpenCC is refused whatever its numbers say.
fn check_run(text: &str) -> Result<(), Failure> {
    if text.contains("opencc無し") {
        return Err(String::from(
            "opencc無し: the yue value was made without OpenCC",
        ));
    }
    check_means_against_record(text)
}

/// The absolute difference of two values, with three decimals.
fn difference_text(a: i64, b: i64) -> String {
    let distance = (a - b).abs();
    format!(
        "{}.{:03}",
        distance / 1_000_000,
        distance % 1_000_000 / 1_000
    )
}

/// The summary's RB-3 section has, for each language, a row with the
/// difference between the two runs.
fn check_run_differences(summary: &str, run1: &str, run2: &str) -> Result<(), Failure> {
    let rows = table_rows(section(summary, "RB-3")?);
    let (one, two) = (means(run1)?, means(run2)?);
    for lang in LANGS {
        let want = difference_text(mean_of(&one, lang)?, mean_of(&two, lang)?);
        let present = rows
            .iter()
            .any(|cells| cells.first() == Some(&lang) && cells.contains(&want.as_str()));
        if !present {
            return Err(format!("{lang}: no row with the run difference {want}"));
        }
    }
    Ok(())
}

fn check_same_bytes(restored: &[u8], record: &[u8]) -> Result<(), Failure> {
    if restored == record {
        Ok(())
    } else {
        Err(String::from(
            "the restored scorecard differs from the record",
        ))
    }
}

fn base(name: &str) -> std::path::PathBuf {
    Path::new(BASE).join(name)
}

#[test]
fn rb03_python_scorecard_reproduces_record_record_file_parses_to_the_five_recorded_means()
-> TestResult {
    let found = means(&read(&base("scorecard_record.md"))?)?;
    let want: Vec<(String, i64)> = RECORD
        .iter()
        .map(|(lang, mean)| ((*lang).to_owned(), *mean))
        .collect();
    assert_eq!(found, want);
    Ok(())
}

#[test]
fn rb03_python_scorecard_reproduces_record_run1_means_within_0_005_of_record() -> TestResult {
    check_run(&read(&base("scorecard_run1.md"))?)?;
    Ok(())
}

#[test]
fn rb03_python_scorecard_reproduces_record_run2_means_within_0_005_of_record() -> TestResult {
    check_run(&read(&base("scorecard_run2.md"))?)?;
    Ok(())
}

#[test]
fn rb03_python_scorecard_reproduces_record_difference_of_exactly_0_005_is_accepted() -> TestResult {
    check_means_against_record(include_str!("fixtures/rb03_ja_plus_0_005.md"))?;
    Ok(())
}

#[test]
fn rb03_python_scorecard_reproduces_record_difference_of_0_006_is_rejected() {
    let said = check_means_against_record(include_str!("fixtures/rb03_ja_plus_0_006.md"));
    assert!(matches!(&said, Err(text) if text.starts_with("ja:")));
}

#[test]
fn rb03_python_scorecard_reproduces_record_difference_below_record_by_0_006_is_rejected() {
    let said = check_means_against_record(include_str!("fixtures/rb03_ja_minus_0_006.md"));
    assert!(matches!(&said, Err(text) if text.starts_with("ja:")));
}

#[test]
fn rb03_python_scorecard_reproduces_record_missing_language_row_is_named() {
    let said = check_means_against_record(include_str!("fixtures/rb03_no_yue_row.md"));
    assert!(matches!(&said, Err(text) if text.starts_with("yue")));
}

#[test]
fn rb03_python_scorecard_reproduces_record_runs_report_t2s_normalisation_for_yue() -> TestResult {
    for name in ["scorecard_run1.md", "scorecard_run2.md"] {
        assert!(read(&base(name))?.contains("yueはt2s正規化"), "{name}");
    }
    Ok(())
}

#[test]
fn rb03_python_scorecard_reproduces_record_run_without_opencc_is_rejected() {
    let said = check_run(include_str!("fixtures/rb03_run_without_opencc.md"));
    assert!(matches!(&said, Err(text) if text.contains("opencc無し")));
}

#[test]
fn rb03_python_scorecard_reproduces_record_runs_contain_the_lid_misjudgement_section() -> TestResult
{
    for name in ["scorecard_run1.md", "scorecard_run2.md"] {
        assert!(read(&base(name))?.contains("LID誤判定の内訳"), "{name}");
    }
    Ok(())
}

#[test]
fn rb03_python_scorecard_reproduces_record_baseline_summary_records_the_difference_between_runs()
-> TestResult {
    let summary = read(&Path::new(RESULTS).join("baseline_python.md"))?;
    check_run_differences(
        &summary,
        &read(&base("scorecard_run1.md"))?,
        &read(&base("scorecard_run2.md"))?,
    )?;
    Ok(())
}

#[test]
fn rb03_python_scorecard_reproduces_record_summary_without_run_difference_is_rejected() -> TestResult
{
    let run = read(&base("scorecard_run1.md"))?;
    let said = check_run_differences(
        include_str!("fixtures/rb03_summary_without_difference.md"),
        &run,
        &run,
    );
    assert!(matches!(&said, Err(text) if text.starts_with("yue")));
    Ok(())
}

#[test]
fn rb03_python_scorecard_reproduces_record_scorecard_md_is_restored_byte_for_byte() -> TestResult {
    let restored = read_bytes(&Path::new(RESULTS).join("scorecard.md"))?;
    check_same_bytes(&restored, &read_bytes(&base("scorecard_record.md"))?)?;
    Ok(())
}

#[test]
fn rb03_python_scorecard_reproduces_record_one_byte_difference_in_restored_scorecard_is_rejected() {
    let said = check_same_bytes(
        include_bytes!("fixtures/rb03_restored_a.md"),
        include_bytes!("fixtures/rb03_restored_b.md"),
    );
    assert!(said.is_err());
}

#[test]
fn rb03_python_scorecard_reproduces_record_missing_scorecard_run_names_its_path() {
    let absent = Path::new(FIXTURES).join("absent").join("scorecard_run1.md");
    assert!(matches!(read(&absent), Err(path) if path.ends_with("scorecard_run1.md")));
}

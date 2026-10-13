//! CX-4: without a rate file the currency is skipped and nothing else is.

mod support;

use std::collections::BTreeSet;
use std::fs;

use hayamimi_relay::annotate::SkipReason;
use hayamimi_relay::lang::Lang;
use hayamimi_relay::pipeline::{Status, Translation};
use hayamimi_relay::units::Kind;
use support::{DEFAULT_NOW, Harness, TempDir, TestResult, dir_with_rates, epoch, rates_a};

const MILE_SRC: &str = "I paid $100 for 5 miles.";
const MILE_EDITED: &str = "5マイルで100ドル払いました。";

fn expect_currency_no_rate(line: &Translation, id: &str) {
    assert_eq!(line.status, Status::Ok, "{id}: status");
    assert!(
        line.skipped
            .iter()
            .any(|skipped| skipped.kind == Kind::Currency && skipped.reason == SkipReason::NoRate),
        "{id}: {:?}",
        line.skipped
    );
}

/// A reader opened on a directory where no rate file was ever made.
fn line_without_file(
    src: &str,
    lang: Lang,
    edited: &str,
) -> Result<Translation, Box<dyn std::error::Error>> {
    let dir = TempDir::new("cx04")?;
    let mut harness = Harness::open(Some(dir.join("fx_rates.json")), None)?;
    Ok(harness.line(src, lang, edited, epoch(DEFAULT_NOW)?))
}

/// T-CX-4-001
#[test]
fn cx04_missing_rate_file_skips_currency_only_missing_file_skips_the_dollar_with_no_rate()
-> TestResult {
    let line = line_without_file("I paid $100.", Lang::En, "私は100ドル払いました。")?;
    assert!(line.annotations.is_empty(), "T-CX-4-001: annotations");
    expect_currency_no_rate(&line, "T-CX-4-001");
    Ok(())
}

/// T-CX-4-002
#[test]
fn cx04_missing_rate_file_skips_currency_only_missing_file_still_annotates_the_mile_on_the_same_line()
-> TestResult {
    let line = line_without_file(MILE_SRC, Lang::En, MILE_EDITED)?;
    let notes: Vec<&str> = line
        .annotations
        .iter()
        .map(|note| note.text.trim())
        .collect();
    assert_eq!(notes, ["（約 8.05 km）"], "T-CX-4-002");
    expect_currency_no_rate(&line, "T-CX-4-002");
    Ok(())
}

/// T-CX-4-003
#[test]
fn cx04_missing_rate_file_skips_currency_only_missing_file_skips_the_yen_for_the_english_reader()
-> TestResult {
    let line = line_without_file(
        "予算は五百万円です。",
        Lang::Ja,
        "The budget is five million yen.",
    )?;
    assert!(line.annotations.is_empty(), "T-CX-4-003: annotations");
    expect_currency_no_rate(&line, "T-CX-4-003");
    Ok(())
}

/// T-CX-4-004
#[test]
fn cx04_missing_rate_file_skips_currency_only_present_file_annotates_both_on_the_same_line()
-> TestResult {
    let (_dir, path) = dir_with_rates(&rates_a())?;
    let mut harness = Harness::open(Some(path), None)?;
    let line = harness.line(MILE_SRC, Lang::En, MILE_EDITED, epoch(DEFAULT_NOW)?);
    let notes: BTreeSet<&str> = line
        .annotations
        .iter()
        .map(|note| note.text.trim())
        .collect();
    let expected: BTreeSet<&str> = ["（約 15,900 円、10/12 の値）", "（約 8.05 km）"]
        .into_iter()
        .collect();
    assert_eq!(line.annotations.len(), 2, "T-CX-4-004: count");
    assert_eq!(notes, expected, "T-CX-4-004");
    Ok(())
}

/// The run where the file existed and was then deleted under an open reader.
#[test]
fn cx04_missing_rate_file_skips_currency_only_deleted_file_skips_the_dollar_with_no_rate()
-> TestResult {
    let (_dir, path) = dir_with_rates(&rates_a())?;
    let mut harness = Harness::open(Some(path.clone()), None)?;
    let before = harness.line(MILE_SRC, Lang::En, MILE_EDITED, epoch(DEFAULT_NOW)?);
    assert_eq!(
        before.annotations.len(),
        2,
        "deleted-file run: before the delete"
    );
    fs::remove_file(&path)?;
    let after = harness.line(MILE_SRC, Lang::En, MILE_EDITED, epoch(DEFAULT_NOW)?);
    let notes: Vec<&str> = after
        .annotations
        .iter()
        .map(|note| note.text.trim())
        .collect();
    assert_eq!(notes, ["（約 8.05 km）"], "deleted-file run");
    expect_currency_no_rate(&after, "deleted-file run");
    Ok(())
}

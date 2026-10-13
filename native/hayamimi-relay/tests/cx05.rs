//! CX-5: an old rate shows its date, and one older than seven days is not used.
//!
//! `now` is 2026-10-20. Days back: 10-17 is 3, 10-13 is 7, 10-12 is 8,
//! 10-20 is 0.

mod support;

use hayamimi_relay::annotate::SkipReason;
use hayamimi_relay::lang::Lang;
use hayamimi_relay::pipeline::Translation;
use hayamimi_relay::units::Kind;
use support::{Harness, TestResult, dir_with_rates, epoch, rates_json};

const NOW: &str = "2026-10-20T09:00:00+09:00";
const DOLLAR_SRC: &str = "I paid $100.";
const DOLLAR_EDITED: &str = "私は100ドル払いました。";
const YEN_SRC: &str = "予算は五百万円です。";
const YEN_EDITED: &str = "The budget is five million yen.";

/// One line against a rate file dated `rate_date` (fetched that morning).
fn line_on(
    rate_date: &str,
    src: &str,
    lang: Lang,
    edited: &str,
) -> Result<Translation, Box<dyn std::error::Error>> {
    let fetched_at = format!("{rate_date}T07:30:00+09:00");
    let (_dir, path) = dir_with_rates(&rates_json(rate_date, &fetched_at, "1.1181", "177.33"))?;
    let mut harness = Harness::open(Some(path), None)?;
    Ok(harness.line(src, lang, edited, epoch(NOW)?))
}

fn expect_note(line: &Translation, note: &str, id: &str) {
    let notes: Vec<&str> = line
        .annotations
        .iter()
        .map(|found| found.text.trim())
        .collect();
    assert_eq!(notes, [note], "{id}");
}

fn expect_stale(line: &Translation, id: &str) {
    assert!(
        line.annotations.is_empty(),
        "{id}: annotations {:?}",
        line.annotations
    );
    assert!(
        line.skipped.iter().any(
            |skipped| skipped.kind == Kind::Currency && skipped.reason == SkipReason::StaleRate
        ),
        "{id}: {:?}",
        line.skipped
    );
}

/// T-CX-5-001
#[test]
fn cx05_old_rates_show_their_date_and_expire_after_seven_days_three_days_old_ja_has_its_date()
-> TestResult {
    let line = line_on("2026-10-17", DOLLAR_SRC, Lang::En, DOLLAR_EDITED)?;
    expect_note(&line, "（約 15,900 円、10/17 の値）", "T-CX-5-001");
    Ok(())
}

/// T-CX-5-002
#[test]
fn cx05_old_rates_show_their_date_and_expire_after_seven_days_three_days_old_en_has_its_date()
-> TestResult {
    let line = line_on("2026-10-17", YEN_SRC, Lang::Ja, YEN_EDITED)?;
    expect_note(&line, "(about $31,500, rate of Oct 17)", "T-CX-5-002");
    Ok(())
}

/// T-CX-5-003
#[test]
fn cx05_old_rates_show_their_date_and_expire_after_seven_days_seven_days_old_is_still_annotated()
-> TestResult {
    let line = line_on("2026-10-13", DOLLAR_SRC, Lang::En, DOLLAR_EDITED)?;
    expect_note(&line, "（約 15,900 円、10/13 の値）", "T-CX-5-003");
    Ok(())
}

/// T-CX-5-004
#[test]
fn cx05_old_rates_show_their_date_and_expire_after_seven_days_eight_days_old_is_skipped_as_stale_rate()
-> TestResult {
    let line = line_on("2026-10-12", DOLLAR_SRC, Lang::En, DOLLAR_EDITED)?;
    expect_stale(&line, "T-CX-5-004");
    Ok(())
}

/// T-CX-5-005
#[test]
fn cx05_old_rates_show_their_date_and_expire_after_seven_days_eight_days_old_en_is_skipped_as_stale_rate()
-> TestResult {
    let line = line_on("2026-10-12", YEN_SRC, Lang::Ja, YEN_EDITED)?;
    expect_stale(&line, "T-CX-5-005");
    Ok(())
}

/// T-CX-5-006
#[test]
fn cx05_old_rates_show_their_date_and_expire_after_seven_days_same_day_rate_is_annotated()
-> TestResult {
    let line = line_on("2026-10-20", DOLLAR_SRC, Lang::En, DOLLAR_EDITED)?;
    expect_note(&line, "（約 15,900 円、10/20 の値）", "T-CX-5-006");
    Ok(())
}

/// T-CX-5-007
#[test]
fn cx05_old_rates_show_their_date_and_expire_after_seven_days_stale_rate_still_annotates_the_mile_on_the_same_line()
-> TestResult {
    let line = line_on(
        "2026-10-12",
        "I paid $100 for 5 miles.",
        Lang::En,
        "5マイルで100ドル払いました。",
    )?;
    expect_note(&line, "（約 8.05 km）", "T-CX-5-007");
    assert!(
        line.skipped.iter().any(
            |skipped| skipped.kind == Kind::Currency && skipped.reason == SkipReason::StaleRate
        ),
        "T-CX-5-007: {:?}",
        line.skipped
    );
    Ok(())
}

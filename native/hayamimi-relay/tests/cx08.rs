//! CX-8: the note names the date of the rate, not the day it was fetched.
//!
//! 2026-10-16 is a Friday, 2026-10-19 a Monday, 2026-10-20 a Tuesday and
//! 2026-10-21 a Wednesday.

mod support;

use std::path::PathBuf;

use hayamimi_relay::fx::{self, AtomicFileStore, FetchOptions, Rates};
use hayamimi_relay::lang::Lang;
use hayamimi_relay::logging::NullSink;
use support::{FakeServer, Harness, Reply, TempDir, TestResult, ecb_xml, epoch};

const PATH: &str = "/stats/eurofxref/eurofxref-daily.xml";
const MONDAY_MORNING: &str = "2026-10-19T07:30:00+09:00";
const WEDNESDAY_MORNING: &str = "2026-10-21T07:30:00+09:00";

/// What the label check refuses.
#[derive(Debug)]
enum LabelFault {
    FetchDateShown,
    RateDateMissing,
}

/// The label must show the rate date (`10/16`, `Oct 16`) and not the fetch
/// date (`10/19`, `Oct 19`).
fn check_label_matches_rate_date(
    label: &str,
    rate_forms: [&str; 2],
    fetch_forms: [&str; 2],
) -> Result<(), LabelFault> {
    if fetch_forms.iter().any(|form| label.contains(form)) {
        return Err(LabelFault::FetchDateShown);
    }
    if rate_forms.iter().any(|form| label.contains(form)) {
        Ok(())
    } else {
        Err(LabelFault::RateDateMissing)
    }
}

/// The directory holds a rate file fetched at `now` from a server that
/// publishes `xml_date`.
struct Fetched {
    _dir: TempDir,
    out: PathBuf,
    rates: Rates,
}

fn fetch_on(xml_date: &str, now: &str) -> Result<Fetched, Box<dyn std::error::Error>> {
    let xml = ecb_xml(xml_date, "1.1181", "177.33");
    let server = FakeServer::start(move |_, _| Reply::xml(&xml))?;
    let dir = TempDir::new("cx08")?;
    let out = dir.join("fx_rates.json");
    let url = server.url(PATH);
    let rates = fx::fetch(
        &FetchOptions {
            source_url: &url,
            out: &out,
        },
        epoch(now)?,
        &AtomicFileStore,
        &mut NullSink,
    )?;
    Ok(Fetched {
        _dir: dir,
        out,
        rates,
    })
}

/// The only note added to the line, trimmed.
fn note_for(
    fetched: &Fetched,
    src: &str,
    lang: Lang,
    edited: &str,
    now: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut harness = Harness::open(Some(fetched.out.clone()), None)?;
    let line = harness.line(src, lang, edited, epoch(now)?);
    match line.annotations.as_slice() {
        [one] => Ok(one.text.trim().to_owned()),
        other => Err(format!("expected one annotation, got {}", other.len()).into()),
    }
}

/// T-CX-8-001
#[test]
fn cx08_weekend_shows_the_ecb_rate_date_monday_fetch_stores_the_friday_rate_date() -> TestResult {
    let fetched = fetch_on("2026-10-16", MONDAY_MORNING)?;
    assert_eq!(
        fetched.rates.rate_date, "2026-10-16",
        "T-CX-8-001: rate_date"
    );
    assert!(
        fetched.rates.fetched_at.starts_with("2026-10-19"),
        "T-CX-8-001: {}",
        fetched.rates.fetched_at
    );
    let on_disk = Rates::parse(&std::fs::read(&fetched.out)?)?;
    assert_eq!(
        on_disk.rate_date, "2026-10-16",
        "T-CX-8-001: file rate_date"
    );
    assert!(
        on_disk.fetched_at.starts_with("2026-10-19"),
        "T-CX-8-001: file fetched_at"
    );
    Ok(())
}

/// T-CX-8-002
#[test]
fn cx08_weekend_shows_the_ecb_rate_date_monday_ja_caption_names_friday() -> TestResult {
    let fetched = fetch_on("2026-10-16", MONDAY_MORNING)?;
    let note = note_for(
        &fetched,
        "I paid $100.",
        Lang::En,
        "私は100ドル払いました。",
        MONDAY_MORNING,
    )?;
    assert_eq!(note, "（約 15,900 円、10/16 の値）", "T-CX-8-002");
    assert!(!note.contains("10/19"), "T-CX-8-002: fetch date shown");
    Ok(())
}

/// T-CX-8-003
#[test]
fn cx08_weekend_shows_the_ecb_rate_date_monday_en_caption_names_friday() -> TestResult {
    let fetched = fetch_on("2026-10-16", MONDAY_MORNING)?;
    let note = note_for(
        &fetched,
        "予算は五百万円です。",
        Lang::Ja,
        "The budget is five million yen.",
        MONDAY_MORNING,
    )?;
    assert_eq!(note, "(about $31,500, rate of Oct 16)", "T-CX-8-003");
    assert!(!note.contains("Oct 19"), "T-CX-8-003: fetch date shown");
    Ok(())
}

/// T-CX-8-004
#[test]
fn cx08_weekend_shows_the_ecb_rate_date_weekday_fetch_shows_the_rate_date_not_the_fetch_date()
-> TestResult {
    let fetched = fetch_on("2026-10-20", WEDNESDAY_MORNING)?;
    let note = note_for(
        &fetched,
        "I paid $100.",
        Lang::En,
        "私は100ドル払いました。",
        WEDNESDAY_MORNING,
    )?;
    assert_eq!(note, "（約 15,900 円、10/20 の値）", "T-CX-8-004");
    assert!(!note.contains("10/21"), "T-CX-8-004: fetch date shown");
    Ok(())
}

/// T-CX-8-005
#[test]
fn cx08_weekend_shows_the_ecb_rate_date_fetch_date_label_is_rejected_by_the_check() -> TestResult {
    let rate_forms = ["10/16", "Oct 16"];
    let fetch_forms = ["10/19", "Oct 19"];
    for wrong in [
        "（約 15,900 円、10/19 の値）",
        "(about $31,500, rate of Oct 19)",
    ] {
        let verdict = check_label_matches_rate_date(wrong, rate_forms, fetch_forms);
        assert!(
            matches!(verdict, Err(LabelFault::FetchDateShown)),
            "T-CX-8-005: {wrong}: {verdict:?}"
        );
    }
    for right in [
        "（約 15,900 円、10/16 の値）",
        "(about $31,500, rate of Oct 16)",
    ] {
        let verdict = check_label_matches_rate_date(right, rate_forms, fetch_forms);
        assert!(verdict.is_ok(), "T-CX-8-005: {right}: {verdict:?}");
    }
    let missing = check_label_matches_rate_date("（約 15,900 円）", rate_forms, fetch_forms);
    assert!(
        matches!(missing, Err(LabelFault::RateDateMissing)),
        "T-CX-8-005: {missing:?}"
    );
    Ok(())
}

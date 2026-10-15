//! E2E-2: a process on the loopback path (en to ja) is within the budget.
//!
//! The acceptance test reads the records of a real run and starts nothing:
//! `docs/results/e2e/e2e2/e2e2.jsonl` is the Rust samples file, and the events
//! file `e2e2.events.jsonl` stands beside it (`support/e2e.rs`). It passes when the events hold a
//! `final` in en and a shown translation in ja, the valid samples (3.1) got a
//! translation at a rate of at least 20 / 23, and the `L` p95 is at most
//! 2000 ms. The p50 is reported and not judged.
//!
//!     cargo test --release --test e2e2 -- --ignored --nocapture
//!
//! The other tests run on records and fixtures kept in the repository: the
//! budget at its boundary, the event counts, a recorded run for a positive
//! control (an `/ingest` run, which gives the same columns), and a run with no
//! translation, which must fail.

mod support;

use std::path::Path;

use hayamimi_eval::compare::read_samples;
use hayamimi_eval::latency::Stat;
use support::e2e::{
    Columns, E2E2_P95_BUDGET_MS, RecordList, budget_failure, count_events, judge_e2e2,
    judge_e2e2_files, load_pooled, load_run, translated_after,
};
use support::{FIXTURES, RESULTS, TestResult, read};

/// One `final` in en and one shown translation in ja, in the form of an events file.
const EVENTS_BOTH: &str = concat!(
    r#"{"event":{"id":"a-1","lang":"en","text":"a","type":"final"},"t_us":1}"#,
    "\n",
    r#"{"event":{"id":"a-1","lang":"ja","of":"final","status":"ok","text":"あ","type":"translation"},"t_us":2}"#,
    "\n",
);

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(FIXTURES).join(name)
}

/// A column set whose `L` has the given p95 (the other figures are not judged).
fn columns_with_p95(p95_ms: i64) -> Columns {
    Columns {
        l: Some(Stat {
            n: 3,
            p50_ms: 900,
            p95_ms,
        }),
        l_asr: None,
        l_tr: None,
    }
}

#[test]
#[ignore = "reads the records in docs/results/e2e/e2e2/"]
fn e2e2_loopback_process_en_to_ja_within_budget() -> TestResult {
    let judged = judge_e2e2_files(&load_run("e2e2")?)?;
    println!("{}", judged.render("e2e2"));
    assert!(judged.failures.is_empty(), "{}", judged.render("e2e2"));
    Ok(())
}

#[test]
fn the_budget_is_2000_ms_and_the_boundary_passes() {
    assert_eq!(E2E2_P95_BUDGET_MS, 2000);
    assert_eq!(budget_failure(&columns_with_p95(1999)), None);
    assert_eq!(budget_failure(&columns_with_p95(2000)), None);
    let over = budget_failure(&columns_with_p95(2001));
    assert!(over.is_some_and(|why| why.contains("L p95 2001 ms > 2000 ms")));
}

#[test]
fn a_run_with_no_l_is_over_no_budget_and_still_fails() {
    let none = Columns {
        l: None,
        l_asr: None,
        l_tr: None,
    };
    assert!(budget_failure(&none).is_some());
}

/// A `final` in en counts, a shown translation in ja counts, and each of
/// these does not: a ja `final`, an en translation, a held translation, an
/// empty one, and the translation of a partial.
#[test]
fn the_events_are_counted_by_language_and_by_what_the_subtitle_shows() -> TestResult {
    let text = concat!(
        r#"{"event":{"lang":"en","text":"a","type":"final"},"t_us":1}"#,
        "\n",
        r#"{"event":{"lang":"en","text":"b","type":"final"},"t_us":2}"#,
        "\n",
        r#"{"event":{"lang":"ja","text":"c","type":"final"},"t_us":3}"#,
        "\n",
        r#"{"event":{"lang":"ja","of":"final","status":"ok","text":"あ","type":"translation"},"t_us":4}"#,
        "\n",
        r#"{"event":{"lang":"ja","of":"final","text":"い","type":"translation"},"t_us":5}"#,
        "\n",
        r#"{"event":{"lang":"en","of":"final","status":"ok","text":"x","type":"translation"},"t_us":6}"#,
        "\n",
        r#"{"event":{"lang":"ja","of":"final","status":"held","text":"う","type":"translation"},"t_us":7}"#,
        "\n",
        r#"{"event":{"lang":"ja","of":"final","status":"ok","text":"","type":"translation"},"t_us":8}"#,
        "\n",
        r#"{"event":{"lang":"ja","of":"partial","status":"ok","text":"え","type":"translation"},"t_us":9}"#,
        "\n\n",
    );
    let counts = count_events(text)?;
    assert_eq!((counts.en_finals, counts.ja_translations), (2, 2));
    Ok(())
}

#[test]
fn an_events_line_that_is_not_an_event_is_refused() {
    assert!(count_events("not json").is_err());
    assert!(count_events(r#"{"t_us":1}"#).is_err());
    assert_eq!(
        count_events("").map(|c| (c.en_finals, c.ja_translations)),
        Ok((0, 0))
    );
}

#[test]
fn a_fast_translated_run_with_both_languages_passes() -> TestResult {
    let base = read_samples(&fixture("e2e_columns.jsonl"))?;
    let rust = translated_after(&base, 100_000);
    let judged = judge_e2e2(&rust, EVENTS_BOTH)?;
    assert!(judged.failures.is_empty(), "{}", judged.render("e2e2"));
    Ok(())
}

/// `L` is 2300, 2000 and 1800 ms: the p95 is the highest, 2300 ms.
#[test]
fn a_run_whose_p95_is_over_2000_ms_fails_on_that_alone() -> TestResult {
    let base = read_samples(&fixture("e2e_columns.jsonl"))?;
    let rust = translated_after(&base, 1_000_000);
    let judged = judge_e2e2(&rust, EVENTS_BOTH)?;
    assert_eq!(judged.failures.len(), 1, "{:?}", judged.failures);
    assert!(judged.failures[0].contains("L p95 2300 ms > 2000 ms"));
    Ok(())
}

/// The positive control: the recorded `/ingest` run `e2e2c` gives its numbers
/// from the same function. Counted with `jq` on the same files: 30 valid, 25
/// translated; 38 `final` in en; 54 shown translations in ja.
#[test]
fn a_recorded_run_gives_its_columns_and_event_counts() -> TestResult {
    let dir = Path::new(RESULTS).join("r8h_verify");
    let rust = load_pooled(&[dir.join("e2e2c.jsonl")])?;
    let judged = judge_e2e2(&rust, &read(&dir.join("e2e2c.events.jsonl"))?)?;
    assert_eq!(
        (judged.rust.share.valid, judged.rust.share.translated),
        (30, 25)
    );
    assert!(judged.rust.columns.l.is_some());
    let counts = judged.events.map(|c| (c.en_finals, c.ja_translations));
    assert_eq!(counts, Some((38, 54)));
    let rendered = judged.render("e2e2");
    assert!(
        rendered.contains("e2e2 rust: 25 of 30 valid samples translated"),
        "{rendered}"
    );
    assert!(
        rendered.contains("38 final in en, 54 shown translation in ja"),
        "{rendered}"
    );
    Ok(())
}

/// The negative control: a run with no translation fails on the share, on
/// the budget (no `L`) and on the events (no shown translation in ja).
#[test]
fn a_run_with_no_translation_fails() -> TestResult {
    let list = RecordList {
        rust: fixture("e2e_no_translation.jsonl"),
        python: Vec::new(),
    };
    let judged = judge_e2e2_files(&list)?;
    let said = judged.failures.join("; ");
    assert!(
        said.contains("0 of 3 valid samples have a translation"),
        "{said}"
    );
    assert!(
        said.contains("rust: no valid sample has a translation"),
        "{said}"
    );
    assert!(said.contains("no shown translation in ja"), "{said}");
    assert!(!said.contains("no `final` in en"), "{said}");
    Ok(())
}

#[test]
fn an_empty_run_with_no_events_fails_on_every_count() -> TestResult {
    let judged = judge_e2e2(&[], "")?;
    let said = judged.failures.join("; ");
    assert!(said.contains("no valid sample"), "{said}");
    assert!(said.contains("hold no `final` in en"), "{said}");
    assert!(said.contains("hold no shown translation in ja"), "{said}");
    Ok(())
}

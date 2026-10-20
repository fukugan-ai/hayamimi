//! E2E-2: a process on the loopback path (en to ja) is within the budget.
//!
//! The acceptance test reads the records of a real run and starts nothing:
//! `docs/results/e2e/e2e2/e2e2.jsonl` is the Rust samples file, and the events
//! file `e2e2.events.jsonl` stands beside it (`support/e2e.rs`). It passes when the events hold a
//! `final` in en and a shown translation in ja, the valid samples (3.1) got a
//! translation at a rate of at least 20 / 23, and the `L` p95 is at most
//! 2000 ms. The p50 is reported and not judged. A Rust sample is valid against
//! the line of the rule that closed it, read from the `hayamimi` stdout beside
//! the samples.
//!
//!     cargo test --release --test e2e2 -- --ignored --nocapture
//!
//! The other tests run on records and fixtures kept in the repository: the
//! budget at its boundary, the event counts, a recorded run for a positive
//! control (an `/ingest` run, which gives the same columns), a run with no
//! translation, which must fail, and the lines of the rules (F88).

mod support;

use std::path::{Path, PathBuf};

use hayamimi_eval::compare::read_samples;
use hayamimi_eval::latency::Stat;
use support::e2e::{
    Columns, Counting, E2E2_P95_BUDGET_MS, Judged, RecordList, Side, append_newline,
    budget_failure, copy_record, count_events, judge_e2e2, judge_e2e2_files, load_run, load_rust,
    share_run, translated_after, write_f88,
};
use support::rp32::scratch_dir;
use support::{FIXTURES, Failure, REPO, RESULTS, TestResult, read};

/// One `final` in en and one shown translation in ja, in the form of an events file.
const EVENTS_BOTH: &str = concat!(
    r#"{"event":{"id":"a-1","lang":"en","text":"a","type":"final"},"t_us":1}"#,
    "\n",
    r#"{"event":{"id":"a-1","lang":"ja","of":"final","status":"ok","text":"あ","type":"translation"},"t_us":2}"#,
    "\n",
);

fn fixture(name: &str) -> PathBuf {
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

/// E2E-2 over the F88 run `samples`, which has its events beside it.
fn judge_f88(samples: &Path) -> Result<Judged, Failure> {
    let list = RecordList {
        rust: samples.to_path_buf(),
        python: Vec::new(),
    };
    judge_e2e2_files(Path::new(REPO), &list)
}

/// The refusal of `judged`, which has to name every one of `names`.
fn assert_refused_naming(judged: &Result<Judged, Failure>, names: &[&str]) {
    assert!(
        judged
            .as_ref()
            .err()
            .is_some_and(|why| names.iter().all(|name| why.contains(name))),
        "{judged:?}"
    );
}

#[test]
#[ignore = "reads the records in docs/results/e2e/e2e2/"]
fn e2e2_loopback_process_en_to_ja_within_budget() -> TestResult {
    let judged = judge_e2e2_files(Path::new(REPO), &load_run("e2e2")?)?;
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
    let rust = Side::flat(translated_after(&base, 100_000));
    let judged = judge_e2e2(&rust, EVENTS_BOTH)?;
    assert!(judged.failures.is_empty(), "{}", judged.render("e2e2"));
    Ok(())
}

/// `L` is 2300, 2000 and 1800 ms: the p95 is the highest, 2300 ms.
#[test]
fn a_run_whose_p95_is_over_2000_ms_fails_on_that_alone() -> TestResult {
    let base = read_samples(&fixture("e2e_columns.jsonl"))?;
    let rust = Side::flat(translated_after(&base, 1_000_000));
    let judged = judge_e2e2(&rust, EVENTS_BOTH)?;
    assert_eq!(judged.failures.len(), 1, "{:?}", judged.failures);
    assert!(judged.failures[0].contains("L p95 2300 ms > 2000 ms"));
    Ok(())
}

/// The positive control: the recorded `/ingest` run `e2e2c` gives its numbers
/// from the same function. Counted with `jq` on the same files: 30 valid, 25
/// translated; 38 `final` in en; 54 shown translations in ja. `e2e2c` is a
/// listed record of a build without the early rule and has no stdout, so every
/// sample is counted at `min_silence`.
#[test]
fn a_recorded_run_gives_its_columns_and_event_counts() -> TestResult {
    let dir = Path::new(RESULTS).join("r8h_verify");
    let rust = load_rust(Path::new(REPO), &dir.join("e2e2c.jsonl"))?;
    let judged = judge_e2e2(&rust, &read(&dir.join("e2e2c.events.jsonl"))?)?;
    assert_eq!(
        (judged.rust.share.valid, judged.rust.share.translated),
        (30, 25)
    );
    assert_eq!(judged.rust.counting, Counting::Listed);
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
    assert!(
        rendered.contains("every sample counted at min_silence"),
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
    let judged = judge_e2e2_files(Path::new(REPO), &list)?;
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

/// The edge of the translated share, 20 / 23: 20 of 23 passes, 19 of 23 does
/// not. `L` (1400 ms) and the events are fixed inside the budget, so only the
/// share can fail.
#[test]
fn the_translated_share_boundary_is_20_of_23() -> TestResult {
    let at_20 = judge_e2e2(&Side::flat(share_run(23, 20)), EVENTS_BOTH)?;
    assert!(at_20.failures.is_empty(), "{:?}", at_20.failures);
    let at_19 = judge_e2e2(&Side::flat(share_run(23, 19)), EVENTS_BOTH)?;
    assert_eq!(at_19.failures.len(), 1, "{:?}", at_19.failures);
    assert!(at_19.failures[0].contains("19 of 23 valid samples have a translation"));
    Ok(())
}

#[test]
fn an_empty_run_with_no_events_fails_on_every_count() -> TestResult {
    let judged = judge_e2e2(&Side::flat(Vec::new()), "")?;
    let said = judged.failures.join("; ");
    assert!(said.contains("no valid sample"), "{said}");
    assert!(said.contains("hold no `final` in en"), "{said}");
    assert!(said.contains("hold no shown translation in ja"), "{said}");
    Ok(())
}

/// T-E2E-2-012: the listed record `e2e2` has no stdout, so every sample is
/// counted at `min_silence`: 75 valid, 70 translated, and the output says so.
#[test]
fn the_listed_e2e2_run_is_counted_at_min_silence_without_a_stdout() -> TestResult {
    let root = scratch_dir("e2e2-012", "root")?;
    let rust = copy_record(&root, "e2e/e2e2/e2e2.jsonl")?;
    copy_record(&root, "e2e/e2e2/e2e2.events.jsonl")?;
    let list = RecordList {
        rust,
        python: Vec::new(),
    };
    let judged = judge_e2e2_files(&root, &list)?;
    assert_eq!(
        (judged.rust.share.valid, judged.rust.share.translated),
        (75, 70)
    );
    assert_eq!(judged.rust.counting, Counting::Listed);
    let rendered = judged.render("e2e2");
    assert!(
        rendered.contains("no hayamimi stdout") && rendered.contains("counted at min_silence"),
        "{rendered}"
    );
    Ok(())
}

/// T-E2E-2-013: the lines of the rules and the missing stdout are judged here
/// as in E2E-1: F88 closed by the early rule is valid, by the silence rule a
/// bad end, and a run off the list with no stdout is refused by its name.
#[test]
fn the_per_sample_line_and_a_missing_stdout_are_judged_in_e2e2_as_in_e2e1() -> TestResult {
    let early = write_f88("e2e2-t013-early", &["mic-1"], Some(&[(1, "early")]))?;
    let judged = judge_f88(&early)?;
    assert_eq!((judged.rust.share.valid, judged.rust.bad_end), (1, 0));
    let silence = write_f88("e2e2-t013-silence", &["mic-1"], Some(&[(1, "silence")]))?;
    let judged = judge_f88(&silence)?;
    assert_eq!((judged.rust.share.valid, judged.rust.bad_end), (0, 1));
    let bare = write_f88("e2e2-t013-bare", &["mic-1"], None)?;
    assert_refused_naming(&judge_f88(&bare), &["f88", "hayamimi.stdout.log"]);
    Ok(())
}

/// T-E2E-2-014: the L9 record has 39 samples closed by the early rule. By the
/// line of each rule 74 samples are valid and the p95 of `L` is 1318 ms; at
/// `min_silence` for every sample 35 are valid and the p95 is 2879 ms. Both are
/// written.
#[test]
fn the_l9_record_counts_74_samples_by_the_per_sample_line_and_35_by_300_ms() -> TestResult {
    let rust = Path::new(RESULTS).join("r8x").join("e1pre.e2e2.jsonl");
    let list = RecordList {
        rust,
        python: Vec::new(),
    };
    let judged = judge_e2e2_files(Path::new(REPO), &list)?;
    assert_eq!(judged.rust.counting, Counting::PerSample);
    let own = judged.rust.columns.l.map(|l| (l.n, l.p95_ms));
    assert_eq!(own, Some((74, 1318)));
    let flat = judged
        .rust
        .at_min_silence
        .and_then(|counted| counted.columns.l)
        .map(|l| (l.n, l.p95_ms));
    assert_eq!(flat, Some((35, 2879)));
    let rendered = judged.render("e1pre.e2e2");
    assert!(rendered.contains("p95 1318 ms (n 74)"), "{rendered}");
    assert!(rendered.contains("p95 2879 ms (n 35)"), "{rendered}");
    Ok(())
}

/// T-E2E-2-015: `e2e2c` of `r8h_verify` with one newline added is not the
/// listed record. The unchanged copy passes, which is the control.
#[test]
fn a_listed_name_with_another_sha256_is_not_exempt_in_e2e2() -> TestResult {
    let kept = scratch_dir("e2e2-015", "kept")?;
    let unchanged = copy_record(&kept, "r8h_verify/e2e2c.jsonl")?;
    copy_record(&kept, "r8h_verify/e2e2c.events.jsonl")?;
    assert!(load_rust(&kept, &unchanged).is_ok());
    let root = scratch_dir("e2e2-015", "changed")?;
    let changed = copy_record(&root, "r8h_verify/e2e2c.jsonl")?;
    copy_record(&root, "r8h_verify/e2e2c.events.jsonl")?;
    append_newline(&changed)?;
    let list = RecordList {
        rust: changed,
        python: Vec::new(),
    };
    assert_refused_naming(
        &judge_e2e2_files(&root, &list),
        &["e2e2c", "hayamimi.stdout.log"],
    );
    Ok(())
}

/// T-E2E-2-016: the three inputs that E2E-1 refuses (T-E2E-1-021 to 023) are
/// refused here too, with the run and the ids named. The same rule on both
/// `final`s is the control.
#[test]
fn the_three_rejected_rule_shapes_are_rejected_in_e2e2_as_in_e2e1() -> TestResult {
    let ids = ["mic-1", "mic-2"];
    let split = write_f88(
        "e2e2-t016-split",
        &ids,
        Some(&[(1, "early"), (2, "silence")]),
    )?;
    assert_refused_naming(&judge_f88(&split), &["f88", "mic-1", "mic-2"]);
    let missing = write_f88("e2e2-t016-missing", &["mic-1"], Some(&[(9, "early")]))?;
    assert_refused_naming(&judge_f88(&missing), &["f88", "mic-1"]);
    let twice = write_f88(
        "e2e2-t016-twice",
        &["mic-1"],
        Some(&[(1, "early"), (1, "early")]),
    )?;
    assert_refused_naming(&judge_f88(&twice), &["f88", "id 1"]);
    let same = write_f88("e2e2-t016-same", &ids, Some(&[(1, "early"), (2, "early")]))?;
    assert_eq!(judge_f88(&same)?.rust.share.valid, 1);
    Ok(())
}

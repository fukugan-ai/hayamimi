//! BR-3: the budget of the wait from the end of speech to the page (design 10.13.3).
//!
//! The real run (T-BR-03-001) plays `lat-dialog` three times through the stack
//! of IP-2 with the translation server the window's unit starts (port 18084),
//! with a page open that times itself (BR-2), and puts the stages of BR-1 and
//! the page stage together. It is `#[ignore]`d because it needs the models,
//! `testdata/` (not in git), the page bundle, a browser and that server. Until
//! the speed measures of B2 are in (`JUDGE_AFTER_B2`), it writes the values to
//! `docs/results/in-person/br03_baseline.md` and judges nothing.
//!
//! The rows 003 to 016 test the function that judges the budget, on waits
//! written by hand; the row 002 reads a recorded run of IP-2.

mod support;

use std::path::{Path, PathBuf};

use hayamimi_eval::latency::{Stage, stage_report};
use support::dialog::{Wait, join_page, judge_budget, render_baseline};
use support::dialog_run::{LLAMA_PORT, Playback, Translator, run_paged};
use support::{Failure, REPO, TestResult, find_above, read};

/// Whether the run judges the budget. The speed measures of B2 (design 10.13.4,
/// IP-S7 and IP-S8) are not in, so the lines are recorded and not applied.
const JUDGE_AFTER_B2: bool = false;
/// Where the values of the real run are written.
const BASELINE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/results/in-person/br03_baseline.md"
);
/// How many times the real run plays `lat-dialog`.
const REPEATS: u32 = 3;

fn scratch() -> Result<PathBuf, Failure> {
    let work = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("br03");
    std::fs::create_dir_all(&work).map_err(|error| format!("{}: {error}", work.display()))?;
    Ok(work)
}

/// T-BR-03-001
#[test]
#[ignore = "starts the hayamimi binary, the relay and chrome-headless-shell, and needs the translation server on port 18084"]
fn br03_the_wait_from_the_end_of_speech_to_the_page_is_inside_the_budget() -> TestResult {
    let work = scratch()?;
    let wav = find_above("testdata/lat-dialog.wav")?;
    let manifest = find_above("testdata/manifests/lat-dialog.json")?;
    let run = run_paged(
        &Translator::Real(LLAMA_PORT),
        &Playback {
            wav: &wav,
            manifest: Some(&manifest),
            repeat: REPEATS,
        },
        &work,
        false,
    )?;
    let samples = read(&run.played)?;
    let events = read(&run.played.with_extension("events.jsonl"))?;
    let report = stage_report(&samples, &events).map_err(|error| error.to_string())?;
    let joined = join_page(&report, &run.with_relay.stamps)?;
    std::fs::write(BASELINE, render_baseline(&report, &joined)?)
        .map_err(|error| format!("{BASELINE}: {error}"))?;
    if JUDGE_AFTER_B2 {
        judge_budget(&joined.warm, &joined.cold_l_page_ms)?;
    }
    Ok(())
}

/// T-BR-03-002
#[test]
fn the_warm_stage_percentiles_of_the_pair_record_are_the_hand_counted_ones() -> TestResult {
    let record = Path::new(REPO).join("docs/results/in-person");
    let samples = read(&record.join("ip02_pair.jsonl"))?;
    let events = read(&record.join("ip02_pair.events.jsonl"))?;
    let report = stage_report(&samples, &events)?;
    assert_eq!(report.warm.len(), 76);
    let stat = |stage: Stage| -> Result<(i64, i64), Failure> {
        let found = report.stat(stage).map_err(|error| error.to_string())?;
        let found = found.ok_or_else(|| format!("no warm value for {}", stage.name()))?;
        Ok((found.p50_ms, found.p95_ms))
    };
    assert_eq!(stat(Stage::Wait)?, (445, 1081));
    assert_eq!(stat(Stage::Decode)?, (268, 472));
    assert_eq!(stat(Stage::Draft)?, (221, 363));
    let cold: Vec<(u32, u32)> = report
        .cold
        .iter()
        .map(|row| (row.repeat, row.utt))
        .collect();
    assert_eq!(cold, [(1, 1), (1, 2)]);
    Ok(())
}

/// A wait whose stages are inside their lines, apart from the two the row varies.
fn wait(l_page_ms: i64, l_settled_page_ms: i64) -> Wait {
    Wait {
        d_ms: 200,
        dec_ms: 200,
        tr_ms: 50,
        l_page_ms,
        l_settled_page_ms,
    }
}

/// Twenty warm waits: `first` of them at `common`, the rest at `last`.
fn twenty(common: i64, last: i64, others: usize) -> Vec<i64> {
    let mut values = vec![common; 20 - others];
    values.extend(vec![last; others]);
    values
}

fn page_waits_of(values: &[i64]) -> Vec<Wait> {
    values.iter().map(|value| wait(*value, 1000)).collect()
}

fn settled_waits_of(values: &[i64]) -> Vec<Wait> {
    values.iter().map(|value| wait(700, *value)).collect()
}

fn says(result: Result<String, Failure>, words: &str) -> bool {
    result.err().is_some_and(|text| text.contains(words))
}

/// T-BR-03-003
#[test]
fn a_page_wait_p50_of_750_ms_is_on_the_line() {
    let warm = page_waits_of(&twenty(750, 750, 0));
    assert!(judge_budget(&warm, &[1000, 1000]).is_ok());
}

/// T-BR-03-004
#[test]
fn a_page_wait_p50_of_751_ms_is_over_the_line() {
    let warm = page_waits_of(&twenty(751, 751, 0));
    let said = judge_budget(&warm, &[1000, 1000]);
    assert!(says(said, "`L_page` の p50 が 751 ms で線 750 ms の外"));
}

/// T-BR-03-005
#[test]
fn a_page_wait_p95_of_1300_ms_is_on_the_line() {
    let warm = page_waits_of(&twenty(700, 1300, 2));
    assert!(judge_budget(&warm, &[1000, 1000]).is_ok());
}

/// T-BR-03-006
#[test]
fn a_page_wait_p95_of_1301_ms_is_over_the_line() {
    let warm = page_waits_of(&twenty(700, 1301, 2));
    let said = judge_budget(&warm, &[1000, 1000]);
    assert!(says(said, "`L_page` の p95 が 1301 ms で線 1300 ms の外"));
}

/// T-BR-03-007
#[test]
fn the_largest_sample_does_not_decide_the_page_wait_p95() {
    let mut values = twenty(700, 700, 0);
    values[18] = 1300;
    values[19] = 5000;
    assert!(judge_budget(&page_waits_of(&values), &[1000, 1000]).is_ok());
}

/// T-BR-03-008
#[test]
fn a_settled_page_wait_p50_of_1100_ms_is_on_the_line() {
    assert!(judge_budget(&settled_waits_of(&twenty(1100, 1100, 0)), &[1000]).is_ok());
}

/// T-BR-03-009
#[test]
fn a_settled_page_wait_p50_of_1101_ms_is_over_the_line() {
    let said = judge_budget(&settled_waits_of(&twenty(1101, 1101, 0)), &[1000]);
    assert!(says(
        said,
        "`L_settled_page` の p50 が 1101 ms で線 1100 ms の外"
    ));
}

/// T-BR-03-010
#[test]
fn a_settled_page_wait_p95_of_1700_ms_is_on_the_line() {
    assert!(judge_budget(&settled_waits_of(&twenty(1000, 1700, 2)), &[1000]).is_ok());
}

/// T-BR-03-011
#[test]
fn a_settled_page_wait_p95_of_1701_ms_is_over_the_line() {
    let said = judge_budget(&settled_waits_of(&twenty(1000, 1701, 2)), &[1000]);
    assert!(says(
        said,
        "`L_settled_page` の p95 が 1701 ms で線 1700 ms の外"
    ));
}

/// T-BR-03-012
#[test]
fn a_cold_sample_of_2500_ms_is_on_the_line() {
    let warm = page_waits_of(&twenty(700, 700, 0));
    assert!(judge_budget(&warm, &[2500, 100]).is_ok());
}

/// T-BR-03-013
#[test]
fn a_cold_sample_of_2501_ms_is_over_the_line() {
    let warm = page_waits_of(&twenty(700, 700, 0));
    let said = judge_budget(&warm, &[2501, 100]);
    assert!(says(
        said,
        "冷えた標本の `L_page` の最大が 2501 ms で線 2500 ms の外"
    ));
}

/// T-BR-03-014
#[test]
fn a_cold_sample_is_kept_out_of_the_warm_percentiles() {
    let warm = page_waits_of(&twenty(700, 700, 0));
    assert!(judge_budget(&warm, &[2400, 100]).is_ok());
    // Two cold samples at 2400 ms: mixed into 22 waits, the 21st is 2400 ms and
    // passes the p95 line of 1300 ms; kept apart they only meet the cold line.
    assert!(judge_budget(&warm, &[2400, 2400]).is_ok());
}

/// T-BR-03-015
#[test]
fn a_stage_outside_its_budget_is_recorded_and_does_not_fail_the_run() -> TestResult {
    let warm: Vec<Wait> = (0..20)
        .map(|_| Wait {
            d_ms: 301,
            dec_ms: 200,
            tr_ms: 50,
            l_page_ms: 700,
            l_settled_page_ms: 1000,
        })
        .collect();
    let record = judge_budget(&warm, &[1000, 1000])?;
    let wait_line = record
        .lines()
        .find(|line| line.starts_with("d:"))
        .ok_or("the record has no line for the wait")?;
    assert!(
        wait_line.contains("301") && wait_line.contains("線の外"),
        "{record}"
    );
    Ok(())
}

/// T-BR-03-016
#[test]
fn a_run_with_no_warm_sample_cannot_pass_the_budget() {
    let said = judge_budget(&[], &[1000, 1000]);
    assert!(says(said, "温まった標本 0"));
}

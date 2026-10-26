//! BR-3: the budget of the wait from the end of speech to the page (design 10.13.3).
//!
//! The real run (T-BR-03-001) plays `lat-dialog` three times through the stack
//! of IP-2 with the translation server the window's unit starts (port 18084),
//! with a page open that times itself (BR-2), and puts the stages of BR-1 and
//! the page stage together. It is `#[ignore]`d because it needs the models,
//! `testdata/` (not in git), the page bundle, a browser and that server. Until
//! the speed measures of B2 are in (`JUDGE_AFTER_B2`), it writes the values and
//! the files of the run to `docs/results/in-person/<step>.*` (each step of the
//! speed work has files of its own) and judges nothing.
//!
//! Two more real runs (T-BR-03-017 and 018) start the programs with the
//! measures of IP-S8 and of IP-S7 switched off by their flags, so that the
//! values of the programs as they are can be set beside the values without each
//! measure, taken on the same machine.
//!
//! The rows 003 to 016 test the function that judges the budget, on waits
//! written by hand; the row 002 reads a recorded run of IP-2. The rows
//! T-BR-04-015 to 017 test the record's split of the draft translation by
//! language, on utterances written by hand, and the rows T-BR-04-018 to 022
//! the line of the draft translation, on the same kind of utterances. The row
//! T-BR-04-009 reads the recorded run with IP-S7 and IP-S8 on.

mod support;

use std::path::{Path, PathBuf};

use hayamimi_eval::latency::{Stage, StageReport, StageRow, stage_report};
use support::dialog::{
    Wait, draft_by_language_text, join_page, judge_budget, render_baseline, spoken_in,
};
use support::dialog_run::{Flags, LLAMA_PORT, Playback, Translator, run_paged_with};
use support::{Failure, REPO, TestResult, find_above, read};

/// Whether the run judges the budget. The speed measures of B2 (design 10.13.4,
/// IP-S7 and IP-S8) are not in, so the lines are recorded and not applied.
const JUDGE_AFTER_B2: bool = false;
/// How many times the real run plays `lat-dialog`.
const REPEATS: u32 = 3;

/// How a real run starts the two programs, and the files its record goes to
/// (design 10.13.9). Each step has files of its own, so the values of the steps
/// before it stay.
struct Variant {
    step: &'static str,
    title: &'static str,
    flags: Flags<'static>,
    /// Whether the run is judged against the line of the draft translation
    /// (see [`judge_draft_p50`]): only the run that has IP-S7 on is.
    judges_draft: bool,
}

/// The programs as they are, with IP-S7 and IP-S8 on.
const CURRENT: Variant = Variant {
    step: "br03_ip_s8",
    title: "BR-3 の IP-S7 と IP-S8 の後の値",
    flags: Flags {
        hayamimi: &[],
        relay: &[],
    },
    judges_draft: true,
};

/// IP-S7 on and IP-S8 off.
const LID_ONLY: Variant = Variant {
    step: "br03_ip_s8_lid_only",
    title: "BR-3 の IP-S7 だけの値（IP-S8 を旗で外した run）",
    flags: Flags {
        hayamimi: &[],
        relay: &["--partial-yield", "off"],
    },
    judges_draft: false,
};

/// IP-S7 and IP-S8 both off: the programs as they were before IP-S7.
const BOTH_OFF: Variant = Variant {
    step: "br03_ip_s8_both_off",
    title: "BR-3 の IP-S7 と IP-S8 を旗で外した値",
    flags: Flags {
        hayamimi: &["--partial-lid", "off"],
        relay: &["--partial-yield", "off"],
    },
    judges_draft: false,
};

/// The file of the variant's record that ends in `suffix`.
fn record_of(variant: &Variant, suffix: &str) -> PathBuf {
    Path::new(REPO)
        .join("docs/results/in-person")
        .join(format!("{}{suffix}", variant.step))
}

/// Writes `bytes` to `path`, naming the path when it cannot.
fn keep(path: &Path, bytes: &[u8]) -> Result<(), Failure> {
    std::fs::write(path, bytes).map_err(|error| format!("{}: {error}", path.display()))
}

fn scratch() -> Result<PathBuf, Failure> {
    let work = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("br03");
    std::fs::create_dir_all(&work).map_err(|error| format!("{}: {error}", work.display()))?;
    Ok(work)
}

/// The events of the two logs of the run that the speed work reads: where the
/// detector closed a segment and how far it lagged, the question put to the
/// identifier in the middle of an utterance, a wait for a slot of the
/// translation server, a text in progress let go, and the translation of a text
/// in progress taken over by the line that closed it.
const EVENTS_KEPT: [&str; 5] = [
    "asr.vad.closed",
    "asr.partial.lang.asked",
    "translate.slot.waited",
    "translate.partial.skipped",
    "translate.draft.reused",
];

/// The lines of the logs of the run in `work` that name one of [`EVENTS_KEPT`].
fn kept_lines(work: &Path) -> Result<String, Failure> {
    let mut kept = String::new();
    for name in ["hayamimi.log", "relay.log"] {
        let text = read(&work.join(name))?;
        for line in text.lines() {
            if EVENTS_KEPT.iter().any(|event| line.contains(event)) {
                kept.push_str(line);
                kept.push('\n');
            }
        }
    }
    Ok(kept)
}

/// The line of the draft translation of a pair run: the `L_tr` of the warm
/// utterances has a p50 no greater than this (design 10.13.4, hand 1).
const DRAFT_P50_LINE_MS: i64 = 100;

/// T-BR-04-009: the p50 of the draft translation is inside its line, and each
/// language of the pair has a warm value of its own. A report with no warm
/// utterance, or in which a language has none, is no pass.
fn judge_draft_p50(report: &StageReport) -> Result<String, Failure> {
    let found = report
        .stat(Stage::Draft)
        .map_err(|error| error.to_string())?;
    let Some(all) = found else {
        return Err(String::from("no warm value for L_tr"));
    };
    if all.p50_ms > DRAFT_P50_LINE_MS {
        return Err(format!(
            "L_tr p50 is {} ms, over the line of {DRAFT_P50_LINE_MS} ms",
            all.p50_ms
        ));
    }
    for lang in ["ja", "en"] {
        let kept = spoken_in(report, lang)
            .stat(Stage::Draft)
            .map_err(|error| error.to_string())?;
        if kept.is_none() {
            return Err(format!("no warm value for L_tr in {lang}"));
        }
    }
    Ok(format!(
        "L_tr p50 {} ms, inside the line of {DRAFT_P50_LINE_MS} ms",
        all.p50_ms
    ))
}

/// Plays `lat-dialog` through the stack started the way `variant` says and
/// keeps the record of the run under the variant's step.
fn record_run(variant: &Variant) -> TestResult {
    let work = scratch()?;
    let wav = find_above("testdata/lat-dialog.wav")?;
    let manifest = find_above("testdata/manifests/lat-dialog.json")?;
    let run = run_paged_with(
        &Translator::Real(LLAMA_PORT),
        &Playback {
            wav: &wav,
            manifest: Some(&manifest),
            repeat: REPEATS,
        },
        &variant.flags,
        &work,
        false,
    )?;
    let samples = read(&run.played)?;
    let events = read(&run.played.with_extension("events.jsonl"))?;
    let report = stage_report(&samples, &events).map_err(|error| error.to_string())?;
    let joined = join_page(&report, &run.with_relay.stamps)?;
    keep(
        &record_of(variant, ".md"),
        render_baseline(variant.title, &report, &joined)?.as_bytes(),
    )?;
    keep(&record_of(variant, ".jsonl"), samples.as_bytes())?;
    keep(&record_of(variant, ".events.jsonl"), events.as_bytes())?;
    keep(
        &record_of(variant, ".log.jsonl"),
        kept_lines(&work)?.as_bytes(),
    )?;
    if JUDGE_AFTER_B2 {
        judge_budget(&joined.warm, &joined.cold_l_page_ms)?;
    }
    if variant.judges_draft {
        judge_draft_p50(&report)?;
    }
    Ok(())
}

/// T-BR-03-001
#[test]
#[ignore = "starts the hayamimi binary, the relay and chrome-headless-shell, and needs the translation server on port 18084"]
fn br03_the_wait_from_the_end_of_speech_to_the_page_is_inside_the_budget() -> TestResult {
    record_run(&CURRENT)
}

/// T-BR-03-017
#[test]
#[ignore = "starts the hayamimi binary, the relay and chrome-headless-shell, and needs the translation server on port 18084"]
fn the_run_with_the_final_translation_not_yielding_is_recorded() -> TestResult {
    record_run(&LID_ONLY)
}

/// T-BR-03-018
#[test]
#[ignore = "starts the hayamimi binary, the relay and chrome-headless-shell, and needs the translation server on port 18084"]
fn the_run_with_both_measures_switched_off_is_recorded() -> TestResult {
    record_run(&BOTH_OFF)
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

/// T-BR-04-009
#[test]
fn the_warm_pair_run_has_its_draft_translation_p50_inside_100_ms_and_split_by_language()
-> TestResult {
    let record = Path::new(REPO).join("docs/results/in-person");
    let samples = read(&record.join("br03_ip_s8.jsonl"))?;
    let events = read(&record.join("br03_ip_s8.events.jsonl"))?;
    let report = stage_report(&samples, &events).map_err(|error| error.to_string())?;
    assert_eq!(report.warm.len(), 76);
    judge_draft_p50(&report)?;
    assert_eq!(
        draft_by_language_text(&report)?,
        "L_tr ja: n 32, p50 139 ms, p95 329 ms\nL_tr en: n 44, p50 4 ms, p95 258 ms\n"
    );
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
    // is over the p95 line of 1300 ms; kept apart they only meet the cold line.
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

/// An utterance spoken in `lang` whose draft translation took `tr_ms`.
fn spoken(lang: &str, tr_ms: i64) -> StageRow {
    StageRow {
        repeat: 1,
        utt: 1,
        lang: Some(lang.to_owned()),
        final_id: None,
        d_us: None,
        dec_us: None,
        tr_us: Some(tr_ms * 1000),
        settle_us: None,
        l_us: None,
        settled_wait_us: None,
    }
}

fn report_of(cold: Vec<StageRow>, warm: Vec<StageRow>) -> StageReport {
    StageReport { cold, warm }
}

/// T-BR-04-015
#[test]
fn the_draft_translation_is_given_for_each_language_with_its_count() -> TestResult {
    let warm = vec![
        spoken("ja", 100),
        spoken("ja", 200),
        spoken("ja", 300),
        spoken("en", 50),
        spoken("en", 60),
    ];
    let text = draft_by_language_text(&report_of(Vec::new(), warm))?;
    assert_eq!(
        text,
        "L_tr ja: n 3, p50 200 ms, p95 300 ms\nL_tr en: n 2, p50 50 ms, p95 60 ms\n"
    );
    Ok(())
}

/// T-BR-04-016
#[test]
fn a_language_with_no_warm_utterance_is_said_to_have_no_value() -> TestResult {
    let warm = vec![spoken("ja", 100), spoken("ja", 200)];
    let text = draft_by_language_text(&report_of(Vec::new(), warm))?;
    let [ja, en] = text.lines().collect::<Vec<_>>()[..] else {
        return Err(format!("two lines were expected: {text}").into());
    };
    assert!(ja.starts_with("L_tr ja: n 2, p50 100 ms"), "{text}");
    assert_eq!(en, "L_tr en: 温まった標本が 0 件で、値は無い");
    Ok(())
}

/// T-BR-04-017
#[test]
fn a_cold_utterance_is_kept_out_of_the_split_by_language() -> TestResult {
    let cold = vec![spoken("en", 999)];
    let warm = vec![spoken("ja", 100), spoken("ja", 200)];
    let text = draft_by_language_text(&report_of(cold, warm))?;
    assert!(text.contains("L_tr ja: n 2"), "{text}");
    assert!(text.contains("L_tr en: 温まった標本が 0 件"), "{text}");
    assert!(!text.contains("999"), "{text}");
    Ok(())
}

/// T-BR-04-018
#[test]
fn a_draft_translation_p50_inside_the_line_passes_when_both_languages_have_a_value() -> TestResult {
    let warm = vec![
        spoken("ja", 150),
        spoken("ja", 60),
        spoken("en", 4),
        spoken("en", 5),
        spoken("en", 6),
    ];
    let said = judge_draft_p50(&report_of(Vec::new(), warm))?;
    assert!(said.contains("p50 6 ms"), "{said}");
    Ok(())
}

/// T-BR-04-019
#[test]
fn a_draft_translation_p50_over_the_line_fails() {
    let warm = vec![
        spoken("ja", 150),
        spoken("ja", 160),
        spoken("ja", 170),
        spoken("en", 101),
        spoken("en", 102),
    ];
    assert!(says(
        judge_draft_p50(&report_of(Vec::new(), warm)),
        "over the line"
    ));
}

/// T-BR-04-020
#[test]
fn a_draft_translation_p50_of_100_ms_is_on_the_line_and_101_ms_is_over_it() -> TestResult {
    let on = vec![spoken("ja", 100), spoken("en", 100)];
    judge_draft_p50(&report_of(Vec::new(), on))?;
    let over = vec![spoken("ja", 101), spoken("en", 101)];
    assert!(says(
        judge_draft_p50(&report_of(Vec::new(), over)),
        "101 ms"
    ));
    Ok(())
}

/// T-BR-04-021
#[test]
fn a_run_with_no_warm_draft_translation_cannot_pass_the_draft_line() {
    assert!(says(
        judge_draft_p50(&report_of(Vec::new(), Vec::new())),
        "no warm value for L_tr"
    ));
}

/// T-BR-04-022
#[test]
fn a_run_in_which_one_language_has_no_warm_draft_translation_cannot_pass() {
    let warm = vec![spoken("ja", 10), spoken("ja", 20)];
    assert!(says(
        judge_draft_p50(&report_of(Vec::new(), warm)),
        "no warm value for L_tr in en"
    ));
}

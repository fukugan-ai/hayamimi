//! RP-32: the share of valid samples that got a translation.
//!
//! A run whose translations the checker stops leaves those samples out of
//! `L`, so a small `L` over the rest proves nothing. The share is counted over
//! the valid samples (3.1: a `final` is there and `L_asr - L_dec` reaches the
//! run's `--min-silence`), with the same validity function RP-3 uses, and it
//! must reach 20 / 23.

mod support;

use std::collections::BTreeMap;
use std::path::Path;

use hayamimi_asr::endpoint::Rule;
use hayamimi_asr::vad::EARLY_MIN_SILENCE_MS;
use hayamimi_eval::compare::{
    TranslatedShare, check_percentiles, check_translated_share, comparison_threshold, read_samples,
    translated_share,
};
use hayamimi_eval::latency::{Sample, Stat, Window, build_samples, parse_event};
use serde_json::Value;
use support::e2e::columns_of;
use support::rp32::{
    AltReport, AltRun, Implementation, LOAD_OK, SHARE_COLUMN_PYTHON, SHARE_COLUMN_RUST,
    alternated_report, check_alternated_runs, check_columns_written, check_dir, check_ports_held,
    check_share_columns_written, f32_sample, l_comparison_set, load_alternated, load_hundredths,
    load_runs, missing_files, missing_rule_files, pairs_to_read, parse_load_file, parse_pairs_text,
    render_columns, render_line_counts, render_share_columns, rp32_dir, scratch_dir, write_f32_dir,
};
use support::rp32_rules::{closing_rules, decided_rules, rule_named, silence_line_ms};
use support::{Failure, TestResult, base};

const R8E: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/results/r8e");
/// The `--min-silence` of the recorded run, in milliseconds.
const MIN_SILENCE_MS: i64 = 300;
const REFERENCES: usize = 64;

fn read_lines(name: &str) -> Result<Vec<Value>, String> {
    let path = std::path::Path::new(R8E).join(name);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).map_err(|e| format!("{}: {e}", path.display())))
        .collect()
}

fn int_of(row: &Value, key: &str) -> Result<i64, String> {
    row.get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("a row has no integer `{key}`: {row}"))
}

/// The recorded Rust run counted the way RP-31 says.
fn recorded_samples() -> Result<Vec<Sample>, String> {
    let mut windows = Vec::new();
    for row in read_lines("rp3_rust.jsonl")? {
        let narrow =
            |key: &str| u32::try_from(int_of(&row, key)?).map_err(|e| format!("{key}: {e}"));
        windows.push(Window {
            repeat: narrow("repeat")?,
            utt: narrow("utt")?,
            t_start_us: int_of(&row, "t_start_us")?,
            t_end_us: int_of(&row, "t_end_us")?,
        });
    }
    let mut events = Vec::new();
    for row in read_lines("rp3_rust.events.jsonl")? {
        let Some(event) = row.get("event") else {
            return Err(format!("a row has no `event`: {row}"));
        };
        events.push(
            parse_event(int_of(&row, "t_us")?, &event.to_string()).map_err(|e| e.to_string())?,
        );
    }
    let references = vec![String::new(); REFERENCES];
    build_samples(&windows, &references, &events).map_err(|e| e.to_string())
}

#[test]
fn rp32_the_translated_share_of_valid_samples_reaches_20_of_23() -> TestResult {
    let samples = recorded_samples()?;
    // A run that translated every valid sample passes.
    let all: Vec<Sample> = samples
        .iter()
        .cloned()
        .map(|sample| Sample {
            t_tr_us: sample.t_final_us.map(|t| t + 100_000),
            ..sample
        })
        .collect();
    assert!(check_translated_share(&all, MIN_SILENCE_MS).is_ok());
    Ok(())
}

#[test]
fn the_recorded_r8e_run_is_below_20_of_23() -> TestResult {
    let samples = recorded_samples()?;
    let share = translated_share(&samples, MIN_SILENCE_MS);
    // 100 valid samples of the 115 (one of the 101 with a `final` has a wrong end), 66 translated.
    assert_eq!((share.valid, share.translated), (100, 66));
    let said = check_translated_share(&samples, MIN_SILENCE_MS);
    assert!(
        matches!(&said, Err(m) if m.to_string().contains("66 of 100")),
        "{said:?}"
    );
    Ok(())
}

// ---- RP-32 judgement of runs measured one after the other (T 4.42) ----

fn f32_run(implementation: Implementation, k: u32) -> AltRun {
    AltRun {
        implementation,
        load_before: Some(String::from(LOAD_OK)),
        load_after: Some(String::from(LOAD_OK)),
        samples: (1..=19).map(|utt| f32_sample(utt, k)).collect(),
        rules: None,
    }
}

/// Python, Rust, Python, Rust, Python, Rust; Rust equals Python.
fn f32_runs() -> Vec<AltRun> {
    (1..=3)
        .flat_map(|k| {
            [
                f32_run(Implementation::Python, k),
                f32_run(Implementation::Rust, k),
            ]
        })
        .collect()
}

/// Delays the translation of utterance `utt` in `run` by `ms`.
fn bump(run: &mut AltRun, utt: u32, ms: i64) {
    for sample in run.samples.iter_mut().filter(|s| s.utt == utt) {
        sample.t_tr_us = sample.t_tr_us.map(|t| t + ms * 1000);
    }
}

fn rejected(result: &Result<AltReport, Failure>, needle: &str) {
    assert!(
        result.as_ref().is_err_and(|why| why.contains(needle)),
        "{result:?}"
    );
}

fn stat(n: usize, p50_ms: i64, p95_ms: i64) -> Option<Stat> {
    Some(Stat { n, p50_ms, p95_ms })
}

#[test]
fn three_alternated_pairs_with_equal_values_pass() -> TestResult {
    let report = check_alternated_runs(&f32_runs(), None)?;
    assert_eq!(report.pairs, 3);
    assert_eq!((report.comparison_set.len(), report.not_slower), (19, 19));
    Ok(())
}

#[test]
fn two_python_runs_in_a_row_are_rejected() {
    let mut runs = f32_runs();
    runs.swap(1, 2);
    rejected(&check_alternated_runs(&runs, None), "run 2");
}

#[test]
fn a_run_order_that_starts_with_rust_is_rejected() {
    let mut runs = f32_runs();
    for pair in 0..3 {
        runs.swap(2 * pair, 2 * pair + 1);
    }
    rejected(&check_alternated_runs(&runs, None), "run 1");
}

#[test]
fn five_runs_leave_the_last_pair_unfinished() {
    let mut runs = f32_runs();
    runs.pop();
    rejected(&check_alternated_runs(&runs, None), "run 5");
}

#[test]
fn two_pairs_with_the_count_recorded_are_accepted() -> TestResult {
    let mut runs = f32_runs();
    runs.truncate(4);
    check_alternated_runs(&runs, Some(2))?;
    Ok(())
}

#[test]
fn two_pairs_without_the_recorded_count_are_rejected() {
    let mut runs = f32_runs();
    runs.truncate(4);
    rejected(&check_alternated_runs(&runs, None), "recorded");
}

#[test]
fn one_pair_is_rejected_even_with_the_count_recorded() {
    let mut runs = f32_runs();
    runs.truncate(2);
    rejected(&check_alternated_runs(&runs, Some(1)), "at least 2 pairs");
}

fn with_load_before(line: &str) -> Vec<AltRun> {
    let mut runs = f32_runs();
    runs[3].load_before = Some(String::from(line));
    runs
}

#[test]
fn a_load_of_1_99_before_a_run_passes() -> TestResult {
    check_alternated_runs(&with_load_before("1.99 0.50 0.50 1/300 1000"), None)?;
    Ok(())
}

#[test]
fn a_load_of_2_00_before_a_run_is_rejected() {
    let runs = with_load_before("2.00 0.50 0.50 1/300 1000");
    rejected(&check_alternated_runs(&runs, None), "run 4");
}

#[test]
fn a_load_of_0_00_before_a_run_passes() -> TestResult {
    check_alternated_runs(&with_load_before("0.00 0.00 0.00 1/300 1000"), None)?;
    Ok(())
}

#[test]
fn a_run_without_a_load_before_record_is_rejected() {
    let mut runs = f32_runs();
    runs[3].load_before = None;
    rejected(
        &check_alternated_runs(&runs, None),
        "run 4 has no load before",
    );
}

#[test]
fn a_run_without_a_load_after_record_is_rejected() {
    let mut runs = f32_runs();
    runs[3].load_after = None;
    rejected(
        &check_alternated_runs(&runs, None),
        "run 4 has no load after",
    );
}

#[test]
fn an_unreadable_load_line_is_rejected_not_read_as_zero() {
    rejected(
        &check_alternated_runs(&with_load_before("n/a"), None),
        "run 4",
    );
}

#[test]
fn the_load_after_a_run_is_recorded_but_not_judged() -> TestResult {
    let mut runs = f32_runs();
    runs[3].load_before = Some(String::from("1.99 0.50 0.50 1/300 1000"));
    runs[3].load_after = Some(String::from("9.99 0.50 0.50 1/300 1000"));
    let report = check_alternated_runs(&runs, None)?;
    assert_eq!(report.loads_after[3], 999);
    Ok(())
}

#[test]
fn p50_one_millisecond_over_at_the_29th_sample_is_rejected() {
    let mut runs = f32_runs();
    bump(&mut runs[3], 10, 1);
    let said = check_alternated_runs(&runs, None);
    rejected(&said, "p50 rust 1103 ms > python 1102 ms");
}

#[test]
fn p50_is_decided_by_the_29th_sample_only() -> TestResult {
    let mut runs = f32_runs();
    bump(&mut runs[5], 10, 1);
    check_alternated_runs(&runs, None)?;
    Ok(())
}

#[test]
fn p95_one_millisecond_over_at_the_55th_sample_is_rejected() {
    let mut runs = f32_runs();
    for index in [1, 3, 5] {
        bump(&mut runs[index], 19, 1);
    }
    let said = check_alternated_runs(&runs, None);
    rejected(&said, "p95 rust 1192 ms > python 1191 ms");
    assert!(said.is_err_and(|why| !why.contains("p50")));
}

#[test]
fn p95_is_decided_by_the_55th_sample_only() -> TestResult {
    let mut runs = f32_runs();
    for index in [3, 5] {
        bump(&mut runs[index], 19, 1);
    }
    check_alternated_runs(&runs, None)?;
    Ok(())
}

/// Delays utterances `utts` by 1 ms in all three Rust runs.
fn slower_utterances(utts: &[u32]) -> Vec<AltRun> {
    let mut runs = f32_runs();
    for index in [1, 3, 5] {
        for utt in utts {
            bump(&mut runs[index], *utt, 1);
        }
    }
    runs
}

#[test]
fn seventeen_of_19_utterances_not_slower_pass() -> TestResult {
    let report = check_alternated_runs(&slower_utterances(&[1, 2]), None)?;
    assert_eq!((report.comparison_set.len(), report.not_slower), (19, 17));
    Ok(())
}

#[test]
fn sixteen_of_19_utterances_not_slower_are_rejected_though_p50_and_p95_pass() {
    let said = check_alternated_runs(&slower_utterances(&[1, 2, 3]), None);
    rejected(&said, "16 of 19 utterances are not slower than python");
    assert!(said.is_err_and(|why| !why.contains("p50") && !why.contains("p95")));
}

#[test]
fn the_comparison_set_ignores_utterances_python_never_finished() -> TestResult {
    let python: Vec<Vec<Sample>> = f32_runs()
        .iter()
        .filter(|run| run.implementation == Implementation::Python)
        .map(|run| {
            run.samples
                .iter()
                .filter(|s| s.utt != 19)
                .cloned()
                .collect()
        })
        .collect();
    let refs: Vec<&[Sample]> = python.iter().map(Vec::as_slice).collect();
    let set = l_comparison_set(&refs);
    assert_eq!(set.len(), 18);
    assert!(!set.contains(&19));
    assert_eq!(comparison_threshold(set.len())?, 16);
    Ok(())
}

#[test]
fn a_held_sample_is_not_counted_in_l() -> TestResult {
    let mut runs = f32_runs();
    let mut held = f32_sample(1, 1);
    held.t_tr_us = None;
    runs[1].samples.push(held);
    let report = check_alternated_runs(&runs, None)?;
    assert_eq!(report.rust.l.map(|l| l.n), Some(57));
    Ok(())
}

#[test]
fn l_asr_l_tr_and_translation_stage_columns_are_written() -> TestResult {
    let report = check_alternated_runs(&f32_runs(), None)?;
    for side in [&report.rust, &report.python] {
        assert_eq!(side.l, stat(57, 1102, 1191));
        assert_eq!(side.l_asr, stat(57, 710, 719));
        assert_eq!(side.l_tr, stat(57, 392, 472));
    }
    let text = render_columns(&report.rust);
    check_columns_written(&text)?;
    assert!(text.contains("translation stage p50 392 p95 472"), "{text}");
    Ok(())
}

fn without_a_column(text: &str, prefix: &str) -> String {
    let kept: Vec<&str> = text.lines().filter(|l| !l.starts_with(prefix)).collect();
    kept.join("\n")
}

#[test]
fn a_missing_translation_stage_column_is_rejected() -> TestResult {
    let report = check_alternated_runs(&f32_runs(), None)?;
    let text = without_a_column(&render_columns(&report.rust), "translation stage");
    let said = check_columns_written(&text);
    assert!(said.is_err_and(|why| why.contains("translation stage")));
    Ok(())
}

#[test]
fn a_missing_l_tr_column_is_rejected() -> TestResult {
    let report = check_alternated_runs(&f32_runs(), None)?;
    let text = without_a_column(&render_columns(&report.rust), "L_tr");
    let said = check_columns_written(&text);
    assert!(said.is_err_and(|why| why.contains("L_tr")));
    Ok(())
}

fn baseline_run(name: &str) -> Result<Vec<Sample>, Box<dyn std::error::Error>> {
    Ok(read_samples(&base()?.join(name))?)
}

/// Two recorded Python runs (0.35 s) give the columns; `jq` counts 95 and 94 samples with an `L`.
#[test]
fn python_against_python_pair_yields_the_same_columns() -> TestResult {
    for (name, n) in [("lat_ja_0.35_1.jsonl", 95), ("lat_ja_0.35_2.jsonl", 94)] {
        let columns = columns_of(&baseline_run(name)?, 350)?;
        assert_eq!(columns.l.map(|l| l.n), Some(n), "{name}");
        check_columns_written(&render_columns(&columns))?;
    }
    Ok(())
}

#[test]
fn the_same_run_against_itself_has_no_difference() -> TestResult {
    let columns = columns_of(&baseline_run("lat_ja_0.35_1.jsonl")?, 350)?;
    let Some(l) = columns.l else {
        return Err("the recorded run has no L".into());
    };
    check_percentiles("L", &l, &l)?;
    Ok(())
}

#[test]
fn the_recorded_r8e_gap_is_read_as_slower_than_python() -> TestResult {
    let rust = columns_of(&recorded_samples()?, MIN_SILENCE_MS)?;
    let python = columns_of(&baseline_run("lat_ja_0.30_1.jsonl")?, MIN_SILENCE_MS)?;
    assert_eq!(rust.l, stat(66, 1533, 2752));
    assert_eq!(python.l, stat(95, 1481, 2008));
    let (Some(r), Some(p)) = (rust.l, python.l) else {
        return Err("a side has no L".into());
    };
    let said = check_percentiles("L", &p, &r);
    assert!(
        said.as_ref()
            .is_err_and(|why| why.to_string().contains("p50") && why.to_string().contains("p95")),
        "{said:?}"
    );
    Ok(())
}

#[test]
fn a_load_line_is_read_in_hundredths() {
    assert_eq!(load_hundredths("1.99 0.50 0.50 1/300 1000"), Ok(199));
    assert_eq!(load_hundredths("0.00 0.00 0.00 1/300 1000"), Ok(0));
    for bad in ["", "n/a", "2.5 0.1", "2 0.1", "x.yy"] {
        assert!(load_hundredths(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn a_load_file_gives_the_line_of_each_run_and_moment() {
    let text = "py1 before 05:33:49 1.88 2.54 3.22 4/691 2616274\n\
                py1 after 05:40:38 2.10 2.21 2.81 2/724 2621146\n\n";
    let found = parse_load_file(text);
    let key = |when: &str| (String::from("py1"), String::from(when));
    assert_eq!(found.len(), 2);
    assert_eq!(
        found.get(&key("before")).map(String::as_str),
        Some("1.88 2.54 3.22 4/691 2616274")
    );
    assert_eq!(load_hundredths(&found[&key("after")]), Ok(210));
}

#[test]
fn a_directory_without_the_runs_names_the_files_it_lacks() {
    let dir = std::path::Path::new(support::RESULTS).join("rp32_no_such_run");
    assert_eq!(missing_files(&dir, 3).len(), 9);
    let said = load_alternated(&dir, 3);
    assert!(said.is_err_and(|why| why.contains("py1.jsonl")
        && why.contains("rs3.jsonl")
        && why.contains("load2.txt")),);
}

/// The F32 runs of `pairs` pairs in a scratch directory under the build tree
/// (see `write_f32_dir`); `name` keeps the tests that run side by side apart.
fn f32_dir(
    name: &str,
    pairs: u32,
    pairs_txt: Option<&str>,
) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let dir = scratch_dir("rp32-test-dirs", name)?;
    write_f32_dir(&dir, pairs, pairs_txt)?;
    Ok(dir)
}

/// T-RP-32-030: no `pairs.txt` and three pairs is read as three pairs.
#[test]
fn a_dir_without_pairs_txt_is_read_as_three_pairs() -> TestResult {
    let dir = f32_dir("t030", 3, None)?;
    let count = pairs_to_read(&dir)?;
    assert_eq!((count.pairs, count.recorded), (3, None));
    check_dir(&dir)?;
    Ok(())
}

/// T-RP-32-031: no `pairs.txt` and two pairs: the third pair is missing, not
/// read as a run of two.
#[test]
fn a_dir_without_pairs_txt_and_two_pairs_is_missing_the_third() -> TestResult {
    let dir = f32_dir("t031", 2, None)?;
    let said = check_dir(&dir);
    assert!(
        said.as_ref()
            .is_err_and(|why| why.contains("py3.jsonl") && why.contains("rs3.jsonl")),
        "{said:?}"
    );
    Ok(())
}

/// T-RP-32-032: `2` in `pairs.txt` reads two pairs.
#[test]
fn pairs_txt_of_2_accepts_two_pairs() -> TestResult {
    let dir = f32_dir("t032", 2, Some("2\n"))?;
    let count = pairs_to_read(&dir)?;
    assert_eq!((count.pairs, count.recorded), (2, Some(2)));
    check_dir(&dir)?;
    Ok(())
}

/// T-RP-32-033: `2` recorded while three pairs stand there. Both numbers are named.
#[test]
fn pairs_txt_of_2_with_three_pairs_present_is_rejected() -> TestResult {
    let dir = f32_dir("t033", 3, Some("2\n"))?;
    let said = check_dir(&dir);
    assert!(
        said.as_ref()
            .is_err_and(|why| why.contains("records 2 pairs") && why.contains("3 pairs stand")),
        "{said:?}"
    );
    Ok(())
}

/// T-RP-32-034: a `pairs.txt` that holds no number is refused as unreadable,
/// and is not read as a count that disagrees. Each input is named on failure.
#[test]
fn an_unreadable_pairs_txt_is_rejected_as_unreadable() -> TestResult {
    for (index, content) in ["two", "", "2 pairs", "-2"].into_iter().enumerate() {
        let dir = f32_dir(&format!("t034-{index}"), 2, Some(content))?;
        let said = check_dir(&dir);
        assert!(
            said.as_ref()
                .is_err_and(|why| why.contains("not a pair count") && !why.contains("records")),
            "pairs.txt {content:?}: {said:?}"
        );
    }
    Ok(())
}

/// T-RP-32-035: `1` is under the lower bound and is named so, not as a mismatch.
#[test]
fn pairs_txt_of_1_is_rejected() -> TestResult {
    let dir = f32_dir("t035", 1, Some("1\n"))?;
    let said = check_dir(&dir);
    assert!(
        said.as_ref()
            .is_err_and(|why| why.contains("at least 2 pairs") && !why.contains("records")),
        "{said:?}"
    );
    Ok(())
}

/// T-RP-32-036: `3` with three pairs gives the result of no `pairs.txt`.
#[test]
fn pairs_txt_of_3_accepts_three_pairs() -> TestResult {
    let dir = f32_dir("t036", 3, Some("3\n"))?;
    let count = pairs_to_read(&dir)?;
    assert_eq!((count.pairs, count.recorded), (3, Some(3)));
    check_dir(&dir)?;
    Ok(())
}

#[test]
fn a_pair_count_is_one_number_or_it_is_refused() {
    assert_eq!(parse_pairs_text("2\n"), Ok(2));
    assert_eq!(parse_pairs_text(" 3 "), Ok(3));
    for bad in ["", "\n", "two", "2 pairs", "-2", "0", "1"] {
        assert!(parse_pairs_text(bad).is_err(), "{bad:?}");
    }
}

/// The real runs of `docs/results/rp32/`: Python, Rust, Python, Rust, Python, Rust.
/// The number of pairs is read from `pairs.txt` there (three when it is absent).
#[test]
#[ignore = "reads the runs in docs/results/rp32/"]
fn rp32_alternated_runs_under_the_same_load_beat_python() -> TestResult {
    let dir = rp32_dir();
    let count = pairs_to_read(&dir)?;
    let runs = load_alternated(&dir, count.pairs)?;
    check_ports_held(&dir, count.pairs)?;
    let said = check_alternated_runs(&runs, count.recorded);
    if let Ok(report) = &said {
        println!("rp32 python:\n{}", render_columns(&report.python));
        println!("rp32 rust:\n{}", render_columns(&report.rust));
        println!("rp32 share:\n{}", render_share_columns(report));
        println!("rp32 lines:\n{}", render_line_counts(report));
        println!(
            "rp32: {} of {} utterances not slower; loads after {:?}",
            report.not_slower,
            report.comparison_set.len(),
            report.loads_after
        );
    }
    assert!(said.is_ok(), "{said:?}");
    Ok(())
}

// ---- the translated share of the pooled Rust samples (T 4.42, 037 to 046) ----

/// Takes the translation away from the samples of `run` whose utterance is in
/// `utts`: a `final` stays, `t_tr` goes (the shape RP-31 reads as a held draft).
fn without_translation(run: &mut AltRun, utts: std::ops::RangeInclusive<u32>) {
    for sample in run.samples.iter_mut().filter(|s| utts.contains(&s.utt)) {
        sample.t_tr_us = None;
    }
}

/// S50: of the 57 valid Rust samples, 50 are translated (run k = 3 lost u = 13 to 19).
fn s50_runs() -> Vec<AltRun> {
    let mut runs = f32_runs();
    without_translation(&mut runs[5], 13..=19);
    runs
}

/// S49: S50 and the Rust sample of k = 2, u = 19 without a translation.
fn s49_runs() -> Vec<AltRun> {
    let mut runs = s50_runs();
    without_translation(&mut runs[3], 19..=19);
    runs
}

/// What `check_translated_share` says about a share below 20 / 23.
const SHARE_REASON: &str = "valid samples have a translation, 20 of 23 are needed";

/// T-RP-32-037: 50 of 57 reaches 20 / 23 (50 x 23 = 1150 >= 57 x 20 = 1140).
#[test]
fn fifty_translated_of_57_valid_samples_pass_the_share() -> TestResult {
    let report = check_alternated_runs(&s50_runs(), None)?;
    assert_eq!(report.rust.l, stat(50, 1091, 1182));
    assert_eq!(report.python.l, stat(57, 1102, 1191));
    assert_eq!((report.comparison_set.len(), report.not_slower), (19, 19));
    assert_eq!(report.rust_share, share_of(57, 50));
    Ok(())
}

fn share_of(valid: usize, translated: usize) -> TranslatedShare {
    TranslatedShare { valid, translated }
}

/// T-RP-32-038: 49 of 57 is one under (49 x 23 = 1127 < 1140). `L`, the
/// percentiles and the paired comparison all pass, so the share is the only reason.
#[test]
fn forty_nine_translated_of_57_valid_samples_fail_the_share_alone() {
    let said = check_alternated_runs(&s49_runs(), None);
    rejected(
        &said,
        "49 of 57 valid samples have a translation, 20 of 23 are needed",
    );
    assert!(said.is_err_and(|why| !why.contains("p50")
        && !why.contains("p95")
        && !why.contains("not slower")),);
}

/// T-RP-32-039: the share, both percentiles and the paired comparison are all
/// listed; none hides another.
#[test]
fn a_short_share_is_listed_beside_the_other_failures() {
    let mut runs = s49_runs();
    for index in [1, 3, 5] {
        for utt in 1..=19 {
            bump(&mut runs[index], utt, 30);
        }
    }
    let said = check_alternated_runs(&runs, None);
    rejected(&said, "49 of 57 valid samples have a translation");
    rejected(&said, "p50 rust 1121 ms > python 1102 ms");
    rejected(&said, "p95 rust 1211 ms > python 1191 ms");
    rejected(
        &said,
        "0 of 19 utterances are not slower than python, 17 are needed",
    );
}

/// A Rust sample of `utt` and `repeat` in the shape of F32 (`L_asr` 700 + u ms,
/// a decode of `decode_ms`), translated.
fn extra_sample(utt: u32, repeat: u32, decode_ms: f64) -> Sample {
    Sample {
        repeat,
        final_latency_ms: Some(decode_ms),
        ..f32_sample(utt, 1)
    }
}

/// The decode that leaves a silence gap of `gap_ms` for utterance `utt`.
fn decode_for_gap(utt: u32, gap_ms: f64) -> f64 {
    f64::from(700 + utt) - gap_ms
}

/// T-RP-32-040: samples that are not valid stay out of the denominator: five
/// without a `final`, three whose silence gap is 299.999 ms (the limit is 300).
#[test]
fn samples_that_are_not_valid_stay_out_of_the_denominator() -> TestResult {
    let mut runs = s50_runs();
    for utt in 1..=5 {
        runs[1].samples.push(Sample {
            t_final_us: None,
            t_tr_us: None,
            ..extra_sample(utt, 2, 100.0)
        });
    }
    for utt in 6..=8 {
        runs[1]
            .samples
            .push(extra_sample(utt, 2, decode_for_gap(utt, 299.999)));
    }
    let report = check_alternated_runs(&runs, None)?;
    assert_eq!(report.rust_share, share_of(57, 50));
    // The limit itself (300.000 ms) is valid: the same sample one micro-unit short is not.
    let mut at_the_limit = s50_runs();
    at_the_limit[1]
        .samples
        .push(extra_sample(6, 2, decode_for_gap(6, 300.0)));
    assert_eq!(
        check_alternated_runs(&at_the_limit, None)?.rust_share,
        share_of(58, 51)
    );
    Ok(())
}

/// T-RP-32-041: the denominator counts samples, not the `final`s tied to them.
#[test]
fn the_denominator_counts_samples_not_finals() -> TestResult {
    let mut runs = s50_runs();
    for run in runs
        .iter_mut()
        .filter(|r| r.implementation == Implementation::Rust)
    {
        for sample in &mut run.samples {
            sample.finals = 2;
        }
    }
    check_alternated_runs(&runs, None)?;
    Ok(())
}

/// F32L: three pairs of 23 utterances x 5 repeats. Every `L` is 1000 ms, every
/// sample valid (`L_asr` 700 ms, a decode of 300 ms).
fn f32l_run(implementation: Implementation) -> AltRun {
    let samples = (1..=23)
        .flat_map(|utt| (1..=5).map(move |repeat| (utt, repeat)))
        .map(|(utt, repeat)| Sample {
            repeat,
            utt,
            t_final_us: Some(700_000),
            t_tr_us: Some(1_000_000),
            final_latency_ms: Some(300.0),
            ..f32_sample(utt, 1)
        })
        .collect();
    AltRun {
        implementation,
        load_before: Some(String::from(LOAD_OK)),
        load_after: Some(String::from(LOAD_OK)),
        samples,
        rules: None,
    }
}

fn f32l_runs() -> Vec<AltRun> {
    (0..3)
        .flat_map(|_| {
            [
                f32l_run(Implementation::Python),
                f32l_run(Implementation::Rust),
            ]
        })
        .collect()
}

/// F32L with 45 Rust samples untranslated: two each for u = 1 to 22, one for u = 23.
fn f32l_with_45_untranslated() -> Vec<AltRun> {
    let mut runs = f32l_runs();
    for sample in &mut runs[1].samples {
        let drops = if sample.utt <= 22 { 2 } else { 1 };
        if sample.repeat <= drops {
            sample.t_tr_us = None;
        }
    }
    runs
}

/// T-RP-32-043: 300 of 345 is exactly 20 / 23 (300 x 23 = 6900 = 345 x 20).
#[test]
fn three_hundred_of_345_valid_samples_pass_the_share_exactly() -> TestResult {
    let report = check_alternated_runs(&f32l_with_45_untranslated(), None)?;
    assert_eq!((report.comparison_set.len(), report.not_slower), (23, 23));
    Ok(())
}

/// T-RP-32-044: one more untranslated sample (299 of 345) is under.
#[test]
fn two_hundred_ninety_nine_of_345_valid_samples_fail_the_share() {
    let mut runs = f32l_with_45_untranslated();
    for sample in runs[1]
        .samples
        .iter_mut()
        .filter(|s| s.utt == 23 && s.repeat == 2)
    {
        sample.t_tr_us = None;
    }
    let said = check_alternated_runs(&runs, None);
    rejected(
        &said,
        "299 of 345 valid samples have a translation, 20 of 23 are needed",
    );
    assert!(said.is_err_and(|why| !why.contains("p50")
        && !why.contains("p95")
        && !why.contains("not slower")),);
}

/// T-RP-32-042 (PY49): Python's own share is short (49 of 57) and is written
/// beside Rust's, but the judgement reads Rust's share only.
#[test]
fn a_short_python_share_is_recorded_and_not_judged() -> TestResult {
    let mut runs = f32_runs();
    without_translation(&mut runs[0], 1..=8);
    let report = check_alternated_runs(&runs, None)?;
    assert_eq!(report.python.l, stat(49, 1113, 1191));
    assert_eq!(report.rust.l, stat(57, 1102, 1191));
    assert_eq!((report.comparison_set.len(), report.not_slower), (11, 11));
    assert_eq!(report.python_share, share_of(57, 49));
    assert_eq!(report.rust_share, share_of(57, 57));
    let text = render_share_columns(&report);
    assert!(
        text.contains(&format!("{SHARE_COLUMN_RUST} 57 / 57")),
        "{text}"
    );
    assert!(
        text.contains(&format!("{SHARE_COLUMN_PYTHON} 49 / 57")),
        "{text}"
    );
    Ok(())
}

/// The `rs1` to `rs3` rows of the recorded dir counted straight from the JSON
/// (the `jq` of T-RP-32-045): the rows with a `final` whose silence gap reaches
/// 300 ms, and those among them with a `t_tr_us`.
fn recorded_rust_share() -> Result<(usize, usize), Box<dyn std::error::Error>> {
    let (mut valid, mut translated) = (0, 0);
    for number in 1..=3 {
        let text = std::fs::read_to_string(rp32_dir().join(format!("rs{number}.jsonl")))?;
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            let row: Value = serde_json::from_str(line)?;
            if !row_has_a_valid_gap(&row) {
                continue;
            }
            valid += 1;
            translated += usize::from(row.get("t_tr_us").is_some_and(|t| !t.is_null()));
        }
    }
    Ok((valid, translated))
}

/// `.t_final_us != null and ((.t_final_us - .t_end_us) / 1000 - .final_latency_ms) >= 300`.
fn row_has_a_valid_gap(row: &Value) -> bool {
    let number = |key: &str| row.get(key).and_then(Value::as_f64);
    match (
        number("t_final_us"),
        number("t_end_us"),
        number("final_latency_ms"),
    ) {
        (Some(t_final), Some(t_end), Some(latency)) => {
            (t_final - t_end) / 1000.0 - latency >= 300.0
        }
        _ => false,
    }
}

/// T-RP-32-045: the recorded runs hold 249 translated of 252 valid Rust samples,
/// which reaches 20 / 23 (249 x 23 = 5727 >= 5040). Whether the dir passes the
/// other checks or not, the share is not among its reasons.
#[test]
fn the_recorded_rp32_dir_has_enough_translated_samples() -> TestResult {
    let counted = recorded_rust_share()?;
    assert_eq!(counted, (252, 249));
    assert!(counted.1 * 23 >= counted.0 * 20);
    let dir = rp32_dir();
    let count = pairs_to_read(&dir)?;
    // No rule is read: the count is the one at the 300 ms line, as `jq` counts it.
    let report = alternated_report(&load_runs(&dir, count.pairs)?, count.recorded)?;
    assert_eq!(report.rust_share, share_of(252, 249));
    let text = render_share_columns(&report);
    assert!(
        text.contains(&format!("{SHARE_COLUMN_RUST} 249 / 252")),
        "{text}"
    );
    let said = check_dir(&dir);
    assert!(
        said.as_ref()
            .err()
            .is_none_or(|why| !why.contains(SHARE_REASON)),
        "{said:?}"
    );
    Ok(())
}

/// T-RP-32-046: an output without a translated-share column is refused, by
/// the name of the column that is missing.
#[test]
fn a_missing_translated_share_column_is_rejected() -> TestResult {
    let report = check_alternated_runs(&s50_runs(), None)?;
    let text = render_share_columns(&report);
    check_share_columns_written(&text)?;
    for heading in [SHARE_COLUMN_RUST, SHARE_COLUMN_PYTHON] {
        let said = check_share_columns_written(&without_a_column(&text, heading));
        assert!(said.is_err_and(|why| why.contains(heading)), "{heading}");
    }
    Ok(())
}

// ---- the line of a Rust sample follows the rule that closed it (T-RP-32-047 on) ----

/// The early detector's silence in milliseconds, from the constant the server logs.
fn early_ms() -> f64 {
    f64::from(EARLY_MIN_SILENCE_MS)
}

/// The F32 runs whose first Rust run records a rule for each sample: the base
/// samples closed by the silence rule, and one more Rust sample (repeat 2,
/// utterance 6) closed by `rule` with a silence gap of `gap_ms`.
fn report_with_extra(rule: Rule, gap_ms: f64) -> Result<AltReport, Failure> {
    let mut runs = f32_runs();
    let mut rules: BTreeMap<(u32, u32), Rule> =
        (1..=19).map(|utt| ((1, utt), Rule::Silence)).collect();
    runs[1]
        .samples
        .push(extra_sample(6, 2, decode_for_gap(6, gap_ms)));
    rules.insert((2, 6), rule);
    runs[1].rules = Some(rules);
    alternated_report(&runs, None)
}

/// Whether the extra sample of [`report_with_extra`] counts as valid: the Rust
/// share holds 58 valid samples instead of the 57 of the base.
fn extra_is_valid(rule: Rule, gap_ms: f64) -> Result<bool, Failure> {
    Ok(report_with_extra(rule, gap_ms)?.rust_share.valid == 58)
}

/// T-RP-32-047: a sample the early rule closed is valid from the early line up,
/// at the line and 1 ms over, and not 1 ms or 0.001 ms under.
#[test]
fn an_early_sample_is_valid_from_the_early_line() -> TestResult {
    let line = early_ms();
    assert!(!extra_is_valid(Rule::Early, line - 1.0)?);
    assert!(!extra_is_valid(Rule::Early, line - 0.001)?);
    assert!(extra_is_valid(Rule::Early, line)?);
    assert!(extra_is_valid(Rule::Early, line + 1.0)?);
    Ok(())
}

/// T-RP-32-048: a sample the silence rule closed is valid from `min_silence` up.
#[test]
fn a_silence_sample_is_valid_from_min_silence() -> TestResult {
    let line = MIN_SILENCE_MS as f64;
    assert!(!extra_is_valid(Rule::Silence, line - 1.0)?);
    assert!(!extra_is_valid(Rule::Silence, line - 0.001)?);
    assert!(extra_is_valid(Rule::Silence, line)?);
    assert!(extra_is_valid(Rule::Silence, line + 1.0)?);
    Ok(())
}

/// T-RP-32-049: the limit on speech and a flush wait for no silence of their own
/// and are held to `min_silence`, not to the early line.
#[test]
fn a_max_speech_or_flush_sample_is_held_to_min_silence() -> TestResult {
    let line = MIN_SILENCE_MS as f64;
    for rule in [Rule::MaxSpeech, Rule::Flush] {
        assert!(!extra_is_valid(rule, early_ms())?, "{rule:?}");
        assert!(!extra_is_valid(rule, line - 1.0)?, "{rule:?}");
        assert!(extra_is_valid(rule, line)?, "{rule:?}");
    }
    Ok(())
}

/// T-RP-32-050: an early sample whose gap is between the two lines is valid on
/// its own line, invalid on `min_silence`, and counted as gained by `early`.
#[test]
fn an_early_sample_between_the_lines_is_gained() -> TestResult {
    let report = report_with_extra(Rule::Early, early_ms() + 1.0)?;
    assert_eq!(report.rust_share.valid, 58);
    assert_eq!(report.flat.rust_share.valid, 57);
    assert_eq!(report.gained.get("early"), Some(&1));
    assert_eq!(report.gained.values().sum::<usize>(), 1);
    Ok(())
}

/// T-RP-32-051: a silence sample under `min_silence` is invalid on both lines,
/// and nothing is gained.
#[test]
fn a_silence_sample_under_min_silence_gains_nothing() -> TestResult {
    let report = report_with_extra(Rule::Silence, early_ms() + 1.0)?;
    assert_eq!(report.rust_share.valid, 57);
    assert_eq!(report.flat.rust_share.valid, 57);
    assert_eq!(report.gained.values().sum::<usize>(), 0);
    Ok(())
}

/// T-RP-32-052: a run that records rules refuses a sample with a `final` and
/// no rule, by run and utterance; it is not read as `min_silence`.
#[test]
fn a_sample_with_a_final_and_no_rule_is_refused() {
    let mut runs = f32_runs();
    let rules: BTreeMap<(u32, u32), Rule> = (1..=18).map(|utt| ((1, utt), Rule::Silence)).collect();
    runs[1].rules = Some(rules);
    let said = alternated_report(&runs, None);
    rejected(
        &said,
        "run 2: repeat 1 utt 19 has a final and no recorded rule",
    );
}

/// T-RP-32-053: the line of each rule, and the names the log gives.
#[test]
fn the_line_of_a_rule_comes_from_the_early_constant_or_min_silence() {
    let early = i64::from(EARLY_MIN_SILENCE_MS);
    assert_eq!(silence_line_ms(Rule::Early, MIN_SILENCE_MS), early);
    for rule in [Rule::Silence, Rule::MaxSpeech, Rule::Flush] {
        assert_eq!(silence_line_ms(rule, MIN_SILENCE_MS), MIN_SILENCE_MS);
    }
    assert_eq!(rule_named("early"), Some(Rule::Early));
    assert_eq!(rule_named("max_speech"), Some(Rule::MaxSpeech));
    for bad in ["Early", "", "silence ", "none"] {
        assert_eq!(rule_named(bad), None, "{bad:?}");
    }
}

fn decided(id: u64, rule: &str) -> String {
    format!(
        "{{\"event\":\"asr.endpoint.decided\",\"id\":{id},\"rule\":\"{rule}\",\"silence_ms\":0}}\n"
    )
}

/// T-RP-32-054: only the decided lines are read; a duplicate id, a rule the
/// server does not have, and a decided line that is not JSON are refused.
#[test]
fn the_decided_lines_are_read_and_the_bad_ones_refused() -> TestResult {
    let log = format!(
        "{{\"event\":\"asr.ready\"}}\nnot json at all\n{}{}",
        decided(1, "early"),
        decided(2, "silence")
    );
    let found = decided_rules(&log)?;
    assert_eq!(found.len(), 2);
    assert_eq!(found.get(&1), Some(&Rule::Early));
    assert_eq!(found.get(&2), Some(&Rule::Silence));
    let twice = decided_rules(&(decided(3, "early") + &decided(3, "early")));
    assert!(twice.is_err_and(|why| why.contains("twice for id 3")));
    let unknown = decided_rules(&decided(4, "quick"));
    assert!(unknown.is_err_and(|why| why.contains("id 4") && why.contains("not a rule")));
    let broken = decided_rules("{\"event\":\"asr.endpoint.decided\",\"id\":");
    assert!(broken.is_err_and(|why| why.contains("not JSON")));
    Ok(())
}

fn final_line(id: &str, t_us: i64) -> String {
    format!("{{\"event\":{{\"id\":\"{id}\",\"text\":\"x\",\"type\":\"final\"}},\"t_us\":{t_us}}}\n")
}

/// T-RP-32-055: every `final` with no decided line is named with the run, and
/// the ones that have a line are not.
#[test]
fn every_final_without_a_decided_line_is_named_with_its_run() {
    let samples = [f32_sample(1, 1), f32_sample(2, 1), f32_sample(3, 1)];
    let events = final_line("mic-1", 701_000)
        + &final_line("mic-2", 702_000)
        + &final_line("mic-3", 703_000);
    let said = closing_rules("rs1", &samples, &events, &decided(1, "early"));
    assert!(
        said.as_ref().is_err_and(|why| why.contains("rs1")
            && why.contains("mic-2, mic-3")
            && !why.contains("mic-1")),
        "{said:?}"
    );
}

/// T-RP-32-056: the rule of a sample is the rule of the `final` received when the
/// sample closed; two `final`s at that moment with different rules are refused.
#[test]
fn the_closing_final_of_a_sample_gives_its_rule() -> TestResult {
    let samples = [f32_sample(1, 1), f32_sample(2, 1)];
    let events = final_line("mic-1", 701_000) + &final_line("mic-2", 702_000);
    let log = decided(1, "early") + &decided(2, "silence");
    let rules = closing_rules("rs1", &samples, &events, &log)?;
    assert_eq!(rules.get(&(1, 1)), Some(&Rule::Early));
    assert_eq!(rules.get(&(1, 2)), Some(&Rule::Silence));
    let same_moment = final_line("mic-1", 701_000) + &final_line("mic-2", 701_000);
    let split = closing_rules("rs1", &samples[..1], &same_moment, &log);
    assert!(split.is_err_and(|why| why.contains("different rules")));
    let nothing = closing_rules("rs1", &samples[..1], &final_line("mic-1", 5), &log);
    assert!(nothing.is_err_and(|why| why.contains("no final arrived")));
    Ok(())
}

/// The F32 dir whose first Rust run has utterance 5 closed by `rule` and a
/// silence gap of 200 ms: the early line and not `min_silence` is over it.
fn dir_with_gap_200_closed_by(
    name: &str,
    rule: &str,
) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let dir = f32_dir(name, 3, None)?;
    let samples = dir.join("rs1.jsonl");
    let mut lines = Vec::new();
    for line in std::fs::read_to_string(&samples)?.lines() {
        let mut row: Value = serde_json::from_str(line)?;
        if row.get("utt").and_then(Value::as_i64) == Some(5) {
            row["final_latency_ms"] = Value::from(505.0);
        }
        lines.push(row.to_string());
    }
    std::fs::write(&samples, lines.join("\n") + "\n")?;
    let log = dir.join("rs1.hayamimi.stdout.log");
    let text = std::fs::read_to_string(&log)?;
    let from = "\"id\":5,\"rule\":\"silence\"";
    assert!(text.contains(from), "the F32 log lost its id 5 line");
    std::fs::write(
        &log,
        text.replace(from, &format!("\"id\":5,\"rule\":\"{rule}\"")),
    )?;
    Ok(dir)
}

/// T-RP-32-057: a directory is read end to end. The same 200 ms gap is valid
/// when the log says the early rule closed the utterance and invalid when it
/// says the silence rule did.
#[test]
fn a_dir_judges_the_gap_by_the_rule_its_log_names() -> TestResult {
    let early = dir_with_gap_200_closed_by("t057-early", "early")?;
    let report = alternated_report(&load_alternated(&early, 3)?, None)?;
    assert_eq!(report.rust_share.valid, 57);
    assert_eq!(report.flat.rust_share.valid, 56);
    assert_eq!(report.gained.get("early"), Some(&1));
    let silence = dir_with_gap_200_closed_by("t057-silence", "silence")?;
    let report = alternated_report(&load_alternated(&silence, 3)?, None)?;
    assert_eq!(report.rust_share.valid, 56);
    assert_eq!(report.gained.values().sum::<usize>(), 0);
    Ok(())
}

/// T-RP-32-058: a Rust run whose log lacks the decided line of one `final` is
/// refused with the run and the id, and no value is printed.
#[test]
fn a_dir_with_an_unjoined_final_is_refused_by_run_and_id() -> TestResult {
    let dir = f32_dir("t058", 3, None)?;
    let log = dir.join("rs2.hayamimi.stdout.log");
    let kept: Vec<String> = std::fs::read_to_string(&log)?
        .lines()
        .filter(|line| !line.contains("\"id\":7,"))
        .map(str::to_owned)
        .collect();
    std::fs::write(&log, kept.join("\n") + "\n")?;
    let said = check_dir(&dir);
    assert!(
        said.as_ref().is_err_and(|why| why.contains("rs2")
            && why.contains("mic-7")
            && !why.contains("mic-8")
            && !why.contains("p50")),
        "{said:?}"
    );
    Ok(())
}

/// T-RP-32-059: a Rust run without its stdout log is refused by the file's name.
#[test]
fn a_dir_without_a_hayamimi_stdout_log_is_refused_by_name() -> TestResult {
    let dir = f32_dir("t059", 3, None)?;
    assert!(missing_rule_files(&dir, 3).is_empty());
    std::fs::remove_file(dir.join("rs2.hayamimi.stdout.log"))?;
    assert_eq!(missing_rule_files(&dir, 3), vec!["rs2.hayamimi.stdout.log"]);
    let said = check_dir(&dir);
    assert!(
        said.as_ref()
            .is_err_and(|why| why.contains("rs2.hayamimi.stdout.log") && !why.contains("p50")),
        "{said:?}"
    );
    Ok(())
}

/// T-RP-32-060: the counts at both lines are written, with the margin over the
/// threshold in words: 2 over at 19 of 19, 0 at 17 of 19.
#[test]
fn both_counts_and_the_margin_are_written() -> TestResult {
    let all = check_alternated_runs(&f32_runs(), None)?;
    let text = render_line_counts(&all);
    assert!(text.contains("not slower, 300 ms line: 19 / 19"), "{text}");
    assert!(
        text.contains("not slower, per-row line: 19 / 19 (17 needed, margin 2)"),
        "{text}"
    );
    let edge = check_alternated_runs(&slower_utterances(&[1, 2]), None)?;
    let text = render_line_counts(&edge);
    assert!(
        text.contains("17 / 19 (17 needed, margin 0: one utterance swapped fails)"),
        "{text}"
    );
    let early = report_with_extra(Rule::Early, early_ms() + 1.0)?;
    let text = render_line_counts(&early);
    assert!(
        text.contains("not on the 300 ms line: 1 (early 1,"),
        "{text}"
    );
    assert!(
        text.contains("quality python: two or more finals 0 / 57"),
        "{text}"
    );
    assert!(text.contains("no final 0"), "{text}");
    Ok(())
}

// ---- the line of a Rust sample follows the rule that ended its final (acceptance) ----
//
// The oracle below reads the files of a run dir on its own, as the `jq` of
// T-RP-32-045 does: it ties each sample to the `final` the tool received when
// the sample closed, that `final` to its `asr.endpoint.decided` line, and counts
// the samples that reach the silence line of that rule.

/// A Rust sample with a `final`: its silence gap in milliseconds and the name of
/// the rule of the `final` that closed it.
struct RuleRow {
    gap_ms: f64,
    rule: String,
}

/// The Rust samples of a dir with a `final`, pooled over its runs, counted
/// against three lines.
struct LineCounts {
    rows: usize,
    /// The rows the early rule closed.
    early_rows: usize,
    /// Valid against the line of each sample's rule.
    per_rule: usize,
    /// Valid against `min_silence` for every sample.
    flat: usize,
    /// Valid against the early line for every sample.
    all_early: usize,
}

/// The JSON rows of a file, blank lines left out; every error names the file.
fn json_rows(path: &Path) -> Result<Vec<Value>, Failure> {
    let named = |why: String| format!("{}: {why}", path.display());
    let text = std::fs::read_to_string(path).map_err(|why| named(why.to_string()))?;
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str::<Value>(line).map_err(|why| named(why.to_string())))
        .collect()
}

/// The rule name of each `asr.endpoint.decided` line of a server's stdout, by `id`.
fn decided_names(log: &[Value]) -> Result<BTreeMap<u64, String>, Failure> {
    let mut names = BTreeMap::new();
    for row in log {
        if row.get("event").and_then(Value::as_str) != Some("asr.endpoint.decided") {
            continue;
        }
        let id = row.get("id").and_then(Value::as_u64);
        let rule = row.get("rule").and_then(Value::as_str);
        let (Some(id), Some(rule)) = (id, rule) else {
            return Err(format!("a decided line has no id or no rule: {row}"));
        };
        names.insert(id, rule.to_owned());
    }
    Ok(names)
}

/// The number of the `final` the tool received at each moment (microseconds).
fn final_numbers(events: &[Value]) -> Result<BTreeMap<i64, u64>, Failure> {
    let mut numbers = BTreeMap::new();
    for row in events {
        if row.pointer("/event/type").and_then(Value::as_str) != Some("final") {
            continue;
        }
        let t_us = row.get("t_us").and_then(Value::as_i64);
        let id = row.pointer("/event/id").and_then(Value::as_str);
        let number = id
            .and_then(|id| id.rsplit_once('-'))
            .and_then(|(_, number)| number.parse::<u64>().ok());
        let (Some(t_us), Some(number)) = (t_us, number) else {
            return Err(format!("a final has no t_us or no numbered id: {row}"));
        };
        numbers.insert(t_us, number);
    }
    Ok(numbers)
}

/// The Rust samples of run `run` in `dir` that have a `final`, each with the
/// rule of the `final` that closed it.
fn rule_rows(dir: &Path, run: usize) -> Result<Vec<RuleRow>, Failure> {
    let samples = json_rows(&dir.join(format!("rs{run}.jsonl")))?;
    let events = json_rows(&dir.join(format!("rs{run}.events.jsonl")))?;
    let log = json_rows(&dir.join(format!("rs{run}.hayamimi.stdout.log")))?;
    let (numbers, rules) = (final_numbers(&events)?, decided_names(&log)?);
    let mut rows = Vec::new();
    for sample in &samples {
        let Some(t_final) = sample.get("t_final_us").and_then(Value::as_i64) else {
            continue;
        };
        let t_end = sample.get("t_end_us").and_then(Value::as_i64);
        let latency = sample.get("final_latency_ms").and_then(Value::as_f64);
        let (Some(t_end), Some(latency)) = (t_end, latency) else {
            return Err(format!(
                "rs{run}: no t_end_us or final_latency_ms: {sample}"
            ));
        };
        let Some(rule) = numbers.get(&t_final).and_then(|number| rules.get(number)) else {
            return Err(format!(
                "rs{run}: no decided line for the final at {t_final}"
            ));
        };
        rows.push(RuleRow {
            gap_ms: (t_final - t_end) as f64 / 1000.0 - latency,
            rule: rule.clone(),
        });
    }
    Ok(rows)
}

/// The silence line in milliseconds of a rule the log names: the early
/// detector's for `early`, `min_silence` for the others.
fn line_of_rule(rule: &str) -> f64 {
    if rule == "early" {
        early_ms()
    } else {
        MIN_SILENCE_MS as f64
    }
}

fn count_reaching(rows: &[RuleRow], line_of: impl Fn(&str) -> f64) -> usize {
    rows.iter()
        .filter(|row| row.gap_ms >= line_of(&row.rule))
        .count()
}

/// The rows of the first `pairs` Rust runs of `dir`, counted against the three lines.
fn line_counts(dir: &Path, pairs: usize) -> Result<LineCounts, Failure> {
    let mut rows = Vec::new();
    for run in 1..=pairs {
        rows.extend(rule_rows(dir, run)?);
    }
    Ok(LineCounts {
        rows: rows.len(),
        early_rows: rows.iter().filter(|row| row.rule == "early").count(),
        per_rule: count_reaching(&rows, line_of_rule),
        flat: count_reaching(&rows, |_| MIN_SILENCE_MS as f64),
        all_early: count_reaching(&rows, |_| early_ms()),
    })
}

/// Reads `dir` with the report and with the oracle and requires the same counts:
/// the report judges a Rust sample against the line of its rule, records the
/// count on `min_silence`, and credits every gained sample to the early rule.
fn counted_by_both(dir: &Path) -> Result<LineCounts, Box<dyn std::error::Error>> {
    let pairs = pairs_to_read(dir)?.pairs;
    let counts = line_counts(dir, pairs)?;
    let report = alternated_report(&load_alternated(dir, pairs)?, None)?;
    let place = dir.display();
    assert_eq!(
        report.rust_share.valid, counts.per_rule,
        "{place}: per rule"
    );
    assert_eq!(report.flat.rust_share.valid, counts.flat, "{place}: flat");
    let gained: usize = report.gained.values().sum();
    assert_eq!(gained + counts.flat, counts.per_rule, "{place}: gained");
    assert_eq!(
        report.gained.get("early"),
        Some(&gained),
        "{place}: by rule"
    );
    Ok(counts)
}

/// The recorded dir: the log names both rules, and the early ones are the gain.
fn check_recorded_dir() -> TestResult {
    let counts = counted_by_both(&rp32_dir())?;
    println!(
        "rp32 rules: {} samples with a final, {} closed by early; valid {} per rule, {} on min_silence, {} on the early line",
        counts.rows, counts.early_rows, counts.per_rule, counts.flat, counts.all_early
    );
    assert!(counts.early_rows > 0 && counts.early_rows < counts.rows);
    assert!(counts.per_rule > counts.flat);
    Ok(())
}

/// A 200 ms gap lies between the two lines: valid when the early rule closed the
/// sample, invalid when the silence rule did. A line chosen without the rule gets
/// one of the two wrong.
fn check_gap_between_the_lines() -> TestResult {
    let early = counted_by_both(&dir_with_gap_200_closed_by("r1317-early", "early")?)?;
    let silence = counted_by_both(&dir_with_gap_200_closed_by("r1317-silence", "silence")?)?;
    assert_eq!(early.per_rule, silence.per_rule + 1);
    assert_eq!(silence.all_early, silence.per_rule + 1);
    assert_eq!(early.flat + 1, early.per_rule);
    Ok(())
}

/// A `final` with no decided line gets no line: the dir is refused by run and
/// id, and is not read as closed by the silence rule.
fn check_unjoined_final() -> TestResult {
    let dir = f32_dir("r1317-unjoined", 3, None)?;
    let log = dir.join("rs2.hayamimi.stdout.log");
    let kept: Vec<String> = std::fs::read_to_string(&log)?
        .lines()
        .filter(|line| !line.contains("\"id\":7,"))
        .map(str::to_owned)
        .collect();
    std::fs::write(&log, kept.join("\n") + "\n")?;
    let said = load_alternated(&dir, 3);
    let named = |why: &String| why.contains("rs2") && why.contains("mic-7");
    assert!(said.as_ref().err().is_some_and(named), "{said:?}");
    Ok(())
}

/// The validity line of a Rust sample follows the rule that ended the `final`
/// that closed it (3.1, the exception for RP-32): `early` is held to
/// `EARLY_MIN_SILENCE_MS`, every other rule to `min_silence`, and a `final` the
/// log does not name has no line.
#[test]
fn rp32_validity_line_follows_the_rule_that_ended_each_final() -> TestResult {
    check_recorded_dir()?;
    check_gap_between_the_lines()?;
    check_unjoined_final()
}

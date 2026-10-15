//! RP-32: the share of valid samples that got a translation.
//!
//! A run whose translations the checker stops leaves those samples out of
//! `L`, so a small `L` over the rest proves nothing. The share is counted over
//! the valid samples (3.1: a `final` is there and `L_asr - L_dec` reaches the
//! run's `--min-silence`), with the same validity function RP-3 uses, and it
//! must reach 20 / 23.

mod support;

use hayamimi_eval::compare::{
    check_percentiles, check_translated_share, comparison_threshold, read_samples, translated_share,
};
use hayamimi_eval::latency::{Sample, Stat, Window, build_samples, parse_event};
use serde_json::Value;
use support::e2e::columns_of;
use support::rp32::{
    AltReport, AltRun, Implementation, check_alternated_runs, check_columns_written,
    l_comparison_set, load_alternated, load_hundredths, missing_files, parse_load_file,
    render_columns, rp32_dir,
};
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

const LOAD_OK: &str = "0.50 0.50 0.50 1/300 1000";

/// F32: `t_end` 0, `L_asr` 700 + u ms, `L` 1000 + 10u + k ms (utterance u, pair k).
/// The silence gap is 600 + u ms, over the 300 ms limit, so every sample is valid.
fn f32_sample(utt: u32, k: u32) -> Sample {
    Sample {
        repeat: 1,
        utt,
        t_start_us: 0,
        t_end_us: 0,
        t_partial_us: None,
        t_partial_tr_us: None,
        t_final_us: Some(i64::from(700 + utt) * 1000),
        t_tr_us: Some(i64::from(1000 + 10 * utt + k) * 1000),
        finals: 1,
        text: String::new(),
        translation: None,
        final_latency_ms: Some(100.0),
        cer_dist: 0,
        cer_denom: 0,
    }
}

fn f32_run(implementation: Implementation, k: u32) -> AltRun {
    AltRun {
        implementation,
        load_before: Some(String::from(LOAD_OK)),
        load_after: Some(String::from(LOAD_OK)),
        samples: (1..=19).map(|utt| f32_sample(utt, k)).collect(),
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

/// The real runs of `docs/results/rp32/`: Python, Rust, Python, Rust, Python, Rust.
#[test]
#[ignore = "reads the runs in docs/results/rp32/"]
fn rp32_alternated_runs_under_the_same_load_beat_python() -> TestResult {
    let runs = load_alternated(&rp32_dir(), 3)?;
    let said = check_alternated_runs(&runs, None);
    if let Ok(report) = &said {
        println!("rp32 python:\n{}", render_columns(&report.python));
        println!("rp32 rust:\n{}", render_columns(&report.rust));
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

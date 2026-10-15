//! RP-32: the share of valid samples that got a translation.
//!
//! A run whose translations the checker stops leaves those samples out of
//! `L`, so a small `L` over the rest proves nothing. The share is counted over
//! the valid samples (3.1: a `final` is there and `L_asr - L_dec` reaches the
//! run's `--min-silence`), with the same validity function RP-3 uses, and it
//! must reach 20 / 23.

mod support;

use hayamimi_eval::compare::{
    TranslatedShare, check_percentiles, check_translated_share, comparison_threshold, read_samples,
    translated_share,
};
use hayamimi_eval::latency::{Sample, Stat, Window, build_samples, parse_event};
use serde_json::Value;
use support::e2e::columns_of;
use support::rp32::{
    AltReport, AltRun, Implementation, SHARE_COLUMN_PYTHON, SHARE_COLUMN_RUST, alternated_report,
    check_alternated_runs, check_columns_written, check_dir, check_share_columns_written,
    l_comparison_set, load_alternated, load_hundredths, missing_files, pairs_to_read,
    parse_load_file, parse_pairs_text, render_columns, render_share_columns, rp32_dir,
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

/// Writes the F32 runs of `pairs` pairs into a scratch directory under the
/// build tree and returns it: `py<N>.jsonl`, `rs<N>.jsonl` (Rust equals
/// Python) and `load<N>.txt` per pair, and `pairs_txt` as `pairs.txt` when given.
/// `name` keeps the tests that run side by side apart.
fn f32_dir(
    name: &str,
    pairs: u32,
    pairs_txt: Option<&str>,
) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../target/rp32-test-dirs")
        .join(name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    std::fs::create_dir_all(&dir)?;
    for k in 1..=pairs {
        let mut load = String::new();
        for run in [format!("py{k}"), format!("rs{k}")] {
            let samples: Vec<String> = (1..=19)
                .map(|utt| serde_json::to_string(&f32_sample(utt, k)))
                .collect::<Result<_, _>>()?;
            std::fs::write(dir.join(format!("{run}.jsonl")), samples.join("\n") + "\n")?;
            for when in ["before", "after"] {
                load.push_str(&format!("{run} {when} 00:00:00 {LOAD_OK}\n"));
            }
        }
        std::fs::write(dir.join(format!("load{k}.txt")), load)?;
    }
    if let Some(text) = pairs_txt {
        std::fs::write(dir.join("pairs.txt"), text)?;
    }
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
    let said = check_alternated_runs(&runs, count.recorded);
    if let Ok(report) = &said {
        println!("rp32 python:\n{}", render_columns(&report.python));
        println!("rp32 rust:\n{}", render_columns(&report.rust));
        println!("rp32 share:\n{}", render_share_columns(report));
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

/// T-RP-32-045: the recorded runs hold 277 translated of 300 valid Rust samples,
/// which reaches 20 / 23 (277 x 23 = 6371 >= 6000). Whether the dir passes the
/// other checks or not, the share is not among its reasons.
#[test]
fn the_recorded_rp32_dir_has_enough_translated_samples() -> TestResult {
    let counted = recorded_rust_share()?;
    assert_eq!(counted, (300, 277));
    assert!(counted.1 * 23 >= counted.0 * 20);
    let dir = rp32_dir();
    let count = pairs_to_read(&dir)?;
    let report = alternated_report(&load_alternated(&dir, count.pairs)?, count.recorded)?;
    assert_eq!(report.rust_share, share_of(300, 277));
    let text = render_share_columns(&report);
    assert!(
        text.contains(&format!("{SHARE_COLUMN_RUST} 277 / 300")),
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

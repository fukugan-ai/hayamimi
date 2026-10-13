//! RB-6: the `--min-silence` sweep. Four values are each played once, and
//! the smallest one whose Japanese CER does not exceed that of the default
//! is named.
//!
//! The inputs are the samples files `lat_ja_<s>_<n>.jsonl` and the RB-6
//! section of `docs/results/baseline_python.md`.

mod support;

use std::path::Path;

use hayamimi_eval::cer::cer;
use support::{
    FIXTURES, Failure, RESULTS, Row, TestResult, load_rows, micros, nearest_rank, parse_rows, read,
    run_files, section, table_rows,
};

const SILENCES: [&str; 4] = ["0.20", "0.25", "0.30", "0.35"];
const DEFAULT_SILENCE: &str = "0.35";

/// One row of the sweep table.
struct SweepRow {
    silence: String,
    p50: i64,
    p95: i64,
    cer_micros: i64,
}

fn integer(text: &str) -> Result<i64, Failure> {
    text.parse::<i64>()
        .map_err(|_| format!("{text:?} is not an integer"))
}

/// The four rows of the table whose first column is `min-silence`.
fn sweep_table(section_text: &str) -> Result<Vec<SweepRow>, Failure> {
    let rows = table_rows(section_text);
    let Some(head) = rows
        .iter()
        .position(|cells| cells.first() == Some(&"min-silence"))
    else {
        return Err(String::from("no sweep table"));
    };
    let width = rows[head].len();
    let body: Vec<&Vec<&str>> = rows[head + 1..]
        .iter()
        .take_while(|cells| cells.len() == width)
        .collect();
    let mut found = Vec::new();
    for silence in SILENCES {
        let Some(cells) = body.iter().find(|cells| cells.first() == Some(&silence)) else {
            return Err(format!("{silence} has no row"));
        };
        found.push(SweepRow {
            silence: silence.to_owned(),
            p50: integer(cells[1])?,
            p95: integer(cells[2])?,
            cer_micros: micros(cells[3])?,
        });
    }
    if body.len() != SILENCES.len() {
        return Err(format!("{} rows, want {}", body.len(), SILENCES.len()));
    }
    Ok(found)
}

/// The smallest value whose CER does not exceed the default's.
fn choose_min_silence(cers: &[(&str, i64)]) -> Result<String, Failure> {
    let Some((_, limit)) = cers.iter().find(|(silence, _)| *silence == DEFAULT_SILENCE) else {
        return Err(format!("no CER for {DEFAULT_SILENCE}"));
    };
    let mut ordered = cers.to_vec();
    ordered.sort_by(|a, b| a.0.cmp(b.0));
    match ordered.iter().find(|(_, value)| value <= limit) {
        Some((silence, _)) => Ok((*silence).to_owned()),
        None => Err(String::from("no value is within the default's CER")),
    }
}

fn cer_pairs(rows: &[SweepRow]) -> Vec<(&str, i64)> {
    rows.iter()
        .map(|row| (row.silence.as_str(), row.cer_micros))
        .collect()
}

/// Exactly one value is named, and it is the one the rule picks.
fn check_named(section_text: &str, rows: &[SweepRow]) -> Result<String, Failure> {
    let named: Vec<&str> = section_text
        .lines()
        .filter_map(|line| line.strip_prefix("名指し："))
        .map(str::trim)
        .collect();
    let [one] = named.as_slice() else {
        return Err(format!("{} values are named, want 1", named.len()));
    };
    let chosen = choose_min_silence(&cer_pairs(rows))?;
    if *one == chosen {
        Ok(chosen)
    } else {
        Err(format!("{one} is named, the rule picks {chosen}"))
    }
}

/// Two numbers are within a factor of ten when neither reaches ten times the other.
fn within_factor_of_ten(a: i64, b: i64) -> bool {
    a < 10 * b && b < 10 * a
}

fn summary_section() -> Result<String, Failure> {
    let summary = read(&Path::new(RESULTS).join("baseline_python.md"))?;
    Ok(section(&summary, "RB-6")?.to_owned())
}

/// The rows of the first run of one `--min-silence` value.
fn first_run(silence: &str) -> Result<Vec<Row>, Failure> {
    match run_files(silence).first() {
        Some((_, path)) => load_rows(path),
        None => Err(format!("no run for --min-silence {silence}")),
    }
}

/// The recognition delays `L_asr` of the valid samples of a run, in whole
/// milliseconds. A sample is valid when it has a `final` and `L_asr - L_dec`
/// reaches the run's `--min-silence`; samples without a `final` and samples
/// whose end is wrong are left out.
fn valid_l_asr(rows: &[Row], silence: &str) -> Result<Vec<i64>, Failure> {
    // `--min-silence` is in seconds; the gap below is in units of 10^-6 ms.
    let limit = micros(silence)? * 1000;
    let mut found = Vec::new();
    for row in rows {
        let Some(t_final) = row.t_final else {
            continue;
        };
        let Some(l_dec) = row.final_latency_ms else {
            return Err(format!(
                "utterance {} repeat {}: a final without latency_ms",
                row.utt, row.repeat
            ));
        };
        let gap = (t_final - row.t_end) * 1000 - (l_dec * 1_000_000.0).round() as i64;
        if gap >= limit {
            found.extend(row.l_asr_ms());
        }
    }
    Ok(found)
}

fn l_asr_percentiles(rows: &[Row], silence: &str) -> Result<(i64, i64), Failure> {
    let values = valid_l_asr(rows, silence)?;
    Ok((nearest_rank(&values, 50)?, nearest_rank(&values, 95)?))
}

/// The corpus CER of a run in units of 10^-4, halves up.
fn cer_in_ten_thousandths(rows: &[Row]) -> Result<i64, Failure> {
    let dist: i64 = rows.iter().map(|row| row.cer_dist).sum();
    let denom: i64 = rows.iter().map(|row| row.cer_denom).sum();
    if denom == 0 {
        return Err(String::from("the references are empty"));
    }
    Ok((dist * 1_000_000 / denom + 50) / 100)
}

#[test]
fn rb06_min_silence_sweep_names_one_value_four_min_silence_values_have_runs() {
    for silence in SILENCES {
        assert!(!run_files(silence).is_empty(), "--min-silence {silence}");
    }
}

#[test]
fn rb06_min_silence_sweep_names_one_value_table_has_the_four_rows() -> TestResult {
    let table = sweep_table(&summary_section()?)?;
    let silences: Vec<&str> = table.iter().map(|row| row.silence.as_str()).collect();
    assert_eq!(silences, SILENCES);
    Ok(())
}

#[test]
fn rb06_min_silence_sweep_names_one_value_table_missing_a_row_is_rejected() {
    let said = sweep_table(include_str!("fixtures/rb06_three_rows.md"));
    assert!(matches!(&said, Err(text) if text.contains("0.25")));
}

#[test]
fn rb06_min_silence_sweep_names_one_value_table_values_match_recomputed_l_asr_percentiles_over_valid_samples()
-> TestResult {
    for row in sweep_table(&summary_section()?)? {
        let (p50, p95) = l_asr_percentiles(&first_run(&row.silence)?, &row.silence)?;
        assert_eq!(
            (row.p50, row.p95),
            (p50, p95),
            "--min-silence {}",
            row.silence
        );
    }
    Ok(())
}

#[test]
fn rb06_min_silence_sweep_names_one_value_table_cer_matches_the_recomputed_ja_cer() -> TestResult {
    for row in sweep_table(&summary_section()?)? {
        let recomputed = cer_in_ten_thousandths(&first_run(&row.silence)?)?;
        assert_eq!(
            row.cer_micros / 100,
            recomputed,
            "--min-silence {}",
            row.silence
        );
    }
    Ok(())
}

#[test]
fn rb06_min_silence_sweep_names_one_value_tie_with_the_default_cer_is_accepted() -> TestResult {
    let cers = [
        ("0.20", 50_000),
        ("0.25", 40_000),
        ("0.30", 36_000),
        ("0.35", 36_000),
    ];
    assert_eq!(choose_min_silence(&cers)?, "0.30");
    Ok(())
}

#[test]
fn rb06_min_silence_sweep_names_one_value_all_equal_cer_names_the_smallest_value() -> TestResult {
    let cers = [
        ("0.20", 36_000),
        ("0.25", 36_000),
        ("0.30", 36_000),
        ("0.35", 36_000),
    ];
    assert_eq!(choose_min_silence(&cers)?, "0.20");
    Ok(())
}

#[test]
fn rb06_min_silence_sweep_names_one_value_value_one_step_above_default_cer_is_not_chosen()
-> TestResult {
    let cers = [
        ("0.20", 37_000),
        ("0.25", 37_000),
        ("0.30", 37_000),
        ("0.35", 36_000),
    ];
    assert_eq!(choose_min_silence(&cers)?, "0.35");
    Ok(())
}

#[test]
fn rb06_min_silence_sweep_names_one_value_baseline_names_exactly_one_value_matching_the_rule()
-> TestResult {
    let section_text = summary_section()?;
    check_named(&section_text, &sweep_table(&section_text)?)?;
    Ok(())
}

#[test]
fn rb06_min_silence_sweep_names_one_value_two_named_values_are_rejected() -> TestResult {
    let text = include_str!("fixtures/rb06_two_values_named.md");
    let said = check_named(text, &sweep_table(text)?);
    assert!(matches!(&said, Err(text) if text.contains("2 values")));
    Ok(())
}

#[test]
fn rb06_min_silence_sweep_names_one_value_no_named_value_is_rejected() -> TestResult {
    let text = include_str!("fixtures/rb06_no_value_named.md");
    let said = check_named(text, &sweep_table(text)?);
    assert!(matches!(&said, Err(text) if text.contains("0 values")));
    Ok(())
}

#[test]
fn rb06_min_silence_sweep_names_one_value_default_row_is_within_a_factor_of_ten_of_rb05()
-> TestResult {
    let table = sweep_table(&summary_section()?)?;
    let Some(default_row) = table.iter().find(|row| row.silence == DEFAULT_SILENCE) else {
        return Err(Box::from("no row for the default value"));
    };
    let (p50, p95) = l_asr_percentiles(&first_run(DEFAULT_SILENCE)?, DEFAULT_SILENCE)?;
    assert!(within_factor_of_ten(default_row.p50, p50));
    assert!(within_factor_of_ten(default_row.p95, p95));
    Ok(())
}

#[test]
fn rb06_min_silence_sweep_names_one_value_factor_of_ten_boundary_between_999_and_1000() {
    assert!(within_factor_of_ten(100, 999));
    assert!(!within_factor_of_ten(100, 1000));
}

#[test]
fn rb06_min_silence_sweep_names_one_value_cer_one_substitution_in_five_is_0_2() {
    assert_eq!(cer("こんにちは", "こんにちわ").micros(), 200_000);
}

#[test]
fn rb06_min_silence_sweep_names_one_value_cer_ignores_punctuation() {
    assert_eq!(cer("こんにちは。", "こんにちは").micros(), 0);
}

#[test]
fn rb06_min_silence_sweep_names_one_value_cer_applies_nfkc() {
    assert_eq!(cer("ＡＢＣ", "ABC").micros(), 0);
}

#[test]
fn rb06_min_silence_sweep_names_one_value_cer_of_empty_hypothesis_is_1() {
    assert_eq!(cer("こんにちは", "").micros(), 1_000_000);
}

#[test]
fn rb06_min_silence_sweep_names_one_value_cer_of_empty_reference_is_0_not_a_division() {
    let empty = cer("。", "あ");
    assert_eq!(empty.denom, 0);
    assert_eq!(empty.micros(), 0);
}

#[test]
fn rb06_min_silence_sweep_names_one_value_bad_end_sample_is_left_out_of_the_sweep_l_asr_percentiles()
-> TestResult {
    let rows = parse_rows(include_str!("fixtures/rb06_sweep_bad_end.jsonl"))?;
    assert_eq!(rows.len(), 6);
    let (p50, _) = l_asr_percentiles(&rows, "0.30")?;
    assert_eq!(p50, 1200);
    // The control: counting the sample with a bad end moves the median.
    let counted: Vec<i64> = rows.iter().filter_map(Row::l_asr_ms).collect();
    assert_eq!(nearest_rank(&counted, 50)?, 1100);
    Ok(())
}

/// `(--min-silence, n, p50, p95)` of the valid samples, recounted with jq.
const JQ_RECOUNT: [(&str, usize, i64, i64); 4] = [
    ("0.20", 100, 1010, 1542),
    ("0.25", 100, 995, 1635),
    ("0.30", 100, 1104, 1724),
    ("0.35", 100, 1185, 1779),
];

#[test]
fn rb06_min_silence_sweep_names_one_value_sweep_rows_match_the_jq_recount_over_valid_samples()
-> TestResult {
    for (silence, count, p50, p95) in JQ_RECOUNT {
        let rows = first_run(silence)?;
        let valid = valid_l_asr(&rows, silence)?;
        assert_eq!(valid.len(), count, "--min-silence {silence}");
        assert_eq!(
            l_asr_percentiles(&rows, silence)?,
            (p50, p95),
            "--min-silence {silence}"
        );
    }
    // The control: the default's median over every sample is 1161, not 1185.
    let rows = first_run(DEFAULT_SILENCE)?;
    let every: Vec<i64> = rows.iter().filter_map(Row::l_asr_ms).collect();
    assert_eq!(nearest_rank(&every, 50)?, 1161);
    Ok(())
}

#[test]
fn rb06_min_silence_sweep_names_one_value_missing_sweep_run_names_its_path() {
    let absent = Path::new(FIXTURES)
        .join("absent")
        .join("lat_ja_0.20_1.jsonl");
    assert!(matches!(load_rows(&absent), Err(path) if path.ends_with("lat_ja_0.20_1.jsonl")));
}

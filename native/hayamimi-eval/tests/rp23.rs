//! RP-23: every stage of the design table is timed on the GPU and on the CPU,
//! idle and under load, and the first recognition after an empty JIT cache is
//! a row of its own.

mod support;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use hayamimi_eval::gpu_results::{
    ENVIRONMENT, JIT, JIT_HEADER, Load, Results, ResultsError, TRANSFER_HEADER, TRANSFERS,
    UNMEASURED, check_repeat_p50, check_sample_count, check_stage_results,
};
use hayamimi_eval::manifest::{Row as ManifestRow, read_rows, write_rows};
use hayamimi_eval::mdtable::render;
use hayamimi_eval::stage_table::{StageRow, parse_stage_table};
use support::stages::{self, LOAD, PROVIDER, REPEATS, Row, SAMPLES, STAGE};
use support::{REPO, RESULTS, TESTDATA, TestResult, read};

/// Where the generated manifest of the tests is written.
const SCRATCH: &str = env!("CARGO_TARGET_TMPDIR");

/// A manifest of `rows` rows, written and read back through the crate's own reader.
fn generated_manifest_rows(rows: usize) -> Result<usize, Box<dyn std::error::Error>> {
    let path: PathBuf = Path::new(SCRATCH).join(format!("rp23_manifest_{rows}.json"));
    let body: Vec<ManifestRow> = (0..rows).map(|_| ManifestRow::new()).collect();
    write_rows(&path, &body)?;
    Ok(read_rows(&path)?.len())
}

/// The manifest rows of the generated recording, read once: tests run in parallel
/// and the manifest is one file.
static MANIFEST_ROWS: OnceLock<Result<usize, String>> = OnceLock::new();

/// The sample count the rows of one stage must hold: the manifest rows times the repeats.
fn expected_samples() -> Result<usize, Box<dyn std::error::Error>> {
    let rows = MANIFEST_ROWS.get_or_init(|| {
        generated_manifest_rows(stages::UTTERANCES).map_err(|error| error.to_string())
    });
    match rows {
        Ok(rows) => Ok(rows * REPEATS),
        Err(message) => Err(Box::from(message.clone())),
    }
}

fn document(cells: &[Row]) -> String {
    [
        stages::environment_section(),
        stages::cells_section(cells),
        stages::transfers_section(&stages::transfer_rows("fp32", "40", "1000", "100")),
        stages::jit_section(&[("empty", "900"), ("warm", "700")]),
    ]
    .join("\n")
}

fn parsed(text: &str) -> Result<Results, ResultsError> {
    Results::parse(text)
}

fn table() -> Vec<StageRow> {
    stages::gpu_rows(&stages::stage_names())
}

fn complete_cells() -> Result<Vec<Row>, Box<dyn std::error::Error>> {
    Ok(stages::cell_rows(
        &stages::stage_names(),
        expected_samples()?,
    ))
}

/// T-RP-23-001
#[test]
fn complete_results_pass() -> TestResult {
    let results = parsed(&document(&complete_cells()?))?;
    check_stage_results(&table(), &results, expected_samples()?)?;
    Ok(())
}

/// T-RP-23-002
#[test]
fn missing_cell_is_named() -> TestResult {
    let cells: Vec<Row> = complete_cells()?
        .into_iter()
        .filter(|row| !(row[STAGE] == "stage 03" && row[PROVIDER] == "cuda" && row[LOAD] == "負荷"))
        .collect();
    let results = parsed(&document(&cells))?;
    let said = check_stage_results(&table(), &results, expected_samples()?);
    assert_eq!(
        said,
        Err(ResultsError::MissingCell {
            stage: String::from("stage 03"),
            provider: "cuda",
            load: Load::Loaded,
        })
    );
    Ok(())
}

/// T-RP-23-003
#[test]
fn row_without_p95_is_rejected() -> TestResult {
    let mut cells = complete_cells()?;
    cells[0][stages::P95] = String::from(UNMEASURED);
    let results = parsed(&document(&cells))?;
    let said = check_stage_results(&table(), &results, expected_samples()?);
    assert!(
        matches!(said, Err(ResultsError::NoPercentile { which: "p95", .. })),
        "{said:?}"
    );
    Ok(())
}

/// T-RP-23-004: one sample short of utterances times repeats.
#[test]
fn sample_count_one_short_of_utterances_times_repeats_is_rejected() -> TestResult {
    let expected = expected_samples()?;
    let said = check_sample_count("stage 01", expected - 1, expected / REPEATS, REPEATS);
    assert_eq!(
        said,
        Err(ResultsError::SampleCount {
            stage: String::from("stage 01"),
            expected,
            found: expected - 1,
        })
    );
    // The same through a whole document: one row of the table is one short.
    let mut cells = complete_cells()?;
    cells[0][SAMPLES] = (expected - 1).to_string();
    let results = parsed(&document(&cells))?;
    let said = check_stage_results(&table(), &results, expected);
    assert!(
        matches!(said, Err(ResultsError::SampleCount { .. })),
        "{said:?}"
    );
    Ok(())
}

/// T-RP-23-005
#[test]
fn sample_count_of_utterances_times_repeats_is_accepted() -> TestResult {
    let expected = expected_samples()?;
    assert_eq!(expected, stages::UTTERANCES * REPEATS);
    check_sample_count("stage 01", expected, expected / REPEATS, REPEATS)?;
    Ok(())
}

/// T-RP-23-006
#[test]
fn row_without_provider_is_not_a_gpu_row() -> TestResult {
    let mut cells = complete_cells()?;
    cells[1][PROVIDER] = String::new();
    let said = parsed(&document(&cells));
    assert!(
        matches!(said, Err(ResultsError::NoProvider { .. })),
        "{said:?}"
    );
    Ok(())
}

/// T-RP-23-007: 1000 and 1100 differ by exactly 10% of the first.
#[test]
fn repeat_p50_difference_of_ten_percent_passes() -> TestResult {
    check_repeat_p50(1000, 1100)?;
    Ok(())
}

/// T-RP-23-008
#[test]
fn repeat_p50_difference_over_ten_percent_is_rejected() {
    assert_eq!(
        check_repeat_p50(1000, 1101),
        Err(ResultsError::RepeatDifference {
            first: 1000,
            second: 1101,
        })
    );
}

fn check_all(text: &str) -> Result<(), Box<dyn std::error::Error>> {
    check_stage_results(&table(), &parsed(text)?, expected_samples()?)?;
    Ok(())
}

/// T-RP-23-009: no transfer section, or a transfer table without bytes and time.
#[test]
fn result_without_transfer_fields_is_rejected() -> TestResult {
    let without_section = [
        stages::environment_section(),
        stages::cells_section(&complete_cells()?),
        stages::jit_section(&[("empty", "900"), ("warm", "700")]),
    ]
    .join("\n");
    let said = check_stage_results(&table(), &parsed(&without_section)?, expected_samples()?);
    assert_eq!(said, Err(ResultsError::NoSection(TRANSFERS)));

    let header = &TRANSFER_HEADER[..4];
    let rows: Vec<Row> = stages::transfer_rows("fp32", "40", "1000", "100")
        .into_iter()
        .map(|row| row[..4].to_vec())
        .collect();
    let text = format!("{TRANSFERS}\n\n{}\n", render(header, &rows));
    let said = parsed(&text);
    assert!(
        matches!(said, Err(ResultsError::NoColumn { column, .. }) if column.contains("bytes")),
        "{said:?}"
    );
    Ok(())
}

/// T-RP-23-010
#[test]
fn result_without_machine_and_build_summary_is_rejected() -> TestResult {
    let without_environment = [
        stages::cells_section(&complete_cells()?),
        stages::transfers_section(&stages::transfer_rows("fp32", "40", "1000", "100")),
        stages::jit_section(&[("empty", "900"), ("warm", "700")]),
    ]
    .join("\n");
    let said = check_stage_results(
        &table(),
        &parsed(&without_environment)?,
        expected_samples()?,
    );
    assert_eq!(said, Err(ResultsError::NoSection(ENVIRONMENT)));
    Ok(())
}

fn with_jit(jit: String) -> Result<String, Box<dyn std::error::Error>> {
    Ok([
        stages::environment_section(),
        stages::cells_section(&complete_cells()?),
        stages::transfers_section(&stages::transfer_rows("fp32", "40", "1000", "100")),
        jit,
    ]
    .join("\n"))
}

/// T-RP-23-012
#[test]
fn result_without_empty_jit_cache_decode_row_is_rejected() -> TestResult {
    let text = with_jit(stages::jit_section(&[("warm", "700")]))?;
    let said = check_all(&text);
    assert_eq!(
        said.err().map(|e| e.to_string()),
        Some(ResultsError::NoJitRow("empty").to_string())
    );
    Ok(())
}

/// T-RP-23-013
#[test]
fn result_without_warm_jit_cache_decode_row_is_rejected() -> TestResult {
    let text = with_jit(stages::jit_section(&[("empty", "900")]))?;
    let said = check_all(&text);
    assert_eq!(
        said.err().map(|e| e.to_string()),
        Some(ResultsError::NoJitRow("warm").to_string())
    );
    Ok(())
}

/// T-RP-23-014: one row for both runs, and a table with no cache column at all.
#[test]
fn merged_cache_state_decode_rows_are_rejected() -> TestResult {
    let merged = with_jit(stages::jit_section(&[("empty and warm", "800")]))?;
    let said = check_all(&merged);
    assert_eq!(
        said.err().map(|e| e.to_string()),
        Some(ResultsError::MergedJitRows(String::from("empty and warm")).to_string())
    );

    let header = &JIT_HEADER[1..];
    let rows: Vec<Row> = vec![vec![
        String::from("1"),
        String::from("800"),
        String::from("700000"),
    ]];
    let no_cache_column = format!("{JIT}\n\n{}\n", render(header, &rows));
    let said = parsed(&no_cache_column);
    assert!(
        matches!(
            said,
            Err(ResultsError::NoColumn {
                column: "cache",
                ..
            })
        ),
        "{said:?}"
    );
    Ok(())
}

/// The generated manifest has the number of rows it was asked for.
#[test]
fn generated_manifest_has_the_requested_rows() -> TestResult {
    assert_eq!(generated_manifest_rows(0)?, 0);
    assert_eq!(generated_manifest_rows(7)?, 7);
    Ok(())
}

/// T-RP-23-011 and the acceptance test: the results written by the stage runs
/// pass the check of T-RP-23-001, with the sample count taken from the real manifest.
#[test]
#[ignore = "reads the real design, the real manifest and the real results"]
fn every_stage_is_timed_on_gpu_and_cpu() -> TestResult {
    let design = read(&Path::new(REPO).join("docs/design/rust_port.ja.md"))?;
    let results = read(&Path::new(RESULTS).join("gpu_stages.md"))?;
    let manifest = Path::new(TESTDATA).join("manifests/lat-ja.json");
    let expected = read_rows(&manifest)?.len() * REPEATS;
    check_stage_results(
        &parse_stage_table(&design)?,
        &Results::parse(&results)?,
        expected,
    )?;
    Ok(())
}

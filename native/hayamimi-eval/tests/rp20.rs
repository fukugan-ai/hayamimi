//! RP-20: every placement in the stage table of the design is decided by a
//! measurement, and every stage has a row in the results.

mod support;

use std::path::Path;

use hayamimi_eval::gpu_results::{CELL_HEADER, Results, ResultsError, check_placements_measured};
use hayamimi_eval::mdtable::render;
use hayamimi_eval::stage_table::{StageRow, count_measure_rows, parse_stage_table};
use support::stages::{self, Row};
use support::{FIXTURES, REPO, RESULTS, TestResult, read};

fn placed(name: &str, placement: &str) -> StageRow {
    StageRow {
        name: name.to_owned(),
        placement: placement.to_owned(),
        evidence: String::from("x"),
    }
}

fn results_for(names: &[String]) -> Result<Results, ResultsError> {
    let rows: Vec<Row> = stages::cell_rows(names, stages::UTTERANCES * stages::REPEATS);
    Results::parse(&stages::cells_section(&rows))
}

/// T-RP-20-001
#[test]
fn counter_reads_a_partial_measure_row() {
    let rows = [
        placed("a", "GPU"),
        placed("b", "CPU"),
        placed("c", "GPU（解き直し）。判定は測る"),
    ];
    assert_eq!(count_measure_rows(&rows), 1);
}

/// T-RP-20-002: the same counter reads 8 rows in the table as it was before R1.
#[test]
fn table_before_r1_has_eight_measure_rows() -> TestResult {
    let before = read(&Path::new(FIXTURES).join("rp20_table_before_r1.md"))?;
    let table = parse_stage_table(&before)?;
    assert_eq!(table.len(), 19);
    assert_eq!(count_measure_rows(&table), 8);
    Ok(())
}

/// T-RP-20-003
#[test]
fn table_without_measure_rows_passes() -> TestResult {
    let names = vec![String::from("a"), String::from("b")];
    let table = [placed("a", "GPU"), placed("b", "CPU")];
    check_placements_measured(&table, &results_for(&names)?)?;
    Ok(())
}

/// T-RP-20-004
#[test]
fn one_remaining_measure_row_is_named() -> TestResult {
    let names = vec![String::from("a"), String::from("b")];
    let table = [placed("a", "GPU"), placed("b", "測る")];
    let said = check_placements_measured(&table, &results_for(&names)?);
    assert_eq!(
        said,
        Err(ResultsError::MeasureRowRemains(String::from("b")))
    );
    Ok(())
}

/// T-RP-20-005
#[test]
fn stage_without_a_result_row_is_named() -> TestResult {
    let names = vec![String::from("a")];
    let table = [placed("a", "GPU"), placed("b", "CPU")];
    let said = check_placements_measured(&table, &results_for(&names)?);
    assert_eq!(said, Err(ResultsError::NoResultRow(String::from("b"))));
    Ok(())
}

/// T-RP-20-006
#[test]
fn all_nineteen_rows_have_results_pass() -> TestResult {
    let names = stages::stage_names();
    assert_eq!(names.len(), 19);
    let table = stages::gpu_rows(&names);
    check_placements_measured(&table, &results_for(&names)?)?;
    Ok(())
}

/// The header of the cell table is the one the document is written with.
#[test]
fn the_fixture_cells_use_the_document_header() {
    let text = render(&CELL_HEADER, &[]);
    assert!(text.starts_with("| 段 | provider |"));
}

/// T-RP-20-007 and the acceptance test `rp20_every_stage_placement_is_measured`:
/// the design table and `docs/results/gpu_stages.md` as they are now.
#[test]
#[ignore = "reads the real design and the real results; fails until the design table is decided"]
fn rp20_every_stage_placement_is_measured() -> TestResult {
    let design = read(&Path::new(REPO).join("docs/design/rust_port.ja.md"))?;
    let results = read(&Path::new(RESULTS).join("gpu_stages.md"))?;
    let table = parse_stage_table(&design)?;
    check_placements_measured(&table, &Results::parse(&results)?)?;
    Ok(())
}

/// T-RP-20-007 under its case-table name.
#[test]
#[ignore = "reads the real design and the real results; fails until the design table is decided"]
fn every_stage_placement_is_measured() -> TestResult {
    rp20_every_stage_placement_is_measured()
}

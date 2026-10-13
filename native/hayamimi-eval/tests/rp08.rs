//! RP-8: a stage left on the CPU has its evidence in the results, and a stage
//! on the GPU has the same measurements.

mod support;

use std::path::Path;

use hayamimi_eval::gpu_results::{Results, ResultsError, check_stage_evidence};
use hayamimi_eval::stage_table::{StageRow, parse_stage_table};
use support::stages::{self, Row, STAGE};
use support::{REPO, RESULTS, TestResult, read};

fn table_row(placement: &str, evidence: &str) -> StageRow {
    StageRow {
        name: String::from("a"),
        placement: placement.to_owned(),
        evidence: evidence.to_owned(),
    }
}

/// Results for stage `a`: the idle GPU and CPU p50 and p95 given, other cells fixed.
fn results_with(gpu: (&str, &str), cpu: (&str, &str)) -> Result<Results, ResultsError> {
    let rows: Vec<Row> = [("cuda", gpu), ("cpu", cpu)]
        .iter()
        .map(|(provider, (p50, p95))| {
            vec![
                String::from("a"),
                (*provider).to_owned(),
                String::from("fp32"),
                String::from("無負荷"),
                String::from("1"),
                String::from("115"),
                (*p50).to_owned(),
                (*p95).to_owned(),
                String::new(),
            ]
        })
        .collect();
    Results::parse(&stages::cells_section(&rows))
}

fn evidence_error(said: Result<(), ResultsError>) -> bool {
    matches!(said, Err(ResultsError::Evidence { stage, .. }) if stage == "a")
}

/// T-RP-8-001: the real table has 19 rows.
#[test]
fn design_table_parser_reads_19_rows() -> TestResult {
    let design = read(&Path::new(REPO).join("docs/design/rust_port.ja.md"))?;
    assert_eq!(parse_stage_table(&design)?.len(), 19);
    Ok(())
}

/// T-RP-8-002
#[test]
fn cpu_row_with_faster_cpu_measurement_passes() -> TestResult {
    let results = results_with(("3000", "4000"), ("1000", "2000"))?;
    check_stage_evidence(&[table_row("CPU", "measured")], &results)?;
    Ok(())
}

/// T-RP-8-003
#[test]
fn cpu_row_with_equal_p50_is_rejected() -> TestResult {
    let results = results_with(("1000", "4000"), ("1000", "2000"))?;
    let said = check_stage_evidence(&[table_row("CPU", "measured")], &results);
    assert!(evidence_error(said));
    Ok(())
}

/// T-RP-8-004
#[test]
fn cpu_row_slower_than_gpu_without_url_is_rejected() -> TestResult {
    let results = results_with(("1000", "2000"), ("3000", "4000"))?;
    let said = check_stage_evidence(&[table_row("CPU", "measured")], &results);
    assert!(evidence_error(said));
    Ok(())
}

/// T-RP-8-005
#[test]
fn cpu_row_with_a_primary_source_url_passes() -> TestResult {
    let results = Results::parse("# no timings at all")?;
    let row = table_row("CPU", "no GPU tool: https://example.org/spec");
    check_stage_evidence(&[row], &results)?;
    Ok(())
}

/// T-RP-8-006: the negative control of the acceptance (the evidence column emptied).
#[test]
fn cpu_row_with_empty_evidence_is_rejected() -> TestResult {
    let results = results_with(("3000", "4000"), ("1000", "2000"))?;
    let said = check_stage_evidence(&[table_row("CPU", "")], &results);
    assert!(evidence_error(said));
    Ok(())
}

/// T-RP-8-007
#[test]
fn gpu_row_without_percentiles_is_rejected() -> TestResult {
    let results = results_with(("未測定", "未測定"), ("1000", "2000"))?;
    let said = check_stage_evidence(&[table_row("GPU", "measured")], &results);
    assert!(evidence_error(said));
    Ok(())
}

/// The first stage name is read from the cell rows, so a stage with a different name has no row.
#[test]
fn a_stage_with_another_name_has_no_timings() -> TestResult {
    let results = results_with(("1000", "2000"), ("1000", "2000"))?;
    assert_eq!(STAGE, 0);
    let row = StageRow {
        name: String::from("other"),
        placement: String::from("GPU"),
        evidence: String::from("x"),
    };
    assert!(check_stage_evidence(&[row], &results).is_err());
    Ok(())
}

/// T-RP-8-008 and the acceptance test `rp08_every_cpu_stage_has_its_evidence`:
/// the design table and `docs/results/gpu_stages.md` as they are now.
#[test]
#[ignore = "reads the real design and the real results"]
fn rp08_every_cpu_stage_has_its_evidence() -> TestResult {
    let design = read(&Path::new(REPO).join("docs/design/rust_port.ja.md"))?;
    let results = read(&Path::new(RESULTS).join("gpu_stages.md"))?;
    let table = parse_stage_table(&design)?;
    check_stage_evidence(&table, &Results::parse(&results)?)?;
    Ok(())
}

/// T-RP-8-008 under its case-table name.
#[test]
#[ignore = "reads the real design and the real results"]
fn real_stage_table_has_evidence_for_every_row() -> TestResult {
    rp08_every_cpu_stage_has_its_evidence()
}

//! RP-8: a stage left on the CPU has its evidence in the results, and a stage
//! on the GPU has the same measurements.

mod support;

use std::path::Path;

use hayamimi_eval::gpu_results::{
    L_ESTIMATE, L_ESTIMATE_METRIC, L_ESTIMATE_ROW, Results, ResultsError, check_stage_evidence,
};
use hayamimi_eval::mdtable::render;
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
    check_stage_evidence(&[table_row("CPU", PAIR_FP32)], &results)?;
    Ok(())
}

/// T-RP-8-003
#[test]
fn cpu_row_with_equal_p50_is_rejected() -> TestResult {
    let results = results_with(("1000", "4000"), ("1000", "2000"))?;
    let said = check_stage_evidence(&[table_row("CPU", PAIR_FP32)], &results);
    assert!(evidence_error(said));
    Ok(())
}

/// T-RP-8-004
#[test]
fn cpu_row_slower_than_gpu_without_url_is_rejected() -> TestResult {
    let results = results_with(("1000", "2000"), ("3000", "4000"))?;
    let said = check_stage_evidence(&[table_row("CPU", PAIR_FP32)], &results);
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

const NO_GPU_PATH: &str = "CPU（GPU 経路なし）";
const NO_GPU_EVIDENCE: &str = "根拠 4。文字列の正規表現の処理で、GPU で動く Rust の道具を見つけていない。R2 が 1 行の CPU の p50 を測る";

fn cell_row(stage: &str, provider: &str, variant: &str, p50: &str, p95: &str) -> Row {
    [stage, provider, variant, "無負荷", "1", "115", p50, p95, ""]
        .iter()
        .map(|c| (*c).to_owned())
        .collect()
}

/// Results with a GPU round-trip row of another stage at `floor` µs and, when given, the CPU row of stage `a`.
fn floor_results(floor: &str, cpu: Option<&str>) -> Result<Results, ResultsError> {
    let mut rows = vec![cell_row("floor", "cuda", "transfer-floor", floor, "300")];
    if let Some(p50) = cpu {
        rows.push(cell_row("a", "cpu", "cpu", p50, "300"));
    }
    Results::parse(&stages::cells_section(&rows))
}

/// A cell table and the `L` estimate table with the given p50 (ms).
fn shrink_results(cpu_p50: &str, latency_ms: &str) -> Result<Results, ResultsError> {
    let cells = stages::cells_section(&[cell_row("a", "cpu", "cpu", cpu_p50, "9999")]);
    let estimate = render(
        &["実装", "指標", "標本数", "p50", "p95"],
        &[vec![
            format!("{L_ESTIMATE_ROW} 無負荷"),
            format!("{L_ESTIMATE_METRIC} = 902 ms + 認識 + 訳 T"),
            String::from("285"),
            latency_ms.to_owned(),
            String::from("1573"),
        ]],
    );
    Results::parse(&format!("{cells}\n{L_ESTIMATE}\n\n{estimate}"))
}

/// T-RP-8-009
#[test]
fn no_gpu_path_row_with_owner_slice_passes_before_measurement() -> TestResult {
    let results = results_with(("未測定", "未測定"), ("未測定", "未測定"))?;
    check_stage_evidence(&[table_row(NO_GPU_PATH, NO_GPU_EVIDENCE)], &results)?;
    Ok(())
}

/// T-RP-8-010
#[test]
fn no_gpu_path_row_without_owner_slice_is_rejected() -> TestResult {
    let results = results_with(("未測定", "未測定"), ("未測定", "未測定"))?;
    let without_owner = NO_GPU_EVIDENCE.replace("R2", "");
    let said = check_stage_evidence(&[table_row(NO_GPU_PATH, &without_owner)], &results);
    assert!(evidence_error(said));
    Ok(())
}

/// The reason of evidence 4 may name CUDA instead of the GPU; a reason that names neither is refused.
#[test]
fn no_gpu_path_reason_may_name_cuda_but_must_name_something() -> TestResult {
    let results = results_with(("未測定", "未測定"), ("未測定", "未測定"))?;
    let named = table_row(
        NO_GPU_PATH,
        "根拠 4。ORT も CUDA も使わない C++ である。R4 が測る",
    );
    check_stage_evidence(&[named], &results)?;
    let unnamed = table_row(NO_GPU_PATH, "根拠 4。文字列の規則で、ITN と同じ。R2 が測る");
    assert!(evidence_error(check_stage_evidence(&[unnamed], &results)));
    Ok(())
}

/// T-RP-8-011: 219 is not below 219.
#[test]
fn no_gpu_path_row_at_the_transfer_floor_is_rejected() -> TestResult {
    let results = floor_results("219", Some("219"))?;
    let said = check_stage_evidence(&[table_row(NO_GPU_PATH, NO_GPU_EVIDENCE)], &results);
    assert!(evidence_error(said));
    Ok(())
}

/// T-RP-8-012: 218 is below 219.
#[test]
fn no_gpu_path_row_below_the_transfer_floor_passes() -> TestResult {
    let results = floor_results("219", Some("218"))?;
    check_stage_evidence(&[table_row(NO_GPU_PATH, NO_GPU_EVIDENCE)], &results)?;
    Ok(())
}

/// T-RP-8-013: 13490 µs is exactly 1% of 1349 ms.
#[test]
fn shrink_bound_at_one_percent_of_latency_is_rejected() -> TestResult {
    let results = shrink_results("13490", "1349")?;
    let said = check_stage_evidence(&[table_row("CPU", "根拠 3。上限は L の 1%")], &results);
    assert!(evidence_error(said));
    Ok(())
}

/// T-RP-8-014: 2455 µs is below 13490 µs.
#[test]
fn shrink_bound_below_one_percent_of_latency_passes() -> TestResult {
    let results = shrink_results("2455", "1349")?;
    check_stage_evidence(&[table_row("CPU", "根拠 3。上限は L の 1%")], &results)?;
    Ok(())
}

/// A bound read from the document: a document without the `L` estimate cannot pass evidence 3.
#[test]
fn shrink_bound_without_an_estimate_of_latency_is_rejected() -> TestResult {
    let cells = stages::cells_section(&[cell_row("a", "cpu", "cpu", "1", "2")]);
    let said = check_stage_evidence(
        &[table_row("CPU", "根拠 3。上限は L の 1%")],
        &Results::parse(&cells)?,
    );
    assert!(evidence_error(said));
    Ok(())
}

/// The owner slice must be a word of its own: `R25` and `XR2` do not own a stage.
#[test]
fn an_owner_slice_inside_another_word_does_not_count() -> TestResult {
    let results = results_with(("未測定", "未測定"), ("未測定", "未測定"))?;
    for text in [
        "R25 が測る。GPU の道具が無い",
        "XR2 が測る。GPU の道具が無い",
    ] {
        let said = check_stage_evidence(&[table_row(NO_GPU_PATH, text)], &results);
        assert!(evidence_error(said), "{text}");
    }
    Ok(())
}

/// The evidence of a row decided by the same-precision comparison on `fp32`.
const PAIR_FP32: &str = "根拠 1（variant `fp32`）";
/// Evidence 1 naming two variants.
const PAIR_TWO: &str = "根拠 1（variant `fp32`）と 根拠 1（variant `ort-encoder fp32`）";
/// The placement of the recognition row.
const RECOGNITION: &str = "CPU（int8）";

/// Stage `a` under no load: the given (provider, variant, p50) rows, each with a p95 of 9999.
fn recognition_results(rows: &[(&str, &str, &str)]) -> Result<Results, ResultsError> {
    let cells: Vec<Row> = rows
        .iter()
        .map(|(provider, variant, p50)| cell_row("a", provider, variant, p50, "9999"))
        .collect();
    Results::parse(&stages::cells_section(&cells))
}

/// One timed row: provider, variant and p50.
type Timed = (&'static str, &'static str, &'static str);

/// The fp32 pair of the whole recognition and the pair of the encoder alone, each as (GPU p50, CPU p50).
fn two_pairs(
    fp32: (&'static str, &'static str),
    alone: (&'static str, &'static str),
) -> [Timed; 4] {
    [
        ("cuda", "fp32", fp32.0),
        ("cpu", "fp32", fp32.1),
        ("cuda", "ort-encoder fp32", alone.0),
        ("cpu", "ort-encoder fp32", alone.1),
    ]
}

/// T-RP-8-015: the CPU is faster in the fp32 pair.
#[test]
fn same_variant_pair_with_faster_cpu_passes() -> TestResult {
    let results = recognition_results(&[
        ("cuda", "fp32", "171414"),
        ("cpu", "fp32", "151299"),
        ("cpu", "int8", "128615"),
    ])?;
    check_stage_evidence(&[table_row(RECOGNITION, PAIR_FP32)], &results)?;
    Ok(())
}

/// T-RP-8-016: the CPU int8 is faster than the GPU fp32, but the fp32 pair favours the GPU.
#[test]
fn cpu_int8_faster_but_same_fp32_pair_slower_is_rejected() -> TestResult {
    let results = recognition_results(&[
        ("cuda", "fp32", "151299"),
        ("cpu", "fp32", "171414"),
        ("cpu", "int8", "128615"),
    ])?;
    let said = check_stage_evidence(&[table_row(RECOGNITION, PAIR_FP32)], &results);
    assert!(evidence_error(said));
    Ok(())
}

/// T-RP-8-017: a GPU row of another variant is not set against the CPU fp32 row.
#[test]
fn gpu_row_of_another_variant_is_not_mixed_into_the_pair() -> TestResult {
    let results = recognition_results(&[
        ("cuda", "fp32", "171414"),
        ("cuda", "ort-encoder fp32", "66262"),
        ("cpu", "fp32", "151299"),
    ])?;
    check_stage_evidence(&[table_row(RECOGNITION, PAIR_FP32)], &results)?;
    Ok(())
}

/// T-RP-8-018: the pair of the other variant favours the GPU and does not count.
#[test]
fn named_variant_pair_alone_decides_the_evidence() -> TestResult {
    let results = recognition_results(&two_pairs(("171414", "151299"), ("66262", "121091")))?;
    check_stage_evidence(&[table_row(RECOGNITION, PAIR_FP32)], &results)?;
    Ok(())
}

/// T-RP-8-019: no variant has a row on both sides.
#[test]
fn row_without_any_same_variant_pair_is_rejected() -> TestResult {
    let results = recognition_results(&[("cuda", "fp16", "459578"), ("cpu", "int8", "128615")])?;
    let said = check_stage_evidence(&[table_row(RECOGNITION, PAIR_FP32)], &results);
    assert!(evidence_error(said));
    Ok(())
}

/// T-RP-8-020: the pair of the other variant favours the CPU and does not rescue the row.
#[test]
fn faster_cpu_pair_of_an_unnamed_variant_does_not_rescue_the_row() -> TestResult {
    let results = recognition_results(&two_pairs(("151299", "171414"), ("121091", "66262")))?;
    let said = check_stage_evidence(&[table_row(RECOGNITION, PAIR_FP32)], &results);
    assert!(evidence_error(said));
    Ok(())
}

/// T-RP-8-021: evidence 1 without a named variant.
#[test]
fn evidence_one_without_a_named_variant_is_rejected() -> TestResult {
    let results = recognition_results(&[("cuda", "fp32", "171414"), ("cpu", "fp32", "151299")])?;
    let said = check_stage_evidence(&[table_row(RECOGNITION, "根拠 1")], &results);
    assert!(evidence_error(said));
    Ok(())
}

/// T-RP-8-022: evidence 1 naming two variants, both pairs favouring the CPU.
#[test]
fn evidence_one_with_two_named_variants_is_rejected() -> TestResult {
    let results = recognition_results(&two_pairs(("171414", "151299"), ("121091", "66262")))?;
    let said = check_stage_evidence(&[table_row(RECOGNITION, PAIR_TWO)], &results);
    assert!(evidence_error(said));
    Ok(())
}

/// T-RP-8-023: the named variant has no GPU row; the pair of another variant does not fill it.
#[test]
fn named_variant_pair_missing_one_side_is_rejected() -> TestResult {
    let results = recognition_results(&[
        ("cpu", "fp32", "151299"),
        ("cuda", "ort-encoder fp32", "121091"),
        ("cpu", "ort-encoder fp32", "66262"),
    ])?;
    let said = check_stage_evidence(&[table_row(RECOGNITION, PAIR_FP32)], &results);
    assert!(evidence_error(said));
    Ok(())
}

/// Evidence 2 is the CPU row against the transfer-floor row of the same stage.
#[test]
fn floor_evidence_passes_below_the_floor_and_fails_at_it() -> TestResult {
    let row = table_row("CPU", "根拠 2。CPU の p50 が下限より小さい");
    let below = recognition_results(&[("cuda", "transfer-floor", "250"), ("cpu", "cpu", "249")])?;
    check_stage_evidence(std::slice::from_ref(&row), &below)?;
    let equal = recognition_results(&[("cuda", "transfer-floor", "250"), ("cpu", "cpu", "250")])?;
    assert!(evidence_error(check_stage_evidence(&[row], &equal)));
    Ok(())
}

/// A pair of two other variants is not a transfer floor.
#[test]
fn floor_evidence_needs_the_transfer_floor_row() -> TestResult {
    let results = recognition_results(&[("cuda", "fp32", "250"), ("cpu", "cpu", "1")])?;
    let said = check_stage_evidence(&[table_row("CPU", "根拠 2。下限より小さい")], &results);
    assert!(evidence_error(said));
    Ok(())
}

/// A CPU row that names no evidence and cites no source is refused even when the CPU is faster.
#[test]
fn cpu_row_naming_no_evidence_and_no_source_is_rejected() -> TestResult {
    let results = results_with(("3000", "4000"), ("1000", "2000"))?;
    let said = check_stage_evidence(&[table_row("CPU", "measured")], &results);
    assert!(evidence_error(said));
    Ok(())
}

/// T-RP-8-008 and the acceptance test `rp08_every_cpu_stage_has_its_evidence`:
/// the design table and `docs/results/gpu_stages.md` as they are now.
#[test]
fn rp08_every_cpu_stage_has_its_evidence() -> TestResult {
    let design = read(&Path::new(REPO).join("docs/design/rust_port.ja.md"))?;
    let results = read(&Path::new(RESULTS).join("gpu_stages.md"))?;
    let table = parse_stage_table(&design)?;
    let results = Results::parse(&results)?;
    // Every row is judged on its own, so one failing row does not hide the rows after it.
    let failures: Vec<String> = table
        .iter()
        .filter_map(|row| {
            check_stage_evidence(std::slice::from_ref(row), &results)
                .err()
                .map(|error| format!("{}: {error}", row.name))
        })
        .collect();
    if failures.is_empty() {
        return Ok(());
    }
    Err(Box::from(failures.join("\n")))
}

/// T-RP-8-008 under its case-table name.
#[test]
fn real_stage_table_has_evidence_for_every_row() -> TestResult {
    rp08_every_cpu_stage_has_its_evidence()
}

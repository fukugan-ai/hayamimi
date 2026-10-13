//! RP-7: the copies between host and device are measured per utterance, and
//! their time stays a small part of the recognition time.

mod support;

use std::path::Path;

use hayamimi_eval::gpu_results::{
    Load, Results, ResultsError, TRANSFER_HEADER, TRANSFERS, check_transfer_fields,
};
use hayamimi_eval::mdtable::render;
use hayamimi_eval::nsys::{
    NsysError, check_int8_copies_more, check_transfer_budget, parse_kernel_time_ns,
    parse_memory_stats,
};
use support::stages::{self, Row};
use support::{FIXTURES, RESULTS, TestResult, read};

/// Prefix of the design row that stands for the recognition itself.
const RECOGNITION_ROW: &str = "認識の encoder";

fn fixture(name: &str) -> Result<String, String> {
    read(&Path::new(FIXTURES).join(name))
}

fn transfer_document(rows: &[Row]) -> String {
    stages::transfers_section(rows)
}

/// T-RP-7-001
#[test]
fn transfer_p95_at_twenty_percent_of_decode_passes() -> TestResult {
    // 2000 us of transfer against a 10 ms recognition: exactly 20%.
    check_transfer_budget(2000, 10_000)?;
    Ok(())
}

/// T-RP-7-002
#[test]
fn transfer_p95_just_over_twenty_percent_is_rejected() {
    let said = check_transfer_budget(2001, 10_000);
    assert!(
        matches!(said, Err(NsysError::OverBudget { .. })),
        "{said:?}"
    );
}

/// T-RP-7-003
#[test]
fn transfer_row_with_all_fields_passes() -> TestResult {
    let rows = stages::transfer_rows("fp32", "10", "20000", "6");
    let results = Results::parse(&transfer_document(&rows))?;
    check_transfer_fields(&results)?;
    Ok(())
}

/// T-RP-7-004
#[test]
fn transfer_row_without_bytes_is_rejected() {
    // Drop the two `bytes` columns (4 and 5) of an otherwise complete table.
    let keep = |index: &usize| !(4..=5).contains(index);
    let header: Vec<&str> = TRANSFER_HEADER
        .iter()
        .enumerate()
        .filter(|(i, _)| keep(i))
        .map(|(_, name)| *name)
        .collect();
    let rows: Vec<Row> = stages::transfer_rows("fp32", "10", "20000", "6")
        .into_iter()
        .map(|row| {
            row.into_iter()
                .enumerate()
                .filter(|(i, _)| keep(i))
                .map(|(_, cell)| cell)
                .collect()
        })
        .collect();
    let text = format!("{TRANSFERS}\n\n{}\n", render(&header, &rows));
    let said = Results::parse(&text);
    assert!(
        matches!(said, Err(ResultsError::NoColumn { column, .. }) if column.contains("bytes")),
        "{said:?}"
    );
}

/// T-RP-7-005: the counts, bytes and times of both directions are the hand-added values of the fixture rows.
#[test]
fn nsys_memory_stats_are_parsed_into_both_directions() -> TestResult {
    let stats = parse_memory_stats(
        &fixture("rp07_fp32_mem_time.csv")?,
        &fixture("rp07_fp32_mem_size.csv")?,
    )?;
    assert_eq!(stats.host_to_device.count, 10);
    assert_eq!(stats.host_to_device.total_ns, 6000);
    assert_eq!(stats.host_to_device.total_bytes, 20_000);
    assert_eq!(stats.device_to_host.count, 4);
    assert_eq!(stats.device_to_host.total_ns, 4000);
    assert_eq!(stats.device_to_host.total_bytes, 8000);
    Ok(())
}

/// T-RP-7-006: the same reader gives the kernel time of the same profile.
#[test]
fn encoder_kernel_time_is_read_by_the_same_function() -> TestResult {
    let nanoseconds = parse_kernel_time_ns(&fixture("rp07_kern_sum.csv")?)?;
    assert_eq!(nanoseconds, 10_000);
    Ok(())
}

/// T-RP-7-007: the negative control; an int8 model on CUDA copies more often than fp32.
#[test]
fn int8_run_shows_more_transfers_than_fp32_run() -> TestResult {
    let fp32 = parse_memory_stats(
        &fixture("rp07_fp32_mem_time.csv")?,
        &fixture("rp07_fp32_mem_size.csv")?,
    )?;
    let int8 = parse_memory_stats(
        &fixture("rp07_int8_mem_time.csv")?,
        &fixture("rp07_int8_mem_size.csv")?,
    )?;
    check_int8_copies_more(&int8, &fp32)?;
    assert!(check_int8_copies_more(&fp32, &int8).is_err());
    Ok(())
}

fn real_results() -> Result<Results, Box<dyn std::error::Error>> {
    let text = read(&Path::new(RESULTS).join("gpu_stages.md"))?;
    Ok(Results::parse(&text)?)
}

/// The p95 of the recognition under no load on CUDA, for the variant the transfers were counted with.
fn recognition_p95_us(results: &Results, variant: &str) -> Option<u64> {
    results
        .cells
        .iter()
        .flatten()
        .filter(|c| {
            c.stage.starts_with(RECOGNITION_ROW)
                && c.provider == "cuda"
                && c.load == Load::Idle
                && c.variant == variant
        })
        .filter_map(|c| c.p95_us)
        .max()
}

/// T-RP-7-008 and the acceptance test `rp07_transfers_between_stages_are_measured_and_small`.
#[test]
fn rp07_transfers_between_stages_are_measured_and_small() -> TestResult {
    let results = real_results()?;
    check_transfer_fields(&results)?;
    let rows = results
        .transfers
        .iter()
        .flatten()
        .filter(|r| r.variant == "fp32");
    let transfer_p95_us: u64 = rows.map(|r| r.time_us_p95).sum();
    let Some(decode_p95_us) = recognition_p95_us(&results, "fp32") else {
        return Err(Box::from(
            "the results have no fp32 recognition row on CUDA",
        ));
    };
    check_transfer_budget(transfer_p95_us, decode_p95_us)?;
    Ok(())
}

/// T-RP-7-008 under its case-table name.
#[test]
fn measured_transfers_stay_under_the_budget() -> TestResult {
    rp07_transfers_between_stages_are_measured_and_small()
}

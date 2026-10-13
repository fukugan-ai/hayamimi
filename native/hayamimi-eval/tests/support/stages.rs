//! Builders for the result documents of the stage measurement, used by the
//! RP-7, RP-8, RP-20 and RP-23 tests. A document is made section by section
//! so that a test can break exactly one thing in it.
#![allow(dead_code)]

use hayamimi_eval::gpu_results::{
    CELL_HEADER, ENVIRONMENT, ENVIRONMENT_HEADER, ENVIRONMENT_KEYS, JIT, JIT_HEADER, STAGES,
    TRANSFER_HEADER, TRANSFERS,
};
use hayamimi_eval::mdtable::render;
use hayamimi_eval::stage_table::StageRow;

/// The number of utterances in the recording the fixtures stand for.
pub const UTTERANCES: usize = 23;
/// How many times it is played.
pub const REPEATS: usize = 5;

/// Column positions in a cell row.
pub const STAGE: usize = 0;
pub const PROVIDER: usize = 1;
pub const LOAD: usize = 3;
pub const SAMPLES: usize = 5;
pub const P50: usize = 6;
pub const P95: usize = 7;

pub type Row = Vec<String>;

/// Stage names that stand for the 19 rows of the design table.
pub fn stage_names() -> Vec<String> {
    (1..=19).map(|n| format!("stage {n:02}")).collect()
}

/// Table rows for `names`, all placed on the GPU.
pub fn gpu_rows(names: &[String]) -> Vec<StageRow> {
    names
        .iter()
        .map(|name| StageRow {
            name: name.clone(),
            placement: String::from("GPU"),
            evidence: String::from("measured"),
        })
        .collect()
}

fn row(cells: [&str; 9]) -> Row {
    cells.iter().map(|c| (*c).to_owned()).collect()
}

/// One cell row per stage, provider and load, with plausible numbers.
pub fn cell_rows(names: &[String], samples: usize) -> Vec<Row> {
    let samples = samples.to_string();
    let mut rows = Vec::new();
    for name in names {
        for provider in ["cuda", "cpu"] {
            for load in ["無負荷", "負荷"] {
                rows.push(row([
                    name, provider, "fp32", load, "1", &samples, "1000", "2000", "",
                ]));
            }
        }
    }
    rows
}

pub fn cells_section(rows: &[Row]) -> String {
    format!("{STAGES}\n\n{}\n", render(&CELL_HEADER, rows))
}

pub fn environment_section() -> String {
    let rows: Vec<Row> = ENVIRONMENT_KEYS
        .iter()
        .map(|key| vec![(*key).to_owned(), String::from("x")])
        .collect();
    format!("{ENVIRONMENT}\n\n{}\n", render(&ENVIRONMENT_HEADER, &rows))
}

/// Both directions, with every column.
pub fn transfer_rows(variant: &str, count: &str, bytes: &str, time_us: &str) -> Vec<Row> {
    ["host→device", "device→host"]
        .iter()
        .map(|direction| {
            vec![
                (*direction).to_owned(),
                variant.to_owned(),
                count.to_owned(),
                count.to_owned(),
                bytes.to_owned(),
                bytes.to_owned(),
                time_us.to_owned(),
                time_us.to_owned(),
            ]
        })
        .collect()
}

pub fn transfers_section(rows: &[Row]) -> String {
    format!("{TRANSFERS}\n\n{}\n", render(&TRANSFER_HEADER, rows))
}

/// One row per `(cache state, first decode in ms)`.
pub fn jit_section(states: &[(&str, &str)]) -> String {
    let rows: Vec<Row> = states
        .iter()
        .map(|(cache, ms)| {
            vec![
                (*cache).to_owned(),
                String::from("1"),
                (*ms).to_owned(),
                String::from("700000"),
            ]
        })
        .collect();
    format!("{JIT}\n\n{}\n", render(&JIT_HEADER, &rows))
}

/// A complete document for `names`.
pub fn complete_document(names: &[String]) -> String {
    [
        environment_section(),
        cells_section(&cell_rows(names, UTTERANCES * REPEATS)),
        transfers_section(&transfer_rows("fp32", "40", "1000", "100")),
        jit_section(&[("empty", "900"), ("warm", "700")]),
    ]
    .join("\n")
}

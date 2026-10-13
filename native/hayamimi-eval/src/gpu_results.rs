//! The result document `docs/results/gpu_stages.md`: what a stage
//! measurement writes and what RP-7, RP-8, RP-20 and RP-23 read back.
//!
//! The document is Markdown tables under fixed headings. Reading is strict: a
//! provider that is missing from a row, a number that is not a number and a
//! column that is not there are errors that name what is wrong, so a document
//! that was only half written cannot pass for a finished one.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::mdtable::{MdTable, first_table, section};
use crate::stage_runs::{FLOOR, HOST};
use crate::stage_table::{StageRow, first_measure_row};

pub const ENVIRONMENT: &str = "## 測った機械と build";
pub const STAGES: &str = "## 段ごとの時間";
pub const TRANSFERS: &str = "## 転送";
pub const JIT: &str = "## JIT の cache と decode_ms";
pub const PROPOSALS: &str = "## 置き場の提案";
pub const NODES: &str = "## node の置き場（ort）";

/// What the environment section must state.
pub const ENVIRONMENT_KEYS: [&str; 9] = [
    "os",
    "cpu",
    "gpu",
    "driver",
    "sherpa-onnx",
    "onnxruntime",
    "cuda",
    "cudnn",
    "build.json sha256",
];

pub const ENVIRONMENT_HEADER: [&str; 2] = ["項目", "値"];
pub const CELL_HEADER: [&str; 9] = [
    "段",
    "provider",
    "variant",
    "負荷",
    "run",
    "標本数",
    "p50_us",
    "p95_us",
    "備考",
];
pub const TRANSFER_HEADER: [&str; 8] = [
    "向き",
    "variant",
    "回数 p50",
    "回数 p95",
    "bytes p50",
    "bytes p95",
    "時間_us p50",
    "時間_us p95",
];
pub const JIT_HEADER: [&str; 4] = ["cache", "run", "decode_ms 最初の1回", "decode_us p50"];
pub const PROPOSAL_HEADER: [&str; 3] = ["段", "提案", "根拠"];
pub const NODE_HEADER: [&str; 4] = ["model", "device", "CUDA の node", "CPU の node"];

/// The two directions a transfer row may name.
pub const DIRECTIONS: [&str; 2] = ["host→device", "device→host"];
/// The two providers a time row may name.
pub const PROVIDERS: [&str; 2] = ["cuda", "cpu"];
/// The cell text of a time that was not measured.
pub const UNMEASURED: &str = "未測定";

/// Whether a second process was hammering the GPU during the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Load {
    Idle,
    Loaded,
}

impl Load {
    pub const ALL: [Load; 2] = [Load::Idle, Load::Loaded];

    pub fn label(self) -> &'static str {
        match self {
            Load::Idle => "無負荷",
            Load::Loaded => "負荷",
        }
    }

    fn from_label(text: &str) -> Option<Load> {
        Load::ALL.into_iter().find(|load| load.label() == text)
    }
}

/// One timed combination of stage, provider, model variant, load and run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    pub stage: String,
    pub provider: String,
    pub variant: String,
    pub load: Load,
    pub run: u32,
    pub samples: usize,
    pub p50_us: Option<u64>,
    pub p95_us: Option<u64>,
    pub note: String,
}

/// The copies between host and device for one utterance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferRow {
    pub direction: String,
    pub variant: String,
    pub count_p50: u64,
    pub count_p95: u64,
    pub bytes_p50: u64,
    pub bytes_p95: u64,
    pub time_us_p50: u64,
    pub time_us_p95: u64,
}

/// The first recognition of a process, by the state of the JIT cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JitRow {
    pub cache: String,
    pub run: u32,
    pub first_decode_ms: u64,
    pub decode_us_p50: u64,
}

/// A placement proposed for a stage, with its evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    pub stage: String,
    pub placement: String,
    pub evidence: String,
}

/// The nodes of one model that ONNX Runtime put on the CUDA provider and on the CPU.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeRow {
    pub model: String,
    pub device: String,
    pub cuda_nodes: u64,
    pub cpu_nodes: u64,
}

/// A parsed result document. A section the document does not have is `None`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Results {
    pub environment: Option<Vec<(String, String)>>,
    pub cells: Option<Vec<Cell>>,
    pub transfers: Option<Vec<TransferRow>>,
    pub jit: Option<Vec<JitRow>>,
    pub proposals: Option<Vec<Proposal>>,
    pub nodes: Option<Vec<NodeRow>>,
    /// The p50 (ms) of the estimate of `L` for recognition and translation on the GPU, no load.
    pub l_estimate_p50_ms: Option<u64>,
}

/// The heading of the section that estimates `L`.
pub const L_ESTIMATE: &str = "## 遅れ L の見積り";
/// The row of that table the shrink bound of evidence 3 is taken from.
pub const L_ESTIMATE_ROW: &str = "Rust 認識 cuda / 訳 cuda 無負荷";
/// The start of the metric of an estimate row.
pub const L_ESTIMATE_METRIC: &str = "L の見積り";
/// Evidence 3 holds when the CPU time is below this share (percent) of the estimate of `L`. A provisional value.
pub const SHRINK_PERCENT: u64 = 1;
/// The start of the evidence column of a row justified by evidence 3.
const SHRINK_EVIDENCE: &str = "根拠 3";
/// The start of the evidence column of a row decided by the same-precision comparison (evidence 1).
const PAIR_EVIDENCE: &str = "根拠 1";
/// What evidence 1 writes before the one variant it names; the variant follows, between backticks.
const NAMING_OPEN: &str = "根拠 1（variant `";
/// The start of the evidence column of a row decided by the transfer floor (evidence 2).
const FLOOR_EVIDENCE: &str = "根拠 2";
/// The slices that own the measurement of a stage with no GPU path.
const OWNER_SLICES: [&str; 3] = ["R2", "R4", "R5"];

#[derive(Debug, PartialEq, Eq)]
pub enum ResultsError {
    NoSection(&'static str),
    NoColumn {
        section: &'static str,
        column: &'static str,
    },
    BadCell {
        section: &'static str,
        column: &'static str,
        value: String,
    },
    NoProvider {
        stage: String,
    },
    MissingEnvironment(&'static str),
    MissingCell {
        stage: String,
        provider: &'static str,
        load: Load,
    },
    NoPercentile {
        stage: String,
        provider: &'static str,
        load: Load,
        which: &'static str,
    },
    SampleCount {
        stage: String,
        expected: usize,
        found: usize,
    },
    NoTransferDirection(&'static str),
    NoJitRow(&'static str),
    MergedJitRows(String),
    RepeatDifference {
        first: u64,
        second: u64,
    },
    MeasureRowRemains(String),
    NoResultRow(String),
    Evidence {
        stage: String,
        why: &'static str,
    },
    /// A model the stage table places on the GPU has no row in the node table.
    NoNodeRow(String),
    /// A model the stage table places on the GPU has no `gpu.session.placed` record.
    NoPlacedEvent(String),
    /// A GPU model has more CPU nodes than the node table records.
    CpuNodesOverRecord {
        model: String,
        found: u64,
        recorded: u64,
    },
}

impl fmt::Display for ResultsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResultsError::NoSection(name) => write!(f, "the document has no section `{name}`"),
            ResultsError::NoColumn { section, column } => {
                write!(f, "section `{section}` has no column `{column}`")
            }
            ResultsError::BadCell {
                section,
                column,
                value,
            } => write!(f, "section `{section}`, column `{column}`: `{value}`"),
            ResultsError::NoProvider { stage } => {
                write!(f, "stage `{stage}`: a row names no provider")
            }
            ResultsError::MissingEnvironment(key) => {
                write!(f, "the environment section does not state `{key}`")
            }
            ResultsError::MissingCell {
                stage,
                provider,
                load,
            } => write!(
                f,
                "stage `{stage}` has no {provider} time under {}",
                load.label()
            ),
            ResultsError::NoPercentile {
                stage,
                provider,
                load,
                which,
            } => write!(
                f,
                "stage `{stage}`: the {provider} row under {} has no {which}",
                load.label()
            ),
            ResultsError::SampleCount {
                stage,
                expected,
                found,
            } => write!(
                f,
                "stage `{stage}`: {found} samples where {expected} are expected"
            ),
            ResultsError::NoTransferDirection(direction) => {
                write!(f, "no transfer row for {direction}")
            }
            ResultsError::NoJitRow(state) => {
                write!(f, "no decode_ms row for the {state} JIT cache")
            }
            ResultsError::MergedJitRows(value) => write!(
                f,
                "the JIT cache state `{value}` is not `empty` or `warm`; the two runs must be separate rows"
            ),
            ResultsError::RepeatDifference { first, second } => {
                write!(
                    f,
                    "two runs gave p50 {first} and {second}, more than 10% apart"
                )
            }
            ResultsError::MeasureRowRemains(stage) => {
                write!(f, "stage `{stage}` still says 測る")
            }
            ResultsError::NoResultRow(stage) => {
                write!(f, "stage `{stage}` has no row in the results")
            }
            ResultsError::Evidence { stage, why } => write!(f, "stage `{stage}`: {why}"),
            ResultsError::NoNodeRow(model) => {
                write!(
                    f,
                    "model `{model}` is on the GPU but has no row in the node table"
                )
            }
            ResultsError::NoPlacedEvent(model) => {
                write!(
                    f,
                    "model `{model}` is on the GPU but no gpu.session.placed record names it"
                )
            }
            ResultsError::CpuNodesOverRecord {
                model,
                found,
                recorded,
            } => write!(
                f,
                "model `{model}`: {found} CPU nodes, more than the {recorded} the node table records"
            ),
        }
    }
}

impl std::error::Error for ResultsError {}

fn table_of(text: &str, name: &'static str) -> Result<Option<MdTable>, ResultsError> {
    let Ok(body) = section(text, name) else {
        return Ok(None);
    };
    first_table(body)
        .map(Some)
        .map_err(|_| ResultsError::NoSection(name))
}

fn cell<'a>(
    table: &MdTable,
    row: &'a [String],
    name: &'static str,
    section: &'static str,
) -> Result<&'a str, ResultsError> {
    table.get(row, name).map_err(|_| ResultsError::NoColumn {
        section,
        column: name,
    })
}

fn number(text: &str, section: &'static str, column: &'static str) -> Result<u64, ResultsError> {
    text.parse::<u64>().map_err(|_| ResultsError::BadCell {
        section,
        column,
        value: text.to_owned(),
    })
}

fn optional_number(
    text: &str,
    section: &'static str,
    column: &'static str,
) -> Result<Option<u64>, ResultsError> {
    if text == UNMEASURED {
        return Ok(None);
    }
    number(text, section, column).map(Some)
}

fn parse_environment(text: &str) -> Result<Option<Vec<(String, String)>>, ResultsError> {
    let Some(table) = table_of(text, ENVIRONMENT)? else {
        return Ok(None);
    };
    table
        .rows
        .iter()
        .map(|row| {
            let key = cell(&table, row, "項目", ENVIRONMENT)?;
            let value = cell(&table, row, "値", ENVIRONMENT)?;
            Ok((key.to_owned(), value.to_owned()))
        })
        .collect::<Result<Vec<_>, ResultsError>>()
        .map(Some)
}

fn parse_cell(table: &MdTable, row: &[String]) -> Result<Cell, ResultsError> {
    let stage = cell(table, row, "段", STAGES)?.to_owned();
    let provider = cell(table, row, "provider", STAGES)?;
    if provider.is_empty() {
        return Err(ResultsError::NoProvider { stage });
    }
    if !PROVIDERS.contains(&provider) {
        return Err(ResultsError::BadCell {
            section: STAGES,
            column: "provider",
            value: provider.to_owned(),
        });
    }
    let load_text = cell(table, row, "負荷", STAGES)?;
    let Some(load) = Load::from_label(load_text) else {
        return Err(ResultsError::BadCell {
            section: STAGES,
            column: "負荷",
            value: load_text.to_owned(),
        });
    };
    let run = number(cell(table, row, "run", STAGES)?, STAGES, "run")?;
    let samples = number(cell(table, row, "標本数", STAGES)?, STAGES, "標本数")?;
    Ok(Cell {
        stage,
        provider: provider.to_owned(),
        variant: cell(table, row, "variant", STAGES)?.to_owned(),
        load,
        run: u32::try_from(run).map_err(|_| ResultsError::BadCell {
            section: STAGES,
            column: "run",
            value: run.to_string(),
        })?,
        samples: usize::try_from(samples).map_err(|_| ResultsError::BadCell {
            section: STAGES,
            column: "標本数",
            value: samples.to_string(),
        })?,
        p50_us: optional_number(cell(table, row, "p50_us", STAGES)?, STAGES, "p50_us")?,
        p95_us: optional_number(cell(table, row, "p95_us", STAGES)?, STAGES, "p95_us")?,
        note: cell(table, row, "備考", STAGES)?.to_owned(),
    })
}

fn parse_cells(text: &str) -> Result<Option<Vec<Cell>>, ResultsError> {
    let Some(table) = table_of(text, STAGES)? else {
        return Ok(None);
    };
    table
        .rows
        .iter()
        .map(|row| parse_cell(&table, row))
        .collect::<Result<Vec<_>, ResultsError>>()
        .map(Some)
}

fn parse_transfer(table: &MdTable, row: &[String]) -> Result<TransferRow, ResultsError> {
    let read = |name: &'static str| -> Result<u64, ResultsError> {
        number(cell(table, row, name, TRANSFERS)?, TRANSFERS, name)
    };
    Ok(TransferRow {
        direction: cell(table, row, "向き", TRANSFERS)?.to_owned(),
        variant: cell(table, row, "variant", TRANSFERS)?.to_owned(),
        count_p50: read("回数 p50")?,
        count_p95: read("回数 p95")?,
        bytes_p50: read("bytes p50")?,
        bytes_p95: read("bytes p95")?,
        time_us_p50: read("時間_us p50")?,
        time_us_p95: read("時間_us p95")?,
    })
}

fn parse_transfers(text: &str) -> Result<Option<Vec<TransferRow>>, ResultsError> {
    let Some(table) = table_of(text, TRANSFERS)? else {
        return Ok(None);
    };
    table
        .rows
        .iter()
        .map(|row| parse_transfer(&table, row))
        .collect::<Result<Vec<_>, ResultsError>>()
        .map(Some)
}

fn parse_jit(text: &str) -> Result<Option<Vec<JitRow>>, ResultsError> {
    let Some(table) = table_of(text, JIT)? else {
        return Ok(None);
    };
    table
        .rows
        .iter()
        .map(|row| {
            let run = number(cell(&table, row, "run", JIT)?, JIT, "run")?;
            Ok(JitRow {
                cache: cell(&table, row, "cache", JIT)?.to_owned(),
                run: u32::try_from(run).map_err(|_| ResultsError::BadCell {
                    section: JIT,
                    column: "run",
                    value: run.to_string(),
                })?,
                first_decode_ms: number(
                    cell(&table, row, "decode_ms 最初の1回", JIT)?,
                    JIT,
                    "decode_ms 最初の1回",
                )?,
                decode_us_p50: number(
                    cell(&table, row, "decode_us p50", JIT)?,
                    JIT,
                    "decode_us p50",
                )?,
            })
        })
        .collect::<Result<Vec<_>, ResultsError>>()
        .map(Some)
}

fn parse_proposals(text: &str) -> Result<Option<Vec<Proposal>>, ResultsError> {
    let Some(table) = table_of(text, PROPOSALS)? else {
        return Ok(None);
    };
    table
        .rows
        .iter()
        .map(|row| {
            Ok(Proposal {
                stage: cell(&table, row, "段", PROPOSALS)?.to_owned(),
                placement: cell(&table, row, "提案", PROPOSALS)?.to_owned(),
                evidence: cell(&table, row, "根拠", PROPOSALS)?.to_owned(),
            })
        })
        .collect::<Result<Vec<_>, ResultsError>>()
        .map(Some)
}

fn parse_nodes(text: &str) -> Result<Option<Vec<NodeRow>>, ResultsError> {
    let Some(table) = table_of(text, NODES)? else {
        return Ok(None);
    };
    table
        .rows
        .iter()
        .map(|row| {
            let count =
                |column: &'static str| number(cell(&table, row, column, NODES)?, NODES, column);
            Ok(NodeRow {
                model: cell(&table, row, "model", NODES)?.to_owned(),
                device: cell(&table, row, "device", NODES)?.to_owned(),
                cuda_nodes: count("CUDA の node")?,
                cpu_nodes: count("CPU の node")?,
            })
        })
        .collect::<Result<Vec<_>, ResultsError>>()
        .map(Some)
}

fn parse_l_estimate(text: &str) -> Result<Option<u64>, ResultsError> {
    let Some(table) = table_of(text, L_ESTIMATE)? else {
        return Ok(None);
    };
    for row in &table.rows {
        let who = cell(&table, row, "実装", L_ESTIMATE)?;
        let what = cell(&table, row, "指標", L_ESTIMATE)?;
        if who.starts_with(L_ESTIMATE_ROW) && what.starts_with(L_ESTIMATE_METRIC) {
            return number(cell(&table, row, "p50", L_ESTIMATE)?, L_ESTIMATE, "p50").map(Some);
        }
    }
    Ok(None)
}

impl Results {
    /// Reads every section the document has.
    pub fn parse(text: &str) -> Result<Results, ResultsError> {
        Ok(Results {
            environment: parse_environment(text)?,
            cells: parse_cells(text)?,
            transfers: parse_transfers(text)?,
            jit: parse_jit(text)?,
            proposals: parse_proposals(text)?,
            nodes: parse_nodes(text)?,
            l_estimate_p50_ms: parse_l_estimate(text)?,
        })
    }

    /// The smallest p50 (µs) of a GPU round trip alone: the lower bound of any GPU placement.
    pub fn transfer_floor_us(&self) -> Option<u64> {
        self.cells
            .iter()
            .flatten()
            .filter(|c| c.provider == "cuda" && c.load == Load::Idle && c.variant == FLOOR)
            .filter_map(|c| c.p50_us)
            .min()
    }

    fn cells_of(&self, stage: &str, provider: &str, load: Load) -> Vec<&Cell> {
        self.cells
            .iter()
            .flatten()
            .filter(|c| c.stage == stage && c.provider == provider && c.load == load)
            .collect()
    }

    /// The fastest p50 and its p95 among the idle rows of one stage and provider that have both.
    /// With `variant` set only the rows of that variant count, so two precisions are never mixed.
    pub fn best_idle(
        &self,
        stage: &str,
        provider: &str,
        variant: Option<&str>,
    ) -> Option<(u64, u64)> {
        self.cells_of(stage, provider, Load::Idle)
            .into_iter()
            .filter(|c| variant.is_none_or(|v| c.variant == v))
            .filter_map(|c| Some((c.p50_us?, c.p95_us?)))
            .min()
    }

    fn proposal_evidence(&self, stage: &str) -> Option<&str> {
        self.proposals
            .iter()
            .flatten()
            .find(|p| p.stage == stage)
            .map(|p| p.evidence.as_str())
    }
}

/// RP-23: the document names the machine and the build.
pub fn check_environment(results: &Results) -> Result<(), ResultsError> {
    let Some(rows) = &results.environment else {
        return Err(ResultsError::NoSection(ENVIRONMENT));
    };
    for key in ENVIRONMENT_KEYS {
        let stated = rows
            .iter()
            .any(|(name, value)| name == key && !value.is_empty());
        if !stated {
            return Err(ResultsError::MissingEnvironment(key));
        }
    }
    Ok(())
}

/// RP-23 and RP-7: both directions of transfer are stated, with count, bytes and time.
pub fn check_transfer_fields(results: &Results) -> Result<(), ResultsError> {
    let Some(rows) = &results.transfers else {
        return Err(ResultsError::NoSection(TRANSFERS));
    };
    for direction in DIRECTIONS {
        if !rows.iter().any(|row| row.direction == direction) {
            return Err(ResultsError::NoTransferDirection(direction));
        }
    }
    Ok(())
}

/// RP-23 (H27): the first recognition of an empty JIT cache and of a warm one are separate rows.
pub fn check_jit_rows(results: &Results) -> Result<(), ResultsError> {
    let Some(rows) = &results.jit else {
        return Err(ResultsError::NoSection(JIT));
    };
    if let Some(row) = rows
        .iter()
        .find(|row| row.cache != "empty" && row.cache != "warm")
    {
        return Err(ResultsError::MergedJitRows(row.cache.clone()));
    }
    for state in ["empty", "warm"] {
        if !rows.iter().any(|row| row.cache == state) {
            return Err(ResultsError::NoJitRow(state));
        }
    }
    Ok(())
}

/// RP-23: a row holds `utterances * repeats` samples.
pub fn check_sample_count(
    stage: &str,
    found: usize,
    utterances: usize,
    repeats: usize,
) -> Result<(), ResultsError> {
    let expected = utterances * repeats;
    if found == expected {
        return Ok(());
    }
    Err(ResultsError::SampleCount {
        stage: stage.to_owned(),
        expected,
        found,
    })
}

/// RP-23: two runs of the same build on the same input differ by at most 10% of the first.
pub fn check_repeat_p50(first_us: u64, second_us: u64) -> Result<(), ResultsError> {
    if first_us.abs_diff(second_us) * 10 <= first_us {
        return Ok(());
    }
    Err(ResultsError::RepeatDifference {
        first: first_us,
        second: second_us,
    })
}

fn check_combination(
    results: &Results,
    stage: &str,
    provider: &'static str,
    load: Load,
    expected_samples: usize,
) -> Result<(), ResultsError> {
    let found = results.cells_of(stage, provider, load);
    if found.is_empty() {
        return Err(ResultsError::MissingCell {
            stage: stage.to_owned(),
            provider,
            load,
        });
    }
    for row in &found {
        let which = match (row.p50_us, row.p95_us) {
            (Some(_), Some(_)) => None,
            (None, _) => Some("p50"),
            (Some(_), None) => Some("p95"),
        };
        if let Some(which) = which {
            return Err(ResultsError::NoPercentile {
                stage: stage.to_owned(),
                provider,
                load,
                which,
            });
        }
        check_sample_count(stage, row.samples, expected_samples, 1)?;
    }
    Ok(())
}

/// RP-23: every stage of the table has a p50 and a p95 for each provider under each load,
/// the document names the machine, the transfers and the two JIT cache states.
/// `expected_samples` is the number of utterances times the number of repeats.
pub fn check_stage_results(
    table: &[StageRow],
    results: &Results,
    expected_samples: usize,
) -> Result<(), ResultsError> {
    check_environment(results)?;
    check_transfer_fields(results)?;
    check_jit_rows(results)?;
    if results.cells.is_none() {
        return Err(ResultsError::NoSection(STAGES));
    }
    for row in table.iter().filter(|row| row.has_gpu_path()) {
        for provider in PROVIDERS {
            for load in Load::ALL {
                check_combination(results, &row.name, provider, load, expected_samples)?;
            }
        }
    }
    Ok(())
}

/// RP-8: a stage left on the CPU has its evidence; a stage on the GPU has both timings.
pub fn check_stage_evidence(table: &[StageRow], results: &Results) -> Result<(), ResultsError> {
    for row in table {
        if !row.has_gpu_path() {
            check_no_gpu_path(row, results)?;
        } else if !row.placement.starts_with("CPU") {
            check_both_timed(row, results)?;
        } else if row.evidence.starts_with(SHRINK_EVIDENCE) {
            check_shrink_bound(row, results)?;
        } else if row.evidence.starts_with(PAIR_EVIDENCE) {
            check_same_variant_pair(row, results)?;
        } else if row.evidence.starts_with(FLOOR_EVIDENCE) {
            check_floor_pair(row, results)?;
        } else {
            check_primary_source(row, results)?;
        }
    }
    Ok(())
}

fn evidence_error(row: &StageRow, why: &'static str) -> ResultsError {
    ResultsError::Evidence {
        stage: row.name.clone(),
        why,
    }
}

/// Whether `text` names one of the slices that own an unmeasured stage, as a word of its own.
fn has_owner_slice(text: &str) -> bool {
    OWNER_SLICES.iter().any(|slice| {
        text.match_indices(slice).any(|(at, _)| {
            let before = text[..at].chars().next_back();
            let after = text[at + slice.len()..].chars().next();
            !before.is_some_and(|c| c.is_ascii_alphanumeric())
                && !after.is_some_and(|c| c.is_ascii_digit())
        })
    })
}

/// Evidence 4: a stage with no GPU path is timed on the CPU below the GPU round trip, or,
/// while its CPU row is unmeasured, names the slice that will time it and why no GPU path exists.
fn check_no_gpu_path(row: &StageRow, results: &Results) -> Result<(), ResultsError> {
    if let Some((cpu_p50, _)) = results.best_idle(&row.name, "cpu", None) {
        let floor = results.transfer_floor_us().ok_or_else(|| {
            evidence_error(
                row,
                "no row of the results gives the GPU round trip to compare with",
            )
        })?;
        if cpu_p50 < floor {
            return Ok(());
        }
        return Err(evidence_error(
            row,
            "the CPU p50 is not below the GPU round-trip floor",
        ));
    }
    if !has_owner_slice(&row.evidence) {
        return Err(evidence_error(
            row,
            "an unmeasured stage with no GPU path names no owner slice (R2, R4 or R5)",
        ));
    }
    // The reason names the GPU or CUDA (what is missing) or cites a source (an `http` URL).
    if !["GPU", "CUDA", "http"]
        .iter()
        .any(|word| row.evidence.contains(word))
    {
        return Err(evidence_error(
            row,
            "an unmeasured stage with no GPU path states no reason why it has none",
        ));
    }
    Ok(())
}

/// Evidence 3: the most a GPU could save, the CPU time, is below `SHRINK_PERCENT` of the estimate of `L`.
fn check_shrink_bound(row: &StageRow, results: &Results) -> Result<(), ResultsError> {
    let (cpu_p50, _) = results
        .best_idle(&row.name, "cpu", None)
        .ok_or_else(|| evidence_error(row, "evidence 3 needs a measured CPU p50"))?;
    let latency_ms = results.l_estimate_p50_ms.ok_or_else(|| {
        evidence_error(row, "the results have no estimate of L to take a share of")
    })?;
    // cpu_us < latency_ms * 1000 * percent / 100, kept in integers.
    if u128::from(cpu_p50) * 100 < u128::from(latency_ms) * 1000 * u128::from(SHRINK_PERCENT) {
        return Ok(());
    }
    Err(evidence_error(
        row,
        "the CPU p50 is not below the share of the estimate of L",
    ))
}

/// A stage not placed on the CPU has a p50 and a p95 for both providers.
fn check_both_timed(row: &StageRow, results: &Results) -> Result<(), ResultsError> {
    let timed = |provider: &str| results.best_idle(&row.name, provider, None).is_some();
    if timed("cuda") && timed("cpu") {
        return Ok(());
    }
    Err(evidence_error(
        row,
        "a stage not on the CPU needs a p50 and a p95 for both providers",
    ))
}

/// The variants a row of evidence 1 names, each written as ``根拠 1（variant `<name>`）``.
fn named_variants(evidence: &str) -> Vec<&str> {
    evidence
        .split(NAMING_OPEN)
        .skip(1)
        .filter_map(|after| after.split_once('`').map(|(name, _)| name))
        .collect()
}

/// Evidence 1: the CPU p50 is below the GPU p50 of the one variant the evidence names.
/// The pairs of the other variants of the stage take no part, whichever side they favour.
fn check_same_variant_pair(row: &StageRow, results: &Results) -> Result<(), ResultsError> {
    let names = named_variants(&row.evidence);
    let [variant] = names.as_slice() else {
        return Err(evidence_error(
            row,
            "evidence 1 must name exactly one variant",
        ));
    };
    let (Some(gpu), Some(cpu)) = (
        results.best_idle(&row.name, "cuda", Some(*variant)),
        results.best_idle(&row.name, "cpu", Some(*variant)),
    ) else {
        return Err(evidence_error(
            row,
            "the named variant lacks its GPU row or its CPU row",
        ));
    };
    if cpu.0 < gpu.0 {
        return Ok(());
    }
    Err(evidence_error(
        row,
        "the CPU p50 of the named variant is not below its GPU p50",
    ))
}

/// Evidence 2: the CPU p50 is below the GPU round trip that carries no kernel.
fn check_floor_pair(row: &StageRow, results: &Results) -> Result<(), ResultsError> {
    let (Some(gpu), Some(cpu)) = (
        results.best_idle(&row.name, "cuda", Some(FLOOR)),
        results.best_idle(&row.name, "cpu", Some(HOST)),
    ) else {
        return Err(evidence_error(
            row,
            "evidence 2 needs a transfer-floor row on the GPU and a CPU row",
        ));
    };
    if cpu.0 < gpu.0 {
        return Ok(());
    }
    Err(evidence_error(
        row,
        "the CPU p50 is not below the GPU transfer floor of the stage",
    ))
}

/// A CPU row that names none of evidence 1 to 3 cites a primary source, in its own cell or in the proposal.
fn check_primary_source(row: &StageRow, results: &Results) -> Result<(), ResultsError> {
    if row.evidence.trim().is_empty() {
        return Err(evidence_error(
            row,
            "the evidence column of a CPU row is empty",
        ));
    }
    let cites = row.evidence.contains("http")
        || results
            .proposal_evidence(&row.name)
            .is_some_and(|text| text.contains("http"));
    if cites {
        return Ok(());
    }
    Err(evidence_error(
        row,
        "a CPU row names no evidence 1, 2 or 3 and cites no primary source URL",
    ))
}

/// RP-20: no row says 測る any more, and every row has a row in the results.
pub fn check_placements_measured(
    table: &[StageRow],
    results: &Results,
) -> Result<(), ResultsError> {
    if let Some(row) = first_measure_row(table) {
        return Err(ResultsError::MeasureRowRemains(row.name.clone()));
    }
    for row in table {
        let has_row = results.cells.iter().flatten().any(|c| c.stage == row.name);
        if !has_row {
            return Err(ResultsError::NoResultRow(row.name.clone()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mdtable::render;

    fn stage(name: &str, placement: &str, evidence: &str) -> StageRow {
        StageRow {
            name: name.to_owned(),
            placement: placement.to_owned(),
            evidence: evidence.to_owned(),
        }
    }

    fn cells_text(rows: &[[&str; 9]]) -> String {
        let rows: Vec<Vec<String>> = rows
            .iter()
            .map(|r| r.iter().map(|c| (*c).to_owned()).collect())
            .collect();
        format!("{STAGES}\n\n{}", render(&CELL_HEADER, &rows))
    }

    #[test]
    fn a_row_without_a_provider_is_not_a_gpu_row() {
        let text = cells_text(&[["s", "", "fp32", "無負荷", "1", "115", "10", "20", ""]]);
        assert_eq!(
            Results::parse(&text),
            Err(ResultsError::NoProvider {
                stage: String::from("s")
            })
        );
    }

    #[test]
    fn an_unknown_provider_is_a_bad_cell() {
        let text = cells_text(&[["s", "tpu", "fp32", "無負荷", "1", "115", "10", "20", ""]]);
        assert!(matches!(
            Results::parse(&text),
            Err(ResultsError::BadCell {
                column: "provider",
                ..
            })
        ));
    }

    #[test]
    fn an_unmeasured_time_reads_as_none() -> Result<(), ResultsError> {
        let text = cells_text(&[["s", "cpu", "-", "無負荷", "1", "0", "未測定", "未測定", "x"]]);
        let results = Results::parse(&text)?;
        let cells = results.cells.ok_or(ResultsError::NoSection(STAGES))?;
        assert_eq!(cells[0].p50_us, None);
        Ok(())
    }

    #[test]
    fn a_time_that_is_not_a_number_is_a_bad_cell() {
        let text = cells_text(&[["s", "cpu", "-", "無負荷", "1", "115", "abc", "2", ""]]);
        assert!(matches!(
            Results::parse(&text),
            Err(ResultsError::BadCell {
                column: "p50_us",
                ..
            })
        ));
    }

    #[test]
    fn the_sample_count_must_equal_utterances_times_repeats() {
        assert_eq!(check_sample_count("s", 115, 23, 5), Ok(()));
        assert_eq!(
            check_sample_count("s", 114, 23, 5),
            Err(ResultsError::SampleCount {
                stage: String::from("s"),
                expected: 115,
                found: 114
            })
        );
    }

    #[test]
    fn a_ten_percent_difference_passes_and_a_little_more_fails() {
        assert_eq!(check_repeat_p50(1000, 1100), Ok(()));
        assert_eq!(check_repeat_p50(1000, 900), Ok(()));
        assert!(check_repeat_p50(1000, 1101).is_err());
        assert!(check_repeat_p50(0, 1).is_err());
        assert_eq!(check_repeat_p50(0, 0), Ok(()));
    }

    #[test]
    fn a_missing_section_is_none_not_an_empty_list() -> Result<(), ResultsError> {
        let results = Results::parse("# nothing")?;
        assert_eq!(results, Results::default());
        assert_eq!(
            check_environment(&results),
            Err(ResultsError::NoSection(ENVIRONMENT))
        );
        Ok(())
    }

    #[test]
    fn a_stage_row_on_the_cpu_needs_a_url_or_a_faster_cpu() -> Result<(), ResultsError> {
        let table = [stage("a", "CPU", "no tool")];
        let slow = cells_text(&[
            ["a", "cuda", "fp32", "無負荷", "1", "5", "10", "20", ""],
            ["a", "cpu", "fp32", "無負荷", "1", "5", "30", "40", ""],
        ]);
        let results = Results::parse(&slow)?;
        assert!(check_stage_evidence(&table, &results).is_err());
        Ok(())
    }

    #[test]
    fn a_stage_row_on_the_cpu_with_a_faster_cpu_passes() -> Result<(), ResultsError> {
        let table = [stage("a", "CPU", "根拠 1（variant `fp32`）")];
        let text = cells_text(&[
            ["a", "cuda", "fp32", "無負荷", "1", "5", "30", "40", ""],
            ["a", "cpu", "fp32", "無負荷", "1", "5", "10", "40", ""],
        ]);
        check_stage_evidence(&table, &Results::parse(&text)?)
    }

    #[test]
    fn equal_p50_does_not_justify_the_cpu() -> Result<(), ResultsError> {
        let table = [stage("a", "CPU", "根拠 1（variant `fp32`）")];
        let text = cells_text(&[
            ["a", "cuda", "fp32", "無負荷", "1", "5", "10", "40", ""],
            ["a", "cpu", "fp32", "無負荷", "1", "5", "10", "40", ""],
        ]);
        let said = check_stage_evidence(&table, &Results::parse(&text)?);
        assert!(
            matches!(said, Err(ResultsError::Evidence { .. })),
            "{said:?}"
        );
        Ok(())
    }

    #[test]
    fn an_empty_evidence_cell_on_a_cpu_row_fails_even_with_numbers() -> Result<(), ResultsError> {
        let table = [stage("a", "CPU", "")];
        let text = cells_text(&[
            ["a", "cuda", "fp32", "無負荷", "1", "5", "30", "40", ""],
            ["a", "cpu", "fp32", "無負荷", "1", "5", "10", "40", ""],
        ]);
        let said = check_stage_evidence(&table, &Results::parse(&text)?);
        assert!(
            matches!(said, Err(ResultsError::Evidence { .. })),
            "{said:?}"
        );
        Ok(())
    }

    #[test]
    fn a_url_in_the_evidence_justifies_a_cpu_row_without_numbers() -> Result<(), ResultsError> {
        let table = [stage("a", "CPU", "https://example.org/no-gpu-tool")];
        check_stage_evidence(&table, &Results::parse("# none")?)
    }

    #[test]
    fn a_gpu_row_without_percentiles_fails() -> Result<(), ResultsError> {
        let table = [stage("a", "GPU", "x")];
        let text = cells_text(&[[
            "a",
            "cuda",
            "fp32",
            "無負荷",
            "1",
            "5",
            "未測定",
            "未測定",
            "",
        ]]);
        let said = check_stage_evidence(&table, &Results::parse(&text)?);
        assert!(
            matches!(said, Err(ResultsError::Evidence { .. })),
            "{said:?}"
        );
        Ok(())
    }

    #[test]
    fn a_remaining_measure_row_is_named() -> Result<(), ResultsError> {
        let table = [stage("a", "GPU", ""), stage("b", "測る", "")];
        let text = cells_text(&[
            ["a", "cuda", "fp32", "無負荷", "1", "5", "1", "2", ""],
            ["b", "cuda", "fp32", "無負荷", "1", "5", "1", "2", ""],
        ]);
        assert_eq!(
            check_placements_measured(&table, &Results::parse(&text)?),
            Err(ResultsError::MeasureRowRemains(String::from("b")))
        );
        Ok(())
    }

    #[test]
    fn a_stage_with_no_result_row_is_named() -> Result<(), ResultsError> {
        let table = [stage("a", "GPU", ""), stage("b", "CPU", "")];
        let text = cells_text(&[["a", "cuda", "fp32", "無負荷", "1", "5", "1", "2", ""]]);
        assert_eq!(
            check_placements_measured(&table, &Results::parse(&text)?),
            Err(ResultsError::NoResultRow(String::from("b")))
        );
        Ok(())
    }

    #[test]
    fn jit_rows_must_be_two_separate_states() -> Result<(), ResultsError> {
        let rows = |cache: &str, other: Option<&str>| {
            let mut all = vec![vec![
                cache.to_owned(),
                String::from("1"),
                String::from("900"),
                String::from("700"),
            ]];
            if let Some(other) = other {
                all.push(vec![
                    other.to_owned(),
                    String::from("1"),
                    String::from("700"),
                    String::from("690"),
                ]);
            }
            format!("{JIT}\n\n{}", render(&JIT_HEADER, &all))
        };
        let both = Results::parse(&rows("empty", Some("warm")))?;
        assert_eq!(check_jit_rows(&both), Ok(()));
        let only_empty = Results::parse(&rows("empty", None))?;
        assert_eq!(
            check_jit_rows(&only_empty),
            Err(ResultsError::NoJitRow("warm"))
        );
        let only_warm = Results::parse(&rows("warm", None))?;
        assert_eq!(
            check_jit_rows(&only_warm),
            Err(ResultsError::NoJitRow("empty"))
        );
        let merged = Results::parse(&rows("empty+warm", None))?;
        assert!(matches!(
            check_jit_rows(&merged),
            Err(ResultsError::MergedJitRows(_))
        ));
        Ok(())
    }

    #[test]
    fn a_transfer_row_without_bytes_names_the_column() {
        let header = [
            "向き",
            "variant",
            "回数 p50",
            "回数 p95",
            "時間_us p50",
            "時間_us p95",
        ];
        let rows = vec![vec![
            String::from("host→device"),
            String::from("fp32"),
            String::from("1"),
            String::from("2"),
            String::from("3"),
            String::from("4"),
        ]];
        let text = format!("{TRANSFERS}\n\n{}", render(&header, &rows));
        assert_eq!(
            Results::parse(&text),
            Err(ResultsError::NoColumn {
                section: TRANSFERS,
                column: "bytes p50"
            })
        );
    }
}

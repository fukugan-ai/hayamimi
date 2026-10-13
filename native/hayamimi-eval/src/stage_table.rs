//! The stage placement table of the Rust port design (the table under
//! heading 3.5), read as rows, and the count RP-20 asks for.

use crate::error::EvalError;
use crate::mdtable::{first_table, section};

/// Heading of the section that holds the table.
pub const SECTION: &str = "### 3.5";

/// The word that marks a placement not yet decided by a measurement.
pub const UNDECIDED: &str = "測る";

/// One row of the table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageRow {
    pub name: String,
    pub placement: String,
    pub evidence: String,
}

/// The rows of the stage table in `design`, in document order.
pub fn parse_stage_table(design: &str) -> Result<Vec<StageRow>, EvalError> {
    let table = first_table(section(design, SECTION)?)?;
    let name = table.column("段")?;
    let placement = table.column("いまの置き場")?;
    // The last column carries the evidence; its title changes when the table is decided.
    let evidence = table.header.len().saturating_sub(1);
    if evidence <= placement {
        return Err(EvalError::Invalid(String::from(
            "the stage table has no evidence column",
        )));
    }
    table
        .rows
        .iter()
        .map(|row| {
            let cell = |at: usize| {
                row.get(at)
                    .cloned()
                    .ok_or_else(|| EvalError::Invalid(format!("a stage row has no cell {at}")))
            };
            Ok(StageRow {
                name: cell(name)?,
                placement: cell(placement)?,
                evidence: cell(evidence)?,
            })
        })
        .collect()
}

/// The start of the placement of a stage that has no GPU implementation to compare with.
pub const NO_GPU_PATH: &str = "CPU（GPU 経路なし）";

impl StageRow {
    /// Whether a GPU implementation of the stage exists, so that it is timed on both providers.
    pub fn has_gpu_path(&self) -> bool {
        !self.placement.starts_with(NO_GPU_PATH)
    }
}

/// How many rows have a GPU path.
pub fn count_with_gpu_path(table: &[StageRow]) -> usize {
    table.iter().filter(|row| row.has_gpu_path()).count()
}

/// How many rows still say `測る` in the placement column.
pub fn count_measure_rows(table: &[StageRow]) -> usize {
    table
        .iter()
        .filter(|row| row.placement.contains(UNDECIDED))
        .count()
}

/// The first row that still says `測る`.
pub fn first_measure_row(table: &[StageRow]) -> Option<&StageRow> {
    table.iter().find(|row| row.placement.contains(UNDECIDED))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DESIGN: &str = "### 3.4 x\n\n| a |\n|---|\n| z |\n\n### 3.5 段ごとの置き場\n\n本文\n\n| 段 | いまの置き場 | 根拠 |\n|---|---|---|\n| VAD | 測る | Silero |\n| 句読点 | GPU | BERT |\n| 取り込み | CPU | 道具が無い |\n\n### 3.6 y\n\n| 段 | いまの置き場 | 根拠 |\n|---|---|---|\n| other | 測る | no |\n";

    #[test]
    fn rows_are_read_from_the_3_5_section_only() -> Result<(), EvalError> {
        let rows = parse_stage_table(DESIGN)?;
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[2].name, "取り込み");
        assert_eq!(rows[2].evidence, "道具が無い");
        Ok(())
    }

    #[test]
    fn a_design_without_the_section_is_an_error() {
        assert!(matches!(
            parse_stage_table("# nothing"),
            Err(EvalError::Invalid(_))
        ));
    }

    #[test]
    fn a_table_with_only_two_columns_has_no_evidence_column() {
        let design = "### 3.5 x\n\n| 段 | いまの置き場 |\n|---|---|\n| a | GPU |\n";
        assert!(matches!(
            parse_stage_table(design),
            Err(EvalError::Invalid(_))
        ));
    }

    fn row(placement: &str) -> StageRow {
        StageRow {
            name: String::from("s"),
            placement: String::from(placement),
            evidence: String::new(),
        }
    }

    #[test]
    fn a_partial_measure_row_is_counted() {
        let rows = [row("GPU"), row("CPU"), row("GPU（解き直し）。判定は測る")];
        assert_eq!(count_measure_rows(&rows), 1);
    }

    #[test]
    fn no_rows_have_no_measure_rows() {
        assert_eq!(count_measure_rows(&[]), 0);
        assert!(first_measure_row(&[]).is_none());
    }

    #[test]
    fn the_first_measure_row_is_named() {
        let rows = [row("GPU"), row("測る"), row("測る")];
        assert_eq!(
            first_measure_row(&rows).map(|r| r.placement.as_str()),
            Some("測る")
        );
    }
}

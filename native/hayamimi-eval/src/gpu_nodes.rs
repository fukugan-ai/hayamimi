//! RP-6: the CPU nodes of the models the stage table places on the GPU.
//!
//! ONNX Runtime keeps some nodes of a CUDA session on the CPU. The node table of
//! `docs/results/gpu_stages.md` records how many, per model, and a model the
//! stage table places on the GPU may not have more CPU nodes than that row.
//! A model the table places on the CPU, and a model the table does not name,
//! are recorded and not judged.

use crate::gpu_results::{NODES, NodeRow, Results, ResultsError};
use crate::stage_table::StageRow;

/// The provider a node row must name to bound a GPU model.
const CUDA: &str = "cuda";

/// The start of the placement of a stage on the GPU.
const GPU_PLACEMENT: &str = "GPU";

/// The model files each stage reads, as file stems, by the start of the stage name. A stage
/// whose model is not an ONNX file (the translation) is not listed, and a model that is not
/// listed is recorded and not judged.
const STAGE_MODELS: [(&str, &[&str]); 4] = [
    ("VAD", &["silero_vad"]),
    ("言語判定", &["tiny-encoder", "tiny-decoder"]),
    (
        "認識の encoder",
        &[
            "encoder-epoch-35-avg-1",
            "decoder-epoch-35-avg-1",
            "joiner-epoch-35-avg-1",
        ],
    ),
    ("句読点（BERT", &["punct_bert"]),
];

/// What a `gpu.session.placed` record says about one model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    pub model: String,
    pub cpu_nodes: u64,
}

/// The row of the table whose stage reads `model`, if the model is listed.
pub fn stage_of_model<'a>(table: &'a [StageRow], model: &str) -> Option<&'a StageRow> {
    let (prefix, _) = STAGE_MODELS.iter().find(|(_, stems)| {
        stems.iter().any(|stem| {
            model
                .strip_prefix(stem)
                .is_some_and(|rest| rest.starts_with('.'))
        })
    })?;
    table.iter().find(|row| row.name.starts_with(prefix))
}

/// The ONNX files (fp32) of the stages the table places on the GPU, in table order.
pub fn gpu_models(table: &[StageRow]) -> Vec<String> {
    table
        .iter()
        .filter(|row| row.placement.starts_with(GPU_PLACEMENT))
        .flat_map(|row| {
            STAGE_MODELS
                .iter()
                .filter(|(prefix, _)| row.name.starts_with(prefix))
                .flat_map(|(_, stems)| stems.iter().map(|stem| format!("{stem}.onnx")))
        })
        .collect()
}

fn node_row<'a>(nodes: &'a [NodeRow], model: &str) -> Option<&'a NodeRow> {
    nodes
        .iter()
        .find(|row| row.model == model && row.device == CUDA)
}

/// Every model the table places on the GPU has a row in the node table of `results`.
pub fn check_gpu_models_have_rows(
    table: &[StageRow],
    results: &Results,
) -> Result<(), ResultsError> {
    let Some(nodes) = &results.nodes else {
        return Err(ResultsError::NoSection(NODES));
    };
    for model in gpu_models(table) {
        if node_row(nodes, &model).is_none() {
            return Err(ResultsError::NoNodeRow(model));
        }
    }
    Ok(())
}

/// Every model the table places on the GPU has a node row and a `gpu.session.placed` record
/// whose CPU nodes are at most the node row's.
pub fn check_gpu_model_nodes(
    table: &[StageRow],
    nodes: &[NodeRow],
    placed: &[Placed],
) -> Result<(), ResultsError> {
    for model in gpu_models(table) {
        let Some(recorded) = node_row(nodes, &model) else {
            return Err(ResultsError::NoNodeRow(model));
        };
        let Some(event) = placed.iter().find(|event| event.model == model) else {
            return Err(ResultsError::NoPlacedEvent(model));
        };
        if event.cpu_nodes > recorded.cpu_nodes {
            return Err(ResultsError::CpuNodesOverRecord {
                model,
                found: event.cpu_nodes,
                recorded: recorded.cpu_nodes,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stage(name: &str, placement: &str) -> StageRow {
        StageRow {
            name: name.to_owned(),
            placement: placement.to_owned(),
            evidence: String::new(),
        }
    }

    fn node(model: &str, cpu_nodes: u64) -> NodeRow {
        NodeRow {
            model: model.to_owned(),
            device: String::from(CUDA),
            cuda_nodes: 100,
            cpu_nodes,
        }
    }

    #[test]
    fn a_table_without_rows_has_no_gpu_models() {
        assert!(gpu_models(&[]).is_empty());
    }

    #[test]
    fn the_models_of_the_gpu_stages_are_listed_in_table_order() {
        let table = [
            stage("VAD（Silero）", "CPU"),
            stage("言語判定（whisper-tiny）", "GPU"),
            stage("句読点（BERT、`ort` の CUDA）", "GPU"),
            stage("訳 T と E（llama-server、CUDA）", "GPU"),
        ];
        assert_eq!(
            gpu_models(&table),
            ["tiny-encoder.onnx", "tiny-decoder.onnx", "punct_bert.onnx"]
        );
    }

    #[test]
    fn a_stage_placed_on_the_cpu_names_no_gpu_model() {
        let table = [stage("言語判定（whisper-tiny）", "CPU")];
        assert!(gpu_models(&table).is_empty());
    }

    #[test]
    fn a_model_is_found_under_its_stage_whatever_its_precision() {
        let table = [
            stage("VAD（Silero）", "CPU"),
            stage("認識の encoder（全 model）", "CPU（int8）"),
        ];
        let found = stage_of_model(&table, "encoder-epoch-35-avg-1.int8.onnx");
        assert_eq!(found.map(|row| row.placement.as_str()), Some("CPU（int8）"));
        let vad = stage_of_model(&table, "silero_vad.onnx");
        assert_eq!(vad.map(|row| row.name.as_str()), Some("VAD（Silero）"));
    }

    #[test]
    fn a_model_that_is_not_listed_has_no_stage() {
        let table = [stage("VAD（Silero）", "CPU")];
        assert!(stage_of_model(&table, "unlisted.onnx").is_none());
        // A stem is a whole file name up to its first dot, not a prefix of a longer name.
        assert!(stage_of_model(&table, "silero_vad_v2.onnx").is_none());
    }

    #[test]
    fn a_node_row_of_a_cpu_session_does_not_bound_a_gpu_model() {
        let table = [stage("句読点（BERT、`ort` の CUDA）", "GPU")];
        let mut cpu_row = node("punct_bert.onnx", 0);
        cpu_row.device = String::from("cpu");
        let placed = [Placed {
            model: String::from("punct_bert.onnx"),
            cpu_nodes: 0,
        }];
        let said = check_gpu_model_nodes(&table, &[cpu_row], &placed);
        assert!(matches!(said, Err(ResultsError::NoNodeRow(model)) if model == "punct_bert.onnx"));
    }

    #[test]
    fn a_document_without_the_node_section_is_an_error() {
        let table = [stage("句読点（BERT、`ort` の CUDA）", "GPU")];
        let said = check_gpu_models_have_rows(&table, &Results::default());
        assert_eq!(said, Err(ResultsError::NoSection(NODES)));
    }

    #[test]
    fn a_table_with_no_gpu_model_needs_no_node_row() {
        let results = Results {
            nodes: Some(Vec::new()),
            ..Results::default()
        };
        assert_eq!(check_gpu_models_have_rows(&[], &results), Ok(()));
    }
}

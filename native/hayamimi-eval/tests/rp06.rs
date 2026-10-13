//! RP-6: inference ran on the GPU where the stage table says so. The cases here are the ones
//! that read fixtures and the results document: the compute-apps table, the providers of the
//! created sessions and the CPU nodes of the models placed on the GPU. The two cases that run
//! the `hayamimi` binary (T-RP-6-012 and T-RP-6-013) wait for the slice that builds it.

mod support;

use std::path::Path;

use hayamimi_eval::gpu_nodes::{
    Placed, check_gpu_model_nodes, check_gpu_models_have_rows, gpu_models,
};
use hayamimi_eval::gpu_results::{NodeRow, Results, ResultsError};
use hayamimi_eval::gpu_sessions::{Created, RunError, check_compute_apps, check_session_providers};
use hayamimi_eval::stage_table::{StageRow, parse_stage_table};
use support::{REPO, RESULTS, TestResult, read};

const HEADER: &str = "process_name, used_memory [MiB]\n";

fn apps(rows: &str) -> String {
    format!("{HEADER}{rows}")
}

/// T-RP-6-001
#[test]
fn hayamimi_with_positive_memory_passes() {
    let said = check_compute_apps(&apps("hayamimi, 2048 MiB\n"), "hayamimi");
    assert_eq!(said, Ok(2048));
}

/// T-RP-6-002
#[test]
fn one_mib_of_used_memory_passes() {
    let said = check_compute_apps(&apps("hayamimi, 1 MiB\n"), "hayamimi");
    assert_eq!(said, Ok(1));
}

/// T-RP-6-003
#[test]
fn zero_mib_of_used_memory_is_rejected() {
    let said = check_compute_apps(&apps("hayamimi, 0 MiB\n"), "hayamimi");
    assert_eq!(said, Err(RunError::NoMemory(String::from("hayamimi"))));
}

/// T-RP-6-004
#[test]
fn missing_hayamimi_process_is_rejected() {
    let said = check_compute_apps(&apps("python, 500 MiB\n"), "hayamimi");
    assert_eq!(said, Err(RunError::NoProcess(String::from("hayamimi"))));
}

/// T-RP-6-005: the same function reads the usage of the translation server.
#[test]
fn llama_server_process_is_read_by_the_same_function() {
    let rows = apps("/usr/bin/llama-server, 6144 MiB\nhayamimi, 2048 MiB\n");
    assert_eq!(check_compute_apps(&rows, "llama-server"), Ok(6144));
}

fn stage(name: &str, placement: &str) -> StageRow {
    StageRow {
        name: name.to_owned(),
        placement: placement.to_owned(),
        evidence: String::new(),
    }
}

const VAD: &str = "VAD（Silero、sherpa-onnx の `provider`）";
const LID: &str = "言語判定（whisper-tiny）";
const RECOGNITION: &str = "認識の encoder（全 model）";
const BERT: &str = "句読点（BERT、`ort` の CUDA）";

/// The table with the language ID and the BERT on the GPU, the VAD and the recognition on the CPU.
fn table() -> Vec<StageRow> {
    vec![
        stage(VAD, "CPU"),
        stage(LID, "GPU"),
        stage(RECOGNITION, "CPU（int8）"),
        stage(BERT, "GPU"),
    ]
}

/// The table with only the BERT on the GPU.
fn bert_only() -> Vec<StageRow> {
    vec![
        stage(VAD, "CPU"),
        stage(RECOGNITION, "CPU（int8）"),
        stage(BERT, "GPU"),
    ]
}

fn created(model: &str, provider: &str) -> Created {
    Created {
        model: model.to_owned(),
        provider: provider.to_owned(),
    }
}

fn sessions() -> Vec<Created> {
    vec![
        created("silero_vad.onnx", "cpu"),
        created("tiny-encoder.onnx", "cuda"),
        created("tiny-decoder.onnx", "cuda"),
        created("encoder-epoch-35-avg-1.int8.onnx", "cpu"),
        created("punct_bert.onnx", "cuda"),
    ]
}

/// T-RP-6-006
#[test]
fn all_sessions_on_cuda_pass() {
    assert_eq!(check_session_providers(&table(), &sessions()), Ok(()));
}

/// T-RP-6-007: one GPU session got the CPU.
#[test]
fn one_cpu_session_names_its_model() {
    let mut run = sessions();
    run[2] = created("tiny-decoder.onnx", "cpu");
    let said = check_session_providers(&table(), &run);
    assert!(
        matches!(said, Err(RunError::WrongProvider { model, .. }) if model == "tiny-decoder.onnx")
    );
}

/// T-RP-6-008: no session event is not "every session is where the table says".
#[test]
fn no_session_events_is_an_error() {
    assert_eq!(
        check_session_providers(&table(), &[]),
        Err(RunError::NoSessions)
    );
}

/// T-RP-6-017: a session of a stage placed on the CPU got CUDA.
#[test]
fn one_cuda_session_in_a_cpu_stage_names_its_model() {
    let mut run = sessions();
    run[3] = created("encoder-epoch-35-avg-1.int8.onnx", "cuda");
    let said = check_session_providers(&table(), &run);
    assert!(matches!(
        said,
        Err(RunError::WrongProvider { model, .. }) if model == "encoder-epoch-35-avg-1.int8.onnx"
    ));
}

fn node(model: &str, cuda_nodes: u64, cpu_nodes: u64) -> NodeRow {
    NodeRow {
        model: model.to_owned(),
        device: String::from("cuda"),
        cuda_nodes,
        cpu_nodes,
    }
}

fn placed(model: &str, cpu_nodes: u64) -> Placed {
    Placed {
        model: model.to_owned(),
        cpu_nodes,
    }
}

/// The node table of the results: the BERT with 453 nodes on CUDA and 77 on the CPU.
fn bert_nodes() -> Vec<NodeRow> {
    vec![node("punct_bert.onnx", 453, 77)]
}

/// T-RP-6-009: 77 is not more than 77.
#[test]
fn gpu_model_at_the_recorded_cpu_node_count_passes() {
    let run = [placed("punct_bert.onnx", 77)];
    assert_eq!(
        check_gpu_model_nodes(&bert_only(), &bert_nodes(), &run),
        Ok(())
    );
}

/// T-RP-6-010: 78 is one more than 77.
#[test]
fn gpu_model_one_node_over_the_recorded_count_is_rejected() {
    let run = [placed("punct_bert.onnx", 78)];
    assert_eq!(
        check_gpu_model_nodes(&bert_only(), &bert_nodes(), &run),
        Err(ResultsError::CpuNodesOverRecord {
            model: String::from("punct_bert.onnx"),
            found: 78,
            recorded: 77,
        })
    );
}

/// T-RP-6-011: a model the table does not name is recorded and not judged.
#[test]
fn cpu_nodes_of_unlisted_models_are_recorded_only() {
    let run = [placed("punct_bert.onnx", 77), placed("unlisted.onnx", 5)];
    assert_eq!(
        check_gpu_model_nodes(&bert_only(), &bert_nodes(), &run),
        Ok(())
    );
}

/// T-RP-6-014: no CPU node at all is within the record.
#[test]
fn gpu_model_with_zero_cpu_nodes_passes() {
    let run = [placed("punct_bert.onnx", 0)];
    assert_eq!(
        check_gpu_model_nodes(&bert_only(), &bert_nodes(), &run),
        Ok(())
    );
}

/// T-RP-6-015: a model placed on the CPU is not judged by its row of the node table.
/// Judging every row of the node table would find 1600 above 1599.
#[test]
fn cpu_placed_model_is_not_judged_by_the_node_table() {
    let mut nodes = bert_nodes();
    nodes.push(node("encoder-epoch-35-avg-1.onnx", 3537, 1599));
    let run = [
        placed("punct_bert.onnx", 77),
        placed("encoder-epoch-35-avg-1.onnx", 1600),
    ];
    assert_eq!(check_gpu_model_nodes(&bert_only(), &nodes, &run), Ok(()));
}

/// T-RP-6-016: no record is not "no CPU node".
#[test]
fn gpu_model_without_a_placed_event_is_rejected() {
    assert_eq!(
        check_gpu_model_nodes(&bert_only(), &bert_nodes(), &[]),
        Err(ResultsError::NoPlacedEvent(String::from("punct_bert.onnx")))
    );
}

/// T-RP-6-018: a model placed on the CPU needs no row in the node table.
#[test]
fn cpu_placed_model_without_a_node_table_row_is_recorded_only() {
    let run = [
        placed("punct_bert.onnx", 77),
        placed("encoder-epoch-35-avg-1.onnx", 5),
    ];
    assert_eq!(
        check_gpu_model_nodes(&bert_only(), &bert_nodes(), &run),
        Ok(())
    );
}

/// T-RP-6-019: a model placed on the GPU needs a row; a count of 0 is not read against a missing row.
#[test]
fn gpu_model_without_a_node_table_row_is_rejected() {
    let run = [placed("tiny-encoder.onnx", 0)];
    assert_eq!(
        check_gpu_model_nodes(&table(), &bert_nodes(), &run),
        Err(ResultsError::NoNodeRow(String::from("tiny-encoder.onnx")))
    );
}

/// The results as they are now: every model the design table places on the GPU, the two
/// whisper-tiny models and the BERT, has a row in the node table. The count of CPU nodes of
/// a run is judged against these rows by the cases above.
#[test]
#[ignore = "reads the real design and the real results"]
fn real_node_table_has_a_row_for_every_gpu_model() -> TestResult {
    let design = read(&Path::new(REPO).join("docs/design/rust_port.ja.md"))?;
    let results = read(&Path::new(RESULTS).join("gpu_stages.md"))?;
    let table = parse_stage_table(&design)?;
    assert_eq!(
        gpu_models(&table),
        ["tiny-encoder.onnx", "tiny-decoder.onnx", "punct_bert.onnx"]
    );
    check_gpu_models_have_rows(&table, &Results::parse(&results)?)?;
    Ok(())
}

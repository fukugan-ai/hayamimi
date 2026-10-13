//! RP-6: what a GPU run shows of where its inference ran.
//!
//! The compute-apps table of `nvidia-smi` names the processes that hold GPU memory, and
//! the sessions a run created name the provider each model got. The provider a model
//! should get is the placement of its stage in the table of the design.

use std::fmt;

use crate::gpu_nodes::stage_of_model;
use crate::stage_table::StageRow;

/// The memory column of the compute-apps table ends with this unit.
const MIB: &str = "MiB";

#[derive(Debug, PartialEq, Eq)]
pub enum RunError {
    /// No row of the compute-apps table names the process.
    NoProcess(String),
    /// The row of the process has no memory figure in MiB.
    BadLine(String),
    /// The process holds no GPU memory.
    NoMemory(String),
    /// The run created no session.
    NoSessions,
    /// A session got another provider than the placement of its stage says.
    WrongProvider {
        model: String,
        expected: &'static str,
        found: String,
    },
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RunError::NoProcess(name) => write!(f, "no compute app is named `{name}`"),
            RunError::BadLine(line) => write!(f, "no memory figure in MiB in `{line}`"),
            RunError::NoMemory(name) => write!(f, "`{name}` holds no GPU memory"),
            RunError::NoSessions => write!(f, "the run created no session"),
            RunError::WrongProvider {
                model,
                expected,
                found,
            } => write!(
                f,
                "model `{model}` got provider `{found}`, not `{expected}`"
            ),
        }
    }
}

impl std::error::Error for RunError {}

/// One `gpu.session.created` record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Created {
    pub model: String,
    pub provider: String,
}

/// The executable of a process name: the last path segment without `.exe`.
fn executable(name: &str) -> &str {
    let file = name.rsplit_once(['/', '\\']).map_or(name, |(_, file)| file);
    match file.strip_suffix(".exe") {
        Some(stem) => stem,
        None => file,
    }
}

/// The memory (MiB) the process holds, from `nvidia-smi --query-compute-apps=process_name,used_memory
/// --format=csv`: a header line, then `<process>, <N> MiB` lines.
pub fn check_compute_apps(csv: &str, process: &str) -> Result<u64, RunError> {
    let mut rows = csv.lines().skip(1).filter_map(|line| line.split_once(", "));
    let Some((_, memory)) = rows.find(|(name, _)| executable(name) == process) else {
        return Err(RunError::NoProcess(process.to_owned()));
    };
    let Some(mib) = memory
        .trim()
        .strip_suffix(MIB)
        .and_then(|figure| figure.trim().parse::<u64>().ok())
    else {
        return Err(RunError::BadLine(memory.to_owned()));
    };
    if mib == 0 {
        return Err(RunError::NoMemory(process.to_owned()));
    }
    Ok(mib)
}

/// The provider a stage placed on the GPU or on the CPU should get; none for a stage inside sherpa-onnx.
fn expected_provider(row: &StageRow) -> Option<&'static str> {
    if row.placement.starts_with("GPU") {
        Some("cuda")
    } else if row.placement.starts_with("CPU") {
        Some("cpu")
    } else {
        None
    }
}

/// Every session of a model the table names got the provider its stage is placed on.
/// A run that created no session proves nothing and is refused.
pub fn check_session_providers(table: &[StageRow], created: &[Created]) -> Result<(), RunError> {
    if created.is_empty() {
        return Err(RunError::NoSessions);
    }
    for session in created {
        let Some(row) = stage_of_model(table, &session.model) else {
            continue;
        };
        let Some(expected) = expected_provider(row) else {
            continue;
        };
        if session.provider != expected {
            return Err(RunError::WrongProvider {
                model: session.model.clone(),
                expected,
                found: session.provider.clone(),
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

    fn session(model: &str, provider: &str) -> Created {
        Created {
            model: model.to_owned(),
            provider: provider.to_owned(),
        }
    }

    #[test]
    fn the_executable_is_the_last_path_segment_without_exe() {
        assert_eq!(executable("/usr/bin/llama-server"), "llama-server");
        assert_eq!(executable("C:\\apps\\hayamimi.exe"), "hayamimi");
        assert_eq!(executable("hayamimi"), "hayamimi");
    }

    #[test]
    fn a_header_alone_names_no_process() {
        let said = check_compute_apps("process_name, used_memory [MiB]\n", "hayamimi");
        assert_eq!(said, Err(RunError::NoProcess(String::from("hayamimi"))));
    }

    #[test]
    fn a_row_without_a_memory_unit_is_a_bad_line() {
        let said = check_compute_apps("process_name, used_memory\nhayamimi, 2048\n", "hayamimi");
        assert_eq!(said, Err(RunError::BadLine(String::from("2048"))));
    }

    #[test]
    fn a_table_of_no_sessions_is_refused() {
        let table = [stage("VAD（Silero）", "CPU")];
        assert_eq!(
            check_session_providers(&table, &[]),
            Err(RunError::NoSessions)
        );
    }

    #[test]
    fn a_model_the_table_does_not_name_is_not_judged() {
        let table = [stage("VAD（Silero）", "CPU")];
        let created = [session("unlisted.onnx", "cuda")];
        assert_eq!(check_session_providers(&table, &created), Ok(()));
    }

    #[test]
    fn a_stage_inside_sherpa_onnx_is_not_judged() {
        let table = [stage("VAD（Silero）", "sherpa-onnx の実装のまま")];
        let created = [session("silero_vad.onnx", "cuda")];
        assert_eq!(check_session_providers(&table, &created), Ok(()));
    }
}

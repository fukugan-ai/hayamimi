//! The command line of the stage measurement: `stages`, `stages-report` and `nsys-transfers`.

use std::path::{Path, PathBuf};

use clap::{Args, ValueEnum};
use serde_json::Value;

use crate::error::EvalError;
use crate::gpu_results::TransferRow;
use crate::log;
use crate::manifest::write_text;
use crate::nsys::{self, MemoryStats};
use crate::ort_stages::{self, Device, OrtRuntime, PlacedRow};
use crate::stage_models::Precision;
use crate::stage_peer;
use crate::stage_report::{self, Inputs, PythonSample};
use crate::stage_runs::{self, Options, Provider};
use crate::stages::{aggregate, write_run};

#[derive(Clone, Copy, ValueEnum)]
pub enum ProviderArg {
    Cuda,
    Cpu,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum CacheArg {
    Empty,
    Warm,
}

#[derive(Args)]
pub struct StagesArgs {
    /// The provider every model runs on. A cuda run fails when the CUDA provider library is not loaded.
    #[arg(long, value_enum)]
    pub provider: ProviderArg,
    /// The recording (16 kHz mono 16-bit wav).
    #[arg(long)]
    pub wav: PathBuf,
    /// Its manifest; default: manifests/<wav stem>.json beside the wav.
    #[arg(long)]
    pub manifest: Option<PathBuf>,
    /// The directory that holds the model directories.
    #[arg(long)]
    pub models: PathBuf,
    /// Copies of the recording to run through every stage.
    #[arg(long, default_value_t = 5)]
    pub repeat: u32,
    /// The run file (.jsonl) to write.
    #[arg(long)]
    pub out: PathBuf,
    /// The llama-server of the translation stage.
    #[arg(long)]
    pub llama_url: Option<String>,
    /// Send translation requests to this llama-server for the whole run.
    #[arg(long)]
    pub load_url: Option<String>,
    #[arg(long, default_value_t = 2)]
    pub load_clients: u32,
    /// The state of the JIT cache this process was started with (a label).
    #[arg(long, value_enum, default_value = "warm")]
    pub cache: CacheArg,
    /// The number of this run among the runs of one combination.
    #[arg(long, default_value_t = 1)]
    pub run: u32,
    /// Measure only these stages, comma separated.
    #[arg(long, value_delimiter = ',')]
    pub only: Vec<String>,
    /// Also run the int8 recognizer on CUDA, to count the copies it adds.
    #[arg(long)]
    pub int8_on_cuda: bool,
    /// Open a profiler capture range around every decode.
    #[arg(long)]
    pub profile_ranges: bool,
    /// `punct_ja_parity.json`: the `input_ids` the punctuation BERT is run on (the stage is left out without it).
    #[arg(long)]
    pub punct_cases: Option<PathBuf>,
    /// Other sources decoding in processes of their own for the whole run (0 measures a source alone).
    #[arg(long, default_value_t = 0)]
    pub peers: u32,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum PrecisionArg {
    Fp32,
    Fp16,
    Int8,
}

#[derive(Args)]
pub struct PeerArgs {
    /// The provider the recognizer runs on.
    #[arg(long, value_enum)]
    pub provider: ProviderArg,
    /// The recording (16 kHz mono 16-bit wav).
    #[arg(long)]
    pub wav: PathBuf,
    /// Its manifest.
    #[arg(long)]
    pub manifest: PathBuf,
    /// The directory that holds the model directories.
    #[arg(long)]
    pub models: PathBuf,
    /// The model variant the recognizer loads.
    #[arg(long, value_enum)]
    pub precision: PrecisionArg,
}

#[derive(Args)]
pub struct R7ReportArgs {
    /// Run files written by `stages`.
    #[arg(long, required = true, num_args = 1..)]
    pub runs: Vec<PathBuf>,
    /// The conditions the runs were taken under (markdown, hand-written), copied in verbatim.
    #[arg(long)]
    pub conditions: PathBuf,
    /// The document to write.
    #[arg(long)]
    pub out: PathBuf,
}

#[derive(Args)]
pub struct OrtPlacedArgs {
    /// The ONNX model whose node placement is counted.
    #[arg(long)]
    pub model: PathBuf,
    /// The provider the session is built for.
    #[arg(long, value_enum)]
    pub provider: ProviderArg,
    /// The placement row (JSON) to write.
    #[arg(long)]
    pub out: PathBuf,
}

#[derive(Args)]
pub struct StagesReportArgs {
    /// Run files written by `stages`.
    #[arg(long, required = true, num_args = 1..)]
    pub runs: Vec<PathBuf>,
    /// The design document that holds the stage table.
    #[arg(long)]
    pub design: PathBuf,
    /// The build.json of the sherpa-onnx build the runs used.
    #[arg(long)]
    pub build_json: PathBuf,
    /// The name of the processor.
    #[arg(long)]
    pub cpu: String,
    /// Transfer rows written by `nsys-transfers`.
    #[arg(long, num_args = 0..)]
    pub transfers: Vec<PathBuf>,
    /// Node placement rows written by `ort-placed`.
    #[arg(long, num_args = 0..)]
    pub placed: Vec<PathBuf>,
    /// Samples of the Python baseline (the .jsonl `stream` wrote).
    #[arg(long, num_args = 0..)]
    pub python: Vec<PathBuf>,
    /// The `--min-silence` of the Python runs, in milliseconds.
    #[arg(long, default_value_t = 350.0)]
    pub min_silence_ms: f64,
    /// The document to write.
    #[arg(long)]
    pub out: PathBuf,
}

#[derive(Args)]
pub struct NsysTransfersArgs {
    /// The directory of `<name>.mem_time.csv` and `<name>.mem_size.csv` pairs, one pair per utterance.
    #[arg(long)]
    pub dir: PathBuf,
    /// The model variant the profiles were taken with.
    #[arg(long)]
    pub variant: String,
    /// The transfer rows (JSON) to write.
    #[arg(long)]
    pub out: PathBuf,
}

fn default_manifest(wav: &Path) -> Result<PathBuf, EvalError> {
    let (Some(stem), Some(directory)) = (wav.file_stem(), wav.parent()) else {
        return Err(EvalError::Invalid(format!(
            "{} has no file name or directory",
            wav.display()
        )));
    };
    Ok(directory
        .join("manifests")
        .join(format!("{}.json", stem.to_string_lossy())))
}

/// Runs the stages and writes the run file.
pub fn stages(args: StagesArgs) -> Result<(), EvalError> {
    // The C library is delay-loaded; the copy beside the executable must be the first ONNX Runtime in the process.
    #[cfg(windows)]
    hayamimi_gpu::runtime::preload_beside_executable("onnxruntime.dll")
        .map_err(|e| EvalError::Invalid(e.to_string()))?;
    let manifest = match args.manifest {
        Some(path) => path,
        None => default_manifest(&args.wav)?,
    };
    let options = Options {
        provider: match args.provider {
            ProviderArg::Cuda => Provider::Cuda,
            ProviderArg::Cpu => Provider::Cpu,
        },
        wav: args.wav,
        manifest,
        models: args.models,
        repeat: args.repeat,
        llama_url: args.llama_url,
        load_url: args.load_url,
        load_clients: args.load_clients,
        cache: match args.cache {
            CacheArg::Empty => String::from("empty"),
            CacheArg::Warm => String::from("warm"),
        },
        run: args.run,
        only: args.only,
        int8_on_cuda: args.int8_on_cuda,
        profile_ranges: args.profile_ranges,
        punct_cases: args.punct_cases,
        peers: args.peers,
    };
    let run = stage_runs::run(&options)?;
    write_run(&args.out, &run)?;
    for line in aggregate(&run)? {
        println!(
            "{}\t{}\t{}\tn={}\tp50_us={}\tp95_us={}",
            line.stage,
            line.variant,
            line.provider,
            line.samples,
            line.p50_us(),
            line.p95_us()
        );
    }
    log::info(
        "eval.stages.finished",
        "worker",
        "stage run written",
        &[("samples", Value::from(run.samples.len()))],
    );
    Ok(())
}

fn provider_of(arg: ProviderArg) -> Provider {
    match arg {
        ProviderArg::Cuda => Provider::Cuda,
        ProviderArg::Cpu => Provider::Cpu,
    }
}

/// Runs as another source: decodes the recording until standard input closes.
pub fn stages_peer(args: PeerArgs) -> Result<(), EvalError> {
    // The C library is delay-loaded; the copy beside the executable must be the first ONNX Runtime in the process.
    #[cfg(windows)]
    hayamimi_gpu::runtime::preload_beside_executable("onnxruntime.dll")
        .map_err(|e| EvalError::Invalid(e.to_string()))?;
    let precision = match args.precision {
        PrecisionArg::Fp32 => Precision::Fp32,
        PrecisionArg::Fp16 => Precision::Fp16,
        PrecisionArg::Int8 => Precision::Int8,
    };
    stage_peer::run_peer(&stage_peer::PeerArgs {
        provider: provider_of(args.provider),
        wav: args.wav,
        manifest: args.manifest,
        models: args.models,
        precision,
    })
}

/// Writes the R7 document from run files.
pub fn r7_report(args: R7ReportArgs) -> Result<(), EvalError> {
    let runs = args
        .runs
        .iter()
        .map(|path| crate::stages::read_run(path))
        .collect::<Result<Vec<_>, _>>()?;
    let sources: Vec<String> = args
        .runs
        .iter()
        .map(|path| path.display().to_string())
        .collect();
    let conditions = read_text(&args.conditions)?;
    let document = crate::r7_report::render_document(&runs, &sources, &conditions)?;
    write_text(&args.out, &document)?;
    println!("{}", args.out.display());
    Ok(())
}

/// Builds one model's session at verbose level and writes where its nodes went.
pub fn ort_placed(args: OrtPlacedArgs) -> Result<(), EvalError> {
    // The ONNX Runtime beside the executable must be the first in the process.
    #[cfg(windows)]
    hayamimi_gpu::runtime::preload_beside_executable("onnxruntime.dll")
        .map_err(|e| EvalError::Invalid(e.to_string()))?;
    let (device, name) = match args.provider {
        ProviderArg::Cuda => (Device::Cuda, "cuda"),
        ProviderArg::Cpu => (Device::Cpu, "cpu"),
    };
    let runtime = OrtRuntime::open(&ort_stages::library_beside_executable()?)?;
    let _session = runtime.session(&args.model, device, 1, true)?;
    let model = args
        .model
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| EvalError::Invalid(format!("{} has no file name", args.model.display())))?;
    let row = ort_stages::placed_row(&model, name, &runtime.take_placements()?)?;
    let mut text = serde_json::to_string_pretty(&row).map_err(|e| EvalError::json(&args.out, e))?;
    text.push('\n');
    write_text(&args.out, &text)?;
    println!(
        "{model} on {name}: {} nodes on CUDA, {} nodes on CPU",
        row.cuda_nodes, row.cpu_nodes
    );
    Ok(())
}

fn read_text(path: &Path) -> Result<String, EvalError> {
    std::fs::read_to_string(path).map_err(|e| EvalError::io(path, e))
}

fn read_transfers(paths: &[PathBuf]) -> Result<Vec<TransferRow>, EvalError> {
    let mut rows = Vec::new();
    for path in paths {
        let more: Vec<TransferRow> =
            serde_json::from_str(&read_text(path)?).map_err(|e| EvalError::json(path, e))?;
        rows.extend(more);
    }
    Ok(rows)
}

/// Writes the result document from run files.
pub fn stages_report(args: StagesReportArgs) -> Result<(), EvalError> {
    let runs = args
        .runs
        .iter()
        .map(|path| crate::stages::read_run(path))
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(beside_peers) = runs.iter().find(|run| run.meta.peers != 0) {
        return Err(EvalError::Invalid(format!(
            "a {} run with {} peers belongs to r7-report, not to this document",
            beside_peers.meta.provider, beside_peers.meta.peers
        )));
    }
    let design = read_text(&args.design)?;
    let build_json = read_text(&args.build_json)?;
    let transfers = read_transfers(&args.transfers)?;
    let mut placed: Vec<PlacedRow> = Vec::new();
    for path in &args.placed {
        placed.push(serde_json::from_str(&read_text(path)?).map_err(|e| EvalError::json(path, e))?);
    }
    let mut python: Vec<PythonSample> = Vec::new();
    for path in &args.python {
        python.extend(stage_report::read_python_baseline(path)?);
    }
    let document = stage_report::render_document(&Inputs {
        runs: &runs,
        design: &design,
        build_json: &build_json,
        cpu: &args.cpu,
        transfers: &transfers,
        placed: &placed,
        python: &python,
        min_silence_ms: args.min_silence_ms,
    })?;
    write_text(&args.out, &document)
}

/// Pairs of `nsys stats` CSV files in `dir`, by the name before `.mem_time.csv`.
fn csv_pairs(dir: &Path) -> Result<Vec<(PathBuf, PathBuf)>, EvalError> {
    let mut times: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| EvalError::io(dir, e))?
        .map(|entry| entry.map(|e| e.path()).map_err(|e| EvalError::io(dir, e)))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(".mem_time.csv"))
        })
        .collect();
    times.sort();
    times
        .into_iter()
        .map(|time| {
            let name = time
                .to_string_lossy()
                .replace(".mem_time.csv", ".mem_size.csv");
            let size = PathBuf::from(name);
            if size.is_file() {
                Ok((time, size))
            } else {
                Err(EvalError::Invalid(format!(
                    "{} has no size CSV beside it",
                    time.display()
                )))
            }
        })
        .collect()
}

/// Summarises one profile per utterance as transfer rows.
pub fn nsys_transfers(args: NsysTransfersArgs) -> Result<(), EvalError> {
    let pairs = csv_pairs(&args.dir)?;
    let mut per_utterance: Vec<MemoryStats> = Vec::with_capacity(pairs.len());
    for (time, size) in &pairs {
        let stats = nsys::parse_memory_stats(&read_text(time)?, &read_text(size)?)
            .map_err(|e| EvalError::Invalid(format!("{}: {e}", time.display())))?;
        per_utterance.push(stats);
    }
    let rows = nsys::transfer_rows(&args.variant, &per_utterance)?;
    let mut text =
        serde_json::to_string_pretty(&rows).map_err(|e| EvalError::json(&args.out, e))?;
    text.push('\n');
    write_text(&args.out, &text)?;
    println!("{} profiles summarised", per_utterance.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_manifest_sits_beside_the_wav_in_manifests() -> Result<(), EvalError> {
        assert_eq!(
            default_manifest(Path::new("d/lat-ja.wav"))?,
            PathBuf::from("d/manifests/lat-ja.json")
        );
        Ok(())
    }

    #[test]
    fn a_directory_that_does_not_exist_is_an_error() {
        assert!(matches!(
            csv_pairs(Path::new("no-such-directory")),
            Err(EvalError::Io { .. })
        ));
    }

    #[test]
    fn a_missing_transfer_file_is_an_error() {
        assert!(read_transfers(&[PathBuf::from("no-such.json")]).is_err());
    }
}

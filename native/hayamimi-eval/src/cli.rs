//! The command line: four sub commands, each a thin call into its module.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

use crate::error::EvalError;
use crate::{cli_stages, fleurs, log, manifest, record, scorecard, scorecard_run, stream};

#[derive(Parser)]
#[command(
    name = "hayamimi-eval",
    version,
    about = "Measurement and scoring tool for hayamimi (development only)"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Play a recording into /ingest at real-time pace and time the answers.
    Stream(StreamArgs),
    /// Record the wire format of a running server.
    Record(RecordArgs),
    /// Pin an evaluation set to its bytes, or join clips into one recording.
    FetchSet(FetchSetArgs),
    /// Write Japanese/target sentence pairs from FLEURS.
    FleursPairs(FleursArgs),
    /// Time every stage of the Rust port on one provider and write a run file.
    Stages(cli_stages::StagesArgs),
    /// Write the result document of the stage measurement from run files.
    StagesReport(cli_stages::StagesReportArgs),
    /// Summarise `nsys stats` CSV files, one pair per utterance, as transfer rows.
    NsysTransfers(cli_stages::NsysTransfersArgs),
    /// Count the nodes ONNX Runtime puts on the CUDA and the CPU provider for one model.
    OrtPlaced(cli_stages::OrtPlacedArgs),
    /// Score the engine on the real-speech sets and write the scorecard as JSON.
    Scorecard(ScorecardArgs),
}

#[derive(Args)]
struct ScorecardArgs {
    /// The engine to score; only hayamimi is built in.
    #[arg(long, default_value = "hayamimi")]
    engine: String,
    /// Evaluation set directories, each with a manifest.json, comma separated.
    #[arg(long, value_delimiter = ',', required = true)]
    sets: Vec<PathBuf>,
    /// The scorecard written (.json).
    #[arg(long)]
    out: PathBuf,
    /// The directory that holds the model directories.
    #[arg(long, default_value = "models")]
    models: PathBuf,
    /// Recognizers kept in memory besides the Japanese one; 0 keeps all.
    #[arg(long, default_value_t = 3)]
    max_resident: usize,
}

#[derive(Args)]
struct StreamArgs {
    /// The recording (16 kHz mono 16-bit wav).
    #[arg(long)]
    wav: PathBuf,
    /// Its manifest; default: manifests/<wav stem>.json beside the wav.
    #[arg(long)]
    manifest: Option<PathBuf>,
    /// Copies of the recording to play.
    #[arg(long, default_value_t = 5)]
    repeat: u32,
    /// The samples file (.jsonl); a summary and the raw events go beside it.
    #[arg(long)]
    out: PathBuf,
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
    /// The server's /ingest port; with `--sink loopback`, its /events port.
    #[arg(long, default_value_t = 8766)]
    port: u16,
    /// Where the recording goes: into /ingest, or out of the default output device.
    #[arg(long, value_enum, default_value_t = SinkArg::Ingest)]
    sink: SinkArg,
    /// With `--sink loopback` on a machine with no output device: the /ingest port to send to instead.
    #[arg(long, default_value_t = 8766)]
    ingest_port: u16,
    /// The port of the relay's `/events`, to read its translations too.
    #[arg(long)]
    relay_port: Option<u16>,
    /// Silence after each copy, in milliseconds.
    #[arg(long, default_value_t = 1500)]
    gap_ms: u32,
    /// Seconds to keep listening after the last sample is sent.
    #[arg(long, default_value_t = 8)]
    drain_s: u32,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum SinkArg {
    Ingest,
    Loopback,
}

impl From<SinkArg> for stream::Sink {
    fn from(arg: SinkArg) -> stream::Sink {
        match arg {
            SinkArg::Ingest => stream::Sink::Ingest,
            SinkArg::Loopback => stream::Sink::Loopback,
        }
    }
}

#[derive(Args)]
struct RecordArgs {
    /// The recording to play while listening.
    #[arg(long)]
    wav: PathBuf,
    /// The directory the capture is written to.
    #[arg(long)]
    out: PathBuf,
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
    #[arg(long, default_value_t = 8833)]
    http_port: u16,
    #[arg(long, default_value_t = 8766)]
    ingest_port: u16,
    #[arg(long, default_value_t = 1500)]
    gap_ms: u32,
    /// Seconds to listen after the audio ends, before the routes are called.
    #[arg(long, default_value_t = 8)]
    tail_s: u32,
    /// Seconds to listen after the session reset.
    #[arg(long, default_value_t = 4)]
    reset_wait_s: u32,
}

#[derive(Args)]
struct FetchSetArgs {
    /// Pin this manifest: add the sha256 of each wav and write --out.
    #[arg(long, conflicts_with = "concat")]
    pin: Option<PathBuf>,
    /// Join the --part clips into --wav-out and write --out.
    #[arg(long, requires = "wav_out", requires = "part")]
    concat: bool,
    /// The manifest written.
    #[arg(long)]
    out: PathBuf,
    /// The joined recording written by --concat.
    #[arg(long)]
    wav_out: Option<PathBuf>,
    /// Silence between clips, in milliseconds.
    #[arg(long, default_value_t = 1500)]
    gap_ms: u32,
    /// A source of clips: golden:<golden.json>:<lang> or manifest:<pinned.json>:<lang>.
    #[arg(long)]
    part: Vec<String>,
}

#[derive(Args)]
struct FleursArgs {
    /// Target languages, comma separated.
    #[arg(long, value_delimiter = ',', required = true)]
    targets: Vec<String>,
    #[arg(long, default_value_t = 50)]
    n: usize,
    #[arg(long, default_value_t = 0)]
    seed: u32,
    /// The directory the sentence files are written to.
    #[arg(long)]
    out: PathBuf,
    /// Also write <lang>.hyp.txt: each reference with its first word dropped.
    #[arg(long)]
    degrade: bool,
}

/// `manifests/<stem>.json` beside the wav.
fn default_manifest(wav: &Path) -> Result<PathBuf, EvalError> {
    let Some(stem) = wav.file_stem() else {
        return Err(EvalError::Invalid(format!(
            "{} has no file name",
            wav.display()
        )));
    };
    let Some(directory) = wav.parent() else {
        return Err(EvalError::Invalid(format!(
            "{} has no parent directory to hold manifests/",
            wav.display()
        )));
    };
    Ok(directory
        .join("manifests")
        .join(format!("{}.json", stem.to_string_lossy())))
}

fn execute(command: Command) -> Result<(), EvalError> {
    match command {
        Command::Stream(args) => {
            let manifest = match args.manifest {
                Some(path) => path,
                None => default_manifest(&args.wav)?,
            };
            stream::run(&stream::Options {
                host: args.host,
                port: args.port,
                sink: args.sink.into(),
                ingest_port: args.ingest_port,
                relay_port: args.relay_port,
                wav: args.wav,
                manifest,
                repeat: args.repeat,
                out: args.out,
                gap_ms: args.gap_ms,
                drain_s: args.drain_s,
            })
            .map(|_| ())
        }
        Command::Record(args) => record::run(&record::Options {
            host: args.host,
            http_port: args.http_port,
            ingest_port: args.ingest_port,
            wav: args.wav,
            out: args.out,
            gap_ms: args.gap_ms,
            tail_s: args.tail_s,
            reset_wait_s: args.reset_wait_s,
        })
        .map(|recorded| {
            for (kind, count) in &recorded.events_by_type {
                println!("{kind}\t{count}");
            }
        }),
        Command::FetchSet(args) => fetch_set(args),
        Command::Stages(args) => cli_stages::stages(args),
        Command::StagesReport(args) => cli_stages::stages_report(args),
        Command::NsysTransfers(args) => cli_stages::nsys_transfers(args),
        Command::OrtPlaced(args) => cli_stages::ort_placed(args),
        Command::Scorecard(args) => scorecard(args),
        Command::FleursPairs(args) => {
            fleurs::run(&args.targets, args.n, args.seed, &args.out, args.degrade).map(|written| {
                for one in written {
                    println!("{}\t{}", one.lang, one.pairs);
                }
            })
        }
    }
}

fn scorecard(args: ScorecardArgs) -> Result<(), EvalError> {
    if args.engine != "hayamimi" {
        return Err(EvalError::Invalid(format!(
            "unknown engine `{}`; only hayamimi is built in",
            args.engine
        )));
    }
    // The C library is delay-loaded; the copy beside the executable must be the first ONNX Runtime in the process.
    #[cfg(windows)]
    hayamimi_gpu::runtime::preload_beside_executable("onnxruntime.dll")
        .map_err(|e| EvalError::Invalid(e.to_string()))?;
    let outcome = scorecard_run::run(&scorecard_run::Options {
        models: args.models,
        sets: args.sets,
        max_resident: std::num::NonZeroUsize::new(args.max_resident),
    })?;
    scorecard::write_json(&args.out, &outcome.card)?;
    for clip in &outcome.clips {
        println!(
            "{}\ttier={}\terr={}\trtf={}\t{}",
            clip.name, clip.tier, clip.err_micros, clip.rtf_micros, clip.text
        );
    }
    for row in &outcome.card.rows {
        println!(
            "{}\t{}\t{}\t{}\t{}",
            row.lang, row.clips, row.tiers, row.mean_err_micros, row.mean_rtf_micros
        );
    }
    Ok(())
}

fn fetch_set(args: FetchSetArgs) -> Result<(), EvalError> {
    if let Some(input) = args.pin {
        let rows = manifest::pin(&input, &args.out)?;
        println!("pinned {rows} rows");
        return Ok(());
    }
    if args.concat {
        let Some(wav_out) = args.wav_out else {
            return Err(EvalError::Invalid(String::from("--concat needs --wav-out")));
        };
        let parts = args
            .part
            .iter()
            .map(|spec| manifest::parse_part(spec))
            .collect::<Result<Vec<_>, _>>()?;
        let joined = manifest::concat(&parts, args.gap_ms, &args.out, &wav_out)?;
        println!(
            "joined {} utterances, {} samples",
            joined.utterances, joined.samples
        );
        return Ok(());
    }
    Err(EvalError::Invalid(String::from(
        "fetch-set needs --pin <manifest> or --concat",
    )))
}

/// Parses the arguments, runs the sub command and returns the exit code.
pub fn run() -> ExitCode {
    let cli = Cli::parse();
    match execute(cli.command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            log::emit(
                hayamimi_relay::logging::Level::Error,
                "eval.command.failed",
                "worker",
                "the command failed",
                &[],
            );
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn the_command_line_definition_is_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn the_default_manifest_sits_in_a_manifests_directory_beside_the_wav() -> Result<(), EvalError>
    {
        assert_eq!(
            default_manifest(Path::new("testdata/lat-ja.wav"))?,
            PathBuf::from("testdata/manifests/lat-ja.json")
        );
        Ok(())
    }

    #[test]
    fn a_wav_in_the_current_directory_has_its_manifest_in_manifests() -> Result<(), EvalError> {
        assert_eq!(
            default_manifest(Path::new("lat-ja.wav"))?,
            PathBuf::from("manifests/lat-ja.json")
        );
        Ok(())
    }

    #[test]
    fn fetch_set_without_a_mode_is_refused() {
        let args = FetchSetArgs {
            pin: None,
            concat: false,
            out: PathBuf::from("x.json"),
            wav_out: None,
            gap_ms: 0,
            part: Vec::new(),
        };
        assert!(matches!(fetch_set(args), Err(EvalError::Invalid(_))));
    }
}

//! RP-6, the two cases that run the binary (T-RP-6-012 and T-RP-6-013): the
//! inference ran on the GPU where the stage table says so, and with the GPU
//! hidden nothing ran and nothing held GPU memory. The cases that read
//! fixtures and the results document are in `hayamimi-eval`'s `rp06.rs`.

mod support;

use std::io::Read;
use std::path::Path;
use std::process::{Child, Command};
use std::thread::JoinHandle;
use std::time::Duration;

use hayamimi_eval::gpu_nodes::{Placed, check_gpu_model_nodes, stage_of_model};
use hayamimi_eval::gpu_results::Results;
use hayamimi_eval::gpu_sessions::{Created, RunError, check_compute_apps, check_session_providers};
use hayamimi_eval::stage_table::{StageRow, parse_stage_table};
use serde_json::Value;
use support::{
    DESIGN, Failure, HIDE_GPU, Output, RESULTS, TestResult, clip, events, models_dir, named, read,
    spawn,
};

const QUERY: [&str; 2] = [
    "--query-compute-apps=process_name,used_memory",
    "--format=csv",
];
/// The process the compute-apps table must name.
const PROCESS: &str = "hayamimi";

/// A run of the binary and the compute-apps tables read while it ran.
struct Sampled {
    output: Output,
    samples: Vec<String>,
}

fn collect(mut pipe: impl Read + Send + 'static) -> JoinHandle<String> {
    std::thread::spawn(move || {
        let mut text = String::new();
        // A pipe that breaks early only shortens the text, which the checks then judge.
        let _ = pipe.read_to_string(&mut text);
        text
    })
}

fn compute_apps() -> Result<String, Failure> {
    let output = Command::new("nvidia-smi")
        .args(QUERY)
        .output()
        .map_err(|error| format!("cannot run nvidia-smi: {error}"))?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Runs the binary and reads the compute-apps table once a second until it ends.
/// The table is read at least once, right after the start.
fn run_sampled(args: &[&str], extra: &[(&str, &str)]) -> Result<Sampled, Failure> {
    let mut child: Child = spawn(args, None, extra)?;
    let stdout = child.stdout.take().ok_or("no stdout pipe")?;
    let stderr = child.stderr.take().ok_or("no stderr pipe")?;
    let (out, err) = (collect(stdout), collect(stderr));
    let mut samples = Vec::new();
    let status = loop {
        samples.push(compute_apps()?);
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            break status;
        }
        std::thread::sleep(Duration::from_secs(1));
    };
    let join = |handle: JoinHandle<String>| {
        handle
            .join()
            .map_err(|_| String::from("a reader thread panicked"))
    };
    Ok(Sampled {
        output: Output {
            code: status.code(),
            stdout: join(out)?,
            stderr: join(err)?,
        },
        samples,
    })
}

fn text_of(event: &Value, key: &str) -> Result<String, Failure> {
    event
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("an event has no string `{key}`"))
}

fn count_of(event: &Value, key: &str) -> Result<u64, Failure> {
    event
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("an event has no count `{key}`"))
}

fn created_of(events: &[Value]) -> Result<Vec<Created>, Failure> {
    named(events, "gpu.session.created")
        .into_iter()
        .map(|event| {
            Ok(Created {
                model: text_of(event, "model")?,
                provider: text_of(event, "actual_provider")?,
            })
        })
        .collect()
}

fn placed_of(events: &[Value]) -> Result<Vec<Placed>, Failure> {
    named(events, "gpu.session.placed")
        .into_iter()
        .map(|event| {
            Ok(Placed {
                model: text_of(event, "model")?,
                cpu_nodes: count_of(event, "cpu_nodes")?,
            })
        })
        .collect()
}

/// The rows of the stage table for the stages that built a model in this run.
/// The language ID stage has no model before the slice that adds it.
fn rows_in_use(table: &[StageRow], created: &[Created]) -> Vec<StageRow> {
    let mut rows: Vec<StageRow> = Vec::new();
    for session in created {
        if let Some(row) = stage_of_model(table, &session.model)
            && !rows.iter().any(|known| known.name == row.name)
        {
            rows.push(row.clone());
        }
    }
    rows
}

/// A run with the GPU visible: the process held GPU memory at some second, every
/// session got the provider of its stage, and the CPU nodes of the GPU models
/// are within the recorded count.
fn check_gpu_inference(run: &Sampled) -> Result<(), Failure> {
    if run.output.code != Some(0) {
        return Err(format!(
            "the run ended with {:?}\n{}",
            run.output.code, run.output.stderr
        ));
    }
    let mut last = Err(RunError::NoProcess(String::from(PROCESS)));
    for sample in &run.samples {
        last = check_compute_apps(sample, PROCESS);
        if last.is_ok() {
            break;
        }
    }
    last.map_err(|error| {
        format!(
            "{error}; {} compute-apps tables were read",
            run.samples.len()
        )
    })?;
    let all = events(&run.output.stdout)?;
    let table = parse_stage_table(&read(Path::new(DESIGN))?).map_err(|error| error.to_string())?;
    let created = created_of(&all)?;
    check_session_providers(&table, &created).map_err(|error| error.to_string())?;
    let results = Results::parse(&read(Path::new(RESULTS))?).map_err(|error| error.to_string())?;
    let Some(nodes) = results.nodes else {
        return Err(String::from("gpu_stages.md has no node table"));
    };
    check_gpu_model_nodes(&rows_in_use(&table, &created), &nodes, &placed_of(&all)?)
        .map_err(|error| error.to_string())
}

/// A run with the GPU hidden: no table names the process, and one
/// `gpu.device.unavailable` event came out.
fn check_hidden_gpu(run: &Sampled) -> Result<(), Failure> {
    for sample in &run.samples {
        if check_compute_apps(sample, PROCESS).is_ok() {
            return Err(String::from(
                "the process held GPU memory with the GPU hidden",
            ));
        }
    }
    let all = events(&run.output.stdout)?;
    let unavailable = named(&all, "gpu.device.unavailable").len();
    if unavailable != 1 {
        return Err(format!(
            "{unavailable} `gpu.device.unavailable` events, want 1"
        ));
    }
    Ok(())
}

fn acceptance_args(models: &str, wav: &str) -> Vec<String> {
    [
        "--wav",
        wav,
        "--no-realtime",
        "--mode",
        "single",
        "--lang",
        "ja",
        "--gpu-diagnose",
        "--models",
        models,
    ]
    .iter()
    .map(|arg| (*arg).to_owned())
    .collect()
}

fn sampled_acceptance(extra: &[(&str, &str)]) -> Result<Sampled, Failure> {
    let models = models_dir()?;
    let models = models.to_str().ok_or("the models path is not UTF-8")?;
    let wav = clip();
    let wav = wav.to_str().ok_or("the clip path is not UTF-8")?;
    let args = acceptance_args(models, wav);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    run_sampled(&args, extra)
}

/// T-RP-6-012
#[test]
#[ignore = "runs the models on the GPU"]
fn wav_run_uses_cuda_and_appears_in_compute_apps() -> TestResult {
    check_gpu_inference(&sampled_acceptance(&[])?)?;
    Ok(())
}

/// T-RP-6-013
#[test]
#[ignore = "runs the hayamimi binary"]
fn hidden_gpu_run_shows_no_compute_app_and_one_unavailable_event() -> TestResult {
    check_hidden_gpu(&sampled_acceptance(&[HIDE_GPU])?)?;
    Ok(())
}

/// The acceptance test of RP-6: the visible run is on the GPU, the hidden run is not.
#[test]
#[ignore = "runs the models on the GPU"]
fn rp06_inference_runs_on_cuda() -> TestResult {
    check_gpu_inference(&sampled_acceptance(&[])?)?;
    check_hidden_gpu(&sampled_acceptance(&[HIDE_GPU])?)?;
    Ok(())
}

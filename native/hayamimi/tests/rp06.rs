//! RP-6, the two cases that run the binary (T-RP-6-012 and T-RP-6-013): the
//! inference ran on the GPU where the stage table says so, and with the GPU
//! hidden the memory in use on the GPU did not rise. The cases that read
//! fixtures and the results document are in `hayamimi-eval`'s `rp06.rs`.

mod support;

use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, ExitStatus};
use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::Duration;

use hayamimi_eval::gpu_memory::{check_memory_flat, check_memory_rise};
use hayamimi_eval::gpu_nodes::{Placed, check_gpu_model_nodes, stage_of_model};
use hayamimi_eval::gpu_results::Results;
use hayamimi_eval::gpu_sessions::{Created, check_session_providers};
use hayamimi_eval::stage_table::{StageRow, parse_stage_table};
use serde_json::Value;
use support::{
    DESIGN, Failure, HIDE_GPU, Output, RESULTS, TestResult, clip, events, models_dir, named, read,
    spawn,
};

/// One integer in MiB per call: the memory in use on the whole GPU.
const QUERY: [&str; 2] = ["--query-gpu=memory.used", "--format=csv,noheader,nounits"];
/// Reads taken before the start, to take the baseline from.
const BEFORE_READS: usize = 3;
/// Seconds after the end during which the memory is read for its return.
const AFTER_SECONDS: usize = 10;

/// A run of the binary and the memory reads taken before, during and after it.
struct Sampled {
    output: Output,
    before: Vec<String>,
    during: Vec<String>,
    after: Vec<String>,
}

fn collect(mut pipe: impl Read + Send + 'static) -> JoinHandle<String> {
    std::thread::spawn(move || {
        let mut text = String::new();
        // A pipe that breaks early only shortens the text, which the checks then judge.
        let _ = pipe.read_to_string(&mut text);
        text
    })
}

/// One read of the memory in use on the GPU, as `nvidia-smi` printed it.
fn memory_used() -> Result<String, Failure> {
    let output = Command::new("nvidia-smi")
        .args(QUERY)
        .output()
        .map_err(|error| format!("cannot run nvidia-smi: {error}"))?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// `count` reads, one second apart.
fn read_every_second(count: usize) -> Result<Vec<String>, Failure> {
    let mut reads = Vec::with_capacity(count);
    for _ in 0..count {
        reads.push(memory_used()?);
        std::thread::sleep(Duration::from_secs(1));
    }
    Ok(reads)
}

/// Reads without a pause until the child ends (a call takes about 0.1 s). The
/// memory is read at least once. Once a second misses the peak: the GPU holds
/// its most for about half a second of a four-second run.
fn read_until_exit(child: &mut Child) -> Result<(Vec<String>, ExitStatus), Failure> {
    let mut during = Vec::new();
    loop {
        during.push(memory_used()?);
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            return Ok((during, status));
        }
    }
}

/// The memory of the whole GPU is the measure, so two sampled runs must not
/// overlap: the tests of this file run on several threads.
static ONE_RUN_AT_A_TIME: Mutex<()> = Mutex::new(());

/// Reads the memory three times, runs the binary reading it as fast as the tool
/// answers, then reads it once a second for ten more seconds.
fn run_sampled(args: &[&str], extra: &[(&str, &str)]) -> Result<Sampled, Failure> {
    // A thread that panicked while holding the lock left nothing to repair.
    let _alone = match ONE_RUN_AT_A_TIME.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    let before = read_every_second(BEFORE_READS)?;
    let mut child: Child = spawn(args, None, extra)?;
    let stdout = child.stdout.take().ok_or("no stdout pipe")?;
    let stderr = child.stderr.take().ok_or("no stderr pipe")?;
    let (out, err) = (collect(stdout), collect(stderr));
    let (during, status) = read_until_exit(&mut child)?;
    let after = read_every_second(AFTER_SECONDS)?;
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
        before,
        during,
        after,
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

/// A run with the GPU visible: the memory in use rose and returned, every
/// session got the provider of its stage, and the CPU nodes of the GPU models
/// are within the recorded count.
fn check_gpu_inference(run: &Sampled) -> Result<(), Failure> {
    if run.output.code != Some(0) {
        return Err(format!(
            "the run ended with {:?}\n{}",
            run.output.code, run.output.stderr
        ));
    }
    check_memory_rise(&run.before, &run.during, &run.after).map_err(|error| {
        format!(
            "{error}; reads before {:?}, during {:?}, after {:?}",
            run.before, run.during, run.after
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

/// A run with the GPU hidden: the memory in use stayed at the baseline, and one
/// `gpu.device.unavailable` event came out.
fn check_hidden_gpu(run: &Sampled) -> Result<(), Failure> {
    check_memory_flat(&run.before, &run.during).map_err(|error| {
        format!(
            "{error}; reads before {:?}, during {:?}",
            run.before, run.during
        )
    })?;
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

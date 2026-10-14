//! RP-22, the cases that run the binary (T-RP-22-007 to T-RP-22-011): the
//! memory in use on the GPU stays within the budget while a long recording is
//! played in real time, a budget that is reached moves a stage to the CPU and
//! does not stop the run, and the recognizers kept in memory stay within the
//! cap. The cases that read fixtures are in `hayamimi-eval`'s `rp22.rs`.
//!
//! The recording is `testdata/lat-multi.wav`, joined by `hayamimi-eval
//! fetch-set --concat` from the Japanese and English clips of `eval_real`, the
//! Mandarin and Korean clips of `eval_real_zhko` and the Cantonese clips of
//! `eval_real_yue`. The memory read is the whole GPU, so a language model
//! server running beside the program counts against the budget.

mod support;

use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::Duration;

use hayamimi_eval::vram::{BUDGET_MIB, check_vram, parse_readings};
use serde_json::Value;
use support::{Failure, Served, TestResult, events, find_above, models_dir, named, serve};

/// One reading per call: the memory in use on the whole GPU, with its header.
const QUERY: [&str; 2] = ["--query-gpu=memory.used", "--format=csv"];
/// Readings taken before the start, one second apart.
const BEFORE_READS: usize = 3;
/// A recognizer not in the Japanese tier, which is always in memory.
const ALWAYS_RESIDENT: &str = "rz";

/// Runs write the memory of the whole GPU, so two of them must not overlap.
static ONE_RUN_AT_A_TIME: Mutex<()> = Mutex::new(());

/// A run of the binary and the memory readings taken before and during it.
struct Measured {
    served: Served,
    before: Vec<u64>,
    during: Vec<u64>,
    /// The budget the run was started with, in MiB.
    budget_mib: u64,
}

/// Where the budget of a run comes from.
#[derive(Clone, Copy)]
enum Plan {
    /// The program's own budget.
    Default,
    /// One MiB over the largest reading taken before this run started. The
    /// whole GPU is read, and the other processes on it move the figure from
    /// one run to the next, so a budget taken from another run's peak may sit
    /// above everything this run reaches. A budget taken from this run's own
    /// start is below what the loaded program holds, whatever the others do.
    OverTheStart,
}

/// The budget of a run in MiB and the arguments that set it.
fn plan_budget(plan: Plan, before: &[u64]) -> Result<(u64, Vec<String>), Failure> {
    match plan {
        Plan::Default => Ok((BUDGET_MIB, Vec::new())),
        Plan::OverTheStart => {
            let Some(start) = before.iter().copied().max() else {
                return Err(String::from("no memory was read before the start"));
            };
            let budget = start + 1;
            let args = vec![String::from("--gpu-budget"), budget.to_string()];
            Ok((budget, args))
        }
    }
}

/// One call of `nvidia-smi`.
fn read_once() -> Result<Vec<u64>, Failure> {
    let output = Command::new("nvidia-smi")
        .args(QUERY)
        .output()
        .map_err(|error| format!("cannot run nvidia-smi: {error}"))?;
    parse_readings(&String::from_utf8_lossy(&output.stdout)).map_err(|error| error.to_string())
}

fn read_before() -> Result<Vec<u64>, Failure> {
    let mut before = Vec::new();
    for _ in 0..BEFORE_READS {
        before.extend(read_once()?);
        std::thread::sleep(Duration::from_secs(1));
    }
    Ok(before)
}

/// Reads without a pause until told to stop; a call takes about 0.1 s.
fn sample_until(stop: Arc<AtomicBool>) -> JoinHandle<Result<Vec<u64>, Failure>> {
    std::thread::spawn(move || {
        let mut during = Vec::new();
        while !stop.load(Ordering::Relaxed) {
            during.extend(read_once()?);
        }
        Ok(during)
    })
}

/// Plays `lat-multi.wav` in real time with the subtitle server on, reading the memory throughout.
fn measured_run(plan: Plan) -> Result<Measured, Failure> {
    // A thread that panicked while holding the lock left nothing to repair.
    let _alone = match ONE_RUN_AT_A_TIME.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    let wav = find_above("testdata/lat-multi.wav")?;
    let models = models_dir()?;
    let (wav, models) = (path_text(&wav)?, path_text(&models)?);
    let before = read_before()?;
    let (budget_mib, budget_args) = plan_budget(plan, &before)?;
    let stop = Arc::new(AtomicBool::new(false));
    let sampler = sample_until(Arc::clone(&stop));
    let mut args = vec!["--wav", wav.as_str(), "--models", models.as_str()];
    args.extend(budget_args.iter().map(String::as_str));
    let served = serve(&args, None, &[]);
    stop.store(true, Ordering::Relaxed);
    let during = sampler
        .join()
        .map_err(|_| String::from("the sampler panicked"))??;
    Ok(Measured {
        served: served?,
        before,
        during,
        budget_mib,
    })
}

fn path_text(path: &std::path::Path) -> Result<String, Failure> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("{} is not UTF-8", path.display()))
}

/// The run with the default budget, made once for the cases that need it.
fn default_run() -> Result<&'static Measured, Failure> {
    static RUN: OnceLock<Result<Measured, Failure>> = OnceLock::new();
    RUN.get_or_init(|| measured_run(Plan::Default))
        .as_ref()
        .map_err(Clone::clone)
}

/// The `gpu.fallback.cpu` events of stdout whose reason is the memory budget.
fn memory_fallbacks(stdout: &str) -> Result<usize, Failure> {
    Ok(named(&events(stdout)?, "gpu.fallback.cpu")
        .into_iter()
        .filter(|event| event.get("reason").and_then(Value::as_str) == Some("memory_budget"))
        .count())
}

/// The messages of the subtitle stream of this `type`; the memory budget ones when `reason` is given.
fn wire_count(wire: &[Value], kind: &str, reason: Option<&str>) -> usize {
    wire.iter()
        .filter(|message| message.get("type").and_then(Value::as_str) == Some(kind))
        .filter(|message| {
            reason.is_none_or(|want| message.get("reason").and_then(Value::as_str) == Some(want))
        })
        .count()
}

/// Where the figures of the default run are kept.
const RECORD: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/results/r3/rp22_run.json"
);

/// Keeps the figures of the default run, whether or not they pass.
fn record_run(run: &Measured) -> Result<(), Failure> {
    let Some(peak) = run.during.iter().copied().max() else {
        return Err(String::from("the run read no memory"));
    };
    let record = serde_json::json!({
        "before_mib": run.before,
        "peak_mib": peak,
        "readings": run.during.len(),
        "budget_mib": BUDGET_MIB,
        "memory_fallbacks": memory_fallbacks(&run.served.output.stdout)?,
    });
    let text = serde_json::to_string_pretty(&record).map_err(|error| error.to_string())?;
    std::fs::write(RECORD, text + "\n").map_err(|error| format!("cannot write {RECORD}: {error}"))
}

/// T-RP-22-007
#[test]
#[ignore = "plays nine minutes of audio in real time on the GPU"]
fn vram_stays_under_budget_during_the_run() -> TestResult {
    let run = default_run()?;
    record_run(run)?;
    assert_eq!(run.served.output.code, Some(0));
    let peak = check_vram(&run.before, &run.during, BUDGET_MIB)?;
    println!(
        "rp22: peak {peak} MiB over {} readings, {} before the start",
        run.during.len(),
        run.before.len()
    );
    Ok(())
}

/// T-RP-22-009
#[test]
#[ignore = "plays nine minutes of audio in real time on the GPU"]
fn budget_not_reached_emits_no_memory_fallback() -> TestResult {
    let run = default_run()?;
    assert_eq!(run.served.output.code, Some(0));
    assert_eq!(memory_fallbacks(&run.served.output.stdout)?, 0);
    assert_eq!(
        wire_count(&run.served.wire, "fallback", Some("memory_budget")),
        0
    );
    // The control: the run did recognise speech, so the zero is not an empty run.
    let finals = wire_count(&run.served.wire, "final", None);
    let timed = named(&events(&run.served.output.stdout)?, "asr.utterance.timed").len();
    assert!(
        finals > 0,
        "no final on /events ({} messages); the log has {timed} utterances",
        run.served.wire.len()
    );
    Ok(())
}

/// T-RP-22-008
#[test]
#[ignore = "plays nine minutes of audio in real time on the GPU"]
fn budget_reached_falls_back_instead_of_stopping() -> TestResult {
    let run = measured_run(Plan::OverTheStart)?;
    let budget = run.budget_mib;
    assert_eq!(run.served.output.code, Some(0), "the run stopped");
    assert!(
        wire_count(&run.served.wire, "final", None) > 0,
        "no final came out"
    );
    let said = memory_fallbacks(&run.served.output.stdout)?;
    assert!(said >= 1, "the budget of {budget} MiB moved no stage");
    let sent = wire_count(&run.served.wire, "fallback", Some("memory_budget"));
    assert_eq!(said, sent, "the log and /events disagree");
    // The comparison is shown able to fail: a reading of this run reached past the budget.
    let compared = check_vram(&run.before, &run.during, budget);
    assert!(compared.is_err(), "{compared:?}");
    println!("rp22: budget {budget} MiB moved {said} stage(s); {compared:?}");
    Ok(())
}

/// The `asr.model.loaded` events of stdout other than the Japanese tier.
fn loaded_beyond_japanese(all: &[Value]) -> Vec<String> {
    named(all, "asr.model.loaded")
        .into_iter()
        .filter_map(|event| event.get("model").and_then(Value::as_str))
        .filter(|model| *model != ALWAYS_RESIDENT)
        .map(str::to_owned)
        .collect()
}

/// The most recognizers beyond the Japanese tier in memory at once, replaying the events in order.
fn most_resident(all: &[Value]) -> usize {
    let (mut now, mut most) = (0_usize, 0_usize);
    for event in all {
        let name = event.get("event").and_then(Value::as_str);
        let japanese = event.get("model").and_then(Value::as_str) == Some(ALWAYS_RESIDENT);
        match (name, japanese) {
            (Some("asr.model.loaded"), false) => now += 1,
            (Some("asr.model.evicted"), false) => now = now.saturating_sub(1),
            _ => {}
        }
        most = most.max(now);
    }
    most
}

/// Plays `lat-multi.wav` as fast as the models go with `--max-resident <cap>`.
fn run_with_cap(cap: &str) -> Result<Vec<Value>, Failure> {
    let wav = find_above("testdata/lat-multi.wav")?;
    let models = models_dir()?;
    let (wav, models) = (path_text(&wav)?, path_text(&models)?);
    let output = support::run(
        &[
            "--wav",
            &wav,
            "--models",
            &models,
            "--no-realtime",
            "--max-resident",
            cap,
        ],
        None,
        &[],
    )?;
    assert_eq!(output.code, Some(0), "{}", output.stderr);
    events(&output.stdout)
}

/// T-RP-22-010
#[test]
#[ignore = "plays lat-multi through the binary as fast as the models go"]
fn least_recently_used_recognizer_is_dropped_at_the_cap() -> TestResult {
    let all = run_with_cap("2")?;
    assert!(
        loaded_beyond_japanese(&all).len() >= 3,
        "the five languages used fewer than three recognizers beyond the Japanese one"
    );
    assert!(most_resident(&all) <= 2);
    assert!(!named(&all, "asr.model.evicted").is_empty());
    Ok(())
}

/// T-RP-22-011
#[test]
#[ignore = "plays lat-multi through the binary as fast as the models go"]
fn default_cap_drops_nothing_for_five_languages() -> TestResult {
    let all = run_with_cap("3")?;
    // Without this the zero below would hold for a run that never used the recognizers.
    assert!(
        loaded_beyond_japanese(&all).len() >= 3,
        "the five languages used fewer than three recognizers beyond the Japanese one"
    );
    assert!(named(&all, "asr.model.evicted").is_empty());
    Ok(())
}

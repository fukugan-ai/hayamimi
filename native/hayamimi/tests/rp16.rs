//! RP-16: a machine without a GPU runs on, stage by stage on the CPU, and
//! says so. The run ends with code 0 and subtitles come out; the GPU is
//! reported missing once; every GPU stage the run loaded is reported moved to
//! the CPU once in the log and once on `/events`; every session ran on the
//! CPU; and the user is told, in the user's language, how to check the GPU.
//! With a GPU present none of that is said and the stage runs on CUDA.

mod support;

use serde_json::Value;
use support::{
    Failure, HIDE_GPU, Output, Served, TestResult, clip, events, models_dir, named, serve,
};

const UNAVAILABLE: &str = "gpu.device.unavailable";
const FALLBACK: &str = "gpu.fallback.cpu";
const CREATED: &str = "gpu.session.created";
const ERROR_CODE: &str = "HAYAMIMI_GPU_DEVICE_UNAVAILABLE";

/// A GPU stage of the design table and the model it builds.
type GpuStage = (&'static str, &'static str);

/// The GPU stages the Japanese route loads: the punctuation model.
const LOADED: [GpuStage; 1] = [("punct", "punct_bert.onnx")];

/// A run, and the GPU stages it loaded.
struct Run {
    served: Served,
    loaded: Vec<GpuStage>,
}

fn text_of<'a>(event: &'a Value, key: &str) -> Option<&'a str> {
    event.get(key).and_then(Value::as_str)
}

/// The log events of `name` that are about `stage`.
fn log_events_of<'a>(all: &'a [Value], name: &str, stage: &str) -> Vec<&'a Value> {
    named(all, name)
        .into_iter()
        .filter(|event| text_of(event, "stage") == Some(stage))
        .collect()
}

/// The `/events` fallbacks about `stage` that say the stage went to `to`.
fn wire_fallbacks_of<'a>(wire: &'a [Value], stage: &str, to: &str) -> Vec<&'a Value> {
    wire.iter()
        .filter(|event| text_of(event, "type") == Some("fallback"))
        .filter(|event| text_of(event, "stage") == Some(stage))
        .filter(|event| text_of(event, "to") == Some(to))
        .collect()
}

fn exactly_once(found: usize, what: &str, stage: &str) -> Result<(), Failure> {
    if found == 1 {
        Ok(())
    } else {
        Err(format!("{found} {what} for the {stage} stage, want 1"))
    }
}

fn check_exit_zero(output: &Output) -> Result<(), Failure> {
    if output.code == Some(0) {
        Ok(())
    } else {
        Err(format!(
            "the run ended with {:?}, want 0\n{}",
            output.code, output.stderr
        ))
    }
}

fn check_subtitles(wire: &[Value]) -> Result<(), Failure> {
    let finals = wire
        .iter()
        .filter(|event| text_of(event, "type") == Some("final"))
        .count();
    if finals == 0 {
        return Err(String::from("no `final` event came out on /events"));
    }
    Ok(())
}

/// One `gpu.device.unavailable` event, with its error code and the reason `no_device`.
fn check_unavailable_once(all: &[Value]) -> Result<(), Failure> {
    let found = named(all, UNAVAILABLE);
    if found.len() != 1 {
        return Err(format!("{} `{UNAVAILABLE}` events, want 1", found.len()));
    }
    if text_of(found[0], "error_code") != Some(ERROR_CODE) {
        return Err(format!("error_code of `{UNAVAILABLE}` is not {ERROR_CODE}"));
    }
    if text_of(found[0], "reason") != Some("no_device") {
        return Err(format!("reason of `{UNAVAILABLE}` is not no_device"));
    }
    Ok(())
}

/// Each loaded GPU stage moved to the CPU once in the log and once on `/events`.
fn check_stages_moved(all: &[Value], wire: &[Value], loaded: &[GpuStage]) -> Result<(), Failure> {
    for (stage, _) in loaded {
        let logged = log_events_of(all, FALLBACK, stage).len();
        exactly_once(logged, &format!("`{FALLBACK}` events"), stage)?;
        let sent = wire_fallbacks_of(wire, stage, "cpu").len();
        exactly_once(sent, "`fallback` events on /events", stage)?;
    }
    Ok(())
}

/// The provider each created session actually ran on, by model.
fn providers(all: &[Value]) -> Vec<(&str, &str)> {
    named(all, CREATED)
        .into_iter()
        .filter_map(|event| Some((text_of(event, "model")?, text_of(event, "actual_provider")?)))
        .collect()
}

fn check_every_session_on_cpu(all: &[Value]) -> Result<(), Failure> {
    match providers(all)
        .iter()
        .find(|(_, provider)| *provider != "cpu")
    {
        Some((model, provider)) => Err(format!("{model} ran on {provider}, want cpu")),
        None => Ok(()),
    }
}

fn check_names_nvidia_smi(output: &Output) -> Result<(), Failure> {
    if output.stderr.contains("nvidia-smi") {
        Ok(())
    } else {
        Err(String::from("stderr does not name nvidia-smi"))
    }
}

/// A run with the GPU hidden: exit 0, subtitles, one device event, each loaded
/// stage moved to the CPU once, every session on the CPU, the check command named.
fn check_no_gpu_run(run: &Run) -> Result<(), Failure> {
    let output = &run.served.output;
    check_exit_zero(output)?;
    check_subtitles(&run.served.wire)?;
    let all = events(&output.stdout)?;
    check_unavailable_once(&all)?;
    check_stages_moved(&all, &run.served.wire, &run.loaded)?;
    check_every_session_on_cpu(&all)?;
    check_names_nvidia_smi(output)
}

/// Each loaded GPU stage built its model on CUDA.
fn check_stages_on_cuda(all: &[Value], loaded: &[GpuStage]) -> Result<(), Failure> {
    let built = providers(all);
    for (stage, model) in loaded {
        match built.iter().find(|(name, _)| name == model) {
            Some((_, "cuda")) => {}
            Some((_, other)) => return Err(format!("the {stage} stage ran on {other}, want cuda")),
            None => return Err(format!("no session was created for the {stage} stage")),
        }
    }
    Ok(())
}

/// A run with the GPU present: exit 0, subtitles, and not one word of fallback.
fn check_gpu_run(run: &Run) -> Result<(), Failure> {
    let output = &run.served.output;
    check_exit_zero(output)?;
    check_subtitles(&run.served.wire)?;
    let all = events(&output.stdout)?;
    for name in [UNAVAILABLE, FALLBACK] {
        let found = named(&all, name).len();
        if found != 0 {
            return Err(format!("{found} `{name}` events on a GPU machine"));
        }
    }
    if run
        .served
        .wire
        .iter()
        .any(|event| text_of(event, "type") == Some("fallback"))
    {
        return Err(String::from(
            "a `fallback` event on /events on a GPU machine",
        ));
    }
    check_stages_on_cuda(&all, &run.loaded)
}

/// The stderr of the two locales are in different languages: the Japanese one
/// holds non-ASCII text, the other is ASCII, and they differ.
fn check_stderr_languages(ja: &str, c: &str) -> Result<(), Failure> {
    if ja == c {
        return Err(String::from("stderr is the same in both locales"));
    }
    if ja.is_ascii() {
        return Err(String::from("the Japanese stderr is ASCII"));
    }
    if !c.is_ascii() {
        return Err(String::from("the C.UTF-8 stderr is not ASCII"));
    }
    Ok(())
}

// ---- fixtures ----

fn log_line(name: &str, fields: &str) -> String {
    format!("{{\"event\":\"{name}\"{fields}}}\n")
}

fn unavailable_line() -> String {
    log_line(
        UNAVAILABLE,
        &format!(",\"error_code\":\"{ERROR_CODE}\",\"reason\":\"no_device\""),
    )
}

fn fallback_line(stage: &str) -> String {
    log_line(
        FALLBACK,
        &format!(",\"stage\":\"{stage}\",\"reason\":\"no_device\""),
    )
}

fn created_line(model: &str, provider: &str) -> String {
    log_line(
        CREATED,
        &format!(",\"model\":\"{model}\",\"actual_provider\":\"{provider}\""),
    )
}

fn wire_fallback(stage: &str) -> Value {
    serde_json::json!({"type": "fallback", "stage": stage, "reason": "no_device", "to": "cpu"})
}

fn wire_final() -> Value {
    serde_json::json!({"type": "final", "text": "x"})
}

fn run_of(code: i32, stdout: &str, stderr: &str, wire: Vec<Value>) -> Run {
    Run {
        served: Served {
            output: Output {
                code: Some(code),
                stdout: stdout.to_owned(),
                stderr: stderr.to_owned(),
            },
            wire,
        },
        loaded: LOADED.to_vec(),
    }
}

/// The log of T-RP-16-001: the device event, a CPU session, the stage moved.
fn hidden_stdout() -> String {
    unavailable_line() + &created_line("punct_bert.onnx", "cpu") + &fallback_line("punct")
}

fn hidden_wire() -> Vec<Value> {
    vec![wire_final(), wire_fallback("punct")]
}

fn hidden_run() -> Run {
    run_of(
        0,
        &hidden_stdout(),
        "check with: nvidia-smi\n",
        hidden_wire(),
    )
}

fn visible_run(stdout: &str, wire: Vec<Value>) -> Run {
    run_of(0, stdout, "", wire)
}

/// Whether the check failed with a message that names `what`.
fn names(said: Result<(), Failure>, what: &str) -> bool {
    said.is_err_and(|text| text.contains(what))
}

// ---- cases ----

/// T-RP-16-001
#[test]
fn no_gpu_run_with_all_required_signals_passes() -> TestResult {
    check_no_gpu_run(&hidden_run())?;
    Ok(())
}

/// T-RP-16-002
#[test]
fn zero_unavailable_events_is_rejected() {
    let stdout = created_line("punct_bert.onnx", "cpu") + &fallback_line("punct");
    let run = run_of(0, &stdout, "nvidia-smi\n", hidden_wire());
    assert!(names(check_no_gpu_run(&run), UNAVAILABLE));
}

/// T-RP-16-003
#[test]
fn two_unavailable_events_is_rejected() {
    let stdout = unavailable_line() + &hidden_stdout();
    let run = run_of(0, &stdout, "nvidia-smi\n", hidden_wire());
    assert!(names(check_no_gpu_run(&run), UNAVAILABLE));
}

/// T-RP-16-004
#[test]
fn cuda_session_after_missing_gpu_is_rejected() {
    let stdout = hidden_stdout() + &created_line("encoder.onnx", "cuda");
    let run = run_of(0, &stdout, "nvidia-smi\n", hidden_wire());
    assert!(names(check_no_gpu_run(&run), "encoder.onnx"));
}

/// T-RP-16-005
#[test]
fn nonzero_exit_code_is_rejected() {
    let run = run_of(2, &hidden_stdout(), "nvidia-smi\n", hidden_wire());
    assert!(names(check_no_gpu_run(&run), "want 0"));
}

/// T-RP-16-006
#[test]
fn stderr_without_nvidia_smi_is_rejected() {
    let run = run_of(0, &hidden_stdout(), "no device\n", hidden_wire());
    assert!(names(check_no_gpu_run(&run), "nvidia-smi"));
}

/// T-RP-16-007
#[test]
fn stderr_language_follows_the_locale() -> TestResult {
    check_stderr_languages("GPU を確かめる: nvidia-smi\n", "check with: nvidia-smi\n")?;
    Ok(())
}

/// T-RP-16-008
#[test]
fn identical_stderr_across_locales_is_rejected() {
    assert!(check_stderr_languages("same\n", "same\n").is_err());
}

/// T-RP-16-009
#[test]
fn gpu_present_run_has_no_fallback_events() -> TestResult {
    let stdout = created_line("punct_bert.onnx", "cuda");
    check_gpu_run(&visible_run(&stdout, vec![wire_final()]))?;
    Ok(())
}

/// T-RP-16-010
#[test]
fn gpu_present_run_that_falls_back_is_rejected() {
    let stdout = created_line("punct_bert.onnx", "cpu") + &fallback_line("punct");
    let run = visible_run(&stdout, vec![wire_final()]);
    assert!(names(check_gpu_run(&run), FALLBACK));
}

/// T-RP-16-011
#[test]
fn missing_fallback_log_event_for_a_loaded_stage_is_named() {
    let stdout = unavailable_line() + &created_line("punct_bert.onnx", "cpu");
    let run = run_of(0, &stdout, "nvidia-smi\n", hidden_wire());
    assert!(names(check_no_gpu_run(&run), "punct"));
}

/// T-RP-16-012
#[test]
fn missing_events_fallback_for_a_loaded_stage_is_named() {
    let run = run_of(0, &hidden_stdout(), "nvidia-smi\n", vec![wire_final()]);
    assert!(names(check_no_gpu_run(&run), "/events"));
}

/// T-RP-16-013
#[test]
fn duplicate_fallback_event_for_a_stage_is_rejected() {
    let stdout = hidden_stdout() + &fallback_line("punct");
    let run = run_of(0, &stdout, "nvidia-smi\n", hidden_wire());
    assert!(names(check_no_gpu_run(&run), "punct"));
}

/// T-RP-16-014
#[test]
fn unloaded_gpu_stage_needs_no_fallback_event() -> TestResult {
    // The language identification stage is not loaded by this run, so it owes no event.
    let run = hidden_run();
    let all = events(&run.served.output.stdout)?;
    assert!(log_events_of(&all, FALLBACK, "lid").is_empty());
    check_no_gpu_run(&run)?;
    Ok(())
}

// ---- real runs ----

fn real_run(locale: &str, extra: &[(&str, &str)]) -> Result<Run, Failure> {
    let models = models_dir()?;
    let models = models.to_str().ok_or("the models path is not UTF-8")?;
    let wav = clip();
    let wav = wav.to_str().ok_or("the clip path is not UTF-8")?;
    let args = [
        "--wav",
        wav,
        "--no-realtime",
        "--mode",
        "single",
        "--lang",
        "ja",
        "--models",
        models,
    ];
    Ok(Run {
        served: serve(&args, Some(locale), extra)?,
        loaded: LOADED.to_vec(),
    })
}

fn hidden_gpu_run(locale: &str) -> Result<Run, Failure> {
    real_run(locale, &[HIDE_GPU])
}

/// T-RP-16-015
#[test]
#[ignore = "runs the hayamimi binary"]
fn hidden_gpu_falls_back_to_cpu_with_a_reason() -> TestResult {
    check_no_gpu_run(&hidden_gpu_run("C.UTF-8")?)?;
    Ok(())
}

/// T-RP-16-016
#[test]
#[ignore = "runs the models on the GPU"]
fn visible_gpu_run_does_not_fall_back() -> TestResult {
    check_gpu_run(&real_run("C.UTF-8", &[])?)?;
    Ok(())
}

/// The acceptance test of RP-16: with the GPU hidden the run goes on and says
/// so, in both languages; with the GPU visible it does not fall back.
#[test]
#[ignore = "runs the hayamimi binary"]
fn rp16_no_gpu_falls_back_to_cpu_and_says_so() -> TestResult {
    let ja = hidden_gpu_run("ja_JP.UTF-8")?;
    let c = hidden_gpu_run("C.UTF-8")?;
    check_no_gpu_run(&ja)?;
    check_no_gpu_run(&c)?;
    // Windows takes the language from the system, which `LANG` does not change.
    #[cfg(unix)]
    check_stderr_languages(&ja.served.output.stderr, &c.served.output.stderr)?;
    check_gpu_run(&real_run("C.UTF-8", &[])?)?;
    Ok(())
}

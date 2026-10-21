//! RP-84, the start: a run of the program reads silence once with the
//! Japanese recognizer, and once the punctuation model, before it reads any
//! audio. Its stdout holds one `asr.warmup.decoded` (10 s of audio, a decode
//! time above zero, `punct_ms` a JSON number), after `asr.model.loaded` and
//! before the first `asr.endpoint.decided`, and no `asr.warmup.failed`.
//!
//! The judgement is checked on stdout written by hand. The real run is
//! `#[ignore]`: it starts the program on the CPU with the models and counts
//! the lines it printed.

mod support;

use serde_json::{Value, json};
use support::{Failure, HIDE_GPU, TestResult, clip, events, models_dir, named, path_text, run};

const LOADED: &str = "asr.model.loaded";
const DECODED: &str = "asr.warmup.decoded";
const FAILED: &str = "asr.warmup.failed";
const DECIDED: &str = "asr.endpoint.decided";

/// The line of the first event named `name`.
fn position(all: &[Value], name: &str) -> Option<usize> {
    all.iter()
        .position(|event| event.get("event").and_then(Value::as_str) == Some(name))
}

fn check_decoded_once(all: &[Value]) -> Result<&Value, Failure> {
    let decoded = named(all, DECODED);
    let [only] = decoded.as_slice() else {
        return Err(format!("{DECODED} is {} lines, not 1", decoded.len()));
    };
    Ok(*only)
}

fn check_fields(decoded: &Value) -> Result<(), Failure> {
    if decoded.get("audio_s").and_then(Value::as_u64) != Some(10) {
        return Err(format!("{DECODED} does not say 10 s of audio: {decoded}"));
    }
    if !decoded
        .get("decode_ms")
        .and_then(Value::as_f64)
        .is_some_and(|ms| ms > 0.0)
    {
        return Err(format!(
            "{DECODED} has no decode time above zero: {decoded}"
        ));
    }
    check_punct_ms(decoded)
}

/// The punctuation model was read at start-up: `punct_ms` is a JSON number,
/// whatever its value. A `null` or a missing field is a stage not warmed.
fn check_punct_ms(decoded: &Value) -> Result<(), Failure> {
    if decoded.get("punct_ms").is_some_and(Value::is_number) {
        return Ok(());
    }
    Err(format!(
        "{DECODED} has no numeric punct_ms, so the punctuation model was not read: {decoded}"
    ))
}

fn check_order(all: &[Value]) -> Result<(), Failure> {
    let (Some(loaded), Some(decoded), Some(decided)) = (
        position(all, LOADED),
        position(all, DECODED),
        position(all, DECIDED),
    ) else {
        return Err(format!(
            "stdout lacks {LOADED}, {DECODED} or {DECIDED}: the start-up decode has no place to stand in"
        ));
    };
    if loaded < decoded && decoded < decided {
        Ok(())
    } else {
        Err(format!(
            "{DECODED} is not between {LOADED} (line {loaded}) and the first {DECIDED} (line {decided})"
        ))
    }
}

/// The start-up decode ran once, in its place, and did not fail.
fn check_start(all: &[Value]) -> Result<(), Failure> {
    check_fields(check_decoded_once(all)?)?;
    check_order(all)?;
    let failed = named(all, FAILED).len();
    if failed > 0 {
        return Err(format!("{FAILED} is {failed} lines, not 0"));
    }
    Ok(())
}

fn decoded_line() -> Value {
    json!({"event": DECODED, "model": "rz", "decode_ms": 950.5, "audio_s": 10, "punct_ms": 448.445})
}

fn start_of(middle: Vec<Value>) -> Vec<Value> {
    let mut all = vec![json!({"event": LOADED, "model": "rz", "load_ms": 1000.0})];
    all.extend(middle);
    all.push(json!({"event": DECIDED, "id": 1}));
    all
}

#[test]
fn a_start_up_decode_between_the_load_and_the_first_decision_passes() -> TestResult {
    check_start(&start_of(vec![decoded_line()]))?;
    Ok(())
}

#[test]
fn a_start_without_the_start_up_decode_is_rejected() {
    let said = check_start(&start_of(Vec::new())).err();
    assert!(
        said.as_deref()
            .is_some_and(|text| text.contains("is 0 lines, not 1")),
        "{said:?}"
    );
}

#[test]
fn a_start_up_decode_after_the_first_decision_is_rejected() {
    let all = vec![
        json!({"event": LOADED, "model": "rz", "load_ms": 1000.0}),
        json!({"event": DECIDED, "id": 1}),
        decoded_line(),
    ];
    let said = check_start(&all).err();
    assert!(
        said.as_deref()
            .is_some_and(|text| text.contains("is not between")),
        "{said:?}"
    );
}

#[test]
fn a_failed_start_up_decode_is_rejected() {
    let failed = json!({"event": FAILED, "error_code": "HAYAMIMI_ASR_WARMUP_FAILED"});
    let said = check_start(&start_of(vec![decoded_line(), failed])).err();
    assert!(
        said.as_deref()
            .is_some_and(|text| text.contains("is 1 lines, not 0")),
        "{said:?}"
    );
}

#[test]
fn a_start_up_decode_that_read_less_than_ten_seconds_is_rejected() {
    let short = json!({"event": DECODED, "model": "rz", "decode_ms": 3.0, "audio_s": 1});
    let said = check_start(&start_of(vec![short])).err();
    assert!(
        said.as_deref()
            .is_some_and(|text| text.contains("10 s of audio")),
        "{said:?}"
    );
}

#[test]
fn a_start_up_decode_without_a_numeric_punct_ms_is_rejected() {
    let cases = [
        json!({"event": DECODED, "model": "rz", "decode_ms": 950.5, "audio_s": 10}),
        json!({"event": DECODED, "model": "rz", "decode_ms": 950.5, "audio_s": 10, "punct_ms": null}),
        json!({"event": DECODED, "model": "rz", "decode_ms": 950.5, "audio_s": 10, "punct_ms": "448.445"}),
    ];
    for decoded in cases {
        let said = check_start(&start_of(vec![decoded])).err();
        assert!(
            said.as_deref()
                .is_some_and(|text| text.contains("no numeric punct_ms")),
            "{said:?}"
        );
    }
}

#[test]
fn a_punct_ms_of_zero_is_a_number() -> TestResult {
    let zero =
        json!({"event": DECODED, "model": "rz", "decode_ms": 950.5, "audio_s": 10, "punct_ms": 0});
    check_start(&start_of(vec![zero]))?;
    Ok(())
}

/// The program on the clip of the acceptance runs, on the CPU: the lines it printed.
#[test]
#[ignore = "runs the hayamimi binary with the models"]
fn a_run_of_the_program_decodes_the_start_up_silence_once_before_any_utterance() -> TestResult {
    let models = path_text(&models_dir()?)?;
    let wav = path_text(&clip())?;
    let args = [
        "--wav",
        wav.as_str(),
        "--no-realtime",
        "--models",
        models.as_str(),
        "--lang",
        "ja",
    ];
    let done = run(&args, None, &[HIDE_GPU])?;
    assert_eq!(done.code, Some(0), "{}", done.stderr);
    let all = events(&done.stdout)?;
    println!(
        "{DECODED}: {} lines, {FAILED}: {} lines, {DECIDED}: {} lines",
        named(&all, DECODED).len(),
        named(&all, FAILED).len(),
        named(&all, DECIDED).len()
    );
    assert!(
        !named(&all, DECIDED).is_empty(),
        "the clip made no utterance"
    );
    check_start(&all)?;
    Ok(())
}

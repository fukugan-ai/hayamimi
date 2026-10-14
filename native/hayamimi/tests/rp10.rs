//! RP-10: it runs on Windows. After the server is up (`ready`), the
//! subtitles of English speech played through the loopback arrive on
//! `/events`: at least one `partial` and one `final` whose `lang` is `en`.
//! When the output device is missing the run does not stop: it says
//! `capture.fallback.single` once and the microphone alone makes the `final`s.
//!
//! `check_event_stream` and `check_capture_fallback` judge a finished run;
//! the fixture cases feed them hand-written runs, and the real runs (ignored)
//! feed them the hayamimi binary.
#![allow(dead_code)]

mod support;

#[path = "rp29/checks.rs"]
mod checks;
#[path = "rp29/fixtures.rs"]
mod fixtures;
#[path = "rp29/scenes.rs"]
mod scenes;

use fixtures::{fell, final_wire, names, scene_wire, served_of, whole};
use scenes::{CAPTURE_SINGLE, text_of};
use serde_json::{Value, json};
use support::{Failure, Served, TestResult};

fn kind(event: &Value) -> Option<&str> {
    text_of(event, "type")
}

/// The index of the first event of one of `kinds`.
fn first_of(events: &[Value], kinds: &[&str]) -> Option<usize> {
    events
        .iter()
        .position(|event| kind(event).is_some_and(|found| kinds.contains(&found)))
}

/// The `error` events of a run: there must be none.
fn check_no_error_event(events: &[Value]) -> Result<(), Failure> {
    match events.iter().find(|event| kind(event) == Some("error")) {
        Some(event) => Err(format!(
            "an `error` event came out: {}",
            text_of(event, "message").map_or_else(|| event.to_string(), str::to_owned)
        )),
        None => Ok(()),
    }
}

/// The finals of the stream come after the server is up, there is at least
/// one `partial` and one `final`, and every `final` is English.
pub fn check_event_stream(events: &[Value]) -> Result<(), Failure> {
    check_event_stream_in(events, Some("en"))
}

/// The same check with the language of the finals fixed by `lang`, or left
/// unchecked with `None` (a build whose English route is not in yet).
pub fn check_event_stream_in(events: &[Value], lang: Option<&str>) -> Result<(), Failure> {
    check_no_error_event(events)?;
    let up =
        first_of(events, &["ready", "session_start"]).ok_or("the server never sent `ready`")?;
    let first_final = first_of(events, &["final"]).ok_or("no `final` event came out")?;
    if first_final < up {
        return Err(String::from("a `final` came before `ready`"));
    }
    first_of(events, &["partial"]).ok_or("no `partial` event came out")?;
    for event in events.iter().filter(|event| kind(event) == Some("final")) {
        let found = text_of(event, "lang");
        if lang.is_some() && found != lang {
            return Err(format!("a `final` has lang {found:?}, want {lang:?}"));
        }
    }
    Ok(())
}

/// A run whose output device was missing: it did not stop, said it once in the
/// log and once on `/events`, made `final`s, and sent no `error`.
pub fn check_capture_fallback(served: &Served) -> Result<(), Failure> {
    check_no_error_event(&served.wire)?;
    checks::check_resource_fallback(&CAPTURE_SINGLE, served, true)
}

/// A run whose devices were all there: `final`s came out and no capture fallback was said.
pub fn check_present_devices(served: &Served) -> Result<(), Failure> {
    if first_of(&served.wire, &["final"]).is_none() {
        return Err(String::from("no `final` event came out"));
    }
    for scene in [&CAPTURE_SINGLE, &scenes::CAPTURE_NONE] {
        checks::check_resource_present(scene, served)?;
    }
    Ok(())
}

fn ready() -> Value {
    json!({"type": "ready", "sr": 16000})
}

fn partial() -> Value {
    json!({"type": "partial", "text": "hello"})
}

fn english_final() -> Value {
    json!({"type": "final", "text": "hello world", "lang": "en"})
}

/// T-RP-10-001
#[test]
fn ready_then_partial_then_english_final_passes() -> TestResult {
    check_event_stream(&[ready(), partial(), english_final()])?;
    Ok(())
}

/// T-RP-10-002
#[test]
fn final_before_ready_is_rejected() {
    let stream = [partial(), english_final(), ready()];
    assert!(names(check_event_stream(&stream), "before `ready`"));
}

/// T-RP-10-003
#[test]
fn no_final_is_rejected() {
    assert!(names(
        check_event_stream(&[ready(), partial()]),
        "no `final`"
    ));
}

/// T-RP-10-004
#[test]
fn no_partial_is_rejected() {
    assert!(names(
        check_event_stream(&[ready(), english_final()]),
        "no `partial`"
    ));
}

/// T-RP-10-005
#[test]
fn non_english_final_is_rejected() {
    let japanese = json!({"type": "final", "text": "こんにちは", "lang": "ja"});
    assert!(names(
        check_event_stream(&[ready(), partial(), japanese]),
        "lang"
    ));
}

/// T-RP-10-006
#[test]
fn missing_output_device_falls_back_to_the_microphone() -> TestResult {
    check_capture_fallback(&fell(&CAPTURE_SINGLE))?;
    Ok(())
}

/// T-RP-10-009
#[test]
fn present_devices_emit_no_capture_fallback() -> TestResult {
    check_present_devices(&whole(0))?;
    Ok(())
}

/// T-RP-10-010
#[test]
fn silent_capture_fallback_is_rejected() {
    let served = served_of(
        0,
        String::new(),
        vec![final_wire(), scene_wire(&CAPTURE_SINGLE)],
    );
    assert!(names(
        check_capture_fallback(&served),
        "capture.fallback.single"
    ));
}

/// T-RP-10-011
#[test]
fn error_event_without_final_is_reported() {
    let stream = [ready(), json!({"type": "error", "message": "no device"})];
    assert!(names(check_event_stream(&stream), "`error`"));
}

/// The same rule holds for the capture run: an `error` event is a failure even when a `final` follows.
#[test]
fn error_event_in_a_capture_run_is_rejected() {
    let mut served = fell(&CAPTURE_SINGLE);
    served
        .wire
        .push(json!({"type": "error", "message": "device lost"}));
    assert!(names(check_capture_fallback(&served), "`error`"));
}

/// A present-device run that still says a capture fallback is rejected.
#[test]
fn capture_fallback_with_the_devices_present_is_rejected() {
    assert!(names(
        check_present_devices(&fell(&CAPTURE_SINGLE)),
        "capture single"
    ));
}

/// T-RP-10-008: the same check on a `--wav` run.
#[test]
#[ignore = "runs the hayamimi binary on an English recording"]
fn wsl_wav_run_passes_the_same_check() -> TestResult {
    let wav = support::path_text(&support::find_above("testdata/lat-en.wav")?)?;
    let models = support::path_text(&support::models_dir()?)?;
    let served = support::serve(
        &["--wav", &wav, "--no-realtime", "--models", &models],
        Some("C.UTF-8"),
        &[],
    )?;
    // The `lang == en` condition is not R5's: the English route comes with R3b.
    check_event_stream_in(&served.wire, None)?;
    Ok(())
}

/// The language condition is the only one the real run leaves out.
#[test]
fn leaving_the_language_unchecked_still_wants_ready_partial_and_final() {
    let japanese = json!({"type": "final", "text": "こんにちは", "lang": "ja"});
    assert!(check_event_stream_in(&[ready(), partial(), japanese.clone()], None).is_ok());
    assert!(names(
        check_event_stream_in(&[ready(), japanese], None),
        "no `partial`"
    ));
}

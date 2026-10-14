//! RP-29: a missing resource other than the GPU does not stop the run. For
//! each scene of design 3.15 a run with the resource taken away exits with
//! code 0, still serves (subtitles, or the server alone when the inference
//! libraries are gone), and says the fall once in the log and once on
//! `/events`; a run with the resource present says nothing of the kind.
//!
//! The scenes, the checks, the hand-written runs and the real runs are in
//! the files under `rp29/`.

mod support;

#[path = "rp29/checks.rs"]
mod checks;
#[path = "rp29/fixtures.rs"]
mod fixtures;
#[path = "rp29/real.rs"]
mod real;
#[path = "rp29/scenes.rs"]
mod scenes;

use checks::{
    check_cleared, check_defaults_match_dump, check_gpu_stays_on_cpu, check_port_move,
    check_replay, check_resource_fallback, check_resource_present, port_of,
};
use fixtures::{
    active_pair, cleared_run, fell, final_wire, log_line, names, scene_line, scene_wire, served_of,
    settings, whole,
};
use scenes::{
    CAPTURE_NONE, CAPTURE_SINGLE, CONFIG, LIBRARY, LID_DISABLED, MODEL_SMALLER, MODEL_SUBSTITUTE,
    SECOND_OPINION_DISABLED, SINK, SPEAKER, TRANSLATE,
};
use support::TestResult;

/// T-RP-29-001
#[test]
fn missing_model_falls_back_and_says_so() -> TestResult {
    check_resource_fallback(&MODEL_SMALLER, &fell(&MODEL_SMALLER), true)?;
    Ok(())
}

/// T-RP-29-002
#[test]
fn present_model_emits_no_fallback_event() -> TestResult {
    check_resource_present(&MODEL_SMALLER, &whole(0))?;
    Ok(())
}

/// T-RP-29-003
#[test]
fn missing_library_keeps_the_server_and_says_so() -> TestResult {
    let served = served_of(
        0,
        scene_line(&LIBRARY),
        vec![
            serde_json::json!({"type": "session_start"}),
            scene_wire(&LIBRARY),
        ],
    );
    check_resource_fallback(&LIBRARY, &served, false)?;
    Ok(())
}

/// T-RP-29-004
#[test]
fn silent_fallback_is_rejected() {
    let served = served_of(
        0,
        String::new(),
        vec![final_wire(), scene_wire(&MODEL_SMALLER)],
    );
    assert!(names(
        check_resource_fallback(&MODEL_SMALLER, &served, true),
        "model"
    ));
}

/// T-RP-29-005
#[test]
fn fallback_with_the_resource_present_is_rejected() {
    assert!(names(
        check_resource_present(&MODEL_SMALLER, &fell(&MODEL_SMALLER)),
        "model"
    ));
}

/// T-RP-29-006
#[test]
fn nonzero_exit_code_in_a_missing_resource_run_is_rejected() {
    let served = served_of(
        1,
        scene_line(&MODEL_SMALLER),
        vec![final_wire(), scene_wire(&MODEL_SMALLER)],
    );
    assert!(names(
        check_resource_fallback(&MODEL_SMALLER, &served, true),
        "want 0"
    ));
}

/// T-RP-29-007
#[test]
fn fallback_not_sent_to_events_is_rejected() {
    let served = served_of(0, scene_line(&MODEL_SMALLER), vec![final_wire()]);
    assert!(names(
        check_resource_fallback(&MODEL_SMALLER, &served, true),
        "/events"
    ));
}

/// T-RP-29-008
#[test]
fn two_fallback_events_for_one_scene_are_rejected() {
    let twice = scene_line(&MODEL_SMALLER) + &scene_line(&MODEL_SMALLER);
    let served = served_of(0, twice, vec![final_wire(), scene_wire(&MODEL_SMALLER)]);
    assert!(names(
        check_resource_fallback(&MODEL_SMALLER, &served, true),
        "want 1"
    ));
}

/// T-RP-29-009
#[test]
fn unreachable_llama_server_holds_translation_and_says_so() -> TestResult {
    check_resource_fallback(&TRANSLATE, &fell(&TRANSLATE), true)?;
    Ok(())
}

/// T-RP-29-010
#[test]
fn unreachable_sink_keeps_translations_locally() -> TestResult {
    check_resource_fallback(&SINK, &fell(&SINK), true)?;
    Ok(())
}

/// T-RP-29-011
#[test]
fn blocked_port_uses_the_tenth_next_port() -> TestResult {
    check_port_move(8833, 8843)?;
    let event = serde_json::json!({"url": "http://127.0.0.1:8843/events"});
    assert_eq!(port_of(&event)?, 8843);
    Ok(())
}

/// T-RP-29-012
#[test]
fn blocked_port_beyond_ten_next_ports_is_rejected() {
    assert!(check_port_move(8833, 8844).is_err());
    assert!(check_port_move(8833, 8833).is_err());
}

/// T-RP-29-013
#[test]
fn missing_capture_devices_fall_back_to_single_or_none() -> TestResult {
    check_resource_fallback(&CAPTURE_SINGLE, &fell(&CAPTURE_SINGLE), true)?;
    check_resource_fallback(&CAPTURE_NONE, &fell(&CAPTURE_NONE), true)?;
    Ok(())
}

/// T-RP-29-014
#[test]
fn broken_config_without_previous_uses_the_dump_defaults() -> TestResult {
    let dump = settings(&[("min_silence_ms", 350), ("max_speech_s", 12)]);
    check_defaults_match_dump(&dump.clone(), &dump)?;
    check_resource_fallback(&CONFIG, &fell(&CONFIG), true)?;
    Ok(())
}

/// T-RP-29-015
#[test]
fn default_config_differing_from_the_dump_is_rejected() {
    let dump = settings(&[("min_silence_ms", 350), ("max_speech_s", 12)]);
    let effective = settings(&[("min_silence_ms", 350), ("max_speech_s", 13)]);
    assert!(names(
        check_defaults_match_dump(&effective, &dump),
        "max_speech_s"
    ));
}

/// T-RP-29-016
#[test]
fn recovered_sink_emits_cleared_events() -> TestResult {
    check_cleared(&cleared_run(), &SINK)?;
    Ok(())
}

/// T-RP-29-017
#[test]
fn recovery_without_cleared_event_is_rejected() {
    let mut run = cleared_run();
    run.output.stdout = scene_line(&SINK);
    assert!(names(check_cleared(&run, &SINK), "cleared"));
}

/// T-RP-29-018
#[test]
fn gpu_stage_stays_on_cpu_when_the_device_returns() -> TestResult {
    let stdout = log_line(
        "gpu.session.created",
        ",\"model\":\"punct_bert.onnx\",\"actual_provider\":\"cpu\"",
    );
    check_gpu_stays_on_cpu(&served_of(0, stdout, vec![final_wire()]))?;
    Ok(())
}

/// T-RP-29-019
#[test]
fn reconnected_client_receives_active_fallbacks_first() -> TestResult {
    check_replay(&active_pair(), &["punct", "vad"])?;
    Ok(())
}

/// T-RP-29-020
#[test]
fn reconnected_client_missing_an_active_fallback_is_named() {
    let stream = vec![
        final_wire(),
        serde_json::json!({"type": "fallback", "stage": "punct", "to": "cpu"}),
    ];
    assert!(names(check_replay(&stream, &["punct", "vad"]), "vad"));
}

/// T-RP-29-022
#[test]
fn missing_recognizer_tier_is_substituted_and_says_so() -> TestResult {
    check_resource_fallback(&MODEL_SUBSTITUTE, &fell(&MODEL_SUBSTITUTE), true)?;
    Ok(())
}

/// T-RP-29-023
#[test]
fn missing_identifier_model_leaves_the_stage_out_and_says_so() -> TestResult {
    check_resource_fallback(&LID_DISABLED, &fell(&LID_DISABLED), true)?;
    Ok(())
}

/// T-RP-29-024
#[test]
fn missing_second_opinion_model_turns_it_off_and_says_so() -> TestResult {
    check_resource_fallback(
        &SECOND_OPINION_DISABLED,
        &fell(&SECOND_OPINION_DISABLED),
        true,
    )?;
    Ok(())
}

/// T-RP-29-025
#[test]
fn present_identifier_emits_no_fallback_event() -> TestResult {
    check_resource_present(&LID_DISABLED, &whole(0))?;
    check_resource_present(&MODEL_SUBSTITUTE, &whole(0))?;
    Ok(())
}

/// T-RP-29-026
#[test]
fn silent_identifier_fallback_is_rejected() {
    let served = served_of(
        0,
        String::new(),
        vec![final_wire(), scene_wire(&LID_DISABLED)],
    );
    assert!(names(
        check_resource_fallback(&LID_DISABLED, &served, true),
        "lid"
    ));
}

/// T-RP-29-028
#[test]
fn silent_speaker_fallback_is_rejected() {
    let served = served_of(0, String::new(), vec![final_wire(), scene_wire(&SPEAKER)]);
    assert!(names(
        check_resource_fallback(&SPEAKER, &served, true),
        "speaker"
    ));
}

/// T-RP-29-021, for the scenes the Japanese route owns.
#[test]
#[ignore = "runs the hayamimi binary"]
fn missing_resources_fall_back_in_real_runs() -> TestResult {
    real::all_scenes()?;
    Ok(())
}

/// The inference libraries are taken away: only a Windows build can start
/// without them, because it loads sherpa-onnx on the first call.
#[cfg(windows)]
#[test]
#[ignore = "runs the hayamimi binary without its libraries"]
fn missing_library_in_a_real_run_keeps_the_server() -> TestResult {
    use std::path::PathBuf;

    let bin_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("rp29-bare");
    std::fs::create_dir_all(&bin_dir)?;
    let exe = bin_dir.join("hayamimi.exe");
    std::fs::copy(env!("CARGO_BIN_EXE_hayamimi"), &exe)?;
    let wav = real::path_text(&support::clip())?;
    let models = real::path_text(&support::models_dir()?)?;
    // Without an engine nothing is decoded, so the run lasts as long as the audio is paced:
    // that is the time the client has to connect.
    let args: Vec<&str> = real::base_args(&wav, &models)
        .into_iter()
        .filter(|arg| *arg != "--no-realtime")
        .collect();
    let port = support::free_port()?;
    let exe = real::path_text(&exe)?;
    let served = support::serve_binary(&exe, &args, (port, vec![port]), Some("C.UTF-8"), &[])?;
    check_resource_fallback(&LIBRARY, &served, false)?;
    Ok(())
}

/// The acceptance test of RP-29: each scene of the Japanese route falls back
/// and says so when its resource is taken away, and says nothing when present.
#[test]
#[ignore = "runs the hayamimi binary"]
fn rp29_missing_resources_fall_back_and_say_so() -> TestResult {
    real::all_scenes()?;
    Ok(())
}

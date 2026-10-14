//! The real runs of RP-29 for the scenes the Japanese route owns: the
//! hayamimi binary is started with a resource taken away, and again with it
//! present.

use std::path::{Path, PathBuf};

use crate::checks::{check_port_move, check_resource_fallback, check_resource_present, port_of};
use serde_json::Value;

use crate::scenes::{CONFIG, MODEL_DISABLED, NEXT_PORTS, PORT, SPEAKER, text_of};
use crate::support::{
    Failure, Served, clip, events, free_port, models_dir, models_without, named, serve, serve_from,
};

pub fn base_args<'a>(wav: &'a str, models: &'a str) -> Vec<&'a str> {
    vec![
        "--wav",
        wav,
        "--no-realtime",
        "--mode",
        "single",
        "--lang",
        "ja",
        "--models",
        models,
    ]
}

pub fn path_text(path: &Path) -> Result<String, Failure> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("{} is not UTF-8", path.display()))
}

fn run_with(models: &Path, extra_args: &[&str]) -> Result<Served, Failure> {
    let wav = path_text(&clip())?;
    let models = path_text(models)?;
    let mut args = base_args(&wav, &models);
    args.extend(extra_args);
    serve(&args, Some("C.UTF-8"), &[])
}

/// The punctuation model is taken away: that stage is left out and subtitles still come.
pub fn model_scene() -> Result<(), Failure> {
    let without = models_without("mojicast-punct-onnx")?;
    check_resource_fallback(&MODEL_DISABLED, &run_with(&without, &[])?, true)?;
    check_resource_present(&MODEL_DISABLED, &run_with(&models_dir()?, &[])?)
}

/// How many `final` events on the wire were followed by a `speaker` event of
/// the same id with a mark, and how many finals there are.
fn marked_finals(served: &Served) -> (usize, usize) {
    let finals: Vec<&Value> = served
        .wire
        .iter()
        .filter(|event| text_of(event, "type") == Some("final"))
        .collect();
    let marked = finals
        .iter()
        .filter(|done| {
            served.wire.iter().any(|event| {
                text_of(event, "type") == Some("speaker")
                    && text_of(event, "id") == text_of(done, "id")
                    && text_of(event, "speaker").is_some_and(|mark| !mark.is_empty())
            })
        })
        .count();
    (marked, finals.len())
}

/// How many `speaker` events were on the wire.
fn speaker_events(served: &Served) -> usize {
    served
        .wire
        .iter()
        .filter(|event| text_of(event, "type") == Some("speaker"))
        .count()
}

/// The speaker model is taken away: the stage is left out, subtitles come
/// without a mark, and with the model present every utterance has one.
pub fn speaker_scene() -> Result<(), Failure> {
    let without = models_without("campplus_sv.onnx")?;
    let bare = run_with(&without, &[])?;
    check_resource_fallback(&SPEAKER, &bare, true)?;
    let (marked, total) = marked_finals(&bare);
    let sent = speaker_events(&bare);
    if total == 0 || marked != 0 || sent != 0 {
        return Err(format!(
            "speaker: without the model, {marked} of {total} finals have a mark and {sent} speaker events were sent"
        ));
    }
    let whole = run_with(&models_dir()?, &[])?;
    check_resource_present(&SPEAKER, &whole)?;
    let (marked, total) = marked_finals(&whole);
    if total == 0 || marked != total || speaker_events(&whole) == 0 {
        return Err(format!(
            "speaker: {marked} of {total} finals have a mark with the model"
        ));
    }
    Ok(())
}

/// The requested port is held: the server moves to one of the next ten.
pub fn port_scene() -> Result<(), Failure> {
    let blocked = free_port()?;
    let _held = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, blocked))
        .map_err(|e| e.to_string())?;
    let wav = path_text(&clip())?;
    let models = path_text(&models_dir()?)?;
    let candidates: Vec<u16> = (blocked + 1..=blocked + NEXT_PORTS).collect();
    let served = serve_from(
        &base_args(&wav, &models),
        blocked,
        candidates,
        Some("C.UTF-8"),
        &[],
    )?;
    check_resource_fallback(&PORT, &served, true)?;
    let all = events(&served.output.stdout)?;
    let event = named(&all, PORT.event)
        .into_iter()
        .next()
        .ok_or("no port event")?;
    check_port_move(blocked, port_of(event)?)?;
    check_resource_present(&PORT, &run_with(&models_dir()?, &[])?)
}

/// The replacement dictionary does not exist: the run uses none and says so.
pub fn config_scene() -> Result<(), Failure> {
    let missing =
        path_text(&PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("rp29-no-such-dictionary.txt"))?;
    let broken = run_with(&models_dir()?, &["--replace", &missing])?;
    check_resource_fallback(&CONFIG, &broken, true)?;
    check_resource_present(&CONFIG, &run_with(&models_dir()?, &[])?)
}

/// Every scene of the Japanese route, one after another.
pub fn all_scenes() -> Result<(), Failure> {
    model_scene()?;
    speaker_scene()?;
    port_scene()?;
    config_scene()
}

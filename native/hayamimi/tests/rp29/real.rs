//! The real runs of RP-29 for the scenes the Japanese route owns: the
//! hayamimi binary is started with a resource taken away, and again with it
//! present.

use std::path::{Path, PathBuf};

use crate::checks::{check_port_move, check_resource_fallback, check_resource_present, port_of};
use serde_json::Value;

#[cfg(unix)]
use crate::scenes::{CAPTURE_NONE, INGEST_PORT};
use crate::scenes::{CONFIG, MODEL_DISABLED, NEXT_PORTS, PORT, SPEAKER, text_of};
#[cfg(unix)]
use crate::support::{BINARY, interrupt, serve_driven};
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

/// Connects to the `/ingest` of the first of `ports` that answers.
#[cfg(unix)]
fn connect_ingest(
    ports: &[u16],
    origin: std::time::Instant,
) -> Result<hayamimi_eval::ingest::Ingest, Failure> {
    let deadline = origin + std::time::Duration::from_secs(120);
    while std::time::Instant::now() < deadline {
        for port in ports {
            if let Ok(client) = hayamimi_eval::ingest::open("127.0.0.1", *port, origin) {
                return Ok(client);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Err(format!("no /ingest answered on {ports:?}"))
}

/// Plays the clip into the `/ingest` of the running binary, checks that a
/// `final` came back to the client, and ends the run the way Ctrl-C does.
#[cfg(unix)]
fn play_and_interrupt(child: &mut std::process::Child, ports: &[u16]) -> Result<(), Failure> {
    let wav = hayamimi_eval::wav::read_wav(&clip()).map_err(|error| error.to_string())?;
    let mut client = connect_ingest(ports, std::time::Instant::now())?;
    client.start().map_err(|error| error.to_string())?;
    let gap = wav.samples.len() / 2;
    let plan = hayamimi_eval::ingest::plan(wav.samples.len(), &[], 1, gap);
    client
        .play(&plan, &wav.samples)
        .map_err(|error| error.to_string())?;
    // The models may still be opening when the audio ends; the answers come after.
    let heard = client
        .finish(std::time::Duration::from_secs(40))
        .map_err(|error| error.to_string())?;
    if !heard
        .events
        .iter()
        .any(|(_, text)| text.contains("\"type\":\"final\""))
    {
        return Err(String::from("no `final` came back on /ingest"));
    }
    interrupt(child)
}

/// A run that takes its audio from `/ingest` on `ingest_ports` (the first
/// that answers), and ends when the audio has been answered.
#[cfg(unix)]
fn run_on_ingest(input: &[&str], requested: u16, ingest_ports: &[u16]) -> Result<Served, Failure> {
    let models = path_text(&models_dir()?)?;
    let number = requested.to_string();
    let mut args = vec!["--models", models.as_str(), "--ws-port", number.as_str()];
    args.extend(input);
    let serve_port = free_port()?;
    serve_driven(
        BINARY,
        &args,
        (serve_port, vec![serve_port]),
        (Some("C.UTF-8"), &[]),
        |child| play_and_interrupt(child, ingest_ports),
    )
}

/// The microphone is missing: the run says so once, goes on with the audio of
/// `/ingest`, makes subtitles from it, and ends with code 0.
#[cfg(unix)]
pub fn capture_scene() -> Result<(), Failure> {
    let port = free_port()?;
    // A name no device has, so that the scene holds on a machine that has a microphone too.
    let missing = ["--input", "mic", "--mic-device", "no-such-microphone-9f3a"];
    check_resource_fallback(
        &CAPTURE_NONE,
        &run_on_ingest(&missing, port, &[port])?,
        true,
    )?;
    let port = free_port()?;
    let present = run_on_ingest(&["--input", "ws"], port, &[port])?;
    check_resource_present(&CAPTURE_NONE, &present)
}

/// The port of `/ingest` is held: the server moves to one of the next ten.
#[cfg(unix)]
pub fn ingest_port_scene() -> Result<(), Failure> {
    let blocked = free_port()?;
    let _held = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, blocked))
        .map_err(|error| error.to_string())?;
    let candidates: Vec<u16> = (blocked + 1..=blocked + NEXT_PORTS).collect();
    let served = run_on_ingest(&["--input", "ws"], blocked, &candidates)?;
    check_resource_fallback(&INGEST_PORT, &served, true)?;
    let all = events(&served.output.stdout)?;
    let event = named(&all, INGEST_PORT.event)
        .into_iter()
        .next()
        .ok_or("no port event")?;
    check_port_move(blocked, port_of(event)?)
}

/// Every scene of the Japanese route, one after another. The scenes that
/// play audio into `/ingest` and end the run with an interrupt are for Unix.
pub fn all_scenes() -> Result<(), Failure> {
    model_scene()?;
    speaker_scene()?;
    port_scene()?;
    config_scene()?;
    #[cfg(unix)]
    {
        ingest_port_scene()?;
        capture_scene()?;
    }
    Ok(())
}

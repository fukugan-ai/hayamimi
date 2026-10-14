//! The real runs of RP-29 for the scenes the Japanese route owns: the
//! hayamimi binary is started with a resource taken away, and again with it
//! present.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use crate::checks::{check_port_move, check_resource_fallback, check_resource_present, port_of};
use crate::scenes::{CONFIG, MODEL_DISABLED, NEXT_PORTS, PORT};
use crate::support::{
    Failure, Served, clip, events, free_port, models_dir, named, serve, serve_from,
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

#[cfg(unix)]
fn link(from: &Path, to: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(from, to)
}

#[cfg(windows)]
fn link(from: &Path, to: &Path) -> std::io::Result<()> {
    if from.is_dir() {
        std::os::windows::fs::symlink_dir(from, to)
    } else {
        std::os::windows::fs::symlink_file(from, to)
    }
}

static NEXT_DIRECTORY: AtomicU32 = AtomicU32::new(0);

/// A models directory that holds everything the real one does except the entry `without`.
fn models_without(without: &str) -> Result<PathBuf, Failure> {
    let real = models_dir()?;
    // Tests run on several threads of one process; each call gets a directory of its own.
    let number = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    let name = format!("rp29-models-without-{without}-{number}");
    let target = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    if target.exists() {
        std::fs::remove_dir_all(&target).map_err(|error| error.to_string())?;
    }
    std::fs::create_dir_all(&target).map_err(|error| error.to_string())?;
    let entries = std::fs::read_dir(&real).map_err(|error| error.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        if entry.file_name() == without {
            continue;
        }
        link(&entry.path(), &target.join(entry.file_name())).map_err(|error| error.to_string())?;
    }
    Ok(target)
}

/// The punctuation model is taken away: that stage is left out and subtitles still come.
pub fn model_scene() -> Result<(), Failure> {
    let without = models_without("mojicast-punct-onnx")?;
    check_resource_fallback(&MODEL_DISABLED, &run_with(&without, &[])?, true)?;
    check_resource_present(&MODEL_DISABLED, &run_with(&models_dir()?, &[])?)
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
    port_scene()?;
    config_scene()
}

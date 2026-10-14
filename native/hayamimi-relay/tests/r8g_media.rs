//! R8g: the real steps of a video job against stand-in programs (`examples/`
//! `fake_ffmpeg` and `fake_recognizer`): the child processes are started,
//! read and waited for, and the `srt` command makes its files from them.
//!
//! `ffmpeg`, `hayamimi` and the translator server are fake, so this pins the
//! plumbing (arguments, the stream, the exit codes, no process left behind),
//! not the recognition. The run on a real video is not done here.
#![cfg(unix)]

mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use hayamimi_relay::cli;
use hayamimi_relay::logging::MemorySink;
use hayamimi_relay::media::Recognizer;
use hayamimi_relay::video::{Recognize, StepFailed};
use support::{FakeServer, Reply, TempDir, TestResult, chat_reply};

const EVENTS: &str = concat!(
    "{\"type\":\"session_start\"}\n",
    "{\"type\":\"partial\",\"text\":\"Hel\"}\n",
    "{\"type\":\"final\",\"id\":\"file-1\",\"source\":\"file\",\"lang\":\"en\",\"text\":\"Hello there.\",\"t0_s\":0.5,\"t1_s\":2.0}\n",
    "{\"type\":\"final\",\"id\":\"file-2\",\"source\":\"file\",\"lang\":\"ja\",\"text\":\"こんにちは。\",\"t0_s\":2.5,\"t1_s\":4.0}\n",
);

/// A program built from `examples/`, next to the test binaries.
fn example(name: &str) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let deps = exe.parent().ok_or("no directory for the test binary")?;
    let path = deps
        .parent()
        .ok_or("no target directory")?
        .join("examples")
        .join(name);
    if path.exists() {
        Ok(path)
    } else {
        Err(format!("{name} was not built"))
    }
}

fn recognizer(name: &str) -> Result<Recognizer, String> {
    Ok(Recognizer {
        program: example(name)?,
        models: None,
    })
}

/// Whether the process whose id the stand-in wrote beside `events` is still
/// in the process table: a zombie is, and so is a process left running.
fn still_listed(events: &Path) -> Result<bool, Box<dyn std::error::Error>> {
    let pid = fs::read_to_string(format!("{}.pid", events.display()))?;
    Ok(Path::new("/proc").join(pid.trim()).exists())
}

#[test]
fn the_recognizer_is_started_read_to_the_end_and_waited_for() -> TestResult {
    let dir = TempDir::new("media-ok")?;
    let events = dir.join("a.events");
    fs::write(&events, EVENTS)?;
    let mut sink = MemorySink::default();
    let lines = recognizer("fake_recognizer")?
        .recognize(&events, &mut sink)
        .map_err(|_| "the step failed")?;
    let ids: Vec<&str> = lines.iter().map(|line| line.id.as_str()).collect();
    assert_eq!(ids, ["file-1", "file-2"]);
    // The partial and the session start are not lines.
    assert!(
        sink.events
            .iter()
            .all(|e| e.level != hayamimi_relay::logging::Level::Error)
    );
    assert!(!still_listed(&events)?);
    Ok(())
}

#[test]
fn a_recognizer_that_ends_badly_fails_the_step_and_is_waited_for() -> TestResult {
    let dir = TempDir::new("media-bad-exit")?;
    let events = dir.join("a.events");
    fs::write(&events, format!("#exit=1\n{EVENTS}"))?;
    let mut sink = MemorySink::default();
    let done = recognizer("fake_recognizer")?.recognize(&events, &mut sink);
    assert_eq!(done, Err(StepFailed));
    let failed: Vec<_> = sink
        .events
        .iter()
        .filter(|e| e.event == "srt.recognize.failed")
        .collect();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].fields["reason"], "exit");
    assert!(!still_listed(&events)?);
    Ok(())
}

#[test]
fn a_recognizer_that_ends_before_it_serves_fails_the_step_at_once() -> TestResult {
    let dir = TempDir::new("media-no-stream")?;
    let events = dir.join("a.events");
    fs::write(&events, EVENTS)?;
    let quits = Recognizer {
        program: PathBuf::from("true"),
        models: None,
    };
    let (mut sink, started) = (MemorySink::default(), Instant::now());
    assert_eq!(quits.recognize(&events, &mut sink), Err(StepFailed));
    assert!(started.elapsed() < Duration::from_secs(10));
    assert_eq!(sink.events[0].fields["reason"], "no_stream");
    Ok(())
}

fn srt_args(video: &Path, llm: &str) -> Result<Vec<String>, String> {
    let text = |path: &Path| path.display().to_string();
    Ok(vec![
        String::from("srt"),
        text(video),
        String::from("--llm"),
        llm.to_owned(),
        String::from("--ffmpeg"),
        text(&example("fake_ffmpeg")?),
        String::from("--hayamimi"),
        text(&example("fake_recognizer")?),
    ])
}

#[test]
fn the_srt_command_makes_both_files_from_the_stand_in_programs() -> TestResult {
    let llm = FakeServer::start(|_, _| Reply::json(&chat_reply("こんにちは。")))?;
    let dir = TempDir::new("media-srt")?;
    // The stand-in ffmpeg hands the video's bytes on as the audio, and the
    // stand-in recognizer serves them as events.
    let video = dir.join("talk.mp4");
    fs::write(&video, EVENTS)?;
    let args = srt_args(&video, &llm.base_url())?;
    assert_eq!(cli::run(&args, 1_790_000_000, Some("en")), 0);
    let ja = fs::read_to_string(dir.join("talk.ja.srt"))?;
    let en = fs::read_to_string(dir.join("talk.en.srt"))?;
    assert_eq!(
        ja,
        "1\n00:00:00,500 --> 00:00:02,000\nこんにちは。\n\n2\n00:00:02,500 --> 00:00:04,000\nこんにちは。\n\n"
    );
    assert_eq!(en, "1\n00:00:00,500 --> 00:00:02,000\nHello there.\n\n");
    // The work file is gone.
    let work = dirs::cache_dir()
        .ok_or("no cache directory")?
        .join("hayamimi")
        .join(format!("srt-{}.wav", std::process::id()));
    assert!(!work.exists());
    // A second run stops on the files and leaves them as they are.
    assert_eq!(cli::run(&args, 1_790_000_000, Some("en")), 1);
    assert_eq!(fs::read_to_string(dir.join("talk.ja.srt"))?, ja);
    Ok(())
}

#[test]
fn the_srt_command_fails_without_files_when_the_audio_cannot_be_taken_out() -> TestResult {
    let llm = FakeServer::start(|_, _| Reply::json(&chat_reply("こんにちは。")))?;
    let dir = TempDir::new("media-no-video")?;
    let missing = dir.join("none.mp4");
    let args = srt_args(&missing, &llm.base_url())?;
    assert_eq!(cli::run(&args, 1_790_000_000, Some("en")), 1);
    assert!(!dir.join("none.ja.srt").exists());
    assert!(llm.requests().is_empty());
    Ok(())
}

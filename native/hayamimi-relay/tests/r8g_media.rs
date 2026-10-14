//! R8g: the real steps of a video job against stand-in programs (`examples/`
//! `fake_ffmpeg` and `fake_recognizer`): the child processes are started,
//! the audio is sent to the recognizer's `/ingest` over a real WebSocket, the
//! lines come back on that connection, and no process is left behind.
//!
//! `ffmpeg`, `hayamimi` and the translator server are fake, so this pins the
//! plumbing (arguments, the stream, the pace, the exit codes, no process
//! left behind), not the recognition. The run on a real video is not done
//! here.
#![cfg(unix)]

mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use hayamimi_relay::cli;
use hayamimi_relay::logging::{Level, MemorySink};
use hayamimi_relay::media::Recognizer;
use hayamimi_relay::video::{Recognize, StepFailed};
use support::{FakeServer, Reply, TempDir, TestResult, chat_reply};

const EVENTS: &str = concat!(
    "{\"type\":\"session_start\"}\n",
    "{\"type\":\"partial\",\"text\":\"Hel\"}\n",
    "{\"type\":\"final\",\"id\":\"file-1\",\"source\":\"file\",\"lang\":\"en\",\"text\":\"Hello there.\",\"t0_s\":0.5,\"t1_s\":2.0}\n",
    "{\"type\":\"final\",\"id\":\"file-2\",\"source\":\"file\",\"lang\":\"ja\",\"text\":\"こんにちは。\",\"t0_s\":2.5,\"t1_s\":4.0}\n",
);

/// A wav file (16 kHz, mono, 16-bit) whose samples are `text`, which the
/// stand-in recognizer reads back as the events to answer with. Spaces pad it
/// to `at_least` bytes, so that a test can give the audio a length.
fn wav_of(text: &str, at_least: usize) -> Vec<u8> {
    let mut data = text.as_bytes().to_vec();
    while data.len() < at_least || data.len() % 2 == 1 {
        data.push(b' ');
    }
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16_u32.to_le_bytes());
    for field in [1_u16, 1] {
        out.extend_from_slice(&field.to_le_bytes());
    }
    out.extend_from_slice(&16_000_u32.to_le_bytes());
    out.extend_from_slice(&32_000_u32.to_le_bytes());
    for field in [2_u16, 16] {
        out.extend_from_slice(&field.to_le_bytes());
    }
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&data);
    out
}

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

/// The stand-in recognizer, told to be quiet for a short time only, and to
/// leave its process id in `scratch`.
fn recognizer(scratch: &Path) -> Result<Recognizer, String> {
    let mut made = Recognizer::new(example("fake_recognizer")?, Some(scratch.to_path_buf()));
    made.tail = Duration::from_millis(100);
    made.idle = Duration::from_millis(300);
    Ok(made)
}

/// Whether the process whose id the stand-in wrote in `scratch` is still in
/// the process table: a zombie is, and so is a process left running.
fn still_listed(scratch: &Path) -> Result<bool, Box<dyn std::error::Error>> {
    let pid = fs::read_to_string(scratch.join("fake.pid"))?;
    Ok(Path::new("/proc").join(pid.trim()).exists())
}

#[test]
fn the_recognizer_is_fed_through_ingest_and_its_lines_come_back() -> TestResult {
    let dir = TempDir::new("media-ok")?;
    let wav = dir.join("a.wav");
    fs::write(&wav, wav_of(EVENTS, 0))?;
    let mut sink = MemorySink::default();
    let lines = recognizer(&dir.join(""))?
        .recognize(&wav, &mut sink)
        .map_err(|_| "the step failed")?;
    let ids: Vec<&str> = lines.iter().map(|line| line.id.as_str()).collect();
    assert_eq!(ids, ["file-1", "file-2"]);
    // The partial and the session start are not lines.
    assert!(sink.events.iter().all(|e| e.level != Level::Error));
    assert!(!still_listed(&dir.join(""))?);
    Ok(())
}

#[test]
fn the_audio_is_sent_at_the_speed_of_speech() -> TestResult {
    let dir = TempDir::new("media-pace")?;
    let wav = dir.join("a.wav");
    // Half a second of samples: the stand-in answers only when the audio ends.
    fs::write(&wav, wav_of(EVENTS, 16_000))?;
    let (mut sink, started) = (MemorySink::default(), Instant::now());
    let lines = recognizer(&dir.join(""))?
        .recognize(&wav, &mut sink)
        .map_err(|_| "the step failed")?;
    assert_eq!(lines.len(), 2);
    assert!(
        started.elapsed() >= Duration::from_millis(500),
        "the audio was sent in {:?}",
        started.elapsed()
    );
    Ok(())
}

#[test]
fn a_recognizer_that_goes_down_mid_stream_fails_the_step_and_is_waited_for() -> TestResult {
    let dir = TempDir::new("media-bad-exit")?;
    let wav = dir.join("a.wav");
    fs::write(&wav, wav_of(&format!("#exit=1\n{EVENTS}"), 0))?;
    let mut sink = MemorySink::default();
    let done = recognizer(&dir.join(""))?.recognize(&wav, &mut sink);
    assert_eq!(done, Err(StepFailed));
    let failed: Vec<_> = sink
        .events
        .iter()
        .filter(|e| e.event == "srt.recognize.failed")
        .collect();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].fields["reason"], "lost");
    assert!(!still_listed(&dir.join(""))?);
    Ok(())
}

#[test]
fn a_recognizer_that_ends_before_it_serves_fails_the_step_at_once() -> TestResult {
    let dir = TempDir::new("media-no-stream")?;
    let wav = dir.join("a.wav");
    fs::write(&wav, wav_of(EVENTS, 0))?;
    let quits = Recognizer::new(PathBuf::from("true"), None);
    let (mut sink, started) = (MemorySink::default(), Instant::now());
    assert_eq!(quits.recognize(&wav, &mut sink), Err(StepFailed));
    assert!(started.elapsed() < Duration::from_secs(10));
    assert_eq!(sink.events[0].fields["reason"], "no_stream");
    Ok(())
}

#[test]
fn audio_that_is_not_a_wav_is_refused_before_a_program_is_started() -> TestResult {
    let dir = TempDir::new("media-not-wav")?;
    let wav = dir.join("a.wav");
    fs::write(&wav, EVENTS)?;
    let mut sink = MemorySink::default();
    let done = recognizer(&dir.join(""))?.recognize(&wav, &mut sink);
    assert_eq!(done, Err(StepFailed));
    assert_eq!(sink.events[0].fields["reason"], "audio");
    // No program was started, so none wrote its process id.
    assert!(!dir.join("fake.pid").exists());
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
    // stand-in recognizer answers with the events they hold.
    let video = dir.join("talk.mp4");
    fs::write(&video, wav_of(EVENTS, 0))?;
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

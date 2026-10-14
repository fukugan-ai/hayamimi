//! CS-15: when either subtitle file of a video is there already, the job
//! stops before the audio is taken out, says which file, and leaves the file
//! as it was.

mod support;

use std::fs;

use hayamimi_relay::pipeline::sha256_hex;
use hayamimi_relay::video::SrtError;
use support::video::{Run, english, run_job};
use support::{TempDir, TestResult};

fn first_run(dir: &TempDir) -> Result<Run, String> {
    run_job(
        &dir.join("talk.mp4"),
        vec![english(1, "Hello there.", 0.5, 2.0)],
    )
}

#[test]
fn cs15_existing_srt_stops_before_extraction() -> TestResult {
    let dir = TempDir::new("cs15")?;
    // The positive control: the first run extracts, and writes both files.
    let first = first_run(&dir)?;
    assert!(first.result.is_ok());
    assert_eq!(first.extracted, 1);
    assert_eq!(first.count("srt.extract.started"), 1);
    let written = sha256_hex(&fs::read_to_string(dir.join("talk.ja.srt"))?);

    // The second run stops: it says which files are there, extracts nothing,
    // asks the translator for nothing, and the file is the same bytes.
    let second = first_run(&dir)?;
    assert_eq!(second.result, Err(SrtError::Exists));
    assert_eq!(second.count("srt.output.exists"), 2);
    assert_eq!(second.count("srt.extract.started"), 0);
    assert_eq!((second.extracted, second.asked), (0, 0));
    assert_eq!(
        sha256_hex(&fs::read_to_string(dir.join("talk.ja.srt"))?),
        written
    );
    Ok(())
}

#[test]
fn cs15_one_of_the_two_files_is_enough_to_stop() -> TestResult {
    let dir = TempDir::new("cs15-one")?;
    fs::write(dir.join("talk.en.srt"), "kept")?;
    let run = first_run(&dir)?;
    assert_eq!(run.result, Err(SrtError::Exists));
    assert_eq!(run.count("srt.output.exists"), 1);
    assert_eq!(run.extracted, 0);
    assert_eq!(fs::read_to_string(dir.join("talk.en.srt"))?, "kept");
    assert!(!dir.join("talk.ja.srt").exists());
    Ok(())
}

#[test]
fn cs15_a_run_after_the_files_are_removed_goes_on() -> TestResult {
    // The negative control: the stop is the files' doing, not a memory of the
    // earlier run.
    let dir = TempDir::new("cs15-removed")?;
    assert!(first_run(&dir)?.result.is_ok());
    fs::remove_file(dir.join("talk.ja.srt"))?;
    fs::remove_file(dir.join("talk.en.srt"))?;
    let again = first_run(&dir)?;
    assert!(again.result.is_ok());
    assert_eq!(again.extracted, 1);
    Ok(())
}

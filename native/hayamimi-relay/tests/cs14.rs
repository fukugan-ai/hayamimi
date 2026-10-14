//! CS-14: a video yields subtitle files beside it, by the language of each
//! line. The steps that need a real video (taking the audio out, the
//! recognizer) are fake, so this pins the files, not the recognition; the run
//! on a real English video and a real Japanese video is not done here.

mod support;

use std::fs;

use hayamimi_relay::video::{SrtError, Written};
use support::video::{english, japanese, run_job, withheld_mark};
use support::{TempDir, TestResult};

const FIRST: &str = "I paid 100 dollars.";
const SECOND: &str = "Hello there.";

#[test]
fn cs14_video_yields_srt_files_by_line_language() -> TestResult {
    let dir = TempDir::new("cs14-en")?;
    let video = dir.join("talk.mp4");
    let lines = vec![english(1, FIRST, 0.5, 2.0), english(2, SECOND, 2.5, 4.0)];
    let run = run_job(&video, lines)?;
    // One cue for each line in both files.
    assert_eq!(run.result, Ok(Written { ja: 2, en: 2 }));
    let mark = withheld_mark()?;
    let ja = fs::read_to_string(dir.join("talk.ja.srt"))?;
    let en = fs::read_to_string(dir.join("talk.en.srt"))?;
    // The times are those of the lines; the withheld line has its mark and
    // its source, and the other has its translation.
    assert_eq!(
        ja,
        format!(
            "1\n00:00:00,500 --> 00:00:02,000\n{mark} {FIRST}\n\n2\n00:00:02,500 --> 00:00:04,000\nこんにちは。\n\n"
        )
    );
    assert_eq!(
        en,
        format!(
            "1\n00:00:00,500 --> 00:00:02,000\n{FIRST}\n\n2\n00:00:02,500 --> 00:00:04,000\n{SECOND}\n\n"
        )
    );
    // Plain UTF-8, no byte order mark.
    assert!(!fs::read(dir.join("talk.ja.srt"))?.starts_with(&[0xEF, 0xBB, 0xBF]));
    // No side file is left behind.
    assert!(!dir.join("talk.ja.srt.part").exists() && !dir.join("talk.en.srt.part").exists());
    assert_eq!(run.count("srt.output.written"), 1);
    Ok(())
}

#[test]
fn cs14_a_japanese_video_yields_only_the_japanese_file_and_asks_for_no_translation() -> TestResult {
    let dir = TempDir::new("cs14-ja")?;
    let video = dir.join("talk.mp4");
    let lines = vec![
        japanese(1, "こんにちは。", 0.5, 2.0),
        japanese(2, "さようなら。", 2.5, 4.0),
    ];
    let run = run_job(&video, lines)?;
    assert_eq!(run.result, Ok(Written { ja: 2, en: 0 }));
    assert!(dir.join("talk.ja.srt").exists());
    assert!(!dir.join("talk.en.srt").exists());
    assert_eq!(run.asked, 0);
    Ok(())
}

#[test]
fn cs14_a_video_with_no_speech_stops_and_says_so_without_writing_a_file() -> TestResult {
    let dir = TempDir::new("cs14-silent")?;
    let video = dir.join("silent.mp4");
    let run = run_job(&video, Vec::new())?;
    assert_eq!(run.result, Err(SrtError::NoSpeech));
    assert_eq!(run.count("srt.output.empty"), 1);
    assert!(!dir.join("silent.ja.srt").exists());
    assert!(!dir.join("silent.en.srt").exists());
    Ok(())
}

#[test]
fn cs14_the_cue_count_is_the_line_count_even_when_a_line_has_no_translation() -> TestResult {
    // The negative control: a build that dropped withheld lines would have
    // one cue here and not two.
    let dir = TempDir::new("cs14-held")?;
    let video = dir.join("talk.mp4");
    let lines = vec![english(1, FIRST, 0.5, 2.0), english(2, FIRST, 2.5, 4.0)];
    let run = run_job(&video, lines)?;
    assert_eq!(run.result, Ok(Written { ja: 2, en: 2 }));
    let ja = fs::read_to_string(dir.join("talk.ja.srt"))?;
    assert_eq!(ja.matches(&withheld_mark()?).count(), 2);
    Ok(())
}

#[test]
fn cs14_a_line_with_no_span_has_no_cue_and_is_said() -> TestResult {
    let dir = TempDir::new("cs14-timeless")?;
    let video = dir.join("talk.mp4");
    let mut timeless = english(1, SECOND, 0.0, 1.0);
    timeless.span = None;
    let lines = vec![timeless, english(2, SECOND, 2.5, 4.0)];
    let run = run_job(&video, lines)?;
    assert_eq!(run.result, Ok(Written { ja: 1, en: 1 }));
    assert_eq!(run.count("srt.cue.skipped"), 1);
    Ok(())
}

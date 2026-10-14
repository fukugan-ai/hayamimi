//! `srt <video>`: the subtitle files of a video, written beside it.
//!
//! The audio goes to a work file in the user's cache directory, which is
//! removed when the command ends, however it ends.

use std::fs;
use std::path::PathBuf;

use crate::cli::{default_rates_path, default_tables_dir};
use crate::lang::Lang;
use crate::llm::HttpLlm;
use crate::logging::{LogSink, StdoutSink};
use crate::media::{Ffmpeg, Recognizer};
use crate::pipeline::{Pipeline, PipelineConfig};
use crate::strings::Strings;
use crate::video::{Job, SrtError, Written, make_srt};

const EXIT_DONE: u8 = 0;
const EXIT_FAILED: u8 = 1;

/// What `srt` was asked to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SrtPlan {
    pub video: PathBuf,
    /// The translator's address.
    pub llm: String,
    pub ffmpeg: PathBuf,
    pub hayamimi: PathBuf,
    pub models: Option<PathBuf>,
}

impl SrtPlan {
    /// `video` with the programs found on the search path and no models
    /// directory named.
    pub fn new(video: PathBuf, llm: &str) -> SrtPlan {
        SrtPlan {
            video,
            llm: llm.to_owned(),
            ffmpeg: PathBuf::from("ffmpeg"),
            hayamimi: PathBuf::from("hayamimi"),
            models: None,
        }
    }
}

/// The work file of one run, removed when it goes out of scope.
struct WorkFile(PathBuf);

impl WorkFile {
    /// A name of this run's own in the cache directory; `None` when the
    /// platform names no cache directory or it cannot be made.
    fn create() -> Option<WorkFile> {
        let dir = dirs::cache_dir()?.join("hayamimi");
        fs::create_dir_all(&dir).ok()?;
        Some(WorkFile(
            dir.join(format!("srt-{}.wav", std::process::id())),
        ))
    }
}

impl Drop for WorkFile {
    fn drop(&mut self) {
        // The file is not there when the audio was never taken out.
        let _ = fs::remove_file(&self.0);
    }
}

fn open_pipeline(llm: &str, sink: &mut dyn LogSink) -> Result<Pipeline<HttpLlm>, String> {
    let config = PipelineConfig {
        rates_path: default_rates_path(),
        tables_dir: default_tables_dir(),
    };
    Pipeline::open(HttpLlm::new(llm), &config, sink)
}

/// The key of the sentence that tells why no file was written.
fn failure_key(error: SrtError) -> &'static str {
    match error {
        SrtError::NoName => "srt_no_name",
        SrtError::Exists => "srt_exists",
        SrtError::Extract => "srt_extract_failed",
        SrtError::Recognize => "srt_recognize_failed",
        SrtError::NoSpeech => "srt_no_speech",
        SrtError::Write => "srt_write_failed",
    }
}

/// Tells the user how the job ended and returns the exit code.
fn say_outcome(outcome: Result<Written, SrtError>, strings: &Strings) -> u8 {
    match outcome {
        Ok(written) => {
            eprintln!(
                "{} ja {}, en {}",
                strings.cli_text("srt_written"),
                written.ja,
                written.en
            );
            EXIT_DONE
        }
        Err(error) => {
            eprintln!("{}", strings.cli_text(failure_key(error)));
            EXIT_FAILED
        }
    }
}

/// Runs the command. Returns the exit code: 0 the files are written, 1 they
/// are not.
pub fn run(plan: SrtPlan, now_epoch: i64, strings: &Strings) -> u8 {
    let Some(work) = WorkFile::create() else {
        eprintln!("{}", strings.cli_text("srt_no_work_dir"));
        return EXIT_FAILED;
    };
    let Ok(japanese) = Strings::embedded(Lang::Ja) else {
        return EXIT_FAILED;
    };
    let mut sink = StdoutSink::new(strings.clone());
    let Ok(mut pipeline) = open_pipeline(&plan.llm, &mut sink) else {
        eprintln!("{}", strings.cli_text("srt_recognize_failed"));
        return EXIT_FAILED;
    };
    let ffmpeg = Ffmpeg {
        program: plan.ffmpeg,
    };
    let recognizer = Recognizer::new(plan.hayamimi, plan.models);
    let job = Job {
        video: &plan.video,
        wav: &work.0,
        extract: &ffmpeg,
        recognize: &recognizer,
        withheld_mark: &japanese.withheld_mark,
        now_epoch,
    };
    say_outcome(make_srt(&job, &mut pipeline, &mut sink), strings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plan_starts_with_the_programs_on_the_search_path() {
        let plan = SrtPlan::new(PathBuf::from("a.mp4"), "http://x:1");
        assert_eq!(plan.ffmpeg, PathBuf::from("ffmpeg"));
        assert_eq!(plan.hayamimi, PathBuf::from("hayamimi"));
        assert_eq!(plan.models, None);
        assert_eq!(plan.llm, "http://x:1");
    }

    #[test]
    fn every_way_the_job_stops_has_a_sentence_in_both_locales() {
        for lang in [Lang::Ja, Lang::En] {
            let Ok(strings) = Strings::embedded(lang) else {
                panic!("embedded table missing");
            };
            for error in [
                SrtError::NoName,
                SrtError::Exists,
                SrtError::Extract,
                SrtError::Recognize,
                SrtError::NoSpeech,
                SrtError::Write,
            ] {
                let key = failure_key(error);
                assert_ne!(strings.cli_text(key), key, "{lang:?} {key}");
            }
            for key in [
                "srt_written",
                "srt_no_work_dir",
                "need_video",
                "send_not_opened",
            ] {
                assert_ne!(strings.cli_text(key), key, "{lang:?} {key}");
            }
        }
    }

    #[test]
    fn a_work_file_is_removed_when_it_goes_out_of_scope() -> std::io::Result<()> {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/unit-tmp");
        fs::create_dir_all(&dir)?;
        let path = dir.join(format!("work-{}.wav", std::process::id()));
        fs::write(&path, b"x")?;
        drop(WorkFile(path.clone()));
        assert!(!path.exists());
        Ok(())
    }
}

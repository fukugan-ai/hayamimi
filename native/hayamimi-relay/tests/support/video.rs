//! A video job with fake steps: nothing is extracted, a list of lines is
//! "recognized", and the translator answers from the text it is asked about.

#![allow(dead_code)]

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use hayamimi_relay::lang::Lang;
use hayamimi_relay::line::{Line, Source};
use hayamimi_relay::llm::{Completion, Llm, LlmError};
use hayamimi_relay::logging::{LogEvent, LogSink, MemorySink};
use hayamimi_relay::pipeline::{LineKind, Pipeline, PipelineConfig};
use hayamimi_relay::strings::Strings;
use hayamimi_relay::video::{Extract, Job, Recognize, SrtError, StepFailed, Written, make_srt};

/// What the Japanese file puts in front of a line whose translation was withheld.
pub fn withheld_mark() -> Result<String, String> {
    Ok(Strings::embedded(Lang::Ja)?.withheld_mark)
}

/// An English line of a video, heard from `start` to `end` seconds.
pub fn english(seq: usize, text: &str, start: f64, end: f64) -> Line {
    heard(seq, Lang::En, text, start, end)
}

/// A Japanese line of a video.
pub fn japanese(seq: usize, text: &str, start: f64, end: f64) -> Line {
    heard(seq, Lang::Ja, text, start, end)
}

fn heard(seq: usize, lang: Lang, text: &str, start: f64, end: f64) -> Line {
    Line {
        kind: LineKind::Final,
        id: format!("file-{seq}"),
        source: Source::File,
        lang: Some(lang),
        text: text.to_owned(),
        span: Some((start, end)),
    }
}

/// Translates English into Japanese; the amount in "100 dollars" comes out
/// wrong, so that V has to withhold that line.
pub struct Translator(pub Arc<AtomicUsize>);

impl Llm for Translator {
    fn complete(&self, request: &Completion<'_>) -> Result<String, LlmError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        let answer = if request.prompt.contains("100 dollars") {
            "私は1500ドル払いました。"
        } else {
            "こんにちは。"
        };
        Ok(answer.to_owned())
    }
}

/// Takes no audio out; counts how often it was asked to.
#[derive(Default)]
pub struct CountingExtract(pub AtomicUsize);

impl Extract for CountingExtract {
    fn extract(&self, _: &Path, _: &Path, _: &mut dyn LogSink) -> Result<(), StepFailed> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

/// "Recognizes" the lines it was given.
pub struct Lines(pub Vec<Line>);

impl Recognize for Lines {
    fn recognize(&self, _: &Path, _: &mut dyn LogSink) -> Result<Vec<Line>, StepFailed> {
        Ok(self.0.clone())
    }
}

/// What a job did.
pub struct Run {
    pub result: Result<Written, SrtError>,
    pub events: Vec<LogEvent>,
    /// How many times the audio was taken out.
    pub extracted: usize,
    /// How many requests the translator got.
    pub asked: usize,
}

impl Run {
    pub fn count(&self, event: &str) -> usize {
        self.events.iter().filter(|e| e.event == event).count()
    }
}

/// Runs the job for `video` over `lines`.
pub fn run_job(video: &Path, lines: Vec<Line>) -> Result<Run, String> {
    let mut sink = MemorySink::default();
    let asked = Arc::new(AtomicUsize::new(0));
    let mut pipeline = Pipeline::open(
        Translator(Arc::clone(&asked)),
        &PipelineConfig::default(),
        &mut sink,
    )?;
    let (extract, recognize) = (CountingExtract::default(), Lines(lines));
    let mark = withheld_mark()?;
    let wav = video.with_extension("wav");
    let job = Job {
        video,
        wav: &wav,
        extract: &extract,
        recognize: &recognize,
        withheld_mark: &mark,
        now_epoch: 1_790_000_000,
    };
    let result = make_srt(&job, &mut pipeline, &mut sink);
    Ok(Run {
        result,
        events: sink.events,
        extracted: extract.0.load(Ordering::SeqCst),
        asked: asked.load(Ordering::SeqCst),
    })
}

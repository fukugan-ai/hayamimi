//! The Japanese route from audio to subtitle text.
//!
//! Audio goes in a window at a time. The detector cuts it into utterances,
//! each gets up to a second of real audio in front of it, the recognizer
//! decodes it, a suspicious decode is retried piece by piece, and the text
//! passes through ITN, punctuation and the replacement dictionary, in that
//! order. Nothing here chooses a language: the route is Japanese only.

use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::audio::{History, SAMPLE_RATE, windows};
use crate::error::AsrError;
use crate::fallback::{Fallback, Gpu, Reason, Stage, To};
use crate::itn::{self, Overrides};
use crate::ort_runtime::{Device, NodeCounts, OrtRuntime};
use crate::punct::{MODEL_FILE as PUNCT_MODEL, Punctuator};
use crate::recognizer::{Recognizer, model_files};
use crate::replace::Replacements;
use crate::retry::{Splitter, looks_truncated};
use crate::vad::{Segment, Vad, VadSettings};

/// The language of the route.
const LANGUAGE: &str = "ja";
/// The file name the VAD session is reported under.
const VAD_MODEL: &str = "silero_vad.onnx";

/// Which optional stages run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stages {
    pub itn: bool,
    pub punctuate: bool,
}

impl Default for Stages {
    fn default() -> Self {
        Stages {
            itn: true,
            punctuate: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Settings {
    /// The directory that holds the model directories.
    pub models: PathBuf,
    pub vad: VadSettings,
    pub stages: Stages,
    pub itn_overrides: Overrides,
    pub replacements: Replacements,
    /// Log the node placement of the punctuation session.
    pub diagnose: bool,
    /// What the start found about the GPU.
    pub gpu: Gpu,
}

impl Settings {
    pub fn new(models: PathBuf) -> Settings {
        Settings {
            models,
            gpu: Gpu::Try,
            vad: VadSettings::default(),
            stages: Stages::default(),
            itn_overrides: Overrides::default(),
            replacements: Replacements::default(),
            diagnose: false,
        }
    }
}

/// One model session the engine created: what was asked for and what ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionReport {
    pub model: String,
    pub requested: &'static str,
    pub actual: &'static str,
    /// The node placement, for a session built through `ort` with `diagnose`.
    pub nodes: Option<NodeCounts>,
}

/// Where the time of one utterance went, in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timings {
    /// The detector's work on the windows since the previous utterance.
    pub vad_ms: f64,
    /// Language identification; the route is Japanese only, so there is none.
    pub lid_ms: f64,
    /// The recognizer, including the split retry.
    pub decode_ms: f64,
    pub punct_ms: f64,
    pub itn_ms: f64,
    /// From the detector handing over the segment to the finished text.
    pub l_dec_ms: f64,
}

/// One finished utterance.
#[derive(Debug, Clone, PartialEq)]
pub struct Final {
    pub id: u64,
    pub text: String,
    /// Seconds of audio decoded, the pre-roll included.
    pub audio_seconds: f64,
    /// Where the utterance starts, in seconds from the start of the audio fed
    /// to the engine; the pre-roll is not counted.
    pub t0_s: f64,
    /// Where the utterance ends, in the same seconds.
    pub t1_s: f64,
    /// When the detector decided the utterance had ended, in milliseconds
    /// since the Unix epoch.
    pub end_wall_ms: u64,
    pub timings: Timings,
    /// Set when punctuation failed and the text went on without it.
    pub punct_error: Option<String>,
}

/// The detector and the recognizer, which only work together.
struct Route {
    recognizer: Recognizer,
    vad: Vad,
    splitter: Splitter,
    history: History,
    vad_ms: f64,
}

pub struct Engine {
    /// `None` when the detector or the recognizer could not be opened: the engine then makes no utterances.
    route: Option<Route>,
    punctuator: Option<Punctuator>,
    stages: Stages,
    itn_overrides: Overrides,
    replacements: Replacements,
    reports: Vec<SessionReport>,
    fallbacks: Vec<Fallback>,
    next_id: u64,
}

fn millis(since: Instant) -> f64 {
    since.elapsed().as_secs_f64() * 1000.0
}

/// Milliseconds since the Unix epoch.
fn epoch_ms() -> Result<u64, AsrError> {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| AsrError::Clock)?;
    u64::try_from(since.as_millis()).map_err(|_| AsrError::Overflow("the epoch milliseconds"))
}

/// Where a segment starts and ends, in seconds from the start of the audio fed
/// to the engine.
fn span_seconds(segment: &Segment) -> (f64, f64) {
    let rate = f64::from(SAMPLE_RATE);
    let start = segment.start as f64 / rate;
    (start, start + segment.samples.len() as f64 / rate)
}

fn cpu_report(model: String) -> SessionReport {
    SessionReport {
        model,
        requested: Device::Cpu.name(),
        actual: Device::Cpu.name(),
        nodes: None,
    }
}

/// The file names of the sessions sherpa-onnx creates for the recognizer.
fn recognizer_models(settings: &Settings) -> Vec<String> {
    model_files(&settings.models)
        .iter()
        .filter_map(|path| path.file_name())
        .filter_map(|name| name.to_str())
        .filter(|name| name.ends_with(".onnx"))
        .map(str::to_owned)
        .collect()
}

/// Why a model that failed to open leaves its stage out, or `None` when the
/// failure is not about the model (and so must not be absorbed).
fn disable_reason(error: &AsrError) -> Option<Reason> {
    match error {
        AsrError::MissingModel(_) => Some(Reason::Missing),
        AsrError::Create(_) | AsrError::Ort(_) | AsrError::Vocab(_) | AsrError::Io(_) => {
            Some(Reason::Unreadable)
        }
        _ => None,
    }
}

fn disabled(stage: Stage, reason: Reason) -> Fallback {
    Fallback {
        stage,
        reason,
        to: To::Disabled,
    }
}

/// A stage opened, or the fallback that leaves it out. An error that is not
/// about the model is returned.
fn open_or_disable<T>(
    opened: Result<T, AsrError>,
    stage: Stage,
    fallbacks: &mut Vec<Fallback>,
) -> Result<Option<T>, AsrError> {
    match opened {
        Ok(value) => Ok(Some(value)),
        Err(error) => match disable_reason(&error) {
            Some(reason) => {
                fallbacks.push(disabled(stage, reason));
                Ok(None)
            }
            None => Err(error),
        },
    }
}

/// Opens the detector and the recognizer, which are of use only together: if
/// either is out, there is no route. The sessions that were built are added to `reports`.
fn open_route(
    settings: &Settings,
    reports: &mut Vec<SessionReport>,
    fallbacks: &mut Vec<Fallback>,
) -> Result<Option<Route>, AsrError> {
    let vad = open_or_disable(
        Vad::open(&settings.models, settings.vad),
        Stage::Vad,
        fallbacks,
    )?;
    let recognizer = open_or_disable(
        Recognizer::open(&settings.models),
        Stage::Recognizer,
        fallbacks,
    )?;
    let (Some(vad), Some(recognizer)) = (vad, recognizer) else {
        return Ok(None);
    };
    reports.push(cpu_report(String::from(VAD_MODEL)));
    reports.extend(recognizer_models(settings).into_iter().map(cpu_report));
    Ok(Some(Route {
        recognizer,
        vad,
        splitter: Splitter::new(&settings.models),
        history: History::new(),
        vad_ms: 0.0,
    }))
}

/// The device the punctuation model is tried on first. A start that found no
/// GPU goes straight to the CPU and says so.
fn first_device(gpu: Gpu, fallbacks: &mut Vec<Fallback>) -> Device {
    match gpu {
        Gpu::Try => Device::Cuda,
        Gpu::Absent(reason) => {
            fallbacks.push(Fallback {
                stage: Stage::Punct,
                reason,
                to: To::Cpu,
            });
            Device::Cpu
        }
    }
}

/// Opens the punctuation model on `device`, and on the CPU when the CUDA
/// session cannot be built. Returns the attempt with the device that ran.
fn open_on_device(
    runtime: &OrtRuntime,
    settings: &Settings,
    device: Device,
    fallbacks: &mut Vec<Fallback>,
) -> (Result<Punctuator, AsrError>, Device) {
    let open = |device| Punctuator::open(runtime, &settings.models, device, settings.diagnose);
    let attempt = open(device);
    if device == Device::Cuda && matches!(attempt, Err(AsrError::Ort(_))) {
        fallbacks.push(Fallback {
            stage: Stage::Punct,
            reason: Reason::SessionFailed,
            to: To::Cpu,
        });
        return (open(Device::Cpu), Device::Cpu);
    }
    (attempt, device)
}

/// The report of the punctuation session that ran on `device`.
fn punct_report(
    runtime: &OrtRuntime,
    settings: &Settings,
    device: Device,
) -> Result<SessionReport, AsrError> {
    let nodes = if settings.diagnose {
        Some(runtime.take_node_counts()?)
    } else {
        None
    };
    Ok(SessionReport {
        model: String::from(PUNCT_MODEL),
        requested: Device::Cuda.name(),
        actual: device.name(),
        nodes,
    })
}

/// Opens the punctuation model where `settings.gpu` says, and on the CPU when
/// the CUDA session cannot be built. The model that cannot be opened at all
/// leaves the stage out. Returns the session's report with the punctuator.
fn open_punct(
    runtime: &OrtRuntime,
    settings: &Settings,
    fallbacks: &mut Vec<Fallback>,
) -> Result<Option<(Punctuator, SessionReport)>, AsrError> {
    let first = first_device(settings.gpu, fallbacks);
    let (attempt, device) = open_on_device(runtime, settings, first, fallbacks);
    let Some(punctuator) = open_or_disable(attempt, Stage::Punct, fallbacks)? else {
        return Ok(None);
    };
    let report = punct_report(runtime, settings, device)?;
    Ok(Some((punctuator, report)))
}

impl Engine {
    /// Opens every model. A stage whose model is missing or unreadable is left
    /// out, a punctuation session that cannot run on the GPU runs on the CPU,
    /// and each such step is listed in [`Engine::fallbacks`]. Only a failure
    /// that is not about a model is an error.
    pub fn open(settings: Settings) -> Result<Engine, AsrError> {
        // The first sherpa-onnx call must come after the ONNX Runtime beside
        // the executable is loaded (Windows); opening the runtime does that.
        let runtime = OrtRuntime::shared()?;
        let mut fallbacks = Vec::new();
        let mut reports = Vec::new();
        let route = open_route(&settings, &mut reports, &mut fallbacks)?;
        let mut punctuator = None;
        if settings.stages.punctuate
            && let Some((opened, report)) = open_punct(runtime, &settings, &mut fallbacks)?
        {
            reports.push(report);
            punctuator = Some(opened);
        }
        Ok(Engine {
            route,
            punctuator,
            stages: settings.stages,
            itn_overrides: settings.itn_overrides,
            replacements: settings.replacements,
            reports,
            fallbacks,
            next_id: 1,
        })
    }

    /// The sessions created while opening, in creation order.
    pub fn sessions(&self) -> &[SessionReport] {
        &self.reports
    }

    /// The stages that did not run as designed, in the order they fell back.
    pub fn fallbacks(&self) -> &[Fallback] {
        &self.fallbacks
    }

    /// Feeds one window of `WINDOW` samples; returns the utterances it finished.
    pub fn feed(&mut self, window: &[f32]) -> Result<Vec<Final>, AsrError> {
        let Some(route) = self.route.as_mut() else {
            return Ok(Vec::new());
        };
        route.history.push(window);
        let started = Instant::now();
        route.vad.accept(window);
        let segments = route.vad.take_segments()?;
        route.vad_ms += millis(started);
        self.finish_segments(segments)
    }

    /// Ends the speech in progress and returns what it made.
    pub fn finish(&mut self) -> Result<Vec<Final>, AsrError> {
        let Some(route) = self.route.as_mut() else {
            return Ok(Vec::new());
        };
        let started = Instant::now();
        route.vad.flush();
        let segments = route.vad.take_segments()?;
        route.vad_ms += millis(started);
        self.finish_segments(segments)
    }

    /// A whole recording at once, without pacing.
    pub fn transcribe(&mut self, samples: &[f32]) -> Result<Vec<Final>, AsrError> {
        let mut finals = Vec::new();
        for window in windows(samples) {
            finals.extend(self.feed(&window)?);
        }
        finals.extend(self.finish()?);
        Ok(finals)
    }

    fn finish_segments(&mut self, segments: Vec<Segment>) -> Result<Vec<Final>, AsrError> {
        if segments.is_empty() {
            return Ok(Vec::new());
        }
        // The detector handed these segments over just now.
        let ended_wall_ms = epoch_ms()?;
        let mut finals = Vec::new();
        for segment in segments {
            if let Some(done) = self.process(&segment, ended_wall_ms)? {
                finals.push(done);
            }
        }
        Ok(finals)
    }

    /// Decodes the whole text of one segment: recognition, retry, ITN,
    /// punctuation, dictionary. A segment with no speech makes no final.
    fn process(
        &mut self,
        segment: &Segment,
        ended_wall_ms: u64,
    ) -> Result<Option<Final>, AsrError> {
        let started = Instant::now();
        let Some(route) = self.route.as_mut() else {
            return Ok(None);
        };
        let decoded = decode_segment(route, segment)?;
        let vad_ms = route.vad_ms;
        if decoded.text.trim().is_empty() {
            return Ok(None);
        }
        let (text, itn_ms) = self.normalize(decoded.text);
        let ((text, punct_error), punct_ms) = timed(|| self.punctuate(text));
        let text = self.replacements.apply(&text);
        if text.trim().is_empty() {
            return Ok(None);
        }
        // The detector's time is handed on with the utterance it led to.
        if let Some(route) = self.route.as_mut() {
            route.vad_ms = 0.0;
        }
        let id = self.next_id;
        self.next_id += 1;
        let (t0_s, t1_s) = span_seconds(segment);
        Ok(Some(Final {
            id,
            text,
            audio_seconds: decoded.seconds,
            t0_s,
            t1_s,
            end_wall_ms: ended_wall_ms,
            timings: Timings {
                vad_ms,
                lid_ms: 0.0,
                decode_ms: decoded.decode_ms,
                punct_ms,
                itn_ms,
                l_dec_ms: millis(started),
            },
            punct_error,
        }))
    }

    /// Inverse text normalisation, when the stage is on, and the time it took.
    fn normalize(&self, text: String) -> (String, f64) {
        if !self.stages.itn {
            return (text, 0.0);
        }
        timed(|| itn::convert(&text, LANGUAGE, &self.itn_overrides))
    }

    /// Punctuation, when there is a model. A failure must not lose the
    /// transcription: the text goes on without marks and the failure is
    /// handed to the caller.
    fn punctuate(&mut self, text: String) -> (String, Option<String>) {
        let Some(punctuator) = self.punctuator.as_mut() else {
            return (text, None);
        };
        match punctuator.restore(&text) {
            Ok(punctuated) => (punctuated, None),
            Err(error) => (text, Some(error.to_string())),
        }
    }
}

/// What the recognizer made of one segment.
struct Decoded {
    text: String,
    /// Seconds of audio decoded, the pre-roll included.
    seconds: f64,
    decode_ms: f64,
}

/// Decodes a segment with its pre-roll, and again piece by piece when the
/// text looks cut short.
fn decode_segment(route: &mut Route, segment: &Segment) -> Result<Decoded, AsrError> {
    let samples = route.history.with_preroll(segment.start, &segment.samples);
    let seconds = samples.len() as f64 / f64::from(SAMPLE_RATE);
    let (text, decode_ms) = timed(|| -> Result<String, AsrError> {
        let text = route.recognizer.decode(&samples)?;
        if looks_truncated(&text, seconds, seconds) {
            return route.splitter.retry(&route.recognizer, &text, &samples);
        }
        Ok(text)
    });
    Ok(Decoded {
        text: text?,
        seconds,
        decode_ms,
    })
}

/// Runs `work` and returns its result with the milliseconds it took.
fn timed<T>(work: impl FnOnce() -> T) -> (T, f64) {
    let started = Instant::now();
    let value = work();
    (value, millis(started))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_stage_runs_by_default() {
        assert_eq!(
            Stages::default(),
            Stages {
                itn: true,
                punctuate: true
            }
        );
    }

    #[test]
    fn settings_start_with_the_measured_vad_and_no_dictionary() {
        let settings = Settings::new(PathBuf::from("models"));
        assert_eq!(settings.vad, VadSettings::default());
        assert!(settings.replacements.is_empty());
        assert!(!settings.diagnose);
    }

    #[test]
    fn the_recognizer_is_reported_as_three_sessions() {
        let names = recognizer_models(&Settings::new(PathBuf::from("models")));
        assert_eq!(
            names,
            [
                "encoder-epoch-35-avg-1.int8.onnx",
                "decoder-epoch-35-avg-1.int8.onnx",
                "joiner-epoch-35-avg-1.int8.onnx"
            ]
        );
    }

    #[test]
    fn a_cpu_session_asked_for_the_cpu_and_got_it() {
        let report = cpu_report(String::from("x.onnx"));
        assert_eq!((report.requested, report.actual), ("cpu", "cpu"));
        assert!(report.nodes.is_none());
    }

    #[test]
    fn a_missing_model_directory_leaves_every_stage_out_and_the_engine_silent()
    -> Result<(), AsrError> {
        let mut engine = Engine::open(Settings::new(PathBuf::from("no-such-models")))?;
        let left_out: Vec<(&str, &str)> = engine
            .fallbacks()
            .iter()
            .map(|fallback| (fallback.stage.name(), fallback.to.name()))
            .collect();
        assert_eq!(
            left_out,
            [
                ("vad", "disabled"),
                ("recognizer", "disabled"),
                ("punct", "disabled")
            ]
        );
        assert!(engine.sessions().is_empty());
        assert!(engine.feed(&[0.0; crate::audio::WINDOW])?.is_empty());
        assert!(engine.finish()?.is_empty());
        Ok(())
    }

    #[test]
    fn the_failures_that_are_about_a_model_leave_a_stage_out_and_the_others_do_not() {
        assert_eq!(
            disable_reason(&AsrError::MissingModel(PathBuf::from("m"))),
            Some(Reason::Missing)
        );
        assert_eq!(
            disable_reason(&AsrError::Create("VAD")),
            Some(Reason::Unreadable)
        );
        assert_eq!(disable_reason(&AsrError::Wav(String::from("w"))), None);
        assert_eq!(disable_reason(&AsrError::Overflow("n")), None);
    }

    #[test]
    fn a_segment_spans_from_its_start_sample_to_its_last() {
        let segment = Segment {
            start: 8_000,
            samples: vec![0.0; 16_000],
        };
        assert_eq!(span_seconds(&segment), (0.5, 1.5));
    }

    #[test]
    fn an_empty_segment_at_the_start_spans_nothing() {
        let segment = Segment {
            start: 0,
            samples: Vec::new(),
        };
        assert_eq!(span_seconds(&segment), (0.0, 0.0));
    }

    #[test]
    fn the_clock_reads_a_time_after_2026() -> Result<(), AsrError> {
        // 2026-01-01T00:00:00Z
        assert!(epoch_ms()? > 1_767_225_600_000);
        Ok(())
    }

    #[test]
    fn a_stage_that_opened_has_no_fallback() -> Result<(), AsrError> {
        let mut fallbacks = Vec::new();
        let opened = open_or_disable(Ok(7), Stage::Vad, &mut fallbacks)?;
        assert_eq!(opened, Some(7));
        assert!(fallbacks.is_empty());
        Ok(())
    }
}

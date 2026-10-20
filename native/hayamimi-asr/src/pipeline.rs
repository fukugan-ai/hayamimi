//! The Japanese route from audio to subtitle text.
//!
//! Audio goes in a window at a time. The detector cuts it into utterances,
//! each gets up to a second of real audio in front of it, the recognizer
//! decodes it, a suspicious decode is retried piece by piece, and the text
//! passes through ITN, punctuation and the replacement dictionary, in that
//! order. Nothing here chooses a language: the route is Japanese only.

use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::audio::{History, SAMPLE_RATE, WINDOW, windows};
use crate::decode::{Recognizers, ResidentEvent};
use crate::endpoint::{self, Decision, Hold, Mode, Plan, Rule, State, Tail};
use crate::error::AsrError;
use crate::fallback::{Fallback, Gpu, Reason, Stage, To, reason_of};
use crate::itn::{self, Overrides};
use crate::lid::{self, Lid};
use crate::ort_runtime::{Device, NodeCounts, OrtRuntime};
use crate::partial::{self, DraftClock};
use crate::punct::{MODEL_FILE as PUNCT_MODEL, Punctuator};
use crate::recognizer::{THREADS, model_files};
use crate::replace::Replacements;
use crate::route::{Policy, Request, RouteError, RouteNotice, Router, Tier, requested_tier};
use crate::speaker_stage::{Mark, SpeakerStage};
use crate::vad::{EARLY_MIN_SILENCE_MS, Segment, Sensitivity, Vad, VadSettings};

/// The language the punctuation model restores marks for.
const PUNCTUATED_LANGUAGE: &str = "ja";
/// The file name the VAD session is reported under.
const VAD_MODEL: &str = "silero_vad.onnx";
/// How many recognizers besides the Japanese one stay in memory unless the
/// command line says otherwise: Parakeet v3, Paraformer and SenseVoice are
/// the three the five languages of the evaluation use.
pub const DEFAULT_MAX_RESIDENT: Option<NonZeroUsize> = NonZeroUsize::new(3);
/// The file name the speaker embedding session is reported under.
const SPEAKER_MODEL: &str = crate::embedder::MODEL_FILE;

/// Which optional stages run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stages {
    pub itn: bool,
    pub punctuate: bool,
    /// The speaker mark of each utterance.
    pub speakers: bool,
}

impl Default for Stages {
    fn default() -> Self {
        Stages {
            itn: true,
            punctuate: true,
            speakers: true,
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
    /// How many recognizers besides the Japanese one stay in memory; `None` keeps them all.
    pub max_resident: Option<NonZeroUsize>,
    /// How languages are chosen. A policy that forces a language opens no
    /// language identifier.
    pub policy: Policy,
    /// How the end of an utterance is decided.
    pub endpoint: Mode,
}

impl Settings {
    /// Settings for Japanese only; a caller that wants languages detected sets `policy`.
    pub fn new(models: PathBuf) -> Settings {
        Settings {
            models,
            policy: Policy::single("ja"),
            gpu: Gpu::Try,
            max_resident: DEFAULT_MAX_RESIDENT,
            vad: VadSettings::default(),
            stages: Stages::default(),
            itn_overrides: Overrides::default(),
            replacements: Replacements::default(),
            diagnose: false,
            // The library keeps the silence rule; the `hayamimi` command line asks for the early one.
            endpoint: Mode::Silence,
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
    /// Language identification, SenseVoice's confirmation probe included; 0
    /// when the language is forced.
    pub lid_ms: f64,
    /// The recognizer, including the split retry.
    pub decode_ms: f64,
    pub punct_ms: f64,
    pub itn_ms: f64,
    /// From the detector handing over the segment to the finished text.
    pub l_dec_ms: f64,
}

/// An early notice that read a sentence and kept it instead of finalising it:
/// the sentence was not complete, or the speech behind it was shorter than
/// [`crate::vad::EARLY_MIN_SEGMENT_MS`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EarlyHold {
    /// Why the sentence was kept.
    pub reason: Hold,
    /// The speech the sentence was read from, in milliseconds.
    pub span_ms: f64,
    /// The form the sentence ends in. The sentence itself is not kept.
    pub tail: Tail,
    /// Whether a rule that counts a noun or an honorific ending as a sentence
    /// end would have finalised the sentence there. The live rule is unchanged.
    pub shadow_complete: bool,
}

impl EarlyHold {
    /// The hold of the sentence `text`, read in `lang` from `samples` samples of speech.
    fn new(reason: Hold, samples: usize, lang: &str, text: &str) -> EarlyHold {
        EarlyHold {
            reason,
            span_ms: samples_ms(samples),
            tail: endpoint::tail_of(lang, text),
            shadow_complete: endpoint::shadow_early_hold(lang, text, samples).is_none(),
        }
    }
}

/// One finished utterance.
#[derive(Debug, Clone, PartialEq)]
pub struct Final {
    pub id: u64,
    pub text: String,
    /// The language the utterance was read in.
    pub lang: String,
    /// The name of the recognizer that was asked for the language.
    pub tier: &'static str,
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
    /// The 16 kHz samples of the utterance, kept only while the speaker stage
    /// is on, for [`Engine::mark_speaker`]. The `final` does not wait for the mark.
    pub speaker_audio: Vec<f32>,
    /// The rule that ended the utterance.
    pub endpoint: Decision,
}

/// What reading a stretch of audio made: the text and where the time went.
#[derive(Debug, Clone, PartialEq)]
struct Solved {
    text: String,
    lang: String,
    tier: &'static str,
    /// Seconds of audio decoded, the pre-roll included.
    seconds: f64,
    lid_ms: f64,
    decode_ms: f64,
    itn_ms: f64,
    punct_ms: f64,
    punct_error: Option<String>,
    l_dec_ms: f64,
}

/// The detector and the recognizers, which only work together.
struct Route {
    router: Router<Recognizers, Lid>,
    vad: Vad,
    history: History,
    vad_ms: f64,
    draft: DraftClock,
    /// What the early detector has finalised inside the voice in progress.
    early: State<Solved>,
    /// The sentences the early path kept, until they are taken.
    early_holds: Vec<EarlyHold>,
}

/// One GPU stage moved to the CPU to give memory back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Demotion {
    pub fallback: Fallback,
    /// The session built on the CPU, when the model could be opened there.
    pub report: Option<SessionReport>,
}

pub struct Engine {
    /// `None` when the detector or the recognizer could not be opened: the engine then makes no utterances.
    route: Option<Route>,
    punctuator: Option<Punctuator>,
    /// Where the punctuation session runs now.
    punct_device: Option<Device>,
    models: PathBuf,
    /// `None` when the stage is off or its model is not there: utterances then carry no mark.
    speakers: Option<SpeakerStage>,
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

/// The milliseconds `samples` samples last.
fn samples_ms(samples: usize) -> f64 {
    samples as f64 * 1000.0 / f64::from(SAMPLE_RATE)
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
        Err(error) => match reason_of(&error) {
            Some(reason) => {
                fallbacks.push(disabled(stage, reason));
                Ok(None)
            }
            None => Err(error),
        },
    }
}

/// Opens the language identifier unless the policy forces a language, in
/// which case there is nothing to identify. A stage that fell back or was
/// left out is added to `fallbacks`, and its sessions to `reports`.
fn open_identifier(
    settings: &Settings,
    reports: &mut Vec<SessionReport>,
    fallbacks: &mut Vec<Fallback>,
) -> Result<Option<Lid>, AsrError> {
    if settings.policy.forced_lang.is_some() {
        return Ok(None);
    }
    let opened = lid::open(&settings.models, settings.gpu)?;
    reports.extend(opened.sessions);
    fallbacks.extend(opened.fallback);
    Ok(opened.lid)
}

/// The English recognizer, when the session may read English: one forced to
/// English, or one that identifies the language. Its model takes seconds to
/// load, so it is built before the session starts and the first English
/// utterance does not wait for it. A session forced to another language never
/// reads English and loads nothing more.
fn english_tier_to_preload(policy: &Policy) -> Option<Tier> {
    let may_read_english = policy
        .forced_lang
        .as_deref()
        .is_none_or(|lang| lang == "en");
    may_read_english.then(|| requested_tier("en", policy.en_tier))
}

/// Opens the detector and the recognizer, which are of use only together: if
/// either is out, there is no route. The sessions that were built are added to `reports`.
fn open_route(
    settings: &Settings,
    reports: &mut Vec<SessionReport>,
    fallbacks: &mut Vec<Fallback>,
) -> Result<Option<Route>, AsrError> {
    let opened = match settings.endpoint {
        Mode::Early => Vad::open_with_early(&settings.models, settings.vad),
        Mode::Silence => Vad::open(&settings.models, settings.vad),
    };
    let vad = open_or_disable(opened, Stage::Vad, fallbacks)?;
    let mut recognizers = Recognizers::new(&settings.models, THREADS, settings.max_resident);
    let recognizer_out = recognizers.load(Tier::Rz)?;
    if let Some(reason) = recognizer_out {
        fallbacks.push(disabled(Stage::Recognizer, reason));
    }
    let (Some(vad), None) = (vad, recognizer_out) else {
        return Ok(None);
    };
    reports.push(cpu_report(String::from(VAD_MODEL)));
    reports.extend(recognizer_models(settings).into_iter().map(cpu_report));
    let identifier = open_identifier(settings, reports, fallbacks)?;
    let mut router = Router::new(recognizers, identifier, settings.policy.clone());
    if let Some(tier) = english_tier_to_preload(&settings.policy) {
        // A model that cannot be loaded is remembered as missing and the first
        // utterance in English is read by the tier that answers for it, as it
        // is said then; the session does not stop here.
        router.prefetch(tier);
    }
    Ok(Some(Route {
        router,
        vad,
        history: History::new(),
        vad_ms: 0.0,
        draft: DraftClock::new(),
        early: State::default(),
        early_holds: Vec::new(),
    }))
}

/// The device the punctuation model is tried on first. A start that found no
/// GPU goes straight to the CPU and says so.
fn first_device(gpu: Gpu, fallbacks: &mut Vec<Fallback>) -> Device {
    match gpu.confirmed() {
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
/// leaves the stage out. Returns the punctuator with the device it runs on
/// and its session's report.
fn open_punct(
    runtime: &OrtRuntime,
    settings: &Settings,
    fallbacks: &mut Vec<Fallback>,
) -> Result<Option<(Punctuator, Device, SessionReport)>, AsrError> {
    let first = first_device(settings.gpu, fallbacks);
    let (attempt, device) = open_on_device(runtime, settings, first, fallbacks);
    let Some(punctuator) = open_or_disable(attempt, Stage::Punct, fallbacks)? else {
        return Ok(None);
    };
    let report = punct_report(runtime, settings, device)?;
    Ok(Some((punctuator, device, report)))
}

/// Opens the speaker stage when it is on. Its model runs on the CPU (design
/// 3.5); a model that is missing or unreadable leaves the stage out.
fn open_speakers(
    settings: &Settings,
    reports: &mut Vec<SessionReport>,
    fallbacks: &mut Vec<Fallback>,
) -> Result<Option<SpeakerStage>, AsrError> {
    if !settings.stages.speakers {
        return Ok(None);
    }
    let opened = open_or_disable(
        SpeakerStage::open(&settings.models),
        Stage::Speaker,
        fallbacks,
    )?;
    if opened.is_some() {
        reports.push(cpu_report(String::from(SPEAKER_MODEL)));
    }
    Ok(opened)
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
        let mut punct_device = None;
        if settings.stages.punctuate
            && let Some((opened, device, report)) = open_punct(runtime, &settings, &mut fallbacks)?
        {
            reports.push(report);
            punctuator = Some(opened);
            punct_device = Some(device);
        }
        let speakers = open_speakers(&settings, &mut reports, &mut fallbacks)?;
        Ok(Engine {
            route,
            punctuator,
            punct_device,
            models: settings.models,
            speakers,
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

    /// What the recognizers did since the last call: models loaded and
    /// dropped, and tiers that gave way to others. An engine with no route
    /// has no recognizers and so nothing to say.
    pub fn take_resident_events(&mut self) -> Vec<ResidentEvent> {
        self.route
            .as_mut()
            .into_iter()
            .flat_map(|route| route.router.decoder_mut().take_events())
            .collect()
    }

    /// The steps the language route took off the design since the last call
    /// (a tier that gave way to another, a stage left out).
    pub fn take_route_notices(&mut self) -> Vec<RouteNotice> {
        self.route
            .as_mut()
            .into_iter()
            .flat_map(|route| route.router.take_notices())
            .collect()
    }

    /// The sentences the early path kept since the last call, each with the
    /// reason. An engine with no route has none.
    pub fn take_early_holds(&mut self) -> Vec<EarlyHold> {
        self.route
            .as_mut()
            .into_iter()
            .flat_map(|route| std::mem::take(&mut route.early_holds))
            .collect()
    }

    /// The recognizer tiers in memory.
    pub fn resident(&self) -> Vec<Tier> {
        self.route
            .as_ref()
            .into_iter()
            .flat_map(|route| route.router.decoder().resident())
            .collect()
    }

    /// Gives GPU memory back by moving the next GPU stage to the CPU: the
    /// stage whose move costs the least latency first (punctuation, 8 ms on
    /// the GPU against 45 ms on the CPU). `None` when no stage is on the GPU.
    /// The model that cannot be opened on the CPU leaves its stage out.
    pub fn demote_for_memory(&mut self) -> Result<Option<Demotion>, AsrError> {
        if self.punct_device != Some(Device::Cuda) {
            return Ok(None);
        }
        // The GPU session is dropped before the CPU one is built, so that its memory is free first.
        self.punctuator = None;
        self.punct_device = None;
        let runtime = OrtRuntime::shared()?;
        let opened = Punctuator::open(runtime, &self.models, Device::Cpu, false);
        let mut left_out = Vec::new();
        let Some(punctuator) = open_or_disable(opened, Stage::Punct, &mut left_out)? else {
            let fallback = disabled(Stage::Punct, Reason::MemoryBudget);
            self.fallbacks.push(fallback);
            return Ok(Some(Demotion {
                fallback,
                report: None,
            }));
        };
        self.punctuator = Some(punctuator);
        self.punct_device = Some(Device::Cpu);
        let fallback = Fallback {
            stage: Stage::Punct,
            reason: Reason::MemoryBudget,
            to: To::Cpu,
        };
        let report = SessionReport {
            model: String::from(PUNCT_MODEL),
            requested: Device::Cuda.name(),
            actual: Device::Cpu.name(),
            nodes: None,
        };
        self.fallbacks.push(fallback);
        self.reports.push(report.clone());
        Ok(Some(Demotion {
            fallback,
            report: Some(report),
        }))
    }

    /// Feeds one window of `WINDOW` samples; returns the utterances it finished.
    pub fn feed(&mut self, window: &[f32]) -> Result<Vec<Final>, AsrError> {
        let Some(route) = self.route.as_mut() else {
            return Ok(Vec::new());
        };
        route.history.push(window);
        let window_start = route.history.end() - window.len();
        let started = Instant::now();
        route.vad.accept(window);
        let notices = route.vad.take_early()?;
        let segments = route.vad.take_segments()?;
        route.vad_ms += millis(started);
        let mut finals = self.early_notices(notices)?;
        finals.extend(self.finish_segments(segments, false)?);
        if let Some(route) = self.route.as_mut() {
            route.draft.observe(route.vad.detected(), window_start);
        }
        Ok(finals)
    }

    /// A draft of the utterance in progress, when one is due: the recognizer
    /// and the replacement dictionary only, with no ITN and no punctuation.
    /// Nothing the final needs is touched. `None` when nothing is due, when
    /// the draft says nothing, and when there is no route.
    pub fn partial(&mut self) -> Result<Option<String>, AsrError> {
        let Some(route) = self.route.as_mut() else {
            return Ok(None);
        };
        let Some(start) = route.draft.due(route.history.end()) else {
            return Ok(None);
        };
        let samples = route.history.draft(start, partial::MAX_SAMPLES);
        // A draft is read in the language of the session; before the first
        // final there is none, and the Japanese tier answers.
        let tier = match route.router.last_lang() {
            Some(lang) => requested_tier(lang, route.router.policy().en_tier),
            None => Tier::Rz,
        };
        let drafted = route.router.decoder_mut().decode_whole(tier, &samples)?;
        let text = self.replacements.apply(&drafted.text);
        Ok((!text.trim().is_empty()).then_some(text))
    }

    /// Replaces the user's replacement dictionary; the next utterance uses it.
    pub fn set_replacements(&mut self, replacements: Replacements) {
        self.replacements = replacements;
    }

    /// Replaces the user's ITN overrides; the next utterance uses them.
    pub fn set_itn_overrides(&mut self, overrides: Overrides) {
        self.itn_overrides = overrides;
    }

    /// Turns punctuation on or off for the next utterance. The model stays
    /// loaded when it is turned off. Turning it on does nothing when the run
    /// started without the model, which [`Engine::can_punctuate`] tells.
    pub fn set_punctuate(&mut self, on: bool) {
        self.stages.punctuate = on;
    }

    /// Whether the punctuation model is loaded, so that [`Engine::set_punctuate`] can take effect.
    pub fn can_punctuate(&self) -> bool {
        self.punctuator.is_some()
    }

    /// What the detector is set to, or `None` without a route.
    pub fn sensitivity(&self) -> Option<Sensitivity> {
        self.route.as_ref().map(|route| route.vad.sensitivity())
    }

    /// Moves the detector to `wanted` when that loses no audio, which is
    /// when it is idle. Returns whether it was moved; a `false` is asked
    /// again at the next window.
    pub fn retune_vad(&mut self, wanted: Sensitivity) -> Result<bool, AsrError> {
        let Some(route) = self.route.as_mut() else {
            return Ok(false);
        };
        if route.vad.sensitivity() == wanted {
            return Ok(true);
        }
        if !route.vad.is_idle() {
            return Ok(false);
        }
        route.vad.rebuild(wanted)?;
        Ok(true)
    }

    /// Ends the speech in progress and returns what it made.
    pub fn finish(&mut self) -> Result<Vec<Final>, AsrError> {
        let Some(route) = self.route.as_mut() else {
            return Ok(Vec::new());
        };
        let started = Instant::now();
        route.vad.flush();
        for notice in route.vad.take_early()? {
            route.early.note_voice(notice.start);
        }
        let segments = route.vad.take_segments()?;
        // A detector aborts when it is fed after a `flush` inside a stream, so
        // the next audio goes to new ones, both of them. Nothing is in
        // progress or waiting.
        route.vad.rebuild(route.vad.sensitivity())?;
        route.vad_ms += millis(started);
        let finals = self.finish_segments(segments, true);
        if let Some(route) = self.route.as_mut() {
            route.early.clear();
        }
        finals
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

    /// Takes the stretches of voice the early detector finished: notes that
    /// voice went on, and, while the main detector is still inside speech,
    /// reads what has been said since the line.
    fn early_notices(&mut self, notices: Vec<Segment>) -> Result<Vec<Final>, AsrError> {
        let mut finals = Vec::new();
        for notice in notices {
            let Some(route) = self.route.as_mut() else {
                break;
            };
            route.early.note_voice(notice.start);
            if route.vad.detected()
                && let Some(done) = self.consider(&notice)?
            {
                finals.push(done);
            }
        }
        Ok(finals)
    }

    /// Reads the speech since the line (or since the speech began) at a
    /// notice of the early detector. A complete sentence is finalised there;
    /// any other is kept for the moment the main detector ends the utterance.
    fn consider(&mut self, notice: &Segment) -> Result<Option<Final>, AsrError> {
        let Some(route) = self.route.as_mut() else {
            return Ok(None);
        };
        let floor = notice.start + notice.samples.len();
        let now = route.history.end();
        let (from, samples) = match route.early.line() {
            Some(line) => (line.at, route.history.slice(line.at, now)),
            None => {
                let Some(start) = route.draft.speech_start() else {
                    return Ok(None);
                };
                (start, route.history.draft(start, usize::MAX))
            }
        };
        if samples.is_empty() {
            return Ok(None);
        }
        let span = now.saturating_sub(from);
        let Some(solved) = self.solve(&samples, span)? else {
            return Ok(None);
        };
        let Some(route) = self.route.as_mut() else {
            return Ok(None);
        };
        if let Some(hold) = endpoint::early_hold(&solved.lang, &solved.text, span) {
            route
                .early_holds
                .push(EarlyHold::new(hold, span, &solved.lang, &solved.text));
            route.early.hold(solved, now, floor);
            return Ok(None);
        }
        let speaker_audio = if self.speakers.is_some() {
            route.history.slice(from, now)
        } else {
            Vec::new()
        };
        route.early.confirm(now, floor);
        route.history.mark_segment_end(now);
        route.draft.restart_at(now);
        let ended_wall_ms = epoch_ms()?;
        let rate = f64::from(SAMPLE_RATE);
        let decision = Decision {
            rule: Rule::Early,
            silence_ms: EARLY_MIN_SILENCE_MS,
            plan: None,
            held_gap_ms: None,
        };
        let span = (from as f64 / rate, now as f64 / rate);
        Ok(self.finalize(solved, span, speaker_audio, ended_wall_ms, decision))
    }

    fn finish_segments(
        &mut self,
        segments: Vec<Segment>,
        flushed: bool,
    ) -> Result<Vec<Final>, AsrError> {
        if segments.is_empty() {
            return Ok(Vec::new());
        }
        // The detector handed these segments over just now.
        let ended_wall_ms = epoch_ms()?;
        let mut finals = Vec::new();
        for segment in segments {
            if let Some(done) = self.on_segment(segment, ended_wall_ms, flushed)? {
                finals.push(done);
            }
        }
        Ok(finals)
    }

    /// A segment of the main detector: read whole, or only what the early
    /// path has not finalised, or not at all when it has finalised it all.
    fn on_segment(
        &mut self,
        segment: Segment,
        ended_wall_ms: u64,
        flushed: bool,
    ) -> Result<Option<Final>, AsrError> {
        let Some(route) = self.route.as_mut() else {
            return Ok(None);
        };
        let decision = decision_of(&segment, route.vad.sensitivity(), flushed);
        let end = segment.start + segment.samples.len();
        let (plan, gap) = route.early.plan_with_gap(segment.start, end, WINDOW);
        let decision = endpoint::note_plan(decision, &plan, gap);
        match plan {
            Plan::Whole => self.process(segment, ended_wall_ms, decision),
            Plan::Drop => {
                route.history.mark_segment_end(end);
                Ok(None)
            }
            Plan::Rest(line) => {
                // INVARIANT: the plan returns `Rest` only for a line inside the segment.
                let samples = segment.samples[line - segment.start..].to_vec();
                self.process(
                    Segment {
                        start: line,
                        samples,
                    },
                    ended_wall_ms,
                    decision,
                )
            }
            Plan::Held(held, start) => {
                route.history.mark_segment_end(end);
                // INVARIANT: `start` is the segment's start or a line inside it.
                let samples = segment.samples[start - segment.start..].to_vec();
                let rate = f64::from(SAMPLE_RATE);
                let span = (start as f64 / rate, end as f64 / rate);
                let mut solved = held.solved;
                // Nothing was read after the detector handed the segment over.
                solved.l_dec_ms = 0.0;
                let speaker_audio = if self.speakers.is_some() {
                    samples
                } else {
                    Vec::new()
                };
                Ok(self.finalize(solved, span, speaker_audio, ended_wall_ms, decision))
            }
        }
    }

    /// Decodes the whole text of one segment: recognition, retry, ITN,
    /// punctuation, dictionary. A segment with no speech makes no final.
    fn process(
        &mut self,
        segment: Segment,
        ended_wall_ms: u64,
        decision: Decision,
    ) -> Result<Option<Final>, AsrError> {
        let Some(route) = self.route.as_mut() else {
            return Ok(None);
        };
        let samples = route.history.with_preroll(segment.start, &segment.samples);
        let Some(solved) = self.solve(&samples, segment.samples.len())? else {
            return Ok(None);
        };
        let span = span_seconds(&segment);
        let speaker_audio = if self.speakers.is_some() {
            segment.samples
        } else {
            Vec::new()
        };
        Ok(self.finalize(solved, span, speaker_audio, ended_wall_ms, decision))
    }

    /// Reads `samples`, of which `speech_samples` are speech: recognition,
    /// retry, ITN, punctuation, dictionary. Audio with no speech gives `None`.
    fn solve(
        &mut self,
        samples: &[f32],
        speech_samples: usize,
    ) -> Result<Option<Solved>, AsrError> {
        let started = Instant::now();
        let Some(route) = self.route.as_mut() else {
            return Ok(None);
        };
        let decoded = decode_samples(route, samples, speech_samples)?;
        if decoded.text.trim().is_empty() {
            return Ok(None);
        }
        let (text, itn_ms) = self.normalize(decoded.text, &decoded.lang);
        let ((text, punct_error), punct_ms) = timed(|| self.punctuate(text, &decoded.lang));
        let text = self.replacements.apply(&text);
        if text.trim().is_empty() {
            return Ok(None);
        }
        Ok(Some(Solved {
            text,
            lang: decoded.lang,
            tier: decoded.tier,
            seconds: decoded.seconds,
            lid_ms: decoded.lid_ms,
            decode_ms: decoded.decode_ms,
            itn_ms,
            punct_ms,
            punct_error,
            l_dec_ms: millis(started),
        }))
    }

    /// Makes the `final` of a read text. The detector's time since the last
    /// one is handed on with it. `None` without a route.
    fn finalize(
        &mut self,
        solved: Solved,
        (t0_s, t1_s): (f64, f64),
        speaker_audio: Vec<f32>,
        ended_wall_ms: u64,
        endpoint: Decision,
    ) -> Option<Final> {
        let vad_ms = std::mem::take(&mut self.route.as_mut()?.vad_ms);
        let id = self.next_id;
        self.next_id += 1;
        Some(Final {
            id,
            text: solved.text,
            lang: solved.lang,
            tier: solved.tier,
            audio_seconds: solved.seconds,
            t0_s,
            t1_s,
            end_wall_ms: ended_wall_ms,
            timings: Timings {
                vad_ms,
                lid_ms: solved.lid_ms,
                decode_ms: solved.decode_ms,
                punct_ms: solved.punct_ms,
                itn_ms: solved.itn_ms,
                l_dec_ms: solved.l_dec_ms,
            },
            punct_error: solved.punct_error,
            speaker_audio,
            endpoint,
        })
    }

    /// Whether utterances get a speaker mark: the stage is on and its model opened.
    pub fn has_speaker_stage(&self) -> bool {
        self.speakers.is_some()
    }

    /// The speaker mark of a finished utterance, to be sent after its `final`;
    /// `None` when the stage is out. Takes `done.speaker_audio` as it is.
    pub fn mark_speaker(&mut self, done: &Final) -> Option<Mark> {
        let stage = self.speakers.as_mut()?;
        Some(stage.mark(&done.speaker_audio))
    }

    /// Inverse text normalisation, when the stage is on, and the time it took.
    fn normalize(&self, text: String, lang: &str) -> (String, f64) {
        if !self.stages.itn {
            return (text, 0.0);
        }
        timed(|| itn::convert(&text, lang, &self.itn_overrides))
    }

    /// Punctuation, when there is a model and the text is Japanese, the one
    /// language the model restores marks for. A failure must not lose the
    /// transcription: the text goes on without marks and the failure is
    /// handed to the caller.
    fn punctuate(&mut self, text: String, lang: &str) -> (String, Option<String>) {
        let Some(punctuator) = self.punctuator.as_mut().filter(|_| self.stages.punctuate) else {
            return (text, None);
        };
        if lang != PUNCTUATED_LANGUAGE {
            return (text, None);
        }
        match punctuator.restore(&text) {
            Ok(punctuated) => (punctuated, None),
            Err(error) => (text, Some(error.to_string())),
        }
    }
}

/// What the recognizer made of one segment.
struct Decoded {
    text: String,
    lang: String,
    tier: &'static str,
    /// Seconds of audio decoded, the pre-roll included.
    seconds: f64,
    lid_ms: f64,
    decode_ms: f64,
}

/// The error of the route as the error of the engine.
fn asr_error_of(error: RouteError) -> AsrError {
    match error {
        RouteError::Decode(inner) | RouteError::Lid(inner) => inner,
        RouteError::NoModels => AsrError::NoRecognizer,
        RouteError::Closed | RouteError::Invalid(_) => AsrError::Route(error.to_string()),
    }
}

/// Reads a segment with its pre-roll: the route names the language, picks the
/// recognizer and retries piece by piece when the text looks cut short.
fn decode_samples(
    route: &mut Route,
    samples: &[f32],
    speech_samples: usize,
) -> Result<Decoded, AsrError> {
    let seconds = samples.len() as f64 / f64::from(SAMPLE_RATE);
    let request = Request {
        samples,
        known_lang: None,
        speech_s: Some(speech_samples as f64 / f64::from(SAMPLE_RATE)),
        live: true,
        second_opinion: None,
    };
    let transcript = route.router.transcribe(&request).map_err(asr_error_of)?;
    Ok(Decoded {
        text: transcript.text,
        lang: transcript.lang,
        tier: transcript.tier.name(),
        seconds,
        lid_ms: transcript.lid_ms + transcript.probe_ms,
        decode_ms: transcript.decode_ms,
    })
}

/// Which rule ended `segment`: a flush, the limit on continuous speech
/// (the detector cuts a segment of about that length), or the silence.
fn decision_of(segment: &Segment, main: Sensitivity, flushed: bool) -> Decision {
    let limit = (f64::from(main.max_speech) * f64::from(SAMPLE_RATE)) as usize;
    if flushed {
        Decision {
            rule: Rule::Flush,
            silence_ms: 0,
            plan: None,
            held_gap_ms: None,
        }
    } else if segment.samples.len() + 2 * WINDOW >= limit {
        Decision {
            rule: Rule::MaxSpeech,
            silence_ms: 0,
            plan: None,
            held_gap_ms: None,
        }
    } else {
        Decision {
            rule: Rule::Silence,
            silence_ms: (main.min_silence * 1000.0).round() as u32,
            plan: None,
            held_gap_ms: None,
        }
    }
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
    use crate::route::EnTier;

    #[test]
    fn every_stage_runs_by_default() {
        assert_eq!(
            Stages::default(),
            Stages {
                itn: true,
                punctuate: true,
                speakers: true
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
                ("punct", "disabled"),
                ("speaker", "disabled")
            ]
        );
        assert!(engine.sessions().is_empty());
        assert!(engine.feed(&[0.0; crate::audio::WINDOW])?.is_empty());
        assert!(engine.finish()?.is_empty());
        Ok(())
    }

    #[test]
    fn the_resident_cap_starts_at_three_and_a_cap_of_none_keeps_all() {
        assert_eq!(DEFAULT_MAX_RESIDENT, NonZeroUsize::new(3));
        let settings = Settings::new(PathBuf::from("models"));
        assert_eq!(settings.max_resident, DEFAULT_MAX_RESIDENT);
    }

    #[test]
    fn an_engine_with_no_route_has_no_recognizers_to_report() -> Result<(), AsrError> {
        let mut engine = Engine::open(Settings::new(PathBuf::from("no-such-models")))?;
        assert!(engine.take_resident_events().is_empty());
        assert!(engine.resident().is_empty());
        Ok(())
    }

    #[test]
    fn with_no_stage_on_the_gpu_there_is_nothing_to_give_back() -> Result<(), AsrError> {
        let mut engine = Engine::open(Settings::new(PathBuf::from("no-such-models")))?;
        assert_eq!(engine.demote_for_memory()?, None);
        Ok(())
    }

    #[test]
    fn a_run_with_the_speaker_stage_off_neither_opens_it_nor_says_it_fell_back()
    -> Result<(), AsrError> {
        let mut settings = Settings::new(PathBuf::from("no-such-models"));
        settings.stages.speakers = false;
        let mut reports = Vec::new();
        let mut fallbacks = Vec::new();
        let opened = open_speakers(&settings, &mut reports, &mut fallbacks)?;
        assert!(opened.is_none());
        assert!(reports.is_empty());
        assert!(fallbacks.is_empty());
        Ok(())
    }

    #[test]
    fn a_missing_speaker_model_leaves_that_stage_out_and_nothing_else() -> Result<(), AsrError> {
        let settings = Settings::new(PathBuf::from("no-such-models"));
        let mut reports = Vec::new();
        let mut fallbacks = Vec::new();
        assert!(open_speakers(&settings, &mut reports, &mut fallbacks)?.is_none());
        assert!(reports.is_empty());
        let left_out: Vec<(&str, &str, &str)> = fallbacks
            .iter()
            .map(|f| (f.stage.name(), f.reason.name(), f.to.name()))
            .collect();
        assert_eq!(left_out, [("speaker", "missing", "disabled")]);
        Ok(())
    }

    #[test]
    fn a_session_forced_to_english_preloads_the_english_recognizer() {
        assert_eq!(
            english_tier_to_preload(&Policy::single("en")),
            Some(Tier::V3)
        );
        let mut english_only = Policy::single("en");
        english_only.en_tier = EnTier::V2;
        assert_eq!(english_tier_to_preload(&english_only), Some(Tier::V2));
    }

    #[test]
    fn a_session_that_identifies_the_language_preloads_the_english_recognizer() {
        assert_eq!(english_tier_to_preload(&Policy::balanced()), Some(Tier::V3));
        assert_eq!(english_tier_to_preload(&Policy::fast()), Some(Tier::V3));
    }

    #[test]
    fn a_session_forced_to_another_language_preloads_nothing() {
        for lang in ["ja", "zh", "ko", "yue", "de"] {
            assert_eq!(
                english_tier_to_preload(&Policy::single(lang)),
                None,
                "{lang}"
            );
        }
        assert_eq!(
            english_tier_to_preload(&Settings::new(PathBuf::new()).policy),
            None
        );
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

    /// The minimum length is a rule of the early path; the main detector's
    /// silence ends a segment shorter than it the same way as a longer one.
    #[test]
    fn a_segment_shorter_than_the_early_minimum_is_still_ended_by_the_silence_rule() {
        let main = Sensitivity {
            threshold: 0.5,
            min_silence: 0.30,
            max_speech: 12.0,
        };
        let short = Segment {
            start: 0,
            samples: vec![0.0; 8_000],
        };
        let decision = decision_of(&short, main, false);
        assert_eq!(decision.rule, Rule::Silence);
        assert_eq!(decision.silence_ms, 300);
    }

    #[test]
    fn samples_are_counted_in_milliseconds_at_16_khz() {
        assert_eq!(samples_ms(0), 0.0);
        assert_eq!(samples_ms(16_000), 1000.0);
        assert_eq!(samples_ms(15_984), 999.0);
    }

    #[test]
    fn an_engine_with_no_route_has_no_early_holds() -> Result<(), AsrError> {
        let mut engine = Engine::open(Settings::new(PathBuf::from("no-such-models")))?;
        assert!(engine.take_early_holds().is_empty());
        Ok(())
    }

    /// A sentence kept as incomplete is counted like one kept as too short,
    /// with its reason and the length of the speech it was read from.
    #[test]
    fn a_hold_carries_its_reason_and_the_speech_it_was_read_from() {
        let incomplete = EarlyHold::new(Hold::Incomplete, 8_000, "ja", "雨が降っていますが。");
        assert_eq!(incomplete.reason, Hold::Incomplete);
        assert_eq!(incomplete.span_ms, 500.0);
        let short = EarlyHold::new(Hold::TooShort, 15_984, "ja", "言ったろう。");
        assert_eq!(short.reason, Hold::TooShort);
        assert_eq!(short.span_ms, 999.0);
    }

    /// The tail and the shadow answer ride on the hold; the sentence does not
    /// (`EarlyHold` is `Copy`, so it cannot own the text).
    #[test]
    fn a_hold_carries_the_tail_and_what_the_shadow_rule_would_have_done() {
        let noun = EarlyHold::new(Hold::Incomplete, 32_000, "ja", "独占配信。");
        assert_eq!((noun.tail, noun.shadow_complete), (Tail::Noun, true));
        let short_noun = EarlyHold::new(Hold::Incomplete, 8_000, "ja", "独占配信。");
        assert_eq!(
            (short_noun.tail, short_noun.shadow_complete),
            (Tail::Noun, false)
        );
        let honorific = EarlyHold::new(Hold::Incomplete, 32_000, "ja", "近藤さん。");
        assert_eq!(
            (honorific.tail, honorific.shadow_complete),
            (Tail::Honorific, true)
        );
        let particle = EarlyHold::new(Hold::Incomplete, 32_000, "ja", "本音とは。");
        assert_eq!(
            (particle.tail, particle.shadow_complete),
            (Tail::Particle, false)
        );
        let short_complete = EarlyHold::new(Hold::TooShort, 8_000, "ja", "言ったろう。");
        assert_eq!(
            (short_complete.tail, short_complete.shadow_complete),
            (Tail::Other, false)
        );
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

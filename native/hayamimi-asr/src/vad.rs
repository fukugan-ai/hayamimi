//! Voice activity detection: Silero through sherpa-onnx, on the CPU.

use std::path::{Path, PathBuf};

use hayamimi_gpu::session_lock::lock_for_session;
use sherpa_onnx::{SileroVadModelConfig, VadModelConfig, VoiceActivityDetector};

use crate::audio::{SAMPLE_RATE, WINDOW};
use crate::error::AsrError;

pub const VAD_FILE: &str = "silero_vad.onnx";
/// Silero's own speech-probability cutoff.
const THRESHOLD: f32 = 0.5;
/// Speech runs shorter than this are ignored.
pub const MIN_SPEECH_SECONDS: f32 = 0.25;
/// Seconds of audio the detector's ring buffer holds.
pub const BUFFER_SECONDS: f32 = 30.0;
/// The model is tiny; one thread is plenty.
pub const THREADS: i32 = 1;

/// The two knobs a user may turn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VadSettings {
    /// Silence in seconds that ends an utterance.
    pub min_silence: f32,
    /// Continuous speech in seconds after which an utterance is finalised.
    pub max_speech: f32,
}

impl Default for VadSettings {
    /// 0.30 s of silence ends an utterance (the Rust route's value in design
    /// H9); 12 s of speech forces a final so that breathless commentary does
    /// not hold subtitles back.
    fn default() -> Self {
        VadSettings {
            min_silence: 0.30,
            max_speech: 12.0,
        }
    }
}

/// One detected speech segment: where it starts (absolute sample index) and its samples.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub start: usize,
    pub samples: Vec<f32>,
}

/// Silero's speech-probability cutoff when the user has not chosen one.
pub const DEFAULT_THRESHOLD: f32 = THRESHOLD;

/// The configuration of a Silero detector on the CPU.
pub fn silero_config(models: &Path, settings: VadSettings) -> Result<VadModelConfig, AsrError> {
    silero_config_with(models, settings, THRESHOLD)
}

/// [`silero_config`] with a speech-probability cutoff of the caller's choice.
pub fn silero_config_with(
    models: &Path,
    settings: VadSettings,
    threshold: f32,
) -> Result<VadModelConfig, AsrError> {
    let model = models.join(VAD_FILE);
    if !model.is_file() {
        return Err(AsrError::MissingModel(model));
    }
    let model = model
        .to_str()
        .ok_or(AsrError::Create("VAD (the model path is not UTF-8)"))?
        .to_owned();
    let window = i32::try_from(WINDOW).map_err(|_| AsrError::Overflow("the VAD window"))?;
    let sample_rate =
        i32::try_from(SAMPLE_RATE).map_err(|_| AsrError::Overflow("the sample rate"))?;
    Ok(VadModelConfig {
        silero_vad: SileroVadModelConfig {
            model: Some(model),
            threshold,
            min_silence_duration: settings.min_silence,
            min_speech_duration: MIN_SPEECH_SECONDS,
            window_size: window,
            max_speech_duration: settings.max_speech,
        },
        sample_rate,
        num_threads: THREADS,
        provider: Some(String::from("cpu")),
        ..VadModelConfig::default()
    })
}

/// The segments waiting in `detector`, taken out in order.
pub fn take_segments(detector: &VoiceActivityDetector) -> Result<Vec<Segment>, AsrError> {
    let mut found = Vec::new();
    while !detector.is_empty() {
        let Some(front) = detector.front() else {
            break;
        };
        let start =
            usize::try_from(front.start()).map_err(|_| AsrError::Overflow("a segment start"))?;
        found.push(Segment {
            start,
            samples: front.samples().to_vec(),
        });
        detector.pop();
    }
    Ok(found)
}

/// What a live detector is set to: the two knobs and Silero's cutoff.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sensitivity {
    pub threshold: f32,
    pub min_silence: f32,
    pub max_speech: f32,
}

impl Sensitivity {
    /// The settings a run starts with.
    pub fn starting(settings: VadSettings) -> Sensitivity {
        Sensitivity {
            threshold: DEFAULT_THRESHOLD,
            min_silence: settings.min_silence,
            max_speech: settings.max_speech,
        }
    }

    fn settings(self) -> VadSettings {
        VadSettings {
            min_silence: self.min_silence,
            max_speech: self.max_speech,
        }
    }
}

/// The live detector that cuts the input into utterances.
///
/// A detector cannot be reconfigured; a change builds a new one, which counts
/// its samples from zero. `offset` is the number of samples fed before the
/// current detector was built, so that segment positions stay on the timeline
/// of the whole input.
pub struct Vad {
    detector: VoiceActivityDetector,
    /// The early detector: the same model at a higher threshold and a short
    /// silence, fed the same windows. Only its notices that a stretch of voice
    /// ended are used; it is never flushed inside a stream.
    early: Option<VoiceActivityDetector>,
    models: PathBuf,
    sensitivity: Sensitivity,
    offset: usize,
    fed: usize,
}

/// The early detector's cutoff: its end line is 0.15 below, at 0.50.
pub const EARLY_THRESHOLD: f32 = 0.65;
/// The early detector's silence in seconds.
pub const EARLY_MIN_SILENCE: f32 = 0.12;
/// The early detector's silence in milliseconds, for the log.
pub const EARLY_MIN_SILENCE_MS: u32 = 120;
/// The shortest stretch of speech, in milliseconds, that a notice of the early
/// detector may finalise. A shorter one is kept for the main detector's silence.
pub const EARLY_MIN_SEGMENT_MS: u32 = 1000;

/// What the early detector is set to when the main one is set to `main`.
fn early_sensitivity(main: Sensitivity) -> Sensitivity {
    Sensitivity {
        threshold: EARLY_THRESHOLD,
        min_silence: EARLY_MIN_SILENCE,
        max_speech: main.max_speech,
    }
}

impl Vad {
    pub fn open(models: &Path, settings: VadSettings) -> Result<Self, AsrError> {
        let sensitivity = Sensitivity::starting(settings);
        Ok(Vad {
            detector: build(models, sensitivity)?,
            early: None,
            models: models.to_path_buf(),
            sensitivity,
            offset: 0,
            fed: 0,
        })
    }

    /// [`Vad::open`] with the early detector as well.
    pub fn open_with_early(models: &Path, settings: VadSettings) -> Result<Self, AsrError> {
        let mut vad = Self::open(models, settings)?;
        vad.early = Some(build(models, early_sensitivity(vad.sensitivity))?);
        Ok(vad)
    }

    /// Whether the early detector is open.
    pub fn has_early(&self) -> bool {
        self.early.is_some()
    }

    /// Feeds one window of `WINDOW` samples to both detectors.
    pub fn accept(&mut self, window: &[f32]) {
        self.detector.accept_waveform(window);
        if let Some(early) = &self.early {
            early.accept_waveform(window);
        }
        self.fed += window.len();
    }

    /// The stretches of voice the early detector finished since the last call,
    /// positioned on the timeline of the whole input. Empty without an early
    /// detector. Taken every window, so that its queue does not grow.
    pub fn take_early(&self) -> Result<Vec<Segment>, AsrError> {
        let Some(early) = &self.early else {
            return Ok(Vec::new());
        };
        let mut found = take_segments(early)?;
        for segment in &mut found {
            segment.start += self.offset;
        }
        Ok(found)
    }

    /// Whether the detector is inside speech that has not been cut yet.
    pub fn detected(&self) -> bool {
        self.detector.detected()
    }

    /// Ends the speech in progress, so that it is queued as a segment. Both
    /// detectors are flushed; the caller rebuilds them straight after.
    pub fn flush(&self) {
        self.detector.flush();
        if let Some(early) = &self.early {
            early.flush();
        }
    }

    /// The segments finished since the last call, positioned on the timeline of the whole input.
    pub fn take_segments(&self) -> Result<Vec<Segment>, AsrError> {
        let mut found = take_segments(&self.detector)?;
        for segment in &mut found {
            segment.start += self.offset;
        }
        Ok(found)
    }

    /// What the detector is set to now.
    pub fn sensitivity(&self) -> Sensitivity {
        self.sensitivity
    }

    /// Whether a new detector can replace this one without losing audio: no
    /// speech in progress and no finished segment waiting to be taken.
    pub fn is_idle(&self) -> bool {
        !self.detector.detected() && self.detector.is_empty()
    }

    /// Replaces the detector with one built for `sensitivity`. The caller
    /// checks [`Vad::is_idle`] first; the old detector is kept when the new
    /// one cannot be built.
    pub fn rebuild(&mut self, sensitivity: Sensitivity) -> Result<(), AsrError> {
        self.detector = build(&self.models, sensitivity)?;
        if self.early.is_some() {
            self.early = Some(build(&self.models, early_sensitivity(sensitivity))?);
        }
        self.sensitivity = sensitivity;
        self.offset = self.fed;
        Ok(())
    }
}

#[cfg(test)]
thread_local! {
    /// Armed by a test: the next detector build on this thread fails once.
    static FAIL_NEXT_BUILD: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn build(models: &Path, sensitivity: Sensitivity) -> Result<VoiceActivityDetector, AsrError> {
    #[cfg(test)]
    if FAIL_NEXT_BUILD.with(std::cell::Cell::take) {
        return Err(AsrError::Create("VAD (injected failure)"));
    }
    let config = silero_config_with(models, sensitivity.settings(), sensitivity.threshold)?;
    let _building = lock_for_session();
    VoiceActivityDetector::create(&config, BUFFER_SECONDS).ok_or(AsrError::Create("VAD"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Engine, Gpu, Reason, Settings};

    #[test]
    fn the_defaults_are_the_measured_values() {
        let settings = VadSettings::default();
        assert_eq!(settings.min_silence, 0.30);
        assert_eq!(settings.max_speech, 12.0);
    }

    #[test]
    fn the_early_detector_is_set_one_step_above_the_main_one() {
        let early = early_sensitivity(Sensitivity::starting(VadSettings::default()));
        assert_eq!(early.threshold, EARLY_THRESHOLD);
        assert_eq!(early.min_silence, EARLY_MIN_SILENCE);
        assert_eq!(early.max_speech, 12.0);
        assert_eq!((EARLY_MIN_SILENCE * 1000.0) as u32, EARLY_MIN_SILENCE_MS);
    }

    #[test]
    fn a_missing_model_is_refused_for_the_early_open_as_well() {
        let said = Vad::open_with_early(Path::new("no-such-models"), VadSettings::default()).err();
        assert!(matches!(said, Some(AsrError::MissingModel(_))), "{said:?}");
    }

    #[test]
    fn a_missing_model_is_refused_before_the_c_library_is_called() {
        let said = Vad::open(Path::new("no-such-models"), VadSettings::default()).err();
        assert!(matches!(said, Some(AsrError::MissingModel(_))), "{said:?}");
    }

    type Check = Result<(), Box<dyn std::error::Error>>;

    /// The first `models/` at or above this crate that holds the VAD model;
    /// model files are not in git, so a worktree reads the main checkout's.
    fn models_dir() -> Result<PathBuf, String> {
        let start = env!("CARGO_MANIFEST_DIR");
        Path::new(start)
            .ancestors()
            .map(|directory| directory.join("models"))
            .find(|models| models.join(VAD_FILE).is_file())
            .ok_or_else(|| format!("no models/{VAD_FILE} at or above {start}"))
    }

    fn fail_next_build() {
        FAIL_NEXT_BUILD.with(|armed| armed.set(true));
    }

    #[test]
    fn live_vad_failed_rebuild_preserves_active_and_pending_state() -> Check {
        let mut vad = Vad::open(&models_dir()?, VadSettings::default())?;
        vad.accept(&[0.0; WINDOW]);
        let active = vad.sensitivity();
        // The request that could not be applied; the caller keeps it.
        let pending = Sensitivity {
            threshold: 0.7,
            ..active
        };

        fail_next_build();
        let said = vad.rebuild(pending).err();
        assert!(matches!(said, Some(AsrError::Create(_))), "{said:?}");
        assert_eq!(vad.sensitivity(), active, "the old settings stay in effect");
        assert!(vad.is_idle(), "the old detector is still the one in use");
        assert_eq!(vad.offset, 0, "the timeline is not moved by a failure");
        vad.accept(&[0.0; WINDOW]);

        // A later change composes with the pending request and applies.
        let composed = Sensitivity {
            min_silence: 0.6,
            ..pending
        };
        vad.rebuild(composed)?;
        assert_eq!(vad.sensitivity(), composed);
        assert_eq!(vad.fed, 2 * WINDOW, "the failed rebuild did not drop audio");
        assert_eq!(vad.offset, vad.fed, "positions continue from the audio fed");
        Ok(())
    }

    /// The engine moves the detector only when the settings asked for differ
    /// from the ones in effect. A build is armed to fail: an update that
    /// changes nothing must not reach it, and one that changes something must.
    #[test]
    fn live_vad_empty_update_does_not_rebuild_detector() -> Check {
        let mut settings = Settings::new(models_dir()?);
        settings.gpu = Gpu::Absent(Reason::NoDevice);
        settings.stages.punctuate = false;
        settings.stages.speakers = false;
        let mut engine = Engine::open(settings)?;
        let Some(unchanged) = engine.sensitivity() else {
            return Err("the engine opened without a voice detector".into());
        };

        fail_next_build();
        assert!(engine.retune_vad(unchanged)?, "nothing to apply is done");
        let changed = Sensitivity {
            threshold: 0.7,
            ..unchanged
        };
        let said = engine.retune_vad(changed).err();
        assert!(matches!(said, Some(AsrError::Create(_))), "{said:?}");
        assert_eq!(engine.sensitivity(), Some(unchanged));
        Ok(())
    }
}

//! Voice activity detection: Silero through sherpa-onnx, on the CPU.

use std::path::{Path, PathBuf};

use hayamimi_gpu::session_lock::lock_for_session;
use sherpa_onnx::{SileroVadModelConfig, VadModelConfig, VoiceActivityDetector};

use crate::audio::{SAMPLE_RATE, WINDOW};
use crate::error::AsrError;

const VAD_FILE: &str = "silero_vad.onnx";
/// Silero's own speech-probability cutoff.
const THRESHOLD: f32 = 0.5;
/// Speech runs shorter than this are ignored.
const MIN_SPEECH_SECONDS: f32 = 0.25;
/// Seconds of audio the detector's ring buffer holds.
const BUFFER_SECONDS: f32 = 30.0;
/// The model is tiny; one thread is plenty.
const THREADS: i32 = 1;

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
    models: PathBuf,
    sensitivity: Sensitivity,
    offset: usize,
    fed: usize,
}

impl Vad {
    pub fn open(models: &Path, settings: VadSettings) -> Result<Self, AsrError> {
        let sensitivity = Sensitivity::starting(settings);
        Ok(Vad {
            detector: build(models, sensitivity)?,
            models: models.to_path_buf(),
            sensitivity,
            offset: 0,
            fed: 0,
        })
    }

    /// Feeds one window of `WINDOW` samples.
    pub fn accept(&mut self, window: &[f32]) {
        self.detector.accept_waveform(window);
        self.fed += window.len();
    }

    /// Whether the detector is inside speech that has not been cut yet.
    pub fn detected(&self) -> bool {
        self.detector.detected()
    }

    /// Ends the speech in progress, so that it is queued as a segment.
    pub fn flush(&self) {
        self.detector.flush();
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
        self.sensitivity = sensitivity;
        self.offset = self.fed;
        Ok(())
    }
}

fn build(models: &Path, sensitivity: Sensitivity) -> Result<VoiceActivityDetector, AsrError> {
    let config = silero_config_with(models, sensitivity.settings(), sensitivity.threshold)?;
    let _building = lock_for_session();
    VoiceActivityDetector::create(&config, BUFFER_SECONDS).ok_or(AsrError::Create("VAD"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_the_measured_values() {
        let settings = VadSettings::default();
        assert_eq!(settings.min_silence, 0.30);
        assert_eq!(settings.max_speech, 12.0);
    }

    #[test]
    fn a_missing_model_is_refused_before_the_c_library_is_called() {
        let said = Vad::open(Path::new("no-such-models"), VadSettings::default()).err();
        assert!(matches!(said, Some(AsrError::MissingModel(_))), "{said:?}");
    }
}

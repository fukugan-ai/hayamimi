//! Voice activity detection: Silero through sherpa-onnx, on the CPU.

use std::path::Path;

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

/// The configuration of a Silero detector on the CPU.
pub fn silero_config(models: &Path, settings: VadSettings) -> Result<VadModelConfig, AsrError> {
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
            threshold: THRESHOLD,
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

/// The live detector that cuts the input into utterances.
pub struct Vad {
    detector: VoiceActivityDetector,
}

impl Vad {
    pub fn open(models: &Path, settings: VadSettings) -> Result<Self, AsrError> {
        let config = silero_config(models, settings)?;
        let detector = VoiceActivityDetector::create(&config, BUFFER_SECONDS)
            .ok_or(AsrError::Create("VAD"))?;
        Ok(Vad { detector })
    }

    /// Feeds one window of `WINDOW` samples.
    pub fn accept(&self, window: &[f32]) {
        self.detector.accept_waveform(window);
    }

    /// Ends the speech in progress, so that it is queued as a segment.
    pub fn flush(&self) {
        self.detector.flush();
    }

    /// The segments finished since the last call.
    pub fn take_segments(&self) -> Result<Vec<Segment>, AsrError> {
        take_segments(&self.detector)
    }
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

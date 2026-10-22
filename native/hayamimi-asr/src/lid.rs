//! Spoken-language identification: whisper-tiny through sherpa-onnx, and the
//! pure helpers around it (the clip it is fed, the tags it and SenseVoice
//! answer with, the script check that corrects a tag the text contradicts).
//!
//! The identifier is the one stage of the language route that is placed on
//! the GPU. It is built on CUDA when the start found a device, and on the
//! CPU, with the same model, when there is none or the CUDA session does not
//! come up. A model that is not there leaves the stage out: the route then
//! reads every segment in the language it was told.

use std::path::{Path, PathBuf};

use hayamimi_gpu::session_lock::lock_for_session;
use hayamimi_gpu::{modules, probe};
use sherpa_onnx::{
    SpokenLanguageIdentification, SpokenLanguageIdentificationConfig,
    SpokenLanguageIdentificationWhisperConfig,
};

use crate::audio::SAMPLE_RATE;
use crate::error::AsrError;
use crate::fallback::{Fallback, Gpu, Reason, Stage, To};
use crate::ort_runtime::Device;
use crate::pipeline::SessionReport;
use crate::recognizer::THREADS;

/// Seconds of a segment the identifier is given; a longer buffer would change
/// the cost and accuracy trade-off the length-versus-accuracy curve was measured at.
pub const LID_MAX_SECONDS: f32 = 4.0;
const MODEL_DIR: &str = "sherpa-onnx-whisper-tiny";
/// fp32: the CUDA execution provider has no kernel for the int8 operators.
const ENCODER: &str = "tiny-encoder.onnx";
const DECODER: &str = "tiny-decoder.onnx";
/// A sample louder than this is speech rather than the quiet of the pre-roll.
const LOUD: f32 = 0.015;
/// The language codes SenseVoice's own identifier can answer with.
const SV_LID_CODES: [&str; 5] = ["ja", "en", "zh", "ko", "yue"];
/// Kana, in Unicode order: hiragana and katakana.
const KANA: std::ops::RangeInclusive<char> = '\u{3040}'..='\u{30FF}';
/// Han ideographs of the main block.
const HAN: std::ops::RangeInclusive<char> = '\u{4E00}'..='\u{9FFF}';
/// Hangul syllables.
const HANGUL: std::ops::RangeInclusive<char> = '\u{AC00}'..='\u{D7AF}';

/// Names the language of a clip of 16 kHz samples.
pub trait Identify {
    fn identify(&self, samples: &[f32]) -> Result<String, AsrError>;
}

/// The part of a segment the identifier reads: the leading quiet of the
/// pre-roll is skipped, because it would eat into the fixed-length window,
/// and the rest is capped at `max_seconds`.
pub fn trim_lid_clip(samples: &[f32], sample_rate: u32, max_seconds: f32) -> &[f32] {
    let rate = sample_rate as usize;
    let first_loud = samples.iter().position(|sample| sample.abs() > LOUD);
    let start = match first_loud {
        Some(at) if at > rate / 10 => at.saturating_sub(rate / 20),
        _ => 0,
    };
    let clip = &samples[start..];
    let longest = (max_seconds * sample_rate as f32) as usize;
    &clip[..clip.len().min(longest)]
}

/// The clip the production identifier reads.
pub fn lid_clip(samples: &[f32]) -> &[f32] {
    trim_lid_clip(samples, SAMPLE_RATE, LID_MAX_SECONDS)
}

pub fn has_kana(text: &str) -> bool {
    text.chars().any(|c| KANA.contains(&c))
}

/// How many letters of a text are in each script that decides a tag.
struct Scripts {
    letters: usize,
    cjk: usize,
    hangul: usize,
}

fn count_scripts(text: &str) -> Scripts {
    let mut scripts = Scripts {
        letters: 0,
        cjk: 0,
        hangul: 0,
    };
    for c in text.chars().filter(|c| c.is_alphabetic()) {
        scripts.letters += 1;
        scripts.cjk += usize::from(KANA.contains(&c) || HAN.contains(&c));
        scripts.hangul += usize::from(HANGUL.contains(&c));
    }
    scripts
}

/// Corrects a tag that contradicts the script of the decoded text: Hangul
/// under another tag is Korean, a Korean tag with no Hangul is a mislabel,
/// CJK under a non-CJK tag is Japanese, and Latin text under a Japanese tag is English.
pub fn script_corrected_lang<'a>(tagged: &'a str, text: &str) -> &'a str {
    let scripts = count_scripts(text);
    if scripts.letters == 0 {
        return tagged;
    }
    let letters = scripts.letters as f64;
    let (hangul, cjk) = (
        scripts.hangul as f64 / letters,
        scripts.cjk as f64 / letters,
    );
    if hangul > 0.3 && tagged != "ko" {
        return "ko";
    }
    if tagged == "ko" && scripts.hangul == 0 && scripts.letters >= 4 {
        return if cjk > 0.3 { "zh" } else { "en" };
    }
    if cjk > 0.3 && !matches!(tagged, "ja" | "zh" | "yue" | "ko") {
        return "ja";
    }
    if cjk < 0.05 && tagged == "ja" && scripts.letters >= 8 {
        return "en";
    }
    tagged
}

/// The one of `ja` and `en` a text is written in, by its letters alone: `ja`
/// when kana and kanji are more than three tenths of them, `en` otherwise
/// (Latin, Hangul and Cyrillic included). `None` when the text has no letter.
/// The threshold and the comparison are those of [`script_corrected_lang`].
pub fn pair_script_lang(text: &str) -> Option<&'static str> {
    let scripts = count_scripts(text);
    if scripts.letters == 0 {
        return None;
    }
    let cjk = scripts.cjk as f64 / scripts.letters as f64;
    Some(if cjk > 0.3 { "ja" } else { "en" })
}

/// The bare code ("ja", "en", "zh", "ko", "yue") inside SenseVoice's raw
/// language tag, or none when no code of the five appears in it.
pub fn sv_lid_tag(sv_tag: &str) -> Option<&'static str> {
    SV_LID_CODES.into_iter().find(|code| sv_tag.contains(code))
}

/// The two model files, encoder then decoder.
pub fn model_files(models: &Path) -> [PathBuf; 2] {
    let directory = models.join(MODEL_DIR);
    [ENCODER, DECODER].map(|name| directory.join(name))
}

/// Builds with `build` only when both files are there. The C library ends the
/// whole process on a missing model instead of reporting it, so the check
/// comes first and a missing file never reaches the library.
pub fn build_lid_guarded<T>(
    files: &[PathBuf; 2],
    build: impl FnOnce(&[String; 2]) -> Result<T, AsrError>,
) -> Result<T, AsrError> {
    if let Some(missing) = files.iter().find(|file| !file.is_file()) {
        return Err(AsrError::MissingModel(missing.clone()));
    }
    let text = |file: &PathBuf| -> Result<String, AsrError> {
        file.to_str().map(str::to_owned).ok_or(AsrError::Create(
            "language identifier (the model path is not UTF-8)",
        ))
    };
    build(&[text(&files[0])?, text(&files[1])?])
}

fn create(files: &[String; 2], provider: Device) -> Result<SpokenLanguageIdentification, AsrError> {
    let config = SpokenLanguageIdentificationConfig {
        whisper: SpokenLanguageIdentificationWhisperConfig {
            encoder: Some(files[0].clone()),
            decoder: Some(files[1].clone()),
            // The library's own default: the whisper tail padding it picks.
            tail_paddings: -1,
        },
        num_threads: THREADS,
        debug: false,
        provider: Some(provider.name().to_owned()),
    };
    let _building = lock_for_session();
    SpokenLanguageIdentification::create(&config).ok_or(AsrError::Create("language identifier"))
}

/// The identifier of the route.
pub struct Lid {
    inner: SpokenLanguageIdentification,
}

impl Identify for Lid {
    fn identify(&self, samples: &[f32]) -> Result<String, AsrError> {
        let clip = lid_clip(samples);
        if clip.is_empty() {
            return Err(AsrError::NoResult);
        }
        let rate = i32::try_from(SAMPLE_RATE).map_err(|_| AsrError::Overflow("the sample rate"))?;
        let stream = self.inner.create_stream();
        stream.accept_waveform(rate, clip);
        let result = self.inner.compute(&stream).ok_or(AsrError::NoResult)?;
        Ok(result.lang)
    }
}

/// What opening the identifier came to.
pub struct Opened {
    /// `None` when the stage is left out.
    pub lid: Option<Lid>,
    /// The step the stage took off the design, if it took one.
    pub fallback: Option<Fallback>,
    /// The two sessions sherpa-onnx created, as the log reports them.
    pub sessions: Vec<SessionReport>,
}

fn guarded_create(files: &[PathBuf; 2], provider: Device) -> Result<Lid, AsrError> {
    build_lid_guarded(files, |paths| {
        Ok(Lid {
            inner: create(paths, provider)?,
        })
    })
}

fn moved_to_cpu(reason: Reason) -> Fallback {
    Fallback {
        stage: Stage::Lid,
        reason,
        to: To::Cpu,
    }
}

/// Whether the process shows no CUDA provider library after a session was
/// asked for CUDA. A module list that cannot be read shows nothing, and then
/// the session is taken to have run where it was asked to; a provider library
/// that an earlier CUDA session already loaded also hides a mismatch.
fn cuda_did_not_run() -> bool {
    modules::loaded_modules().is_ok_and(|loaded| probe::provider_of(&loaded) != Device::Cuda.name())
}

/// Builds on CUDA; on a failed session, or when CUDA is not what ran, again on the CPU.
fn open_cuda(files: &[PathBuf; 2]) -> (Result<Lid, AsrError>, Device, Option<Fallback>) {
    let attempt = guarded_create(files, Device::Cuda);
    let reason = match &attempt {
        Err(AsrError::Create(_)) => Reason::SessionFailed,
        Ok(_) if cuda_did_not_run() => Reason::ProviderMismatch,
        _ => return (attempt, Device::Cuda, None),
    };
    let again = guarded_create(files, Device::Cpu);
    (again, Device::Cpu, Some(moved_to_cpu(reason)))
}

fn reports(files: &[PathBuf; 2], ran_on: Device) -> Vec<SessionReport> {
    files
        .iter()
        .filter_map(|file| file.file_name()?.to_str())
        .map(|name| SessionReport {
            model: name.to_owned(),
            requested: Device::Cuda.name(),
            actual: ran_on.name(),
            nodes: None,
        })
        .collect()
}

/// The stage left out, when the failure is about its model; any other failure is returned.
fn left_out(error: AsrError) -> Result<Opened, AsrError> {
    let reason = match error {
        AsrError::MissingModel(_) => Reason::Missing,
        AsrError::Create(_) => Reason::Unreadable,
        other => return Err(other),
    };
    Ok(Opened {
        lid: None,
        fallback: Some(Fallback {
            stage: Stage::Lid,
            reason,
            to: To::Disabled,
        }),
        sessions: Vec::new(),
    })
}

/// Opens the identifier under `models`. `gpu` is what the start found: with a
/// device the stage tries CUDA first, without one it is built on the CPU and
/// the step is reported. A missing or unreadable model leaves the stage out.
///
/// A `Gpu::Try` is confirmed against the driver first ([`Gpu::confirmed`]):
/// a CUDA session asked for with no device behind it ends the process.
pub fn open(models: &Path, gpu: Gpu) -> Result<Opened, AsrError> {
    open_as_told(models, gpu.confirmed())
}

/// [`open`] without the driver check: `gpu` is believed. It exists so that a
/// test can show what the check prevents; a caller has no use for it.
pub fn open_as_told(models: &Path, gpu: Gpu) -> Result<Opened, AsrError> {
    let files = model_files(models);
    let (attempt, ran_on, fallback) = match gpu {
        Gpu::Try => open_cuda(&files),
        Gpu::Absent(reason) => (
            guarded_create(&files, Device::Cpu),
            Device::Cpu,
            Some(moved_to_cpu(reason)),
        ),
    };
    match attempt {
        Ok(lid) => Ok(Opened {
            lid: Some(lid),
            fallback,
            sessions: reports(&files, ran_on),
        }),
        Err(error) => left_out(error),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    const RATE: u32 = 16_000;

    /// A directory under the workspace's `target`, never `/tmp`, holding the two model files.
    fn model_directory(name: &str) -> Result<[PathBuf; 2], std::io::Error> {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../target/lid-tests")
            .join(name);
        std::fs::create_dir_all(&directory)?;
        let files = [directory.join(ENCODER), directory.join(DECODER)];
        for file in &files {
            std::fs::write(file, b"x")?;
        }
        Ok(files)
    }

    /// `script_correction_matrix`
    #[test]
    fn script_correction_matrix() {
        let f = script_corrected_lang;
        assert_eq!(f("en", "選手紹介のときのブルーカーペット"), "ja");
        assert_eq!(f("ja", "NICETOMEETYOUEVERYONE"), "en");
        assert_eq!(f("vi", "안녕하세요 반갑습니다 여러분"), "ko");
        assert_eq!(f("yue", "值得在这座迷人村庄"), "yue");
        assert_eq!(f("ja", "はい"), "ja");
        assert_eq!(f("en", "Nice to meet you."), "en");
    }

    #[test]
    fn a_korean_tag_over_text_with_no_hangul_is_a_mislabel() {
        assert_eq!(script_corrected_lang("ko", "这是一个很长的句子"), "zh");
        assert_eq!(script_corrected_lang("ko", "this is english"), "en");
        assert_eq!(script_corrected_lang("ko", "ok"), "ko");
    }

    #[test]
    fn text_with_no_letters_keeps_its_tag() {
        assert_eq!(script_corrected_lang("ja", "123 ..."), "ja");
        assert_eq!(script_corrected_lang("ja", ""), "ja");
    }

    #[test]
    fn has_kana() {
        assert!(super::has_kana("漢字とかなの文"));
        assert!(!super::has_kana("值得在这座迷人村庄"));
    }

    #[test]
    fn a_text_is_ja_above_three_tenths_kana_and_kanji_and_en_at_or_below() {
        assert_eq!(pair_script_lang("日本語だabcdef"), Some("ja"));
        assert_eq!(pair_script_lang("日本語abcdefg"), Some("en"));
        assert_eq!(pair_script_lang("Hello there"), Some("en"));
        assert_eq!(pair_script_lang("안녕하세요"), Some("en"));
    }

    #[test]
    fn a_text_with_no_letter_has_no_pair_language() {
        assert_eq!(pair_script_lang(""), None);
        assert_eq!(pair_script_lang("。、！ 123"), None);
    }

    #[test]
    fn sv_lid_tag_normalizes_bracketed_tag() {
        assert_eq!(sv_lid_tag("<|ja|>"), Some("ja"));
        assert_eq!(sv_lid_tag("<|yue|>"), Some("yue"));
        assert_eq!(sv_lid_tag("<|unk|>"), None);
    }

    #[test]
    fn trim_lid_clip_skips_leading_quiet_preroll() {
        let mut clip = vec![0.0_f32; 4000];
        clip.extend(std::iter::repeat_n(0.5_f32, 1000));
        let trimmed = trim_lid_clip(&clip, RATE, LID_MAX_SECONDS);
        // The first loud sample is at 4000, past the tolerance of 1600, so the
        // clip starts a margin of 800 samples before it.
        assert_eq!(trimmed.len(), clip.len() - (4000 - 800));
        assert_eq!(trimmed[800], 0.5);
    }

    #[test]
    fn trim_lid_clip_leaves_short_preroll_alone() {
        let mut clip = vec![0.0_f32; 500];
        clip.extend(std::iter::repeat_n(0.5_f32, 1000));
        assert_eq!(
            trim_lid_clip(&clip, RATE, LID_MAX_SECONDS).len(),
            clip.len()
        );
    }

    #[test]
    fn trim_lid_clip_caps_to_max_seconds() {
        let clip = vec![1.0_f32; RATE as usize * 6];
        assert_eq!(trim_lid_clip(&clip, RATE, 4.0).len(), RATE as usize * 4);
    }

    #[test]
    fn a_silent_clip_is_kept_whole() {
        let clip = vec![0.0_f32; 3000];
        assert_eq!(trim_lid_clip(&clip, RATE, LID_MAX_SECONDS).len(), 3000);
        assert!(trim_lid_clip(&[], RATE, LID_MAX_SECONDS).is_empty());
    }

    /// `lid_max_seconds_is_the_documented_knob`: the production clip is the
    /// shared trimmer at the documented window, and nothing longer reaches the model.
    #[test]
    fn lid_max_seconds_is_the_documented_knob() {
        assert_eq!(LID_MAX_SECONDS, 4.0);
        let clip = vec![1.0_f32; RATE as usize * 6];
        assert_eq!(lid_clip(&clip).len(), RATE as usize * 4);
    }

    #[test]
    fn build_lid_guarded_never_reaches_sherpa_onnx_when_missing() {
        let missing = [
            PathBuf::from("no-such-dir/tiny-encoder.onnx"),
            PathBuf::from("no-such-dir/tiny-decoder.onnx"),
        ];
        let reached = Cell::new(false);
        let built = build_lid_guarded(&missing, |_| {
            reached.set(true);
            Ok(())
        });
        assert!(matches!(built, Err(AsrError::MissingModel(_))), "{built:?}");
        assert!(!reached.get(), "the builder ran with a model missing");
    }

    #[test]
    fn build_lid_guarded_builds_normally_when_present() -> Result<(), Box<dyn std::error::Error>> {
        let files = model_directory("guard-present")?;
        let built = build_lid_guarded(&files, |paths| Ok(paths.len()))?;
        assert_eq!(built, 2);
        Ok(())
    }

    #[test]
    fn one_missing_file_of_the_two_is_named() -> Result<(), Box<dyn std::error::Error>> {
        let files = model_directory("guard-one-missing")?;
        std::fs::remove_file(&files[1])?;
        let built = build_lid_guarded(&files, |_| Ok(()));
        assert!(
            matches!(&built, Err(AsrError::MissingModel(path)) if *path == files[1]),
            "{built:?}"
        );
        Ok(())
    }

    #[test]
    fn a_missing_model_leaves_the_stage_out_and_says_why() -> Result<(), AsrError> {
        let opened = open(Path::new("no-such-models"), Gpu::Try)?;
        assert!(opened.lid.is_none());
        assert!(opened.sessions.is_empty());
        assert_eq!(
            opened.fallback,
            Some(Fallback {
                stage: Stage::Lid,
                reason: Reason::Missing,
                to: To::Disabled
            })
        );
        Ok(())
    }

    #[test]
    fn a_model_that_is_missing_is_not_also_reported_as_a_missing_gpu() -> Result<(), AsrError> {
        let opened = open(Path::new("no-such-models"), Gpu::Absent(Reason::NoDevice))?;
        let fallback = opened.fallback.map(|step| (step.reason, step.to));
        assert_eq!(fallback, Some((Reason::Missing, To::Disabled)));
        Ok(())
    }

    #[test]
    fn the_models_are_the_fp32_pair_of_the_tiny_directory() {
        let names: Vec<String> = model_files(Path::new("models"))
            .iter()
            .map(|file| file.display().to_string().replace('\\', "/"))
            .collect();
        assert_eq!(
            names,
            [
                "models/sherpa-onnx-whisper-tiny/tiny-encoder.onnx",
                "models/sherpa-onnx-whisper-tiny/tiny-decoder.onnx"
            ]
        );
    }

    #[test]
    fn only_a_failure_about_the_model_leaves_the_stage_out() {
        assert!(left_out(AsrError::Create("x")).is_ok_and(|opened| opened.lid.is_none()));
        assert!(left_out(AsrError::Overflow("x")).is_err());
    }
}

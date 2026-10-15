//! R8q: an engine for a session that may read English loads the English
//! recognizer when it opens, so the first English utterance does not wait for
//! the load. A session forced to another language loads nothing more, and an
//! English recognizer that cannot be loaded does not stop the session: the
//! first English utterance is read by the tier that answers for it, and the
//! route says so.
//!
//! The models and the clips are not in git; a worktree reads them from the
//! main checkout.

mod support;

use std::path::{Path, PathBuf};

use hayamimi_asr::audio::read_wav;
use hayamimi_asr::route::{Policy, RouteNotice};
use hayamimi_asr::{Engine, Gpu, Reason, ResidentEvent, Settings, Tier};
use support::{TestResult, models_dir, testdata_dir};

/// The directory of the English recognizer under the models directory.
const ENGLISH_MODEL: &str = "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8";

/// Settings that open the recognizers and nothing else that costs time, on
/// the CPU, for `policy`.
fn settings_for(models: PathBuf, policy: Policy) -> Settings {
    let mut settings = Settings::new(models);
    settings.policy = policy;
    settings.gpu = Gpu::Absent(Reason::NoDevice);
    settings.stages.punctuate = false;
    settings.stages.speakers = false;
    settings
}

/// The tiers an engine loaded while it opened, in order.
fn loaded_while_opening(
    models: PathBuf,
    policy: Policy,
) -> Result<Vec<Tier>, Box<dyn std::error::Error>> {
    let mut engine = Engine::open(settings_for(models, policy))?;
    Ok(engine
        .take_resident_events()
        .iter()
        .filter_map(|event| match event {
            ResidentEvent::Loaded { tier, .. } => Some(*tier),
            _ => None,
        })
        .collect())
}

#[test]
fn a_session_forced_to_english_loads_the_english_recognizer_when_it_opens() -> TestResult {
    let loaded = loaded_while_opening(models_dir()?, Policy::single("en"))?;
    assert_eq!(loaded, [Tier::Rz, Tier::V3]);
    Ok(())
}

#[test]
fn a_session_that_identifies_the_language_loads_it_too() -> TestResult {
    let loaded = loaded_while_opening(models_dir()?, Policy::balanced())?;
    assert_eq!(loaded, [Tier::Rz, Tier::V3]);
    Ok(())
}

#[test]
fn a_session_forced_to_japanese_loads_no_english_recognizer() -> TestResult {
    let loaded = loaded_while_opening(models_dir()?, Policy::single("ja"))?;
    assert_eq!(loaded, [Tier::Rz]);
    Ok(())
}

#[cfg(unix)]
fn link(from: &Path, to: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(from, to)
}

#[cfg(windows)]
fn link(from: &Path, to: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(from, to)
}

/// A models directory that holds everything the real one does except the English recognizer.
fn models_without_english() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let real = models_dir()?;
    let target = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("preload-without-english");
    std::fs::create_dir_all(&target)?;
    for entry in std::fs::read_dir(&real)? {
        let entry = entry?;
        let place = target.join(entry.file_name());
        if entry.file_name() == ENGLISH_MODEL || place.exists() {
            continue;
        }
        link(&entry.path(), &place)?;
    }
    Ok(target)
}

#[test]
fn an_english_recognizer_that_is_not_there_does_not_stop_the_session() -> TestResult {
    let models = models_without_english()?;
    let clip = testdata_dir()?.join("eval_real").join("en_01.wav");
    let mut engine = Engine::open(settings_for(models, Policy::single("en")))?;
    let loaded: Vec<Tier> = engine
        .take_resident_events()
        .iter()
        .filter_map(|event| match event {
            ResidentEvent::Loaded { tier, .. } => Some(*tier),
            _ => None,
        })
        .collect();
    assert_eq!(loaded, [Tier::Rz]);
    // The first English utterance is read, by the tier that answers, and it is said.
    let finals = engine.transcribe(&read_wav(&clip)?)?;
    assert!(!finals.is_empty(), "no final came out");
    let said = engine.take_route_notices();
    assert!(
        said.iter().any(|notice| matches!(
            notice,
            RouteNotice::Substituted {
                requested: Tier::V3,
                ..
            }
        )),
        "{said:?}"
    );
    Ok(())
}

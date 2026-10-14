//! RP-16, the language-identification row: the identifier is placed on the
//! GPU, runs on the CPU when there is no device, and is left out, with the
//! step reported, when its model is not there. It names the Japanese golden
//! clip as Japanese in each case it runs.
//!
//! The real-model cases are `#[ignore]`d. With the device hidden
//! (`CUDA_VISIBLE_DEVICES` empty) `lid::open` still goes to the CPU for a
//! `Gpu::Try`, because the driver is asked again before a CUDA session is
//! built; `gpu_guard.rs` runs that case in a child process and shows what
//! happens without the check.

mod support;

use std::path::Path;

use hayamimi_asr::audio::read_wav;
use hayamimi_asr::lid::{self, Identify, Opened};
use hayamimi_asr::{Fallback, Gpu, Reason, Stage, To};
use support::{GOLDEN, TestResult, models_dir};

const GOLDEN_CLIPS: [&str; 8] = [
    "ja_014", "ja_024", "ja_033", "ja_036", "ja_044", "ja_079", "ja_085", "ja_090",
];

/// A clip the identifier names Japanese on the CPU and on the GPU.
fn golden_clip() -> Result<Vec<f32>, hayamimi_asr::AsrError> {
    read_wav(&Path::new(GOLDEN).join("ja_024.wav"))
}

fn every_session_ran_on(opened: &Opened, provider: &str) -> bool {
    !opened.sessions.is_empty()
        && opened
            .sessions
            .iter()
            .all(|session| session.actual == provider)
}

fn names_the_clip_japanese(opened: &Opened) -> TestResult {
    let Some(identifier) = &opened.lid else {
        return Err("the identifier was not built".into());
    };
    assert_eq!(identifier.identify(&golden_clip()?)?, "ja");
    Ok(())
}

#[test]
#[ignore = "runs the whisper-tiny identifier"]
fn lid_without_a_device_runs_on_the_cpu_and_says_so() -> TestResult {
    let opened = lid::open(&models_dir()?, Gpu::Absent(Reason::NoDevice))?;
    assert_eq!(
        opened.fallback,
        Some(Fallback {
            stage: Stage::Lid,
            reason: Reason::NoDevice,
            to: To::Cpu,
        })
    );
    assert!(every_session_ran_on(&opened, "cpu"));
    names_the_clip_japanese(&opened)
}

#[test]
#[ignore = "runs the whisper-tiny identifier, on the GPU when there is one"]
fn lid_placed_on_the_gpu_runs_where_it_says() -> TestResult {
    let opened = lid::open(&models_dir()?, Gpu::Try)?;
    match opened.fallback {
        // A device was there and the identifier ran on it.
        None => assert!(every_session_ran_on(&opened, "cuda")),
        // No usable device: the same model on the CPU, reported once.
        Some(step) => {
            assert_eq!((step.stage, step.to), (Stage::Lid, To::Cpu));
            assert!(every_session_ran_on(&opened, "cpu"));
        }
    }
    eprintln!("lid fallback: {:?}", opened.fallback);
    names_the_clip_japanese(&opened)
}

/// Whisper-tiny alone mislabels some Japanese clips (that is why a second
/// identifier confirms a switch); measured on the CPU, 6 of the 8 are named
/// Japanese, one Korean and one Chinese.
#[test]
#[ignore = "runs the whisper-tiny identifier over the eight golden clips"]
fn lid_names_most_golden_clips_japanese() -> TestResult {
    let opened = lid::open(&models_dir()?, Gpu::Absent(Reason::NoDevice))?;
    let Some(identifier) = &opened.lid else {
        return Err("the identifier was not built".into());
    };
    let mut japanese = 0_usize;
    for name in GOLDEN_CLIPS {
        let clip = read_wav(&Path::new(GOLDEN).join(format!("{name}.wav")))?;
        let named = identifier.identify(&clip)?;
        eprintln!("{name}: {named}");
        japanese += usize::from(named == "ja");
    }
    assert!(
        japanese >= 6,
        "{japanese} of {} named ja",
        GOLDEN_CLIPS.len()
    );
    Ok(())
}

#[test]
fn lid_with_no_model_is_left_out_not_a_stop() -> TestResult {
    let absent = Path::new(support::TARGET).join("lid-tests/absent");
    for gpu in [Gpu::Try, Gpu::Absent(Reason::DriverUnreadable)] {
        let opened = lid::open(&absent, gpu)?;
        assert!(opened.lid.is_none());
        assert!(opened.sessions.is_empty());
        assert_eq!(
            opened.fallback,
            Some(Fallback {
                stage: Stage::Lid,
                reason: Reason::Missing,
                to: To::Disabled,
            })
        );
    }
    Ok(())
}

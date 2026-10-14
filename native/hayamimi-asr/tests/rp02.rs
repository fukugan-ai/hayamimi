//! RP-2: the eight golden clips. The final text of every clip, scored against
//! the reference sentence, is no worse than the rate the golden record holds.
//! Rates are compared in ten-thousandths, the precision of the record.

mod support;

use std::path::Path;
use std::sync::OnceLock;

use hayamimi_asr::audio::read_wav;
use hayamimi_asr::{Engine, Settings, Stages};
use hayamimi_eval::cer::{Cer, cer};
use hayamimi_eval::sha::sha256_file;
use serde_json::{Value, json};
use support::{Failure, GOLDEN, TestResult, models_dir, read_json};

const CLIPS: usize = 8;

/// One clip's rate in ten-thousandths.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Score {
    wav: String,
    tt: i64,
}

/// What the golden record holds for one clip.
struct Recorded {
    wav: String,
    sha256: String,
    reference: String,
    tt: i64,
}

/// What a run produced for one clip.
#[derive(Debug, Clone)]
struct Run {
    wav: String,
    finals: Vec<String>,
    tt: i64,
}

/// A rate as ten-thousandths, rounded to nearest the way the record was rounded.
fn ten_thousandths(rate: Cer) -> Result<i64, Failure> {
    if rate.denom == 0 {
        return Err(String::from("a reference is empty after normalisation"));
    }
    let (dist, denom) = (rate.dist as i64, rate.denom as i64);
    Ok((dist * 20_000 + denom) / (2 * denom))
}

fn field<'a>(clip: &'a Value, key: &str) -> Result<&'a Value, Failure> {
    clip.get(key)
        .ok_or_else(|| format!("a golden clip has no `{key}`"))
}

fn text_field(clip: &Value, key: &str) -> Result<String, Failure> {
    field(clip, key)?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("`{key}` is not a string"))
}

fn recorded_clips() -> Result<Vec<Recorded>, Failure> {
    let golden = read_json(&Path::new(GOLDEN).join("golden.json"))?;
    let clips = golden
        .get("clips")
        .and_then(Value::as_array)
        .ok_or_else(|| String::from("golden.json has no `clips` array"))?;
    clips
        .iter()
        .map(|clip| {
            let rate = field(clip, "cer_finals_vs_reference")?
                .as_f64()
                .ok_or_else(|| String::from("`cer_finals_vs_reference` is not a number"))?;
            Ok(Recorded {
                wav: text_field(clip, "wav")?,
                sha256: text_field(clip, "sha256")?,
                reference: text_field(clip, "reference")?,
                tt: (rate * 10_000.0).round() as i64,
            })
        })
        .collect()
}

fn scores_of(recorded: &[Recorded]) -> Vec<Score> {
    recorded
        .iter()
        .map(|clip| Score {
            wav: clip.wav.clone(),
            tt: clip.tt,
        })
        .collect()
}

/// Every clip of the record has a result no worse than its recorded rate.
/// The error names every clip that is worse, or says that clips are missing.
fn check_golden(recorded: &[Score], rust: &[Score]) -> Result<(), Failure> {
    if rust.len() != CLIPS {
        return Err(format!("{} clips in the result, want {CLIPS}", rust.len()));
    }
    let mut worse = Vec::new();
    for clip in recorded {
        let Some(got) = rust.iter().find(|score| score.wav == clip.wav) else {
            return Err(format!("{} is missing from the result", clip.wav));
        };
        if got.tt > clip.tt {
            worse.push(format!("{} ({} > {})", clip.wav, got.tt, clip.tt));
        }
    }
    if worse.is_empty() {
        Ok(())
    } else {
        Err(format!("worse than the record: {}", worse.join(", ")))
    }
}

/// The clips run through the route with `stages`, each on a fresh engine.
fn run_clips(stages: Stages) -> Result<Vec<Run>, Failure> {
    let models = models_dir()?;
    let mut runs = Vec::new();
    for clip in recorded_clips()? {
        let mut settings = Settings::new(models.clone());
        settings.stages = stages;
        let mut engine = Engine::open(settings).map_err(|error| error.to_string())?;
        let samples = read_wav(&Path::new(GOLDEN).join(&clip.wav)).map_err(|e| e.to_string())?;
        let finals: Vec<String> = engine
            .transcribe(&samples)
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|done| done.text)
            .collect();
        let tt = ten_thousandths(cer(&clip.reference, &finals.concat()))?;
        runs.push(Run {
            wav: clip.wav,
            finals,
            tt,
        });
    }
    Ok(runs)
}

fn scores_of_runs(runs: &[Run]) -> Vec<Score> {
    runs.iter()
        .map(|run| Score {
            wav: run.wav.clone(),
            tt: run.tt,
        })
        .collect()
}

/// The run with every stage, made once for the whole test binary.
fn gpu_run() -> Result<&'static [Run], Failure> {
    static RUN: OnceLock<Result<Vec<Run>, Failure>> = OnceLock::new();
    RUN.get_or_init(|| {
        let runs = run_clips(Stages::default())?;
        write_golden_gpu(&runs)?;
        Ok(runs)
    })
    .as_ref()
    .map(Vec::as_slice)
    .map_err(Clone::clone)
}

/// Writes what the route said, per clip, beside the golden record.
fn write_golden_gpu(runs: &[Run]) -> Result<(), Failure> {
    let clips: Vec<Value> = runs
        .iter()
        .map(|run| {
            json!({
                "wav": run.wav,
                "finals": run.finals,
                "cer_finals_vs_reference": run.tt as f64 / 10_000.0,
            })
        })
        .collect();
    let document = json!({
        "_generated_by": "native/hayamimi-asr/tests/rp02.rs",
        "_purpose": "What the Rust route's recognizer, punctuation model on the GPU and string stages said for the eight golden clips. golden.json holds what the Python route said.",
        "clips": clips,
    });
    let text = serde_json::to_string_pretty(&document).map_err(|error| error.to_string())?;
    let path = Path::new(GOLDEN).join("golden_gpu.json");
    std::fs::write(&path, format!("{text}\n"))
        .map_err(|error| format!("cannot write {}: {error}", path.display()))
}

fn score(wav: &str, tt: i64) -> Score {
    Score {
        wav: wav.to_owned(),
        tt,
    }
}

/// T-RP-2-001
#[test]
fn golden_json_has_eight_clips_and_reads_ja_014_cer_0_0222() -> TestResult {
    let clips = recorded_clips()?;
    assert_eq!(clips.len(), CLIPS);
    let Some(first) = clips.iter().find(|clip| clip.wav == "ja_014.wav") else {
        return Err("ja_014.wav is not in the record".into());
    };
    assert_eq!(first.tt, 222);
    Ok(())
}

/// T-RP-2-002
#[test]
fn equal_cer_passes() -> TestResult {
    let recorded = scores_of(&recorded_clips()?);
    check_golden(&recorded, &recorded)?;
    Ok(())
}

/// T-RP-2-003
#[test]
fn cer_one_ten_thousandth_above_record_is_rejected() -> TestResult {
    let recorded = scores_of(&recorded_clips()?);
    let mut rust = recorded.clone();
    rust[0].tt += 1;
    let said = check_golden(&recorded, &rust);
    assert!(
        said.as_ref().is_err_and(|text| text.contains("ja_014.wav")),
        "{said:?}"
    );
    Ok(())
}

/// T-RP-2-004: a run without ITN is worse on exactly the two clips that hold numerals.
#[test]
fn itn_off_fixture_names_ja_085_and_ja_090() -> TestResult {
    let recorded = scores_of(&recorded_clips()?);
    let rust: Vec<Score> = recorded
        .iter()
        .map(|clip| match clip.wav.as_str() {
            "ja_085.wav" | "ja_090.wav" => score(&clip.wav, clip.tt + 1_000),
            _ => clip.clone(),
        })
        .collect();
    let said = check_golden(&recorded, &rust);
    assert!(
        said.as_ref()
            .is_err_and(|text| text.contains("ja_085.wav") && text.contains("ja_090.wav")),
        "{said:?}"
    );
    Ok(())
}

/// T-RP-2-005
#[test]
fn seven_clips_are_rejected() -> TestResult {
    let recorded = scores_of(&recorded_clips()?);
    let seven = &recorded[..CLIPS - 1];
    assert!(check_golden(&recorded, seven).is_err());
    Ok(())
}

/// T-RP-2-006
#[test]
fn golden_wavs_have_the_recorded_sha256() -> TestResult {
    for clip in recorded_clips()? {
        let got = sha256_file(&Path::new(GOLDEN).join(&clip.wav))?;
        assert_eq!(got, clip.sha256, "{}", clip.wav);
    }
    Ok(())
}

/// T-RP-2-007
#[test]
#[ignore = "runs the models on the GPU"]
fn golden_gpu_json_is_written_with_eight_clips() -> TestResult {
    gpu_run()?;
    let written = read_json(&Path::new(GOLDEN).join("golden_gpu.json"))?;
    let clips = written
        .get("clips")
        .and_then(Value::as_array)
        .ok_or("golden_gpu.json has no `clips`")?;
    assert_eq!(clips.len(), CLIPS);
    for clip in clips {
        assert!(clip.get("finals").is_some_and(Value::is_array), "{clip}");
    }
    Ok(())
}

/// T-RP-2-008
#[test]
#[ignore = "runs the models on the GPU"]
fn hayamimi_gpu_golden_cer_not_worse_than_recorded() -> TestResult {
    let recorded = scores_of(&recorded_clips()?);
    check_golden(&recorded, &scores_of_runs(gpu_run()?))?;
    Ok(())
}

/// T-RP-2-009: without ITN, `check_golden` fails exactly on the numeral clips.
#[test]
#[ignore = "runs the models on the GPU"]
fn itn_off_run_fails_ja_085_and_ja_090() -> TestResult {
    let recorded = scores_of(&recorded_clips()?);
    let stages = Stages {
        itn: false,
        ..Stages::default()
    };
    let rust = scores_of_runs(&run_clips(stages)?);
    let said = check_golden(&recorded, &rust);
    assert!(
        said.as_ref()
            .is_err_and(|text| text.contains("ja_085.wav") && text.contains("ja_090.wav")),
        "{said:?}"
    );
    Ok(())
}

/// The acceptance test of RP-2.
#[test]
#[ignore = "runs the models on the GPU"]
fn rp02_ja_golden_cer_not_worse_than_recorded() -> TestResult {
    let recorded = scores_of(&recorded_clips()?);
    check_golden(&recorded, &scores_of_runs(gpu_run()?))?;
    Ok(())
}

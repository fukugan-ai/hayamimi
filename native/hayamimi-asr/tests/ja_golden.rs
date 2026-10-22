//! The recorded Japanese golden clips: the record describes the clips that are
//! committed, its references are the FLEURS ones, and the route still says
//! what the record says. Exact text is not the pass condition. The pass
//! condition is the rate against the recorded text, per clip, at most the
//! tolerance the record carries: int8 kernels are not bit-reproducible across
//! CPU families.

mod support;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use hayamimi_asr::audio::read_wav;
use hayamimi_asr::{Engine, Settings};
use hayamimi_eval::cer::cer;
use hayamimi_eval::sha::sha256_file;
use serde_json::Value;
use support::{Failure, GOLDEN, REPO, TestResult, models_dir, read_json};

fn record() -> Result<Value, Failure> {
    read_json(&Path::new(GOLDEN).join("golden.json"))
}

fn clips_of(record: &Value) -> Result<&Vec<Value>, Failure> {
    record
        .get("clips")
        .and_then(Value::as_array)
        .ok_or_else(|| String::from("golden.json has no `clips` array"))
}

fn text_of<'a>(item: &'a Value, key: &str) -> Result<&'a str, Failure> {
    item.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("a record has no text `{key}`"))
}

fn tolerance_of(record: &Value) -> Result<f64, Failure> {
    record
        .get("_cer_tolerance")
        .and_then(Value::as_f64)
        .ok_or_else(|| String::from("golden.json has no numeric `_cer_tolerance`"))
}

/// The recorded final texts of one clip, in order.
fn finals_of(clip: &Value) -> Result<Vec<&str>, Failure> {
    let finals = clip
        .get("finals")
        .and_then(Value::as_array)
        .ok_or_else(|| String::from("a golden clip has no `finals` array"))?;
    finals
        .iter()
        .map(|text| {
            text.as_str()
                .ok_or_else(|| String::from("a recorded final is not text"))
        })
        .collect()
}

/// The `.wav` files in the golden directory, sorted by name.
fn wavs_on_disk() -> Result<Vec<String>, Failure> {
    let listing =
        std::fs::read_dir(GOLDEN).map_err(|error| format!("cannot read {GOLDEN}: {error}"))?;
    let mut names = Vec::new();
    for entry in listing {
        let entry = entry.map_err(|error| format!("cannot list {GOLDEN}: {error}"))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".wav") {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

/// The FLEURS bench manifest of the Japanese clips, at or above the checkout.
/// The clips are not in git, so a worktree reads them from the main checkout.
fn fleurs_manifest() -> Result<PathBuf, Failure> {
    let start = Path::new(REPO)
        .canonicalize()
        .map_err(|error| format!("cannot resolve {REPO}: {error}"))?;
    start
        .ancestors()
        .map(|directory| directory.join("testdata/fleurs_bench/ja/manifest.json"))
        .find(|manifest| manifest.is_file())
        .ok_or_else(|| {
            format!(
                "no testdata/fleurs_bench/ja/manifest.json at or above {}",
                start.display()
            )
        })
}

/// What the route says for one clip, its final texts joined, on a fresh engine.
fn route_text(models: &Path, wav: &str) -> Result<String, Failure> {
    let mut engine = Engine::open(Settings::new(models.to_path_buf()))
        .map_err(|error| format!("{wav}: {error}"))?;
    let samples = read_wav(&Path::new(GOLDEN).join(wav)).map_err(|error| error.to_string())?;
    let finals = engine
        .transcribe(&samples)
        .map_err(|error| format!("{wav}: {error}"))?;
    Ok(finals.into_iter().map(|done| done.text).collect())
}

/// The rate of `got` against the recorded text `want`.
fn rate(wav: &str, want: &str, got: &str) -> Result<f64, Failure> {
    let measured = cer(want, got);
    if measured.denom == 0 {
        return Err(format!(
            "{wav}: the recorded text is empty after normalisation"
        ));
    }
    Ok(measured.dist as f64 / measured.denom as f64)
}

#[test]
fn golden_file_is_self_consistent() -> TestResult {
    let record = record()?;
    assert!(!text_of(&record, "_generated_by")?.is_empty());
    assert!(tolerance_of(&record)? > 0.0);
    let clips = clips_of(&record)?;
    let listed: Vec<&str> = clips
        .iter()
        .map(|clip| text_of(clip, "wav"))
        .collect::<Result<_, _>>()?;
    assert_eq!(listed, wavs_on_disk()?);
    for clip in clips {
        let wav = text_of(clip, "wav")?;
        assert!(!finals_of(clip)?.is_empty(), "{wav}: no finals recorded");
        assert_eq!(text_of(clip, "sha256")?.len(), 64, "{wav}");
        assert!(!text_of(clip, "reference")?.is_empty(), "{wav}");
    }
    Ok(())
}

#[test]
fn golden_wavs_are_the_ones_that_were_recorded() -> TestResult {
    let record = record()?;
    let clips = clips_of(&record)?;
    assert!(!clips.is_empty());
    for clip in clips {
        let wav = text_of(clip, "wav")?;
        let found = sha256_file(&Path::new(GOLDEN).join(wav)).map_err(|error| error.to_string())?;
        assert_eq!(
            found,
            text_of(clip, "sha256")?,
            "{wav} is not the recorded file"
        );
    }
    Ok(())
}

#[test]
#[ignore = "reads testdata/fleurs_bench/ja/manifest.json, which git does not carry"]
fn references_still_match_the_fleurs_manifest() -> TestResult {
    let manifest = read_json(&fleurs_manifest()?)?;
    let entries = manifest
        .as_array()
        .ok_or_else(|| String::from("the FLEURS manifest is not an array"))?;
    let mut by_wav: HashMap<&str, (Option<&Value>, &str)> = HashMap::new();
    for entry in entries {
        by_wav.insert(
            text_of(entry, "wav")?,
            (entry.get("id"), text_of(entry, "ref")?),
        );
    }
    let record = record()?;
    let clips = clips_of(&record)?;
    assert!(!clips.is_empty());
    for clip in clips {
        let wav = text_of(clip, "wav")?;
        let Some((id, reference)) = by_wav.get(wav) else {
            return Err(format!("{wav} is not in the FLEURS manifest").into());
        };
        assert!(id.is_some(), "{wav}: the manifest entry has no id");
        assert_eq!(clip.get("fleurs_id"), *id, "{wav}");
        assert_eq!(text_of(clip, "reference")?, *reference, "{wav}");
    }
    Ok(())
}

#[test]
#[ignore = "runs the models on the GPU"]
fn pipeline_still_produces_the_golden_text() -> TestResult {
    let record = record()?;
    let tolerance = tolerance_of(&record)?;
    let models = models_dir()?;
    let mut failures = Vec::new();
    for clip in clips_of(&record)? {
        let wav = text_of(clip, "wav")?;
        let got = route_text(&models, wav)?;
        let value = rate(wav, &finals_of(clip)?.concat(), &got)?;
        if value > tolerance {
            failures.push(format!("{wav}: CER {value:.4} > {tolerance}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    Ok(())
}

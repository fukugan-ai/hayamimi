//! The recorded Japanese golden clips: the record describes the clips that are
//! committed, the references are the FLEURS ones, and the route still says
//! what the record says. Exact text is not the pass condition; the rate
//! against the recorded text is, at most the tolerance the record carries.

mod support;

use std::collections::HashMap;
use std::error::Error;
use std::path::{Path, PathBuf};

use hayamimi_asr::audio::read_wav;
use hayamimi_asr::{Engine, Settings};
use hayamimi_eval::cer::cer;
use hayamimi_eval::sha::sha256_file;
use serde_json::Value;
use support::{GOLDEN, REPO, TestResult, models_dir, read_json};

type Fallible<T> = Result<T, Box<dyn Error>>;

fn record() -> Fallible<Value> {
    Ok(read_json(&Path::new(GOLDEN).join("golden.json"))?)
}

fn clips_of(record: &Value) -> Fallible<&Vec<Value>> {
    record
        .get("clips")
        .and_then(Value::as_array)
        .ok_or_else(|| "golden.json has no `clips` array".into())
}

fn text_of<'a>(clip: &'a Value, key: &str) -> Fallible<&'a str> {
    clip.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("a golden clip has no text `{key}`").into())
}

/// The recorded final texts of one clip, in order.
fn finals_of(clip: &Value) -> Fallible<Vec<&str>> {
    let finals = clip
        .get("finals")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{} has no `finals` array", clip["wav"]))?;
    finals
        .iter()
        .map(|text| text.as_str().ok_or_else(|| "a final is not text".into()))
        .collect()
}

/// The `.wav` files in the golden directory, sorted by name.
fn wavs_on_disk() -> Fallible<Vec<String>> {
    let mut names = Vec::new();
    for entry in std::fs::read_dir(GOLDEN)? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if name.ends_with(".wav") {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

/// The FLEURS bench manifest of the Japanese clips, at or above the checkout.
fn fleurs_manifest() -> Fallible<PathBuf> {
    let start = Path::new(REPO).canonicalize()?;
    start
        .ancestors()
        .map(|directory| directory.join("testdata/fleurs_bench/ja/manifest.json"))
        .find(|manifest| manifest.is_file())
        .ok_or_else(|| {
            format!(
                "no testdata/fleurs_bench/ja/manifest.json at or above {}",
                start.display()
            )
            .into()
        })
}

#[test]
fn golden_file_is_self_consistent() -> TestResult {
    let record = record()?;
    assert!(!text_of(&record, "_generated_by")?.is_empty());
    let tolerance = record
        .get("_cer_tolerance")
        .and_then(Value::as_f64)
        .ok_or("golden.json has no numeric `_cer_tolerance`")?;
    assert!(tolerance > 0.0, "{tolerance}");
    let listed: Vec<&str> = clips_of(&record)?
        .iter()
        .map(|clip| text_of(clip, "wav"))
        .collect::<Fallible<_>>()?;
    assert_eq!(listed, wavs_on_disk()?);
    for clip in clips_of(&record)? {
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
        let got = sha256_file(&Path::new(GOLDEN).join(wav))?;
        assert_eq!(
            got,
            text_of(clip, "sha256")?,
            "{wav} is not the recorded file"
        );
    }
    Ok(())
}

#[test]
fn references_still_match_the_fleurs_manifest() -> TestResult {
    let entries: Value = serde_json::from_str(&std::fs::read_to_string(fleurs_manifest()?)?)?;
    let mut by_wav: HashMap<&str, (&str, &str)> = HashMap::new();
    for entry in entries.as_array().ok_or("the manifest is not an array")? {
        by_wav.insert(
            text_of(entry, "wav")?,
            (text_of(entry, "id")?, text_of(entry, "ref")?),
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
        assert_eq!(text_of(clip, "fleurs_id")?, *id, "{wav}");
        assert_eq!(text_of(clip, "reference")?, *reference, "{wav}");
    }
    Ok(())
}

#[test]
#[ignore = "runs the models on the GPU"]
fn pipeline_still_produces_the_golden_text() -> TestResult {
    let record = record()?;
    let tolerance = record
        .get("_cer_tolerance")
        .and_then(Value::as_f64)
        .ok_or("golden.json has no numeric `_cer_tolerance`")?;
    let models = models_dir()?;
    let mut failures = Vec::new();
    for clip in clips_of(&record)? {
        let wav = text_of(clip, "wav")?;
        let mut engine = Engine::open(Settings::new(models.clone()))?;
        let samples = read_wav(&Path::new(GOLDEN).join(wav))?;
        let got: String = engine
            .transcribe(&samples)?
            .into_iter()
            .map(|done| done.text)
            .collect();
        let rate = cer(&finals_of(clip)?.concat(), &got);
        if rate.denom == 0 {
            return Err(format!("{wav}: the recorded text is empty after normalisation").into());
        }
        let value = rate.dist as f64 / rate.denom as f64;
        if value > tolerance {
            failures.push(format!("{wav}: CER {value:.4} > {tolerance}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    Ok(())
}

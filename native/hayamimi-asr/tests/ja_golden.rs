//! The recorded Japanese golden clips: `golden.json` describes the clips that
//! are committed and carries the tolerance, its references are the FLEURS
//! ones, and the route still says what `golden_gpu.json` says (the text the
//! Rust route produced for the same clips on the GPU). Exact text is not the
//! pass condition. The pass condition is the rate against `golden_gpu.json`,
//! per clip, at most the tolerance `golden.json` carries: int8 kernels are not
//! bit-reproducible across CPU families.

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

/// What the Rust route said for the golden clips on the GPU.
fn gpu_record() -> Result<Value, Failure> {
    read_json(&Path::new(GOLDEN).join("golden_gpu.json"))
}

fn clips_of(record: &Value) -> Result<&Vec<Value>, Failure> {
    record
        .get("clips")
        .and_then(Value::as_array)
        .ok_or_else(|| String::from("a golden record has no `clips` array"))
}

fn wavs_of(clips: &[Value]) -> Result<Vec<&str>, Failure> {
    clips.iter().map(|clip| text_of(clip, "wav")).collect()
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

/// The failure line of one clip, or `None` when the rate of `got` against
/// `want` is at most `tolerance`.
fn verdict(wav: &str, want: &str, got: &str, tolerance: f64) -> Result<Option<String>, Failure> {
    let value = rate(wav, want, got)?;
    Ok((value > tolerance).then(|| format!("{wav}: CER {value:.4} > {tolerance}")))
}

/// The text the route is expected to produce for each clip, in the order of
/// `golden.json`: the finals `golden_gpu.json` records for it, joined. The two
/// records must list the same clips in the same order.
fn expected_texts<'a>(
    golden: &'a Value,
    gpu: &'a Value,
) -> Result<Vec<(&'a str, String)>, Failure> {
    let wavs = wavs_of(clips_of(golden)?)?;
    let gpu_clips = clips_of(gpu)?;
    if wavs != wavs_of(gpu_clips)? {
        return Err(String::from(
            "golden_gpu.json lists other clips, or the same clips in another order, than golden.json",
        ));
    }
    wavs.into_iter()
        .zip(gpu_clips)
        .map(|(wav, clip)| finals_of(clip).map(|finals| (wav, finals.concat())))
        .collect()
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
    let golden = record()?;
    let gpu = gpu_record()?;
    let tolerance = tolerance_of(&golden)?;
    let models = models_dir()?;
    let mut failures = Vec::new();
    for (wav, want) in expected_texts(&golden, &gpu)? {
        let got = route_text(&models, wav)?;
        failures.extend(verdict(wav, &want, &got, tolerance)?);
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    Ok(())
}

/// `text` with its first character replaced by another one.
fn with_first_character_changed(text: &str) -> String {
    let mut characters = text.chars();
    characters.next();
    std::iter::once('亜').chain(characters).collect()
}

#[test]
fn golden_gpu_lists_the_clips_of_golden_in_the_same_order() -> TestResult {
    let golden = record()?;
    let gpu = gpu_record()?;
    let wavs: Vec<&str> = expected_texts(&golden, &gpu)?
        .into_iter()
        .map(|(wav, _)| wav)
        .collect();
    assert_eq!(wavs, wavs_of(clips_of(&golden)?)?);
    let mut reversed = gpu.clone();
    let Some(clips) = reversed.get_mut("clips").and_then(Value::as_array_mut) else {
        return Err(String::from("golden_gpu.json has no `clips` array").into());
    };
    clips.reverse();
    assert!(expected_texts(&golden, &reversed).is_err());
    Ok(())
}

#[test]
fn the_expected_text_is_what_golden_gpu_records() -> TestResult {
    let golden = record()?;
    let gpu = gpu_record()?;
    let expected = expected_texts(&golden, &gpu)?;
    let clips = clips_of(&gpu)?;
    assert!(!clips.is_empty());
    assert_eq!(expected.len(), clips.len());
    for ((wav, want), clip) in expected.iter().zip(clips) {
        assert_eq!(text_of(clip, "wav")?, *wav);
        assert_eq!(*want, finals_of(clip)?.concat(), "{wav}");
    }
    Ok(())
}

#[test]
fn a_one_character_change_in_the_gpu_text_fails_the_verdict() -> TestResult {
    let golden = record()?;
    let gpu = gpu_record()?;
    let tolerance = tolerance_of(&golden)?;
    for (wav, want) in expected_texts(&golden, &gpu)? {
        assert_eq!(verdict(wav, &want, &want, tolerance)?, None, "{wav}");
        let changed = with_first_character_changed(&want);
        assert_ne!(changed, want, "{wav}");
        assert!(
            verdict(wav, &want, &changed, tolerance)?.is_some(),
            "{wav}: one changed character passed"
        );
    }
    Ok(())
}

#[test]
fn the_verdict_passes_at_the_tolerance_and_names_an_empty_record() -> TestResult {
    let want = "あ".repeat(100);
    let one_off = format!("{}い", "あ".repeat(99));
    assert_eq!(verdict("x.wav", &want, &one_off, 0.01)?, None);
    assert!(verdict("x.wav", &want, &one_off, 0.005)?.is_some());
    assert!(verdict("x.wav", "。", "あ", 0.01).is_err());
    Ok(())
}

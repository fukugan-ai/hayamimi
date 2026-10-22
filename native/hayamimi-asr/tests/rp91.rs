//! RP-91: a buffer within the cap reads the same as before.
//!
//! The 15 Japanese clips of `testdata/eval_real` (3.11 to 8.80 s) are read by a
//! recognizer with a cap and by one with none. A clip that is not longer than
//! the cap must read byte for byte the same, and the error over the 15 clips
//! must not be worse. The models and the audio are not in git; a worktree
//! reads them from the main checkout.
//!
//!     cargo test --release -p hayamimi-asr --test rp91 -- --ignored --nocapture

mod support;

use std::error::Error;
use std::path::Path;

use hayamimi_asr::audio::read_wav;
use hayamimi_asr::{DecodeCap, Recognizers, Tier};
use hayamimi_eval::cer::{Cer, cer};
use serde_json::Value;
use support::long::THREADS;
use support::{Failure, TestResult, models_dir, read_json, testdata_dir};

/// The samples of the 15 Japanese clips, `ja_01` to `ja_15`.
const CLIP_SAMPLES: [usize; 15] = [
    49_771, 51_548, 125_012, 87_240, 134_268, 135_651, 76_185, 104_263, 110_867, 140_721, 59_065,
    58_653, 95_080, 60_130, 81_017,
];

/// A clip of the evaluation set and the sentence it says.
struct Clip {
    name: String,
    samples: Vec<f32>,
    reference: String,
}

/// What one clip read as under the cap and with none.
struct Reading {
    name: String,
    length: usize,
    reference: String,
    off: String,
    capped: String,
}

fn cap_of(seconds: f64) -> Result<DecodeCap, Box<dyn Error>> {
    Ok(DecodeCap::from_seconds(seconds, 0.0)?)
}

// --- the judgements of RP-91 --------------------------------------------------

/// How many of the clips, given by their lengths, the cap cuts.
fn longer_than_the_cap(lengths: &[usize], cap: DecodeCap) -> usize {
    lengths
        .iter()
        .filter(|length| cap.cuts(Tier::Rz, **length))
        .count()
}

/// Every clip the cap does not cut reads the same text with the cap and with none.
fn judge_unchanged(readings: &[Reading], cap: DecodeCap) -> Result<(), Failure> {
    for reading in readings
        .iter()
        .filter(|reading| !cap.cuts(Tier::Rz, reading.length))
    {
        if reading.off != reading.capped {
            return Err(format!(
                "{} reads `{}` under the cap and `{}` with none",
                reading.name, reading.capped, reading.off
            ));
        }
    }
    Ok(())
}

/// The error under the cap is not above the error with none.
fn judge_not_worse(capped: u64, off: u64) -> Result<(), Failure> {
    if capped <= off {
        return Ok(());
    }
    Err(format!(
        "the capped error ({capped} micros) is {} micros above the uncapped error ({off} micros)",
        capped - off
    ))
}

// --- decoding -----------------------------------------------------------------

fn japanese_rows(testdata: &Path) -> Result<Vec<(String, String)>, Box<dyn Error>> {
    let Value::Array(rows) = read_json(&testdata.join("eval_real").join("manifest.json"))? else {
        return Err("eval_real/manifest.json is not a list".into());
    };
    rows.iter()
        .filter(|row| row["lang"] == "ja")
        .map(|row| match (row["wav"].as_str(), row["ref"].as_str()) {
            (Some(wav), Some(reference)) => Ok((wav.to_owned(), reference.to_owned())),
            _ => Err("a row of eval_real/manifest.json lacks `wav` or `ref`".into()),
        })
        .collect()
}

/// The 15 Japanese clips, checked against the lengths this test was written for.
fn clips(testdata: &Path) -> Result<Vec<Clip>, Box<dyn Error>> {
    let rows = japanese_rows(testdata)?;
    if rows.len() != CLIP_SAMPLES.len() {
        return Err(format!("{} Japanese clips, not {}", rows.len(), CLIP_SAMPLES.len()).into());
    }
    let mut clips = Vec::with_capacity(rows.len());
    for ((wav, reference), expected) in rows.into_iter().zip(CLIP_SAMPLES) {
        let samples = read_wav(&testdata.join("eval_real").join(&wav))?;
        if samples.len() != expected {
            return Err(format!("{wav} has {} samples, not {expected}", samples.len()).into());
        }
        clips.push(Clip {
            name: wav,
            samples,
            reference,
        });
    }
    Ok(clips)
}

fn recognizers(cap: DecodeCap) -> Result<Recognizers, Failure> {
    Ok(Recognizers::new(&models_dir()?, THREADS, None, cap))
}

fn read_all(clips: &[Clip], cap: DecodeCap) -> Result<Vec<Reading>, Box<dyn Error>> {
    let mut with_cap = recognizers(cap)?;
    let mut with_none = recognizers(DecodeCap::Off)?;
    let mut readings = Vec::with_capacity(clips.len());
    for clip in clips {
        readings.push(Reading {
            name: clip.name.clone(),
            length: clip.samples.len(),
            reference: clip.reference.clone(),
            off: with_none.decode_whole(Tier::Rz, &clip.samples)?.text,
            capped: with_cap.decode_whole(Tier::Rz, &clip.samples)?.text,
        });
    }
    Ok(readings)
}

/// The summed error of the texts `pick` takes (dist and denom added, then divided).
fn total(readings: &[Reading], pick: fn(&Reading) -> &str) -> Cer {
    readings.iter().fold(Cer::default(), |sum, reading| {
        sum.plus(cer(&reading.reference, pick(reading)))
    })
}

fn off_text(reading: &Reading) -> &str {
    &reading.off
}

fn capped_text(reading: &Reading) -> &str {
    &reading.capped
}

/// Reads the 15 clips under `cap` and with none, prints what differs, and
/// returns the readings.
fn compare(cap: DecodeCap) -> Result<Vec<Reading>, Box<dyn Error>> {
    let readings = read_all(&clips(&testdata_dir()?)?, cap)?;
    let lengths: Vec<usize> = readings.iter().map(|reading| reading.length).collect();
    println!(
        "{} of {} clips are longer than the cap",
        longer_than_the_cap(&lengths, cap),
        lengths.len()
    );
    for reading in readings
        .iter()
        .filter(|reading| reading.off != reading.capped)
    {
        println!(
            "{} ({} samples): `{}` under the cap, `{}` with none",
            reading.name, reading.length, reading.capped, reading.off
        );
    }
    let (capped, off) = (total(&readings, capped_text), total(&readings, off_text));
    println!(
        "error: capped {} micros ({}/{}), uncapped {} micros ({}/{})",
        capped.micros(),
        capped.dist,
        capped.denom,
        off.micros(),
        off.dist,
        off.denom
    );
    Ok(readings)
}

// --- acceptance ---------------------------------------------------------------

#[test]
#[ignore = "reads testdata/eval_real and the models, which are not in git"]
fn rp91_a_buffer_within_the_cap_reads_the_same_as_before() -> TestResult {
    let cap = cap_of(10.0)?;
    let readings = compare(cap)?;
    judge_unchanged(&readings, cap)?;
    judge_not_worse(
        total(&readings, capped_text).micros(),
        total(&readings, off_text).micros(),
    )?;
    Ok(())
}

#[test]
#[ignore = "reads testdata/eval_real and the models, which are not in git"]
fn a_cap_just_above_the_longest_clip_changes_nothing() -> TestResult {
    let cap = cap_of(8.8)?;
    let readings = compare(cap)?;
    let lengths: Vec<usize> = readings.iter().map(|reading| reading.length).collect();
    assert_eq!(longer_than_the_cap(&lengths, cap), 0);
    judge_unchanged(&readings, cap)?;
    Ok(())
}

#[test]
#[ignore = "reads testdata/eval_real and the models, which are not in git"]
fn the_clips_within_an_8_79_second_cap_read_the_same_and_the_longer_one_is_counted_out()
-> TestResult {
    let cap = cap_of(8.79)?;
    let readings = compare(cap)?;
    let lengths: Vec<usize> = readings.iter().map(|reading| reading.length).collect();
    assert_eq!(longer_than_the_cap(&lengths, cap), 1);
    judge_unchanged(&readings, cap)?;
    Ok(())
}

#[test]
#[ignore = "reads testdata/eval_real and the models, which are not in git"]
fn a_two_second_cap_changes_a_short_clip() -> TestResult {
    let readings = compare(cap_of(2.0)?)?;
    let changed = readings
        .iter()
        .filter(|reading| reading.off != reading.capped)
        .count();
    assert!(changed >= 1, "no clip reads differently under a 2 s cap");
    Ok(())
}

// --- the checks that need neither the models nor the audio ---------------------

#[test]
fn the_clips_longer_than_the_cap_are_counted_and_printed() -> TestResult {
    for (seconds, longer) in [(10.0, 0), (8.8, 0), (8.79, 1), (2.0, 15)] {
        let counted = longer_than_the_cap(&CLIP_SAMPLES, cap_of(seconds)?);
        println!("{counted} of 15 clips are longer than a cap of {seconds} s");
        assert_eq!(counted, longer, "{seconds} s");
    }
    Ok(())
}

#[test]
fn a_capped_error_equal_to_the_uncapped_error_passes_for_short_clips() {
    assert_eq!(judge_not_worse(38_000, 38_000), Ok(()));
}

#[test]
fn a_capped_error_one_millionth_above_the_uncapped_error_is_rejected() {
    let said = judge_not_worse(38_001, 38_000).err();
    assert!(
        said.as_deref()
            .is_some_and(|text| text.contains("1 micros above")),
        "{said:?}"
    );
}

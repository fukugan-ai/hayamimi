//! R3b: the recognizer tiers decode real clips, the cap drops the least
//! recently used tier, a tier that is not installed gives way to another, and
//! the split retry recovers an utterance the whole-buffer decode dropped.
//!
//! The clips and the models are not in git; a worktree reads them from the
//! main checkout.

mod support;

use std::num::NonZeroUsize;

use hayamimi_asr::audio::{SAMPLE_RATE, read_wav};
use hayamimi_asr::retry::Splitter;
use hayamimi_asr::{LangTag, Reason, Recognizers, ResidentEvent, Tier};
use hayamimi_eval::cer::cer;
use serde_json::Value;
use support::{TestResult, models_dir, read_json, testdata_dir};

const THREADS: i32 = 2;

/// A clip of the evaluation set and the sentence it says.
struct Clip {
    samples: Vec<f32>,
    reference: String,
}

/// The `index`-th clip of `lang` in the manifest of `set`.
fn clip(set: &str, lang: &str, index: usize) -> Result<Clip, Box<dyn std::error::Error>> {
    let directory = testdata_dir()?.join(set);
    let manifest = read_json(&directory.join("manifest.json"))?;
    let Value::Array(rows) = manifest else {
        return Err(format!("{set}/manifest.json is not a list").into());
    };
    let Some(row) = rows.iter().filter(|row| row["lang"] == lang).nth(index) else {
        return Err(format!("{set} has no clip {index} of {lang}").into());
    };
    let (Some(wav), Some(reference)) = (row["wav"].as_str(), row["ref"].as_str()) else {
        return Err(format!("a row of {set} lacks `wav` or `ref`").into());
    };
    Ok(Clip {
        samples: read_wav(&directory.join(wav))?,
        reference: reference.to_owned(),
    })
}

fn recognizers(cap: Option<usize>) -> Result<Recognizers, Box<dyn std::error::Error>> {
    Ok(Recognizers::new(
        &models_dir()?,
        THREADS,
        cap.and_then(NonZeroUsize::new),
    ))
}

/// The character error rate of `hyp` against `reference`, in millionths, ignoring case.
fn rate(reference: &str, hyp: &str) -> u64 {
    cer(&reference.to_lowercase(), &hyp.to_lowercase()).micros()
}

fn assert_reads(tier: Tier, set: &str, lang: &str) -> TestResult {
    let sample = clip(set, lang, 0)?;
    let decoded = recognizers(None)?.decode(tier, lang, &sample.samples)?;
    assert_eq!(decoded.tier, tier);
    let micros = rate(&sample.reference, &decoded.text);
    assert!(
        micros < 400_000,
        "{tier:?}: `{}` against `{}` is {micros} millionths",
        decoded.text,
        sample.reference
    );
    Ok(())
}

#[test]
fn the_japanese_tier_reads_a_japanese_clip() -> TestResult {
    assert_reads(Tier::Rz, "eval_real", "ja")
}

#[test]
fn the_parakeet_tier_reads_an_english_clip() -> TestResult {
    assert_reads(Tier::V3, "eval_real", "en")
}

#[test]
fn the_paraformer_tier_reads_a_mandarin_clip() -> TestResult {
    assert_reads(Tier::Pz, "eval_real_zhko", "zh")
}

#[test]
fn the_sensevoice_tier_reads_korean_and_cantonese_clips() -> TestResult {
    assert_reads(Tier::Sv, "eval_real_zhko", "ko")?;
    assert_reads(Tier::Sv, "eval_real_yue", "yue")
}

#[test]
fn the_generalist_tier_answers_for_a_clip_of_any_language() -> TestResult {
    let sample = clip("eval_real", "en", 0)?;
    let decoded = recognizers(None)?.decode(Tier::Omni, "en", &sample.samples)?;
    assert_eq!(decoded.tier, Tier::Omni);
    assert!(!decoded.text.trim().is_empty());
    Ok(())
}

#[test]
fn sensevoice_reports_the_language_it_heard() -> TestResult {
    let mut recognizers = recognizers(None)?;
    for (set, lang) in [
        ("eval_real", "ja"),
        ("eval_real", "en"),
        ("eval_real_zhko", "zh"),
        ("eval_real_zhko", "ko"),
        ("eval_real_yue", "yue"),
    ] {
        let sample = clip(set, lang, 0)?;
        let decoded = recognizers.decode_whole(Tier::Sv, &sample.samples)?;
        assert_eq!(decoded.lang, LangTag::Known(lang.to_owned()), "{lang}");
    }
    assert!(
        recognizers
            .take_events()
            .iter()
            .all(|event| *event != ResidentEvent::LangUnknown)
    );
    Ok(())
}

#[test]
fn a_tier_that_does_not_identify_languages_gives_no_language() -> TestResult {
    let sample = clip("eval_real", "ja", 0)?;
    let decoded = recognizers(None)?.decode_whole(Tier::Rz, &sample.samples)?;
    assert_eq!(decoded.lang, LangTag::NotApplicable);
    Ok(())
}

fn kinds(events: &[ResidentEvent]) -> Vec<String> {
    events
        .iter()
        .map(|event| match event {
            ResidentEvent::Loaded { tier, .. } => format!("loaded {}", tier.name()),
            ResidentEvent::Evicted { tier } => format!("evicted {}", tier.name()),
            ResidentEvent::Substituted {
                requested, used, ..
            } => {
                format!("substituted {} by {}", requested.name(), used.name())
            }
            ResidentEvent::CutIntoChunks { .. } => String::from("chunks"),
            ResidentEvent::LangUnknown => String::from("lang unknown"),
        })
        .collect()
}

/// Decodes one clip with each of `tiers` in turn and returns the events.
fn run_tiers(
    cap: Option<usize>,
    tiers: &[(Tier, &str, &str)],
) -> Result<(Recognizers, Vec<String>), Box<dyn std::error::Error>> {
    let mut recognizers = recognizers(cap)?;
    for (tier, set, lang) in tiers {
        let sample = clip(set, lang, 0)?;
        recognizers.decode(*tier, lang, &sample.samples)?;
    }
    let events = kinds(&recognizers.take_events());
    Ok((recognizers, events))
}

const FIVE_LANGUAGES: [(Tier, &str, &str); 4] = [
    (Tier::Rz, "eval_real", "ja"),
    (Tier::V3, "eval_real", "en"),
    (Tier::Pz, "eval_real_zhko", "zh"),
    (Tier::Sv, "eval_real_zhko", "ko"),
];

#[test]
fn least_recently_used_recognizer_is_dropped_at_the_cap() -> TestResult {
    let (recognizers, events) = run_tiers(Some(2), &FIVE_LANGUAGES)?;
    assert_eq!(
        events,
        [
            "loaded rz",
            "loaded v3",
            "loaded pz",
            // The victim goes before the new recognizer is built, so the cap holds in memory.
            "evicted v3",
            "loaded sv",
        ]
    );
    // The Japanese tier is not counted: two others are resident beside it.
    assert_eq!(recognizers.resident(), [Tier::Rz, Tier::Pz, Tier::Sv]);
    Ok(())
}

#[test]
fn default_cap_drops_nothing_for_five_languages() -> TestResult {
    let (recognizers, events) = run_tiers(Some(3), &FIVE_LANGUAGES)?;
    assert!(
        events.iter().all(|event| !event.starts_with("evicted")),
        "{events:?}"
    );
    assert_eq!(
        recognizers.resident(),
        [Tier::Rz, Tier::Pz, Tier::Sv, Tier::V3]
    );
    Ok(())
}

#[test]
fn a_tier_used_again_is_kept_and_the_one_not_used_goes() -> TestResult {
    let mut again = FIVE_LANGUAGES.to_vec();
    again.insert(3, (Tier::V3, "eval_real", "en"));
    let (_, events) = run_tiers(Some(2), &again)?;
    assert_eq!(
        events.iter().rev().nth(1).map(String::as_str),
        Some("evicted pz")
    );
    assert_eq!(events.last().map(String::as_str), Some("loaded sv"));
    Ok(())
}

#[test]
fn a_tier_that_is_not_installed_gives_way_once_and_says_which() -> TestResult {
    let sample = clip("eval_real", "en", 0)?;
    let mut recognizers = recognizers(None)?;
    let first = recognizers.decode(Tier::V2, "en", &sample.samples)?;
    assert_eq!(first.tier, Tier::V3);
    let events = recognizers.take_events();
    assert_eq!(kinds(&events), ["loaded v3", "substituted v2 by v3"]);
    assert!(matches!(
        events.last(),
        Some(ResidentEvent::Substituted {
            reason: Reason::Missing,
            ..
        })
    ));
    recognizers.decode(Tier::V2, "en", &sample.samples)?;
    assert!(recognizers.take_events().is_empty(), "said twice");
    Ok(())
}

#[test]
fn a_non_suspicious_buffer_is_byte_identical_to_the_whole_decode() -> TestResult {
    // A short, dense clip is never suspected, so the retry changes nothing.
    let sample = clip("eval_real", "ja", 0)?;
    let mut recognizers = recognizers(None)?;
    let whole = recognizers.decode_whole(Tier::Rz, &sample.samples)?;
    let decoded = recognizers.decode(Tier::Rz, "ja", &sample.samples)?;
    assert_eq!(decoded.text, whole.text);
    Ok(())
}

/// Eight clips of one recording set with half a second of silence after each:
/// the buffer and the sentences it holds.
fn long_buffer() -> Result<(Vec<f32>, String), Box<dyn std::error::Error>> {
    let gap = vec![0.0_f32; (SAMPLE_RATE / 2) as usize];
    let mut buffer = Vec::new();
    let mut sentences = String::new();
    for index in 0..8 {
        let sample = clip("eval_real", "ja", index)?;
        buffer.extend(&sample.samples);
        buffer.extend(&gap);
        sentences.push_str(&sample.reference);
    }
    Ok((buffer, sentences))
}

#[test]
fn a_dropped_multi_utterance_buffer_is_recovered_by_the_retry() -> TestResult {
    let (buffer, sentences) = long_buffer()?;
    let mut recognizers = recognizers(None)?;
    let whole = recognizers.decode_whole(Tier::Rz, &buffer)?;
    let decoded = recognizers.decode(Tier::Rz, "ja", &buffer)?;
    let (before, after) = (
        rate(&sentences, &whole.text),
        rate(&sentences, &decoded.text),
    );
    assert!(
        after + 100_000 < before,
        "the retry left the error at {after} millionths, from {before}"
    );
    assert!(decoded.text.chars().count() > 2 * whole.text.chars().count());
    Ok(())
}

#[test]
fn pause_free_buffer_has_nothing_to_split() -> TestResult {
    // Noise with no silence in it, ten seconds long: nothing to cut at.
    let mut state = 0x2545_f491_u32;
    let mut level = 0.0_f32;
    let mut noise = Vec::new();
    for _ in 0..(10 * SAMPLE_RATE) {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        level += (f32::from((state >> 16) as u16) / 32_768.0 - 1.0) * 0.05;
        noise.push(level.clamp(-1.0, 1.0));
    }
    let mut splitter = Splitter::new(&models_dir()?);
    assert!(splitter.speech_pieces(&noise)?.0.is_none());
    // Too short to hold two utterances, whatever it holds.
    let (pieces, seconds) = splitter.speech_pieces(&vec![0.0; 2 * SAMPLE_RATE as usize])?;
    assert!(pieces.is_none());
    assert_eq!(seconds, 2.0);
    Ok(())
}

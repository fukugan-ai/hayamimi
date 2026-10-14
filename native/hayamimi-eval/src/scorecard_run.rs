//! RP-1: the Rust run of the scorecard. Every clip of every set is decoded by
//! the recognizer tier of its language, then the digit pass of the text
//! pipeline is applied and the clip is scored.
//!
//! No language detector runs here: the route that detects languages is a
//! separate slice. Until it joins, the tier is chosen from the manifest's
//! true language, the card says so (`lid: oracle`), and the count of right
//! detections stays empty, so a run of this kind cannot pass the LID check.

use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::time::Instant;

use hayamimi_asr::audio::read_wav;
use hayamimi_asr::itn::{self, Overrides};
use hayamimi_asr::{Recognizers, ResidentEvent, Tier};

use crate::error::EvalError;
use crate::manifest::read_rows;
use crate::score::Scorer;
use crate::scorecard::{ClipRow, Lid, Scorecard, summarize};

/// Threads of each recognizer, as the Python scorecard uses.
const THREADS: i32 = 6;

pub struct Options {
    pub models: PathBuf,
    pub sets: Vec<PathBuf>,
    pub max_resident: Option<NonZeroUsize>,
}

/// A clip of an evaluation set.
struct Clip {
    name: String,
    lang: String,
    reference: String,
    path: PathBuf,
}

/// The tier that answers for `lang` on the default route.
pub fn tier_of(lang: &str) -> Result<Tier, EvalError> {
    match lang {
        "ja" => Ok(Tier::Rz),
        "en" => Ok(Tier::V3),
        "zh" => Ok(Tier::Pz),
        "ko" | "yue" => Ok(Tier::Sv),
        other => Err(EvalError::Invalid(format!("no tier answers for `{other}`"))),
    }
}

fn text_of<'a>(row: &'a crate::manifest::Row, key: &str, set: &Path) -> Result<&'a str, EvalError> {
    row.get(key)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| EvalError::Manifest(format!("{}: a row has no `{key}`", set.display())))
}

/// The clips a set's `manifest.json` lists.
fn load_set(set: &Path) -> Result<Vec<Clip>, EvalError> {
    read_rows(&set.join("manifest.json"))?
        .iter()
        .map(|row| {
            let name = text_of(row, "wav", set)?;
            Ok(Clip {
                name: name.to_owned(),
                lang: text_of(row, "lang", set)?.to_owned(),
                reference: text_of(row, "ref", set)?.to_owned(),
                path: set.join(name),
            })
        })
        .collect()
}

/// The name of the log event a recognizer event stands for, if it is a fallback.
fn fallback_event(event: &ResidentEvent) -> Option<&'static str> {
    match event {
        ResidentEvent::Substituted { .. } => Some("model.fallback.substitute"),
        ResidentEvent::CutIntoChunks { .. } => Some("model.fallback.chunks"),
        ResidentEvent::Loaded { .. }
        | ResidentEvent::Evicted { .. }
        | ResidentEvent::LangUnknown => None,
    }
}

/// Real-time factor in millionths: seconds of work per second of audio.
fn rtf_micros(work_us: u128, dur_ms: u64) -> Result<u64, EvalError> {
    if dur_ms == 0 {
        return Err(EvalError::Invalid(String::from("a clip lasts 0 ms")));
    }
    let micros = work_us * 1000 / u128::from(dur_ms);
    u64::try_from(micros).map_err(|_| EvalError::Invalid(String::from("the work took too long")))
}

struct Run {
    recognizers: Recognizers,
    scorer: Scorer,
    clips: Vec<ClipRow>,
    fallbacks: Vec<String>,
}

impl Run {
    fn note_events(&mut self) {
        for event in self.recognizers.take_events() {
            let Some(name) = fallback_event(&event) else {
                continue;
            };
            if !self.fallbacks.iter().any(|known| known == name) {
                self.fallbacks.push(name.to_owned());
            }
        }
    }

    fn score_clip(&mut self, clip: &Clip) -> Result<(), EvalError> {
        let samples = read_wav(&clip.path)
            .map_err(|error| EvalError::Invalid(format!("{}: {error}", clip.path.display())))?;
        let dur_ms = samples.len() as u64 / 16;
        let started = Instant::now();
        let decoded = self
            .recognizers
            .decode(tier_of(&clip.lang)?, &clip.lang, &samples)
            .map_err(|error| EvalError::Invalid(format!("{}: {error}", clip.path.display())))?;
        let work_us = started.elapsed().as_micros();
        let text = itn::convert(&decoded.text, &clip.lang, &Overrides::default());
        let err_micros = self
            .scorer
            .score(&clip.lang, &clip.reference, &text)
            .micros();
        self.clips.push(ClipRow {
            name: clip.name.clone(),
            text,
            lang: clip.lang.clone(),
            detected: None,
            tier: decoded.tier.name().to_owned(),
            err_micros,
            dur_ms,
            rtf_micros: rtf_micros(work_us, dur_ms)?,
        });
        self.note_events();
        Ok(())
    }
}

/// The card of a run and the clips it was made from.
pub struct Outcome {
    pub card: Scorecard,
    pub clips: Vec<ClipRow>,
}

/// Runs every set.
pub fn run(options: &Options) -> Result<Outcome, EvalError> {
    let mut run = Run {
        recognizers: Recognizers::new(&options.models, THREADS, options.max_resident),
        scorer: Scorer::new()?,
        clips: Vec::new(),
        fallbacks: Vec::new(),
    };
    for set in &options.sets {
        for clip in load_set(set)? {
            run.score_clip(&clip)?;
        }
    }
    let card = Scorecard {
        engine: String::from("hayamimi"),
        lid: Lid::Oracle,
        rows: summarize(&run.clips)?,
        fallbacks: run.fallbacks,
    };
    Ok(Outcome {
        card,
        clips: run.clips,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use hayamimi_asr::Reason;

    #[test]
    fn each_language_of_the_scorecard_has_its_tier() -> Result<(), EvalError> {
        let tiers: Vec<Tier> = ["ja", "en", "zh", "ko", "yue"]
            .into_iter()
            .map(tier_of)
            .collect::<Result<_, _>>()?;
        assert_eq!(tiers, [Tier::Rz, Tier::V3, Tier::Pz, Tier::Sv, Tier::Sv]);
        Ok(())
    }

    #[test]
    fn a_language_with_no_tier_is_refused() {
        assert!(tier_of("fr").is_err());
    }

    #[test]
    fn a_substitution_and_a_chunked_cut_are_fallbacks_and_a_load_is_not() {
        let substituted = ResidentEvent::Substituted {
            requested: Tier::V2,
            used: Tier::V3,
            reason: Reason::Missing,
        };
        let chunks = ResidentEvent::CutIntoChunks {
            reason: Reason::Missing,
        };
        let loaded = ResidentEvent::Loaded {
            tier: Tier::Rz,
            ms: 1.0,
        };
        assert_eq!(
            fallback_event(&substituted),
            Some("model.fallback.substitute")
        );
        assert_eq!(fallback_event(&chunks), Some("model.fallback.chunks"));
        assert_eq!(fallback_event(&loaded), None);
    }

    #[test]
    fn the_real_time_factor_is_work_per_second_of_audio_in_millionths() -> Result<(), EvalError> {
        // 450 ms of work on 5 s of audio: 0.09.
        assert_eq!(rtf_micros(450_000, 5_000)?, 90_000);
        Ok(())
    }

    #[test]
    fn a_set_without_a_manifest_is_named() {
        let said = load_set(Path::new("no/such/set"))
            .err()
            .map(|error| error.to_string());
        assert!(
            said.as_deref()
                .is_some_and(|text| text.contains("no/such/set")),
            "{said:?}"
        );
    }
}

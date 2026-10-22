//! RP-1: the Rust run of the scorecard. Every clip of every set goes through
//! the language route of the program (identifier, router, recognizer tiers),
//! then the digit pass of the text pipeline is applied for the language the
//! route chose, and the clip is scored against the reference.
//!
//! Each set starts a fresh session and its clips follow one another in it,
//! as in the Python scorecard: only the first clip of a set opens a session.
//! The language the route chose is kept per clip, which is what the count of
//! right detections is made from.

use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::time::Instant;

use hayamimi_asr::audio::read_wav;
use hayamimi_asr::itn::{self, Overrides};
use hayamimi_asr::lid::{self, Lid};
use hayamimi_asr::route::{Policy, Request, Router};
use hayamimi_asr::{DecodeCap, Gpu, Recognizers, ResidentEvent};

use crate::error::EvalError;
use crate::manifest::read_rows;
use crate::score::Scorer;
use crate::scorecard::{ClipRow, Lid as LidMode, Scorecard, summarize};

/// Threads of each recognizer, as the Python scorecard uses.
const THREADS: i32 = 6;

pub struct Options {
    pub models: PathBuf,
    pub sets: Vec<PathBuf>,
    pub max_resident: Option<NonZeroUsize>,
    /// The longest audio one decode of the Japanese tier reads.
    pub decode_cap: DecodeCap,
}

/// A clip of an evaluation set.
struct Clip {
    name: String,
    lang: String,
    reference: String,
    path: PathBuf,
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
        | ResidentEvent::LangUnknown
        | ResidentEvent::DecodeChunked { .. } => None,
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
    router: Router<Recognizers, Lid>,
    scorer: Scorer,
    clips: Vec<ClipRow>,
    fallbacks: Vec<String>,
}

/// The router of the default mode over the recognizers and the identifier.
fn open_router(options: &Options) -> Result<Router<Recognizers, Lid>, EvalError> {
    let opened = lid::open(&options.models, Gpu::Try)
        .map_err(|error| EvalError::Invalid(format!("the identifier: {error}")))?;
    let Some(identifier) = opened.lid else {
        return Err(EvalError::Invalid(String::from(
            "the identifier is not installed, so no language can be detected",
        )));
    };
    let recognizers = Recognizers::new(
        &options.models,
        THREADS,
        options.max_resident,
        options.decode_cap,
    );
    Ok(Router::new(
        recognizers,
        Some(identifier),
        Policy::balanced(),
    ))
}

impl Run {
    fn note_events(&mut self) {
        for event in self.router.decoder_mut().take_events() {
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
        let request = Request::live(&samples);
        let started = Instant::now();
        let read = self
            .router
            .transcribe(&request)
            .map_err(|error| EvalError::Invalid(format!("{}: {error}", clip.path.display())))?;
        let work_us = started.elapsed().as_micros();
        let text = itn::convert(&read.text, &read.lang, &Overrides::default());
        let err_micros = self
            .scorer
            .score(&clip.lang, &clip.reference, &text)
            .micros();
        self.clips.push(ClipRow {
            name: clip.name.clone(),
            text,
            lang: clip.lang.clone(),
            detected: Some(read.lang),
            tier: read.tier.name().to_owned(),
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
        router: open_router(options)?,
        scorer: Scorer::new()?,
        clips: Vec::new(),
        fallbacks: Vec::new(),
    };
    for set in &options.sets {
        // One session per manifest, as the Python scorecard does: the clips of
        // a set follow one another, so the first one alone opens a session.
        run.router.reset_session();
        for clip in load_set(set)? {
            run.score_clip(&clip)?;
        }
    }
    let card = Scorecard {
        engine: String::from("hayamimi"),
        lid: LidMode::Detected,
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
    use hayamimi_asr::{Reason, Tier};

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

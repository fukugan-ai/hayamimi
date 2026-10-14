//! When a new language detection becomes the language of the session.
//!
//! The identifier is one model's guess and has no confidence to threshold. A
//! misfire on babble or a jingle must not move the session, and a real switch
//! must not wait long. Two rules do that:
//!
//! - For the five languages SenseVoice's own identifier covers, a switch needs
//!   both identifiers to agree on the same audio ([`resolve_dual_confirm`]).
//! - For every other language, a switch needs the same new language on
//!   several detections in a row, each long enough to be speech
//!   ([`resolve_sticky`]).
//!
//! The refine pass, which re-judges a merged group of segments, gates the same
//! way ([`resolve_refine_lang`]). Every function here is pure: the caller
//! supplies the SenseVoice reading of the same audio.

/// The languages SenseVoice's identifier can arbitrate.
pub const DUAL_CONFIRM_LANGS: [&str; 5] = ["ja", "en", "zh", "ko", "yue"];
/// A candidate shorter than this is taken for non-speech and never confirms a switch.
pub const MIN_PROBE_S: f64 = 0.5;
/// A merged group shorter than this is not judged again: below it, a lone
/// re-judgment is no better than a coin flip and can undo a live decision.
pub const REFINE_MIN_REGROUP_S: f64 = 2.5;

/// A candidate language waiting for its confirmations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    pub lang: String,
    pub count: u32,
}

/// What the identifier said about one segment.
#[derive(Debug, Clone, Copy)]
pub struct Detection<'a> {
    pub lang: &'a str,
    /// Seconds of speech in the segment, when known.
    pub speech_s: Option<f64>,
    /// SenseVoice's reading of this audio, when the session has just begun
    /// and the probe ran.
    pub probe_lang: Option<&'a str>,
}

/// The two knobs of the hysteresis.
#[derive(Debug, Clone, Copy)]
pub struct Hysteresis {
    /// A candidate shorter than this many seconds does not count.
    pub min_switch_s: f64,
    /// Detections of one new language in a row that confirm a switch.
    pub confirm: u32,
}

/// The decision about one detection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sticky<'a> {
    /// The language to read this segment in.
    pub lang: &'a str,
    /// The generalist must not be asked when the specialist comes back empty.
    pub suppress_fallback: bool,
    /// The candidate, as it stands after this detection.
    pub pending: Option<Pending>,
}

fn counted(pending: Option<Pending>, lang: &str) -> Pending {
    match pending {
        Some(found) if found.lang == lang => Pending {
            count: found.count + 1,
            ..found
        },
        _ => Pending {
            lang: lang.to_owned(),
            count: 1,
        },
    }
}

/// The language to read a segment in while no switch is confirmed: the
/// session's own, or at the session's start the probe's, or the detection.
fn held_language<'a>(detection: &Detection<'a>, last: Option<&'a str>) -> &'a str {
    match (last, detection.probe_lang) {
        (Some(session), _) => session,
        (None, Some(probe)) => probe,
        (None, None) => detection.lang,
    }
}

/// Decides whether a detection is a real switch or is held at the session's
/// language. Staying on the current language needs no confirmation, so noise
/// cannot lock a session onto a wrong language.
///
/// A candidate shorter than `min_switch_s` neither starts nor extends nor
/// resets a pending one. The session's start goes through the same
/// confirmation as any switch, and until a candidate is confirmed the
/// segments are read in the probe's language rather than in a guess the probe
/// may contradict.
pub fn resolve_sticky<'a>(
    detection: &Detection<'a>,
    last: Option<&'a str>,
    hysteresis: Hysteresis,
    pending: Option<Pending>,
) -> Sticky<'a> {
    if last == Some(detection.lang) {
        return Sticky {
            lang: detection.lang,
            suppress_fallback: false,
            pending: None,
        };
    }
    let held = held_language(detection, last);
    if detection
        .speech_s
        .is_some_and(|speech| speech < hysteresis.min_switch_s)
    {
        return Sticky {
            lang: held,
            suppress_fallback: true,
            pending,
        };
    }
    let candidate = counted(pending, detection.lang);
    if candidate.count < hysteresis.confirm {
        return Sticky {
            lang: held,
            suppress_fallback: false,
            pending: Some(candidate),
        };
    }
    Sticky {
        lang: detection.lang,
        suppress_fallback: false,
        pending: None,
    }
}

/// Switch confirmation for the five languages both identifiers cover: the
/// switch is accepted when the two agree on the same audio. Returns the
/// language to read the segment in and whether this call is the reason the
/// session's language changed.
///
/// At the session's start there is no language to hold, so the segment is
/// read in SenseVoice's language, which is the more accurate of the two on
/// its own; the caller must not take a bootstrap shorter than [`MIN_PROBE_S`]
/// for the session's language. The first identifier cannot say Cantonese and
/// folds it into Chinese, so Chinese against a Cantonese probe is the only
/// way it can agree with one.
pub fn resolve_dual_confirm<'a>(
    lang: &'a str,
    last: Option<&'a str>,
    speech_s: Option<f64>,
    sv_lang: Option<&'a str>,
) -> (&'a str, bool) {
    if last == Some(lang) {
        return (lang, false);
    }
    let too_short = speech_s.is_some_and(|speech| speech < MIN_PROBE_S);
    let agreed = sv_lang == Some(lang);
    let Some(held) = last else {
        let resolved = match sv_lang {
            Some(probe) => probe,
            None => lang,
        };
        return (resolved, !too_short && agreed);
    };
    if too_short {
        return (held, false);
    }
    if agreed {
        return (lang, true);
    }
    if lang == "zh" && sv_lang == Some("yue") {
        return ("yue", true);
    }
    (held, false)
}

/// Whether the refine pass's re-judgment of a merged group replaces the fast
/// path's language: only when it differs, the group is long enough, and
/// SenseVoice's reading of the merged audio agrees with it. Returns the
/// language and whether it changed.
pub fn resolve_refine_lang<'a>(
    current: &'a str,
    whisper_lang: &'a str,
    sv_lang: Option<&'a str>,
    group_duration_s: f64,
) -> (&'a str, bool) {
    if whisper_lang == current || group_duration_s < REFINE_MIN_REGROUP_S {
        return (current, false);
    }
    if sv_lang == Some(whisper_lang) {
        return (whisper_lang, true);
    }
    (current, false)
}

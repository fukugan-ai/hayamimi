//! Which recognizer each language goes to, and what a missing one gives way to.
//!
//! The catalogue is tiered: Japanese to the fastest recognizer, Chinese and
//! Korean and Cantonese to the specialists, English and the other European
//! languages to the multilingual Parakeet, everything else to the
//! 1600-language generalist.

use super::RouteError;

/// A recognizer of the catalogue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tier {
    /// ReazonSpeech zipformer: Japanese, and the draft model of every session.
    Rz,
    /// Paraformer: Chinese.
    Pz,
    /// SenseVoice: Korean and Cantonese, and the second language identifier.
    Sv,
    /// Parakeet TDT v3: English and 24 European languages.
    V3,
    /// Parakeet TDT v2: English only, opted into.
    V2,
    /// Omnilingual CTC: everything else.
    Omni,
    /// Parakeet CTC Japanese: the second opinion on Japanese.
    Pja,
}

impl Tier {
    /// Every tier a recognizer can be built for.
    pub const ALL: [Tier; 7] = [
        Tier::Rz,
        Tier::Pz,
        Tier::Sv,
        Tier::V3,
        Tier::V2,
        Tier::Omni,
        Tier::Pja,
    ];

    /// The tier name used in events.
    pub fn name(self) -> &'static str {
        match self {
            Tier::Rz => "rz",
            Tier::Pz => "pz",
            Tier::Sv => "sv",
            Tier::V3 => "v3",
            Tier::V2 => "v2",
            Tier::Omni => "omni",
            Tier::Pja => "pja",
        }
    }
}

/// The language of the first tier. A session with no language known yet is
/// read by this tier's model.
pub const TIER0_LANG: &str = "ja";
pub const RZ_LANGS: [&str; 1] = ["ja"];
pub const PARA_LANGS: [&str; 1] = ["zh"];
pub const SV_LANGS: [&str; 2] = ["ko", "yue"];
/// Languages of the Parakeet TDT v3 model.
pub const V3_LANGS: [&str; 25] = [
    "bg", "hr", "cs", "da", "nl", "en", "et", "fi", "fr", "de", "el", "hu", "it", "lv", "lt", "mt",
    "pl", "pt", "ro", "sk", "sl", "es", "sv", "ru", "uk",
];

/// Whether a language code goes to a tier of its own. A forced language
/// outside these would ride the generalist for every segment.
pub fn is_routable(lang: &str) -> bool {
    [&RZ_LANGS[..], &PARA_LANGS, &SV_LANGS, &V3_LANGS]
        .iter()
        .any(|set| set.contains(&lang))
}

/// Which model reads English.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnTier {
    /// The multilingual model, which also covers the other V3 languages.
    V3,
    /// The English-only model, which reads English with fewer errors.
    V2,
}

/// Reads an `--en-tier` value.
pub fn resolve_en_tier(requested: &str) -> Result<EnTier, RouteError> {
    match requested {
        "v3" => Ok(EnTier::V3),
        "v2" => Ok(EnTier::V2),
        other => Err(RouteError::Invalid(format!(
            "unknown en_tier {other:?}, must be one of \"v3\", \"v2\""
        ))),
    }
}

/// The tier a language is routed to, before anything is known to be missing.
pub fn requested_tier(lang: &str, en_tier: EnTier) -> Tier {
    if RZ_LANGS.contains(&lang) {
        Tier::Rz
    } else if PARA_LANGS.contains(&lang) {
        Tier::Pz
    } else if SV_LANGS.contains(&lang) {
        Tier::Sv
    } else if lang == "en" && en_tier == EnTier::V2 {
        Tier::V2
    } else if V3_LANGS.contains(&lang) {
        Tier::V3
    } else {
        Tier::Omni
    }
}

/// The default an opted-in tier gives way to before the generic chain: the
/// English-only model gives way to the multilingual one, not to the Japanese
/// model, whose English is unpunctuated capitals.
pub(super) const TIER_FALLBACK: [(Tier, &[Tier]); 1] = [(Tier::V2, &[Tier::V3])];
/// The order a missing tier gives way in, Japanese first.
const GENERIC_CHAIN: [Tier; 5] = [Tier::Rz, Tier::Sv, Tier::V3, Tier::Pz, Tier::Omni];

/// The tiers to try for `requested`, in order, none twice.
pub fn fallback_chain(requested: Tier) -> Vec<Tier> {
    let mut chain = vec![requested];
    let defaults = TIER_FALLBACK
        .iter()
        .find(|(opt_in, _)| *opt_in == requested)
        .map(|(_, defaults)| *defaults);
    for candidate in defaults
        .into_iter()
        .flatten()
        .chain(GENERIC_CHAIN.iter())
        .copied()
    {
        if !chain.contains(&candidate) {
            chain.push(candidate);
        }
    }
    chain
}

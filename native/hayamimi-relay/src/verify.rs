//! Verification (V): the source and the edited translation are compared once,
//! by rules and tables, never by a language model.
//!
//! The checks run in a fixed order and the first one that fails names the
//! hold reason: form, number, negation, uncertainty, proper noun.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::lang::Lang;
use crate::logging::LogSink;
use crate::numbers;
use crate::watched::Watched;

pub const DEFAULT_MARKERS: &str = include_str!("../data/verify_markers.toml");
pub const DEFAULT_GLOSSARY: &str = include_str!("../data/glossary.toml");

/// The end-of-sequence marker a model may leave in its text.
pub const EOS_MARKER: &str = "</s>";

/// Why a line is withheld.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HoldReason {
    Number,
    Negation,
    Uncertainty,
    ProperNoun,
    Form,
    Unavailable,
}

impl HoldReason {
    /// The wire name of the reason.
    pub fn as_str(self) -> &'static str {
        match self {
            HoldReason::Number => "number",
            HoldReason::Negation => "negation",
            HoldReason::Uncertainty => "uncertainty",
            HoldReason::ProperNoun => "proper_noun",
            HoldReason::Form => "form",
            HoldReason::Unavailable => "unavailable",
        }
    }
}

/// The outcome of verification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Ok,
    Held(HoldReason),
}

/// Markers of one kind (negation or uncertainty) for one language.
#[derive(Clone, Debug, Deserialize)]
pub struct MarkerSet {
    pub markers: Vec<String>,
    /// Substrings counted wherever they occur (English `n't`).
    #[serde(default)]
    pub suffixes: Vec<String>,
    /// Phrases removed before markers are looked for.
    #[serde(default)]
    pub exclude: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct LangMarkers {
    pub negation: MarkerSet,
    pub uncertainty: MarkerSet,
}

#[derive(Clone, Debug, Deserialize)]
pub struct FormRules {
    pub prefixes: Vec<String>,
}

/// `verify_markers.toml`.
#[derive(Clone, Debug, Deserialize)]
pub struct Markers {
    pub form: FormRules,
    pub ja: LangMarkers,
    pub en: LangMarkers,
}

impl Markers {
    /// Parses the table. English entries are lower-cased and every exclude
    /// list is ordered longest first, so the longest phrase is removed first.
    pub fn parse(bytes: &[u8]) -> Result<Markers, String> {
        let text = std::str::from_utf8(bytes).map_err(|error| error.to_string())?;
        let mut markers: Markers = toml::from_str(text).map_err(|error| error.to_string())?;
        for set in [&mut markers.en.negation, &mut markers.en.uncertainty] {
            for list in [&mut set.markers, &mut set.suffixes, &mut set.exclude] {
                for entry in list.iter_mut() {
                    *entry = entry.to_lowercase();
                }
            }
        }
        for prefix in &mut markers.form.prefixes {
            *prefix = prefix.to_lowercase();
        }
        for set in [
            &mut markers.ja.negation,
            &mut markers.ja.uncertainty,
            &mut markers.en.negation,
            &mut markers.en.uncertainty,
        ] {
            set.exclude
                .sort_by_key(|entry| std::cmp::Reverse(entry.chars().count()));
        }
        Ok(markers)
    }

    fn embedded() -> Result<Markers, String> {
        Markers::parse(DEFAULT_MARKERS.as_bytes())
    }
}

/// One proper name and its spellings.
#[derive(Clone, Debug, Deserialize)]
pub struct Term {
    pub en: String,
    pub ja: Vec<String>,
}

/// `glossary.toml`.
#[derive(Clone, Debug, Deserialize)]
pub struct Glossary {
    #[serde(default, rename = "term")]
    pub terms: Vec<Term>,
}

impl Glossary {
    pub fn parse(bytes: &[u8]) -> Result<Glossary, String> {
        let text = std::str::from_utf8(bytes).map_err(|error| error.to_string())?;
        toml::from_str(text).map_err(|error| error.to_string())
    }

    fn embedded() -> Result<Glossary, String> {
        Glossary::parse(DEFAULT_GLOSSARY.as_bytes())
    }
}

/// Verifies one source and translation against already-loaded tables.
pub fn verify_with(
    markers: &Markers,
    glossary: &Glossary,
    src: &str,
    translation: &str,
    src_lang: Lang,
) -> Verdict {
    if !form_ok(markers, src, translation, src_lang.other()) {
        return Verdict::Held(HoldReason::Form);
    }
    if numbers::compared_values(src) != numbers::compared_values(translation) {
        return Verdict::Held(HoldReason::Number);
    }
    let (src_markers, dst_markers) = match src_lang {
        Lang::Ja => (&markers.ja, &markers.en),
        Lang::En => (&markers.en, &markers.ja),
    };
    let flag = |set: &MarkerSet, lang: Lang, text: &str| has_marker(set, lang, text);
    if flag(&src_markers.negation, src_lang, src)
        != flag(&dst_markers.negation, src_lang.other(), translation)
    {
        return Verdict::Held(HoldReason::Negation);
    }
    if flag(&src_markers.uncertainty, src_lang, src)
        != flag(&dst_markers.uncertainty, src_lang.other(), translation)
    {
        return Verdict::Held(HoldReason::Uncertainty);
    }
    if !proper_nouns_ok(glossary, src, translation, src_lang) {
        return Verdict::Held(HoldReason::ProperNoun);
    }
    Verdict::Ok
}

/// Whether a model's text carries the end-of-sequence marker anywhere.
pub fn contains_eos(text: &str) -> bool {
    text.contains(EOS_MARKER)
}

fn is_japanese_char(c: char) -> bool {
    matches!(c,
        '\u{3040}'..='\u{30FF}' | '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}'
        | '\u{FF66}'..='\u{FF9F}' | '々')
}

/// The form check: non-empty, no end marker, not the source itself, in the
/// target language's script, no preface, no quotes the source lacks.
fn form_ok(markers: &Markers, src: &str, translation: &str, target: Lang) -> bool {
    let text = translation.trim();
    if text.is_empty() || contains_eos(text) || text == src.trim() {
        return false;
    }
    let in_target_script = match target {
        Lang::Ja => text.chars().any(is_japanese_char),
        Lang::En => {
            text.chars().any(|c| c.is_ascii_alphabetic()) && !text.chars().any(is_japanese_char)
        }
    };
    if !in_target_script {
        return false;
    }
    let lower = text.to_lowercase();
    if markers
        .form
        .prefixes
        .iter()
        .any(|prefix| lower.starts_with(prefix.as_str()))
    {
        return false;
    }
    !wrapped_in_foreign_quotes(src, text)
}

fn wrapped_in_foreign_quotes(src: &str, text: &str) -> bool {
    const PAIRS: [(char, char); 6] = [
        ('"', '"'),
        ('“', '”'),
        ('‘', '’'),
        ('「', '」'),
        ('『', '』'),
        ('\'', '\''),
    ];
    let (first, last) = (text.chars().next(), text.chars().next_back());
    PAIRS.iter().any(|(open, close)| {
        first == Some(*open)
            && last == Some(*close)
            && text.chars().count() >= 2
            && !src.trim_start().starts_with(*open)
    })
}

fn has_marker(set: &MarkerSet, lang: Lang, text: &str) -> bool {
    match lang {
        Lang::Ja => {
            let cleaned = remove_substrings(text, &set.exclude);
            set.markers
                .iter()
                .any(|marker| cleaned.contains(marker.as_str()))
        }
        Lang::En => {
            let lower = text.to_lowercase().replace('’', "'");
            let cleaned = remove_phrases(&lower, &set.exclude);
            set.suffixes
                .iter()
                .any(|suffix| cleaned.contains(suffix.as_str()))
                || set
                    .markers
                    .iter()
                    .any(|marker| find_word(&cleaned, marker, 0).is_some())
        }
    }
}

/// `text` with every occurrence of each entry replaced by a separator that
/// no marker contains.
fn remove_substrings(text: &str, entries: &[String]) -> String {
    let mut cleaned = text.to_owned();
    for entry in entries {
        if cleaned.contains(entry.as_str()) {
            cleaned = cleaned.replace(entry.as_str(), "\u{1}");
        }
    }
    cleaned
}

/// Like [`remove_substrings`] but only whole-word occurrences.
fn remove_phrases(lower: &str, phrases: &[String]) -> String {
    let mut cleaned = lower.to_owned();
    for phrase in phrases {
        let mut from = 0;
        while let Some(at) = find_word(&cleaned, phrase, from) {
            cleaned.replace_range(at..at + phrase.len(), " ");
            from = at + 1;
        }
    }
    cleaned
}

/// The byte offset of the first occurrence of `needle` in `haystack` at or
/// after `from` that is not glued to a letter or digit on either side.
fn find_word(haystack: &str, needle: &str, from: usize) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }
    let mut start = from;
    while let Some(relative) = haystack.get(start..)?.find(needle) {
        let at = start + relative;
        let end = at + needle.len();
        let before_ok = haystack[..at]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric());
        let after_ok = haystack[end..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric());
        if before_ok && after_ok {
            return Some(at);
        }
        start = at + haystack[at..].chars().next().map_or(1, char::len_utf8);
    }
    None
}

/// Maximal runs of ASCII letters and digits that contain a letter and are
/// at least two characters long.
fn ascii_words(text: &str) -> Vec<&str> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| word.len() >= 2 && word.bytes().any(|b| b.is_ascii_alphabetic()))
        .collect()
}

fn proper_nouns_ok(glossary: &Glossary, src: &str, translation: &str, src_lang: Lang) -> bool {
    let src_lower = src.to_lowercase();
    let translation_lower = translation.to_lowercase();
    match src_lang {
        Lang::Ja => {
            let terms_ok = glossary.terms.iter().all(|term| {
                let present = term
                    .ja
                    .iter()
                    .any(|form| src_lower.contains(&form.to_lowercase()));
                !present || translation_lower.contains(&term.en.to_lowercase())
            });
            terms_ok
                && ascii_words(src)
                    .iter()
                    .all(|word| translation_lower.contains(&word.to_lowercase()))
        }
        Lang::En => glossary.terms.iter().all(|term| {
            let present = find_word(&src_lower, &term.en.to_lowercase(), 0).is_some();
            !present
                || term
                    .ja
                    .iter()
                    .any(|form| translation_lower.contains(&form.to_lowercase()))
        }),
    }
}

/// The verification step with its tables, re-read when their files change.
pub struct Verifier {
    markers: Watched<Markers>,
    glossary: Watched<Glossary>,
}

impl Verifier {
    /// Opens the built-in tables, overlaid by `verify_markers.toml` and
    /// `glossary.toml` in `tables_dir` when those files exist.
    pub fn open(tables_dir: Option<&Path>, sink: &mut dyn LogSink) -> Result<Verifier, String> {
        let file = |name: &str| -> Option<PathBuf> { tables_dir.map(|dir| dir.join(name)) };
        Ok(Verifier {
            markers: Watched::open(
                "verify_markers",
                file("verify_markers.toml"),
                Markers::embedded,
                Markers::parse,
                sink,
            )?,
            glossary: Watched::open(
                "glossary",
                file("glossary.toml"),
                Glossary::embedded,
                Glossary::parse,
                sink,
            )?,
        })
    }

    /// Verifies one line, first picking up any table file that changed.
    pub fn verify(
        &mut self,
        src: &str,
        translation: &str,
        src_lang: Lang,
        sink: &mut dyn LogSink,
    ) -> Verdict {
        self.markers.refresh(sink);
        self.glossary.refresh(sink);
        verify_with(
            self.markers.current(),
            self.glossary.current(),
            src,
            translation,
            src_lang,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tables() -> Result<(Markers, Glossary), String> {
        Ok((Markers::embedded()?, Glossary::embedded()?))
    }

    fn check(src: &str, translation: &str, lang: Lang) -> Result<Verdict, String> {
        let (markers, glossary) = tables()?;
        Ok(verify_with(&markers, &glossary, src, translation, lang))
    }

    #[test]
    fn a_matching_pair_passes() {
        assert_eq!(check("こんにちは。", "Hello.", Lang::Ja), Ok(Verdict::Ok));
    }

    #[test]
    fn a_changed_amount_is_held_for_number() {
        let said = check(
            "予算は五百万円を想定しています。",
            "The budget is assumed to be 1,500,000 yen.",
            Lang::Ja,
        );
        assert_eq!(said, Ok(Verdict::Held(HoldReason::Number)));
    }

    #[test]
    fn a_lost_negation_is_held() {
        let said = check(
            "Sorry, I was not there.",
            "すみません、そこにいました。",
            Lang::En,
        );
        assert_eq!(said, Ok(Verdict::Held(HoldReason::Negation)));
    }

    #[test]
    fn an_added_hedge_is_held_for_uncertainty() {
        let said = check(
            "The delivery is delayed.",
            "納品が遅れる可能性があります。",
            Lang::En,
        );
        assert_eq!(said, Ok(Verdict::Held(HoldReason::Uncertainty)));
    }

    #[test]
    fn a_dropped_product_name_is_held_for_proper_noun() {
        let said = check("Zoomで共有します。", "I will share it.", Lang::Ja);
        assert_eq!(said, Ok(Verdict::Held(HoldReason::ProperNoun)));
    }

    #[test]
    fn the_marker_in_a_translation_is_held_for_form() {
        let said = check("こんにちは。", "Hello.</s>", Lang::Ja);
        assert_eq!(said, Ok(Verdict::Held(HoldReason::Form)));
    }

    #[test]
    fn an_unchanged_source_is_held_for_form() {
        assert_eq!(
            check("Hello.", "Hello.", Lang::En),
            Ok(Verdict::Held(HoldReason::Form))
        );
    }

    #[test]
    fn quotes_the_source_lacks_are_held_for_form() {
        let said = check(
            "すみません、少しゆっくり話してください。",
            "\u{201c}Please speak slowly.\u{201d}",
            Lang::Ja,
        );
        assert_eq!(said, Ok(Verdict::Held(HoldReason::Form)));
    }

    #[test]
    fn a_preface_is_held_for_form() {
        let said = check("こんにちは。", "Here is the translation: Hello.", Lang::Ja);
        assert_eq!(said, Ok(Verdict::Held(HoldReason::Form)));
    }

    #[test]
    fn wrong_script_is_held_for_form() {
        assert_eq!(
            check("こんにちは。", "こんにちは。 Hello", Lang::Ja),
            Ok(Verdict::Held(HoldReason::Form))
        );
        assert_eq!(
            check("Hello.", "Hello again.", Lang::En),
            Ok(Verdict::Held(HoldReason::Form))
        );
    }

    #[test]
    fn an_empty_translation_is_held_for_form() {
        assert_eq!(
            check("こんにちは。", "  ", Lang::Ja),
            Ok(Verdict::Held(HoldReason::Form))
        );
    }

    #[test]
    fn a_polite_apology_is_not_a_negation() {
        let said = check(
            "すみません、少し音声が途切れました。",
            "Sorry, the audio cut out a bit.",
            Lang::Ja,
        );
        assert_eq!(said, Ok(Verdict::Ok));
    }

    #[test]
    fn how_about_is_not_a_hedge() {
        let said = check(
            "来週の火曜日はいかがですか。",
            "How about next Tuesday?",
            Lang::Ja,
        );
        assert_eq!(said, Ok(Verdict::Ok));
    }

    #[test]
    fn find_word_respects_word_boundaries() {
        assert_eq!(find_word("a note", "not", 0), None);
        assert_eq!(find_word("it is not so", "not", 0), Some(6));
        assert_eq!(find_word("", "x", 0), None);
        assert_eq!(find_word("no", "", 0), None);
    }

    #[test]
    fn contains_eos_finds_the_marker_anywhere() {
        assert!(contains_eos("a</s>b"));
        assert!(!contains_eos("a/s>b"));
    }

    #[test]
    fn markers_reject_garbage() {
        assert!(Markers::parse(b"[[[").is_err());
        assert!(Glossary::parse(&[0xff]).is_err());
    }

    #[test]
    fn the_verifier_uses_the_built_in_tables() -> Result<(), Box<dyn std::error::Error>> {
        let mut sink = crate::logging::NullSink;
        let mut verifier = Verifier::open(None, &mut sink)?;
        let verdict = verifier.verify("こんにちは。", "Hello.", Lang::Ja, &mut sink);
        assert_eq!(verdict, Verdict::Ok);
        Ok(())
    }
}

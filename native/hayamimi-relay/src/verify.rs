//! Verification (V): the source and the edited translation are compared once,
//! by rules and tables, never by a language model.
//!
//! The checks run in a fixed order and the first one that fails names the
//! hold reason: form, number, negation, uncertainty, proper noun.
//!
//! Into Chinese, Korean and Spanish a disagreement in hedging is not a hold
//! reason: the translation goes out and [`HEDGE_EVENT`] records the hedge
//! words of both sides. Negation, numbers, form and names hold there as
//! everywhere, and hedging still holds into Japanese and English.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::lang::Lang;
use crate::logging::{Level, LogEvent, LogSink};
use crate::numbers::{self, Tongue};
use crate::target::Target;
use crate::watched::Watched;

pub const DEFAULT_MARKERS: &str = include_str!("../data/verify_markers.toml");
pub const DEFAULT_GLOSSARY: &str = include_str!("../data/glossary.toml");

/// The event that records a hedge disagreement that did not hold a line.
pub const HEDGE_EVENT: &str = "translate.hedge.logged";

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

/// How the markers of a language are looked for in a text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scan {
    /// Anywhere in the text: languages written without spaces between words,
    /// or with endings glued to the stem.
    Substring,
    /// As whole words of the lower-cased text.
    Words,
}

impl Scan {
    fn of(lang: Lang) -> Scan {
        match lang {
            Lang::Ja => Scan::Substring,
            Lang::En => Scan::Words,
        }
    }
}

/// `verify_markers.toml`.
#[derive(Clone, Debug, Deserialize)]
pub struct Markers {
    pub form: FormRules,
    pub ja: LangMarkers,
    pub en: LangMarkers,
    /// The languages of `--translate` that have a table of their own. A
    /// language without one is not checked for negation and uncertainty.
    #[serde(default)]
    pub zh: Option<LangMarkers>,
    #[serde(default)]
    pub ko: Option<LangMarkers>,
    #[serde(default)]
    pub es: Option<LangMarkers>,
}

impl Markers {
    /// Parses the table. The entries of the word-matched languages are
    /// lower-cased and every exclude list is ordered longest first, so the
    /// longest phrase is removed first.
    pub fn parse(bytes: &[u8]) -> Result<Markers, String> {
        let text = std::str::from_utf8(bytes).map_err(|error| error.to_string())?;
        let mut markers: Markers = toml::from_str(text).map_err(|error| error.to_string())?;
        for prefix in &mut markers.form.prefixes {
            *prefix = prefix.to_lowercase();
        }
        let mut word_matched = vec![&mut markers.en];
        word_matched.extend(markers.es.as_mut());
        for table in word_matched {
            for set in [&mut table.negation, &mut table.uncertainty] {
                for list in [&mut set.markers, &mut set.suffixes, &mut set.exclude] {
                    for entry in list.iter_mut() {
                        *entry = entry.to_lowercase();
                    }
                }
            }
        }
        let mut tables = vec![&mut markers.ja, &mut markers.en];
        tables.extend(markers.zh.as_mut());
        tables.extend(markers.ko.as_mut());
        tables.extend(markers.es.as_mut());
        for table in tables {
            for set in [&mut table.negation, &mut table.uncertainty] {
                set.exclude
                    .sort_by_key(|entry| std::cmp::Reverse(entry.chars().count()));
            }
        }
        Ok(markers)
    }

    /// The table of a language of `--translate` and how it is scanned; `None`
    /// for a language without one.
    fn table_for(&self, code: &str) -> Option<(&LangMarkers, Scan)> {
        match code {
            "zh" => self.zh.as_ref().map(|table| (table, Scan::Substring)),
            "ko" => self.ko.as_ref().map(|table| (table, Scan::Substring)),
            "es" => self.es.as_ref().map(|table| (table, Scan::Words)),
            _ => None,
        }
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
    let dst_code = src_lang.other().code();
    if !numbers_agree(src, src_lang.code(), translation, dst_code) {
        return Verdict::Held(HoldReason::Number);
    }
    let (src_markers, dst_markers) = match src_lang {
        Lang::Ja => (&markers.ja, &markers.en),
        Lang::En => (&markers.en, &markers.ja),
    };
    let (src_side, dst_side) = (
        (src_markers, Scan::of(src_lang)),
        (dst_markers, Scan::of(src_lang.other())),
    );
    if let Some(reason) = marker_mismatch(src_side, dst_side, src, translation) {
        return Verdict::Held(reason);
    }
    if !proper_nouns_ok(glossary, src, translation, src_lang) {
        return Verdict::Held(HoldReason::ProperNoun);
    }
    Verdict::Ok
}

/// A marker table and how its language is scanned.
type Side<'a> = (&'a LangMarkers, Scan);

/// Whether exactly one of the source and the translation is negated, each
/// side read with its own table and scan.
fn negation_differs(
    (src_table, src_scan): Side<'_>,
    (dst_table, dst_scan): Side<'_>,
    src: &str,
    translation: &str,
) -> bool {
    has_marker(&src_table.negation, src_scan, src)
        != has_marker(&dst_table.negation, dst_scan, translation)
}

/// The hedge words of a source and of its translation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HedgeMismatch<'a> {
    /// The first hedge word of the source, as the table spells it.
    pub src: Option<&'a str>,
    /// The first hedge word of the translation, as the table spells it.
    pub dst: Option<&'a str>,
}

/// The hedge words of both texts when exactly one of them is hedged.
fn hedge_mismatch<'a>(
    (src_table, src_scan): Side<'a>,
    (dst_table, dst_scan): Side<'a>,
    src: &str,
    translation: &str,
) -> Option<HedgeMismatch<'a>> {
    let seen = HedgeMismatch {
        src: find_marker(&src_table.uncertainty, src_scan, src),
        dst: find_marker(&dst_table.uncertainty, dst_scan, translation),
    };
    (seen.src.is_some() != seen.dst.is_some()).then_some(seen)
}

/// The first of negation and uncertainty on which the source and the
/// translation disagree, each side read with its own table and scan.
fn marker_mismatch(
    src_side: Side<'_>,
    dst_side: Side<'_>,
    src: &str,
    translation: &str,
) -> Option<HoldReason> {
    if negation_differs(src_side, dst_side, src, translation) {
        return Some(HoldReason::Negation);
    }
    hedge_mismatch(src_side, dst_side, src, translation).map(|_| HoldReason::Uncertainty)
}

/// Verifies a translation into `target`. Into the other language of the live
/// pair this is [`verify_with`]. Into any other language only what holds in
/// every language is checked: the form, the numbers, and the Latin names a
/// Japanese source carries. No table of negation or hedging markers exists for
/// those languages, so those two checks are not made. A language that has a
/// table in `verify_markers.toml` (Chinese, Korean, Spanish) is also checked
/// for negation against the table of the source language; a disagreement in
/// hedging does not hold there and is read by [`hedge_into`].
pub fn verify_into(
    markers: &Markers,
    glossary: &Glossary,
    src: &str,
    translation: &str,
    src_lang: Lang,
    target: &Target,
) -> Verdict {
    if target.lang() == Some(src_lang.other()) {
        return verify_with(markers, glossary, src, translation, src_lang);
    }
    if !form_into(markers, src, translation, target) {
        return Verdict::Held(HoldReason::Form);
    }
    if !numbers_agree(src, src_lang.code(), translation, target.code) {
        return Verdict::Held(HoldReason::Number);
    }
    if let Some(reason) = marker_mismatch_into(markers, src, translation, src_lang, target) {
        return Verdict::Held(reason);
    }
    if src_lang == Lang::Ja && !latin_names_kept(src, translation) {
        return Verdict::Held(HoldReason::ProperNoun);
    }
    Verdict::Ok
}

/// The table of the source language and how it is scanned.
fn source_side(markers: &Markers, src_lang: Lang) -> Side<'_> {
    let table = match src_lang {
        Lang::Ja => &markers.ja,
        Lang::En => &markers.en,
    };
    (table, Scan::of(src_lang))
}

/// The negation check for a translation into `target`; `None` when the target
/// has no table (negation is then unchecked) or the negation agrees.
fn marker_mismatch_into(
    markers: &Markers,
    src: &str,
    translation: &str,
    src_lang: Lang,
    target: &Target,
) -> Option<HoldReason> {
    let dst_side = markers.table_for(target.code)?;
    negation_differs(source_side(markers, src_lang), dst_side, src, translation)
        .then_some(HoldReason::Negation)
}

/// The hedge words of a source and of its translation into `target` when
/// exactly one is hedged. `None` when they agree and when `target` has no
/// table of its own (Chinese, Korean and Spanish have one).
pub fn hedge_into<'a>(
    markers: &'a Markers,
    src: &str,
    translation: &str,
    src_lang: Lang,
    target: &Target,
) -> Option<HedgeMismatch<'a>> {
    let dst_side = markers.table_for(target.code)?;
    hedge_mismatch(source_side(markers, src_lang), dst_side, src, translation)
}

/// Whether the numbers of a source and of its translation are the same, each
/// text read with the number words of its own language.
fn numbers_agree(src: &str, src_code: &str, translation: &str, dst_code: &str) -> bool {
    numbers::compared_values(src, Tongue::of_code(src_code))
        == numbers::compared_values(translation, Tongue::of_code(dst_code))
}

/// Whether every Latin word of a Japanese source stands in its translation.
fn latin_names_kept(src: &str, translation: &str) -> bool {
    let translation_lower = translation.to_lowercase();
    ascii_words(src)
        .iter()
        .all(|word| translation_lower.contains(&word.to_lowercase()))
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
    form_checks(markers, src, translation, |text| match target {
        Lang::Ja => text.chars().any(is_japanese_char),
        Lang::En => {
            text.chars().any(|c| c.is_ascii_alphabetic()) && !text.chars().any(is_japanese_char)
        }
    })
}

/// [`form_ok`] for a target that is not one of the live pair.
fn form_into(markers: &Markers, src: &str, translation: &str, target: &Target) -> bool {
    form_checks(markers, src, translation, |text| target.script.fits(text))
}

/// The form checks every target shares; `in_target_script` is the one that
/// differs by language.
fn form_checks(
    markers: &Markers,
    src: &str,
    translation: &str,
    in_target_script: impl Fn(&str) -> bool,
) -> bool {
    let text = translation.trim();
    if text.is_empty() || contains_eos(text) || text == src.trim() {
        return false;
    }
    if !in_target_script(text) {
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

fn has_marker(set: &MarkerSet, scan: Scan, text: &str) -> bool {
    find_marker(set, scan, text).is_some()
}

/// The first entry of `set` found in `text`, as the table spells it.
fn find_marker<'a>(set: &'a MarkerSet, scan: Scan, text: &str) -> Option<&'a str> {
    match scan {
        Scan::Substring => {
            let cleaned = remove_substrings(text, &set.exclude);
            set.markers
                .iter()
                .map(String::as_str)
                .find(|marker| cleaned.contains(marker))
        }
        Scan::Words => {
            let lower = text.to_lowercase().replace('’', "'");
            let cleaned = remove_phrases(&lower, &set.exclude);
            let suffix = set
                .suffixes
                .iter()
                .map(String::as_str)
                .find(|suffix| cleaned.contains(suffix));
            suffix.or_else(|| {
                set.markers
                    .iter()
                    .map(String::as_str)
                    .find(|marker| find_word(&cleaned, marker, 0).is_some())
            })
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
/// at least two characters long, leaving out measures (`4892m`, `km2`), which
/// a translation writes in its own way.
fn ascii_words(text: &str) -> Vec<&str> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| word.len() >= 2 && word.bytes().any(|b| b.is_ascii_alphabetic()))
        .filter(|word| !is_measure(word))
        .collect()
}

/// A figure with its unit (`4892m`) or a length unit squared or cubed (`km2`).
fn is_measure(word: &str) -> bool {
    let unit_exponent = word
        .strip_suffix(['2', '3'])
        .is_some_and(|unit| matches!(unit, "m" | "cm" | "km" | "mm" | "dm"));
    word.starts_with(|c: char| c.is_ascii_digit()) || unit_exponent
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
            terms_ok && latin_names_kept(src, translation)
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

    /// Verifies one line `id` translated into `target`, first picking up any
    /// table file that changed. A line that passes with a hedge the other side
    /// lacks (into a language that has a table of its own) is recorded in
    /// [`HEDGE_EVENT`].
    pub fn verify_into(
        &mut self,
        id: &str,
        src: &str,
        translation: &str,
        src_lang: Lang,
        target: &Target,
        sink: &mut dyn LogSink,
    ) -> Verdict {
        self.markers.refresh(sink);
        self.glossary.refresh(sink);
        let markers = self.markers.current();
        let verdict = verify_into(
            markers,
            self.glossary.current(),
            src,
            translation,
            src_lang,
            target,
        );
        if verdict == Verdict::Ok
            && let Some(hedge) = hedge_into(markers, src, translation, src_lang, target)
        {
            sink.emit(hedge_event(id, target.code, &hedge));
        }
        verdict
    }
}

/// [`HEDGE_EVENT`] for line `id` translated into `lang`. It carries table
/// entries only, never the text of the line.
fn hedge_event(id: &str, lang: &str, hedge: &HedgeMismatch<'_>) -> LogEvent {
    LogEvent::new(Level::Info, HEDGE_EVENT, "service")
        .with("id", id)
        .with("lang", lang)
        .with("src_hedge", hedge.src)
        .with("dst_hedge", hedge.dst)
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

    fn check_into(
        src: &str,
        translation: &str,
        code: &str,
    ) -> Result<Verdict, Box<dyn std::error::Error>> {
        let (markers, glossary) = tables()?;
        let Some(target) = Target::from_code(code) else {
            return Err(format!("no target {code}").into());
        };
        Ok(verify_into(
            &markers,
            &glossary,
            src,
            translation,
            Lang::Ja,
            &target,
        ))
    }

    #[test]
    fn a_translation_into_the_live_language_is_verified_as_before()
    -> Result<(), Box<dyn std::error::Error>> {
        for (src, translation) in [
            ("こんにちは。", "Hello."),
            ("Zoomで共有します。", "I will share it."),
            (
                "予算は五百万円を想定しています。",
                "The budget is 1,500,000 yen.",
            ),
            ("こんにちは。", "Hello.</s>"),
        ] {
            assert_eq!(
                check_into(src, translation, "en")?,
                check(src, translation, Lang::Ja)?,
                "{src}"
            );
        }
        Ok(())
    }

    #[test]
    fn a_matching_translation_into_another_language_passes()
    -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(
            check_into("猫が寝ています。", "El gato duerme.", "es")?,
            Verdict::Ok
        );
        assert_eq!(
            check_into("猫が寝ています。", "猫在睡觉。", "zh")?,
            Verdict::Ok
        );
        assert_eq!(
            check_into("猫が寝ています。", "고양이가 잔다.", "ko")?,
            Verdict::Ok
        );
        Ok(())
    }

    #[test]
    fn a_changed_amount_is_held_for_number_in_another_language()
    -> Result<(), Box<dyn std::error::Error>> {
        let said = check_into(
            "予算は五百万円です。",
            "El presupuesto es de 7 millones de yenes.",
            "es",
        )?;
        assert_eq!(said, Verdict::Held(HoldReason::Number));
        Ok(())
    }

    #[test]
    fn the_wrong_script_is_held_for_form_in_another_language()
    -> Result<(), Box<dyn std::error::Error>> {
        let kana_in_chinese = check_into("こんにちは。", "こんにちは、猫です。", "zh")?;
        assert_eq!(kana_in_chinese, Verdict::Held(HoldReason::Form));
        let han_in_spanish = check_into("こんにちは。", "Hola 猫", "es")?;
        assert_eq!(han_in_spanish, Verdict::Held(HoldReason::Form));
        let marker = check_into("こんにちは。", "Hola.</s>", "es")?;
        assert_eq!(marker, Verdict::Held(HoldReason::Form));
        Ok(())
    }

    #[test]
    fn a_dropped_latin_name_is_held_in_another_language() -> Result<(), Box<dyn std::error::Error>>
    {
        let said = check_into("Zoomで共有します。", "Lo compartiré.", "es")?;
        assert_eq!(said, Verdict::Held(HoldReason::ProperNoun));
        let kept = check_into("Zoomで共有します。", "Lo compartiré por Zoom.", "es")?;
        assert_eq!(kept, Verdict::Ok);
        Ok(())
    }

    /// There is no marker table for French, so a lost negation passes: the
    /// limit of the check, pinned so that it is not mistaken for a guarantee.
    #[test]
    fn negation_is_not_checked_into_a_language_without_a_marker_table()
    -> Result<(), Box<dyn std::error::Error>> {
        let said = check_into("そこにいませんでした。", "J'étais là.", "fr")?;
        assert_eq!(said, Verdict::Ok);
        Ok(())
    }

    /// The pairs of H36: a Japanese sentence, a translation that keeps its
    /// negation or hedge, and one that drops it.
    const NEGATION_CASES: [(&str, &str, &str, &str); 3] = [
        (
            "zh",
            "そこにいませんでした。",
            "我没有在那里。",
            "我在那里。",
        ),
        (
            "ko",
            "そこにいませんでした。",
            "저는 거기에 없었어요.",
            "저는 거기에 있었어요.",
        ),
        (
            "es",
            "そこにいませんでした。",
            "No estaba allí.",
            "Estaba allí.",
        ),
    ];
    const UNCERTAINTY_CASES: [(&str, &str, &str, &str); 3] = [
        (
            "zh",
            "明日は雨かもしれません。",
            "明天可能会下雨。",
            "明天会下雨。",
        ),
        (
            "ko",
            "明日は雨かもしれません。",
            "내일은 비가 올 수도 있어요.",
            "내일은 비가 와요.",
        ),
        (
            "es",
            "明日は雨かもしれません。",
            "Quizás llueva mañana.",
            "Lloverá mañana.",
        ),
    ];

    #[test]
    fn a_translation_that_dropped_the_negation_is_held_in_zh_ko_and_es()
    -> Result<(), Box<dyn std::error::Error>> {
        for (code, src, kept, dropped) in NEGATION_CASES {
            assert_eq!(check_into(src, kept, code)?, Verdict::Ok, "{code} kept");
            assert_eq!(
                check_into(src, dropped, code)?,
                Verdict::Held(HoldReason::Negation),
                "{code} dropped"
            );
        }
        Ok(())
    }

    /// The hedge words of both sides when exactly one is hedged.
    type Seen = Option<(Option<String>, Option<String>)>;

    /// The hedge words of `translation` against `src` (Japanese), into `code`.
    fn hedge_of(
        src: &str,
        translation: &str,
        code: &str,
    ) -> Result<Seen, Box<dyn std::error::Error>> {
        let markers = Markers::embedded()?;
        let Some(target) = Target::from_code(code) else {
            return Err(format!("no target {code}").into());
        };
        let seen = hedge_into(&markers, src, translation, Lang::Ja, &target);
        Ok(seen.map(|hedge| (hedge.src.map(str::to_owned), hedge.dst.map(str::to_owned))))
    }

    #[test]
    fn a_translation_that_dropped_the_hedge_passes_in_zh_ko_and_es_and_is_read_as_a_mismatch()
    -> Result<(), Box<dyn std::error::Error>> {
        for (code, src, kept, dropped) in UNCERTAINTY_CASES {
            assert_eq!(check_into(src, kept, code)?, Verdict::Ok, "{code} kept");
            assert_eq!(
                check_into(src, dropped, code)?,
                Verdict::Ok,
                "{code} dropped"
            );
            assert_eq!(hedge_of(src, kept, code)?, None, "{code} kept");
            let seen = hedge_of(src, dropped, code)?;
            assert!(
                matches!(&seen, Some((Some(_), None))),
                "{code} dropped: {seen:?}"
            );
        }
        Ok(())
    }

    #[test]
    fn a_hedge_the_source_lacks_is_read_as_a_mismatch_in_zh_ko_and_es()
    -> Result<(), Box<dyn std::error::Error>> {
        for (code, _, hedged, plain) in UNCERTAINTY_CASES {
            let plain_src = "明日は雨です。";
            let seen = hedge_of(plain_src, hedged, code)?;
            assert!(
                matches!(&seen, Some((None, Some(_)))),
                "{code} added: {seen:?}"
            );
            assert_eq!(check_into(plain_src, plain, code)?, Verdict::Ok, "{code}");
        }
        Ok(())
    }

    #[test]
    fn a_language_without_a_table_has_no_hedge_to_read() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(
            hedge_of("明日は雨かもしれません。", "Il pleuvra demain.", "fr")?,
            None
        );
        assert_eq!(
            hedge_of("明日は雨かもしれません。", "It will rain tomorrow.", "en")?,
            None
        );
        Ok(())
    }

    #[test]
    fn the_hedge_event_names_the_table_entries_and_no_text() {
        let hedge = HedgeMismatch {
            src: Some("かもしれ"),
            dst: None,
        };
        let event = hedge_event("mic-7", "zh", &hedge);
        assert_eq!(event.event, HEDGE_EVENT);
        assert_eq!(event.level, Level::Info);
        assert_eq!(event.fields["id"], "mic-7");
        assert_eq!(event.fields["lang"], "zh");
        assert_eq!(event.fields["src_hedge"], "かもしれ");
        assert!(event.fields["dst_hedge"].is_null());
    }

    #[test]
    fn a_negation_the_source_lacks_is_held_in_zh_ko_and_es()
    -> Result<(), Box<dyn std::error::Error>> {
        for (code, _, negated, plain) in NEGATION_CASES {
            let said = check_into("そこにいました。", negated, code)?;
            assert_eq!(said, Verdict::Held(HoldReason::Negation), "{code}");
            assert_eq!(check_into("そこにいました。", plain, code)?, Verdict::Ok);
        }
        Ok(())
    }

    #[test]
    fn words_that_only_look_like_negation_are_not_counted_in_zh_and_es()
    -> Result<(), Box<dyn std::error::Error>> {
        // "非常" (very), "不过" (however) and "sin embargo" are not negations.
        let zh = check_into(
            "とても大きいです。しかし高いです。",
            "非常大。不过很贵。",
            "zh",
        )?;
        assert_eq!(zh, Verdict::Ok);
        let es = check_into("しかし大きいです。", "Sin embargo, es grande.", "es")?;
        assert_eq!(es, Verdict::Ok);
        Ok(())
    }

    #[test]
    fn spanish_markers_are_whole_words() -> Result<(), Box<dyn std::error::Error>> {
        // "nota" holds "no" and "creó" is not "creo": neither is a marker.
        let said = check_into("メモを書きました。", "Escribí una nota.", "es")?;
        assert_eq!(said, Verdict::Ok);
        let said = check_into("彼が作りました。", "Él lo creó.", "es")?;
        assert_eq!(said, Verdict::Ok);
        Ok(())
    }

    #[test]
    fn the_embedded_table_has_zh_ko_and_es() -> Result<(), String> {
        let markers = Markers::embedded()?;
        for code in ["zh", "ko", "es"] {
            assert!(markers.table_for(code).is_some(), "{code}");
        }
        assert!(markers.table_for("fr").is_none());
        Ok(())
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

    /// A correct translation passes and the same translation with the quantity
    /// changed is held: the first half proves the fix, the second the control.
    fn passes_and_control(
        src: &str,
        good: &str,
        wrong: &str,
        held: HoldReason,
    ) -> Result<(), String> {
        let (markers, glossary) = tables()?;
        let verdict =
            |translation: &str| verify_with(&markers, &glossary, src, translation, Lang::Ja);
        assert_eq!(verdict(good), Verdict::Ok, "{good}");
        assert_eq!(verdict(wrong), Verdict::Held(held), "{wrong}");
        Ok(())
    }

    #[test]
    fn a_squared_unit_and_a_measure_are_not_quantities_or_names() -> Result<(), String> {
        passes_and_control(
            "公園の面積は19,500 km2です。",
            "The park covers 19,500 km² of land.",
            "The park covers 7,500 km² of land.",
            HoldReason::Number,
        )?;
        passes_and_control(
            "標高4892mの山です。",
            "It is a 4,892 m mountain.",
            "It is a 4,000 m mountain.",
            HoldReason::Number,
        )
    }

    #[test]
    fn a_japanese_month_matches_the_month_name() -> Result<(), String> {
        passes_and_control(
            "12月に開催されます。",
            "It takes place in December.",
            "It takes place in June.",
            HoldReason::Number,
        )
    }

    #[test]
    fn hearsay_is_not_a_hedge_but_appearance_is() -> Result<(), String> {
        let (markers, glossary) = tables()?;
        let said = verify_with(
            &markers,
            &glossary,
            "彼はドアベルを作ったそうです。",
            "He said he built a doorbell.",
            Lang::Ja,
        );
        assert_eq!(said, Verdict::Ok);
        let looks = verify_with(
            &markers,
            &glossary,
            "雨が降りそうです。",
            "It is raining.",
            Lang::Ja,
        );
        assert_eq!(looks, Verdict::Held(HoldReason::Uncertainty));
        Ok(())
    }

    #[test]
    fn an_implied_prohibition_needs_a_negation_in_the_translation() -> Result<(), String> {
        passes_and_control(
            "冗談を言うなどもってのほかです。",
            "Do not make jokes.",
            "Make jokes.",
            HoldReason::Negation,
        )?;
        passes_and_control(
            "決勝に上がれませんでした。",
            "The team failed to reach the final.",
            "The team reached the final.",
            HoldReason::Negation,
        )
    }

    #[test]
    fn about_before_a_noun_is_not_a_hedge_but_before_a_figure_is() -> Result<(), String> {
        passes_and_control(
            "生態系に関する情報です。",
            "It is information about the ecosystem.",
            "It is information, about 3.",
            HoldReason::Number,
        )?;
        let (markers, glossary) = tables()?;
        let held = verify_with(
            &markers,
            &glossary,
            "1日前のことです。",
            "It was about a day ago.",
            Lang::Ja,
        );
        assert_eq!(held, Verdict::Held(HoldReason::Uncertainty));
        Ok(())
    }

    #[test]
    fn not_only_is_not_a_negation() -> Result<(), String> {
        let (markers, glossary) = tables()?;
        let said = verify_with(
            &markers,
            &glossary,
            "知識だけでなく文化も大切です。",
            "Not only knowledge but culture matters.",
            Lang::Ja,
        );
        assert_eq!(said, Verdict::Ok);
        Ok(())
    }

    #[test]
    fn chinese_liang_and_spanish_number_words_are_the_numbers_they_say()
    -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(
            check_into("2人いました。", "有两个人。", "zh")?,
            Verdict::Ok
        );
        assert_eq!(
            check_into("2人いました。", "有三个人。", "zh")?,
            Verdict::Held(HoldReason::Number)
        );
        assert_eq!(
            check_into("2時間かかりました。", "Tardó dos horas.", "es")?,
            Verdict::Ok
        );
        assert_eq!(
            check_into("2時間かかりました。", "Tardó tres horas.", "es")?,
            Verdict::Held(HoldReason::Number)
        );
        assert_eq!(
            check_into("80%でした。", "Fue el ochenta por ciento.", "es")?,
            Verdict::Ok
        );
        assert_eq!(
            check_into("意味がありました。", "Eso tenía sentido.", "es")?,
            Verdict::Ok
        );
        Ok(())
    }

    #[test]
    fn chinese_words_that_only_contain_a_negation_character_are_not_negations()
    -> Result<(), Box<dyn std::error::Error>> {
        let said = check_into("それぞれ4位と5位です。", "分别是第四和第五。", "zh")?;
        assert_eq!(said, Verdict::Ok);
        Ok(())
    }
}

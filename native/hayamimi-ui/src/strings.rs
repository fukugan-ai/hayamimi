//! The words the pages show, from the `strings.<locale>.toml` tables.
//!
//! The tables are compiled in. A page picks one when it starts, from the
//! locale of the browser, and English when the locale is not one of the two.
//! The values of `final`, `lang` and `speaker` come from the wire and are never
//! looked up here.

use std::collections::BTreeMap;
use std::fmt;

pub const JA: &str = include_str!("../data/strings.ja.toml");
pub const EN: &str = include_str!("../data/strings.en.toml");

/// The keys of the tables, so that a misspelt key is a compile error.
pub mod keys {
    pub const LISTENING: &str = "ui.dashboard.listening";
    pub const WAITING: &str = "ui.dashboard.waiting";
    pub const FEED: &str = "ui.dashboard.feed";
    pub const REFINED: &str = "ui.dashboard.refined";
    pub const CHIP_FINALS: &str = "ui.dashboard.chip_finals";
    pub const CHIP_LATENCY: &str = "ui.dashboard.chip_latency";
    pub const CHIP_LANGS: &str = "ui.dashboard.chip_langs";
    pub const EMPTY_FEED: &str = "ui.dashboard.empty_feed";
    pub const EMPTY_REFINED: &str = "ui.common.empty_refined";
    pub const JUMP_LATEST: &str = "ui.common.jump_latest";
    pub const CONNECTING: &str = "ui.common.connecting";
    pub const RECONNECTING: &str = "ui.common.reconnecting";
    pub const CONNECTED: &str = "ui.common.connected";
    pub const TR_FIRST: &str = "ui.tr.first";
    pub const TR_HELD: &str = "ui.tr.held";
    pub const TR_UNAVAILABLE: &str = "ui.tr.unavailable";
    pub const FALLBACK_BADGE: &str = "ui.fallback.badge";
    /// The prefix of the words for the reasons a translation is withheld.
    pub const TR_REASON: &str = "ui.tr.reason";
    /// The prefix of the words for the stages that can run on a fallback.
    pub const FALLBACK_STAGE: &str = "ui.fallback.stage";
    /// The prefix of the words for the reasons a stage runs on a fallback.
    pub const FALLBACK_REASON: &str = "ui.fallback.reason";
    /// The prefix of the words for where a stage runs on a fallback.
    pub const FALLBACK_TO: &str = "ui.fallback.to";
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Locale {
    Ja,
    En,
}

impl Locale {
    /// `ja`, `ja-JP` and `ja_JP.UTF-8` are Japanese; every other tag is English.
    pub fn from_tag(tag: &str) -> Locale {
        let japanese = tag
            .split(['-', '_', '.'])
            .next()
            .is_some_and(|language| language.eq_ignore_ascii_case("ja"));
        if japanese { Locale::Ja } else { Locale::En }
    }

    /// The locale of the browser, read once when the page starts.
    pub fn detect() -> Locale {
        match sys_locale::get_locale() {
            Some(tag) => Locale::from_tag(&tag),
            // No locale to read: English, the language of the default table.
            None => Locale::En,
        }
    }
}

#[derive(Debug)]
pub enum StringsError {
    Toml(toml::de::Error),
    /// A value of the table that is not text.
    NotText(String),
}

impl fmt::Display for StringsError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StringsError::Toml(error) => write!(out, "{error}"),
            StringsError::NotText(key) => write!(out, "{key} is not text"),
        }
    }
}

impl std::error::Error for StringsError {}

/// A table of words by dotted key.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Strings {
    table: BTreeMap<String, String>,
}

impl Strings {
    pub fn parse(text: &str) -> Result<Strings, StringsError> {
        let parsed: toml::Table = toml::from_str(text).map_err(StringsError::Toml)?;
        let mut table = BTreeMap::new();
        flatten("", &parsed, &mut table)?;
        Ok(Strings { table })
    }

    pub fn embedded(locale: Locale) -> Result<Strings, StringsError> {
        Strings::parse(match locale {
            Locale::Ja => JA,
            Locale::En => EN,
        })
    }

    /// The word of a key; the key itself when the table has none.
    pub fn text<'a>(&'a self, key: &'a str) -> &'a str {
        match self.table.get(key) {
            Some(word) => word,
            None => key,
        }
    }

    /// The word for a value of the wire under `prefix`, or the value itself
    /// when the table does not know it (a stage or a reason that has no word yet).
    pub fn word(&self, prefix: &str, value: &str) -> String {
        match self.table.get(&format!("{prefix}.{value}")) {
            Some(word) => word.clone(),
            None => value.to_owned(),
        }
    }

    /// Every key of the table, in order.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.table.keys().map(String::as_str)
    }
}

fn flatten(
    prefix: &str,
    table: &toml::Table,
    out: &mut BTreeMap<String, String>,
) -> Result<(), StringsError> {
    for (key, value) in table {
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        match value {
            toml::Value::Table(inner) => flatten(&path, inner, out)?,
            toml::Value::String(word) => {
                out.insert(path, word.clone());
            }
            _ => return Err(StringsError::NotText(path)),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_japanese_tag_is_japanese_and_everything_else_is_english() {
        for tag in ["ja", "ja-JP", "ja_JP.UTF-8", "JA"] {
            assert_eq!(Locale::from_tag(tag), Locale::Ja, "{tag}");
        }
        for tag in ["en-US", "ko", "jam", "", "zh-CN"] {
            assert_eq!(Locale::from_tag(tag), Locale::En, "{tag}");
        }
    }

    #[test]
    fn the_embedded_tables_parse_and_have_the_same_keys() -> Result<(), StringsError> {
        let ja = Strings::embedded(Locale::Ja)?;
        let en = Strings::embedded(Locale::En)?;
        assert!(ja.keys().count() >= 23);
        assert_eq!(ja.keys().collect::<Vec<_>>(), en.keys().collect::<Vec<_>>());
        Ok(())
    }

    #[test]
    fn every_key_in_the_code_is_in_both_tables() -> Result<(), StringsError> {
        let named = [
            keys::LISTENING,
            keys::WAITING,
            keys::FEED,
            keys::REFINED,
            keys::CHIP_FINALS,
            keys::CHIP_LATENCY,
            keys::CHIP_LANGS,
            keys::EMPTY_FEED,
            keys::EMPTY_REFINED,
            keys::JUMP_LATEST,
            keys::CONNECTING,
            keys::RECONNECTING,
            keys::CONNECTED,
            keys::TR_FIRST,
            keys::TR_HELD,
            keys::TR_UNAVAILABLE,
            keys::FALLBACK_BADGE,
        ];
        for locale in [Locale::Ja, Locale::En] {
            let strings = Strings::embedded(locale)?;
            for key in named {
                assert!(
                    strings.keys().any(|have| have == key),
                    "{locale:?} lacks {key}"
                );
            }
        }
        Ok(())
    }

    #[test]
    fn a_value_with_no_word_shows_as_itself() -> Result<(), StringsError> {
        let strings = Strings::embedded(Locale::Ja)?;
        assert_eq!(strings.word(keys::TR_REASON, "number"), "数の不一致");
        assert_eq!(strings.word(keys::TR_REASON, "never_seen"), "never_seen");
        assert_eq!(strings.text("ui.nothing.here"), "ui.nothing.here");
        Ok(())
    }

    #[test]
    fn a_value_that_is_not_text_is_refused() {
        let found = Strings::parse("[ui]\nn = 3\n");
        assert!(matches!(found, Err(StringsError::NotText(key)) if key == "ui.n"));
        assert!(matches!(Strings::parse("= ="), Err(StringsError::Toml(_))));
    }
}

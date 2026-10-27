//! Locale string tables (`strings.<locale>.toml`): marks, annotation
//! templates, unit labels, log messages and command-line text.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::calendar::Date;
use crate::lang::Lang;

pub const DEFAULT_JA: &str = include_str!("../data/strings.ja.toml");
pub const DEFAULT_EN: &str = include_str!("../data/strings.en.toml");

/// How one unit is written next to its value.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct UnitLabel {
    pub label: String,
    /// A space between the value and the label (`8.05 km`, but `37.0℃`).
    #[serde(default = "yes")]
    pub space: bool,
    /// The label comes before the value (`$31,500`).
    #[serde(default)]
    pub prefix: bool,
}

fn yes() -> bool {
    true
}

/// Templates for the converted-value note added to a subtitle.
#[derive(Clone, Debug, Deserialize)]
pub struct AnnotationStrings {
    /// Template without a date; `{values}` is replaced.
    pub plain: String,
    /// Template for currency; `{values}` and `{date}` are replaced.
    pub currency: String,
    /// Separator between several converted values.
    pub join: String,
    /// Date template over `{mm}`, `{dd}`, `{m}`, `{d}` and `{mon}`.
    pub date: String,
    /// Twelve month names for `{mon}`.
    pub months: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Strings {
    pub source_mark: String,
    pub no_translation_mark: String,
    pub withheld_mark: String,
    pub annotation: AnnotationStrings,
    pub unit: BTreeMap<String, UnitLabel>,
    pub messages: BTreeMap<String, String>,
    pub cli: BTreeMap<String, String>,
}

impl Strings {
    /// Parses a table file; the error is a short description for the caller.
    pub fn parse(bytes: &[u8]) -> Result<Strings, String> {
        let text = std::str::from_utf8(bytes).map_err(|error| error.to_string())?;
        let strings: Strings = toml::from_str(text).map_err(|error| error.to_string())?;
        if strings.annotation.months.len() != 12 {
            return Err(String::from("annotation.months needs twelve names"));
        }
        Ok(strings)
    }

    /// The compiled-in table for a language.
    pub fn embedded(lang: Lang) -> Result<Strings, String> {
        let text = match lang {
            Lang::Ja => DEFAULT_JA,
            Lang::En => DEFAULT_EN,
        };
        Strings::parse(text.as_bytes())
    }

    /// The log message for an event name; the event name itself when the
    /// table has no entry.
    pub fn message_for<'a>(&'a self, event: &'a str) -> &'a str {
        match self.messages.get(event) {
            Some(message) => message,
            None => event,
        }
    }

    /// A command-line string by key; the key itself when missing.
    pub fn cli_text<'a>(&'a self, key: &'a str) -> &'a str {
        match self.cli.get(key) {
            Some(text) => text,
            None => key,
        }
    }

    /// Formats a date with this table's template.
    pub fn format_date(&self, date: Date) -> String {
        let (month, day) = (date.month, date.day);
        let name = self
            .annotation
            .months
            .get(usize::from(month).saturating_sub(1))
            .map_or("", String::as_str);
        self.annotation
            .date
            .replace("{mm}", &format!("{month:02}"))
            .replace("{dd}", &format!("{day:02}"))
            .replace("{mon}", name)
            .replace("{m}", &month.to_string())
            .replace("{d}", &day.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_embedded_tables_parse() {
        assert!(Strings::embedded(Lang::Ja).is_ok());
        assert!(Strings::embedded(Lang::En).is_ok());
    }

    #[test]
    fn parse_rejects_a_table_with_the_wrong_month_count() {
        let broken = DEFAULT_EN.replace("\"Dec\"]", "]");
        assert!(Strings::parse(broken.as_bytes()).is_err());
    }

    #[test]
    fn parse_rejects_garbage() {
        assert!(Strings::parse(b"[[[").is_err());
        assert!(Strings::parse(&[0xff, 0xfe]).is_err());
    }

    #[test]
    fn dates_follow_the_locale_template() {
        let (ja, en) = (Strings::embedded(Lang::Ja), Strings::embedded(Lang::En));
        let date = Date {
            year: 2026,
            month: 10,
            day: 6,
        };
        let said_ja = ja.ok().map(|s| s.format_date(date));
        let said_en = en.ok().map(|s| s.format_date(date));
        assert_eq!(said_ja.as_deref(), Some("10/06"));
        assert_eq!(said_en.as_deref(), Some("Oct 6"));
    }

    #[test]
    fn unit_labels_default_to_a_spaced_suffix() {
        let en = Strings::embedded(Lang::En);
        let label = en.ok().and_then(|s| s.unit.get("mile").cloned());
        assert_eq!(
            label,
            Some(UnitLabel {
                label: String::from("mile"),
                space: true,
                prefix: false
            })
        );
    }

    #[test]
    fn missing_message_and_cli_keys_echo_the_key() {
        let Ok(en) = Strings::embedded(Lang::En) else {
            panic!("embedded table missing");
        };
        assert_eq!(en.message_for("no.such.event"), "no.such.event");
        assert_eq!(en.cli_text("no_such_key"), "no_such_key");
        assert_eq!(en.message_for("fx.fetch.ok"), "Exchange rates saved");
    }

    /// T-RP-96-013
    #[test]
    fn the_retried_event_has_a_message_in_both_languages() -> Result<(), Box<dyn std::error::Error>>
    {
        for lang in [Lang::Ja, Lang::En] {
            let table = Strings::embedded(lang)?;
            let said = table.message_for("translate.request.retried");
            assert!(
                !said.is_empty() && said != "translate.request.retried",
                "{lang:?}: {said}"
            );
        }
        Ok(())
    }
}

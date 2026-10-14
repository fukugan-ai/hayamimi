//! The two languages the relay translates between.

use serde::{Deserialize, Serialize};

/// A subtitle language. The wire form is the lowercase ISO 639-1 code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    Ja,
    En,
}

impl Lang {
    /// The language on the other side of a translation.
    pub fn other(self) -> Lang {
        match self {
            Lang::Ja => Lang::En,
            Lang::En => Lang::Ja,
        }
    }

    /// The ISO 639-1 code.
    pub fn code(self) -> &'static str {
        match self {
            Lang::Ja => "ja",
            Lang::En => "en",
        }
    }

    /// The language whose ISO 639-1 code is exactly `code`; `None` for any
    /// other code, which the relay does not translate.
    pub fn from_code(code: &str) -> Option<Lang> {
        match code {
            "ja" => Some(Lang::Ja),
            "en" => Some(Lang::En),
            _ => None,
        }
    }

    /// The language a locale tag such as `ja_JP.UTF-8` or `en-US` names;
    /// anything that is not Japanese reads as English.
    pub fn from_locale_tag(tag: &str) -> Lang {
        if tag
            .get(..2)
            .is_some_and(|head| head.eq_ignore_ascii_case("ja"))
        {
            Lang::Ja
        } else {
            Lang::En
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn other_flips_the_language() {
        assert_eq!(Lang::Ja.other(), Lang::En);
        assert_eq!(Lang::En.other(), Lang::Ja);
    }

    #[test]
    fn code_is_lowercase_iso() {
        assert_eq!(Lang::Ja.code(), "ja");
        assert_eq!(Lang::En.code(), "en");
    }

    #[test]
    fn a_code_names_a_language_only_when_it_is_exact() {
        assert_eq!(Lang::from_code("ja"), Some(Lang::Ja));
        assert_eq!(Lang::from_code("en"), Some(Lang::En));
        assert_eq!(Lang::from_code("zh"), None);
        assert_eq!(Lang::from_code("JA"), None);
        assert_eq!(Lang::from_code(""), None);
    }

    #[test]
    fn locale_tags_pick_japanese_only_for_ja() {
        assert_eq!(Lang::from_locale_tag("ja_JP.UTF-8"), Lang::Ja);
        assert_eq!(Lang::from_locale_tag("JA"), Lang::Ja);
        assert_eq!(Lang::from_locale_tag("en-US"), Lang::En);
        assert_eq!(Lang::from_locale_tag(""), Lang::En);
        assert_eq!(Lang::from_locale_tag("j"), Lang::En);
    }
}

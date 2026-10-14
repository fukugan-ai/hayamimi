//! The languages a translation can be written in.
//!
//! [`Lang`] is the pair the relay shows live, Japanese and English. A
//! [`Target`] is a language the translator is asked to write for
//! `--translate`: its ISO 639-1 code, the name the prompt gives the model and
//! the script its text has to be in. The table is closed; a code that is not in
//! it is not translated, and the caller says so and goes on.

use crate::lang::Lang;

/// The script a translation into one language is written in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Script {
    Latin,
    Han,
    Hangul,
    Cyrillic,
}

/// A language the translator can be asked to write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    /// The ISO 639-1 code, lowercase.
    pub code: &'static str,
    /// The name in the prompt to the translator.
    pub name: &'static str,
    pub script: Script,
}

const fn target(code: &'static str, name: &'static str, script: Script) -> Target {
    Target { code, name, script }
}

/// Every language `--translate` accepts.
pub const TARGETS: [Target; 15] = [
    target("en", "English", Script::Latin),
    target("zh", "Chinese", Script::Han),
    target("ko", "Korean", Script::Hangul),
    target("es", "Spanish", Script::Latin),
    target("fr", "French", Script::Latin),
    target("de", "German", Script::Latin),
    target("it", "Italian", Script::Latin),
    target("pt", "Portuguese", Script::Latin),
    target("nl", "Dutch", Script::Latin),
    target("pl", "Polish", Script::Latin),
    target("tr", "Turkish", Script::Latin),
    target("id", "Indonesian", Script::Latin),
    target("vi", "Vietnamese", Script::Latin),
    target("ru", "Russian", Script::Cyrillic),
    target("uk", "Ukrainian", Script::Cyrillic),
];

impl Target {
    /// The target whose code is exactly `code`; `None` for any other code.
    pub fn from_code(code: &str) -> Option<Target> {
        TARGETS.iter().find(|one| one.code == code).copied()
    }

    /// The live language this target is, when it is one.
    pub fn lang(&self) -> Option<Lang> {
        Lang::from_code(self.code)
    }
}

fn is_kana(c: char) -> bool {
    matches!(c, '\u{3040}'..='\u{30FF}' | '\u{FF66}'..='\u{FF9F}')
}

fn is_han(c: char) -> bool {
    matches!(c, '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' | '々')
}

fn is_hangul(c: char) -> bool {
    matches!(c, '\u{1100}'..='\u{11FF}' | '\u{3130}'..='\u{318F}' | '\u{AC00}'..='\u{D7AF}')
}

fn is_cyrillic(c: char) -> bool {
    matches!(c, '\u{0400}'..='\u{04FF}')
}

impl Script {
    /// Whether `text` is written in this script: it holds a letter of the
    /// script and none of the CJK scripts that a translation into another
    /// language would carry over from a Japanese source.
    pub fn fits(self, text: &str) -> bool {
        let any = |test: fn(char) -> bool| text.chars().any(test);
        let kana = any(is_kana);
        let han = any(is_han);
        let hangul = any(is_hangul);
        match self {
            Script::Latin => any(|c| c.is_ascii_alphabetic()) && !(kana || han || hangul),
            Script::Han => han && !(kana || hangul),
            // Hanja stand in a Korean sentence now and then; kana never do.
            Script::Hangul => hangul && !kana,
            Script::Cyrillic => any(is_cyrillic) && !(kana || han || hangul),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_listed_code_names_its_target() {
        let found = Target::from_code("ko");
        assert_eq!(found.map(|one| one.name), Some("Korean"));
        assert_eq!(found.map(|one| one.script), Some(Script::Hangul));
    }

    #[test]
    fn an_unlisted_or_inexact_code_names_nothing() {
        assert_eq!(Target::from_code("xx"), None);
        assert_eq!(Target::from_code("ZH"), None);
        assert_eq!(Target::from_code(""), None);
        // Japanese is the source of a translation, never its target.
        assert_eq!(Target::from_code("ja"), None);
    }

    #[test]
    fn the_codes_are_unique_and_lowercase() {
        for (at, one) in TARGETS.iter().enumerate() {
            assert_eq!(one.code, one.code.to_ascii_lowercase());
            assert!(TARGETS[..at].iter().all(|before| before.code != one.code));
        }
    }

    #[test]
    fn english_is_the_live_language_and_the_others_are_not() {
        assert_eq!(Target::from_code("en").and_then(|one| one.lang()), Some(Lang::En));
        assert_eq!(Target::from_code("zh").and_then(|one| one.lang()), None);
    }

    #[test]
    fn latin_text_fits_latin_only_without_cjk() {
        assert!(Script::Latin.fits("El gato duerme."));
        assert!(!Script::Latin.fits("猫が寝ています。"));
        assert!(!Script::Latin.fits("The cat 寝ます."));
        assert!(!Script::Latin.fits("   "));
    }

    #[test]
    fn han_text_fits_chinese_but_kana_does_not() {
        assert!(Script::Han.fits("猫在睡觉。"));
        assert!(!Script::Han.fits("猫が寝ています。"));
        assert!(!Script::Han.fits("고양이가 잔다."));
        assert!(!Script::Han.fits("The cat sleeps."));
    }

    #[test]
    fn hangul_text_fits_korean_with_hanja_but_not_kana() {
        assert!(Script::Hangul.fits("고양이가 잔다."));
        assert!(Script::Hangul.fits("大韓民国은 나라다."));
        assert!(!Script::Hangul.fits("猫が寝ています。"));
        assert!(!Script::Hangul.fits("猫在睡觉。"));
    }

    #[test]
    fn cyrillic_text_fits_russian() {
        assert!(Script::Cyrillic.fits("Кошка спит."));
        assert!(!Script::Cyrillic.fits("The cat sleeps."));
        assert!(!Script::Cyrillic.fits("Кошка 猫"));
    }
}

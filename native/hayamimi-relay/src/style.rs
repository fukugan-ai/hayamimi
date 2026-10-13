//! Style tables the edit step (E) shows to the model: English calques to
//! avoid (`calques.toml`) and the Japanese register (`ja_style.toml`).

use serde::Deserialize;

pub const DEFAULT_CALQUES: &str = include_str!("../data/calques.toml");
pub const DEFAULT_JA_STYLE: &str = include_str!("../data/ja_style.toml");

/// A literal pattern and the wording to prefer.
#[derive(Clone, Debug, Deserialize)]
pub struct Pattern {
    pub literal: String,
    pub natural: String,
}

/// `calques.toml`.
#[derive(Clone, Debug, Deserialize)]
pub struct Calques {
    #[serde(default, rename = "pattern")]
    pub patterns: Vec<Pattern>,
}

impl Calques {
    pub fn parse(bytes: &[u8]) -> Result<Calques, String> {
        let text = std::str::from_utf8(bytes).map_err(|error| error.to_string())?;
        toml::from_str(text).map_err(|error| error.to_string())
    }

    pub fn embedded() -> Result<Calques, String> {
        Calques::parse(DEFAULT_CALQUES.as_bytes())
    }

    /// How many patterns appear in `text` (ASCII case ignored).
    pub fn count_in(&self, text: &str) -> usize {
        let lower = text.to_lowercase();
        self.patterns
            .iter()
            .filter(|pattern| lower.contains(&pattern.literal.to_lowercase()))
            .count()
    }
}

/// `ja_style.toml`.
#[derive(Clone, Debug, Deserialize)]
pub struct JaStyle {
    pub note: String,
    pub polite_endings: Vec<String>,
    pub plain_endings: Vec<String>,
    #[serde(default, rename = "pattern")]
    pub patterns: Vec<Pattern>,
}

impl JaStyle {
    pub fn parse(bytes: &[u8]) -> Result<JaStyle, String> {
        let text = std::str::from_utf8(bytes).map_err(|error| error.to_string())?;
        toml::from_str(text).map_err(|error| error.to_string())
    }

    pub fn embedded() -> Result<JaStyle, String> {
        JaStyle::parse(DEFAULT_JA_STYLE.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_embedded_tables_parse() {
        assert!(Calques::embedded().is_ok());
        assert!(JaStyle::embedded().is_ok());
    }

    #[test]
    fn calques_are_counted_ignoring_case() -> Result<(), String> {
        let calques = Calques::embedded()?;
        assert_eq!(calques.count_in("Please Take Care Of Me."), 1);
        assert_eq!(calques.count_in("Nice to meet you."), 0);
        Ok(())
    }

    #[test]
    fn garbage_is_refused() {
        assert!(Calques::parse(b"[[[").is_err());
        assert!(JaStyle::parse(b"x = 1").is_err());
    }
}

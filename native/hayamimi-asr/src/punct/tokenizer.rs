//! The character-level tokenizer of the punctuation model.
//!
//! The model's own tokenizer normalises with NFKC, splits into morphemes and
//! then splits every morpheme into single characters. The morpheme split has
//! one observable effect, which is that white space is dropped, so the
//! tokenizer here is: NFKC, drop white space, one token per character. The
//! vocabulary is a plain list whose line number is the token id.

use std::collections::HashMap;

use unicode_normalization::UnicodeNormalization;

use crate::error::AsrError;

use super::rules::MAX_CHARS;

const PAD: &str = "[PAD]";
const UNK: &str = "[UNK]";
const CLS: &str = "[CLS]";
const SEP: &str = "[SEP]";

/// The characters a text is cut into, and the ids the model reads for them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Encoded {
    /// The normalised characters, at most `MAX_CHARS` of them.
    pub chars: Vec<char>,
    /// `[CLS]`, one id per character, `[SEP]`.
    pub ids: Vec<i64>,
}

#[derive(Debug)]
pub struct Tokenizer {
    /// Single-character entries; a later line of the same character wins.
    chars: HashMap<char, i64>,
    unk: i64,
    cls: i64,
    sep: i64,
}

/// NFKC-normalised characters without white space.
pub fn normalized_chars(text: &str) -> Vec<char> {
    text.nfkc().filter(|c| !c.is_whitespace()).collect()
}

fn special(vocab: &HashMap<&str, i64>, name: &str) -> Result<i64, AsrError> {
    vocab
        .get(name)
        .copied()
        .ok_or_else(|| AsrError::Vocab(format!("no {name} line")))
}

impl Tokenizer {
    /// Reads the vocabulary text. The four special tokens must be present.
    pub fn from_vocab(text: &str) -> Result<Tokenizer, AsrError> {
        let mut entries: HashMap<&str, i64> = HashMap::new();
        let mut chars: HashMap<char, i64> = HashMap::new();
        for (line, token) in text.lines().enumerate() {
            let id = i64::try_from(line).map_err(|_| AsrError::Overflow("a vocab line number"))?;
            entries.insert(token, id);
            let mut letters = token.chars();
            if let (Some(only), None) = (letters.next(), letters.next()) {
                chars.insert(only, id);
            }
        }
        // The model input never holds [PAD], but a vocabulary without it is not this model's.
        special(&entries, PAD)?;
        Ok(Tokenizer {
            chars,
            unk: special(&entries, UNK)?,
            cls: special(&entries, CLS)?,
            sep: special(&entries, SEP)?,
        })
    }

    /// The id of one character.
    fn id_of(&self, c: char) -> i64 {
        // DEFAULT: a character the vocabulary lacks reads as [UNK]; the model's tokenizer specifies this.
        self.chars.get(&c).copied().unwrap_or(self.unk)
    }

    /// `[CLS]`, the id of each character (`[UNK]` for one the vocabulary lacks), `[SEP]`.
    pub fn ids_for(&self, chars: &[char]) -> Vec<i64> {
        let mut ids = Vec::with_capacity(chars.len() + 2);
        ids.push(self.cls);
        ids.extend(chars.iter().map(|c| self.id_of(*c)));
        ids.push(self.sep);
        ids
    }

    /// Normalises `text`, cuts it to `MAX_CHARS` characters and reads the ids.
    pub fn encode(&self, text: &str) -> Encoded {
        let mut chars = normalized_chars(text);
        chars.truncate(MAX_CHARS);
        let ids = self.ids_for(&chars);
        Encoded { chars, ids }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VOCAB: &str = "[PAD]\n[UNK]\n[CLS]\n[SEP]\nあ\nい\n1\n";

    fn tokenizer() -> Result<Tokenizer, AsrError> {
        Tokenizer::from_vocab(VOCAB)
    }

    #[test]
    fn the_line_number_is_the_id_and_the_text_is_wrapped() -> Result<(), AsrError> {
        assert_eq!(tokenizer()?.encode("あい").ids, [2, 4, 5, 3]);
        Ok(())
    }

    #[test]
    fn a_character_the_vocabulary_lacks_is_unknown() -> Result<(), AsrError> {
        assert_eq!(tokenizer()?.encode("あ猫").ids, [2, 4, 1, 3]);
        Ok(())
    }

    #[test]
    fn white_space_is_dropped_and_full_width_digits_are_folded() -> Result<(), AsrError> {
        let encoded = tokenizer()?.encode(" あ \u{3000}１\n");
        assert_eq!(encoded.chars, ['あ', '1']);
        assert_eq!(encoded.ids, [2, 4, 6, 3]);
        Ok(())
    }

    #[test]
    fn empty_text_is_only_the_two_specials() -> Result<(), AsrError> {
        let encoded = tokenizer()?.encode("");
        assert!(encoded.chars.is_empty());
        assert_eq!(encoded.ids, [2, 3]);
        Ok(())
    }

    #[test]
    fn text_is_cut_at_the_character_limit() -> Result<(), AsrError> {
        let encoded = tokenizer()?.encode(&"あ".repeat(MAX_CHARS + 1));
        assert_eq!(encoded.chars.len(), MAX_CHARS);
        assert_eq!(encoded.ids.len(), MAX_CHARS + 2);
        Ok(())
    }

    #[test]
    fn exactly_the_limit_is_not_cut() -> Result<(), AsrError> {
        let encoded = tokenizer()?.encode(&"あ".repeat(MAX_CHARS));
        assert_eq!(encoded.chars.len(), MAX_CHARS);
        Ok(())
    }

    #[test]
    fn a_vocabulary_without_a_special_token_is_refused() {
        let said = Tokenizer::from_vocab("[PAD]\n[UNK]\n[CLS]\nあ\n")
            .err()
            .map(|error| error.to_string());
        assert!(
            said.as_deref().is_some_and(|text| text.contains("[SEP]")),
            "{said:?}"
        );
    }

    #[test]
    fn a_later_line_of_the_same_character_wins() -> Result<(), AsrError> {
        let vocab = format!("{VOCAB}あ\n");
        assert_eq!(Tokenizer::from_vocab(&vocab)?.encode("あ").ids, [2, 7, 3]);
        Ok(())
    }

    #[test]
    fn skipping_normalisation_keeps_full_width_characters() -> Result<(), AsrError> {
        let plain: Vec<char> = "１".chars().collect();
        assert_eq!(tokenizer()?.ids_for(&plain), [2, 1, 3]);
        Ok(())
    }
}

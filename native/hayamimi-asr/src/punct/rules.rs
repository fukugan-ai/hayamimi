//! The string rules of punctuation restoration: where a mark goes, given the
//! model's scores, and which sentence ends in a question mark.
//!
//! This module reads numbers and writes a string. It has no model and no
//! device, so it can be measured on its own and judged against the design's
//! table of placements.

use crate::error::AsrError;

/// A comma is inserted where its score reaches this value.
pub const COMMA_THRESHOLD: f32 = 0.5;
/// A period is inserted where its score reaches this value; it beats a comma.
pub const PERIOD_THRESHOLD: f32 = 0.5;
/// The model reads at most this many characters (its positions are 512, two of
/// them taken by the begin and end markers). Characters beyond are dropped.
pub const MAX_CHARS: usize = 500;

/// Marks that are never followed or preceded by an inserted mark. Half-width
/// and full-width forms are both listed because NFKC folds the full-width
/// ones into the half-width ones before this set is consulted.
pub const MARKS: [char; 21] = [
    '。', '、', '！', '？', '!', '?', '…', '「', '」', '『', '』', '（', '）', '(', ')', '【',
    '】', '・', ',', '.', '\n',
];

/// Sentence endings that make a sentence a question. The model scores commas
/// and periods only, so the question mark is a hand-written rule.
pub const QUESTION_SUFFIXES: [&str; 10] = [
    "ですか",
    "ますか",
    "でしょうか",
    "かな",
    "かしら",
    "かい",
    "の",
    "だろうか",
    "でしたか",
    "ましたか",
];

pub fn is_mark(c: char) -> bool {
    MARKS.contains(&c)
}

fn sigmoid(logit: f32) -> f32 {
    1.0 / (1.0 + (-logit).exp())
}

/// The scores of the position after `chars[index]`: row `index + 1` of the
/// output, because row 0 belongs to the begin marker. Returns (comma, period).
fn scores(logits: &[f32], index: usize) -> Option<(f32, f32)> {
    let row = logits.get((index + 1) * 2..(index + 1) * 2 + 2)?;
    Some((sigmoid(row[0]), sigmoid(row[1])))
}

/// The restored text for `chars` from the model's logits: a flat
/// `[chars.len() + 2, 2]` array of (comma, period) logits.
///
/// A mark is inserted after a character unless that character or the next one
/// is already a mark. The last character always ends the text with a period if
/// no mark closes it, and the question rule then rewrites sentence ends.
pub fn punctuate(chars: &[char], logits: &[f32]) -> Result<String, AsrError> {
    let expected = (chars.len() + 2) * 2;
    if logits.len() != expected {
        return Err(AsrError::Shape(format!(
            "{} logits for {} characters, want {expected}",
            logits.len(),
            chars.len()
        )));
    }
    let mut out = String::with_capacity(chars.len() * 4);
    for (index, c) in chars.iter().enumerate() {
        out.push(*c);
        let Some((comma, period)) = scores(logits, index) else {
            return Err(AsrError::Shape(format!("no scores for character {index}")));
        };
        let next_is_mark = chars.get(index + 1).is_some_and(|next| is_mark(*next));
        if is_mark(*c) || next_is_mark {
            continue;
        }
        if period >= PERIOD_THRESHOLD {
            out.push('。');
        } else if comma >= COMMA_THRESHOLD {
            out.push('、');
        }
    }
    if out.chars().next_back().is_some_and(|last| !is_mark(last)) {
        out.push('。');
    }
    Ok(apply_question_marks(&out))
}

/// Rewrites each sentence-final period into a question mark when the sentence
/// ends in a question suffix. Text that ends in a mark other than a period
/// still gets a period appended, so a full-width question mark at the end of
/// the input comes out as `?。`.
pub fn apply_question_marks(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 3);
    for sentence in text.split('。').filter(|part| !part.is_empty()) {
        out.push_str(sentence);
        let question = QUESTION_SUFFIXES
            .iter()
            .any(|suffix| sentence.ends_with(suffix));
        out.push(if question { '？' } else { '。' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Logits for `n` characters that give every position the same scores.
    fn flat(n: usize, comma: f32, period: f32) -> Vec<f32> {
        (0..n + 2).flat_map(|_| [comma, period]).collect()
    }

    fn chars(text: &str) -> Vec<char> {
        text.chars().collect()
    }

    #[test]
    fn a_period_is_added_at_the_end_when_the_model_scores_low_everywhere() -> Result<(), AsrError> {
        let text = chars("今日は晴れ");
        assert_eq!(punctuate(&text, &flat(5, -9.0, -9.0))?, "今日は晴れ。");
        Ok(())
    }

    #[test]
    fn a_high_comma_score_inserts_a_comma_after_every_character() -> Result<(), AsrError> {
        let text = chars("あい");
        assert_eq!(punctuate(&text, &flat(2, 9.0, -9.0))?, "あ、い、。");
        Ok(())
    }

    #[test]
    fn a_period_beats_a_comma_at_the_same_position() -> Result<(), AsrError> {
        let text = chars("あい");
        assert_eq!(punctuate(&text, &flat(2, 9.0, 9.0))?, "あ。い。");
        Ok(())
    }

    #[test]
    fn the_threshold_is_inclusive() -> Result<(), AsrError> {
        let text = chars("あい");
        // A logit of 0 scores exactly 0.5.
        assert_eq!(punctuate(&text, &flat(2, -9.0, 0.0))?, "あ。い。");
        Ok(())
    }

    #[test]
    fn no_mark_goes_after_a_mark_or_before_one() -> Result<(), AsrError> {
        let text = chars("あ、い");
        // Position 0 is before a mark, position 1 is a mark, position 2 is last.
        assert_eq!(punctuate(&text, &flat(3, 9.0, -9.0))?, "あ、い、。");
        Ok(())
    }

    #[test]
    fn a_text_that_already_ends_in_a_mark_gets_no_extra_period() -> Result<(), AsrError> {
        let text = chars("だめ!");
        assert_eq!(punctuate(&text, &flat(3, -9.0, -9.0))?, "だめ!。");
        Ok(())
    }

    #[test]
    fn a_question_suffix_turns_the_final_period_into_a_question_mark() -> Result<(), AsrError> {
        let text = chars("これは本当ですか");
        assert_eq!(
            punctuate(&text, &flat(8, -9.0, -9.0))?,
            "これは本当ですか？"
        );
        Ok(())
    }

    #[test]
    fn the_row_of_a_character_is_the_row_after_the_begin_marker() -> Result<(), AsrError> {
        // Only the row of the first character asks for a comma; row 0 (begin) asks for none.
        let mut logits = flat(2, -9.0, -9.0);
        logits[2] = 9.0;
        assert_eq!(punctuate(&chars("あい"), &logits)?, "あ、い。");
        Ok(())
    }

    #[test]
    fn logits_of_the_wrong_length_are_a_shape_error() {
        let said = punctuate(&chars("あい"), &flat(3, 0.0, 0.0));
        assert!(matches!(said, Err(AsrError::Shape(_))), "{said:?}");
    }

    #[test]
    fn question_suffixes_cover_all_ten_endings() {
        for suffix in QUESTION_SUFFIXES {
            assert_eq!(
                apply_question_marks(&format!("あ{suffix}。")),
                format!("あ{suffix}？")
            );
        }
    }

    #[test]
    fn a_declarative_sentence_keeps_its_period() {
        assert_eq!(apply_question_marks("これはペンです。"), "これはペンです。");
    }

    #[test]
    fn each_sentence_is_judged_on_its_own_ending() {
        assert_eq!(
            apply_question_marks("行きますか。行きます。"),
            "行きますか？行きます。"
        );
    }

    #[test]
    fn empty_text_stays_empty() {
        assert_eq!(apply_question_marks(""), "");
    }

    #[test]
    fn a_text_ending_in_a_question_mark_gains_a_period_as_the_original_does() {
        assert_eq!(apply_question_marks("本当?"), "本当?。");
    }

    #[test]
    fn the_mark_set_has_the_half_width_and_full_width_forms() {
        for c in ['！', '!', '？', '?', '（', '(', '\n'] {
            assert!(is_mark(c), "{c}");
        }
        assert!(!is_mark('あ'));
    }
}

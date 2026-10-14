//! The translation inside E's answer.
//!
//! The prompt for E ends in labelled sections: the source (`<language>:`),
//! the draft (`Draft <language>:`) and the place where the answer goes
//! (`Rewritten <language>:`). A model sometimes writes the sections back
//! instead of only the rewritten text. Such an answer holds a Japanese or an
//! English sentence somewhere, so the form check of V passes it, and the
//! template would reach the subtitle.
//!
//! [`translation_of`] returns the text behind the last label of the answer.
//! The language word of a label is read loosely because the model misspells
//! it (`Japaese`, `Drafted Japanese`).
//!
//! An answer that is empty behind its labels, or that still has a draft or
//! rewritten label inside a line, yields the empty text, which V holds for
//! `form`. V itself is not made stricter: a correct translation may carry
//! Latin letters or line breaks.
//!
//! A line of a translation that itself starts with a word such as `Draft:` is
//! read as a label; the text before it is dropped.

use crate::verify::contains_eos;

/// Longest head of a label, in characters, from its first letter to its colon.
const MAX_LABEL_CHARS: usize = 40;

/// Longest head of a label, in words: the keyword and the language name.
const MAX_LABEL_WORDS: usize = 2;

/// Longest keyword of the draft label (`Drafted`), in bytes. The keyword
/// starts with `draf` because the model cuts it short (`Draf`).
const MAX_DRAFT_WORD: usize = 7;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Label {
    /// `<source language>:` in front of the source sentence.
    Source,
    /// `Draft <language>:` in front of T's translation.
    Draft,
    /// `Rewritten <language>:` in front of the answer.
    Rewritten,
}

/// The translation E's `answer` carries, without the labels of the prompt.
/// `source_name` is the name of the source language as the prompt spells it.
///
/// An answer with the end marker is returned as it stands, so that V withholds
/// it as it always did.
pub fn translation_of<'a>(answer: &'a str, source_name: &str) -> &'a str {
    let text = answer.trim();
    if contains_eos(text) {
        return text;
    }
    let rest = match last_label_end(text, source_name) {
        Some(end) => text[end..].trim(),
        None => text,
    };
    if has_inline_label(rest) { "" } else { rest }
}

/// The byte offset behind the last label that starts a line of `text`. The
/// source label counts only at the very start of the text.
fn last_label_end(text: &str, source_name: &str) -> Option<usize> {
    std::iter::once(0)
        .chain(text.match_indices('\n').map(|(at, _)| at + 1))
        .filter_map(|start| {
            let body = text[start..].trim_start_matches([' ', '\t']);
            let offset = text.len() - body.len();
            let (label, len) = label_at(body, source_name)?;
            (label != Label::Source || offset == 0).then_some(offset + len)
        })
        .next_back()
}

/// Whether a draft or rewritten label starts at any word of `text`.
fn has_inline_label(text: &str) -> bool {
    std::iter::once(0)
        .chain(
            text.char_indices()
                .filter(|(_, c)| c.is_whitespace())
                .map(|(at, c)| at + c.len_utf8()),
        )
        .any(|start| {
            matches!(
                label_at(&text[start..], ""),
                Some((Label::Draft | Label::Rewritten, _))
            )
        })
}

fn is_colon(c: char) -> bool {
    matches!(c, ':' | '：')
}

/// The label that starts `text` and the byte length of it through its colon.
fn label_at(text: &str, source_name: &str) -> Option<(Label, usize)> {
    let (at, colon) = text
        .char_indices()
        .take(MAX_LABEL_CHARS)
        .find(|(_, c)| is_colon(*c))?;
    let label = label_of(&text[..at], source_name)?;
    Some((label, at + colon.len_utf8()))
}

/// The label whose words, from the first letter to the colon, are `head`.
fn label_of(head: &str, source_name: &str) -> Option<Label> {
    if head.contains(['\n', '\r']) {
        return None;
    }
    let mut words = head.split_whitespace();
    let first = words.next()?;
    let words_after = words.clone().count();
    let only_letters = std::iter::once(first)
        .chain(words)
        .all(|word| word.chars().all(char::is_alphabetic));
    if !only_letters || words_after + 1 > MAX_LABEL_WORDS {
        return None;
    }
    if words_after == 0 && first.eq_ignore_ascii_case(source_name) {
        return Some(Label::Source);
    }
    if starts_with_ignore_case(first, "draf") && first.len() <= MAX_DRAFT_WORD {
        return Some(Label::Draft);
    }
    if first.eq_ignore_ascii_case("rewritten") {
        return Some(Label::Rewritten);
    }
    None
}

fn starts_with_ignore_case(word: &str, prefix: &str) -> bool {
    word.as_bytes()
        .get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = "English";
    const JA: &str = "彼は中に入って手紙を調べた。";

    #[test]
    fn a_plain_translation_is_returned_trimmed() {
        assert_eq!(translation_of(&format!("  {JA}\n"), SRC), JA);
    }

    #[test]
    fn the_text_behind_the_rewritten_label_is_kept() {
        let answer = format!(
            "English:\nHe went in.\n\nDraft Japanese:\n下書き\n\nRewritten Japanese:\n{JA}"
        );
        assert_eq!(translation_of(&answer, SRC), JA);
    }

    #[test]
    fn a_label_with_trailing_spaces_and_a_misspelled_language_is_read() {
        let answer =
            format!("He went in.  \n\nDrafted Japaese:  \n下書き  \n\nRewritten Japaese:  \n{JA}");
        assert_eq!(translation_of(&answer, SRC), JA);
    }

    #[test]
    fn a_label_on_the_same_line_as_the_text_is_read() {
        assert_eq!(
            translation_of(&format!("Rewritten Japanese: {JA}"), SRC),
            JA
        );
    }

    #[test]
    fn a_full_width_colon_ends_a_label() {
        assert_eq!(
            translation_of(&format!("Rewritten Japanese：{JA}"), SRC),
            JA
        );
    }

    #[test]
    fn only_the_source_label_in_front_leaves_the_source_for_v() {
        assert_eq!(translation_of("English:\nHe went in.", SRC), "He went in.");
    }

    #[test]
    fn the_source_label_is_read_only_at_the_start() {
        let line = "彼は言った。\nEnglish: yes";
        assert_eq!(translation_of(line, SRC), line);
    }

    #[test]
    fn the_source_label_is_the_name_of_the_source_language() {
        let ja = "Japanese:\n彼は入った。\n\nDraft English:\nHe went in.\n\nRewritten English:\nHe entered.";
        assert_eq!(translation_of(ja, "Japanese"), "He entered.");
        assert_eq!(
            translation_of("Japanese:\nHe entered.", SRC),
            "Japanese:\nHe entered."
        );
    }

    #[test]
    fn a_label_with_nothing_behind_it_yields_the_empty_text() {
        assert_eq!(
            translation_of("Draft Japanese:\n下書き\n\nRewritten Japanese:\n", SRC),
            ""
        );
        assert_eq!(translation_of("Rewritten Japanese:", SRC), "");
        assert_eq!(translation_of("", SRC), "");
    }

    #[test]
    fn a_label_inside_a_line_yields_the_empty_text() {
        assert_eq!(
            translation_of(&format!("{JA} Rewritten Japanese: {JA}"), SRC),
            ""
        );
        assert_eq!(
            translation_of(
                &format!("{JA}\nDraft Japanese: x\n{JA} Draft Japanese: y"),
                SRC
            ),
            ""
        );
    }

    #[test]
    fn the_end_marker_keeps_the_answer_as_it_stands() {
        let answer = format!("Rewritten Japanese:\n{JA}</s>");
        assert_eq!(translation_of(&answer, SRC), answer);
    }

    #[test]
    fn a_colon_in_a_sentence_is_not_a_label() {
        for text in [
            "彼は言った。理由は次の通り: 雨だった。",
            "The draft was late: nobody came.",
            "Draft 3: the plan",
            "Rewriting history: a note",
            "彼が書いた草稿: 長い",
        ] {
            assert_eq!(translation_of(text, SRC), text, "{text}");
        }
    }

    #[test]
    fn a_label_head_over_two_words_is_not_a_label() {
        let text = "Draft of the plan: yes";
        assert_eq!(translation_of(text, SRC), text);
    }

    #[test]
    fn a_label_in_lower_case_is_read() {
        assert_eq!(
            translation_of(&format!("rewritten japanese:\n{JA}"), SRC),
            JA
        );
    }
}

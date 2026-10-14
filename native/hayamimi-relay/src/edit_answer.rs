//! The translation inside E's answer.
//!
//! The prompt for E ends in labelled sections: the source (`<language>:`),
//! the draft (`Draft <language>:`) and the place where the answer goes
//! (`Rewritten <language>:`). A model sometimes writes the sections back
//! instead of only the rewritten text. Such an answer holds a Japanese or an
//! English sentence somewhere, so the form check of V passes it, and the
//! template would reach the subtitle.
//!
//! [`translation_of`] returns the text behind the last draft or rewritten
//! label of the answer. A label is the keyword and the name of the language of
//! the translation, then a colon, and it stands at the start of a line. The
//! name is read within two edits of the name the prompt used, because the model
//! misspells it (`Japaese`). A word that is not that name (`Draft law:`), a
//! keyword inside a line (`The draft: ...`) and a lone source label
//! (`English: ...`) are text, not labels. The source label needs no reading of
//! its own: when the answer has a draft or rewritten label, everything before
//! the last one is dropped, the source label included.
//!
//! An answer that is empty behind its labels yields the empty text, which V
//! holds for `form`. V itself is not made stricter: a correct translation may
//! carry Latin letters or line breaks. A label inside a line is left in the
//! text, and V decides on it as on any other text.

use crate::verify::contains_eos;

/// Longest head of a label, in characters, from its first letter to its colon.
const MAX_LABEL_CHARS: usize = 40;

/// Longest keyword of the draft label (`Drafted`), in bytes. The keyword
/// starts with `draf` because the model cuts it short (`Draf`).
const MAX_DRAFT_WORD: usize = 7;

/// How far the language word may be from the language name, in edits.
const MAX_EDITS: usize = 2;

/// Longest word, in bytes, that is compared with a language name.
const MAX_NAME_BYTES: usize = 24;

/// The translation E's `answer` carries, without the labels of the prompt.
/// `target_name` is the name of the language of the translation as the prompt
/// spells it.
///
/// An answer with the end marker is returned as it stands, so that V withholds
/// it as it always did.
pub fn translation_of<'a>(answer: &'a str, target_name: &str) -> &'a str {
    let text = answer.trim();
    if contains_eos(text) {
        return text;
    }
    match last_label_end(text, target_name) {
        Some(end) => text[end..].trim(),
        None => text,
    }
}

/// The byte offset behind the last label that starts a line of `text`.
fn last_label_end(text: &str, target_name: &str) -> Option<usize> {
    std::iter::once(0)
        .chain(text.match_indices('\n').map(|(at, _)| at + 1))
        .filter_map(|start| {
            let body = text[start..].trim_start_matches([' ', '\t']);
            let len = label_len(body, target_name)?;
            Some(text.len() - body.len() + len)
        })
        .next_back()
}

fn is_colon(c: char) -> bool {
    matches!(c, ':' | '：')
}

/// The byte length through the colon of the label that starts `body`.
fn label_len(body: &str, target_name: &str) -> Option<usize> {
    let (at, colon) = body
        .char_indices()
        .take(MAX_LABEL_CHARS)
        .find(|(_, c)| is_colon(*c))?;
    let head = &body[..at];
    if head.contains(['\n', '\r']) {
        return None;
    }
    let mut words = head.split_whitespace();
    let (keyword, language) = (words.next()?, words.next()?);
    let reads_as_label =
        words.next().is_none() && is_keyword(keyword) && names_language(language, target_name);
    reads_as_label.then_some(at + colon.len_utf8())
}

fn is_keyword(word: &str) -> bool {
    let draft = word.len() <= MAX_DRAFT_WORD
        && word
            .as_bytes()
            .get(..4)
            .is_some_and(|head| head.eq_ignore_ascii_case(b"draf"))
        && word.chars().all(char::is_alphabetic);
    draft || word.eq_ignore_ascii_case("rewritten")
}

/// Whether `word` is `name` or a misspelling of it, ignoring case.
fn names_language(word: &str, name: &str) -> bool {
    let (a, b) = (word.as_bytes(), name.as_bytes());
    if a.len() > MAX_NAME_BYTES || b.len() > MAX_NAME_BYTES || a.len().abs_diff(b.len()) > MAX_EDITS
    {
        return false;
    }
    let mut previous = [0_usize; MAX_NAME_BYTES + 1];
    for (j, slot) in previous.iter_mut().enumerate().take(b.len() + 1) {
        *slot = j;
    }
    let mut current = previous;
    for (i, x) in a.iter().enumerate() {
        current[0] = i + 1;
        for (j, y) in b.iter().enumerate() {
            let cost = usize::from(!x.eq_ignore_ascii_case(y));
            current[j + 1] = (previous[j] + cost)
                .min(previous[j + 1] + 1)
                .min(current[j] + 1);
        }
        previous = current;
    }
    previous[b.len()] <= MAX_EDITS
}

#[cfg(test)]
mod tests {
    use super::*;

    const TO: &str = "Japanese";
    const JA: &str = "彼は中に入って手紙を調べた。";

    #[test]
    fn a_plain_translation_is_returned_trimmed() {
        assert_eq!(translation_of(&format!("  {JA}\n"), TO), JA);
    }

    #[test]
    fn the_text_behind_the_rewritten_label_is_kept() {
        let answer = format!(
            "English:\nHe went in.\n\nDraft Japanese:\n下書き\n\nRewritten Japanese:\n{JA}"
        );
        assert_eq!(translation_of(&answer, TO), JA);
    }

    #[test]
    fn a_label_with_trailing_spaces_and_a_misspelled_language_is_read() {
        let answer =
            format!("He went in.  \n\nDrafted Japaese:  \n下書き  \n\nRewritten Japaese:  \n{JA}");
        assert_eq!(translation_of(&answer, TO), JA);
    }

    #[test]
    fn a_label_on_the_same_line_as_the_text_is_read() {
        assert_eq!(translation_of(&format!("Rewritten Japanese: {JA}"), TO), JA);
    }

    #[test]
    fn a_full_width_colon_ends_a_label() {
        assert_eq!(translation_of(&format!("Rewritten Japanese：{JA}"), TO), JA);
    }

    #[test]
    fn a_label_in_lower_case_is_read() {
        assert_eq!(
            translation_of(&format!("rewritten japanese:\n{JA}"), TO),
            JA
        );
    }

    #[test]
    fn into_english_the_language_is_english() {
        let answer = "Japanese:\n彼は入った。\n\nDraft English:\nHe went in.\n\nRewritten English:\nHe entered.";
        assert_eq!(translation_of(answer, "English"), "He entered.");
    }

    #[test]
    fn a_label_with_nothing_behind_it_yields_the_empty_text() {
        assert_eq!(
            translation_of("Draft Japanese:\n下書き\n\nRewritten Japanese:\n", TO),
            ""
        );
        assert_eq!(translation_of("Rewritten Japanese:", TO), "");
        assert_eq!(translation_of("", TO), "");
    }

    #[test]
    fn the_end_marker_keeps_the_answer_as_it_stands() {
        let answer = format!("Rewritten Japanese:\n{JA}</s>");
        assert_eq!(translation_of(&answer, TO), answer);
    }

    #[test]
    fn text_that_only_looks_like_a_label_is_returned_as_it_is() {
        for text in [
            "The draft: nobody read it.",
            "Final draft: ready.",
            "The Draft Board: it called him up.",
            "He said the rewritten: version is better.",
            "Draft law: passed.",
            "Draft plan: submit by Friday.",
            "Japanese: language class",
            "English: 英語の授業",
            "English:\nHe went in.",
            "彼は言った。理由は次の通り: 雨だった。",
            "Draft 3: the plan",
            "Rewriting history: a note",
            "Draft: the plan",
        ] {
            assert_eq!(translation_of(text, TO), text, "{text}");
        }
    }

    #[test]
    fn a_label_inside_a_line_is_not_a_label() {
        let text = format!("{JA} Rewritten Japanese: {JA}");
        assert_eq!(translation_of(&text, TO), text);
    }

    #[test]
    fn the_language_word_is_read_within_two_edits() {
        assert!(names_language("Japanese", "Japanese"));
        assert!(names_language("japaese", "Japanese"));
        assert!(names_language("Japnse", "Japanese"));
        assert!(!names_language("law", "Japanese"));
        assert!(!names_language("plan", "Japanese"));
        assert!(!names_language("", "Japanese"));
        assert!(!names_language("Japaneseeee", "Japanese"));
    }
}

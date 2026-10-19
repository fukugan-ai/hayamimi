//! The user's replacement dictionary: literal find-and-replace pairs applied
//! to the finished text, after ITN and punctuation, so that the user has the
//! last word over everything the earlier stages produced.

use std::path::Path;

use crate::error::AsrError;

/// The separators that split a line into the wrong and the right form, tried
/// in this order.
const SEPARATORS: [char; 3] = ['=', '\t', '→'];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Replacements {
    pairs: Vec<(String, String)>,
}

impl Replacements {
    /// Reads `wrong=right` lines. Blank lines and lines starting with `#` are
    /// skipped, and so are lines without a separator and pairs whose wrong
    /// form is empty (an empty pattern would match between every character).
    pub fn parse(text: &str) -> Replacements {
        let mut pairs = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some(separator) = SEPARATORS.iter().find(|sep| line.contains(**sep)) else {
                continue;
            };
            if let Some((wrong, right)) = line.split_once(*separator) {
                let wrong = wrong.trim();
                if !wrong.is_empty() {
                    pairs.push((wrong.to_owned(), right.trim().to_owned()));
                }
            }
        }
        Replacements { pairs }
    }

    /// The dictionary of `pairs`, applied in the order given. A pair whose
    /// wrong form is empty is left out, as when reading a file.
    pub fn from_pairs(pairs: Vec<(String, String)>) -> Replacements {
        Replacements {
            pairs: pairs
                .into_iter()
                .filter(|(wrong, _)| !wrong.is_empty())
                .collect(),
        }
    }

    /// The pairs, in the order they are applied.
    pub fn pairs(&self) -> &[(String, String)] {
        &self.pairs
    }

    pub fn load(path: &Path) -> Result<Replacements, AsrError> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| AsrError::Io(format!("{}: {error}", path.display())))?;
        Ok(Replacements::parse(&text))
    }

    pub fn len(&self) -> usize {
        self.pairs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }

    /// Applies every pair, in file order, to `text`.
    pub fn apply(&self, text: &str) -> String {
        let mut out = text.to_owned();
        for (wrong, right) in &self.pairs {
            out = out.replace(wrong.as_str(), right);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pair_is_applied() {
        let replacements = Replacements::parse("ぐるぐる=グルグル\n");
        assert_eq!(replacements.apply("ぐるぐる回る"), "グルグル回る");
    }

    #[test]
    fn tab_and_arrow_separate_too() {
        let replacements = Replacements::parse("あ\tい\nう→え\n");
        assert_eq!(replacements.apply("あう"), "いえ");
    }

    #[test]
    fn comments_blank_lines_and_lines_without_a_separator_are_skipped() {
        let replacements = Replacements::parse("# note\n\n   \nnoseparator\n");
        assert!(replacements.is_empty());
    }

    #[test]
    fn an_empty_wrong_form_is_skipped() {
        let replacements = Replacements::parse("=x\n");
        assert_eq!(replacements.len(), 0);
        assert_eq!(replacements.apply("abc"), "abc");
    }

    #[test]
    fn an_empty_right_form_deletes() {
        let replacements = Replacements::parse("えー=\n");
        assert_eq!(replacements.apply("えーこんにちは"), "こんにちは");
    }

    #[test]
    fn the_first_separator_in_the_order_wins() {
        let replacements = Replacements::parse("a=b→c\n");
        assert_eq!(replacements.apply("a"), "b→c");
    }

    #[test]
    fn pairs_apply_in_file_order() {
        let replacements = Replacements::parse("a=b\nb=c\n");
        assert_eq!(replacements.apply("a"), "c");
    }

    #[test]
    fn whitespace_around_the_forms_is_trimmed() {
        let replacements = Replacements::parse("  a  =  b  \n");
        assert_eq!(replacements.apply("a"), "b");
    }

    #[test]
    fn pairs_given_directly_keep_their_order_and_drop_an_empty_wrong_form() {
        let replacements = Replacements::from_pairs(vec![
            (String::from("a"), String::from("b")),
            (String::new(), String::from("x")),
            (String::from("b"), String::from("c")),
        ]);
        assert_eq!(replacements.len(), 2);
        assert_eq!(replacements.apply("a"), "c");
        assert_eq!(replacements.pairs()[1].0, "b");
    }

    #[test]
    fn load_replacements_formats() -> Result<(), Box<dyn std::error::Error>> {
        let scratch = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../target/unit-tmp"))
            .join(format!("replace-{}", std::process::id()));
        std::fs::create_dir_all(&scratch)?;
        let file = scratch.join("dict.txt");
        std::fs::write(&file, "# comment\nA=B\nC\tD\nE→F\n\n")?;
        let loaded = Replacements::load(&file);
        std::fs::remove_dir_all(&scratch)?;
        let loaded = loaded?;
        let pairs: Vec<(&str, &str)> = loaded
            .pairs()
            .iter()
            .map(|(wrong, right)| (wrong.as_str(), right.as_str()))
            .collect();
        assert_eq!(pairs, [("A", "B"), ("C", "D"), ("E", "F")]);
        Ok(())
    }

    #[test]
    fn load_replacements_empty_path() {
        // No dictionary configured means no pairs, and the text passes unchanged.
        let none = Replacements::default();
        assert!(none.is_empty());
        assert!(Replacements::parse("").is_empty());
        assert_eq!(none.apply("A=B"), "A=B");
    }

    #[test]
    fn a_missing_file_is_an_io_error() {
        let said = Replacements::load(Path::new("no-such-dictionary.txt"));
        assert!(matches!(said, Err(AsrError::Io(_))), "{said:?}");
    }
}

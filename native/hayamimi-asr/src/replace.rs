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

    /// Where the tests that need a file make it; the directory is not tracked.
    const SCRATCH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../target/unit-tmp");

    fn pair(wrong: &str, right: &str) -> (String, String) {
        (wrong.to_owned(), right.to_owned())
    }

    #[test]
    fn load_replacements_formats() -> Result<(), Box<dyn std::error::Error>> {
        let text = "# comment\nA=B\nC\tD\nE→F\n\n";
        let directory = Path::new(SCRATCH).join(format!("replace-formats-{}", std::process::id()));
        std::fs::create_dir_all(&directory)?;
        let file = directory.join("dict.txt");
        std::fs::write(&file, text)?;
        let loaded = Replacements::load(&file)?;
        assert_eq!(
            loaded.pairs(),
            [pair("A", "B"), pair("C", "D"), pair("E", "F")]
        );
        assert_eq!(loaded, Replacements::parse(text));
        Ok(())
    }

    #[test]
    fn load_replacements_empty_path() {
        // No dictionary path is `None`, so nothing is loaded and the settings
        // hold the empty dictionary, which leaves a text as it is.
        let none = crate::pipeline::Settings::new(std::path::PathBuf::new()).replacements;
        assert!(none.is_empty());
        assert_eq!(none, Replacements::parse(""));
        assert_eq!(none.apply("abc"), "abc");
    }

    #[test]
    fn a_missing_file_is_an_io_error() {
        let said = Replacements::load(Path::new("no-such-dictionary.txt"));
        assert!(matches!(said, Err(AsrError::Io(_))), "{said:?}");
    }
}

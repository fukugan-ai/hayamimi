//! The error of one clip, scored the way the Python scorecard scores it:
//! English by word error rate, Cantonese by character error rate after both
//! sides are folded from traditional to simplified characters, every other
//! language by character error rate.

use ferrous_opencc::OpenCC;
use ferrous_opencc::config::BuiltinConfig;
use unicode_normalization::UnicodeNormalization;

use crate::cer::{Cer, cer, is_punctuation, levenshtein};
use crate::error::EvalError;

/// The words an English error rate compares: lower case, NFKC, punctuation
/// turned into a space, runs of space collapsed.
pub fn normalize_en(text: &str) -> Vec<String> {
    let folded: String = text
        .to_lowercase()
        .nfkc()
        .map(|c| if is_punctuation(c) { ' ' } else { c })
        .collect();
    folded.split_whitespace().map(str::to_owned).collect()
}

/// The word error of `hyp` against `reference`. A reference with no word
/// scores 0 whatever the hypothesis says.
pub fn wer(reference: &str, hyp: &str) -> Cer {
    let r = normalize_en(reference);
    let h = normalize_en(hyp);
    Cer {
        dist: levenshtein(&r, &h),
        denom: r.len(),
    }
}

/// Scores a clip by the metric of its language.
pub struct Scorer {
    t2s: OpenCC,
}

impl Scorer {
    pub fn new() -> Result<Scorer, EvalError> {
        let t2s = OpenCC::from_config(BuiltinConfig::T2s)
            .map_err(|error| EvalError::Invalid(format!("OpenCC t2s did not open: {error}")))?;
        Ok(Scorer { t2s })
    }

    /// The error of `hyp` against `reference` for a clip of `lang`.
    pub fn score(&self, lang: &str, reference: &str, hyp: &str) -> Cer {
        match lang {
            "en" => wer(reference, hyp),
            "yue" => cer(&self.t2s.convert(reference), &self.t2s.convert(hyp)),
            _ => cer(reference, hyp),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_and_punctuation_do_not_count_as_word_errors() {
        assert_eq!(wer("Hello, World.", "hello world").dist, 0);
    }

    #[test]
    fn one_wrong_word_in_six_is_a_sixth() {
        let measured = wer("the cat sat on the mat", "the cat sat on a mat");
        assert_eq!(measured, Cer { dist: 1, denom: 6 });
        assert_eq!(measured.micros(), 166_666);
    }

    #[test]
    fn a_reference_with_no_word_scores_zero() {
        assert_eq!(wer(" , . ", "x").micros(), 0);
    }

    #[test]
    fn an_empty_hypothesis_costs_every_word() {
        assert_eq!(wer("a b", "").micros(), 1_000_000);
    }

    #[test]
    fn cantonese_is_folded_to_simplified_on_both_sides() -> Result<(), EvalError> {
        let scorer = Scorer::new()?;
        assert_eq!(scorer.score("yue", "學習", "学习").dist, 0);
        assert_eq!(scorer.score("zh", "學習", "学习").dist, 2);
        Ok(())
    }

    #[test]
    fn a_language_without_a_metric_of_its_own_scores_by_characters() -> Result<(), EvalError> {
        let scorer = Scorer::new()?;
        assert_eq!(
            scorer.score("ko", "안녕하세요", "안녕하세오"),
            Cer { dist: 1, denom: 5 }
        );
        Ok(())
    }
}

//! chrF as sacrebleu computes it with its defaults: character n-grams of
//! order 1 to 6, white space left out, beta 2, case kept. A corpus score is
//! the score of the statistics summed over every sentence, not the mean of
//! the sentence scores.
//!
//! A sentence the translator withheld is an empty hypothesis. It stays in
//! the corpus: its reference n-grams count against recall.

use std::collections::HashMap;
use std::path::Path;

use crate::error::EvalError;

/// The longest character n-gram.
const ORDER: usize = 6;
/// beta squared, with beta 2: recall counts four times as much as precision.
const BETA_SQUARED: f64 = 4.0;

/// For each n-gram length, the n-grams of the hypothesis and of the reference
/// and how many of them match.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    hypothesis: [u64; ORDER],
    reference: [u64; ORDER],
    matching: [u64; ORDER],
}

/// The characters of `text` that chrF counts: everything but white space.
fn counted_chars(text: &str) -> Vec<char> {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

fn gram_counts(chars: &[char], length: usize) -> HashMap<&[char], u64> {
    let mut counts = HashMap::new();
    for gram in chars.windows(length) {
        *counts.entry(gram).or_insert(0) += 1;
    }
    counts
}

fn total(counts: &HashMap<&[char], u64>) -> u64 {
    counts.values().sum()
}

/// How many n-grams the two multisets share.
fn overlap(left: &HashMap<&[char], u64>, right: &HashMap<&[char], u64>) -> u64 {
    left.iter()
        .map(|(gram, count)| right.get(gram).map_or(0, |other| (*count).min(*other)))
        .sum()
}

impl Stats {
    /// The statistics of one reference and its hypothesis.
    pub fn of(reference: &str, hypothesis: &str) -> Stats {
        let (reference, hypothesis) = (counted_chars(reference), counted_chars(hypothesis));
        let mut stats = Stats::default();
        for at in 0..ORDER {
            let reference_grams = gram_counts(&reference, at + 1);
            let hypothesis_grams = gram_counts(&hypothesis, at + 1);
            stats.reference[at] = total(&reference_grams);
            stats.hypothesis[at] = total(&hypothesis_grams);
            stats.matching[at] = overlap(&hypothesis_grams, &reference_grams);
        }
        stats
    }

    /// Adds the statistics of another sentence.
    pub fn add(&mut self, other: &Stats) {
        for at in 0..ORDER {
            self.hypothesis[at] += other.hypothesis[at];
            self.reference[at] += other.reference[at];
            self.matching[at] += other.matching[at];
        }
    }

    /// The chrF score, 0 to 100: the F-beta of the precision and the recall
    /// averaged over the n-gram lengths that both sides have.
    pub fn score(&self) -> f64 {
        let (mut precision, mut recall, mut orders) = (0.0, 0.0, 0.0);
        for at in 0..ORDER {
            if self.hypothesis[at] > 0 && self.reference[at] > 0 {
                precision += self.matching[at] as f64 / self.hypothesis[at] as f64;
                recall += self.matching[at] as f64 / self.reference[at] as f64;
                orders += 1.0;
            }
        }
        if orders == 0.0 {
            return 0.0;
        }
        let (precision, recall) = (precision / orders, recall / orders);
        let denominator = BETA_SQUARED * precision + recall;
        if denominator == 0.0 {
            return 0.0;
        }
        100.0 * (1.0 + BETA_SQUARED) * precision * recall / denominator
    }
}

/// The score of one sentence.
pub fn sentence_chrf(reference: &str, hypothesis: &str) -> f64 {
    Stats::of(reference, hypothesis).score()
}

/// A corpus score and the number of sentence pairs it was taken over.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Corpus {
    pub pairs: usize,
    pub score: f64,
}

/// The corpus score of `hypotheses` against `references`, line for line. A
/// different number of lines is an error that names both numbers.
pub fn corpus_chrf(references: &[&str], hypotheses: &[&str]) -> Result<Corpus, EvalError> {
    if references.len() != hypotheses.len() {
        return Err(EvalError::Invalid(format!(
            "{} hypothesis lines against {} reference lines",
            hypotheses.len(),
            references.len()
        )));
    }
    let mut sum = Stats::default();
    for (reference, hypothesis) in references.iter().zip(hypotheses) {
        sum.add(&Stats::of(reference, hypothesis));
    }
    Ok(Corpus {
        pairs: references.len(),
        score: sum.score(),
    })
}

const REFERENCE_SUFFIX: &str = ".ref.txt";
const HYPOTHESIS_SUFFIX: &str = ".hyp.txt";

fn read_lines(path: &Path) -> Result<String, EvalError> {
    std::fs::read_to_string(path).map_err(|source| EvalError::io(path, source))
}

/// The languages that have a `<lang>.hyp.txt` in `hyps`, ordered by code.
fn hypothesis_languages(hyps: &Path) -> Result<Vec<String>, EvalError> {
    let entries = std::fs::read_dir(hyps).map_err(|source| EvalError::io(hyps, source))?;
    let mut languages = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| EvalError::io(hyps, source))?;
        let name = entry.file_name();
        if let Some(code) = name
            .to_str()
            .and_then(|name| name.strip_suffix(HYPOTHESIS_SUFFIX))
        {
            languages.push(code.to_owned());
        }
    }
    languages.sort();
    Ok(languages)
}

/// The corpus chrF of every `<lang>.hyp.txt` of `hyps` against the
/// `<lang>.ref.txt` of `pairs`. A hypothesis file without its reference is an
/// error, and so is a directory without hypothesis files.
pub fn score_directories(pairs: &Path, hyps: &Path) -> Result<Vec<(String, Corpus)>, EvalError> {
    let languages = hypothesis_languages(hyps)?;
    if languages.is_empty() {
        return Err(EvalError::Invalid(format!(
            "{}: no *{HYPOTHESIS_SUFFIX} file",
            hyps.display()
        )));
    }
    let mut scores = Vec::with_capacity(languages.len());
    for code in languages {
        let reference = read_lines(&pairs.join(format!("{code}{REFERENCE_SUFFIX}")))?;
        let hypothesis = read_lines(&hyps.join(format!("{code}{HYPOTHESIS_SUFFIX}")))?;
        let references: Vec<&str> = reference.lines().collect();
        let hypotheses: Vec<&str> = hypothesis.lines().collect();
        scores.push((code, corpus_chrf(&references, &hypotheses)?));
    }
    Ok(scores)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(left: f64, right: f64) -> bool {
        (left - right).abs() < 1e-9
    }

    #[test]
    fn identical_sentences_score_100() {
        assert!(close(sentence_chrf("The cat sat.", "The cat sat."), 100.0));
        assert!(close(
            sentence_chrf("猫が寝ている。", "猫が寝ている。"),
            100.0
        ));
    }

    #[test]
    fn white_space_is_not_counted() {
        assert!(close(sentence_chrf("a b c d", "abcd"), 100.0));
        // The ideographic space is white space too.
        assert!(close(sentence_chrf("猫　寝る", "猫寝る"), 100.0));
    }

    #[test]
    fn case_is_kept() {
        assert!(sentence_chrf("Cat", "cat") < 100.0);
    }

    #[test]
    fn an_empty_hypothesis_scores_zero() {
        assert!(close(sentence_chrf("The cat sat.", ""), 0.0));
        assert!(close(sentence_chrf("", ""), 0.0));
    }

    #[test]
    fn nothing_in_common_scores_zero() {
        assert!(close(sentence_chrf("aaaa", "bbbb"), 0.0));
    }

    /// A reference of "abcd" and a hypothesis of "abxd": order 1 matches
    /// a, b, d of 4; order 2 matches ab of 3; order 3 and 4 match none.
    /// precision = recall = (3/4 + 1/3 + 0 + 0) / 4 over the orders both
    /// sides have (1 to 4), so the score is 100 times that average.
    #[test]
    fn a_short_pair_is_scored_over_the_orders_both_sides_have() {
        let average = (3.0 / 4.0 + 1.0 / 3.0) / 4.0;
        assert!(close(sentence_chrf("abcd", "abxd"), 100.0 * average));
    }

    #[test]
    fn a_missing_word_costs_recall_more_than_an_extra_word_costs_precision() {
        let reference = "the quick brown fox jumps";
        let missing = sentence_chrf(reference, "the quick brown fox");
        let extra = sentence_chrf(reference, "the quick brown fox jumps over");
        assert!(extra > missing, "{extra} {missing}");
    }

    #[test]
    fn a_repeated_gram_is_matched_no_more_often_than_the_reference_has_it() {
        // "aaa" has 3 one-grams "a"; the reference has 1.
        let stats = Stats::of("a", "aaa");
        assert_eq!(stats.matching[0], 1);
        assert_eq!(stats.hypothesis[0], 3);
        assert_eq!(stats.reference[0], 1);
    }

    #[test]
    fn the_corpus_score_is_taken_over_summed_statistics() -> Result<(), EvalError> {
        // The second pair is longer and worse, so that the sum and the mean differ.
        let references = ["abcdef", "uvwxyzuvwxyz0123"];
        let hypotheses = ["abcdef", "uvwxyz"];
        let corpus = corpus_chrf(&references, &hypotheses)?;
        let mut sum = Stats::of(references[0], hypotheses[0]);
        sum.add(&Stats::of(references[1], hypotheses[1]));
        assert!(close(corpus.score, sum.score()));
        let mean = (sentence_chrf(references[0], hypotheses[0])
            + sentence_chrf(references[1], hypotheses[1]))
            / 2.0;
        assert!((corpus.score - mean).abs() > 1e-6);
        assert_eq!(corpus.pairs, 2);
        Ok(())
    }

    #[test]
    fn an_empty_hypothesis_stays_in_the_corpus() -> Result<(), EvalError> {
        let references = ["abcdef", "uvwxyz"];
        let counted = corpus_chrf(&references, &["abcdef", ""])?;
        let dropped = corpus_chrf(&references[..1], &["abcdef"])?;
        assert_eq!(counted.pairs, 2);
        assert!(counted.score < dropped.score);
        Ok(())
    }

    #[test]
    fn a_different_line_count_is_refused_with_both_numbers() {
        let said = corpus_chrf(&["a", "b"], &["a"])
            .err()
            .map(|e| e.to_string());
        assert!(
            said.as_deref()
                .is_some_and(|text| text.contains('1') && text.contains('2')),
            "{said:?}"
        );
    }

    #[test]
    fn no_pairs_score_zero() -> Result<(), EvalError> {
        let corpus = corpus_chrf(&[], &[])?;
        assert_eq!(corpus.pairs, 0);
        assert!(close(corpus.score, 0.0));
        Ok(())
    }

    #[test]
    fn a_hypothesis_directory_without_files_is_refused() {
        let said = score_directories(Path::new("."), Path::new("src/no-such-directory"));
        assert!(matches!(said, Err(EvalError::Io { .. })));
    }
}

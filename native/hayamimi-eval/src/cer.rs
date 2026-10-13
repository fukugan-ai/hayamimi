//! Japanese character error rate, scored the way the Python evaluation
//! scores it: NFKC, then punctuation and whitespace removed, then the
//! Levenshtein distance over code points divided by the reference length.

use unicode_normalization::UnicodeNormalization;

/// An edit distance and the reference length it is measured against. Rates
/// of many utterances are combined by summing both, never by averaging rates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cer {
    pub dist: usize,
    pub denom: usize,
}

impl Cer {
    /// The rate in millionths. A reference that is empty after
    /// normalisation scores 0 rather than dividing by zero.
    pub fn micros(self) -> u64 {
        if self.denom == 0 {
            return 0;
        }
        self.dist as u64 * 1_000_000 / self.denom as u64
    }

    /// The sum of two measurements.
    pub fn plus(self, other: Cer) -> Cer {
        Cer {
            dist: self.dist + other.dist,
            denom: self.denom + other.denom,
        }
    }
}

/// ASCII punctuation, and the CJK and full-width punctuation blocks.
fn is_punctuation(c: char) -> bool {
    c.is_ascii_punctuation()
        || ('\u{3000}'..='\u{303f}').contains(&c)
        || ('\u{ff01}'..='\u{ff0f}').contains(&c)
        || ('\u{ff1a}'..='\u{ff20}').contains(&c)
        || ('\u{ff3b}'..='\u{ff40}').contains(&c)
        || ('\u{ff5b}'..='\u{ff65}').contains(&c)
}

/// The text a Japanese error rate compares.
pub fn normalize_ja(text: &str) -> Vec<char> {
    text.nfkc()
        .filter(|c| !is_punctuation(*c) && !c.is_whitespace())
        .collect()
}

/// Levenshtein distance over two code point sequences.
fn levenshtein(a: &[char], b: &[char]) -> usize {
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0_usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        current[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let substitution = previous[j] + usize::from(ca != cb);
            current[j + 1] = substitution.min(previous[j + 1] + 1).min(current[j] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[b.len()]
}

/// The distance and reference length of `hyp` against `reference`.
pub fn cer(reference: &str, hyp: &str) -> Cer {
    let r = normalize_ja(reference);
    let h = normalize_ja(hyp);
    Cer {
        dist: levenshtein(&r, &h),
        denom: r.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_substitution_in_five_characters_is_0_2() {
        let measured = cer("こんにちは", "こんにちわ");
        assert_eq!(measured, Cer { dist: 1, denom: 5 });
        assert_eq!(measured.micros(), 200_000);
    }

    #[test]
    fn punctuation_is_not_counted() {
        assert_eq!(cer("こんにちは。", "こんにちは").dist, 0);
    }

    #[test]
    fn full_width_letters_fold_to_ascii() {
        assert_eq!(cer("ＡＢＣ", "ABC").dist, 0);
    }

    #[test]
    fn an_empty_hypothesis_costs_the_whole_reference() {
        assert_eq!(cer("こんにちは", "").micros(), 1_000_000);
    }

    #[test]
    fn an_empty_reference_costs_the_whole_hypothesis_in_distance() {
        assert_eq!(cer("", "あい"), Cer { dist: 2, denom: 0 });
    }

    #[test]
    fn a_reference_empty_after_normalisation_scores_zero() {
        let measured = cer("。", "あ");
        assert_eq!(measured.denom, 0);
        assert_eq!(measured.micros(), 0);
    }

    #[test]
    fn rates_combine_by_summing_both_parts() {
        let total = cer("あいうえお", "あいうえ").plus(cer("かき", "かき"));
        assert_eq!(total, Cer { dist: 1, denom: 7 });
    }

    #[test]
    fn insertions_and_deletions_each_cost_one() {
        assert_eq!(cer("あい", "あいう").dist, 1);
        assert_eq!(cer("あいう", "あい").dist, 1);
    }
}

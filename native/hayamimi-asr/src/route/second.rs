//! The agreement gate of the Japanese second opinion.
//!
//! The Parakeet CTC model reads clean single-speaker Japanese better than the
//! ReazonSpeech tier, but on broadcast audio it reads the off-caption speech
//! around the caption. When the two disagree, that pickup is almost always
//! why, so agreement between them is a clean-audio signal: the second reading
//! is adopted only when the two hypotheses are close.

/// Mutual character error rate at or below which the second reading is adopted.
pub const SECOND_OPINION_THRESHOLD: f64 = 0.25;

/// Whether a character carries no words: space, and the marks of a sentence.
fn is_blind(c: char) -> bool {
    c.is_whitespace() || "。、．，,.!?！？".contains(c)
}

fn without_marks(text: &str) -> Vec<char> {
    text.chars().filter(|c| !is_blind(*c)).collect()
}

/// The edit distance between two character strings (insertion, deletion and
/// substitution each cost one), in one row of working space.
fn levenshtein(a: &[char], b: &[char]) -> usize {
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = (above + 1)
                .min(row[j] + 1)
                .min(diagonal + usize::from(ca != cb));
            diagonal = above;
        }
    }
    row[b.len()]
}

/// The character error rate of `b` against `a`, blind to spaces and marks.
/// The denominator is the length of `a`, so callers pass the primary text first.
pub fn hyp_agreement_cer(a: &str, b: &str) -> f64 {
    let (a, b) = (without_marks(a), without_marks(b));
    if a.is_empty() {
        return if b.is_empty() { 0.0 } else { 1.0 };
    }
    levenshtein(&a, &b) as f64 / a.len() as f64
}

/// The text to keep and whether it is the second reading. An empty text on
/// either side keeps the primary; so does a second reading that differs from
/// it by more than `threshold`.
pub fn choose_second_opinion<'a>(
    primary: &'a str,
    second: &'a str,
    threshold: f64,
) -> (&'a str, bool) {
    if second.trim().is_empty() || primary.trim().is_empty() {
        return (primary, false);
    }
    if hyp_agreement_cer(primary, second) <= threshold {
        (second, true)
    } else {
        (primary, false)
    }
}

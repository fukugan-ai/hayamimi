//! The four-class Japanese punctuation model, chosen with `--punct-model 4class`.
//!
//! One token classifier reads the text and says, for each token, which of
//! `、` `。` `？` `！` follows it (or none). Unlike the BERT restorer next to
//! it, there is no second pass for the question mark: the model predicts it.
//!
//! This file holds the parts that need no model: the labels, the decode of the
//! scores into labels (with the gates that hold back the marks the model
//! over-uses on speech), and the rebuild of the text from the labels.

use crate::error::AsrError;

/// What follows a token, as the classifier head numbers it. The order is the
/// order of the head's output classes; changing it would relabel every
/// prediction without an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Label {
    /// No mark follows.
    None,
    /// `、`
    Comma,
    /// `。`
    Period,
    /// `？`
    Question,
    /// `！`
    Exclaim,
}

/// The labels in class order.
pub const LABELS: [Label; CLASSES] = [
    Label::None,
    Label::Comma,
    Label::Period,
    Label::Question,
    Label::Exclaim,
];

/// The classes of the head: none, and the four marks.
pub const CLASSES: usize = 5;

impl Label {
    /// The character this label inserts, or `None` for no mark.
    pub fn mark(self) -> Option<char> {
        match self {
            Label::None => None,
            Label::Comma => Some('、'),
            Label::Period => Some('。'),
            Label::Question => Some('？'),
            Label::Exclaim => Some('！'),
        }
    }
}

/// The class a gate holds back.
const COMMA_CLASS: usize = 1;
const EXCLAIM_CLASS: usize = 4;

/// The probability a mark must reach to be eligible; `None` leaves the mark
/// ungated. The model was trained on dense web prose and over-uses two marks
/// on speech: `！` fires on emphatic narration as confidently as on a real
/// exclamation, so it is held back for good by default (a value above 1 means
/// "never"), and `、` has to reach 0.8.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gates {
    pub exclaim: Option<f32>,
    pub comma: Option<f32>,
}

/// The `！` gate of a run that does not say otherwise: above 1, so never.
pub const EXCLAIM_THRESHOLD: f32 = 1.01;
/// The `、` gate of a run that does not say otherwise.
pub const COMMA_THRESHOLD: f32 = 0.8;

impl Default for Gates {
    fn default() -> Self {
        Gates {
            exclaim: Some(EXCLAIM_THRESHOLD),
            comma: Some(COMMA_THRESHOLD),
        }
    }
}

/// The softmax of one row of scores.
fn softmax(row: &[f32; CLASSES]) -> [f32; CLASSES] {
    let top = row.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let mut exps = row.map(|score| (score - top).exp());
    let sum: f32 = exps.iter().sum();
    exps.iter_mut().for_each(|e| *e /= sum);
    exps
}

/// Whether `class` may be chosen: a gated mark needs its probability to reach
/// its threshold, and every other class (`None` above all) is always eligible.
fn is_eligible(class: usize, probs: &[f32; CLASSES], gates: Gates) -> bool {
    let gate = match class {
        COMMA_CLASS => gates.comma,
        EXCLAIM_CLASS => gates.exclaim,
        _ => None,
    };
    gate.is_none_or(|threshold| probs[class] >= threshold)
}

/// The eligible class with the highest score, the first of equals. A score
/// that is not a number is never the highest. `None` is always eligible, so a
/// class always remains.
fn best_class(row: &[f32; CLASSES], gates: Gates) -> usize {
    let probs = softmax(row);
    let mut best = 0;
    for class in 1..CLASSES {
        if is_eligible(class, &probs, gates) && (row[best].is_nan() || row[class] > row[best]) {
            best = class;
        }
    }
    best
}

/// The label of each row of `logits`, a flat `[rows, 5]` array.
///
/// A gated mark is removed from the candidates before the highest is chosen,
/// so a comma that fails its gate cannot hand the position to a weak `！`.
pub fn decode(logits: &[f32], gates: Gates) -> Result<Vec<Label>, AsrError> {
    let (rows, rest) = logits.as_chunks::<CLASSES>();
    if !rest.is_empty() {
        return Err(AsrError::Shape(format!(
            "{} scores do not make rows of {CLASSES}",
            logits.len()
        )));
    }
    Ok(rows
        .iter()
        .map(|row| LABELS[best_class(row, gates)])
        .collect())
}

/// The text with the marks of `labels` put after the last character each
/// token covers. `offsets` are the `(start, end)` character positions of the
/// tokens in `text`, parallel to `labels`; an empty span is a special token
/// and is skipped. A position gets one mark: the first token that ends there
/// decides it.
pub fn reconstruct(text: &str, offsets: &[(usize, usize)], labels: &[Label]) -> String {
    let mut mark_at: Vec<Option<char>> = vec![None; text.chars().count()];
    for (&(start, end), label) in offsets.iter().zip(labels) {
        if end <= start {
            continue;
        }
        let slot = end.checked_sub(1).and_then(|last| mark_at.get_mut(last));
        if let (Some(slot), Some(mark)) = (slot, label.mark()) {
            slot.get_or_insert(mark);
        }
    }
    text.chars()
        .zip(mark_at)
        .flat_map(|(c, mark)| [Some(c), mark])
        .flatten()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const O: Label = Label::None;
    const COMMA: Label = Label::Comma;
    const PERIOD: Label = Label::Period;
    const Q: Label = Label::Question;
    const EXCLAIM: Label = Label::Exclaim;

    #[test]
    fn reconstruct_inserts_period_and_comma_after_token_end() {
        let text = "きょうはいいてんきです";
        // Three tokens: きょうは (0, 4), いいてんきです (4, 11), and the end marker (0, 0).
        let offsets = [(0, 4), (4, 11), (0, 0)];
        let labels = [COMMA, PERIOD, O];
        assert_eq!(
            reconstruct(text, &offsets, &labels),
            "きょうは、いいてんきです。"
        );
    }

    #[test]
    fn reconstruct_inserts_question_mark() {
        assert_eq!(
            reconstruct("だいじょうぶですか", &[(0, 9)], &[Q]),
            "だいじょうぶですか？"
        );
    }

    #[test]
    fn reconstruct_inserts_exclamation_mark() {
        assert_eq!(reconstruct("すごい", &[(0, 3)], &[EXCLAIM]), "すごい！");
    }

    #[test]
    fn reconstruct_no_mark_is_noop() {
        let text = "なにもおきません";
        let length = text.chars().count();
        assert_eq!(reconstruct(text, &[(0, length)], &[O]), text);
    }

    #[test]
    fn reconstruct_ignores_special_token_offsets() {
        // The begin and end markers report (0, 0): they are not "a mark before
        // the first character", whatever label they carry.
        let offsets = [(0, 0), (0, 3), (0, 0)];
        let labels = [PERIOD, O, PERIOD];
        assert_eq!(reconstruct("abc", &offsets, &labels), "abc");
    }

    #[test]
    fn reconstruct_only_first_mark_per_position_wins() {
        // Two tokens end at the same character. The earlier one is kept and
        // the later one never adds a second mark.
        let out = reconstruct("ab", &[(0, 2), (0, 2)], &[PERIOD, Q]);
        assert_eq!(out, "ab。");
    }

    #[test]
    fn reconstruct_multi_sentence_marks_mid_and_end() {
        let text = "きょうはあめですあしたははれるでしょうか";
        let length = text.chars().count();
        let offsets = [(0, 8), (8, length)];
        assert_eq!(
            reconstruct(text, &offsets, &[PERIOD, Q]),
            "きょうはあめです。あしたははれるでしょうか？"
        );
    }

    #[test]
    fn a_span_past_the_end_of_the_text_adds_no_mark() {
        assert_eq!(reconstruct("ab", &[(0, 5)], &[PERIOD]), "ab");
        assert_eq!(reconstruct("", &[(0, 1)], &[PERIOD]), "");
    }

    #[test]
    fn labels_longer_than_the_offsets_are_not_read() {
        assert_eq!(reconstruct("ab", &[(0, 2)], &[PERIOD, Q, COMMA]), "ab。");
    }

    #[test]
    fn punct4_labels_order() {
        // The head's classes are numbered in this order; a different order
        // would relabel every prediction.
        let marks = LABELS.map(Label::mark);
        assert_eq!(
            marks,
            [None, Some('、'), Some('。'), Some('？'), Some('！')]
        );
        assert_eq!(COMMA_CLASS, 1);
        assert_eq!(LABELS[COMMA_CLASS], Label::Comma);
        assert_eq!(LABELS[EXCLAIM_CLASS], Label::Exclaim);
    }

    /// Scores whose softmax is `probs`: the classes are `O 、 。 ？ ！`.
    fn log_probs(probs: [f32; CLASSES]) -> Vec<f32> {
        probs.iter().map(|p| (p + 1e-9).ln()).collect()
    }

    fn gated(exclaim: f32, comma: f32) -> Gates {
        Gates {
            exclaim: Some(exclaim),
            comma: Some(comma),
        }
    }

    const UNGATED: Gates = Gates {
        exclaim: None,
        comma: None,
    };

    #[test]
    fn decode_gates_remove_weak_marks_before_argmax() -> Result<(), AsrError> {
        // A comma at 0.52 fails a 0.8 gate and `！` at 0.45 is second: the
        // position must fall to 。 (or none), never to `！`.
        let weak = log_probs([0.004, 0.523, 0.026, 0.0001, 0.448]);
        assert_eq!(decode(&weak, gated(1.01, 0.8))?, [PERIOD]);
        // Gates off: plain argmax.
        assert_eq!(decode(&weak, UNGATED)?, [COMMA]);
        // A confident `！` passes its gate.
        let sure = log_probs([0.005, 0.005, 0.02, 0.0, 0.97]);
        assert_eq!(decode(&sure, gated(0.9, 0.8))?, [EXCLAIM]);
        assert_eq!(decode(&sure, gated(0.99, 0.8))?, [PERIOD]);
        // None is never gated, so a row where every mark is weak yields none.
        let faint = log_probs([0.4, 0.3, 0.1, 0.05, 0.15]);
        assert_eq!(decode(&faint, gated(0.5, 0.5))?, [O]);
        Ok(())
    }

    #[test]
    fn the_default_gates_never_emit_an_exclamation_and_hold_the_comma_at_eight_tenths()
    -> Result<(), AsrError> {
        let gates = Gates::default();
        assert_eq!(gates.exclaim, Some(1.01));
        assert_eq!(gates.comma, Some(0.8));
        let sure = log_probs([0.005, 0.005, 0.02, 0.0, 0.97]);
        assert_eq!(decode(&sure, gates)?, [PERIOD]);
        Ok(())
    }

    #[test]
    fn decode_reads_one_label_per_row() -> Result<(), AsrError> {
        let mut logits = log_probs([0.9, 0.025, 0.025, 0.025, 0.025]);
        logits.extend(log_probs([0.01, 0.01, 0.96, 0.01, 0.01]));
        logits.extend(log_probs([0.01, 0.01, 0.01, 0.96, 0.01]));
        assert_eq!(decode(&logits, UNGATED)?, [O, PERIOD, Q]);
        assert_eq!(decode(&[], UNGATED)?, []);
        Ok(())
    }

    #[test]
    fn a_row_of_the_wrong_width_is_a_shape_error() {
        let said = decode(&[0.0; 7], UNGATED).err().map(|e| e.to_string());
        assert!(
            said.as_deref()
                .is_some_and(|text| text.contains("rows of 5")),
            "{said:?}"
        );
    }

    #[test]
    fn a_score_that_is_not_a_number_is_never_chosen() -> Result<(), AsrError> {
        let logits = [f32::NAN, 1.0, 0.0, 0.0, 0.0];
        assert_eq!(decode(&logits, UNGATED)?, [COMMA]);
        Ok(())
    }
}

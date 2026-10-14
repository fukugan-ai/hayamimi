//! The state of one translation row and how the next event changes it.
//!
//! A row is in one of five states. `Partial` is a translation of what is still
//! being spoken: the part it shares with the previous one is drawn as settled
//! and the rest faint. `First` is the early translation of a line, replaced once
//! by `Confirmed`. `Held` and `Unavailable` show a mark and no text.

use crate::wire::{Annotation, Of, Status, TranslationEvent};

#[derive(Clone, Debug, PartialEq)]
pub enum TrState {
    /// A translation of a partial; `confirmed` characters at the start are settled.
    Partial {
        confirmed: usize,
    },
    First,
    Confirmed,
    /// The translation was withheld; `reason` is the wire word.
    Held {
        reason: Option<String>,
    },
    Unavailable,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrRow {
    /// The language translated into.
    pub lang: String,
    pub text: String,
    pub annotations: Vec<Annotation>,
    pub state: TrState,
}

/// How a piece of a translation is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// Text of the translation.
    Plain,
    /// The part of a partial's translation that is not settled yet.
    Faint,
    /// A converted value written into the translation.
    Note,
}

/// A run of text drawn in one tone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Piece {
    pub text: String,
    pub tone: Tone,
}

/// Length in Unicode scalar values of the longest common prefix of `a` and `b`.
pub fn common_prefix_chars(a: &str, b: &str) -> usize {
    a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count()
}

/// The row a translation of a partial makes, given the row it follows. A
/// translation that is withheld shows nothing, so it makes no row.
pub fn partial_row(event: &TranslationEvent, previous: Option<&TrRow>) -> Option<TrRow> {
    if event.status != Status::Ok {
        return None;
    }
    let before = previous.map_or("", |row| row.text.as_str());
    Some(TrRow {
        lang: event.lang.clone(),
        text: event.text.clone(),
        annotations: event.annotations.clone(),
        state: TrState::Partial {
            confirmed: common_prefix_chars(before, &event.text),
        },
    })
}

/// The row a translation of a final or of a refined paragraph makes, given the
/// row of the same language it follows.
///
/// A draft that arrives after the line was decided changes nothing. A refined
/// paragraph has no draft, so what it gets is its translation.
pub fn settled_row(event: &TranslationEvent, previous: Option<&TrRow>) -> TrRow {
    let draft = event.of == Of::Final && !event.settled && event.status == Status::Ok;
    if let Some(row) = previous
        && draft
        && !matches!(row.state, TrState::First | TrState::Partial { .. })
    {
        return row.clone();
    }
    TrRow {
        lang: event.lang.clone(),
        text: event.text.clone(),
        annotations: event.annotations.clone(),
        state: settled_state(event),
    }
}

fn settled_state(event: &TranslationEvent) -> TrState {
    if event.is_unavailable() {
        TrState::Unavailable
    } else if event.status == Status::Held {
        TrState::Held {
            reason: event.hold_reason.clone(),
        }
    } else if event.of == Of::Final && !event.settled {
        TrState::First
    } else {
        TrState::Confirmed
    }
}

/// Splits a row's text into the runs it is drawn in.
pub fn pieces(row: &TrRow) -> Vec<Piece> {
    match row.state {
        TrState::Partial { confirmed } => split_at_chars(&row.text, confirmed),
        TrState::First | TrState::Confirmed => with_annotations(&row.text, &row.annotations),
        TrState::Held { .. } | TrState::Unavailable => Vec::new(),
    }
}

/// The settled part and the faint rest of a partial's translation.
fn split_at_chars(text: &str, confirmed: usize) -> Vec<Piece> {
    let cut = text
        .char_indices()
        .nth(confirmed)
        .map_or(text.len(), |(at, _)| at);
    let (done, rest) = text.split_at(cut);
    [(done, Tone::Plain), (rest, Tone::Faint)]
        .into_iter()
        .filter(|(part, _)| !part.is_empty())
        .map(|(part, tone)| Piece {
            text: part.to_owned(),
            tone,
        })
        .collect()
}

/// The text with each annotation written after the character at its `end`. An
/// `end` past the last character writes it at the end.
fn with_annotations(text: &str, annotations: &[Annotation]) -> Vec<Piece> {
    let mut ordered: Vec<&Annotation> = annotations.iter().collect();
    ordered.sort_by_key(|note| note.end);
    let mut out = Vec::new();
    let mut from = 0;
    for note in ordered {
        let at = byte_offset(text, note.end).max(from);
        push_plain(&mut out, &text[from..at]);
        out.push(Piece {
            text: note.text.clone(),
            tone: Tone::Note,
        });
        from = at;
    }
    push_plain(&mut out, &text[from..]);
    out
}

fn push_plain(out: &mut Vec<Piece>, part: &str) {
    if !part.is_empty() {
        out.push(Piece {
            text: part.to_owned(),
            tone: Tone::Plain,
        });
    }
}

/// Byte offset of the `n`th scalar value; the length of the text past the last.
fn byte_offset(text: &str, n: usize) -> usize {
    text.char_indices().nth(n).map_or(text.len(), |(at, _)| at)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(of: Of, text: &str, settled: bool) -> TranslationEvent {
        TranslationEvent {
            of,
            id: Some(String::from("mic-1")),
            lang: String::from("en"),
            text: text.to_owned(),
            status: Status::Ok,
            hold_reason: None,
            settled,
            annotations: Vec::new(),
        }
    }

    fn tones(row: &TrRow) -> Vec<(String, Tone)> {
        pieces(row)
            .into_iter()
            .map(|piece| (piece.text, piece.tone))
            .collect()
    }

    #[test]
    fn the_common_prefix_counts_scalar_values() {
        assert_eq!(
            common_prefix_chars("予算は五百万", "予算は五百万円を想定"),
            6
        );
        assert_eq!(common_prefix_chars("", "a"), 0);
        assert_eq!(common_prefix_chars("a", ""), 0);
        assert_eq!(common_prefix_chars("same", "same"), 4);
        assert_eq!(common_prefix_chars("abc", "xbc"), 0);
    }

    #[test]
    fn a_partial_series_settles_the_prefix_shared_with_the_one_before() {
        let first = partial_row(&event(Of::Partial, "予算は五百万", false), None);
        let Some(first) = first else {
            panic!("a translation of a partial made no row");
        };
        assert_eq!(first.state, TrState::Partial { confirmed: 0 });
        let second = partial_row(
            &event(Of::Partial, "予算は五百万円を想定", false),
            Some(&first),
        );
        let Some(second) = second else {
            panic!("a translation of a partial made no row");
        };
        assert_eq!(second.state, TrState::Partial { confirmed: 6 });
        assert_eq!(
            tones(&second),
            [
                (String::from("予算は五百万"), Tone::Plain),
                (String::from("円を想定"), Tone::Faint)
            ]
        );
        let third = partial_row(
            &event(Of::Partial, "予算は五百万円で", false),
            Some(&second),
        );
        let Some(third) = third else {
            panic!("a translation of a partial made no row");
        };
        // Only the one before counts: 7 characters are shared with the second.
        assert_eq!(third.state, TrState::Partial { confirmed: 7 });
    }

    #[test]
    fn a_withheld_translation_of_a_partial_shows_nothing() {
        let mut held = event(Of::Partial, "", false);
        held.status = Status::Held;
        assert_eq!(partial_row(&held, None), None);
    }

    #[test]
    fn a_draft_is_first_and_the_deciding_event_is_confirmed() {
        let first = settled_row(&event(Of::Final, "early", false), None);
        assert_eq!(first.state, TrState::First);
        let decided = settled_row(&event(Of::Final, "better", true), Some(&first));
        assert_eq!(decided.state, TrState::Confirmed);
        assert_eq!(decided.text, "better");
    }

    #[test]
    fn a_draft_that_arrives_after_the_line_was_decided_changes_nothing() {
        let decided = settled_row(&event(Of::Final, "better", true), None);
        let late = settled_row(&event(Of::Final, "early", false), Some(&decided));
        assert_eq!(late, decided);
    }

    #[test]
    fn a_refined_paragraph_is_confirmed_whatever_its_settled_flag() {
        let row = settled_row(&event(Of::Refine, "text", false), None);
        assert_eq!(row.state, TrState::Confirmed);
    }

    #[test]
    fn a_withheld_line_keeps_its_reason_and_an_unreachable_translator_is_unavailable() {
        let mut held = event(Of::Final, "", true);
        held.status = Status::Held;
        held.hold_reason = Some(String::from("number"));
        assert_eq!(
            settled_row(&held, None).state,
            TrState::Held {
                reason: Some(String::from("number"))
            }
        );
        held.hold_reason = Some(String::from("unavailable"));
        assert_eq!(settled_row(&held, None).state, TrState::Unavailable);
    }

    #[test]
    fn an_annotation_is_written_after_the_character_at_its_end() {
        let mut row = settled_row(&event(Of::Final, "It costs $100 today", true), None);
        row.annotations = vec![Annotation {
            end: 13,
            text: String::from(" (about 15000 yen)"),
        }];
        assert_eq!(
            tones(&row),
            [
                (String::from("It costs $100"), Tone::Plain),
                (String::from(" (about 15000 yen)"), Tone::Note),
                (String::from(" today"), Tone::Plain),
            ]
        );
    }

    #[test]
    fn positions_count_scalar_values_not_bytes() {
        let mut row = settled_row(&event(Of::Final, "価格は百ドルです", true), None);
        row.annotations = vec![Annotation {
            end: 6,
            text: String::from("（約1.5万円）"),
        }];
        assert_eq!(tones(&row)[0].0, "価格は百ドル");
        assert_eq!(tones(&row)[1], (String::from("（約1.5万円）"), Tone::Note));
    }

    #[test]
    fn an_end_past_the_last_character_writes_the_note_at_the_end() {
        let mut row = settled_row(&event(Of::Final, "ab", true), None);
        row.annotations = vec![Annotation {
            end: 99,
            text: String::from("N"),
        }];
        assert_eq!(
            tones(&row),
            [
                (String::from("ab"), Tone::Plain),
                (String::from("N"), Tone::Note)
            ]
        );
    }

    #[test]
    fn annotations_are_written_in_the_order_of_their_ends() {
        let mut row = settled_row(&event(Of::Final, "abcd", true), None);
        row.annotations = vec![
            Annotation {
                end: 3,
                text: String::from("Y"),
            },
            Annotation {
                end: 1,
                text: String::from("X"),
            },
        ];
        let text: String = pieces(&row)
            .iter()
            .map(|piece| piece.text.as_str())
            .collect();
        assert_eq!(text, "aXbcYd");
    }

    #[test]
    fn a_held_or_unavailable_row_has_no_pieces() {
        let mut held = event(Of::Final, "", true);
        held.status = Status::Held;
        assert!(pieces(&settled_row(&held, None)).is_empty());
    }
}

//! Parts that more than one page draws: the language and speaker marks, a
//! translation row, a refined paragraph and the fallback mark.

use std::rc::Rc;

use dioxus::prelude::*;

use crate::css::lang_class;
use crate::model::{Fallback, RefineRow};
use crate::strings::{Strings, keys};
use crate::tr::{Tone, TrRow, TrState, pieces};

/// The table of words the page was started with, provided by the root.
pub type Words = Rc<Strings>;

/// The language of a line, in the colour of that language.
#[component]
pub fn LangBadge(lang: String) -> Element {
    let class = format!("lang-badge {}", lang_class(&lang));
    let shown = if lang.is_empty() { "??" } else { lang.as_str() };
    rsx! { span { class: "{class}", "{shown}" } }
}

/// The speaker of a line. The caller draws it only when there is one.
#[component]
pub fn SpeakerBadge(speaker: String) -> Element {
    rsx! { span { class: "spk", "{speaker}" } }
}

/// The sentence that says how many older rows were dropped from a column.
pub fn dropped_text(words: &Strings, count: usize) -> String {
    words.text(keys::DROPPED).replace("{n}", &count.to_string())
}

/// The first line of a column that dropped rows; nothing while none were dropped.
#[component]
pub fn DroppedNote(count: usize) -> Element {
    let words = use_context::<Words>();
    rsx! {
        if count > 0 {
            div { class: "dropped", "{dropped_text(&words, count)}" }
        }
    }
}

/// The class of a piece of a translation on the dashboard and the transcript.
fn piece_class(tone: Tone) -> Option<&'static str> {
    match tone {
        Tone::Plain => None,
        Tone::Faint => Some("tr-rest"),
        Tone::Note => Some("tr-note"),
    }
}

/// A translation row of the dashboard and the transcript: the arrow and the
/// language, then the text in the colour its state gives, or a mark.
#[component]
pub fn TrView(row: TrRow) -> Element {
    let words = use_context::<Words>();
    let class = format!("tr {}", lang_class(&row.lang));
    let text_class = if row.state == TrState::Confirmed {
        "tr-text confirmed"
    } else {
        "tr-text"
    };
    rsx! {
        div { class: "{class}",
            span { class: "tr-arrow", "→{row.lang}" }
            match &row.state {
                TrState::Unavailable => rsx! {
                    span { class: "tr-mark", "{words.text(keys::TR_UNAVAILABLE)}" }
                },
                TrState::Held { reason } => rsx! {
                    span { class: "tr-mark", "{words.text(keys::TR_HELD)}" }
                    if let Some(reason) = reason {
                        span { class: "tr-reason", "{words.word(keys::TR_REASON, reason)}" }
                    }
                },
                _ => rsx! {
                    span { class: "{text_class}",
                        for piece in pieces(&row) {
                            span { class: piece_class(piece.tone), "{piece.text}" }
                        }
                    }
                    if row.state == TrState::First {
                        span { class: "tr-mark", "{words.text(keys::TR_FIRST)}" }
                    }
                },
            }
        }
    }
}

/// A refined paragraph with the translations under it.
#[component]
pub fn RefineParagraph(row: RefineRow) -> Element {
    let class = lang_class(&row.lang);
    let lang = if row.lang.is_empty() {
        "?"
    } else {
        row.lang.as_str()
    };
    rsx! {
        div { class: "refine",
            span { class: "refine-meta",
                if !row.speaker.is_empty() {
                    "{row.speaker} · "
                }
                i { class: "{class}", "{lang}" }
            }
            div { class: "refine-text", "{row.text}" }
            for tr in row.trs.iter() {
                TrView { key: "{tr.lang}", row: tr.clone() }
            }
        }
    }
}

/// `stage → to (reason)`, with the table's words where it has them.
pub fn fallback_label(words: &Strings, found: &Fallback) -> String {
    let stage = words.word(keys::FALLBACK_STAGE, &found.stage);
    let to = words.word(keys::FALLBACK_TO, &found.to);
    match &found.reason {
        Some(reason) => format!(
            "{stage} → {to} ({})",
            words.word(keys::FALLBACK_REASON, reason)
        ),
        None => format!("{stage} → {to}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strings::Locale;

    #[test]
    fn the_dropped_note_puts_the_number_in_the_sentence_of_each_language()
    -> Result<(), crate::strings::StringsError> {
        let ja = Strings::embedded(Locale::Ja)?;
        let en = Strings::embedded(Locale::En)?;
        assert_eq!(dropped_text(&ja, 12), "古い 12 行は表示から外しました");
        assert_eq!(dropped_text(&en, 1), "1 older lines are no longer shown");
        Ok(())
    }

    #[test]
    fn a_fallback_reads_stage_arrow_to_and_the_reason_in_brackets()
    -> Result<(), crate::strings::StringsError> {
        let words = Strings::embedded(Locale::En)?;
        let with = Fallback {
            stage: String::from("translate"),
            reason: Some(String::from("unreachable")),
            to: String::from("cpu"),
        };
        assert_eq!(
            fallback_label(&words, &with),
            "translate → cpu (unreachable)"
        );
        let without = Fallback {
            reason: None,
            ..with
        };
        assert_eq!(fallback_label(&words, &without), "translate → cpu");
        Ok(())
    }

    #[test]
    fn a_piece_gets_a_class_only_when_its_tone_asks_for_one() {
        assert_eq!(piece_class(Tone::Plain), None);
        assert_eq!(piece_class(Tone::Faint), Some("tr-rest"));
        assert_eq!(piece_class(Tone::Note), Some("tr-note"));
    }
}

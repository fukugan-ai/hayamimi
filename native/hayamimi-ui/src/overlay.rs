//! The overlay (`/`): the confirmed line, the line being heard, their
//! translations, and a small mark when a stage runs on a fallback.
//!
//! The page is a browser source for a video mixer, so it has no ground of its
//! own and shows nothing until the first message arrives. `?show=final` and
//! `?show=partial` keep one of the two lines.

use dioxus::prelude::*;

use crate::css;
use crate::model::Model;
use crate::parts::Words;
use crate::strings::keys;
use crate::tr::{Tone, TrRow, pieces};

/// Which of the two lines the page keeps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Show {
    #[default]
    Both,
    Final,
    Partial,
}

impl Show {
    /// Reads the `show` of a query string (`?show=final&x=1`). Any value but
    /// `final` and `partial`, and no value, keep both lines.
    pub fn from_query(search: &str) -> Show {
        let value = search
            .trim_start_matches('?')
            .split('&')
            .find_map(|pair| pair.strip_prefix("show="));
        match value {
            Some("final") => Show::Final,
            Some("partial") => Show::Partial,
            _ => Show::Both,
        }
    }

    /// The inline style that hides a line this setting does not keep.
    fn hides(self, line: Show) -> Option<&'static str> {
        (self != Show::Both && self != line).then_some("display: none")
    }
}

fn piece_class(tone: Tone) -> Option<&'static str> {
    (tone == Tone::Faint).then_some("tr-faint")
}

/// A translation line under a line of text. A withheld translation and one
/// that cannot be made leave the line empty.
#[component]
fn OverlayTr(dom_id: &'static str, row: Option<TrRow>, hidden: Option<&'static str>) -> Element {
    rsx! {
        span { id: "{dom_id}", style: hidden,
            if let Some(row) = row {
                for piece in pieces(&row) {
                    span { class: piece_class(piece.tone), "{piece.text}" }
                }
            }
        }
    }
}

#[component]
fn FallbackBadge(count: usize) -> Element {
    let words = use_context::<Words>();
    rsx! {
        div { id: "fallback-badge",
            "{words.text(keys::FALLBACK_BADGE)}"
            if count >= 2 {
                " {count}"
            }
        }
    }
}

#[component]
pub fn OverlayPage(model: Signal<Model>, show: Show) -> Element {
    let sheet = css::overlay();
    let state = model.read();
    let hide_final = show.hides(Show::Final);
    let hide_partial = show.hides(Show::Partial);
    rsx! {
        style { "{sheet}" }
        if show != Show::Partial && !state.fallbacks.is_empty() {
            FallbackBadge { count: state.fallbacks.len() }
        }
        div { id: "box",
            span { id: "final-line", style: hide_final, "{state.overlay.final_text}" }
            if state.tr_seen {
                OverlayTr { dom_id: "final-tr", row: state.overlay.final_tr.clone(), hidden: hide_final }
            }
            span { id: "partial-line", style: hide_partial, "{state.overlay.partial_text}" }
            if state.tr_seen {
                OverlayTr { dom_id: "partial-tr", row: state.partial_tr.clone(), hidden: hide_partial }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_query_picks_one_line_and_anything_else_keeps_both() {
        assert_eq!(Show::from_query("?show=final"), Show::Final);
        assert_eq!(Show::from_query("?show=partial"), Show::Partial);
        assert_eq!(Show::from_query("?a=1&show=final&b=2"), Show::Final);
        for query in [
            "",
            "?",
            "?show=",
            "?show=x",
            "?show=Final",
            "?other=final",
            "?showy=final",
        ] {
            assert_eq!(Show::from_query(query), Show::Both, "{query}");
        }
    }

    #[test]
    fn a_line_the_setting_does_not_keep_is_hidden() {
        assert_eq!(Show::Final.hides(Show::Partial), Some("display: none"));
        assert_eq!(Show::Partial.hides(Show::Final), Some("display: none"));
        assert_eq!(Show::Final.hides(Show::Final), None);
        assert_eq!(Show::Both.hides(Show::Final), None);
        assert_eq!(Show::Both.hides(Show::Partial), None);
    }

    #[test]
    fn only_the_unsettled_part_of_a_partial_translation_is_faint() {
        assert_eq!(piece_class(Tone::Faint), Some("tr-faint"));
        assert_eq!(piece_class(Tone::Plain), None);
        assert_eq!(piece_class(Tone::Note), None);
    }
}

//! The transcript (`/transcript`): the refined paragraphs and their
//! translations, and nothing else.

use dioxus::prelude::*;

use crate::css;
use crate::model::Model;
use crate::parts::{DroppedNote, RefineParagraph, Words};
use crate::scroll::{self, use_follow};
use crate::strings::keys;

const TRANSCRIPT_ID: &str = "transcript";

#[component]
pub fn TranscriptPage(model: Signal<Model>) -> Element {
    let words = use_context::<Words>();
    let sheet = css::transcript();
    let mut following = use_follow(TRANSCRIPT_ID, move || model.read().refine_total());
    let state = model.read();
    let first = state.dropped_refines;
    rsx! {
        style { "{sheet}" }
        div {
            id: TRANSCRIPT_ID,
            class: "transcript scroll",
            role: "log",
            aria_live: "polite",
            onscroll: move |_| following.set(scroll::near_end(TRANSCRIPT_ID)),
            div { class: "transcript-body",
                DroppedNote { count: first }
                if state.refines.is_empty() {
                    div { class: "empty", "{words.text(keys::EMPTY_REFINED)}" }
                }
                for (number, row) in state.refines.iter().enumerate().map(|(at, row)| (first + at, row)) {
                    RefineParagraph { key: "{number}", row: row.clone() }
                }
            }
        }
        if !*following.read() {
            button { class: "latest", onclick: move |_| following.set(true), "{words.text(keys::JUMP_LATEST)}" }
        }
    }
}

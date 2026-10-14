//! The dashboard (`/dashboard`): the strip of what is being heard, the feed of
//! confirmed lines, the column of refined paragraphs, and the fallback band.

use dioxus::prelude::*;

use crate::css;
use crate::model::{Conn, FinalRow, Model};
use crate::parts::{LangBadge, RefineParagraph, SpeakerBadge, TrView, Words, fallback_label};
use crate::scroll::{self, use_follow};
use crate::strings::keys;

const FEED_ID: &str = "feed";
const REFINED_ID: &str = "refined";

/// The addresses the footer offers for a browser source and the transcript.
pub const LINKS: [&str; 4] = ["/", "/?show=final", "/?show=partial", "/transcript"];

/// What a header chip shows for a value that does not exist yet.
fn or_dash(value: Option<String>) -> String {
    // DEFAULT: a chip with no value yet shows a dash, as the dashboard has always drawn it.
    value.unwrap_or_else(|| String::from("–"))
}

/// The latency of a line as the feed writes it: whole milliseconds.
pub fn latency_text(latency_ms: Option<f64>) -> String {
    latency_ms.map_or_else(String::new, |ms| format!("{:.0}ms", ms.round()))
}

#[component]
fn Chip(label: String, value: String, unit: Option<&'static str>) -> Element {
    rsx! {
        div { class: "chip",
            "{label} "
            b { "{value}" }
            if let Some(unit) = unit {
                "{unit}"
            }
        }
    }
}

#[component]
fn Header(model: Signal<Model>) -> Element {
    let words = use_context::<Words>();
    let state = model.read();
    let dot = if state.conn == Conn::Connected {
        "dot on"
    } else {
        "dot"
    };
    let mean = or_dash(state.stats.mean_latency_ms().map(|ms| ms.to_string()));
    let langs = or_dash((!state.stats.langs.is_empty()).then(|| state.stats.langs.join(" ")));
    rsx! {
        header { class: "top",
            div { class: "{dot}" }
            div { class: "logotype",
                "早"
                em { "耳" }
            }
            div { class: "romaji", "hayamimi" }
            div { class: "spacer" }
            Chip { label: words.text(keys::CHIP_FINALS).to_owned(), value: state.stats.finals.to_string() }
            Chip { label: words.text(keys::CHIP_LATENCY).to_owned(), value: mean, unit: " ms" }
            Chip { label: words.text(keys::CHIP_LANGS).to_owned(), value: langs }
        }
    }
}

#[component]
fn FallbackBand(model: Signal<Model>) -> Element {
    let words = use_context::<Words>();
    let state = model.read();
    if state.fallbacks.is_empty() {
        return rsx! {};
    }
    rsx! {
        div { class: "fb-band", role: "status",
            span { class: "fb-label", "{words.text(keys::FALLBACK_BADGE)}" }
            div { class: "fb-chips",
                for found in state.fallbacks.iter() {
                    span { key: "{found.stage}", class: "fb-chip", "{fallback_label(&words, found)}" }
                }
            }
        }
    }
}

#[component]
fn Strip(model: Signal<Model>) -> Element {
    let words = use_context::<Words>();
    let state = model.read();
    let waiting = state.strip_text.is_empty();
    let class = if waiting || state.strip_quiet {
        "strip quiet"
    } else {
        "strip"
    };
    let shown = if waiting {
        words.text(keys::WAITING)
    } else {
        state.strip_text.as_str()
    };
    rsx! {
        div { class: "{class}", aria_live: "off",
            div { class: "strip-label", "{words.text(keys::LISTENING)}" }
            div { class: "strip-text",
                "{shown}"
                span { class: "caret" }
            }
            if let Some(row) = state.partial_tr.clone() {
                TrView { row }
            }
        }
    }
}

#[component]
fn FeedRow(row: FinalRow) -> Element {
    rsx! {
        div { class: "row",
            div { class: "badges",
                LangBadge { lang: row.lang.clone() }
                if !row.speaker.is_empty() {
                    SpeakerBadge { speaker: row.speaker.clone() }
                }
            }
            div { class: "row-text", "{row.text}" }
            div { class: "lat", "{latency_text(row.latency_ms)}" }
            for tr in row.trs.iter() {
                TrView { key: "{tr.lang}", row: tr.clone() }
            }
        }
    }
}

/// The button that takes a reader who scrolled up back to the end.
#[component]
fn LatestButton(following: Signal<bool>) -> Element {
    let words = use_context::<Words>();
    let mut following = following;
    rsx! {
        if !*following.read() {
            button { class: "latest", onclick: move |_| following.set(true), "{words.text(keys::JUMP_LATEST)}" }
        }
    }
}

#[component]
fn FeedColumn(model: Signal<Model>) -> Element {
    let words = use_context::<Words>();
    let mut following = use_follow(FEED_ID, move || model.read().finals.len());
    let rows = model.read().finals.clone();
    rsx! {
        section { class: "col",
            h2 {
                "{words.text(keys::FEED)}"
                span { "LIVE" }
            }
            div {
                id: FEED_ID,
                class: "scroll",
                role: "log",
                aria_live: "polite",
                onscroll: move |_| following.set(scroll::near_end(FEED_ID)),
                if rows.is_empty() {
                    div { class: "empty", "{words.text(keys::EMPTY_FEED)}" }
                }
                for (at, row) in rows.into_iter().enumerate() {
                    FeedRow { key: "{at}", row }
                }
            }
            LatestButton { following }
        }
    }
}

#[component]
fn RefinedColumn(model: Signal<Model>) -> Element {
    let words = use_context::<Words>();
    let mut following = use_follow(REFINED_ID, move || model.read().refines.len());
    let rows = model.read().refines.clone();
    rsx! {
        section { class: "col",
            h2 {
                "{words.text(keys::REFINED)}"
                span { "REFINED" }
            }
            div {
                id: REFINED_ID,
                class: "scroll",
                role: "log",
                aria_live: "polite",
                onscroll: move |_| following.set(scroll::near_end(REFINED_ID)),
                if rows.is_empty() {
                    div { class: "empty", "{words.text(keys::EMPTY_REFINED)}" }
                }
                for (at, row) in rows.into_iter().enumerate() {
                    RefineParagraph { key: "{at}", row }
                }
            }
            LatestButton { following }
        }
    }
}

#[component]
fn Footer(model: Signal<Model>) -> Element {
    let words = use_context::<Words>();
    let conn = match model.read().conn {
        Conn::Connecting => keys::CONNECTING,
        Conn::Connected => keys::CONNECTED,
        Conn::Reconnecting => keys::RECONNECTING,
    };
    rsx! {
        footer { class: "foot",
            span { "{words.text(conn)}" }
            for link in LINKS {
                a { key: "{link}", href: "{link}", "{link}" }
            }
        }
    }
}

#[component]
pub fn DashboardPage(model: Signal<Model>) -> Element {
    let sheet = css::dashboard();
    rsx! {
        style { "{sheet}" }
        div { class: "dash",
            Header { model }
            FallbackBand { model }
            Strip { model }
            div { class: "main",
                FeedColumn { model }
                RefinedColumn { model }
            }
            Footer { model }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_latency_is_written_in_whole_milliseconds_and_a_missing_one_is_blank() {
        assert_eq!(latency_text(Some(827.4)), "827ms");
        assert_eq!(latency_text(Some(402.5)), "403ms");
        assert_eq!(latency_text(None), "");
    }

    #[test]
    fn a_chip_without_a_value_shows_a_dash() {
        assert_eq!(or_dash(None), "–");
        assert_eq!(or_dash(Some(String::from("370"))), "370");
    }

    #[test]
    fn the_footer_offers_the_four_addresses_of_the_server() {
        assert_eq!(
            LINKS,
            ["/", "/?show=final", "/?show=partial", "/transcript"]
        );
    }
}

//! The root of the bundle: one state, its streams, and the page the address names.

use std::rc::Rc;

use dioxus::prelude::*;

use crate::dashboard::DashboardPage;
use crate::model::Model;
use crate::overlay::OverlayPage;
use crate::parts::Words;
use crate::route::Page;
use crate::strings::{Locale, Strings};
use crate::transcript::TranscriptPage;
use crate::web::{self, EventStream};

/// The stream every page reads, on the server that served the page.
const EVENTS_PATH: &str = "/events";

#[component]
pub fn App() -> Element {
    use_context_provider(|| -> Words { Rc::new(embedded_words(Locale::detect())) });
    let model = use_signal(Model::default);
    let _stream = use_hook(|| Rc::new(open_stream(model)));
    let _relay = use_hook(|| Rc::new(open_relay(model)));
    web::use_deadline_timer(model);
    match web::current_page() {
        Page::Overlay(show) => rsx! { OverlayPage { model, show } },
        Page::Dashboard => rsx! { DashboardPage { model } },
        Page::Transcript => rsx! { TranscriptPage { model } },
    }
}

/// The compiled-in table of a locale. A test reads both tables, so a failure
/// is not expected; it is named on the console, and the table is then empty
/// and every word shows as its key, which names what is missing.
fn embedded_words(locale: Locale) -> Strings {
    match Strings::embedded(locale) {
        Ok(strings) => strings,
        Err(error) => {
            web_sys::console::warn_2(&"ui.strings.unreadable".into(), &error.to_string().into());
            Strings::default()
        }
    }
}

/// Opens the stream; one that cannot be opened is named on the console and the
/// page stays on its first screen.
fn open_stream(model: Signal<Model>) -> Option<EventStream> {
    match EventStream::open(EVENTS_PATH, model) {
        Ok(stream) => Some(stream),
        Err(error) => {
            web_sys::console::warn_2(&"ui.events.unavailable".into(), &error);
            None
        }
    }
}

/// Opens the relay stream the address names (`?relay=<url>`), the translations
/// of the lines of the first stream. A page without one opens nothing; one that
/// cannot be opened is named on the console and the page shows no translation.
fn open_relay(model: Signal<Model>) -> Option<EventStream> {
    let url = web::relay_address()?;
    match EventStream::open_relay(&url, model) {
        Ok(stream) => Some(stream),
        Err(error) => {
            web_sys::console::warn_2(&"ui.relay.unavailable".into(), &error);
            None
        }
    }
}

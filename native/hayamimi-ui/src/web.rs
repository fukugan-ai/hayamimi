//! The part that talks to the browser: the `/events` stream, the clock and the
//! timer that makes `Model::expire` run when `Model::next_deadline` comes.

use std::cell::RefCell;
use std::rc::Rc;

use dioxus::prelude::*;
use gloo_timers::callback::Timeout;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{EventSource, MessageEvent, console};

use crate::model::{Conn, Model};
use crate::overlay::Show;
use crate::route::{Page, page_for, relay_url};
use crate::wire;

/// Milliseconds since the epoch, on the browser's clock.
pub fn now_ms() -> u64 {
    js_sys::Date::now() as u64
}

/// A browser callback that takes no argument.
type Callback = Closure<dyn FnMut()>;

/// An open connection to an `/events` stream. The browser connects again by
/// itself when the connection breaks. Dropping this closes it.
pub struct EventStream {
    source: EventSource,
    /// Present on the stream of the server that served the page, the only one
    /// that writes the connection mark of the model.
    _on_open: Option<Callback>,
    _on_error: Option<Callback>,
    _on_message: Closure<dyn FnMut(MessageEvent)>,
}

impl EventStream {
    /// Opens `path` on the server that served the page, and writes what
    /// arrives into `model`, with the connection mark. A message the pages
    /// cannot read is left out and named on the console.
    pub fn open(path: &str, model: Signal<Model>) -> Result<EventStream, wasm_bindgen::JsValue> {
        Self::connect(path, model, true)
    }

    /// Opens the `/events` of the relay at `url` and writes what arrives into
    /// `model`. It leaves the connection mark alone, so a relay that is down
    /// does not change what the page says about its own server.
    pub fn open_relay(
        url: &str,
        model: Signal<Model>,
    ) -> Result<EventStream, wasm_bindgen::JsValue> {
        Self::connect(url, model, false)
    }

    fn connect(
        address: &str,
        model: Signal<Model>,
        marks_connection: bool,
    ) -> Result<EventStream, wasm_bindgen::JsValue> {
        let source = EventSource::new(address)?;
        let (on_open, on_error) = if marks_connection {
            let (on_open, on_error) = connection_marks(model);
            source.set_onopen(Some(on_open.as_ref().unchecked_ref()));
            source.set_onerror(Some(on_error.as_ref().unchecked_ref()));
            (Some(on_open), Some(on_error))
        } else {
            (None, None)
        };
        let on_message = message_writer(model);
        source.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        Ok(EventStream {
            source,
            _on_open: on_open,
            _on_error: on_error,
            _on_message: on_message,
        })
    }
}

/// The handlers that write the connection mark of the model.
fn connection_marks(model: Signal<Model>) -> (Callback, Callback) {
    let mut on_open_model = model;
    let on_open = Callback::new(move || on_open_model.write().set_conn(Conn::Connected));
    let mut on_error_model = model;
    let on_error = Callback::new(move || on_error_model.write().set_conn(Conn::Reconnecting));
    (on_open, on_error)
}

/// The handler that writes each message into the model.
fn message_writer(model: Signal<Model>) -> Closure<dyn FnMut(MessageEvent)> {
    let mut on_message_model = model;
    Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
        let Some(data) = event.data().as_string() else {
            return;
        };
        match wire::parse(&data) {
            Ok(found) => on_message_model.write().apply(found, now_ms()),
            Err(error) => console::warn_2(&"ui.event.rejected".into(), &error.to_string().into()),
        }
    })
}

impl Drop for EventStream {
    fn drop(&mut self) {
        self.source.close();
    }
}

/// The page the address bar names.
pub fn current_page() -> Page {
    let location = web_sys::window().map(|window| window.location());
    let Some((Ok(path), Ok(search))) = location.map(|at| (at.pathname(), at.search())) else {
        // DEFAULT: an address that cannot be read is the overlay, which is the page for `/`.
        return Page::Overlay(Show::Both);
    };
    page_for(&path, &search)
}

/// The relay stream the address bar names (`?relay=<url>`), if it names one.
pub fn relay_address() -> Option<String> {
    let search = web_sys::window()?.location().search().ok()?;
    relay_url(&search)
}

/// A timer for the model's next deadline: each change of the model replaces
/// it, and it runs `Model::expire` when it fires.
pub fn use_deadline_timer(model: Signal<Model>) {
    let slot = use_hook(|| Rc::new(RefCell::new(None::<Timeout>)));
    use_effect(move || {
        let due = model.read().next_deadline();
        *slot.borrow_mut() = due.and_then(|at| arm(model, at));
    });
}

fn arm(model: Signal<Model>, at: u64) -> Option<Timeout> {
    let wait = u32::try_from(at.saturating_sub(now_ms())).ok()?;
    let mut state = model;
    Some(Timeout::new(wait, move || state.write().expire(now_ms())))
}

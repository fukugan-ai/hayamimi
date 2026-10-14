//! Following the end of a list that grows.
//!
//! A list follows its end while the reader is within `NEAR_END_PX` of it. Once
//! the reader scrolls further up, new rows stop moving the list and a button
//! offers the way back. Outside a browser there is no list to scroll, so both
//! functions do nothing.

use dioxus::prelude::*;

/// How far from the end counts as being at the end.
pub const NEAR_END_PX: f64 = 48.0;

#[cfg(feature = "web")]
mod platform {
    use super::NEAR_END_PX;

    fn element(id: &str) -> Option<web_sys::Element> {
        web_sys::window()?.document()?.get_element_by_id(id)
    }

    /// Whether the list `id` is scrolled to within `NEAR_END_PX` of its end.
    pub fn near_end(id: &str) -> bool {
        let Some(list) = element(id) else {
            // DEFAULT: a list that is not on the page has nothing above its end to read.
            return true;
        };
        let left = list.scroll_height() - list.scroll_top() - list.client_height();
        f64::from(left) <= NEAR_END_PX
    }

    /// Scrolls the list `id` to its end.
    pub fn to_end(id: &str) {
        if let Some(list) = element(id) {
            list.set_scroll_top(list.scroll_height());
        }
    }
}

#[cfg(not(feature = "web"))]
mod platform {
    pub fn near_end(_id: &str) -> bool {
        true
    }

    pub fn to_end(_id: &str) {}
}

pub use platform::{near_end, to_end};

/// Whether the list `id` follows its end. `count` reads the number of rows, so
/// that the list is moved after each new row once the page has drawn it.
pub fn use_follow(id: &'static str, count: impl Fn() -> usize + 'static) -> Signal<bool> {
    let following = use_signal(|| true);
    use_effect(move || {
        let _rows = count();
        if *following.read() {
            to_end(id);
        }
    });
    following
}

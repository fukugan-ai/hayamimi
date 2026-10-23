//! The three pages of hayamimi, drawn with Dioxus and built for WebAssembly.
//!
//! - `/` is the overlay for a video mixer's browser source;
//! - `/dashboard` is the page of the person running the subtitles;
//! - `/transcript` is the refined text for reading.
//!
//! All three read the `/events` stream of the server that serves them, and a
//! page opened with `?relay=<url>` reads the `/events` of that relay as well,
//! for the translations of the lines (`route::relay_url`). `wire`
//! turns one message into a `UiEvent`, `model` is the state the pages share, and
//! the page modules only draw it. With the `web` feature the crate also holds
//! the part that talks to the browser (`app`); without it the pages render to a
//! string, which is how they are tested.
//!
//! `lanes` is not a page of the bundle: it picks the lines of the two-lane
//! subtitle band of the desktop overlay window out of the same `Model`, and
//! draws them.
//!
//! # Building and serving the bundle
//!
//! ```text
//! cargo build --release --target wasm32-unknown-unknown -p hayamimi-ui --features web
//! wasm-bindgen --target web --no-typescript --out-dir <bundle> --out-name hayamimi-ui \
//!     target/wasm32-unknown-unknown/release/hayamimi-ui.wasm
//! ```
//!
//! The server answers `/`, `/dashboard` and `/transcript` with
//! `web/index.html`, and `/ui/<path>` with the files of `<bundle>`, `snippets/`
//! included. The pages open the same-origin `EVENTS_PATH` (`app.rs`), so the
//! server that serves them is the one they listen to. `examples/preview.rs`
//! does all of this with a scripted session:
//!
//! ```text
//! cargo run --release -p hayamimi-ui --example preview -- <bundle> <port>
//! ```

#![forbid(unsafe_code)]

pub mod css;
pub mod dashboard;
pub mod lanes;
pub mod model;
pub mod overlay;
pub mod parts;
pub mod route;
pub mod scroll;
pub mod strings;
pub mod tr;
pub mod transcript;
pub mod wire;

#[cfg(feature = "web")]
mod app;
#[cfg(feature = "web")]
mod web;

#[cfg(feature = "web")]
pub use app::App;

//! The subtitle window of hayamimi: a borderless, transparent, always-on-top
//! band along the bottom of the screen that lets the mouse through.
//!
//! Three `/events` streams (the two recognizers' finals, the relay's
//! translations) are read on native threads (`feed`), joined into one `Model`
//! by `hayamimi-ui`, and drawn by its `LanesPage` in a Dioxus desktop window
//! (`app`, `window`). Where the band sits is one formula (`place`) over the
//! work area and the settings file (`config`), which is read again while the
//! window runs. The Win32 calls that tao has no door for live in `win`, the
//! one file that holds `unsafe`.
//!
//! The window and its webview exist on Windows only. The parts that decide
//! things (`place`, `config`, `log`, `feed`, `fonts`) do not depend on the
//! operating system and are tested on Linux as well.

#![deny(unsafe_code)]

pub mod config;
pub mod feed;
pub mod fonts;
pub mod log;
pub mod place;
pub mod probe;
pub mod rendered;

#[cfg(windows)]
pub mod app;
#[cfg(windows)]
pub mod win;
#[cfg(windows)]
pub mod window;

/// The class name the window is created with; a test finds the window by it.
pub const WINDOW_CLASS: &str = "hayamimi-overlay";

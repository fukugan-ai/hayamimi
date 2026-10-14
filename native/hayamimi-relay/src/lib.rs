//! The translation relay of hayamimi.
//!
//! A line of recognized speech goes through three stages (translate, edit,
//! verify) and then a unit annotation. Verification and annotation are rules
//! and tables, never a language model: a translation whose numbers, negation,
//! hedging or proper names do not match the source is withheld, and a
//! converted value is added only when it can be placed with certainty.
//!
//! The relay follows the `/events` streams of the recognizers (`follow`),
//! translates each line in a lane of its own source (`relay`) and serves the
//! translations (`hub`). A translation of T that passed verification is served
//! at once as a draft and replaced by the settled one; only the settled one is
//! sent to a subtitle file and to the outside destinations (`deliver`, `srt`).
//! The destinations are the Zoom caption API and the Teams CART link
//! (`captions`), whose addresses come from a file that is read again when it
//! changes and only while it is private (`send_config`, `access`).
//! The text of an utterance still in progress is translated again each time
//! it grows (`partial`); that translation is for the screens only.
//! The same translation makes the subtitle files of a video (`video`, with
//! `ffmpeg` and the recognizer run by `media`, which feeds the recognizer's
//! `/ingest` through `ws_client` and reads the samples with `pcm`).
//! The crate also holds the daily exchange-rate fetch and its scheduler
//! registration. Table and rate files are re-read when they change.
//!
//! `eval` translates the Japanese side of sentence pairs into the languages of
//! `--translate` (`target`) and writes them for a scorer.

pub mod access;
pub mod annotate;
pub mod calendar;
pub mod captions;
pub mod cli;
pub mod deliver;
pub mod eval;
pub mod follow;
pub mod fx;
pub mod hub;
pub mod lang;
pub mod line;
pub mod llm;
pub mod logging;
pub mod media;
pub mod numbers;
pub mod partial;
pub mod pcm;
pub mod pipeline;
pub mod reach;
pub mod relay;
pub mod schedule;
pub mod send_config;
pub mod spelled;
pub mod srt;
pub mod srt_command;
pub mod strings;
pub mod style;
pub mod target;
pub mod units;
pub mod verify;
pub mod video;
pub mod watched;
pub mod ws_client;

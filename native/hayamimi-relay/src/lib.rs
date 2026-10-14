//! The translation relay of hayamimi.
//!
//! A line of recognized speech goes through three stages (translate, edit,
//! verify) and then a unit annotation. Verification and annotation are rules
//! and tables, never a language model: a translation whose numbers, negation,
//! hedging or proper names do not match the source is withheld, and a
//! converted value is added only when it can be placed with certainty.
//!
//! The crate also holds the daily exchange-rate fetch and its scheduler
//! registration. Table and rate files are re-read when they change.

pub mod annotate;
pub mod calendar;
pub mod cli;
pub mod fx;
pub mod lang;
pub mod llm;
pub mod logging;
pub mod numbers;
pub mod pipeline;
pub mod reach;
pub mod schedule;
pub mod strings;
pub mod style;
pub mod units;
pub mod verify;
pub mod watched;

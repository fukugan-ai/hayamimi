//! RP-1: the scorecard, its per-language summary, its reading from the Python
//! run, and the comparison of a Rust run against the Python baseline.
//!
//! Errors are kept in millionths so that "not worse" is an integer compare:
//! `0.038` is `38_000`.

use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::EvalError;

/// The languages of the scorecard, in the order its table lists them.
pub const LANGS: [&str; 5] = ["ja", "en", "zh", "ko", "yue"];

/// The mark a Python run leaves in its table when OpenCC was missing.
const WITHOUT_OPENCC: &str = "opencc無し";

/// How the language of each clip reached the recognizer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lid {
    /// A language detector chose it: the number of right choices is a measurement.
    Detected,
    /// The manifest's true language was used: no detector ran, so nothing was measured.
    Oracle,
}

/// One clip of a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipRow {
    /// The wav file name, which tells the clip apart in a listing.
    pub name: String,
    /// What the engine wrote for the clip, after the digit pass.
    pub text: String,
    pub lang: String,
    /// The language the detector chose; `None` when none ran.
    pub detected: Option<String>,
    pub tier: String,
    pub err_micros: u64,
    pub dur_ms: u64,
    pub rtf_micros: u64,
}

/// One language of the scorecard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LangRow {
    pub lang: String,
    pub clips: u32,
    /// Clips whose detected language was the true one; `None` when no detector ran.
    pub lid_correct: Option<u32>,
    /// The tiers that answered, the most used first, joined by `+`.
    pub tiers: String,
    /// The error of each clip weighted by its duration, in millionths.
    pub mean_err_micros: u64,
    pub mean_rtf_micros: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scorecard {
    pub engine: String,
    pub lid: Lid,
    pub rows: Vec<LangRow>,
    /// The `model.fallback.*` events the run raised, once each.
    pub fallbacks: Vec<String>,
}

/// Why a run is not accepted against the baseline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScoreError {
    /// The run raised a fallback event, so it did not measure the default route.
    Fallback(String),
    MissingLanguage(String),
    Worse {
        lang: String,
        python_micros: u64,
        rust_micros: u64,
    },
    /// The run did not detect languages, so its count of right detections is not a measurement.
    LidNotMeasured(String),
    LidWorse {
        lang: String,
        python: u32,
        rust: u32,
    },
}

impl fmt::Display for ScoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScoreError::Fallback(event) => write!(f, "the run raised {event}"),
            ScoreError::MissingLanguage(lang) => write!(f, "{lang} has no row"),
            ScoreError::Worse {
                lang,
                python_micros,
                rust_micros,
            } => write!(
                f,
                "{lang}: error {rust_micros} is worse than the baseline {python_micros} (millionths)"
            ),
            ScoreError::LidNotMeasured(lang) => {
                write!(f, "{lang}: the run did not detect languages")
            }
            ScoreError::LidWorse { lang, python, rust } => {
                write!(f, "{lang}: {rust} right detections, baseline {python}")
            }
        }
    }
}

impl std::error::Error for ScoreError {}

/// The mean of the clips' errors weighted by their duration, in millionths.
fn weighted_error(clips: &[&ClipRow], lang: &str) -> Result<u64, EvalError> {
    let total_ms: u64 = clips.iter().map(|clip| clip.dur_ms).sum();
    if total_ms == 0 {
        return Err(EvalError::Invalid(format!("{lang}: the clips last 0 ms")));
    }
    let weighted: u64 = clips.iter().map(|clip| clip.err_micros * clip.dur_ms).sum();
    Ok(weighted / total_ms)
}

/// The tiers of the clips, the most used first, ties in name order.
fn tiers_of(clips: &[&ClipRow]) -> String {
    let mut counted: Vec<(&str, usize)> = Vec::new();
    for clip in clips {
        match counted.iter_mut().find(|(tier, _)| *tier == clip.tier) {
            Some(entry) => entry.1 += 1,
            None => counted.push((clip.tier.as_str(), 1)),
        }
    }
    counted.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    counted
        .iter()
        .map(|(tier, _)| *tier)
        .collect::<Vec<_>>()
        .join("+")
}

/// The right detections, or `None` when any clip of the language had no detector.
fn lid_correct_of(clips: &[&ClipRow], lang: &str) -> Result<Option<u32>, EvalError> {
    if clips.iter().any(|clip| clip.detected.is_none()) {
        return Ok(None);
    }
    let right = clips
        .iter()
        .filter(|clip| clip.detected.as_deref() == Some(lang))
        .count();
    u32::try_from(right)
        .map(Some)
        .map_err(|_| EvalError::Invalid(format!("{lang}: too many clips")))
}

fn lang_row(lang: &str, clips: &[&ClipRow]) -> Result<LangRow, EvalError> {
    let count = u32::try_from(clips.len())
        .map_err(|_| EvalError::Invalid(format!("{lang}: too many clips")))?;
    let rtf_sum: u64 = clips.iter().map(|clip| clip.rtf_micros).sum();
    Ok(LangRow {
        lang: lang.to_owned(),
        clips: count,
        lid_correct: lid_correct_of(clips, lang)?,
        tiers: tiers_of(clips),
        mean_err_micros: weighted_error(clips, lang)?,
        mean_rtf_micros: rtf_sum / u64::from(count),
    })
}

/// The per-language rows of a run, in the order of [`LANGS`]; a language with no clip has none.
pub fn summarize(clips: &[ClipRow]) -> Result<Vec<LangRow>, EvalError> {
    let mut rows = Vec::new();
    for lang in LANGS {
        let of_lang: Vec<&ClipRow> = clips.iter().filter(|clip| clip.lang == lang).collect();
        if !of_lang.is_empty() {
            rows.push(lang_row(lang, &of_lang)?);
        }
    }
    Ok(rows)
}

fn row_of<'a>(card: &'a Scorecard, lang: &str) -> Result<&'a LangRow, ScoreError> {
    card.rows
        .iter()
        .find(|row| row.lang == lang)
        .ok_or_else(|| ScoreError::MissingLanguage(lang.to_owned()))
}

/// The step of the Python table: its errors are recorded to 3 decimals.
const TABLE_STEP_MICROS: u64 = 1_000;

/// An error rounded half-up to the 3 decimals of the Python table, so that it is
/// compared with a value recorded at the same precision.
fn at_table_places(micros: u64) -> u64 {
    (micros + TABLE_STEP_MICROS / 2) / TABLE_STEP_MICROS * TABLE_STEP_MICROS
}

/// One language: the error no worse than the baseline's, and no fewer right detections.
fn check_language(python: &LangRow, rust: &LangRow) -> Result<(), ScoreError> {
    if at_table_places(rust.mean_err_micros) > python.mean_err_micros {
        return Err(ScoreError::Worse {
            lang: python.lang.clone(),
            python_micros: python.mean_err_micros,
            rust_micros: rust.mean_err_micros,
        });
    }
    let Some(rust_right) = rust.lid_correct else {
        return Err(ScoreError::LidNotMeasured(python.lang.clone()));
    };
    match python.lid_correct {
        Some(python_right) if rust_right < python_right => Err(ScoreError::LidWorse {
            lang: python.lang.clone(),
            python: python_right,
            rust: rust_right,
        }),
        _ => Ok(()),
    }
}

/// A run is accepted when it raised no fallback event and, in every language
/// of the baseline, its error is no worse and it detected the language as
/// often.
pub fn check_scorecard(python: &Scorecard, rust: &Scorecard) -> Result<(), ScoreError> {
    if let Some(event) = rust.fallbacks.first() {
        return Err(ScoreError::Fallback(event.clone()));
    }
    for row in &python.rows {
        check_language(row, row_of(rust, &row.lang)?)?;
    }
    Ok(())
}

/// A decimal such as `0.038` in millionths.
fn parse_micros(text: &str) -> Result<u64, EvalError> {
    let bad = || EvalError::Invalid(format!("`{text}` is not a decimal with at most six places"));
    let (whole, fraction) = text.trim().split_once('.').ok_or_else(bad)?;
    if fraction.len() > 6 {
        return Err(bad());
    }
    let whole: u64 = whole.parse().map_err(|_| bad())?;
    let fraction: u64 = format!("{fraction:0<6}").parse().map_err(|_| bad())?;
    Ok(whole * 1_000_000 + fraction)
}

/// `15/15` is 15 right detections.
fn parse_lid(text: &str) -> Result<u32, EvalError> {
    let bad = || EvalError::Invalid(format!("`{text}` is not a count of right detections"));
    let (right, _) = text.trim().split_once('/').ok_or_else(bad)?;
    right.trim().parse().map_err(|_| bad())
}

/// One row of the Python table: `| ja | 15 | 15/15 | rz | 0.038 | 0.090 |`.
fn parse_row(cells: &[&str]) -> Result<LangRow, EvalError> {
    let clips = cells[1]
        .parse()
        .map_err(|_| EvalError::Invalid(format!("`{}` is not a clip count", cells[1])))?;
    Ok(LangRow {
        lang: cells[0].to_owned(),
        clips,
        lid_correct: Some(parse_lid(cells[2])?),
        tiers: cells[3].to_owned(),
        mean_err_micros: parse_micros(cells[4])?,
        mean_rtf_micros: parse_micros(cells[5])?,
    })
}

/// The language rows of the table in a Python scorecard document.
pub fn parse_markdown(text: &str) -> Result<Vec<LangRow>, EvalError> {
    if text.contains(WITHOUT_OPENCC) {
        return Err(EvalError::Invalid(format!(
            "{WITHOUT_OPENCC}: the yue figure was made without OpenCC"
        )));
    }
    let mut rows = Vec::new();
    for line in text.lines().filter(|line| line.starts_with('|')) {
        let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
        if cells.len() == 6 && LANGS.contains(&cells[0]) {
            rows.push(parse_row(&cells)?);
        }
    }
    Ok(rows)
}

/// The mean of the same language over the runs; the most right detections.
fn merge_language(lang: &str, runs: &[Vec<LangRow>]) -> Result<LangRow, EvalError> {
    let found: Vec<&LangRow> = runs
        .iter()
        .map(|rows| rows.iter().find(|row| row.lang == lang))
        .collect::<Option<_>>()
        .ok_or_else(|| EvalError::Invalid(format!("{lang} is missing from a run")))?;
    let count = found.len() as u64;
    let mut merged = found[0].clone();
    merged.mean_err_micros = found.iter().map(|row| row.mean_err_micros).sum::<u64>() / count;
    merged.mean_rtf_micros = found.iter().map(|row| row.mean_rtf_micros).sum::<u64>() / count;
    merged.lid_correct = found.iter().map(|row| row.lid_correct).max().flatten();
    Ok(merged)
}

/// The baseline: the mean error of each language over the Python runs, and
/// the most right detections any of them made.
pub fn python_baseline(documents: &[&str]) -> Result<Scorecard, EvalError> {
    let runs = documents
        .iter()
        .map(|text| parse_markdown(text))
        .collect::<Result<Vec<_>, _>>()?;
    let Some(first) = runs.first() else {
        return Err(EvalError::Invalid(String::from("no Python run was given")));
    };
    let rows = first
        .iter()
        .map(|row| merge_language(&row.lang, &runs))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Scorecard {
        engine: String::from("python"),
        lid: Lid::Detected,
        rows,
        fallbacks: Vec::new(),
    })
}

pub fn write_json(path: &Path, card: &Scorecard) -> Result<(), EvalError> {
    let text =
        serde_json::to_string_pretty(card).map_err(|source| EvalError::json(path, source))?;
    std::fs::write(path, text + "\n").map_err(|source| EvalError::io(path, source))
}

pub fn read_json(path: &Path) -> Result<Scorecard, EvalError> {
    let text = std::fs::read_to_string(path).map_err(|source| EvalError::io(path, source))?;
    serde_json::from_str(&text).map_err(|source| EvalError::json(path, source))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(lang: &str, detected: Option<&str>, err: u64, dur_ms: u64) -> ClipRow {
        ClipRow {
            name: String::from("a.wav"),
            text: String::new(),
            lang: lang.to_owned(),
            detected: detected.map(str::to_owned),
            tier: String::from("rz"),
            err_micros: err,
            dur_ms,
            rtf_micros: 100_000,
        }
    }

    fn row(lang: &str, err: u64, lid: Option<u32>) -> LangRow {
        LangRow {
            lang: lang.to_owned(),
            clips: 15,
            lid_correct: lid,
            tiers: String::from("rz"),
            mean_err_micros: err,
            mean_rtf_micros: 90_000,
        }
    }

    fn card(rows: Vec<LangRow>) -> Scorecard {
        Scorecard {
            engine: String::from("test"),
            lid: Lid::Detected,
            rows,
            fallbacks: Vec::new(),
        }
    }

    #[test]
    fn the_error_of_a_language_is_weighted_by_the_length_of_its_clips() -> Result<(), EvalError> {
        let rows = summarize(&[
            clip("ja", Some("ja"), 100_000, 3_000),
            clip("ja", Some("ja"), 0, 1_000),
        ])?;
        assert_eq!(rows[0].mean_err_micros, 75_000);
        assert_eq!(rows[0].lid_correct, Some(2));
        Ok(())
    }

    #[test]
    fn a_wrong_detection_is_not_counted_right() -> Result<(), EvalError> {
        let rows = summarize(&[
            clip("ko", Some("ja"), 0, 1_000),
            clip("ko", Some("ko"), 0, 1_000),
        ])?;
        assert_eq!(rows[0].lid_correct, Some(1));
        Ok(())
    }

    #[test]
    fn a_run_with_no_detector_counts_no_detections() -> Result<(), EvalError> {
        let rows = summarize(&[clip("ja", None, 0, 1_000)])?;
        assert_eq!(rows[0].lid_correct, None);
        Ok(())
    }

    #[test]
    fn languages_come_out_in_the_order_of_the_table() -> Result<(), EvalError> {
        let rows = summarize(&[clip("yue", None, 0, 1), clip("ja", None, 0, 1)])?;
        let order: Vec<&str> = rows.iter().map(|row| row.lang.as_str()).collect();
        assert_eq!(order, ["ja", "yue"]);
        Ok(())
    }

    #[test]
    fn clips_that_last_nothing_are_refused() {
        assert!(summarize(&[clip("ja", None, 0, 0)]).is_err());
    }

    #[test]
    fn the_most_used_tier_comes_first() -> Result<(), EvalError> {
        let mut sv = clip("ko", None, 0, 1);
        sv.tier = String::from("sv");
        let rows = summarize(&[clip("ko", None, 0, 1), sv.clone(), sv])?;
        assert_eq!(rows[0].tiers, "sv+rz");
        Ok(())
    }

    #[test]
    fn a_decimal_is_read_in_millionths() -> Result<(), EvalError> {
        assert_eq!(parse_micros("0.038")?, 38_000);
        assert_eq!(parse_micros(" 1.0 ")?, 1_000_000);
        assert!(parse_micros("38").is_err());
        assert!(parse_micros("0.1234567").is_err());
        Ok(())
    }

    #[test]
    fn a_table_row_of_the_python_scorecard_is_read() -> Result<(), EvalError> {
        let text = "| lang | clips |\n|---|---|\n| ja | 15 | 14/15 | rz | 0.038 | 0.090 |\n";
        let rows = parse_markdown(text)?;
        assert_eq!(
            rows,
            [LangRow {
                lid_correct: Some(14),
                ..row("ja", 38_000, None)
            }]
        );
        Ok(())
    }

    #[test]
    fn a_run_made_without_opencc_is_refused() {
        assert!(parse_markdown("本番経路 yueは生CER: opencc無し").is_err());
    }

    #[test]
    fn the_baseline_is_the_mean_error_and_the_most_right_detections() -> Result<(), EvalError> {
        let one = "| ja | 15 | 14/15 | rz | 0.038 | 0.090 |";
        let two = "| ja | 15 | 15/15 | rz | 0.040 | 0.100 |";
        let baseline = python_baseline(&[one, two])?;
        assert_eq!(baseline.rows[0].mean_err_micros, 39_000);
        assert_eq!(baseline.rows[0].lid_correct, Some(15));
        Ok(())
    }

    #[test]
    fn a_language_missing_from_a_run_is_refused() {
        let one =
            "| ja | 15 | 15/15 | rz | 0.038 | 0.090 |\n| en | 15 | 15/15 | v3 | 0.023 | 0.1 |";
        let two = "| ja | 15 | 15/15 | rz | 0.038 | 0.090 |";
        assert!(python_baseline(&[one, two]).is_err());
    }

    #[test]
    fn no_run_gives_no_baseline() {
        assert!(python_baseline(&[]).is_err());
    }

    #[test]
    fn an_equal_run_is_accepted_and_a_worse_one_is_named() {
        let python = card(vec![row("ja", 38_000, Some(15))]);
        assert_eq!(check_scorecard(&python, &python), Ok(()));
        let worse = card(vec![row("ja", 38_500, Some(15))]);
        assert!(matches!(
            check_scorecard(&python, &worse),
            Err(ScoreError::Worse { lang, .. }) if lang == "ja"
        ));
    }

    #[test]
    fn an_error_is_rounded_half_up_to_the_table_places_before_the_compare() {
        // 0.061454 is 0.061 in the table; 0.0615 is 0.062.
        let python = card(vec![row("yue", 61_000, Some(15))]);
        for passes in [61_000, 61_454, 61_499] {
            let run = card(vec![row("yue", passes, Some(15))]);
            assert_eq!(check_scorecard(&python, &run), Ok(()), "{passes}");
        }
        for fails in [61_500, 61_501, 62_000] {
            let run = card(vec![row("yue", fails, Some(15))]);
            assert!(check_scorecard(&python, &run).is_err(), "{fails}");
        }
    }

    #[test]
    fn rounding_at_the_table_places_is_half_up_and_keeps_table_values() {
        assert_eq!(at_table_places(0), 0);
        assert_eq!(at_table_places(499), 0);
        assert_eq!(at_table_places(500), 1_000);
        assert_eq!(at_table_places(86_939), 87_000);
        assert_eq!(at_table_places(38_000), 38_000);
    }

    #[test]
    fn a_missing_language_fewer_detections_and_no_detector_are_each_refused() {
        let python = card(vec![row("ja", 38_000, Some(15))]);
        assert_eq!(
            check_scorecard(&python, &card(Vec::new())),
            Err(ScoreError::MissingLanguage(String::from("ja")))
        );
        assert!(matches!(
            check_scorecard(&python, &card(vec![row("ja", 38_000, Some(14))])),
            Err(ScoreError::LidWorse {
                python: 15,
                rust: 14,
                ..
            })
        ));
        assert_eq!(
            check_scorecard(&python, &card(vec![row("ja", 0, None)])),
            Err(ScoreError::LidNotMeasured(String::from("ja")))
        );
    }

    #[test]
    fn a_run_that_raised_a_fallback_event_is_refused_whatever_its_numbers() {
        let python = card(vec![row("ja", 38_000, Some(15))]);
        let mut rust = python.clone();
        rust.fallbacks
            .push(String::from("model.fallback.substitute"));
        assert_eq!(
            check_scorecard(&python, &rust),
            Err(ScoreError::Fallback(String::from(
                "model.fallback.substitute"
            )))
        );
    }

    #[test]
    fn a_scorecard_survives_its_json_form() -> Result<(), EvalError> {
        let original = card(vec![row("ja", 38_000, None)]);
        let text = serde_json::to_string(&original)
            .map_err(|error| EvalError::Invalid(error.to_string()))?;
        let back: Scorecard =
            serde_json::from_str(&text).map_err(|error| EvalError::Invalid(error.to_string()))?;
        assert_eq!(back, original);
        Ok(())
    }
}

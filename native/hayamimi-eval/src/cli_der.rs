//! The command line of the `der` sub command: score the Rust route's speaker
//! labels on an evaluation set of meetings (a manifest, wavs and RTTM files).

use std::path::{Path, PathBuf};

use clap::Args;
use serde::Deserialize;

use crate::der_run::{self, MeetingScore, Options};
use crate::error::EvalError;
use crate::manifest::write_text;

/// The model the search for the models directory looks for.
const MODELS_MARK: &str = "silero_vad.onnx";

#[derive(Args)]
pub struct DerArgs {
    /// The evaluation set: a directory with manifest.json, the wavs and the RTTM files.
    #[arg(long)]
    pub set: PathBuf,
    /// Seconds around each reference turn boundary left out of the scoring.
    #[arg(long, default_value_t = 0.25)]
    pub collar: f64,
    /// Score only this meeting.
    #[arg(long)]
    pub meeting: Option<String>,
    /// Silence in seconds that ends an utterance (the Python evaluation's default).
    #[arg(long, default_value_t = 0.35)]
    pub min_silence: f32,
    /// Seconds of continuous speech after which an utterance is finalised.
    #[arg(long, default_value_t = 12.0)]
    pub max_speech: f32,
    /// The directory that holds the model directories; default: models/ at or above the current directory.
    #[arg(long)]
    pub models: Option<PathBuf>,
    /// Also write the report to this file.
    #[arg(long)]
    pub out: Option<PathBuf>,
}

/// One row of the set's manifest.
#[derive(Debug, Deserialize)]
struct Entry {
    meeting: String,
    wav: String,
    rttm: String,
}

fn read_manifest(set: &Path) -> Result<Vec<Entry>, EvalError> {
    let path = set.join("manifest.json");
    let text = std::fs::read_to_string(&path).map_err(|source| EvalError::io(&path, source))?;
    serde_json::from_str(&text).map_err(|source| EvalError::json(&path, source))
}

/// The meetings to score: all of them, or the one named.
fn chosen(entries: Vec<Entry>, meeting: Option<&str>) -> Result<Vec<Entry>, EvalError> {
    let Some(name) = meeting else {
        return Ok(entries);
    };
    let found: Vec<Entry> = entries
        .into_iter()
        .filter(|entry| entry.meeting == name)
        .collect();
    if found.is_empty() {
        return Err(EvalError::Invalid(format!(
            "meeting {name:?} is not in the manifest"
        )));
    }
    Ok(found)
}

/// The first `models/` at or above `start` that holds the voice detector model.
fn find_models(start: &Path) -> Result<PathBuf, EvalError> {
    start
        .ancestors()
        .map(|directory| directory.join("models"))
        .find(|models| models.join(MODELS_MARK).is_file())
        .ok_or_else(|| {
            EvalError::Invalid(format!(
                "no models/{MODELS_MARK} at or above {}",
                start.display()
            ))
        })
}

fn models_directory(given: Option<PathBuf>) -> Result<PathBuf, EvalError> {
    match given {
        Some(models) => Ok(models),
        None => {
            let here =
                std::env::current_dir().map_err(|source| EvalError::io(Path::new("."), source))?;
            find_models(&here)
        }
    }
}

/// Scores each meeting and prints its line as soon as it is done. Returns the report.
fn score_all(args: &DerArgs, entries: &[Entry], models: &Path) -> Result<String, EvalError> {
    let options = Options {
        min_silence: args.min_silence,
        max_speech: args.max_speech,
    };
    let mut report = String::new();
    let mut scores: Vec<MeetingScore> = Vec::new();
    for entry in entries {
        let score = der_run::score_meeting(
            models,
            &args.set.join(&entry.wav),
            &args.set.join(&entry.rttm),
            args.collar,
            &options,
        )?;
        let line = der_run::meeting_line(&entry.meeting, &score);
        println!("{line}");
        report.push_str(&line);
        report.push('\n');
        scores.push(score);
    }
    if let Some(mean) = der_run::mean_line(&scores, args.collar) {
        println!("{mean}");
        report.push_str(&mean);
        report.push('\n');
    }
    Ok(report)
}

/// `hayamimi-eval der`.
pub fn der(args: DerArgs) -> Result<(), EvalError> {
    let entries = chosen(read_manifest(&args.set)?, args.meeting.as_deref())?;
    let models = models_directory(args.models.clone())?;
    let report = score_all(&args, &entries, &models)?;
    if let Some(out) = &args.out {
        write_text(out, &report)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str) -> Entry {
        Entry {
            meeting: name.to_owned(),
            wav: format!("{name}.wav"),
            rttm: format!("{name}.rttm"),
        }
    }

    #[test]
    fn every_meeting_is_chosen_when_none_is_named() -> Result<(), EvalError> {
        assert_eq!(chosen(vec![entry("A"), entry("B")], None)?.len(), 2);
        Ok(())
    }

    #[test]
    fn a_named_meeting_is_the_only_one_chosen() -> Result<(), EvalError> {
        let found = chosen(vec![entry("A"), entry("B")], Some("B"))?;
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].meeting, "B");
        Ok(())
    }

    #[test]
    fn a_meeting_that_is_not_in_the_manifest_is_named_in_the_error() {
        let said = chosen(vec![entry("A")], Some("Z"))
            .err()
            .map(|error| error.to_string());
        assert!(
            said.as_deref().is_some_and(|text| text.contains("\"Z\"")),
            "{said:?}"
        );
    }

    #[test]
    fn the_manifest_rows_are_read_by_their_names() -> Result<(), serde_json::Error> {
        let rows: Vec<Entry> = serde_json::from_str(
            r#"[{"meeting":"M","split":"dev","wav":"M.wav","rttm":"M.rttm","duration_s":600.0}]"#,
        )?;
        assert_eq!(
            (rows[0].wav.as_str(), rows[0].rttm.as_str()),
            ("M.wav", "M.rttm")
        );
        Ok(())
    }

    #[test]
    fn a_directory_with_no_models_above_it_is_an_error() {
        assert!(find_models(Path::new("/no/such/place")).is_err());
    }

    #[test]
    fn the_command_line_names_a_set() {
        use clap::Parser;
        #[derive(Parser)]
        struct Probe {
            #[command(flatten)]
            args: DerArgs,
        }
        assert!(Probe::try_parse_from(["probe"]).is_err());
        assert!(Probe::try_parse_from(["probe", "--set", "testdata/eval_diar"]).is_ok());
    }
}

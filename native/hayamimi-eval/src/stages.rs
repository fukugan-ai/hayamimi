//! `stages`: the samples one measurement run writes, and how they are read back.
//!
//! A run is one process measuring every stage it can on one provider under
//! one load. It writes a JSON-lines file: first a `meta` line that says what
//! the run was, then one `sample` line per timed call. The report reads many
//! such files and turns them into the tables of `gpu_stages.md`.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::EvalError;
use crate::manifest::write_text;
use crate::percentile::nearest_rank;

/// What a run was.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Meta {
    /// `cuda` or `cpu`.
    pub provider: String,
    /// Whether two clients were sending translation requests to the GPU server during the run.
    pub load: bool,
    /// `empty` when the process was started with an empty JIT cache, `warm` otherwise.
    pub cache: String,
    /// The number of this run among the runs of one combination.
    pub run: u32,
    pub repeat: u32,
    pub utterances: usize,
    /// The operating system the run was measured on.
    pub os: String,
}

/// One timed call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sample {
    pub stage: String,
    pub variant: String,
    pub utt: u32,
    pub repeat: u32,
    /// The duration of the call in nanoseconds.
    pub ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Line {
    Meta(Meta),
    Sample(Sample),
}

/// The lines of one run file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunFile {
    pub meta: Meta,
    pub samples: Vec<Sample>,
}

/// Writes a run as JSON lines: the meta line, then the samples.
pub fn write_run(path: &Path, run: &RunFile) -> Result<(), EvalError> {
    let mut text = String::new();
    let meta = serde_json::to_string(&Line::Meta(run.meta.clone()))
        .map_err(|e| EvalError::json(path, e))?;
    text.push_str(&meta);
    text.push('\n');
    for sample in &run.samples {
        let line = serde_json::to_string(&Line::Sample(sample.clone()))
            .map_err(|e| EvalError::json(path, e))?;
        text.push_str(&line);
        text.push('\n');
    }
    write_text(path, &text)
}

/// Reads a run file. The first line must be the meta line.
pub fn read_run(path: &Path) -> Result<RunFile, EvalError> {
    let text = std::fs::read_to_string(path).map_err(|e| EvalError::io(path, e))?;
    parse_run(&text, &path.display().to_string())
}

fn parse_run(text: &str, origin: &str) -> Result<RunFile, EvalError> {
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let Some(first) = lines.next() else {
        return Err(EvalError::Invalid(format!(
            "{origin}: the run file is empty"
        )));
    };
    let meta = match serde_json::from_str::<Line>(first) {
        Ok(Line::Meta(meta)) => meta,
        Ok(Line::Sample(_)) => {
            return Err(EvalError::Invalid(format!(
                "{origin}: the first line is a sample, not the meta line"
            )));
        }
        Err(error) => return Err(EvalError::Invalid(format!("{origin}: line 1: {error}"))),
    };
    let mut samples = Vec::new();
    for (index, line) in lines.enumerate() {
        match serde_json::from_str::<Line>(line) {
            Ok(Line::Sample(sample)) => samples.push(sample),
            Ok(Line::Meta(_)) => {
                return Err(EvalError::Invalid(format!(
                    "{origin}: line {}: a second meta line",
                    index + 2
                )));
            }
            Err(error) => {
                return Err(EvalError::Invalid(format!(
                    "{origin}: line {}: {error}",
                    index + 2
                )));
            }
        }
    }
    Ok(RunFile { meta, samples })
}

/// The p50 and p95 of the samples of one stage and variant in one run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Aggregate {
    pub stage: String,
    pub variant: String,
    pub provider: String,
    pub load: bool,
    pub cache: String,
    pub run: u32,
    pub samples: usize,
    pub p50_ns: u64,
    pub p95_ns: u64,
}

impl Aggregate {
    /// The p50 in whole microseconds, rounded up: a call under a microsecond reads as 1.
    pub fn p50_us(&self) -> u64 {
        self.p50_ns.div_ceil(1000)
    }

    /// The p95 in whole microseconds, rounded up.
    pub fn p95_us(&self) -> u64 {
        self.p95_ns.div_ceil(1000)
    }
}

fn as_signed(value: u64) -> Result<i64, EvalError> {
    i64::try_from(value).map_err(|_| EvalError::Invalid(format!("{value} ns does not fit i64")))
}

fn as_unsigned(value: i64) -> Result<u64, EvalError> {
    u64::try_from(value).map_err(|_| EvalError::Invalid(format!("{value} ns is negative")))
}

/// Nearest-rank p50 and p95 for every (stage, variant) of a run.
pub fn aggregate(run: &RunFile) -> Result<Vec<Aggregate>, EvalError> {
    let mut groups: BTreeMap<(&str, &str), Vec<i64>> = BTreeMap::new();
    for sample in &run.samples {
        groups
            .entry((sample.stage.as_str(), sample.variant.as_str()))
            .or_default()
            .push(as_signed(sample.ns)?);
    }
    groups
        .into_iter()
        .map(|((stage, variant), values)| {
            Ok(Aggregate {
                stage: stage.to_owned(),
                variant: variant.to_owned(),
                provider: run.meta.provider.clone(),
                load: run.meta.load,
                cache: run.meta.cache.clone(),
                run: run.meta.run,
                samples: values.len(),
                p50_ns: as_unsigned(nearest_rank(&values, 50)?)?,
                p95_ns: as_unsigned(nearest_rank(&values, 95)?)?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta() -> Meta {
        Meta {
            provider: String::from("cuda"),
            load: false,
            cache: String::from("warm"),
            run: 1,
            repeat: 5,
            utterances: 23,
            os: String::from("windows"),
        }
    }

    fn sample(stage: &str, ns: u64) -> Sample {
        Sample {
            stage: stage.to_owned(),
            variant: String::from("fp32"),
            utt: 0,
            repeat: 0,
            ns,
        }
    }

    fn json(line: &Line) -> Result<String, EvalError> {
        serde_json::to_string(line).map_err(|e| EvalError::Invalid(e.to_string()))
    }

    #[test]
    fn a_run_written_and_parsed_is_the_same_run() -> Result<(), EvalError> {
        let run = RunFile {
            meta: meta(),
            samples: vec![sample("vad", 12), sample("lid", 34)],
        };
        let mut text = json(&Line::Meta(run.meta.clone()))?;
        for s in &run.samples {
            text.push('\n');
            text.push_str(&json(&Line::Sample(s.clone()))?);
        }
        assert_eq!(parse_run(&text, "t")?, run);
        Ok(())
    }

    #[test]
    fn an_empty_file_is_an_error() {
        assert!(matches!(parse_run("", "t"), Err(EvalError::Invalid(_))));
    }

    #[test]
    fn a_file_that_starts_with_a_sample_is_an_error() -> Result<(), EvalError> {
        let line = json(&Line::Sample(sample("vad", 1)))?;
        assert!(matches!(parse_run(&line, "t"), Err(EvalError::Invalid(_))));
        Ok(())
    }

    #[test]
    fn a_line_that_is_not_json_names_its_number() {
        let said = parse_run(
            "{\"kind\":\"meta\",\"provider\":\"cpu\",\"load\":false,\"cache\":\"warm\",\"run\":1,\"repeat\":5,\"utterances\":23,\"os\":\"w\"}\nnope",
            "t",
        )
        .err()
        .map(|e| e.to_string());
        assert!(
            said.as_deref().is_some_and(|t| t.contains("line 2")),
            "{said:?}"
        );
    }

    #[test]
    fn percentiles_are_nearest_rank_per_stage_and_variant() -> Result<(), EvalError> {
        let samples = (1..=20)
            .map(|n| sample("vad", n * 10))
            .chain([sample("lid", 7)])
            .collect();
        let found = aggregate(&RunFile {
            meta: meta(),
            samples,
        })?;
        let vad = found
            .iter()
            .find(|a| a.stage == "vad")
            .ok_or_else(|| EvalError::Invalid(String::from("no vad")))?;
        assert_eq!((vad.samples, vad.p50_ns, vad.p95_ns), (20, 100, 190));
        assert_eq!((vad.p50_us(), vad.p95_us()), (1, 1));
        let lid = found
            .iter()
            .find(|a| a.stage == "lid")
            .ok_or_else(|| EvalError::Invalid(String::from("no lid")))?;
        assert_eq!((lid.samples, lid.p50_ns, lid.p95_ns), (1, 7, 7));
        assert_eq!(lid.p50_us(), 1);
        Ok(())
    }

    #[test]
    fn a_run_without_samples_has_no_aggregates() -> Result<(), EvalError> {
        let run = RunFile {
            meta: meta(),
            samples: Vec::new(),
        };
        assert!(aggregate(&run)?.is_empty());
        Ok(())
    }
}

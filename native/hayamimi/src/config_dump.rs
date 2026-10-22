//! `hayamimi config dump`: prints the specification of the Japanese route as
//! one JSON document, `docs/spec/ja_pipeline_spec.json`.
//!
//! No model is loaded and no recognizer is built. A value that the Rust route
//! exports as a constant (or as a default of `hayamimi`) is read from there,
//! so a change to it fails `native/hayamimi/tests/ja_pipeline_spec.rs`. The
//! values listed in `docs/spec/ja_pipeline.md` as declared values are written
//! here, because the constant is private to a module that is not exported or
//! is not a number of the route. Object keys come out sorted, because
//! `serde_json` keeps its maps in key order.
//!
//! `--with-models` adds the size and sha256 of the files the route loads. A
//! models directory that does not exist gives an empty `models` object.

use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;
use std::time::{SystemTime, SystemTimeError, UNIX_EPOCH};

use hayamimi_asr::audio::{PREROLL_SECONDS, SAMPLE_RATE, WINDOW};
use hayamimi_asr::punct::rules::{
    COMMA_THRESHOLD, MARKS, MAX_CHARS, PERIOD_THRESHOLD, QUESTION_SUFFIXES,
};
use hayamimi_asr::route::SECOND_OPINION_THRESHOLD;
use hayamimi_asr::{decode, punct, recognizer, retry, vad};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::args::{self, ConfigDump, DEFAULT_DECODE_CAP, DEFAULT_MAX_SPEECH, DEFAULT_MIN_SILENCE};
use crate::run::{EXIT_FAILED, EXIT_OK, EXIT_REFUSED};
use crate::strings::{self, Strings, keys};

/// The `generated_by` of the document.
pub const GENERATED_BY: &str = "hayamimi config dump";
/// The `schema_version` of the document.
pub const SCHEMA_VERSION: u32 = 1;
/// The document the values are described in.
const DESCRIBES: &str = "docs/spec/ja_pipeline.md";

const RECOGNIZER_DIR: &str = "sherpa-onnx-zipformer-ja-en-reazonspeech-2025-01-17";
const PUNCT_DIR: &str = "mojicast-punct-onnx";
const VOCAB_FILE: &str = "vocab.txt";
const ENCODER_FILES: &str = "encoder-*.int8.onnx";
const DECODER_FILES: &str = "decoder-*.int8.onnx";
const JOINER_FILES: &str = "joiner-*.int8.onnx";
const TOKENS_FILE: &str = "tokens.txt";

/// The files the route loads: the key in `models`, the directory under the
/// models directory, and the file name, where one `*` stands for any run.
const ROUTE_MODELS: [(&str, &str, &str); 7] = [
    ("recognizer.encoder", RECOGNIZER_DIR, ENCODER_FILES),
    ("recognizer.decoder", RECOGNIZER_DIR, DECODER_FILES),
    ("recognizer.joiner", RECOGNIZER_DIR, JOINER_FILES),
    ("recognizer.tokens", RECOGNIZER_DIR, TOKENS_FILE),
    ("vad", "", vad::VAD_FILE),
    ("punctuation.model", PUNCT_DIR, punct::MODEL_FILE),
    ("punctuation.vocab", PUNCT_DIR, VOCAB_FILE),
];

/// The bytes read from a model file at a time.
const HASH_BLOCK: usize = 1 << 20;

/// Why a dump could not be written.
#[derive(Debug)]
pub enum DumpError {
    /// A file or the standard output could not be read or written.
    Io {
        target: String,
        source: io::Error,
    },
    Json(serde_json::Error),
    Clock(SystemTimeError),
}

impl DumpError {
    fn io(target: &Path, source: io::Error) -> DumpError {
        DumpError::Io {
            target: target.display().to_string(),
            source,
        }
    }
}

impl fmt::Display for DumpError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DumpError::Io { target, source } => write!(out, "{target}: {source}"),
            DumpError::Json(error) => fmt::Display::fmt(error, out),
            DumpError::Clock(error) => fmt::Display::fmt(error, out),
        }
    }
}

impl std::error::Error for DumpError {}

impl From<serde_json::Error> for DumpError {
    fn from(error: serde_json::Error) -> DumpError {
        DumpError::Json(error)
    }
}

impl From<SystemTimeError> for DumpError {
    fn from(error: SystemTimeError) -> DumpError {
        DumpError::Clock(error)
    }
}

/// A single-precision constant as the shortest decimal that reads back to it
/// (`0.35`, not the double nearest to the single), so it compares equal to
/// the number written in the document.
fn single(value: f32) -> Result<Value, DumpError> {
    Ok(serde_json::from_str(&format!("{value:?}"))?)
}

fn route_section() -> Value {
    json!({
        "forced_lang": "ja",
        "language_identification": false,
        "second_opinion_default": false,
        "second_opinion_agree_threshold": SECOND_OPINION_THRESHOLD,
        "postprocessing_order": ["itn", "punctuation", "replacements"],
    })
}

fn recognizer_section() -> Result<Value, DumpError> {
    Ok(json!({
        "name": RECOGNIZER_DIR,
        "api": "sherpa_onnx.OfflineRecognizer.from_transducer",
        "files": {
            "encoder": ENCODER_FILES,
            "decoder": DECODER_FILES,
            "joiner": JOINER_FILES,
            "tokens": TOKENS_FILE,
        },
        "model_type": "zipformer",
        "decoding_method": "modified_beam_search",
        "modeling_unit": "cjkchar",
        "num_threads": recognizer::THREADS,
        "hotwords_file": "",
        "hotwords_score": single(recognizer::HOTWORDS_SCORE)?,
        "hotwords_usable": false,
        "feature_sample_rate": SAMPLE_RATE,
        "feature_dim": 80,
    }))
}

fn vad_section() -> Result<Value, DumpError> {
    Ok(json!({
        "model": vad::VAD_FILE,
        "threshold": single(vad::DEFAULT_THRESHOLD)?,
        "min_silence_duration": single(DEFAULT_MIN_SILENCE)?,
        "min_speech_duration": single(vad::MIN_SPEECH_SECONDS)?,
        "max_speech_duration": single(DEFAULT_MAX_SPEECH)?,
        "window_size": WINDOW,
        "sample_rate": SAMPLE_RATE,
        "buffer_size_in_seconds": single(vad::BUFFER_SECONDS)?,
        "num_threads": vad::THREADS,
        "speech_padding_supported": false,
    }))
}

fn preroll_section() -> Value {
    json!({
        "seconds": PREROLL_SECONDS,
        "history_keep_seconds": 30.0,
        "clamped_to_previous_segment_end": true,
        "source": "real audio from the rolling capture history",
    })
}

fn head_dropout_retry_section() -> Result<Value, DumpError> {
    Ok(json!({
        "min_buffer_seconds": retry::MIN_BUFFER_SECONDS,
        "min_silence_seconds": single(retry::MIN_SILENCE_SECONDS)?,
        "min_speech_seconds": single(vad::MIN_SPEECH_SECONDS)?,
        "pad_seconds": retry::PAD_SECONDS,
        "pad_real_context_each_side": true,
        "pad_zero_samples_each_side": true,
        "sample_rate": SAMPLE_RATE,
        "density_floor_cjk": decode::DENSITY_FLOOR_SPACELESS,
        "density_floor_latin": decode::DENSITY_FLOOR_SPACED,
        "retry_tail_chars": decode::TAIL_CHARS,
        "retry_tail_match": decode::TAIL_MATCH,
        "fallback_chunk_seconds": decode::CHUNK_SECONDS,
        "fallback_overlap_seconds": decode::CHUNK_OVERLAP_SECONDS,
    }))
}

/// The cap on the audio of one decode of the Japanese tier, as `hayamimi`
/// starts: `off`, or the seconds of a cap and of the overlap of its pieces.
fn decode_cap_section() -> Value {
    let cap = DEFAULT_DECODE_CAP;
    json!({
        "cap_s": cap.cap_seconds().map_or_else(|| json!("off"), |seconds| json!(seconds)),
        "overlap_s": cap.overlap_seconds(),
    })
}

/// The punctuation marks in code point order.
fn sorted_marks() -> Vec<String> {
    let mut marks = MARKS.to_vec();
    marks.sort_unstable();
    marks.into_iter().map(String::from).collect()
}

fn punctuation_section() -> Result<Value, DumpError> {
    Ok(json!({
        "model": punct::MODEL_FILE,
        "vocab": VOCAB_FILE,
        "input_names": ["input_ids", "attention_mask"],
        "output_name": "logits",
        "comma_threshold": single(COMMA_THRESHOLD)?,
        "period_threshold": single(PERIOD_THRESHOLD)?,
        "max_chars": MAX_CHARS,
        "force_final_period": true,
        "intra_op_num_threads": 2,
        "question_suffixes": QUESTION_SUFFIXES,
        "punctuation_chars": sorted_marks(),
        "normalization": "NFKC",
    }))
}

fn itn_section() -> Value {
    json!({
        "enabled": true,
        "module": "hayamimi-asr::itn",
        "applicable_langs": ["ja", "yue", "zh"],
    })
}

/// The effective configuration of the Japanese route.
pub fn config() -> Result<Value, DumpError> {
    Ok(json!({
        "route": route_section(),
        "recognizer": recognizer_section()?,
        "vad": vad_section()?,
        "preroll": preroll_section(),
        "head_dropout_retry": head_dropout_retry_section()?,
        "decode_cap": decode_cap_section(),
        "punctuation": punctuation_section()?,
        "itn": itn_section(),
    }))
}

/// `seconds` since the Unix epoch as `YYYY-MM-DDTHH:MM:SSZ`.
fn utc_stamp(seconds: u64) -> String {
    let (days, in_day) = (seconds / 86_400, seconds % 86_400);
    // The days are counted from 0000-03-01, so that a leap day ends its year.
    let shifted = days + 719_468;
    let era = shifted / 146_097;
    let day_of_era = shifted % 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    let (hour, minute, second) = (in_day / 3_600, in_day % 3_600 / 60, in_day % 60);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The sha256 of a file as 64 lower-case hexadecimal digits, and its size.
fn hash_file(path: &Path) -> Result<(String, u64), DumpError> {
    let mut file = fs::File::open(path).map_err(|error| DumpError::io(path, error))?;
    let mut hasher = Sha256::new();
    let mut block = vec![0_u8; HASH_BLOCK];
    let mut total = 0_u64;
    loop {
        let read = file
            .read(&mut block)
            .map_err(|error| DumpError::io(path, error))?;
        if read == 0 {
            return Ok((hex(&hasher.finalize()), total));
        }
        hasher.update(&block[..read]);
        // A read count is at most the block size, which fits in 64 bits.
        total += read as u64;
    }
}

/// Whether `name` is `pattern`, where one `*` in the pattern stands for any run.
fn matches_pattern(name: &str, pattern: &str) -> bool {
    match pattern.split_once('*') {
        Some((head, tail)) => {
            name.len() >= head.len() + tail.len() && name.starts_with(head) && name.ends_with(tail)
        }
        None => name == pattern,
    }
}

/// The first file of `base` (in name order) that matches `pattern`; `None`
/// when `base` does not exist or holds none.
fn first_match(base: &Path, pattern: &str) -> Result<Option<String>, DumpError> {
    let listing = match fs::read_dir(base) {
        Ok(listing) => listing,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(DumpError::io(base, error)),
    };
    let mut names = Vec::new();
    for entry in listing {
        let name = entry
            .map_err(|error| DumpError::io(base, error))?
            .file_name();
        if let Some(name) = name.to_str().filter(|name| matches_pattern(name, pattern)) {
            names.push(name.to_owned());
        }
    }
    Ok(names.into_iter().min())
}

/// The entry of one model file and its size (0 when the file is absent).
fn model_entry(models: &Path, subdir: &str, pattern: &str) -> Result<(Value, u64), DumpError> {
    let base = models.join(subdir);
    let Some(name) = first_match(&base, pattern)? else {
        return Ok((json!({"file": null, "present": false}), 0));
    };
    let (sha256, bytes) = hash_file(&base.join(&name))?;
    let file = if subdir.is_empty() {
        name
    } else {
        format!("{subdir}/{name}")
    };
    let entry = json!({"file": file, "present": true, "bytes": bytes, "sha256": sha256});
    Ok((entry, bytes))
}

/// The size and sha256 of every file the route loads; `{}` when `models` is
/// not a directory.
fn models_section(models: &Path) -> Result<Value, DumpError> {
    if !models.is_dir() {
        return Ok(json!({}));
    }
    let mut section = Map::new();
    let mut total = 0_u64;
    for (key, subdir, pattern) in ROUTE_MODELS {
        let (entry, bytes) = model_entry(models, subdir, pattern)?;
        total += bytes;
        section.insert(key.to_owned(), entry);
    }
    section.insert("_total_bytes".to_owned(), json!(total));
    Ok(Value::Object(section))
}

/// The document. The model files are hashed only when `models` is given.
pub fn build_spec(models: Option<&Path>, generated_at: &str) -> Result<Value, DumpError> {
    let mut spec = json!({
        "generated_by": GENERATED_BY,
        "generated_at": generated_at,
        "schema_version": SCHEMA_VERSION,
        "describes": DESCRIBES,
        "config": config()?,
    });
    if let Some(models) = models {
        spec["models"] = models_section(models)?;
    }
    Ok(spec)
}

/// The document as the text that is written: indented by two spaces, with a
/// newline at the end.
fn document(options: &ConfigDump) -> Result<String, DumpError> {
    let stamp = utc_stamp(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs());
    let models = options.with_models.then_some(options.models.as_path());
    let mut text = serde_json::to_string_pretty(&build_spec(models, &stamp)?)?;
    text.push('\n');
    Ok(text)
}

fn emit(options: &ConfigDump) -> Result<(), DumpError> {
    let text = document(options)?;
    match &options.out {
        Some(path) => fs::write(path, text).map_err(|error| DumpError::io(path, error)),
        None => io::stdout()
            .write_all(text.as_bytes())
            .map_err(|error| DumpError::io(Path::new("stdout"), error)),
    }
}

fn say(strings: &Strings, keys: &[&str]) {
    for key in keys {
        eprintln!("{}", strings.cli_text(key));
    }
}

/// Runs `hayamimi config <args>` and returns the exit code.
pub fn run(args: &[String], locale_tag: Option<&str>) -> u8 {
    let Ok(strings) = Strings::embedded(strings::resolve(locale_tag)) else {
        // The compiled-in tables are checked by a test; without one there is no text to print.
        return EXIT_FAILED;
    };
    let options = match args::parse_config(args) {
        Ok(options) => options,
        Err(error) => {
            say(&strings, &[error.key(), keys::USAGE_HINT]);
            return EXIT_REFUSED;
        }
    };
    match emit(&options) {
        Ok(()) => EXIT_OK,
        Err(error) => {
            eprintln!("{}", strings.cli_text(keys::RUN_FAILED));
            eprintln!("  {error}");
            EXIT_FAILED
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn words(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn the_dump_says_the_decode_cap_is_off_by_default() -> TestResult {
        let dump = config()?;
        assert_eq!(
            dump["decode_cap"],
            json!({ "cap_s": "off", "overlap_s": 0.0 })
        );
        Ok(())
    }

    #[test]
    fn the_epoch_is_the_first_second_of_1970() {
        assert_eq!(utc_stamp(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn a_known_second_reads_as_its_date() {
        assert_eq!(utc_stamp(1_000_000_000), "2001-09-09T01:46:40Z");
    }

    #[test]
    fn a_leap_day_is_a_day_of_february() {
        assert_eq!(utc_stamp(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(utc_stamp(1_709_164_800), "2024-02-29T00:00:00Z");
    }

    #[test]
    fn the_last_second_of_february_in_a_common_century_year_is_the_28th() {
        assert_eq!(utc_stamp(4_107_542_399), "2100-02-28T23:59:59Z");
        assert_eq!(utc_stamp(4_107_542_400), "2100-03-01T00:00:00Z");
    }

    #[test]
    fn a_star_stands_for_any_run_and_nothing_else_does() {
        assert!(matches_pattern("encoder-epoch-35.int8.onnx", ENCODER_FILES));
        assert!(matches_pattern("encoder-.int8.onnx", ENCODER_FILES));
        assert!(!matches_pattern("encoder-epoch-35.onnx", ENCODER_FILES));
        assert!(!matches_pattern(
            "decoder-epoch-35.int8.onnx",
            ENCODER_FILES
        ));
        assert!(matches_pattern("tokens.txt", TOKENS_FILE));
        assert!(!matches_pattern("tokens.txt.bak", TOKENS_FILE));
        // The head and the tail may not overlap.
        assert!(!matches_pattern("encoder-.onnx", "encoder-*-.onnx"));
    }

    #[test]
    fn the_config_has_the_eight_sections() -> TestResult {
        let Value::Object(sections) = config()? else {
            return Err("the config is not an object".into());
        };
        let names: Vec<&str> = sections.keys().map(String::as_str).collect();
        let mut expected = [
            "route",
            "recognizer",
            "vad",
            "preroll",
            "head_dropout_retry",
            "decode_cap",
            "punctuation",
            "itn",
        ];
        expected.sort_unstable();
        assert_eq!(names, expected);
        Ok(())
    }

    #[test]
    fn a_spec_without_models_has_no_models_key() -> TestResult {
        let spec = build_spec(None, "2026-01-01T00:00:00Z")?;
        assert!(spec.get("models").is_none());
        assert_eq!(spec["generated_by"], GENERATED_BY);
        assert_eq!(spec["generated_at"], "2026-01-01T00:00:00Z");
        Ok(())
    }

    #[test]
    fn a_models_directory_that_does_not_exist_gives_an_empty_models_object() -> TestResult {
        let missing = Path::new("no_such_models_directory");
        let spec = build_spec(Some(missing), "2026-01-01T00:00:00Z")?;
        assert_eq!(spec["models"], json!({}));
        assert_eq!(spec["config"], config()?);
        Ok(())
    }

    #[test]
    fn a_file_is_hashed_to_the_digest_of_its_bytes() -> TestResult {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let (sha256, bytes) = hash_file(&manifest)?;
        let content = include_bytes!("../Cargo.toml");
        assert_eq!(sha256, hex(&Sha256::digest(content)));
        assert_eq!(bytes, content.len() as u64);
        Ok(())
    }

    #[test]
    fn a_file_that_is_not_there_is_an_io_error() {
        let result = hash_file(Path::new("no_such_model_file.onnx"));
        assert!(matches!(result, Err(DumpError::Io { .. })));
    }

    #[test]
    fn a_missing_directory_holds_no_match() -> TestResult {
        assert_eq!(first_match(Path::new("no_such_directory"), "*")?, None);
        Ok(())
    }

    #[test]
    fn a_command_other_than_dump_is_refused() {
        assert_eq!(run(&[], None), EXIT_REFUSED);
        assert_eq!(run(&words("load"), None), EXIT_REFUSED);
        assert_eq!(run(&words("dump --bogus"), None), EXIT_REFUSED);
    }
}

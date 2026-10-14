//! The command line of `hayamimi models fetch`.

use std::path::PathBuf;

use crate::strings::keys;

use super::keys as models_keys;

/// The directory that holds the model directories when `--dir` is not given.
pub const DEFAULT_DIR: &str = "models";
/// Where the sherpa-onnx release assets are downloaded from: `<base>/<tag>/<file>`.
pub const RELEASE_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download";
/// Where Hugging Face repositories are read from: `<base>/<repo>/resolve/main/<file>`.
pub const HF_URL: &str = "https://huggingface.co";

/// What `hayamimi models fetch` was asked to fetch, and from where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// Only the core of Japanese and English.
    pub minimal: bool,
    /// Also the English-only Parakeet v2 recognizer.
    pub en_parakeet_v2: bool,
    /// Also the four-class Japanese punctuation model.
    pub punct_4class: bool,
    pub dir: PathBuf,
    pub release_url: String,
    pub hf_url: String,
}

impl Options {
    /// The options of a bare `fetch`: the whole set, from the published sources.
    pub fn new() -> Options {
        Options {
            minimal: false,
            en_parakeet_v2: false,
            punct_4class: false,
            dir: PathBuf::from(DEFAULT_DIR),
            release_url: String::from(RELEASE_URL),
            hf_url: String::from(HF_URL),
        }
    }
}

/// What the command line asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Parsed {
    Help,
    Fetch(Options),
}

/// Why a command line was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionError {
    UnknownCommand,
    UnknownOption,
    MissingValue,
}

impl OptionError {
    /// The key into the locale's `[cli]` table.
    pub fn key(self) -> &'static str {
        match self {
            OptionError::UnknownCommand => models_keys::UNKNOWN_COMMAND,
            OptionError::UnknownOption => keys::UNKNOWN_OPTION,
            OptionError::MissingValue => keys::MISSING_VALUE,
        }
    }
}

type Rest<'a> = std::slice::Iter<'a, String>;

fn value_of(rest: &mut Rest<'_>) -> Result<String, OptionError> {
    rest.next().cloned().ok_or(OptionError::MissingValue)
}

fn parse_fetch(args: &[String]) -> Result<Parsed, OptionError> {
    let mut options = Options::new();
    let mut rest = args.iter();
    while let Some(option) = rest.next() {
        match option.as_str() {
            "--help" => return Ok(Parsed::Help),
            "--minimal" => options.minimal = true,
            "--en-parakeet-v2" => options.en_parakeet_v2 = true,
            "--punct-4class" => options.punct_4class = true,
            "--dir" => options.dir = PathBuf::from(value_of(&mut rest)?),
            "--release-url" => options.release_url = value_of(&mut rest)?,
            "--hf-url" => options.hf_url = value_of(&mut rest)?,
            _ => return Err(OptionError::UnknownOption),
        }
    }
    Ok(Parsed::Fetch(options))
}

/// Parses the arguments after `models`.
pub fn parse(args: &[String]) -> Result<Parsed, OptionError> {
    match args.split_first() {
        Some((command, rest)) if command == "fetch" => parse_fetch(rest),
        Some((flag, _)) if flag == "--help" => Ok(Parsed::Help),
        _ => Err(OptionError::UnknownCommand),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_owned).collect()
    }

    fn fetch(line: &str) -> Result<Options, OptionError> {
        match parse(&words(line))? {
            Parsed::Fetch(options) => Ok(options),
            Parsed::Help => Err(OptionError::UnknownCommand),
        }
    }

    #[test]
    fn a_bare_fetch_takes_the_whole_set_from_the_published_sources() -> Result<(), OptionError> {
        let options = fetch("fetch")?;
        assert_eq!(options, Options::new());
        assert!(!options.minimal && !options.en_parakeet_v2 && !options.punct_4class);
        assert_eq!(options.dir, PathBuf::from("models"));
        assert!(options.release_url.starts_with("https://github.com/"));
        assert!(options.hf_url.starts_with("https://huggingface.co"));
        Ok(())
    }

    #[test]
    fn every_option_is_read() -> Result<(), OptionError> {
        let options = fetch(
            "fetch --minimal --en-parakeet-v2 --punct-4class --dir d --release-url http://r --hf-url http://h",
        )?;
        assert!(options.minimal && options.en_parakeet_v2 && options.punct_4class);
        assert_eq!(options.dir, PathBuf::from("d"));
        assert_eq!(options.release_url, "http://r");
        assert_eq!(options.hf_url, "http://h");
        Ok(())
    }

    #[test]
    fn an_option_without_its_value_is_refused() {
        for line in ["fetch --dir", "fetch --release-url", "fetch --hf-url"] {
            assert_eq!(fetch(line), Err(OptionError::MissingValue), "{line}");
        }
    }

    #[test]
    fn an_unknown_option_or_command_is_refused() {
        assert_eq!(fetch("fetch --gpu"), Err(OptionError::UnknownOption));
        assert_eq!(fetch("get"), Err(OptionError::UnknownCommand));
        assert_eq!(parse(&[]), Err(OptionError::UnknownCommand));
    }

    #[test]
    fn help_is_asked_for_with_the_flag() {
        assert_eq!(parse(&words("--help")), Ok(Parsed::Help));
    }

    #[test]
    fn help_after_fetch_asks_for_the_text_wherever_it_stands() {
        for line in [
            "fetch --help",
            "fetch --minimal --help",
            "fetch --help --minimal",
        ] {
            assert_eq!(parse(&words(line)), Ok(Parsed::Help), "{line}");
        }
    }

    #[test]
    fn a_value_that_looks_like_help_is_a_value() -> Result<(), OptionError> {
        let options = fetch("fetch --dir --help")?;
        assert_eq!(options.dir, PathBuf::from("--help"));
        Ok(())
    }

    #[test]
    fn every_refusal_names_a_key() {
        for error in [
            OptionError::UnknownCommand,
            OptionError::UnknownOption,
            OptionError::MissingValue,
        ] {
            assert!(!error.key().is_empty());
        }
    }
}

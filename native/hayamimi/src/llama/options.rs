//! The command line of `hayamimi llama`.

use std::path::PathBuf;

use super::keys;

/// The port the server listens on when `--port` is not given. The relay and
/// the measuring tools reach it on the same address.
pub const DEFAULT_PORT: u16 = 8099;
/// How long a start waits for the model to load and the server to answer,
/// before it counts the way it tried as failed. A 3.5 GB model read from a
/// cold disk takes tens of seconds.
pub const DEFAULT_WAIT_SECONDS: u64 = 180;

/// What `hayamimi llama start` was asked to start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// A file name in the install's `models/llm`, or a path.
    pub model: String,
    pub port: u16,
    /// The install root; the user's local data directory when not given.
    pub dir: Option<PathBuf>,
    pub wait_seconds: u64,
}

/// What the command line asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Parsed {
    Help,
    Start(Options),
}

/// Why a command line was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionError {
    UnknownCommand,
    UnknownOption,
    MissingValue,
    BadNumber,
    NeedModel,
}

impl OptionError {
    /// The key into the locale's `[cli]` table.
    pub fn key(self) -> &'static str {
        match self {
            OptionError::UnknownCommand => keys::UNKNOWN_COMMAND,
            OptionError::UnknownOption => crate::strings::keys::UNKNOWN_OPTION,
            OptionError::MissingValue => crate::strings::keys::MISSING_VALUE,
            OptionError::BadNumber => crate::strings::keys::BAD_NUMBER,
            OptionError::NeedModel => keys::NEED_MODEL,
        }
    }
}

type Rest<'a> = std::slice::Iter<'a, String>;

fn value_of<'a>(rest: &mut Rest<'a>) -> Result<&'a String, OptionError> {
    rest.next().ok_or(OptionError::MissingValue)
}

fn number_of<T: std::str::FromStr + PartialEq + Default>(
    rest: &mut Rest<'_>,
) -> Result<T, OptionError> {
    let number: T = value_of(rest)?
        .parse()
        .map_err(|_| OptionError::BadNumber)?;
    if number == T::default() {
        return Err(OptionError::BadNumber);
    }
    Ok(number)
}

fn parse_start(args: &[String]) -> Result<Options, OptionError> {
    let mut model = None;
    let mut options = Options {
        model: String::new(),
        port: DEFAULT_PORT,
        dir: None,
        wait_seconds: DEFAULT_WAIT_SECONDS,
    };
    let mut rest = args.iter();
    while let Some(option) = rest.next() {
        match option.as_str() {
            "--model" => model = Some(value_of(&mut rest)?.clone()),
            "--port" => options.port = number_of(&mut rest)?,
            "--wait" => options.wait_seconds = number_of(&mut rest)?,
            "--dir" => options.dir = Some(PathBuf::from(value_of(&mut rest)?)),
            _ => return Err(OptionError::UnknownOption),
        }
    }
    options.model = model.ok_or(OptionError::NeedModel)?;
    Ok(options)
}

/// Parses the arguments after `llama`.
pub fn parse(args: &[String]) -> Result<Parsed, OptionError> {
    match args.split_first() {
        Some((command, rest)) if command == "start" => parse_start(rest).map(Parsed::Start),
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

    fn start(line: &str) -> Result<Options, OptionError> {
        match parse(&words(line))? {
            Parsed::Start(options) => Ok(options),
            Parsed::Help => Err(OptionError::UnknownCommand),
        }
    }

    #[test]
    fn a_model_is_all_a_start_needs() -> Result<(), OptionError> {
        let options = start("start --model a.gguf")?;
        assert_eq!(options.model, "a.gguf");
        assert_eq!(options.port, DEFAULT_PORT);
        assert_eq!(options.wait_seconds, DEFAULT_WAIT_SECONDS);
        assert_eq!(options.dir, None);
        Ok(())
    }

    #[test]
    fn every_option_is_read() -> Result<(), OptionError> {
        let options = start("start --model a.gguf --port 9000 --wait 5 --dir root")?;
        assert_eq!(options.port, 9000);
        assert_eq!(options.wait_seconds, 5);
        assert_eq!(options.dir, Some(PathBuf::from("root")));
        Ok(())
    }

    #[test]
    fn a_start_without_a_model_is_refused() {
        assert_eq!(start("start"), Err(OptionError::NeedModel));
        assert_eq!(start("start --port 9000"), Err(OptionError::NeedModel));
    }

    #[test]
    fn a_number_must_be_a_positive_integer_that_fits() {
        for line in [
            "start --model a --port 0",
            "start --model a --port 70000",
            "start --model a --port -1",
            "start --model a --port x",
            "start --model a --wait 0",
        ] {
            assert_eq!(start(line), Err(OptionError::BadNumber), "{line}");
        }
    }

    #[test]
    fn an_option_without_its_value_is_refused() {
        for line in [
            "start --model",
            "start --model a --port",
            "start --model a --dir",
        ] {
            assert_eq!(start(line), Err(OptionError::MissingValue), "{line}");
        }
    }

    #[test]
    fn an_unknown_option_or_command_is_refused() {
        assert_eq!(
            start("start --model a --gpu"),
            Err(OptionError::UnknownOption)
        );
        assert_eq!(start("stop"), Err(OptionError::UnknownCommand));
        assert_eq!(parse(&[]), Err(OptionError::UnknownCommand));
    }

    #[test]
    fn help_is_asked_for_with_the_flag() {
        assert_eq!(parse(&words("--help")), Ok(Parsed::Help));
    }

    #[test]
    fn every_refusal_names_a_key() {
        for error in [
            OptionError::UnknownCommand,
            OptionError::UnknownOption,
            OptionError::MissingValue,
            OptionError::BadNumber,
            OptionError::NeedModel,
        ] {
            assert!(!error.key().is_empty());
        }
    }
}

//! The command line. Every refusal names a key of the locale's `[cli]` table.

use std::num::NonZeroUsize;
use std::path::PathBuf;

use hayamimi_asr::DEFAULT_MAX_RESIDENT;
use hayamimi_asr::route::{Policy, is_routable};

use crate::inputs::{Input, IoArgs};
use crate::strings::keys;

/// The port the subtitle events are served on when `--serve` has no value.
pub const DEFAULT_PORT: u16 = 8833;
const DEFAULT_MODELS: &str = "models";
const DEFAULT_MIN_SILENCE: f32 = 0.30;
const DEFAULT_MAX_SPEECH: f32 = 12.0;
/// The memory in use on the GPU, in MiB, at which a GPU stage moves to the
/// CPU. It counts every process of the GPU, the translator's included.
pub const DEFAULT_GPU_BUDGET_MIB: u64 = 14_000;

/// How the language of each utterance is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Identify the language, and switch when two identifiers agree (the default).
    Balanced,
    /// Identify the language, and switch at the first detection.
    Fast,
    /// Read everything in one language.
    Single,
}

/// The language a `single` run reads when `--lang` does not name one.
const DEFAULT_SINGLE_LANG: &str = "ja";

/// A parsed command line that asks for a run.
#[derive(Debug, Clone, PartialEq)]
pub struct Args {
    pub mode: Mode,
    /// A language to read everything in; it makes the run `single` whatever `--mode` says.
    pub lang: Option<String>,
    pub wav: PathBuf,
    /// Pace the file at the speed of the audio.
    pub realtime: bool,
    pub models: PathBuf,
    pub min_silence: f32,
    pub max_speech: f32,
    pub replace: Option<PathBuf>,
    pub serve: Option<u16>,
    pub diagnose: bool,
    /// How many recognizers besides the Japanese one stay in memory; `None` keeps them all.
    pub max_resident: Option<NonZeroUsize>,
    /// The GPU memory in use, in MiB, at which a GPU stage moves to the CPU.
    pub gpu_budget_mib: u64,
    /// Where the audio comes from besides the wav file.
    pub io: IoArgs,
}

/// What the command line asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum Parsed {
    Help,
    Run(Args),
}

/// Why a command line was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgError {
    UnknownOption,
    MissingValue,
    BadNumber,
    ModeUnsupported,
    LangUnsupported,
    NeedWav,
    UnknownInput,
    BadAddress,
}

impl ArgError {
    /// The key into the locale's `[cli]` table.
    pub fn key(self) -> &'static str {
        match self {
            ArgError::UnknownOption => keys::UNKNOWN_OPTION,
            ArgError::MissingValue => keys::MISSING_VALUE,
            ArgError::BadNumber => keys::BAD_NUMBER,
            ArgError::ModeUnsupported => keys::MODE_UNSUPPORTED,
            ArgError::LangUnsupported => keys::LANG_UNSUPPORTED,
            ArgError::NeedWav => keys::NEED_WAV,
            ArgError::UnknownInput => keys::UNKNOWN_INPUT,
            ArgError::BadAddress => keys::BAD_ADDRESS,
        }
    }
}

fn positive_seconds(text: &str) -> Result<f32, ArgError> {
    match text.parse::<f32>() {
        Ok(value) if value.is_finite() && value > 0.0 => Ok(value),
        _ => Err(ArgError::BadNumber),
    }
}

/// `--max-resident`: a number of recognizers, where zero or less keeps them all.
fn resident_cap(text: &str) -> Result<Option<NonZeroUsize>, ArgError> {
    let Ok(value) = text.parse::<i64>() else {
        return Err(ArgError::BadNumber);
    };
    if value <= 0 {
        return Ok(None);
    }
    let Ok(count) = usize::try_from(value) else {
        return Err(ArgError::BadNumber);
    };
    Ok(NonZeroUsize::new(count))
}

/// `--gpu-budget`: a positive number of MiB.
fn positive_mib(text: &str) -> Result<u64, ArgError> {
    match text.parse::<u64>() {
        Ok(value) if value > 0 => Ok(value),
        _ => Err(ArgError::BadNumber),
    }
}

/// The arguments not yet read.
type Rest<'a> = std::iter::Peekable<std::slice::Iter<'a, String>>;

/// The value that follows an option.
fn value_of<'a>(rest: &mut Rest<'a>) -> Result<&'a String, ArgError> {
    rest.next().ok_or(ArgError::MissingValue)
}

/// `--serve` takes an optional port: a following number is the port.
fn port_of(rest: &mut Rest<'_>) -> Result<u16, ArgError> {
    match rest.next_if(|next| next.parse::<u16>().is_ok()) {
        Some(text) => text.parse::<u16>().map_err(|_| ArgError::BadNumber),
        None => Ok(DEFAULT_PORT),
    }
}

/// `--mode`: one of the three ways of choosing a language.
fn mode_of(text: &str) -> Result<Mode, ArgError> {
    match text {
        "balanced" => Ok(Mode::Balanced),
        "fast" => Ok(Mode::Fast),
        "single" => Ok(Mode::Single),
        _ => Err(ArgError::ModeUnsupported),
    }
}

/// `--lang`: a language that has a recognizer of its own.
fn lang_of(text: &str) -> Result<String, ArgError> {
    if is_routable(text) {
        Ok(text.to_owned())
    } else {
        Err(ArgError::LangUnsupported)
    }
}

impl Args {
    /// The policy of the language route this command line asks for.
    pub fn policy(&self) -> Policy {
        match (&self.lang, self.mode) {
            (Some(lang), _) => Policy::single(lang),
            (None, Mode::Single) => Policy::single(DEFAULT_SINGLE_LANG),
            (None, Mode::Balanced) => Policy::balanced(),
            (None, Mode::Fast) => Policy::fast(),
        }
    }

    fn starting() -> Args {
        Args {
            mode: Mode::Balanced,
            lang: None,
            wav: PathBuf::new(),
            realtime: true,
            models: PathBuf::from(DEFAULT_MODELS),
            min_silence: DEFAULT_MIN_SILENCE,
            max_speech: DEFAULT_MAX_SPEECH,
            replace: None,
            serve: None,
            diagnose: false,
            max_resident: DEFAULT_MAX_RESIDENT,
            gpu_budget_mib: DEFAULT_GPU_BUDGET_MIB,
            io: IoArgs::default(),
        }
    }

    /// Reads one option and the value it takes.
    fn apply(&mut self, option: &str, rest: &mut Rest<'_>) -> Result<(), ArgError> {
        match option {
            "--wav" => self.wav = PathBuf::from(value_of(rest)?),
            "--no-realtime" => self.realtime = false,
            "--mode" => self.mode = mode_of(value_of(rest)?)?,
            "--lang" => self.lang = Some(lang_of(value_of(rest)?)?),
            "--models" => self.models = PathBuf::from(value_of(rest)?),
            "--min-silence" => self.min_silence = positive_seconds(value_of(rest)?)?,
            "--max-speech" => self.max_speech = positive_seconds(value_of(rest)?)?,
            "--replace" => self.replace = Some(PathBuf::from(value_of(rest)?)),
            "--serve" => self.serve = Some(port_of(rest)?),
            "--gpu-diagnose" => self.diagnose = true,
            "--max-resident" => self.max_resident = resident_cap(value_of(rest)?)?,
            "--gpu-budget" => self.gpu_budget_mib = positive_mib(value_of(rest)?)?,
            _ => return self.io.apply(option, rest),
        }
        Ok(())
    }

    /// The input of the run.
    pub fn input(&self) -> Input {
        self.io.kind()
    }
}

/// Parses the arguments after the program name.
pub fn parse(args: &[String]) -> Result<Parsed, ArgError> {
    let mut parsed = Args::starting();
    let mut seen_wav = false;
    let mut rest = args.iter().peekable();
    while let Some(arg) = rest.next() {
        if matches!(arg.as_str(), "--help" | "-h") {
            return Ok(Parsed::Help);
        }
        seen_wav |= arg == "--wav";
        parsed.apply(arg, &mut rest)?;
    }
    if parsed.io.is_complete(seen_wav) {
        Ok(Parsed::Run(parsed))
    } else {
        Err(ArgError::NeedWav)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    fn run(list: &[&str]) -> Result<Args, ArgError> {
        match parse(&args(list))? {
            Parsed::Run(parsed) => Ok(parsed),
            Parsed::Help => Err(ArgError::UnknownOption),
        }
    }

    #[test]
    fn the_acceptance_command_line_parses() -> Result<(), ArgError> {
        let parsed = run(&[
            "--wav",
            "a.wav",
            "--no-realtime",
            "--mode",
            "single",
            "--lang",
            "ja",
            "--gpu-diagnose",
        ])?;
        assert_eq!(parsed.wav, PathBuf::from("a.wav"));
        assert!(!parsed.realtime);
        assert!(parsed.diagnose);
        assert_eq!(parsed.min_silence, 0.30);
        assert_eq!(parsed.max_speech, 12.0);
        assert_eq!(parsed.models, PathBuf::from("models"));
        Ok(())
    }

    #[test]
    fn three_recognizers_stay_in_memory_and_14000_mib_is_the_gpu_budget_unless_told_otherwise()
    -> Result<(), ArgError> {
        let parsed = run(&["--wav", "a.wav"])?;
        assert_eq!(parsed.max_resident, NonZeroUsize::new(3));
        assert_eq!(parsed.gpu_budget_mib, 14_000);
        Ok(())
    }

    #[test]
    fn the_resident_cap_is_a_count_and_zero_or_less_keeps_them_all() -> Result<(), ArgError> {
        let cap = |text: &str| run(&["--wav", "a", "--max-resident", text]).map(|a| a.max_resident);
        assert_eq!(cap("2")?, NonZeroUsize::new(2));
        assert_eq!(cap("0")?, None);
        assert_eq!(cap("-1")?, None);
        assert_eq!(cap("x"), Err(ArgError::BadNumber));
        assert_eq!(cap("1.5"), Err(ArgError::BadNumber));
        assert_eq!(
            parse(&args(&["--wav", "a", "--max-resident"])),
            Err(ArgError::MissingValue)
        );
        Ok(())
    }

    #[test]
    fn the_gpu_budget_is_a_positive_number_of_mib() -> Result<(), ArgError> {
        let budget =
            |text: &str| run(&["--wav", "a", "--gpu-budget", text]).map(|a| a.gpu_budget_mib);
        assert_eq!(budget("13999")?, 13_999);
        for bad in ["0", "-5", "x", "1.5"] {
            assert_eq!(budget(bad), Err(ArgError::BadNumber), "{bad}");
        }
        Ok(())
    }

    #[test]
    fn a_run_is_paced_unless_told_otherwise() -> Result<(), ArgError> {
        assert!(run(&["--wav", "a.wav"])?.realtime);
        Ok(())
    }

    #[test]
    fn help_wins_wherever_it_stands() {
        assert_eq!(parse(&args(&["--help"])), Ok(Parsed::Help));
        assert_eq!(parse(&args(&["--wav", "a.wav", "-h"])), Ok(Parsed::Help));
    }

    #[test]
    fn no_input_is_refused() {
        assert_eq!(parse(&args(&[])), Err(ArgError::NeedWav));
    }

    #[test]
    fn an_unknown_option_is_refused() {
        assert_eq!(parse(&args(&["--wat"])), Err(ArgError::UnknownOption));
    }

    #[test]
    fn an_option_without_its_value_is_refused() {
        assert_eq!(parse(&args(&["--wav"])), Err(ArgError::MissingValue));
        assert_eq!(parse(&args(&["--models"])), Err(ArgError::MissingValue));
    }

    #[test]
    fn a_run_identifies_languages_unless_told_to_read_one() -> Result<(), ArgError> {
        let policy = |list: &[&str]| run(list).map(|parsed| parsed.policy());
        assert_eq!(policy(&["--wav", "a"])?, Policy::balanced());
        assert_eq!(policy(&["--wav", "a", "--mode", "fast"])?, Policy::fast());
        assert_eq!(
            policy(&["--wav", "a", "--mode", "single"])?,
            Policy::single("ja")
        );
        assert_eq!(
            policy(&["--wav", "a", "--lang", "ko"])?,
            Policy::single("ko")
        );
        assert_eq!(
            policy(&["--wav", "a", "--mode", "single", "--lang", "ja"])?,
            Policy::single("ja")
        );
        Ok(())
    }

    #[test]
    fn a_mode_or_language_that_does_not_exist_is_refused() {
        assert_eq!(
            parse(&args(&["--wav", "a", "--mode", "slow"])),
            Err(ArgError::ModeUnsupported)
        );
        assert_eq!(
            parse(&args(&["--wav", "a", "--lang", "xx"])),
            Err(ArgError::LangUnsupported)
        );
    }

    #[test]
    fn seconds_must_be_positive_numbers() {
        for bad in ["0", "-1", "x", "nan", "inf"] {
            assert_eq!(
                parse(&args(&["--wav", "a", "--min-silence", bad])),
                Err(ArgError::BadNumber),
                "{bad}"
            );
        }
        assert_eq!(
            run(&["--wav", "a", "--max-speech", "5"]).map(|a| a.max_speech),
            Ok(5.0)
        );
    }

    #[test]
    fn serve_takes_an_optional_port() -> Result<(), ArgError> {
        assert_eq!(run(&["--wav", "a", "--serve"])?.serve, Some(8833));
        assert_eq!(run(&["--wav", "a", "--serve", "9000"])?.serve, Some(9000));
        assert_eq!(run(&["--serve", "--wav", "a"])?.serve, Some(8833));
        assert_eq!(run(&["--wav", "a"])?.serve, None);
        Ok(())
    }

    #[test]
    fn every_refusal_names_a_key_the_table_holds() {
        for error in [
            ArgError::UnknownOption,
            ArgError::MissingValue,
            ArgError::BadNumber,
            ArgError::ModeUnsupported,
            ArgError::LangUnsupported,
            ArgError::NeedWav,
        ] {
            assert!(keys::ALL.contains(&error.key()), "{error:?}");
        }
    }
}

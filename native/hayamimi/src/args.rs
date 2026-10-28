//! The command line. Every refusal names a key of the locale's `[cli]` table.

use std::net::{IpAddr, Ipv4Addr};
use std::num::NonZeroUsize;
use std::path::PathBuf;

use hayamimi_asr::DEFAULT_MAX_RESIDENT;
use hayamimi_asr::endpoint::{HeldReuse, Mode as Endpoint};
use hayamimi_asr::recognizer::THREADS;
use hayamimi_asr::route::{Policy, is_routable};
use hayamimi_relay::origin::{Guard, Origin};

use crate::inputs::{Input, IoArgs};
use crate::strings::keys;

/// The port the subtitle events are served on when `--serve` has no value.
pub const DEFAULT_PORT: u16 = 8833;
const DEFAULT_MODELS: &str = "models";
pub const DEFAULT_MIN_SILENCE: f32 = 0.30;
pub const DEFAULT_MAX_SPEECH: f32 = 12.0;
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
    /// Identify the language of every utterance and read it as Japanese or
    /// English, whatever the identifier says.
    Pair,
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
    /// How the end of an utterance is decided (`--endpoint`).
    pub endpoint: Endpoint,
    /// Whether a pair run reads the drafts of an utterance in the language
    /// heard after its first two seconds (`--partial-lid`).
    pub partial_lid: bool,
    /// Whether a pair run takes the language of a final from the question put
    /// to the identifier when its voice reached four seconds (`--final-lid`).
    pub final_midway: bool,
    /// How far a sentence the early detector kept may have been read past the
    /// end of the segment and still be its text (`--held-reuse`).
    pub held_reuse: HeldReuse,
    /// Whether the drafts of the utterance in progress are read by a worker
    /// thread of its own (`--draft-worker`), so that a draft never holds the
    /// feed back.
    pub draft_worker: bool,
    /// How many threads each recognizer uses (`--asr-threads`).
    pub asr_threads: i32,
    /// Whether a draft read by the Japanese tier takes the marks of a final
    /// (`--draft-marks`).
    pub draft_marks: bool,
    /// Whether the speech so far is read as soon as the level of the voice
    /// falls, so that the early notice that follows finds the text ready
    /// (`--predecode`).
    pub predecode: bool,
    pub replace: Option<PathBuf>,
    pub serve: Option<u16>,
    /// The address `--serve` listens on: the loopback unless `--serve-host` names another.
    pub serve_host: IpAddr,
    /// The pages of other hosts `--serve` is read from besides the pages of
    /// this machine (`--allow-origin`, given once for each).
    pub allow_origins: Vec<Origin>,
    pub diagnose: bool,
    /// Whether the groups of finished utterances are read again (`--no-refine` turns it off).
    pub refine: bool,
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
    Run(Box<Args>),
}

/// Why a command line was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgError {
    UnknownOption,
    MissingValue,
    BadNumber,
    ModeUnsupported,
    EndpointUnsupported,
    PartialLidUnsupported,
    FinalLidUnsupported,
    HeldReuseUnsupported,
    DraftWorkerUnsupported,
    AsrThreadsUnsupported,
    DraftMarksUnsupported,
    PredecodeUnsupported,
    LangUnsupported,
    NeedWav,
    UnknownInput,
    BadAddress,
    BadOrigin,
}

impl ArgError {
    /// The key into the locale's `[cli]` table.
    pub fn key(self) -> &'static str {
        match self {
            ArgError::UnknownOption => keys::UNKNOWN_OPTION,
            ArgError::MissingValue => keys::MISSING_VALUE,
            ArgError::BadNumber => keys::BAD_NUMBER,
            ArgError::ModeUnsupported => keys::MODE_UNSUPPORTED,
            ArgError::EndpointUnsupported => keys::ENDPOINT_UNSUPPORTED,
            ArgError::PartialLidUnsupported => keys::PARTIAL_LID_UNSUPPORTED,
            ArgError::FinalLidUnsupported => keys::FINAL_LID_UNSUPPORTED,
            ArgError::HeldReuseUnsupported => keys::HELD_REUSE_UNSUPPORTED,
            ArgError::DraftWorkerUnsupported => keys::DRAFT_WORKER_UNSUPPORTED,
            ArgError::AsrThreadsUnsupported => keys::ASR_THREADS_UNSUPPORTED,
            ArgError::DraftMarksUnsupported => keys::DRAFT_MARKS_UNSUPPORTED,
            ArgError::PredecodeUnsupported => keys::PREDECODE_UNSUPPORTED,
            ArgError::LangUnsupported => keys::LANG_UNSUPPORTED,
            ArgError::NeedWav => keys::NEED_WAV,
            ArgError::UnknownInput => keys::UNKNOWN_INPUT,
            ArgError::BadAddress => keys::BAD_ADDRESS,
            ArgError::BadOrigin => keys::BAD_ORIGIN,
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

/// `--serve-host`: an IP address.
fn address_of(rest: &mut Rest<'_>) -> Result<IpAddr, ArgError> {
    value_of(rest)?
        .parse::<IpAddr>()
        .map_err(|_| ArgError::BadAddress)
}

/// `--allow-origin`: `http://host[:port]` or `https://host[:port]`.
fn origin_of(rest: &mut Rest<'_>) -> Result<Origin, ArgError> {
    Origin::parse(value_of(rest)?).ok_or(ArgError::BadOrigin)
}

/// `--mode`: one of the four ways of choosing a language.
fn mode_of(text: &str) -> Result<Mode, ArgError> {
    match text {
        "balanced" => Ok(Mode::Balanced),
        "fast" => Ok(Mode::Fast),
        "pair" => Ok(Mode::Pair),
        "single" => Ok(Mode::Single),
        _ => Err(ArgError::ModeUnsupported),
    }
}

/// `--endpoint`: how the end of an utterance is decided.
fn endpoint_of(text: &str) -> Result<Endpoint, ArgError> {
    match text {
        "early" => Ok(Endpoint::Early),
        "silence" => Ok(Endpoint::Silence),
        _ => Err(ArgError::EndpointUnsupported),
    }
}

/// `--partial-lid`: `on` or `off`.
fn partial_lid_of(text: &str) -> Result<bool, ArgError> {
    match text {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => Err(ArgError::PartialLidUnsupported),
    }
}

/// `--final-lid`: `midway` or `close`.
fn final_lid_of(text: &str) -> Result<bool, ArgError> {
    match text {
        "midway" => Ok(true),
        "close" => Ok(false),
        _ => Err(ArgError::FinalLidUnsupported),
    }
}

/// `--held-reuse`: `covering` or `strict`.
fn held_reuse_of(text: &str) -> Result<HeldReuse, ArgError> {
    match text {
        "covering" => Ok(HeldReuse::Covering),
        "strict" => Ok(HeldReuse::Strict),
        _ => Err(ArgError::HeldReuseUnsupported),
    }
}

/// `--draft-worker`: `on` or `off`.
fn draft_worker_of(text: &str) -> Result<bool, ArgError> {
    match text {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => Err(ArgError::DraftWorkerUnsupported),
    }
}

/// `--draft-marks`: `on` or `off`.
fn draft_marks_of(text: &str) -> Result<bool, ArgError> {
    match text {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => Err(ArgError::DraftMarksUnsupported),
    }
}

/// `--predecode`: `on` or `off`.
fn predecode_of(text: &str) -> Result<bool, ArgError> {
    match text {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => Err(ArgError::PredecodeUnsupported),
    }
}

/// The most threads `--asr-threads` accepts.
const MAX_ASR_THREADS: i32 = 32;

/// `--asr-threads`: a whole number of threads, from 1 to `MAX_ASR_THREADS`.
fn asr_threads_of(text: &str) -> Result<i32, ArgError> {
    match text.parse::<i32>() {
        Ok(count) if (1..=MAX_ASR_THREADS).contains(&count) => Ok(count),
        _ => Err(ArgError::AsrThreadsUnsupported),
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
    /// The pages `--serve` lets read its streams and routes: the pages of this
    /// machine, and the origins `--allow-origin` named.
    pub fn guard(&self) -> Guard {
        Guard::new(self.allow_origins.clone())
    }

    /// The policy of the language route this command line asks for.
    pub fn policy(&self) -> Policy {
        let chosen = match (&self.lang, self.mode) {
            (Some(lang), _) => Policy::single(lang),
            (None, Mode::Single) => Policy::single(DEFAULT_SINGLE_LANG),
            (None, Mode::Balanced) => Policy::balanced(),
            (None, Mode::Fast) => Policy::fast(),
            (None, Mode::Pair) => Policy::pair(),
        };
        Policy {
            partial_lid: chosen.partial_lid && self.partial_lid,
            final_midway: chosen.final_midway && self.final_midway,
            held_reuse: self.held_reuse,
            ..chosen
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
            endpoint: Endpoint::Early,
            partial_lid: true,
            final_midway: true,
            held_reuse: HeldReuse::Covering,
            draft_worker: false,
            asr_threads: THREADS,
            draft_marks: false,
            predecode: false,
            replace: None,
            serve: None,
            serve_host: IpAddr::V4(Ipv4Addr::LOCALHOST),
            allow_origins: Vec::new(),
            diagnose: false,
            refine: true,
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
            "--endpoint" => self.endpoint = endpoint_of(value_of(rest)?)?,
            "--partial-lid" => self.partial_lid = partial_lid_of(value_of(rest)?)?,
            "--final-lid" => self.final_midway = final_lid_of(value_of(rest)?)?,
            "--held-reuse" => self.held_reuse = held_reuse_of(value_of(rest)?)?,
            "--draft-worker" => self.draft_worker = draft_worker_of(value_of(rest)?)?,
            "--asr-threads" => self.asr_threads = asr_threads_of(value_of(rest)?)?,
            "--draft-marks" => self.draft_marks = draft_marks_of(value_of(rest)?)?,
            "--predecode" => self.predecode = predecode_of(value_of(rest)?)?,
            "--replace" => self.replace = Some(PathBuf::from(value_of(rest)?)),
            "--serve" => self.serve = Some(port_of(rest)?),
            "--serve-host" => self.serve_host = address_of(rest)?,
            "--allow-origin" => self.allow_origins.push(origin_of(rest)?),
            "--gpu-diagnose" => self.diagnose = true,
            "--no-refine" => self.refine = false,
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

    /// Whether the run starts a draft worker: it was asked for, and the run
    /// shows drafts at all (`--no-partial` turns them off).
    pub fn uses_draft_worker(&self) -> bool {
        self.draft_worker && self.io.partial
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
        Ok(Parsed::Run(Box::new(parsed)))
    } else {
        Err(ArgError::NeedWav)
    }
}

/// What `hayamimi config dump` asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigDump {
    /// Also hash the model files under `models` (`--with-models`).
    pub with_models: bool,
    pub models: PathBuf,
    /// Write the document to this file instead of the standard output (`--out`).
    pub out: Option<PathBuf>,
}

/// Parses the arguments after `config`: the one command `dump` and its options.
pub fn parse_config(args: &[String]) -> Result<ConfigDump, ArgError> {
    let Some((command, options)) = args.split_first() else {
        return Err(ArgError::UnknownOption);
    };
    if command != "dump" {
        return Err(ArgError::UnknownOption);
    }
    let mut parsed = ConfigDump {
        with_models: false,
        models: PathBuf::from(DEFAULT_MODELS),
        out: None,
    };
    let mut rest = options.iter().peekable();
    while let Some(option) = rest.next() {
        match option.as_str() {
            "--with-models" => parsed.with_models = true,
            "--models" => parsed.models = PathBuf::from(value_of(&mut rest)?),
            "--out" => parsed.out = Some(PathBuf::from(value_of(&mut rest)?)),
            _ => return Err(ArgError::UnknownOption),
        }
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    fn run(list: &[&str]) -> Result<Args, ArgError> {
        match parse(&args(list))? {
            Parsed::Run(parsed) => Ok(*parsed),
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
    fn the_endpoint_is_early_unless_silence_is_asked_for() -> Result<(), ArgError> {
        assert_eq!(run(&["--wav", "a.wav"])?.endpoint, Endpoint::Early);
        let silence = run(&["--wav", "a.wav", "--endpoint", "silence"])?;
        assert_eq!(silence.endpoint, Endpoint::Silence);
        let early = run(&["--wav", "a.wav", "--endpoint", "early"])?;
        assert_eq!(early.endpoint, Endpoint::Early);
        assert_eq!(
            run(&["--wav", "a.wav", "--endpoint", "late"]).err(),
            Some(ArgError::EndpointUnsupported)
        );
        assert_eq!(
            run(&["--wav", "a.wav", "--endpoint"]).err(),
            Some(ArgError::MissingValue)
        );
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
    fn groups_are_read_again_unless_no_refine_is_given() -> Result<(), ArgError> {
        assert!(run(&["--wav", "a.wav"])?.refine);
        assert!(!run(&["--wav", "a.wav", "--no-refine"])?.refine);
        assert!(!run(&["--no-refine", "--wav", "a.wav"])?.refine);
        Ok(())
    }

    #[test]
    fn both_help_texts_name_the_no_refine_option() -> Result<(), String> {
        use hayamimi_relay::lang::Lang;
        for lang in [Lang::En, Lang::Ja] {
            let table = crate::strings::Strings::embedded(lang)?;
            assert!(
                table
                    .cli_text(keys::HELP)
                    .lines()
                    .any(|line| line.trim_start().starts_with("--no-refine ")),
                "{lang:?} does not list --no-refine"
            );
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

    fn policy_of(list: &[&str]) -> Result<Policy, ArgError> {
        run(list).map(|parsed| parsed.policy())
    }

    #[test]
    fn partial_lid_is_on_in_a_pair_run_unless_it_is_switched_off() -> Result<(), ArgError> {
        let pair = ["--wav", "a", "--mode", "pair"];
        assert!(policy_of(&pair)?.partial_lid);
        let off = ["--wav", "a", "--mode", "pair", "--partial-lid", "off"];
        let switched = policy_of(&off)?;
        assert!(!switched.partial_lid);
        // The only difference from the pair policy is the switch.
        assert_eq!(
            switched,
            Policy {
                partial_lid: false,
                ..Policy::pair()
            }
        );
        let on = ["--wav", "a", "--mode", "pair", "--partial-lid", "on"];
        assert_eq!(policy_of(&on)?, Policy::pair());
        Ok(())
    }

    #[test]
    fn partial_lid_asks_nothing_outside_a_pair_run() -> Result<(), ArgError> {
        for mode in ["balanced", "fast", "single"] {
            let line = ["--wav", "a", "--mode", mode, "--partial-lid", "on"];
            assert!(!policy_of(&line)?.partial_lid, "{mode}");
        }
        Ok(())
    }

    #[test]
    fn partial_lid_takes_on_or_off_and_nothing_else() {
        assert_eq!(
            run(&["--wav", "a", "--partial-lid", "maybe"]).err(),
            Some(ArgError::PartialLidUnsupported)
        );
        assert_eq!(
            run(&["--wav", "a", "--partial-lid"]).err(),
            Some(ArgError::MissingValue)
        );
    }

    /// T-BR-36-011
    #[test]
    fn held_reuse_takes_strict_or_covering_and_nothing_else() -> Result<(), ArgError> {
        assert_eq!(run(&["--wav", "a"])?.held_reuse, HeldReuse::Covering);
        let covering = ["--wav", "a", "--held-reuse", "covering"];
        assert_eq!(run(&covering)?.held_reuse, HeldReuse::Covering);
        let strict = ["--wav", "a", "--held-reuse", "strict"];
        assert_eq!(run(&strict)?.held_reuse, HeldReuse::Strict);
        assert_eq!(policy_of(&strict)?.held_reuse, HeldReuse::Strict);
        assert_eq!(
            run(&["--wav", "a", "--held-reuse", "maybe"]).err(),
            Some(ArgError::HeldReuseUnsupported)
        );
        assert_eq!(
            run(&["--wav", "a", "--held-reuse"]).err(),
            Some(ArgError::MissingValue)
        );
        Ok(())
    }

    /// T-BR-37-011
    #[test]
    fn final_lid_takes_midway_or_close_and_nothing_else_and_asks_nothing_outside_a_pair_run()
    -> Result<(), ArgError> {
        let pair = ["--wav", "a", "--mode", "pair"];
        assert!(policy_of(&pair)?.final_midway);
        let midway = ["--wav", "a", "--mode", "pair", "--final-lid", "midway"];
        assert!(policy_of(&midway)?.final_midway);
        let close = ["--wav", "a", "--mode", "pair", "--final-lid", "close"];
        let closed = policy_of(&close)?;
        assert!(!closed.final_midway);
        // The only difference from the pair policy is the switch.
        assert_eq!(
            closed,
            Policy {
                final_midway: false,
                ..Policy::pair()
            }
        );
        for mode in ["balanced", "fast", "single"] {
            for flag in [["--final-lid", "midway"], ["--final-lid", "close"]] {
                let line = ["--wav", "a", "--mode", mode, flag[0], flag[1]];
                assert!(!policy_of(&line)?.final_midway, "{mode} {flag:?}");
            }
        }
        assert_eq!(
            run(&["--wav", "a", "--final-lid", "maybe"]).err(),
            Some(ArgError::FinalLidUnsupported)
        );
        assert_eq!(
            run(&["--wav", "a", "--final-lid"]).err(),
            Some(ArgError::MissingValue)
        );
        Ok(())
    }

    /// T-BR-38-016
    #[test]
    fn draft_worker_takes_on_or_off_and_nothing_else_and_starts_no_worker_without_drafts()
    -> Result<(), ArgError> {
        assert!(!run(&["--wav", "a"])?.uses_draft_worker());
        let on = run(&["--wav", "a", "--draft-worker", "on"])?;
        assert!(on.draft_worker && on.uses_draft_worker());
        let off = run(&["--wav", "a", "--draft-worker", "off"])?;
        assert!(!off.draft_worker && !off.uses_draft_worker());
        assert_eq!(
            run(&["--wav", "a", "--draft-worker", "maybe"]).err(),
            Some(ArgError::DraftWorkerUnsupported)
        );
        assert_eq!(
            run(&["--wav", "a", "--draft-worker"]).err(),
            Some(ArgError::MissingValue)
        );
        // A run that shows no drafts has nothing for a worker to read.
        let silent = run(&["--wav", "a", "--draft-worker", "on", "--no-partial"])?;
        assert!(silent.draft_worker && !silent.uses_draft_worker());
        assert_eq!(
            ArgError::DraftWorkerUnsupported.key(),
            "draft_worker_unsupported"
        );
        Ok(())
    }

    /// T-BR-39-001
    #[test]
    fn asr_threads_starts_at_the_recognizer_default_and_takes_one_to_thirty_two()
    -> Result<(), ArgError> {
        assert_eq!(run(&["--wav", "a"])?.asr_threads, THREADS);
        for (text, count) in [("1", 1), ("4", 4), ("8", 8), ("32", 32)] {
            let parsed = run(&["--wav", "a", "--asr-threads", text])?;
            assert_eq!(parsed.asr_threads, count, "{text}");
        }
        Ok(())
    }

    /// T-BR-39-002
    #[test]
    fn asr_threads_refuses_what_is_not_a_whole_number_from_one_to_thirty_two() {
        for text in ["0", "33", "-1", "8.5", "", "four", "2147483648"] {
            assert_eq!(
                run(&["--wav", "a", "--asr-threads", text]).err(),
                Some(ArgError::AsrThreadsUnsupported),
                "{text:?}"
            );
        }
        assert_eq!(
            run(&["--wav", "a", "--asr-threads"]).err(),
            Some(ArgError::MissingValue)
        );
        assert_eq!(
            ArgError::AsrThreadsUnsupported.key(),
            "asr_threads_unsupported"
        );
    }

    /// T-BR-40-001
    #[test]
    fn draft_marks_starts_off_and_takes_on_or_off() -> Result<(), ArgError> {
        assert!(!run(&["--wav", "a"])?.draft_marks);
        assert!(run(&["--wav", "a", "--draft-marks", "on"])?.draft_marks);
        assert!(!run(&["--wav", "a", "--draft-marks", "off"])?.draft_marks);
        Ok(())
    }

    /// T-BR-40-002
    #[test]
    fn draft_marks_refuses_what_is_not_on_or_off() {
        for text in ["maybe", "", "ON", "1", "true"] {
            assert_eq!(
                run(&["--wav", "a", "--draft-marks", text]).err(),
                Some(ArgError::DraftMarksUnsupported),
                "{text:?}"
            );
        }
        assert_eq!(
            run(&["--wav", "a", "--draft-marks"]).err(),
            Some(ArgError::MissingValue)
        );
        assert_eq!(
            ArgError::DraftMarksUnsupported.key(),
            "draft_marks_unsupported"
        );
    }

    /// T-BR-43-015
    #[test]
    fn predecode_starts_off_and_takes_on_or_off() -> Result<(), ArgError> {
        assert!(!run(&["--wav", "a"])?.predecode);
        assert!(run(&["--wav", "a", "--predecode", "on"])?.predecode);
        assert!(!run(&["--wav", "a", "--predecode", "off"])?.predecode);
        Ok(())
    }

    /// T-BR-43-016
    #[test]
    fn predecode_refuses_what_is_not_on_or_off() {
        for text in ["maybe", "", "ON", "1", "true"] {
            assert_eq!(
                run(&["--wav", "a", "--predecode", text]).err(),
                Some(ArgError::PredecodeUnsupported),
                "{text:?}"
            );
        }
        assert_eq!(
            run(&["--wav", "a", "--predecode"]).err(),
            Some(ArgError::MissingValue)
        );
        assert_eq!(
            ArgError::PredecodeUnsupported.key(),
            "predecode_unsupported"
        );
    }

    /// T-IP-03-019
    #[test]
    fn a_run_with_mode_pair_uses_the_pair_policy() -> Result<(), ArgError> {
        assert_eq!(
            policy_of(&["--wav", "a", "--mode", "pair"])?,
            Policy::pair()
        );
        Ok(())
    }

    /// T-IP-03-020
    #[test]
    fn a_lang_wins_over_mode_pair() -> Result<(), ArgError> {
        assert_eq!(
            policy_of(&["--wav", "a", "--mode", "pair", "--lang", "ja"])?,
            Policy::single("ja")
        );
        Ok(())
    }

    /// T-IP-03-021
    #[test]
    fn a_run_without_mode_keeps_the_balanced_default_and_fast_and_single_are_unchanged()
    -> Result<(), ArgError> {
        assert_eq!(policy_of(&["--wav", "a"])?, Policy::balanced());
        assert_eq!(
            policy_of(&["--wav", "a", "--mode", "fast"])?,
            Policy::fast()
        );
        assert_eq!(
            policy_of(&["--wav", "a", "--mode", "single"])?,
            Policy::single("ja")
        );
        Ok(())
    }

    /// T-IP-03-022
    #[test]
    fn a_mode_name_outside_the_four_is_refused() {
        for name in ["paired", "Pair"] {
            assert_eq!(
                parse(&args(&["--wav", "a", "--mode", name])),
                Err(ArgError::ModeUnsupported),
                "{name}"
            );
        }
        assert_eq!(
            parse(&args(&["--wav", "a", "--mode"])),
            Err(ArgError::MissingValue)
        );
    }

    /// T-IP-03-023
    #[test]
    fn both_usage_texts_name_the_pair_mode_beside_the_three_old_ones() {
        let mut named = 0;
        for text in [crate::strings::EN, crate::strings::JA] {
            let lines = text.lines().filter(|line| {
                line.contains("--mode <mode>") || line.trim_start().starts_with("mode_unsupported")
            });
            for line in lines {
                named += 1;
                for word in ["pair", "balanced", "fast", "single"] {
                    assert!(line.contains(word), "`{word}` is not in: {line}");
                }
            }
        }
        assert_eq!(named, 4);
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
    fn serve_listens_on_the_loopback_unless_it_is_given_an_address() -> Result<(), ArgError> {
        assert_eq!(
            run(&["--wav", "a", "--serve"])?.serve_host,
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        );
        assert_eq!(
            run(&["--wav", "a", "--serve", "0", "--serve-host", "0.0.0.0"])?.serve_host,
            IpAddr::V4(Ipv4Addr::UNSPECIFIED)
        );
        assert_eq!(
            run(&["--serve-host", "192.168.200.20", "--wav", "a"])?.serve_host,
            IpAddr::V4(Ipv4Addr::new(192, 168, 200, 20))
        );
        Ok(())
    }

    #[test]
    fn serve_takes_each_allow_origin_it_is_given_and_refuses_what_is_not_an_origin()
    -> Result<(), ArgError> {
        assert!(run(&["--wav", "a"])?.allow_origins.is_empty());
        let given = run(&[
            "--wav",
            "a",
            "--allow-origin",
            "http://192.168.1.10:8833",
            "--allow-origin",
            "https://desk.test",
        ])?;
        let named: Vec<Origin> = ["http://192.168.1.10:8833", "https://desk.test"]
            .iter()
            .filter_map(|text| Origin::parse(text))
            .collect();
        assert_eq!(named.len(), 2);
        assert_eq!(given.allow_origins, named);
        for bad in ["null", "desk.test", "http://desk.test/x", "*"] {
            assert_eq!(
                parse(&args(&["--wav", "a", "--allow-origin", bad])),
                Err(ArgError::BadOrigin),
                "{bad}"
            );
        }
        assert_eq!(
            parse(&args(&["--wav", "a", "--allow-origin"])),
            Err(ArgError::MissingValue)
        );
        Ok(())
    }

    #[test]
    fn a_serve_host_that_is_not_an_address_or_has_no_value_is_refused() {
        assert_eq!(
            parse(&args(&["--wav", "a", "--serve-host", "lan"])),
            Err(ArgError::BadAddress)
        );
        assert_eq!(
            parse(&args(&["--wav", "a", "--serve-host"])),
            Err(ArgError::MissingValue)
        );
    }

    #[test]
    fn config_dump_reads_nothing_of_the_models_unless_asked() {
        assert_eq!(
            parse_config(&args(&["dump"])),
            Ok(ConfigDump {
                with_models: false,
                models: PathBuf::from("models"),
                out: None,
            })
        );
    }

    #[test]
    fn config_dump_takes_its_three_options() {
        let parsed = parse_config(&args(&[
            "dump",
            "--with-models",
            "--models",
            "m",
            "--out",
            "spec.json",
        ]));
        assert_eq!(
            parsed,
            Ok(ConfigDump {
                with_models: true,
                models: PathBuf::from("m"),
                out: Some(PathBuf::from("spec.json")),
            })
        );
    }

    #[test]
    fn config_without_dump_or_with_another_command_is_refused() {
        assert_eq!(parse_config(&[]), Err(ArgError::UnknownOption));
        assert_eq!(
            parse_config(&args(&["--with-models"])),
            Err(ArgError::UnknownOption)
        );
        assert_eq!(parse_config(&args(&["load"])), Err(ArgError::UnknownOption));
    }

    #[test]
    fn config_dump_refuses_an_unknown_option_and_a_missing_value() {
        assert_eq!(
            parse_config(&args(&["dump", "--bogus"])),
            Err(ArgError::UnknownOption)
        );
        assert_eq!(
            parse_config(&args(&["dump", "--out"])),
            Err(ArgError::MissingValue)
        );
        assert_eq!(
            parse_config(&args(&["dump", "--models"])),
            Err(ArgError::MissingValue)
        );
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

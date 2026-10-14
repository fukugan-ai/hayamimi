//! The command line: `live` (alias `run`), `eval`, `fx-fetch` and `fx-schedule`.

use std::fs::File;
use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::calendar;
use crate::deliver::{Outputs, Words};
use crate::eval;
use crate::follow::Backoff;
use crate::fx::{self, AtomicFileStore, FetchOptions, RATES_FILE_NAME};
use crate::hub;
use crate::lang::Lang;
use crate::llm::HttpLlm;
use crate::logging::{Level, LogEvent, LogSink, SharedSink, StdoutSink};
use crate::pipeline::{Pipeline, PipelineConfig};
use crate::relay::{self, Lanes, MakePipeline, Relay, RelayConfig};
use crate::schedule::{self, DEFAULT_TASK_NAME, TaskSpec};
use crate::strings::Strings;

/// The streams `run` follows when none is named: the microphone's recognizer
/// and the loopback's recognizer, on the ports the recognizers serve on.
pub const DEFAULT_EVENTS: [&str; 2] = [
    "http://127.0.0.1:8833/events",
    "http://127.0.0.1:8834/events",
];

/// The translator `live` calls when `--llm` is not given: llama-server on its
/// default port.
pub const DEFAULT_LLM: &str = "http://127.0.0.1:8080";

/// The port `live` serves on when `--serve` is not given: the one after the
/// two recognizers' subtitle ports. A taken port moves to the next free one.
pub const DEFAULT_SERVE: u16 = 8835;

/// What `fx-schedule` was asked to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScheduleAction {
    Install,
    Remove,
}

/// What `live` was asked to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Live {
    pub events: Vec<String>,
    pub llm: String,
    pub serve: u16,
    /// Where to write the settled translations as SRT cues.
    pub srt: Option<PathBuf>,
    /// The `--translate` codes, when the option is given.
    pub translate: Option<Vec<String>>,
}

/// A parsed command line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Follow the recognizers' events and serve the translations.
    Run(Live),
    FxFetch {
        out: Option<PathBuf>,
        source_url: Option<String>,
    },
    FxSchedule {
        action: ScheduleAction,
        task_name: Option<String>,
        out: Option<PathBuf>,
        source_url: Option<String>,
    },
    /// Translate sentence pairs for a scorer.
    Eval {
        pairs: PathBuf,
        out: PathBuf,
        llm: String,
    },
}

/// Why a command line was refused. Each variant names a key of the
/// locale's `cli` string table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CliError {
    UnknownCommand,
    MissingValue,
    UnknownOption,
    NeedInstallOrRemove,
    BadPort,
    NeedPairsAndOut,
}

impl CliError {
    /// The key into the locale's `cli` string table.
    pub fn key(self) -> &'static str {
        match self {
            CliError::UnknownCommand => "unknown_command",
            CliError::MissingValue => "missing_value",
            CliError::UnknownOption => "unknown_option",
            CliError::NeedInstallOrRemove => "need_install_or_remove",
            CliError::BadPort => "bad_port",
            CliError::NeedPairsAndOut => "need_pairs_and_out",
        }
    }
}

/// The codes of a `--translate` value: comma separated, blanks around a code
/// dropped, empty codes dropped.
fn translate_codes(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|code| !code.is_empty())
        .map(str::to_owned)
        .collect()
}

/// What `live` was given. `llm` and `serve` start at the design's defaults;
/// `events` stays empty until named and then falls to [`DEFAULT_EVENTS`].
fn parse_run_options(args: &[String]) -> Result<Live, CliError> {
    let mut options = Live {
        events: Vec::new(),
        llm: DEFAULT_LLM.to_owned(),
        serve: DEFAULT_SERVE,
        srt: None,
        translate: None,
    };
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let option = arg.as_str();
        if !matches!(
            option,
            "--events" | "--llm" | "--serve" | "--srt" | "--translate"
        ) {
            return Err(CliError::UnknownOption);
        }
        let value = rest.next().ok_or(CliError::MissingValue)?;
        match option {
            "--events" => options.events.push(value.clone()),
            "--llm" => options.llm.clone_from(value),
            "--srt" => options.srt = Some(PathBuf::from(value)),
            "--translate" => {
                let codes = translate_codes(value);
                if codes.is_empty() {
                    return Err(CliError::MissingValue);
                }
                options.translate.get_or_insert_with(Vec::new).extend(codes);
            }
            _ => options.serve = value.parse().map_err(|_| CliError::BadPort)?,
        }
    }
    Ok(options)
}

fn parse_run(args: &[String]) -> Result<Command, CliError> {
    let mut live = parse_run_options(args)?;
    if live.events.is_empty() {
        live.events = DEFAULT_EVENTS.map(str::to_owned).to_vec();
    }
    Ok(Command::Run(live))
}

fn parse_eval(args: &[String]) -> Result<Command, CliError> {
    let (mut pairs, mut out) = (None, None);
    let mut llm = DEFAULT_LLM.to_owned();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let slot = match arg.as_str() {
            "--pairs" => &mut pairs,
            "--out" => &mut out,
            "--llm" => {
                llm.clone_from(rest.next().ok_or(CliError::MissingValue)?);
                continue;
            }
            _ => return Err(CliError::UnknownOption),
        };
        *slot = Some(PathBuf::from(rest.next().ok_or(CliError::MissingValue)?));
    }
    match (pairs, out) {
        (Some(pairs), Some(out)) => Ok(Command::Eval { pairs, out, llm }),
        _ => Err(CliError::NeedPairsAndOut),
    }
}

#[derive(Default)]
struct Options {
    out: Option<PathBuf>,
    source_url: Option<String>,
    task_name: Option<String>,
    install: bool,
    remove: bool,
}

fn parse_options(args: &[String], allow_schedule: bool) -> Result<Options, CliError> {
    let mut options = Options::default();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--out" => {
                options.out = Some(PathBuf::from(rest.next().ok_or(CliError::MissingValue)?))
            }
            "--source-url" => {
                options.source_url = Some(rest.next().ok_or(CliError::MissingValue)?.clone())
            }
            "--task-name" if allow_schedule => {
                options.task_name = Some(rest.next().ok_or(CliError::MissingValue)?.clone());
            }
            "--install" if allow_schedule => options.install = true,
            "--remove" if allow_schedule => options.remove = true,
            _ => return Err(CliError::UnknownOption),
        }
    }
    Ok(options)
}

/// Parses the arguments after the program name.
pub fn parse_args(args: &[String]) -> Result<Command, CliError> {
    let Some((name, rest)) = args.split_first() else {
        return Err(CliError::UnknownCommand);
    };
    match name.as_str() {
        // `live` is the name in the design; `run` stays as an alias.
        "live" | "run" => parse_run(rest),
        "eval" => parse_eval(rest),
        "fx-fetch" => {
            let options = parse_options(rest, false)?;
            Ok(Command::FxFetch {
                out: options.out,
                source_url: options.source_url,
            })
        }
        "fx-schedule" => {
            let options = parse_options(rest, true)?;
            let action = match (options.install, options.remove) {
                (true, false) => ScheduleAction::Install,
                (false, true) => ScheduleAction::Remove,
                _ => return Err(CliError::NeedInstallOrRemove),
            };
            Ok(Command::FxSchedule {
                action,
                task_name: options.task_name,
                out: options.out,
                source_url: options.source_url,
            })
        }
        _ => Err(CliError::UnknownCommand),
    }
}

/// The rate file in the user's local data directory; `None` when the
/// platform names no such directory.
pub fn default_rates_path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|dir| dir.join("hayamimi").join(RATES_FILE_NAME))
}

fn print_error(strings: &Strings, key: &str) {
    eprintln!("{}", strings.cli_text(key));
    eprintln!("{}", strings.cli_text("usage"));
}

fn run_fetch(
    out: Option<PathBuf>,
    source_url: Option<String>,
    now: i64,
    sink: &mut dyn LogSink,
    strings: &Strings,
) -> u8 {
    let Some(out) = out.or_else(default_rates_path) else {
        print_error(strings, "no_default_dir");
        return 2;
    };
    let source = match source_url {
        Some(url) => url,
        None => fx::DEFAULT_SOURCE_URL.to_owned(),
    };
    let options = FetchOptions {
        source_url: &source,
        out: &out,
    };
    u8::from(fx::fetch(&options, now, &AtomicFileStore, sink).is_err())
}

fn run_schedule(
    action: ScheduleAction,
    task_name: Option<String>,
    out: Option<PathBuf>,
    source_url: Option<String>,
    now: i64,
    sink: &mut dyn LogSink,
    strings: &Strings,
) -> u8 {
    let name = match task_name {
        Some(name) => name,
        None => DEFAULT_TASK_NAME.to_owned(),
    };
    let result = match action {
        ScheduleAction::Remove => schedule::remove(&name, sink),
        ScheduleAction::Install => {
            let Some(out) = out.or_else(default_rates_path) else {
                print_error(strings, "no_default_dir");
                return 2;
            };
            let Ok(exe) = std::env::current_exe() else {
                return 1;
            };
            let spec = TaskSpec {
                name: &name,
                exe: &exe,
                out: &out,
                source_url: source_url.as_deref(),
            };
            schedule::install(&spec, calendar::jst_date(now), sink)
        }
    };
    u8::from(result.is_err())
}

/// The per-user directory of table files that override the built-in ones.
fn default_tables_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("hayamimi"))
}

/// Builds the pipeline of one lane against the llama-server at `llm_url`.
fn pipeline_factory(llm_url: String) -> MakePipeline<HttpLlm> {
    Arc::new(move |sink: &mut dyn LogSink| {
        let config = PipelineConfig {
            rates_path: default_rates_path(),
            tables_dir: default_tables_dir(),
        };
        Pipeline::open(HttpLlm::new(&llm_url), &config, sink)
    })
}

/// Says where the translations are served, and that the port moved when it did.
fn announce_serving(
    requested: u16,
    address: SocketAddr,
    sink: &mut dyn LogSink,
    strings: &Strings,
) {
    let moved = requested != 0 && address.port() != requested;
    let (event, key) = if moved {
        (
            LogEvent::new(Level::Warn, "server.fallback.port", "transport")
                .with("reason", "in_use"),
            "port_moved",
        )
    } else {
        (
            LogEvent::new(Level::Info, "relay.serve.started", "transport"),
            "serving_at",
        )
    };
    sink.emit(event.with("port", address.port()));
    eprintln!("{} http://{address}/events", strings.cli_text(key));
}

/// Opens the subtitle file. A file that cannot be opened is left out and the
/// run goes on without it.
fn open_srt(
    path: &Path,
    sink: &mut dyn LogSink,
    strings: &Strings,
) -> Option<Box<dyn Write + Send>> {
    match File::create(path) {
        Ok(file) => Some(Box::new(file)),
        Err(_) => {
            sink.emit(
                LogEvent::new(Level::Error, "srt.output.failed", "storage").with_error("SRT_OPEN"),
            );
            eprintln!("{}", strings.cli_text("srt_not_opened"));
            None
        }
    }
}

/// Starts the relay `live` asks for and returns its handle. The relay works
/// in threads of its own; the caller keeps the handle, and stops the relay
/// with it or lets it end with the process.
pub fn start_live(
    live: &Live,
    now: i64,
    strings: &Strings,
    sink: &SharedSink,
) -> std::io::Result<Relay> {
    let mut log = sink.clone();
    let listener = hub::bind(live.serve)?;
    let config = RelayConfig {
        events: live.events.clone(),
        lanes: Lanes::PerSource,
        backoff: Backoff::STANDARD,
        started_epoch: now,
        translate: live.translate.clone(),
    };
    let outputs = Outputs {
        destinations: Vec::new(),
        srt: live
            .srt
            .as_deref()
            .and_then(|path| open_srt(path, &mut log, strings)),
        words: Words::from_strings(strings),
    };
    relay::start_handle(
        listener,
        config,
        pipeline_factory(live.llm.clone()),
        log,
        outputs,
    )
}

fn run_relay(live: &Live, now: i64, strings: &Strings) -> u8 {
    let mut sink = SharedSink::new(StdoutSink::new(strings.clone()));
    let Ok(started) = start_live(live, now, strings, &sink) else {
        print_error(strings, "cannot_serve");
        return 1;
    };
    for code in started.skipped() {
        eprintln!("{} {code}", strings.cli_text("eval_target_skipped"));
    }
    announce_serving(live.serve, started.address(), &mut sink, strings);
    // The relay works in threads of its own and ends with the process.
    loop {
        std::thread::park();
    }
}

/// The time one request of `eval` may take: a measurement waits for a slow
/// answer where a live subtitle does not.
const EVAL_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// Translates the pairs in `pairs` into `out`. A language that cannot be
/// translated is said to be left out; any other failure stops the run.
fn run_eval(
    pairs: &Path,
    out: &Path,
    llm_url: &str,
    sink: &mut dyn LogSink,
    strings: &Strings,
) -> u8 {
    let llm = HttpLlm::with_timeout(llm_url, EVAL_REQUEST_TIMEOUT);
    match eval::run(&llm, &eval::EvalOptions { pairs, out }, sink) {
        Ok(report) => {
            for code in &report.skipped {
                eprintln!("{} {code}", strings.cli_text("eval_target_skipped"));
            }
            0
        }
        Err(error) => {
            eprintln!("{}: {error}", strings.cli_text("eval_failed"));
            1
        }
    }
}

/// Runs a command line. Returns the process exit code: 0 done, 1 failed,
/// 2 the command line was refused. `run` does not return.
pub fn run(args: &[String], now_epoch: i64, locale_tag: Option<&str>) -> u8 {
    // A machine that reports no locale is read as English, the documented default.
    let lang = match locale_tag {
        Some(tag) => Lang::from_locale_tag(tag),
        None => Lang::En,
    };
    let Ok(strings) = Strings::embedded(lang) else {
        return 1;
    };
    let command = match parse_args(args) {
        Ok(command) => command,
        Err(error) => {
            print_error(&strings, error.key());
            return 2;
        }
    };
    let mut sink = StdoutSink::new(strings.clone());
    match command {
        Command::Run(live) => run_relay(&live, now_epoch, &strings),
        Command::FxFetch { out, source_url } => {
            run_fetch(out, source_url, now_epoch, &mut sink, &strings)
        }
        Command::FxSchedule {
            action,
            task_name,
            out,
            source_url,
        } => run_schedule(
            action, task_name, out, source_url, now_epoch, &mut sink, &strings,
        ),
        Command::Eval { pairs, out, llm } => run_eval(&pairs, &out, &llm, &mut sink, &strings),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn fx_fetch_takes_out_and_source() {
        let parsed = parse_args(&args(&[
            "fx-fetch",
            "--out",
            "a.json",
            "--source-url",
            "http://x",
        ]));
        assert_eq!(
            parsed,
            Ok(Command::FxFetch {
                out: Some(PathBuf::from("a.json")),
                source_url: Some(String::from("http://x"))
            })
        );
    }

    #[test]
    fn fx_schedule_needs_exactly_one_action() {
        assert_eq!(
            parse_args(&args(&["fx-schedule"])),
            Err(CliError::NeedInstallOrRemove)
        );
        assert_eq!(
            parse_args(&args(&["fx-schedule", "--install", "--remove"])),
            Err(CliError::NeedInstallOrRemove)
        );
        assert!(matches!(
            parse_args(&args(&["fx-schedule", "--remove", "--task-name", "t"])),
            Ok(Command::FxSchedule {
                action: ScheduleAction::Remove,
                ..
            })
        ));
    }

    #[test]
    fn bad_command_lines_are_refused_with_a_key() {
        assert_eq!(parse_args(&[]), Err(CliError::UnknownCommand));
        assert_eq!(parse_args(&args(&["nope"])), Err(CliError::UnknownCommand));
        assert_eq!(
            parse_args(&args(&["fx-fetch", "--out"])),
            Err(CliError::MissingValue)
        );
        assert_eq!(
            parse_args(&args(&["fx-fetch", "--install"])),
            Err(CliError::UnknownOption)
        );
        assert_eq!(CliError::MissingValue.key(), "missing_value");
    }

    #[test]
    fn a_refused_command_line_exits_with_two() {
        assert_eq!(run(&args(&["nope"]), 0, Some("en")), 2);
        assert_eq!(run(&args(&["nope"]), 0, None), 2);
    }

    #[test]
    fn run_takes_the_translator_the_port_and_any_number_of_streams() {
        let parsed = parse_args(&args(&[
            "run",
            "--llm",
            "http://x:1",
            "--serve",
            "9100",
            "--events",
            "http://a/events",
            "--events",
            "http://b/events",
        ]));
        assert_eq!(
            parsed,
            Ok(Command::Run(Live {
                events: vec![
                    String::from("http://a/events"),
                    String::from("http://b/events")
                ],
                llm: String::from("http://x:1"),
                serve: 9100,
                srt: None,
                translate: None,
            }))
        );
    }

    #[test]
    fn run_takes_the_file_the_subtitles_are_written_to() {
        let parsed = parse_args(&args(&[
            "live", "--llm", "u", "--serve", "0", "--srt", "live.srt",
        ]));
        assert!(matches!(
            parsed,
            Ok(Command::Run(Live { srt: Some(ref path), .. })) if path == Path::new("live.srt")
        ));
        assert_eq!(
            parse_args(&args(&["run", "--llm", "u", "--serve", "0", "--srt"])),
            Err(CliError::MissingValue)
        );
    }

    #[test]
    fn a_subtitle_file_that_cannot_be_opened_is_left_out_and_said() {
        let mut sink = crate::logging::MemorySink::default();
        let Ok(strings) = Strings::embedded(Lang::En) else {
            panic!("embedded table missing");
        };
        let missing = Path::new("/no/such/directory/live.srt");
        assert!(open_srt(missing, &mut sink, &strings).is_none());
        assert_eq!(
            sink.events
                .iter()
                .map(|e| e.event.as_str())
                .collect::<Vec<_>>(),
            ["srt.output.failed"]
        );
    }

    #[test]
    fn live_is_the_name_and_run_is_its_alias() {
        let rest = ["--serve", "9100", "--llm", "http://x:1"];
        let with = |name: &'static str| {
            let mut line = vec![name];
            line.extend(rest);
            parse_args(&args(&line))
        };
        assert!(matches!(with("live"), Ok(Command::Run(_))));
        assert_eq!(with("live"), with("run"));
    }

    #[test]
    fn run_without_streams_follows_the_two_recognizer_ports() {
        let parsed = parse_args(&args(&["run", "--serve", "0", "--llm", "http://x:1"]));
        let Ok(Command::Run(live)) = parsed else {
            panic!("run was refused: {parsed:?}");
        };
        assert_eq!(live.events, DEFAULT_EVENTS);
    }

    #[test]
    fn live_without_options_uses_the_designed_defaults() {
        let expected = Command::Run(Live {
            events: DEFAULT_EVENTS.map(str::to_owned).to_vec(),
            llm: String::from("http://127.0.0.1:8080"),
            serve: 8835,
            srt: None,
            translate: None,
        });
        assert_eq!(parse_args(&args(&["live"])), Ok(expected));
    }

    #[test]
    fn one_named_option_leaves_the_other_defaults() {
        let parsed = parse_args(&args(&["live", "--serve", "9100"]));
        assert!(matches!(
            parsed,
            Ok(Command::Run(Live { ref llm, serve: 9100, .. })) if llm == DEFAULT_LLM
        ));
        let parsed = parse_args(&args(&["live", "--llm", "http://x:1"]));
        assert!(matches!(
            parsed,
            Ok(Command::Run(Live {
                serve: DEFAULT_SERVE,
                ..
            }))
        ));
    }

    fn translate_of(list: &[&str]) -> Result<Option<Vec<String>>, CliError> {
        match parse_args(&args(list))? {
            Command::Run(live) => Ok(live.translate),
            _ => Err(CliError::UnknownCommand),
        }
    }

    #[test]
    fn translate_takes_comma_separated_codes_and_may_repeat() {
        let codes = |list: &[&str]| list.iter().map(|c| (*c).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            translate_of(&["live", "--translate", "en,zh"]),
            Ok(Some(codes(&["en", "zh"])))
        );
        assert_eq!(
            translate_of(&["live", "--translate", " zh , ko ,", "--translate", "es"]),
            Ok(Some(codes(&["zh", "ko", "es"])))
        );
        assert_eq!(translate_of(&["live"]), Ok(None));
    }

    #[test]
    fn translate_without_a_code_is_refused() {
        assert_eq!(
            translate_of(&["live", "--translate"]),
            Err(CliError::MissingValue)
        );
        assert_eq!(
            translate_of(&["live", "--translate", " , "]),
            Err(CliError::MissingValue)
        );
    }

    #[test]
    fn run_refuses_a_port_that_is_not_a_port_number() {
        for port in ["abc", "-1", "65536", ""] {
            let parsed = parse_args(&args(&["run", "--llm", "u", "--serve", port]));
            assert_eq!(parsed, Err(CliError::BadPort), "{port:?}");
        }
        assert!(parse_args(&args(&["run", "--llm", "u", "--serve", "65535"])).is_ok());
    }

    #[test]
    fn run_refuses_an_unknown_option_and_a_missing_value() {
        assert_eq!(
            parse_args(&args(&["run", "--llm", "u", "--serve", "1", "--install"])),
            Err(CliError::UnknownOption)
        );
        assert_eq!(
            parse_args(&args(&["run", "--llm"])),
            Err(CliError::MissingValue)
        );
    }

    #[test]
    fn the_run_refusals_have_text_in_both_locales() {
        for lang in [Lang::Ja, Lang::En] {
            let Ok(strings) = Strings::embedded(lang) else {
                panic!("embedded table missing");
            };
            for key in [
                CliError::BadPort.key(),
                "cannot_serve",
                "serving_at",
                "port_moved",
                "sink_local",
                "sink_cleared",
                "srt_not_opened",
            ] {
                assert_ne!(strings.cli_text(key), key, "{lang:?} {key}");
            }
        }
    }

    #[test]
    fn serving_events_are_worded_in_both_locales() {
        for lang in [Lang::Ja, Lang::En] {
            let Ok(strings) = Strings::embedded(lang) else {
                panic!("embedded table missing");
            };
            for event in [
                "events.subscribe.opened",
                "events.subscribe.lost",
                "events.line.skipped",
                "translate.line.skipped",
                "translate.lane.failed",
                "translate.publish.failed",
                "relay.serve.started",
                "server.fallback.port",
                "translate.edit.held",
                "translate.send.ok",
                "translate.send.failed",
                "translate.deliver.failed",
                "sink.fallback.local",
                "sink.fallback.cleared",
                "srt.cue.skipped",
                "srt.output.failed",
                "translate.fallback.skipped",
                "translate.eval.written",
                "translate.eval.held",
            ] {
                assert_ne!(strings.message_for(event), event, "{lang:?} {event}");
            }
        }
    }

    #[test]
    fn every_command_line_text_of_eval_exists_in_both_locales() {
        for lang in [Lang::Ja, Lang::En] {
            let Ok(strings) = Strings::embedded(lang) else {
                panic!("embedded table missing");
            };
            for key in [
                CliError::NeedPairsAndOut.key(),
                "eval_target_skipped",
                "eval_failed",
            ] {
                assert_ne!(strings.cli_text(key), key, "{lang:?} {key}");
            }
            assert!(
                strings
                    .cli_text("usage")
                    .contains("hayamimi-relay eval --pairs")
            );
        }
    }

    #[test]
    fn eval_takes_the_pairs_the_output_and_the_translator() {
        let parsed = parse_args(&args(&[
            "eval",
            "--pairs",
            "pairs",
            "--out",
            "hyps",
            "--llm",
            "http://x:1",
        ]));
        assert_eq!(
            parsed,
            Ok(Command::Eval {
                pairs: PathBuf::from("pairs"),
                out: PathBuf::from("hyps"),
                llm: String::from("http://x:1"),
            })
        );
    }

    #[test]
    fn eval_asks_the_default_translator_when_none_is_named() {
        let parsed = parse_args(&args(&["eval", "--out", "hyps", "--pairs", "pairs"]));
        assert!(matches!(
            parsed,
            Ok(Command::Eval { ref llm, .. }) if llm == DEFAULT_LLM
        ));
    }

    #[test]
    fn eval_needs_both_directories() {
        assert_eq!(
            parse_args(&args(&["eval", "--pairs", "pairs"])),
            Err(CliError::NeedPairsAndOut)
        );
        assert_eq!(
            parse_args(&args(&["eval", "--out", "hyps"])),
            Err(CliError::NeedPairsAndOut)
        );
        assert_eq!(parse_args(&args(&["eval"])), Err(CliError::NeedPairsAndOut));
    }

    #[test]
    fn eval_refuses_an_unknown_option_and_a_missing_value() {
        assert_eq!(
            parse_args(&args(&["eval", "--serve", "1"])),
            Err(CliError::UnknownOption)
        );
        assert_eq!(
            parse_args(&args(&["eval", "--pairs"])),
            Err(CliError::MissingValue)
        );
        assert_eq!(
            parse_args(&args(&["eval", "--llm"])),
            Err(CliError::MissingValue)
        );
    }

    #[test]
    fn the_default_rates_path_ends_with_the_fixed_name() {
        assert!(default_rates_path().is_none_or(|path| path.ends_with("hayamimi/fx_rates.json")));
    }
}

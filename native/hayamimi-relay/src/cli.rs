//! The command line: `live` (also `run`), `srt`, `fx-fetch` and `fx-schedule`.

use std::fs::File;
use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::calendar;
use crate::captions::{HttpCaptions, Kind};
use crate::deliver::{Destination, Outputs, Words};
use crate::follow::Backoff;
use crate::fx::{self, AtomicFileStore, FetchOptions, RATES_FILE_NAME};
use crate::hub;
use crate::lang::Lang;
use crate::llm::HttpLlm;
use crate::logging::{Level, LogEvent, LogSink, SharedSink, StdoutSink};
use crate::partial::PartialTranslation;
use crate::pipeline::{Pipeline, PipelineConfig};
use crate::relay::{self, Lanes, MakePipeline, RelayConfig};
use crate::schedule::{self, DEFAULT_TASK_NAME, TaskSpec};
use crate::send_config::SendFile;
use crate::srt_command::{self, SrtPlan};
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

/// A parsed command line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Follow the recognizers' events and serve the translations.
    Run {
        events: Vec<String>,
        llm: String,
        serve: u16,
        /// Where to write the settled translations as SRT cues.
        srt: Option<PathBuf>,
        /// The file that names the Zoom token and the Teams link; without it
        /// nothing is sent outside.
        send: Option<PathBuf>,
        /// Whether the text of an utterance in progress is translated too.
        partial_translation: PartialTranslation,
    },
    /// Make the subtitle files of a video.
    Srt(SrtPlan),
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
    NeedVideo,
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
            CliError::NeedVideo => "need_video",
        }
    }
}

/// What `live` was given. `llm` and `serve` start at the design's defaults;
/// `events` stays empty until named and then falls to [`DEFAULT_EVENTS`].
struct RunOptions {
    events: Vec<String>,
    llm: String,
    serve: u16,
    srt: Option<PathBuf>,
    send: Option<PathBuf>,
    partial_translation: PartialTranslation,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            events: Vec::new(),
            llm: DEFAULT_LLM.to_owned(),
            serve: DEFAULT_SERVE,
            srt: None,
            send: None,
            partial_translation: PartialTranslation::On,
        }
    }
}

fn parse_run_options(args: &[String]) -> Result<RunOptions, CliError> {
    let mut options = RunOptions::default();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let option = arg.as_str();
        if option == "--no-partial-translation" {
            options.partial_translation = PartialTranslation::Off;
            continue;
        }
        if !matches!(
            option,
            "--events" | "--llm" | "--serve" | "--srt" | "--send"
        ) {
            return Err(CliError::UnknownOption);
        }
        let value = rest.next().ok_or(CliError::MissingValue)?;
        match option {
            "--events" => options.events.push(value.clone()),
            "--llm" => options.llm.clone_from(value),
            "--srt" => options.srt = Some(PathBuf::from(value)),
            "--send" => options.send = Some(PathBuf::from(value)),
            _ => options.serve = value.parse().map_err(|_| CliError::BadPort)?,
        }
    }
    Ok(options)
}

fn parse_run(args: &[String]) -> Result<Command, CliError> {
    let options = parse_run_options(args)?;
    let events = if options.events.is_empty() {
        DEFAULT_EVENTS.map(str::to_owned).to_vec()
    } else {
        options.events
    };
    Ok(Command::Run {
        events,
        llm: options.llm,
        serve: options.serve,
        srt: options.srt,
        send: options.send,
        partial_translation: options.partial_translation,
    })
}

/// `srt <video> [--llm <url>] [--ffmpeg <program>] [--hayamimi <program>]
/// [--models <dir>]`.
fn parse_srt(args: &[String]) -> Result<Command, CliError> {
    let (video, rest) = args.split_first().ok_or(CliError::NeedVideo)?;
    if video.starts_with("--") {
        return Err(CliError::NeedVideo);
    }
    let mut plan = SrtPlan::new(PathBuf::from(video), DEFAULT_LLM);
    let mut rest = rest.iter();
    while let Some(arg) = rest.next() {
        let option = arg.as_str();
        if !matches!(option, "--llm" | "--ffmpeg" | "--hayamimi" | "--models") {
            return Err(CliError::UnknownOption);
        }
        let value = rest.next().ok_or(CliError::MissingValue)?;
        match option {
            "--llm" => plan.llm.clone_from(value),
            "--ffmpeg" => plan.ffmpeg = PathBuf::from(value),
            "--hayamimi" => plan.hayamimi = PathBuf::from(value),
            _ => plan.models = Some(PathBuf::from(value)),
        }
    }
    Ok(Command::Srt(plan))
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
        "srt" => parse_srt(rest),
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

pub(crate) fn print_error(strings: &Strings, key: &str) {
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
pub(crate) fn default_tables_dir() -> Option<PathBuf> {
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

/// The Zoom and Teams destinations. Both read `path` for their addresses
/// every time a caption goes out, so the file may not be there yet.
fn send_destinations(
    path: Option<PathBuf>,
    sink: &SharedSink,
) -> Result<Vec<Box<dyn Destination>>, String> {
    let Some(path) = path else {
        return Ok(Vec::new());
    };
    [Kind::Zoom, Kind::Teams]
        .into_iter()
        .map(|kind| {
            let file = SendFile::open(path.clone(), sink.clone())?;
            Ok(Box::new(HttpCaptions::new(kind, file)) as Box<dyn Destination>)
        })
        .collect()
}

/// What `live` runs with, apart from the clock and the locale.
struct RunPlan {
    events: Vec<String>,
    llm: String,
    serve: u16,
    srt: Option<PathBuf>,
    send: Option<PathBuf>,
    partial_translation: PartialTranslation,
}

fn run_relay(plan: RunPlan, now: i64, strings: &Strings) -> u8 {
    let RunPlan {
        events,
        llm,
        serve,
        srt,
        send,
        partial_translation,
    } = plan;
    let mut sink = SharedSink::new(StdoutSink::new(strings.clone()));
    let Ok(listener) = hub::bind(serve) else {
        print_error(strings, "cannot_serve");
        return 1;
    };
    let Ok(destinations) = send_destinations(send, &sink) else {
        print_error(strings, "send_not_opened");
        return 1;
    };
    let config = RelayConfig {
        events,
        lanes: Lanes::PerSource,
        backoff: Backoff::STANDARD,
        started_epoch: now,
    };
    let outputs = Outputs {
        destinations,
        srt: srt.and_then(|path| open_srt(&path, &mut sink, strings)),
        words: Words::from_strings(strings),
    };
    let started = relay::start_partials(
        listener,
        config,
        pipeline_factory(llm),
        sink.clone(),
        outputs,
        partial_translation,
    );
    let Ok(address) = started else {
        print_error(strings, "cannot_serve");
        return 1;
    };
    announce_serving(serve, address, &mut sink, strings);
    // The relay works in threads of its own and ends with the process.
    loop {
        std::thread::park();
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
        Command::Run {
            events,
            llm,
            serve,
            srt,
            send,
            partial_translation,
        } => {
            let plan = RunPlan {
                events,
                llm,
                serve,
                srt,
                send,
                partial_translation,
            };
            run_relay(plan, now_epoch, &strings)
        }
        Command::Srt(plan) => srt_command::run(plan, now_epoch, &strings),
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
            Ok(Command::Run {
                events: vec![
                    String::from("http://a/events"),
                    String::from("http://b/events")
                ],
                llm: String::from("http://x:1"),
                serve: 9100,
                srt: None,
                send: None,
                partial_translation: PartialTranslation::On,
            })
        );
    }

    #[test]
    fn the_translation_of_the_text_in_progress_is_on_unless_it_is_turned_off() {
        let on = |list: &[&str]| match parse_args(&args(list)) {
            Ok(Command::Run {
                partial_translation,
                ..
            }) => Some(partial_translation),
            _ => None,
        };
        assert_eq!(on(&["live"]), Some(PartialTranslation::On));
        assert_eq!(
            on(&["live", "--no-partial-translation"]),
            Some(PartialTranslation::Off)
        );
        assert_eq!(
            on(&[
                "live",
                "--serve",
                "0",
                "--no-partial-translation",
                "--srt",
                "a.srt"
            ]),
            Some(PartialTranslation::Off)
        );
    }

    #[test]
    fn the_switch_that_turns_the_translation_off_takes_no_value() {
        let parsed = parse_args(&args(&["live", "--no-partial-translation", "yes"]));
        assert_eq!(parsed, Err(CliError::UnknownOption));
    }

    #[test]
    fn run_takes_the_file_the_subtitles_are_written_to() {
        let parsed = parse_args(&args(&[
            "live", "--llm", "u", "--serve", "0", "--srt", "live.srt",
        ]));
        assert!(matches!(
            parsed,
            Ok(Command::Run { srt: Some(ref path), .. }) if path == Path::new("live.srt")
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
        assert!(matches!(with("live"), Ok(Command::Run { .. })));
        assert_eq!(with("live"), with("run"));
    }

    #[test]
    fn run_without_streams_follows_the_two_recognizer_ports() {
        let parsed = parse_args(&args(&["run", "--serve", "0", "--llm", "http://x:1"]));
        let Ok(Command::Run { events, .. }) = parsed else {
            panic!("run was refused: {parsed:?}");
        };
        assert_eq!(events, DEFAULT_EVENTS);
    }

    #[test]
    fn live_without_options_uses_the_designed_defaults() {
        let expected = Command::Run {
            events: DEFAULT_EVENTS.map(str::to_owned).to_vec(),
            llm: String::from("http://127.0.0.1:8080"),
            serve: 8835,
            srt: None,
            send: None,
            partial_translation: PartialTranslation::On,
        };
        assert_eq!(parse_args(&args(&["live"])), Ok(expected));
    }

    #[test]
    fn live_sends_outside_only_when_it_is_given_the_file() {
        let parsed = parse_args(&args(&["live", "--send", "send.toml"]));
        assert!(matches!(
            parsed,
            Ok(Command::Run { send: Some(ref path), .. }) if path == Path::new("send.toml")
        ));
        assert_eq!(
            parse_args(&args(&["live", "--send"])),
            Err(CliError::MissingValue)
        );
    }

    #[test]
    fn srt_takes_a_video_and_leaves_the_other_defaults() {
        let parsed = parse_args(&args(&["srt", "talk.mp4"]));
        assert_eq!(
            parsed,
            Ok(Command::Srt(SrtPlan::new(
                PathBuf::from("talk.mp4"),
                DEFAULT_LLM
            )))
        );
    }

    #[test]
    fn srt_takes_the_programs_the_translator_and_the_models() {
        let parsed = parse_args(&args(&[
            "srt",
            "talk.mp4",
            "--llm",
            "http://x:1",
            "--ffmpeg",
            "f",
            "--hayamimi",
            "h",
            "--models",
            "m",
        ]));
        let Ok(Command::Srt(plan)) = parsed else {
            panic!("srt was refused: {parsed:?}");
        };
        assert_eq!(plan.llm, "http://x:1");
        assert_eq!(plan.ffmpeg, PathBuf::from("f"));
        assert_eq!(plan.hayamimi, PathBuf::from("h"));
        assert_eq!(plan.models, Some(PathBuf::from("m")));
    }

    #[test]
    fn srt_needs_a_video_and_refuses_what_it_does_not_know() {
        assert_eq!(parse_args(&args(&["srt"])), Err(CliError::NeedVideo));
        assert_eq!(
            parse_args(&args(&["srt", "--llm", "u"])),
            Err(CliError::NeedVideo)
        );
        assert_eq!(
            parse_args(&args(&["srt", "a.mp4", "--serve", "1"])),
            Err(CliError::UnknownOption)
        );
        assert_eq!(
            parse_args(&args(&["srt", "a.mp4", "--llm"])),
            Err(CliError::MissingValue)
        );
        assert_eq!(CliError::NeedVideo.key(), "need_video");
    }

    #[test]
    fn one_named_option_leaves_the_other_defaults() {
        let parsed = parse_args(&args(&["live", "--serve", "9100"]));
        assert!(matches!(
            parsed,
            Ok(Command::Run { ref llm, serve: 9100, .. }) if llm == DEFAULT_LLM
        ));
        let parsed = parse_args(&args(&["live", "--llm", "http://x:1"]));
        assert!(matches!(
            parsed,
            Ok(Command::Run {
                serve: DEFAULT_SERVE,
                ..
            })
        ));
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
                "srt.output.exists",
                "srt.extract.started",
                "srt.extract.failed",
                "srt.recognize.failed",
                "srt.output.empty",
                "srt.output.written",
                "send.config.too_open",
                "translate.partial.ok",
                "translate.partial.held",
                "translate.partial.aborted",
                "translate.partial.failed",
                "translate.partial.skipped",
                "translate.partial.kept",
            ] {
                assert_ne!(strings.message_for(event), event, "{lang:?} {event}");
            }
        }
    }

    #[test]
    fn the_default_rates_path_ends_with_the_fixed_name() {
        assert!(default_rates_path().is_none_or(|path| path.ends_with("hayamimi/fx_rates.json")));
    }
}

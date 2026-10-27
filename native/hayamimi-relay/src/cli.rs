//! The command line: `live` (also `run`), `eval`, `srt`, `fx-fetch` and
//! `fx-schedule`.

use std::fs::File;
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::calendar;
use crate::captions::{HttpCaptions, Kind};
use crate::deliver::{Destination, Outputs, Words};
use crate::eval;
use crate::follow::Backoff;
use crate::fx::{self, AtomicFileStore, FetchOptions, RATES_FILE_NAME};
use crate::hub;
use crate::lan::{self, LanUrl};
use crate::lang::Lang;
use crate::llm::{HttpLlm, Llm};
use crate::logging::{Level, LogEvent, LogSink, SharedSink, StdoutSink};
use crate::partial::PartialTranslation;
use crate::pipeline::{Pipeline, PipelineConfig};
use crate::reach::{Reach, Watch};
use crate::relay::{self, Lanes, MakePipeline, Makers, Relay, RelayConfig, Watches};
use crate::schedule::{self, DEFAULT_TASK_NAME, TaskSpec};
use crate::send_config::SendFile;
use crate::srt_command::{self, SrtPlan};
use crate::strings::Strings;
use crate::verify_dump::VerifyDump;
use crate::warmup;

/// The streams `run` follows when none is named: the microphone's recognizer
/// and the loopback's recognizer, on the ports the recognizers serve on.
pub const DEFAULT_EVENTS: [&str; 2] = [
    "http://127.0.0.1:8833/events",
    "http://127.0.0.1:8834/events",
];

/// The translator `live` calls when `--llm` is not given: llama-server on its
/// default port.
pub const DEFAULT_LLM: &str = "http://127.0.0.1:8080";

/// The translator for the languages outside the live pair when `--llm-other`
/// is not given: the second llama-server (`hayamimi-llama-other.service`).
pub const DEFAULT_LLM_OTHER: &str = "http://127.0.0.1:18081";

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
    /// The translator of the languages outside the live pair.
    pub llm_other: String,
    pub serve: u16,
    /// The address `serve` listens on: the loopback unless `--serve-host` names another.
    pub serve_host: IpAddr,
    /// Where to write the settled translations as SRT cues.
    pub srt: Option<PathBuf>,
    /// The file that names the Zoom token and the Teams link; without it
    /// nothing is sent outside.
    pub send: Option<PathBuf>,
    /// The `--translate` codes, when the option is given.
    pub translate: Option<Vec<String>>,
    /// Whether the text of an utterance in progress is translated too.
    pub partial_translation: PartialTranslation,
    /// The developer's file for the translations handed to V (`--verify-text`);
    /// without it nothing is written.
    pub verify_text: Option<PathBuf>,
}

/// A parsed command line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Follow the recognizers' events and serve the translations.
    Run(Live),
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
    /// Translate sentence pairs for a scorer.
    Eval {
        pairs: PathBuf,
        out: PathBuf,
        llm: String,
        llm_other: String,
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
    BadAddress,
    BadPartialYield,
    NeedPairsAndOut,
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
            CliError::BadAddress => "bad_address",
            CliError::BadPartialYield => "bad_partial_yield",
            CliError::NeedPairsAndOut => "need_pairs_and_out",
            CliError::NeedVideo => "need_video",
        }
    }
}

/// The value of `--partial-yield`: `on` or `off`.
fn yield_choice(value: &str) -> Result<bool, CliError> {
    match value {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => Err(CliError::BadPartialYield),
    }
}

/// What the two switches about the text in progress make together. Turning the
/// translation off wins; otherwise the text in progress yields to a `final`
/// being translated unless `--partial-yield off` was given.
fn partial_choice(translation_off: bool, yielding: bool) -> PartialTranslation {
    match (translation_off, yielding) {
        (true, _) => PartialTranslation::Off,
        (false, true) => PartialTranslation::Yielding,
        (false, false) => PartialTranslation::On,
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
        llm_other: DEFAULT_LLM_OTHER.to_owned(),
        serve: DEFAULT_SERVE,
        serve_host: IpAddr::V4(Ipv4Addr::LOCALHOST),
        srt: None,
        send: None,
        translate: None,
        partial_translation: PartialTranslation::Yielding,
        verify_text: None,
    };
    let (mut translation_off, mut yielding) = (false, true);
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let option = arg.as_str();
        if option == "--no-partial-translation" {
            translation_off = true;
            continue;
        }
        if !matches!(
            option,
            "--events"
                | "--llm"
                | "--llm-other"
                | "--serve"
                | "--serve-host"
                | "--srt"
                | "--send"
                | "--translate"
                | "--verify-text"
                | "--partial-yield"
        ) {
            return Err(CliError::UnknownOption);
        }
        let value = rest.next().ok_or(CliError::MissingValue)?;
        match option {
            "--partial-yield" => yielding = yield_choice(value)?,
            "--events" => options.events.push(value.clone()),
            "--llm" => options.llm.clone_from(value),
            "--llm-other" => options.llm_other.clone_from(value),
            "--serve-host" => {
                options.serve_host = value.parse().map_err(|_| CliError::BadAddress)?;
            }
            "--srt" => options.srt = Some(PathBuf::from(value)),
            "--send" => options.send = Some(PathBuf::from(value)),
            "--verify-text" => options.verify_text = Some(PathBuf::from(value)),
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
    options.partial_translation = partial_choice(translation_off, yielding);
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
    let mut llm_other = DEFAULT_LLM_OTHER.to_owned();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let slot = match arg.as_str() {
            "--pairs" => &mut pairs,
            "--out" => &mut out,
            "--llm" => {
                llm.clone_from(rest.next().ok_or(CliError::MissingValue)?);
                continue;
            }
            "--llm-other" => {
                llm_other.clone_from(rest.next().ok_or(CliError::MissingValue)?);
                continue;
            }
            _ => return Err(CliError::UnknownOption),
        };
        *slot = Some(PathBuf::from(rest.next().ok_or(CliError::MissingValue)?));
    }
    match (pairs, out) {
        (Some(pairs), Some(out)) => Ok(Command::Eval {
            pairs,
            out,
            llm,
            llm_other,
        }),
        _ => Err(CliError::NeedPairsAndOut),
    }
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
        "eval" => parse_eval(rest),
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

/// Builds the pipeline of one lane against the llama-server at `llm_url`, and
/// the watch on whether that server can be reached. The live pair and the
/// other languages each get their own server and watch; every lane of one
/// server shares its watch. A request the server's translator has to be asked
/// again is said on `log`.
fn pipeline_factory(llm_url: String, log: SharedSink) -> (MakePipeline<Reach<HttpLlm>>, Watch) {
    let watch = Watch::new();
    let link = watch.link();
    let make: MakePipeline<Reach<HttpLlm>> = Arc::new(move |sink: &mut dyn LogSink| {
        let config = PipelineConfig {
            rates_path: default_rates_path(),
            tables_dir: default_tables_dir(),
        };
        let llm = HttpLlm::new(&llm_url).logging_to(log.clone());
        Pipeline::open(link.wrap(llm), &config, sink)
    });
    (make, watch)
}

/// The event that says where the translations are served and which
/// llama-servers they are asked of, with the key of the line printed beside
/// it. It is the port-moved warning when the requested port was taken.
fn serving_event(live: &Live, address: SocketAddr) -> (LogEvent, &'static str) {
    let moved = live.serve != 0 && address.port() != live.serve;
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
    let event = event
        .with("port", address.port())
        .with("scope", lan::scope_of(address.ip()))
        .with("llm", live.llm.as_str())
        .with("llm_other", live.llm_other.as_str());
    (event, key)
}

/// Says where the translations are served, and that the port moved when it did.
fn announce_serving(live: &Live, address: SocketAddr, sink: &mut dyn LogSink, strings: &Strings) {
    let (event, key) = serving_event(live, address);
    sink.emit(event);
    eprintln!("{} http://{address}/events", strings.cli_text(key));
    announce_lan(address, strings);
}

/// Sends the translator its first requests (the forms of T and E, one after
/// the other), then says where the translations are served. A first request
/// that fails is logged and the run goes on. `connect` makes the translator
/// for one request with the time limit it is given.
fn announce_when_warm<L: Llm>(
    connect: impl Fn(Duration) -> L,
    live: &Live,
    address: SocketAddr,
    sink: &mut dyn LogSink,
    strings: &Strings,
) {
    warmup::warm_up(connect, sink);
    announce_serving(live, address, sink, strings);
}

/// The start-up path of `live`: the first requests go to the translator at
/// `live.llm`, the two together within [`warmup::TIMEOUT`], which is longer
/// than a live subtitle may wait because a translator that has just started
/// answers its first request late. Then the serving is announced.
pub fn announce_after_first_request(
    live: &Live,
    address: SocketAddr,
    sink: &mut dyn LogSink,
    strings: &Strings,
) {
    let connect = |limit| HttpLlm::with_timeout(&live.llm, limit);
    announce_when_warm(connect, live, address, sink, strings);
}

/// Says the URL a client on the LAN opens, when the server is not on the loopback.
fn announce_lan(address: SocketAddr, strings: &Strings) {
    match lan::lan_url(address, "/events") {
        LanUrl::Loopback => {}
        LanUrl::At(url) => eprintln!("{} {url}", strings.cli_text("lan_at")),
        LanUrl::Unknown => eprintln!("{}", strings.cli_text("lan_unknown")),
    }
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

/// Why `live` could not start; each names a key of the locale's `cli` table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveError {
    /// No port could be opened.
    CannotServe,
    /// The file of caption addresses could not be set up.
    SendNotOpened,
}

impl LiveError {
    pub fn key(self) -> &'static str {
        match self {
            LiveError::CannotServe => "cannot_serve",
            LiveError::SendNotOpened => "send_not_opened",
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

/// Starts the relay `live` asks for and returns its handle. The relay works
/// in threads of its own; the caller keeps the handle, and stops the relay
/// with it or lets it end with the process.
pub fn start_live(
    live: &Live,
    now: i64,
    strings: &Strings,
    sink: &SharedSink,
) -> Result<Relay, LiveError> {
    let mut log = sink.clone();
    let listener = hub::bind_on(live.serve_host, live.serve).map_err(|_| LiveError::CannotServe)?;
    let destinations =
        send_destinations(live.send.clone(), sink).map_err(|_| LiveError::SendNotOpened)?;
    let config = RelayConfig {
        events: live.events.clone(),
        lanes: Lanes::PerSource,
        backoff: Backoff::STANDARD,
        started_epoch: now,
        translate: live.translate.clone(),
        verify_dump: live
            .verify_text
            .as_deref()
            .and_then(|path| VerifyDump::open(path, &mut log)),
    };
    let outputs = Outputs {
        destinations,
        srt: live
            .srt
            .as_deref()
            .and_then(|path| open_srt(path, &mut log, strings)),
        words: Words::from_strings(strings),
    };
    let (pair, pair_watch) = pipeline_factory(live.llm.clone(), log.clone());
    let (other, other_watch) = pipeline_factory(live.llm_other.clone(), log.clone());
    relay::start_handle(
        listener,
        config,
        Makers {
            pair,
            other,
            watches: Watches {
                pair: Some(pair_watch),
                other: Some(other_watch),
            },
        },
        log,
        outputs,
        live.partial_translation,
    )
    .map_err(|_| LiveError::CannotServe)
}

fn run_relay(live: &Live, now: i64, strings: &Strings) -> u8 {
    let mut sink = SharedSink::new(StdoutSink::new(strings.clone()));
    let started = match start_live(live, now, strings, &sink) {
        Ok(started) => started,
        Err(error) => {
            print_error(strings, error.key());
            return 1;
        }
    };
    for code in started.skipped() {
        eprintln!("{} {code}", strings.cli_text("eval_target_skipped"));
    }
    announce_after_first_request(live, started.address(), &mut sink, strings);
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
    other_url: &str,
    sink: &mut dyn LogSink,
    strings: &Strings,
) -> u8 {
    let llm = HttpLlm::with_timeout(llm_url, EVAL_REQUEST_TIMEOUT);
    let other = HttpLlm::with_timeout(other_url, EVAL_REQUEST_TIMEOUT);
    match eval::run(&llm, &other, &eval::EvalOptions { pairs, out }, sink) {
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
    // The measurement tool answers in English only and has its own parser.
    if let [name, rest @ ..] = args
        && name == "vprobe"
    {
        return crate::vprobe::run(rest);
    }
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
        Command::Eval {
            pairs,
            out,
            llm,
            llm_other,
        } => run_eval(&pairs, &out, &llm, &llm_other, &mut sink, &strings),
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
                llm_other: String::from(DEFAULT_LLM_OTHER),
                serve: 9100,
                serve_host: IpAddr::V4(Ipv4Addr::LOCALHOST),
                srt: None,
                send: None,
                translate: None,
                partial_translation: PartialTranslation::Yielding,
                verify_text: None,
            }))
        );
    }

    fn serve_host_of(list: &[&str]) -> Result<IpAddr, CliError> {
        match parse_args(&args(list))? {
            Command::Run(live) => Ok(live.serve_host),
            _ => Err(CliError::UnknownCommand),
        }
    }

    #[test]
    fn live_listens_on_the_loopback_unless_it_is_given_an_address() -> Result<(), CliError> {
        assert_eq!(serve_host_of(&["live"])?, IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert_eq!(
            serve_host_of(&["live", "--serve-host", "0.0.0.0"])?,
            IpAddr::V4(Ipv4Addr::UNSPECIFIED)
        );
        assert_eq!(
            serve_host_of(&["live", "--serve-host", "192.168.200.20", "--serve", "0"])?,
            IpAddr::V4(Ipv4Addr::new(192, 168, 200, 20))
        );
        Ok(())
    }

    #[test]
    fn a_serve_host_that_is_not_an_address_or_has_no_value_is_refused() {
        assert_eq!(
            serve_host_of(&["live", "--serve-host", "lan"]),
            Err(CliError::BadAddress)
        );
        assert_eq!(
            serve_host_of(&["live", "--serve-host"]),
            Err(CliError::MissingValue)
        );
    }

    fn live_of(list: &[&str]) -> Result<Live, String> {
        match parse_args(&args(list)) {
            Ok(Command::Run(live)) => Ok(live),
            other => Err(format!("not a live command line: {other:?}")),
        }
    }

    fn address(port: u16) -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
    }

    fn text_field(event: &LogEvent, key: &str) -> Option<String> {
        event
            .fields
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    }

    #[test]
    fn the_start_event_names_the_two_translators_it_was_given() -> Result<(), String> {
        let live = live_of(&[
            "live",
            "--llm",
            "http://127.0.0.1:1",
            "--llm-other",
            "http://127.0.0.1:2",
            "--serve",
            "8835",
        ])?;
        let (event, key) = serving_event(&live, address(8835));
        assert_eq!(
            (event.event.as_str(), key),
            ("relay.serve.started", "serving_at")
        );
        assert_eq!(
            text_field(&event, "llm").as_deref(),
            Some("http://127.0.0.1:1")
        );
        assert_eq!(
            text_field(&event, "llm_other").as_deref(),
            Some("http://127.0.0.1:2")
        );
        assert_eq!(event.fields.get("port"), Some(&serde_json::json!(8835)));
        assert_eq!(text_field(&event, "scope").as_deref(), Some("loopback"));
        Ok(())
    }

    #[test]
    fn the_start_event_names_the_default_translators_when_none_is_given() -> Result<(), String> {
        let (event, _) = serving_event(&live_of(&["live"])?, address(DEFAULT_SERVE));
        assert_eq!(text_field(&event, "llm").as_deref(), Some(DEFAULT_LLM));
        assert_eq!(
            text_field(&event, "llm_other").as_deref(),
            Some(DEFAULT_LLM_OTHER)
        );
        Ok(())
    }

    /// A translator that records what it is asked and answers or fails.
    struct FirstRequest {
        asked: std::cell::RefCell<Vec<String>>,
        fails: bool,
    }

    impl Llm for &FirstRequest {
        fn complete(
            &self,
            request: &crate::llm::Completion<'_>,
        ) -> Result<String, crate::llm::LlmError> {
            self.asked.borrow_mut().push(request.prompt.to_owned());
            if self.fails {
                Err(crate::llm::LlmError::Status(503))
            } else {
                Ok(String::from("Hello."))
            }
        }
    }

    /// The names of the events the relay logged, in order, for one start.
    fn names_logged_when_warm(fails: bool) -> Result<(Vec<String>, Vec<String>), String> {
        let llm = FirstRequest {
            asked: std::cell::RefCell::new(Vec::new()),
            fails,
        };
        let mut sink = crate::logging::MemorySink::default();
        announce_when_warm(
            |_| &llm,
            &live_of(&["live"])?,
            address(DEFAULT_SERVE),
            &mut sink,
            &Strings::embedded(Lang::En)?,
        );
        let names = sink
            .events
            .iter()
            .map(|event| event.event.clone())
            .collect();
        Ok((names, llm.asked.take()))
    }

    #[test]
    fn the_first_requests_go_out_once_each_before_the_relay_says_it_serves() -> Result<(), String> {
        let (names, asked) = names_logged_when_warm(false)?;
        assert_eq!(
            names,
            [
                warmup::SENT_EVENT,
                warmup::SENT_EVENT,
                "relay.serve.started"
            ]
        );
        assert_eq!(asked, [warmup::prompt(), warmup::prompt()]);
        Ok(())
    }

    #[test]
    fn first_requests_that_fail_do_not_stop_the_relay_from_saying_it_serves() -> Result<(), String>
    {
        let (names, asked) = names_logged_when_warm(true)?;
        assert_eq!(
            names,
            [
                warmup::FAILED_EVENT,
                warmup::FAILED_EVENT,
                "relay.serve.started"
            ]
        );
        assert_eq!(asked, [warmup::prompt(), warmup::prompt()]);
        Ok(())
    }

    #[test]
    fn the_port_moved_warning_names_the_translators_too() -> Result<(), String> {
        let live = live_of(&["live", "--llm", "http://127.0.0.1:1", "--serve", "8835"])?;
        let (event, key) = serving_event(&live, address(8836));
        assert_eq!(
            (event.event.as_str(), key),
            ("server.fallback.port", "port_moved")
        );
        assert_eq!(
            text_field(&event, "llm").as_deref(),
            Some("http://127.0.0.1:1")
        );
        assert_eq!(
            text_field(&event, "llm_other").as_deref(),
            Some(DEFAULT_LLM_OTHER)
        );
        Ok(())
    }

    #[test]
    fn the_verify_text_file_is_named_by_an_option_and_absent_otherwise() {
        let file_of = |list: &[&str]| match parse_args(&args(list)) {
            Ok(Command::Run(live)) => Some(live.verify_text),
            _ => None,
        };
        assert_eq!(file_of(&["live"]), Some(None));
        assert_eq!(
            file_of(&["live", "--verify-text", "dump.jsonl"]),
            Some(Some(PathBuf::from("dump.jsonl")))
        );
        let missing = parse_args(&args(&["live", "--verify-text"]));
        assert_eq!(missing, Err(CliError::MissingValue));
    }

    fn partial_translation_of(list: &[&str]) -> Option<PartialTranslation> {
        match parse_args(&args(list)) {
            Ok(Command::Run(Live {
                partial_translation,
                ..
            })) => Some(partial_translation),
            _ => None,
        }
    }

    #[test]
    fn the_translation_of_the_text_in_progress_yields_unless_it_is_turned_off() {
        assert_eq!(
            partial_translation_of(&["live"]),
            Some(PartialTranslation::Yielding)
        );
        assert_eq!(
            partial_translation_of(&["live", "--no-partial-translation"]),
            Some(PartialTranslation::Off)
        );
        assert_eq!(
            partial_translation_of(&[
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
    fn partial_yield_off_translates_the_text_in_progress_without_yielding() {
        assert_eq!(
            partial_translation_of(&["live", "--partial-yield", "off"]),
            Some(PartialTranslation::On)
        );
        assert_eq!(
            partial_translation_of(&["live", "--partial-yield", "on"]),
            Some(PartialTranslation::Yielding)
        );
    }

    #[test]
    fn turning_the_translation_off_wins_over_the_yield_in_either_order() {
        for list in [
            ["live", "--no-partial-translation", "--partial-yield", "on"],
            ["live", "--partial-yield", "on", "--no-partial-translation"],
            ["live", "--partial-yield", "off", "--no-partial-translation"],
            ["live", "--no-partial-translation", "--partial-yield", "off"],
        ] {
            assert_eq!(
                partial_translation_of(&list),
                Some(PartialTranslation::Off),
                "{list:?}"
            );
        }
    }

    #[test]
    fn the_yield_takes_on_or_off_and_nothing_else() {
        for value in ["maybe", "", "ON", "1"] {
            let parsed = parse_args(&args(&["live", "--partial-yield", value]));
            assert_eq!(parsed, Err(CliError::BadPartialYield), "{value:?}");
        }
        assert_eq!(
            parse_args(&args(&["live", "--partial-yield"])),
            Err(CliError::MissingValue)
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
            llm_other: String::from(DEFAULT_LLM_OTHER),
            serve: 8835,
            serve_host: IpAddr::V4(Ipv4Addr::LOCALHOST),
            srt: None,
            send: None,
            translate: None,
            partial_translation: PartialTranslation::Yielding,
            verify_text: None,
        });
        assert_eq!(parse_args(&args(&["live"])), Ok(expected));
    }

    #[test]
    fn live_sends_outside_only_when_it_is_given_the_file() {
        let parsed = parse_args(&args(&["live", "--send", "send.toml"]));
        assert!(matches!(
            parsed,
            Ok(Command::Run(Live { send: Some(ref path), .. })) if path == Path::new("send.toml")
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
                CliError::BadAddress.key(),
                CliError::BadPartialYield.key(),
                "cannot_serve",
                "serving_at",
                "port_moved",
                "lan_at",
                "lan_unknown",
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
                warmup::SENT_EVENT,
                warmup::FAILED_EVENT,
                "server.fallback.port",
                "translate.edit.held",
                "translate.draft.held",
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
                llm_other: String::from(DEFAULT_LLM_OTHER),
            })
        );
    }

    #[test]
    fn the_second_translator_is_named_with_llm_other() {
        let parsed = parse_args(&args(&["live", "--llm-other", "http://y:2"]));
        assert!(matches!(
            parsed,
            Ok(Command::Run(Live { ref llm_other, .. })) if llm_other == "http://y:2"
        ));
        let parsed = parse_args(&args(&[
            "eval",
            "--pairs",
            "p",
            "--out",
            "o",
            "--llm-other",
            "u",
        ]));
        assert!(matches!(
            parsed,
            Ok(Command::Eval { ref llm_other, .. }) if llm_other == "u"
        ));
        assert_eq!(
            parse_args(&args(&["live", "--llm-other"])),
            Err(CliError::MissingValue)
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

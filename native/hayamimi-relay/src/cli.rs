//! The command line: `fx-fetch` and `fx-schedule`.

use std::path::PathBuf;

use crate::calendar;
use crate::fx::{self, AtomicFileStore, FetchOptions, RATES_FILE_NAME};
use crate::lang::Lang;
use crate::logging::{LogSink, StdoutSink};
use crate::schedule::{self, DEFAULT_TASK_NAME, TaskSpec};
use crate::strings::Strings;

/// What `fx-schedule` was asked to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScheduleAction {
    Install,
    Remove,
}

/// A parsed command line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
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
}

impl CliError {
    /// The key into the locale's `cli` string table.
    pub fn key(self) -> &'static str {
        match self {
            CliError::UnknownCommand => "unknown_command",
            CliError::MissingValue => "missing_value",
            CliError::UnknownOption => "unknown_option",
            CliError::NeedInstallOrRemove => "need_install_or_remove",
        }
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

/// Runs a command line. Returns the process exit code: 0 done, 1 failed,
/// 2 the command line was refused.
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
        assert_eq!(parse_args(&args(&["live"])), Err(CliError::UnknownCommand));
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
    fn the_default_rates_path_ends_with_the_fixed_name() {
        assert!(default_rates_path().is_none_or(|path| path.ends_with("hayamimi/fx_rates.json")));
    }
}

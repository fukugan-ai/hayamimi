//! Registering the morning rate fetch with the Windows task scheduler.
//!
//! The task runs `hayamimi-relay fx-fetch` every day at 07:30 and, if the
//! machine was off at that time, as soon as it is available again. Removing
//! the task undoes the registration. The task name is a parameter so that a
//! test never touches the real task.

use std::fmt;
use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;

use crate::calendar::Date;
use crate::logging::{Level, LogEvent, LogSink};

/// The task the user's machine runs every morning.
pub const DEFAULT_TASK_NAME: &str = "hayamimi-fx-fetch";
/// Local time of the daily run.
pub const DAILY_TIME: &str = "07:30:00";

/// The task to register.
pub struct TaskSpec<'a> {
    pub name: &'a str,
    /// The relay executable the task starts.
    pub exe: &'a Path,
    /// Where the fetch writes the rate file.
    pub out: &'a Path,
    /// A source other than the ECB file; only tests pass one.
    pub source_url: Option<&'a str>,
}

/// Why a scheduler operation failed.
#[derive(Debug)]
pub enum ScheduleError {
    /// The name is empty or holds a character task names cannot hold.
    BadName,
    /// `schtasks` could not be started (it exists only on Windows).
    Spawn(io::Error),
    /// The XML file for the task could not be written.
    Write(io::Error),
    /// `schtasks` ran and reported failure.
    Failed(Option<i32>),
}

impl fmt::Display for ScheduleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScheduleError::BadName => write!(f, "bad task name"),
            ScheduleError::Spawn(why) => write!(f, "cannot start schtasks: {why}"),
            ScheduleError::Write(why) => write!(f, "cannot write the task file: {why}"),
            ScheduleError::Failed(Some(code)) => write!(f, "schtasks exited with {code}"),
            ScheduleError::Failed(None) => write!(f, "schtasks was stopped"),
        }
    }
}

impl std::error::Error for ScheduleError {}

/// A task name must be non-empty and free of path separators and the
/// characters the scheduler rejects.
pub fn check_task_name(name: &str) -> Result<(), ScheduleError> {
    let bad = name.is_empty()
        || name
            .chars()
            .any(|c| c.is_control() || "\\/:*?\"<>|".contains(c));
    if bad {
        Err(ScheduleError::BadName)
    } else {
        Ok(())
    }
}

fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// The command-line arguments the task passes to the relay.
fn task_arguments(spec: &TaskSpec<'_>) -> String {
    let mut arguments = format!("fx-fetch --out \"{}\"", spec.out.display());
    if let Some(url) = spec.source_url {
        arguments.push_str(&format!(" --source-url \"{url}\""));
    }
    arguments
}

/// The task definition: a daily trigger at 07:30 that also runs a missed
/// start, one instance at a time, for the signed-in user.
pub fn task_xml(spec: &TaskSpec<'_>, first_day: Date) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.4" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Fetch the daily ECB reference rates for hayamimi subtitles.</Description>
  </RegistrationInfo>
  <Triggers>
    <CalendarTrigger>
      <StartBoundary>{year:04}-{month:02}-{day:02}T{DAILY_TIME}</StartBoundary>
      <Enabled>true</Enabled>
      <ScheduleByDay>
        <DaysInterval>1</DaysInterval>
      </ScheduleByDay>
    </CalendarTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <StartWhenAvailable>true</StartWhenAvailable>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <ExecutionTimeLimit>PT5M</ExecutionTimeLimit>
    <Enabled>true</Enabled>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{command}</Command>
      <Arguments>{arguments}</Arguments>
    </Exec>
  </Actions>
</Task>
"#,
        year = first_day.year,
        month = first_day.month,
        day = first_day.day,
        command = escape_xml(&spec.exe.display().to_string()),
        arguments = escape_xml(&task_arguments(spec)),
    )
}

/// UTF-16LE with a byte-order mark, the encoding `schtasks /XML` expects
/// when the declaration says UTF-16.
pub fn xml_to_utf16le(xml: &str) -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xFE];
    for unit in xml.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

fn run_schtasks(arguments: &[&str]) -> Result<(), ScheduleError> {
    let output = Command::new("schtasks")
        .args(arguments)
        .output()
        .map_err(ScheduleError::Spawn)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(ScheduleError::Failed(output.status.code()))
    }
}

fn report(sink: &mut dyn LogSink, ok_event: &str, name: &str, result: &Result<(), ScheduleError>) {
    // Changing the scheduler is a configuration change, so it is audited.
    let event = match result {
        Ok(()) => LogEvent::new(Level::Info, ok_event, "audit"),
        Err(_) => {
            LogEvent::new(Level::Error, "schedule.task.failed", "audit").with_error("SCHEDULE")
        }
    };
    sink.emit(event.with("task", name));
}

/// Registers (or replaces) the daily task.
pub fn install(
    spec: &TaskSpec<'_>,
    first_day: Date,
    sink: &mut dyn LogSink,
) -> Result<(), ScheduleError> {
    check_task_name(spec.name)?;
    let xml_path = std::env::temp_dir().join(format!("hayamimi-task-{}.xml", std::process::id()));
    let result = fs::write(&xml_path, xml_to_utf16le(&task_xml(spec, first_day)))
        .map_err(ScheduleError::Write)
        .and_then(|()| match xml_path.to_str() {
            Some(path) => run_schtasks(&["/Create", "/TN", spec.name, "/XML", path, "/F"]),
            None => Err(ScheduleError::Write(io::Error::new(
                io::ErrorKind::InvalidInput,
                "temporary path is not valid text",
            ))),
        });
    // The XML file is only the hand-over to the scheduler.
    let _ = fs::remove_file(&xml_path);
    report(sink, "schedule.task.installed", spec.name, &result);
    result
}

/// Removes the daily task.
pub fn remove(name: &str, sink: &mut dyn LogSink) -> Result<(), ScheduleError> {
    check_task_name(name)?;
    let result = run_schtasks(&["/Delete", "/TN", name, "/F"]);
    report(sink, "schedule.task.removed", name, &result);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::MemorySink;
    use std::path::PathBuf;

    fn spec<'a>(exe: &'a Path, out: &'a Path, url: Option<&'a str>) -> TaskSpec<'a> {
        TaskSpec {
            name: "hayamimi-fx-fetch-test-1",
            exe,
            out,
            source_url: url,
        }
    }

    fn day() -> Date {
        Date {
            year: 2026,
            month: 10,
            day: 14,
        }
    }

    #[test]
    fn names_with_separators_or_nothing_are_refused() {
        assert!(check_task_name("hayamimi-fx-fetch").is_ok());
        assert!(matches!(check_task_name(""), Err(ScheduleError::BadName)));
        assert!(matches!(
            check_task_name("a\\b"),
            Err(ScheduleError::BadName)
        ));
        assert!(matches!(
            check_task_name("a\nb"),
            Err(ScheduleError::BadName)
        ));
    }

    #[test]
    fn the_xml_has_a_daily_trigger_that_runs_missed_starts() {
        let (exe, out) = (
            PathBuf::from("C:\\r\\relay.exe"),
            PathBuf::from("C:\\d\\fx_rates.json"),
        );
        let xml = task_xml(&spec(&exe, &out, None), day());
        assert!(xml.contains("<StartBoundary>2026-10-14T07:30:00</StartBoundary>"));
        assert!(xml.contains("<DaysInterval>1</DaysInterval>"));
        assert!(xml.contains("<StartWhenAvailable>true</StartWhenAvailable>"));
        assert!(xml.contains("<Command>C:\\r\\relay.exe</Command>"));
    }

    #[test]
    fn the_arguments_are_escaped_and_carry_the_source_only_when_given() {
        let (exe, out) = (PathBuf::from("r.exe"), PathBuf::from("o&.json"));
        let plain = task_xml(&spec(&exe, &out, None), day());
        assert!(plain.contains("fx-fetch --out &quot;o&amp;.json&quot;</Arguments>"));
        let with_url = task_xml(&spec(&exe, &out, Some("http://127.0.0.1:1/x")), day());
        assert!(with_url.contains("--source-url &quot;http://127.0.0.1:1/x&quot;"));
    }

    #[test]
    fn utf16_output_starts_with_a_byte_order_mark() {
        assert_eq!(xml_to_utf16le("a"), [0xFF, 0xFE, b'a', 0]);
        assert_eq!(xml_to_utf16le("あ"), [0xFF, 0xFE, 0x42, 0x30]);
    }

    #[test]
    fn a_bad_name_stops_before_anything_runs() {
        let mut sink = MemorySink::default();
        assert!(matches!(remove("", &mut sink), Err(ScheduleError::BadName)));
        assert!(sink.events.is_empty());
    }

    #[test]
    fn errors_describe_themselves() {
        assert_eq!(
            ScheduleError::Failed(Some(1)).to_string(),
            "schtasks exited with 1"
        );
        assert_eq!(ScheduleError::BadName.to_string(), "bad task name");
    }

    #[test]
    fn report_audits_success_and_failure() {
        let mut sink = MemorySink::default();
        report(&mut sink, "schedule.task.removed", "t", &Ok(()));
        report(
            &mut sink,
            "schedule.task.removed",
            "t",
            &Err(ScheduleError::Failed(Some(1))),
        );
        assert_eq!(sink.events[0].component, "audit");
        assert_eq!(sink.events[1].event, "schedule.task.failed");
    }
}

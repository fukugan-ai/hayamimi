//! What `hayamimi models fetch` says as log events on stdout. An event carries
//! the stage and the source by name and the reason as a token; never an
//! address, a path or a message.

use hayamimi_relay::logging::Level;
use serde_json::Value;

use super::error::FetchError;
use super::items::{Item, Source};
use super::keys;
use crate::log::event_line;
use crate::notice::Notice;
use crate::strings::Strings;

/// The component of every event of the command.
const COMPONENT: &str = "models";
/// The error code of a source that did not give a group.
const SOURCE_FAILED_CODE: &str = "HAYAMIMI_MODELS_SOURCE_FAILED";

/// How a run ended, group by group.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tally {
    pub fetched: usize,
    pub present: usize,
    /// Groups no source gave; their files, if any, were left as they were.
    pub unavailable: usize,
    /// Of those, the groups with no file of theirs on disk at all.
    pub empty: usize,
    pub manifest_written: bool,
}

/// `models.group.fetched`: a group came from `source`.
pub fn fetched_line(strings: &Strings, item: &Item, source: Source, files: usize) -> String {
    event_line(
        strings,
        Level::Info,
        keys::EVENT_FETCHED,
        COMPONENT,
        None,
        &[
            ("stage", Value::from(item.stage)),
            ("source", Value::from(source.name())),
            ("files", Value::from(files)),
        ],
    )
}

/// `models.group.present`: a group was already there and was not fetched.
pub fn present_line(strings: &Strings, item: &Item) -> String {
    event_line(
        strings,
        Level::Info,
        keys::EVENT_PRESENT,
        COMPONENT,
        None,
        &[("stage", Value::from(item.stage))],
    )
}

/// `models.source.failed`: one source did not give the group.
pub fn source_failed_line(
    strings: &Strings,
    item: &Item,
    source: Source,
    error: &FetchError,
) -> String {
    event_line(
        strings,
        Level::Warn,
        keys::EVENT_SOURCE_FAILED,
        COMPONENT,
        Some(SOURCE_FAILED_CODE),
        &[
            ("stage", Value::from(item.stage)),
            ("source", Value::from(source.name())),
            ("reason", Value::from(error.reason())),
        ],
    )
}

/// `models.fetch.finished`: the end of a run, with the count of each outcome.
pub fn finished_line(strings: &Strings, tally: &Tally) -> String {
    event_line(
        strings,
        Level::Info,
        keys::EVENT_FINISHED,
        COMPONENT,
        None,
        &[
            ("fetched", Value::from(tally.fetched)),
            ("present", Value::from(tally.present)),
            ("unavailable", Value::from(tally.unavailable)),
            ("manifest_written", Value::from(tally.manifest_written)),
        ],
    )
}

/// `models.fallback.previous`: some groups came from no source, so the files
/// already there stay. One event for the whole run.
pub fn previous_line(strings: &Strings, notice: &Notice, tally: &Tally) -> String {
    event_line(
        strings,
        Level::Warn,
        notice.event,
        COMPONENT,
        Some(notice.error_code),
        &[
            ("stage", Value::from(notice.stage)),
            ("reason", Value::from(notice.reason)),
            ("groups", Value::from(tally.unavailable)),
            ("empty", Value::from(tally.empty)),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models_fetch::items::MINIMAL;
    use hayamimi_relay::lang::Lang;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn parsed(line: &str) -> Result<Value, serde_json::Error> {
        serde_json::from_str(line)
    }

    #[test]
    fn a_source_failure_carries_the_stage_the_source_and_a_token_but_no_address() -> TestResult {
        let strings = Strings::embedded(Lang::En)?;
        let error = FetchError::Status {
            url: String::from("http://secret.example/x"),
            code: 404,
        };
        let line = source_failed_line(&strings, &MINIMAL[0], MINIMAL[0].sources[0], &error);
        let event = parsed(&line)?;
        assert_eq!(event["event"], "models.source.failed");
        assert_eq!(event["stage"], "recognizer");
        assert_eq!(event["source"], "release");
        assert_eq!(event["reason"], "http_status");
        assert_eq!(event["level"], "warn");
        assert!(!line.contains("secret.example"));
        Ok(())
    }

    #[test]
    fn the_previous_event_is_one_warning_with_the_notice_fields() -> TestResult {
        let strings = Strings::embedded(Lang::En)?;
        let tally = Tally {
            unavailable: 4,
            empty: 1,
            ..Tally::default()
        };
        let notice = Notice::models_previous();
        let event = parsed(&previous_line(&strings, &notice, &tally))?;
        assert_eq!(event["event"], "models.fallback.previous");
        assert_eq!(event["error_code"], "HAYAMIMI_MODELS_FALLBACK");
        assert_eq!(
            (event["stage"].as_str(), event["reason"].as_str()),
            (Some("models"), Some("unreachable"))
        );
        assert_eq!(
            (event["groups"].as_u64(), event["empty"].as_u64()),
            (Some(4), Some(1))
        );
        Ok(())
    }

    #[test]
    fn the_counts_of_a_run_are_in_the_finished_event() -> TestResult {
        let strings = Strings::embedded(Lang::Ja)?;
        let tally = Tally {
            fetched: 2,
            present: 1,
            ..Tally::default()
        };
        let event = parsed(&finished_line(&strings, &tally))?;
        assert_eq!(event["fetched"], 2);
        assert_eq!(event["present"], 1);
        assert_eq!(event["manifest_written"], false);
        Ok(())
    }

    #[test]
    fn a_fetched_and_a_present_group_name_their_stage() -> TestResult {
        let strings = Strings::embedded(Lang::En)?;
        let item = &MINIMAL[3];
        let fetched = parsed(&fetched_line(&strings, item, item.sources[0], 2))?;
        assert_eq!(
            (fetched["stage"].as_str(), fetched["source"].as_str()),
            (Some("punctuation"), Some("huggingface"))
        );
        assert_eq!(fetched["files"], 2);
        let present = parsed(&present_line(&strings, item))?;
        assert_eq!(present["event"], "models.group.present");
        Ok(())
    }
}

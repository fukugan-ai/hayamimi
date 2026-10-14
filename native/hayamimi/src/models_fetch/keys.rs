//! The keys of the locale tables that `hayamimi models fetch` uses: texts of
//! the `[cli]` table, and the messages of its log events.

pub const HELP: &str = "models_help";
pub const UNKNOWN_COMMAND: &str = "models_unknown_command";
pub const PRESENT: &str = "models_present";
pub const FETCHED: &str = "models_fetched";
pub const SOURCE_FAILED: &str = "models_source_failed";
pub const GROUP_FAILED: &str = "models_group_failed";
pub const DONE: &str = "models_done";
pub const MANIFEST_FAILED: &str = "models_manifest_failed";
pub const DIR_FAILED: &str = "models_dir_failed";

/// Every `[cli]` key above, for the check that both tables hold them all.
#[cfg(test)]
pub const ALL: [&str; 9] = [
    HELP,
    UNKNOWN_COMMAND,
    PRESENT,
    FETCHED,
    SOURCE_FAILED,
    GROUP_FAILED,
    DONE,
    MANIFEST_FAILED,
    DIR_FAILED,
];

/// The log event of a group that was fetched.
pub const EVENT_FETCHED: &str = "models.group.fetched";
/// The log event of a group that was already there.
pub const EVENT_PRESENT: &str = "models.group.present";
/// The log event of one source that did not give a group.
pub const EVENT_SOURCE_FAILED: &str = "models.source.failed";
/// The log event of the end of a run.
pub const EVENT_FINISHED: &str = "models.fetch.finished";

/// Every log event above that is not a fallback notice, for the same check.
#[cfg(test)]
pub const EVENTS: [&str; 4] = [
    EVENT_FETCHED,
    EVENT_PRESENT,
    EVENT_SOURCE_FAILED,
    EVENT_FINISHED,
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strings::Strings;
    use hayamimi_relay::lang::Lang;

    #[test]
    fn both_tables_hold_every_key_and_event_of_the_command() -> Result<(), String> {
        for lang in [Lang::En, Lang::Ja] {
            let table = Strings::embedded(lang)?;
            for key in ALL {
                assert!(table.cli.contains_key(key), "{lang:?} lacks `{key}`");
            }
            for event in EVENTS {
                assert!(
                    table.messages.contains_key(event),
                    "{lang:?} lacks `{event}`"
                );
            }
        }
        Ok(())
    }
}

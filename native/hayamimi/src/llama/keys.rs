//! The keys of the locale tables that `hayamimi llama` uses: texts of the
//! `[cli]` table, and the messages of its log events.

pub const HELP: &str = "llama_help";
pub const UNKNOWN_COMMAND: &str = "llama_unknown_command";
pub const NEED_MODEL: &str = "llama_need_model";
pub const NO_DATA_DIR: &str = "llama_no_data_dir";
pub const MODEL_MISSING: &str = "llama_model_missing";
pub const STARTED: &str = "llama_started";
pub const RUNNING: &str = "llama_running";
pub const UNAVAILABLE: &str = "llama_unavailable";

/// Every `[cli]` key above.
pub const ALL: [&str; 8] = [
    HELP,
    UNKNOWN_COMMAND,
    NEED_MODEL,
    NO_DATA_DIR,
    MODEL_MISSING,
    STARTED,
    RUNNING,
    UNAVAILABLE,
];

/// The log event of a server that was started and answers.
pub const EVENT_STARTED: &str = "translate.server.started";
/// The log event of a server that already answered, so none was started.
pub const EVENT_RUNNING: &str = "translate.server.running";
/// The log event of one way of starting that did not come up.
pub const EVENT_FAILED: &str = "translate.server.failed";
/// The log event of a start that left no server.
pub const EVENT_UNAVAILABLE: &str = "translate.server.unavailable";

/// Every log event above.
pub const EVENTS: [&str; 4] = [
    EVENT_STARTED,
    EVENT_RUNNING,
    EVENT_FAILED,
    EVENT_UNAVAILABLE,
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

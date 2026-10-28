//! Locale string tables (`strings.<locale>.toml`): the text of log messages
//! and of the command line. No natural-language sentence is written in code;
//! code names a key and the table of the reader's locale supplies the words.

use std::collections::{BTreeMap, BTreeSet};

use hayamimi_relay::lang::Lang;
use serde::Deserialize;

pub const EN: &str = include_str!("../data/strings.en.toml");
pub const JA: &str = include_str!("../data/strings.ja.toml");

/// The keys of the `[cli]` table that code uses.
pub mod keys {
    pub const HELP: &str = "help";
    pub const USAGE_HINT: &str = "usage_hint";
    pub const UNKNOWN_OPTION: &str = "unknown_option";
    pub const MISSING_VALUE: &str = "missing_value";
    pub const BAD_NUMBER: &str = "bad_number";
    pub const MODE_UNSUPPORTED: &str = "mode_unsupported";
    pub const ENDPOINT_UNSUPPORTED: &str = "endpoint_unsupported";
    pub const PARTIAL_LID_UNSUPPORTED: &str = "partial_lid_unsupported";
    pub const FINAL_LID_UNSUPPORTED: &str = "final_lid_unsupported";
    pub const HELD_REUSE_UNSUPPORTED: &str = "held_reuse_unsupported";
    pub const DRAFT_WORKER_UNSUPPORTED: &str = "draft_worker_unsupported";
    pub const ASR_THREADS_UNSUPPORTED: &str = "asr_threads_unsupported";
    pub const DRAFT_MARKS_UNSUPPORTED: &str = "draft_marks_unsupported";
    pub const LANG_UNSUPPORTED: &str = "lang_unsupported";
    pub const NEED_WAV: &str = "need_wav";
    pub const NO_GPU_CHECK: &str = "no_gpu_check";
    pub const RUN_FAILED: &str = "run_failed";
    pub const UNKNOWN_INPUT: &str = "unknown_input";
    pub const BAD_ADDRESS: &str = "bad_address";
    pub const BAD_ORIGIN: &str = "bad_origin";
    pub const LAN_URL: &str = "lan_url";
    pub const LAN_URL_UNKNOWN: &str = "lan_url_unknown";

    /// Every fixed key above, for the check that both tables hold them all.
    /// The keys of a fallback notice are derived from its fields (`stage_*`,
    /// `reason_*`, `to_*`) and are checked against the lists in `notice`.
    pub const ALL: [&str; 22] = [
        HELP,
        USAGE_HINT,
        UNKNOWN_OPTION,
        MISSING_VALUE,
        BAD_NUMBER,
        MODE_UNSUPPORTED,
        ENDPOINT_UNSUPPORTED,
        PARTIAL_LID_UNSUPPORTED,
        FINAL_LID_UNSUPPORTED,
        HELD_REUSE_UNSUPPORTED,
        DRAFT_WORKER_UNSUPPORTED,
        ASR_THREADS_UNSUPPORTED,
        DRAFT_MARKS_UNSUPPORTED,
        LANG_UNSUPPORTED,
        NEED_WAV,
        NO_GPU_CHECK,
        RUN_FAILED,
        UNKNOWN_INPUT,
        BAD_ADDRESS,
        BAD_ORIGIN,
        LAN_URL,
        LAN_URL_UNKNOWN,
    ];
}

#[derive(Debug, Clone, Deserialize)]
pub struct Strings {
    /// Log messages by event name.
    pub messages: BTreeMap<String, String>,
    /// Command-line text by key.
    pub cli: BTreeMap<String, String>,
}

/// The language of a locale tag such as `ja_JP.UTF-8`. English is the default:
/// it is what a machine that reports no locale, or one this build has no table
/// for, reads.
pub fn resolve(tag: Option<&str>) -> Lang {
    match tag {
        Some(tag) => Lang::from_locale_tag(tag),
        // DEFAULT: a machine that reports no locale reads English, as the design fixes.
        None => Lang::En,
    }
}

impl Strings {
    /// Parses a table file; the error is a short description for the caller.
    pub fn parse(text: &str) -> Result<Strings, String> {
        toml::from_str(text).map_err(|error| error.to_string())
    }

    /// The compiled-in table of a language.
    pub fn embedded(lang: Lang) -> Result<Strings, String> {
        match lang {
            Lang::Ja => Strings::parse(JA),
            Lang::En => Strings::parse(EN),
        }
    }

    /// The log message of an event; the event name itself when the table has
    /// no entry (a test keeps every event the code emits in the table).
    pub fn message_for<'a>(&'a self, event: &'a str) -> &'a str {
        match self.messages.get(event) {
            Some(message) => message,
            None => event,
        }
    }

    /// A command-line text by key; the key itself when the table lacks it (a
    /// test keeps every key in `keys::ALL` in both tables).
    pub fn cli_text<'a>(&'a self, key: &'a str) -> &'a str {
        match self.cli.get(key) {
            Some(text) => text,
            None => key,
        }
    }

    /// Every line of every command-line text. A text built only from table
    /// values has each of its lines among these.
    pub fn cli_lines(&self) -> BTreeSet<&str> {
        self.cli.values().flat_map(|text| text.lines()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_embedded_tables_parse() -> Result<(), String> {
        Strings::embedded(Lang::En)?;
        Strings::embedded(Lang::Ja)?;
        Ok(())
    }

    #[test]
    fn every_key_the_code_uses_is_in_both_tables() -> Result<(), String> {
        for lang in [Lang::En, Lang::Ja] {
            let table = Strings::embedded(lang)?;
            for key in keys::ALL {
                assert!(table.cli.contains_key(key), "{lang:?} lacks `{key}`");
            }
        }
        Ok(())
    }

    #[test]
    fn every_stage_reason_and_destination_a_notice_can_name_has_a_line_in_both_tables()
    -> Result<(), String> {
        use crate::notice::{DESTINATIONS, REASONS, STAGES};
        for lang in [Lang::En, Lang::Ja] {
            let table = Strings::embedded(lang)?;
            for (prefix, names) in [
                ("stage_", &STAGES[..]),
                ("reason_", &REASONS[..]),
                ("to_", &DESTINATIONS[..]),
            ] {
                for name in names {
                    let key = format!("{prefix}{name}");
                    assert!(table.cli.contains_key(&key), "{lang:?} lacks `{key}`");
                }
            }
        }
        Ok(())
    }

    /// T-BR-38-021
    #[test]
    fn the_draft_lines_have_a_message_in_both_languages() -> Result<(), String> {
        use crate::log::{DRAFT_LOST, DRAFT_READY, PARTIAL_READ};
        let english = Strings::embedded(Lang::En)?;
        let japanese = Strings::embedded(Lang::Ja)?;
        for event in [PARTIAL_READ, DRAFT_READY, DRAFT_LOST] {
            for table in [&english, &japanese] {
                assert!(
                    table.messages.contains_key(event),
                    "a table lacks `{event}`"
                );
                assert!(!table.message_for(event).is_empty(), "`{event}` is empty");
            }
            assert_ne!(
                english.message_for(event),
                japanese.message_for(event),
                "`{event}` reads the same in both languages"
            );
        }
        Ok(())
    }

    /// T-BR-39-005
    #[test]
    fn the_asr_threads_help_line_and_refusal_are_in_both_languages() -> Result<(), String> {
        let english = Strings::embedded(Lang::En)?;
        let japanese = Strings::embedded(Lang::Ja)?;
        for table in [&english, &japanese] {
            assert!(
                table.cli_text(keys::HELP).contains("--asr-threads <n>"),
                "a help text lacks the option"
            );
            let refusal = table.cli_text(keys::ASR_THREADS_UNSUPPORTED);
            assert!(refusal.contains("--asr-threads"), "{refusal}");
            assert!(refusal.contains("32"), "{refusal}");
        }
        assert_ne!(
            english.cli_text(keys::ASR_THREADS_UNSUPPORTED),
            japanese.cli_text(keys::ASR_THREADS_UNSUPPORTED)
        );
        Ok(())
    }

    /// T-BR-40-003
    #[test]
    fn the_draft_marks_help_line_and_refusal_are_in_both_languages() -> Result<(), String> {
        let english = Strings::embedded(Lang::En)?;
        let japanese = Strings::embedded(Lang::Ja)?;
        for table in [&english, &japanese] {
            assert!(
                table
                    .cli_text(keys::HELP)
                    .contains("--draft-marks <on|off>"),
                "a help text lacks the option"
            );
            let refusal = table.cli_text(keys::DRAFT_MARKS_UNSUPPORTED);
            assert!(refusal.contains("--draft-marks"), "{refusal}");
            assert!(refusal.contains("off"), "{refusal}");
        }
        assert_ne!(
            english.cli_text(keys::DRAFT_MARKS_UNSUPPORTED),
            japanese.cli_text(keys::DRAFT_MARKS_UNSUPPORTED)
        );
        Ok(())
    }

    #[test]
    fn a_missing_key_reads_as_itself() -> Result<(), String> {
        let table = Strings::embedded(Lang::En)?;
        assert_eq!(table.cli_text("no-such-key"), "no-such-key");
        assert_eq!(table.message_for("no.such.event"), "no.such.event");
        Ok(())
    }

    #[test]
    fn the_locale_tags_resolve_to_a_language() {
        assert_eq!(resolve(Some("ja_JP.UTF-8")), Lang::Ja);
        assert_eq!(resolve(Some("ja-JP")), Lang::Ja);
        assert_eq!(resolve(Some("C.UTF-8")), Lang::En);
        assert_eq!(resolve(Some("fr_FR.UTF-8")), Lang::En);
        assert_eq!(resolve(None), Lang::En);
    }

    #[test]
    fn the_lines_of_a_multi_line_text_are_listed_one_by_one() -> Result<(), String> {
        let table = Strings::embedded(Lang::En)?;
        let lines = table.cli_lines();
        assert!(lines.contains("options:"));
        assert!(lines.contains("Unknown option."));
        Ok(())
    }
}

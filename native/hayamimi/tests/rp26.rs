//! RP-26: messages follow the locale. The two string tables hold the same
//! keys, the text the program prints (`--help` and the stop of a start) is
//! built only from table values, English output is ASCII, Japanese output is
//! not, and the locale resolves the way the design says.

mod support;

use std::collections::BTreeSet;

use hayamimi_app::notice::Notice;
use hayamimi_app::strings::{EN, JA, Strings, keys, resolve};
use hayamimi_asr::{Fallback, Reason, Stage, To};
use hayamimi_relay::lang::Lang;
use support::{Failure, HIDE_GPU, Output, TestResult, clip, models_dir, run};

/// A line of the `[cli]` table a fallback notice prints: the reason of a missing device.
const REASON: &str = "reason_no_device";

/// What the program printed in one locale.
#[derive(Debug, Clone)]
struct Printed {
    help: String,
    stop: String,
}

/// The keys of both tables, written `table.key`.
fn key_set(text: &str) -> Result<BTreeSet<String>, Failure> {
    let parsed: toml::Table = toml::from_str(text).map_err(|error| error.to_string())?;
    let mut keys = BTreeSet::new();
    for table in ["messages", "cli"] {
        let Some(entries) = parsed.get(table).and_then(toml::Value::as_table) else {
            return Err(format!("no [{table}] table"));
        };
        keys.extend(entries.keys().map(|key| format!("{table}.{key}")));
    }
    Ok(keys)
}

/// The two tables have the same keys; the error names the first key that only one has.
fn check_key_sets(en: &str, ja: &str) -> Result<(), Failure> {
    let (en, ja) = (key_set(en)?, key_set(ja)?);
    if let Some(key) = en.difference(&ja).next() {
        return Err(format!(
            "`{key}` is in the English table but not the Japanese"
        ));
    }
    if let Some(key) = ja.difference(&en).next() {
        return Err(format!(
            "`{key}` is in the Japanese table but not the English"
        ));
    }
    Ok(())
}

/// Every non-empty line of `text` is a line of some table text.
fn check_from_table(text: &str, table: &Strings) -> Result<(), Failure> {
    let known = table.cli_lines();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        if !known.contains(line) {
            return Err(format!("`{line}` is not in the table"));
        }
    }
    Ok(())
}

/// English output is built from the English table and is ASCII; Japanese
/// output is built from the Japanese table and holds non-ASCII text; the two differ.
fn check_locale_output(
    en: &Printed,
    ja: &Printed,
    tables: (&Strings, &Strings),
) -> Result<(), Failure> {
    if en.help == ja.help || en.stop == ja.stop {
        return Err(String::from("the output is the same in both locales"));
    }
    for text in [&en.help, &en.stop] {
        check_from_table(text, tables.0)?;
        if !text.is_ascii() {
            return Err(String::from("the English output is not ASCII"));
        }
    }
    for text in [&ja.help, &ja.stop] {
        check_from_table(text, tables.1)?;
    }
    if ja.help.is_ascii() || ja.stop.is_ascii() {
        return Err(String::from("the Japanese output is ASCII"));
    }
    Ok(())
}

fn tables() -> Result<(Strings, Strings), Failure> {
    Ok((Strings::parse(EN)?, Strings::parse(JA)?))
}

/// What a run printed, as the text of stderr.
fn stop_of(output: &Output) -> String {
    output.stderr.clone()
}

/// `--help` and the fallback text of a start without a CUDA device, in one locale.
fn printed_in(locale: &str) -> Result<Printed, Failure> {
    let help = run(&["--help"], Some(locale), &[])?;
    let wav = clip();
    let wav = wav.to_str().ok_or("the clip path is not UTF-8")?;
    let models = models_dir()?;
    let models = models.to_str().ok_or("the models path is not UTF-8")?;
    let args = ["--wav", wav, "--no-realtime", "--models", models];
    let stop = run(&args, Some(locale), &[HIDE_GPU])?;
    Ok(Printed {
        help: help.stdout,
        stop: stop_of(&stop),
    })
}

fn printed(help: &str, stop: &str) -> Printed {
    Printed {
        help: help.to_owned(),
        stop: stop.to_owned(),
    }
}

/// T-RP-26-001
#[test]
fn identical_key_sets_pass() -> TestResult {
    check_key_sets(EN, JA)?;
    Ok(())
}

/// T-RP-26-002
#[test]
fn missing_key_in_ja_is_named() {
    let without: String = JA
        .lines()
        .filter(|line| !line.starts_with("need_wav ="))
        .map(|line| format!("{line}\n"))
        .collect();
    let said = check_key_sets(EN, &without);
    assert!(
        said.as_ref()
            .is_err_and(|text| text.contains("cli.need_wav")),
        "{said:?}"
    );
}

/// T-RP-26-003
#[test]
fn extra_key_in_en_is_named() {
    let with = format!("{EN}\nextra_key = \"x\"\n");
    let said = check_key_sets(&with, JA);
    assert!(
        said.as_ref()
            .is_err_and(|text| text.contains("cli.extra_key")),
        "{said:?}"
    );
}

/// T-RP-26-004
#[test]
fn output_built_only_from_table_values_passes() -> TestResult {
    let (en, ja) = tables()?;
    let en_out = printed(
        en.cli_text(keys::HELP),
        &format!(
            "{}\n{}\n",
            en.cli_text(REASON),
            en.cli_text(keys::NO_GPU_CHECK)
        ),
    );
    let ja_out = printed(
        ja.cli_text(keys::HELP),
        &format!(
            "{}\n{}\n",
            ja.cli_text(REASON),
            ja.cli_text(keys::NO_GPU_CHECK)
        ),
    );
    check_locale_output(&en_out, &ja_out, (&en, &ja))?;
    Ok(())
}

/// T-RP-26-005
#[test]
fn hardcoded_line_not_in_the_table_is_named() -> TestResult {
    let (en, ja) = tables()?;
    let en_out = printed(
        en.cli_text(keys::HELP),
        &format!("{}\nNo GPU, sorry.\n", en.cli_text(REASON)),
    );
    let ja_out = printed(ja.cli_text(keys::HELP), ja.cli_text(REASON));
    let said = check_locale_output(&en_out, &ja_out, (&en, &ja));
    assert!(
        said.as_ref()
            .is_err_and(|text| text.contains("No GPU, sorry.")),
        "{said:?}"
    );
    Ok(())
}

/// T-RP-26-006
#[test]
fn en_is_ascii_and_ja_has_non_ascii_text() -> TestResult {
    let (en, ja) = tables()?;
    assert!(en.cli_text(keys::HELP).is_ascii());
    assert!(en.cli_text(REASON).is_ascii());
    assert!(!ja.cli_text(keys::HELP).is_ascii());
    assert!(!ja.cli_text(REASON).is_ascii());
    Ok(())
}

/// T-RP-26-007
#[test]
fn identical_output_for_both_locales_is_rejected() -> TestResult {
    let (en, _) = tables()?;
    let same = printed(en.cli_text(keys::HELP), en.cli_text(REASON));
    let said = check_locale_output(&same, &same, (&en, &en));
    assert!(
        said.as_ref().is_err_and(|text| text.contains("the same")),
        "{said:?}"
    );
    Ok(())
}

/// T-RP-26-008
#[test]
fn locale_resolution_defaults_to_english() {
    assert_eq!(resolve(Some("ja_JP.UTF-8")), Lang::Ja);
    assert_eq!(resolve(Some("C.UTF-8")), Lang::En);
    assert_eq!(resolve(Some("fr_FR.UTF-8")), Lang::En);
}

/// The lines a hidden-GPU run prints, built from the notice's keys.
fn fallback_stderr(table: &Strings) -> String {
    let notice = Notice::from_fallback(&Fallback {
        stage: Stage::Punct,
        reason: Reason::NoDevice,
        to: To::Cpu,
    });
    let mut text = String::new();
    for key in notice.stderr_keys() {
        text.push_str(table.cli_text(&key));
        text.push('\n');
    }
    text.push_str(table.cli_text(keys::NO_GPU_CHECK));
    text.push('\n');
    text
}

/// T-RP-26-010
#[test]
fn fallback_stderr_built_only_from_table_values_passes() -> TestResult {
    let (en, ja) = tables()?;
    check_from_table(&fallback_stderr(&en), &en)?;
    check_from_table(&fallback_stderr(&ja), &ja)?;
    assert!(fallback_stderr(&en).is_ascii());
    assert!(!fallback_stderr(&ja).is_ascii());
    Ok(())
}

/// T-RP-26-011
#[test]
fn hardcoded_fallback_line_is_named() -> TestResult {
    let (en, _) = tables()?;
    let text = format!("{}Running on the CPU now, sorry.\n", fallback_stderr(&en));
    let said = check_from_table(&text, &en);
    assert!(
        said.as_ref()
            .is_err_and(|line| line.contains("Running on the CPU now, sorry.")),
        "{said:?}"
    );
    Ok(())
}

/// The output of the real runs follows the locale. Windows takes the language
/// from the system, which `LANG` does not change, so there the output is only
/// required to be built from one of the two tables.
fn check_real_output() -> Result<(), Failure> {
    let (en_table, ja_table) = tables()?;
    let en = printed_in("C.UTF-8")?;
    let ja = printed_in("ja_JP.UTF-8")?;
    if cfg!(windows) {
        for text in [&en.help, &en.stop, &ja.help, &ja.stop] {
            check_from_table(text, &en_table).or_else(|_| check_from_table(text, &ja_table))?;
        }
        return Ok(());
    }
    check_locale_output(&en, &ja, (&en_table, &ja_table))
}

/// T-RP-26-009
#[test]
#[ignore = "runs the hayamimi binary"]
fn messages_follow_the_locale() -> TestResult {
    check_real_output()?;
    Ok(())
}

/// The acceptance test of RP-26: the key sets are equal and the real output follows the locale.
#[test]
#[ignore = "runs the hayamimi binary"]
fn rp26_messages_follow_the_locale() -> TestResult {
    check_key_sets(EN, JA)?;
    check_real_output()?;
    Ok(())
}

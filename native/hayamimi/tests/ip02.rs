//! IP-2: a run of `lat-dialog` is recorded without gaps (this is what passes),
//! and each utterance of it is read in its own language (this is only recorded).
//!
//! The real run (T-IP-02-001) starts `hayamimi`, the relay and `hayamimi-eval
//! stream` against the translation server of the machine; it is `#[ignore]`d and
//! runs with `-- --ignored`, as does the row that reads the real manifest.
//! Every other row feeds the counting tools (`support/dialog.rs`) a record made
//! by hand: samples that start 10 s apart, and finals that arrive 6 s after
//! their utterance began, so that each final belongs to the utterance before it.
//!
//! The record file of the real run is `docs/results/in-person/ip02.md`. The line
//! it is judged by is a provisional value: a count over it is written down as
//! "線の外" and does not fail the test.

mod support;

use std::error::Error;
use std::path::PathBuf;

use hayamimi_eval::latency::Sample;
use serde_json::json;
use support::dialog::{
    Cell, Tally, Verdict, cells_of, check_complete, count_by_step, count_by_step_in, langs_of,
    manifest_rows, match_fallbacks, record_dialog_run, record_verdict, same_language_translations,
    switches, tally,
};
use support::dialog_run::run_stack;
use support::{TestResult, find_above, read};

/// Utterances of the hand-made record, and their repeats.
const UTTERANCES: u32 = 4;
const REPEATS: u32 = 3;
/// The provisional line of mixed-up utterances out of 30.
const LINE: usize = 2;
/// Utterances of `lat-dialog`, and the repeats of a run.
const DIALOG_UTTERANCES: u32 = 30;
const DIALOG_REPEATS: u32 = 3;
/// The record of the real run.
const RECORD: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/results/in-person/ip02.md"
);

fn langs4() -> Vec<String> {
    ["ja", "en", "ja", "en"].map(String::from).to_vec()
}

/// Where utterance `utt` of `repeat` starts on the run's clock, in microseconds.
fn start_of(repeat: u32, utt: u32) -> i64 {
    i64::from((repeat - 1) * UTTERANCES + (utt - 1)) * 10_000_000
}

fn sample(repeat: u32, utt: u32) -> Sample {
    Sample {
        repeat,
        utt,
        t_start_us: start_of(repeat, utt),
        t_end_us: start_of(repeat, utt) + 5_000_000,
        t_partial_us: None,
        t_partial_tr_us: None,
        t_final_us: None,
        t_tr_us: None,
        finals: 0,
        text: String::new(),
        translation: None,
        final_latency_ms: None,
        cer_dist: 0,
        cer_denom: 0,
    }
}

/// The samples file of a run of `REPEATS` copies of `UTTERANCES` utterances.
fn samples_text() -> Result<String, Box<dyn Error>> {
    let mut text = String::new();
    for repeat in 1..=REPEATS {
        for utt in 1..=UTTERANCES {
            text.push_str(&serde_json::to_string(&sample(repeat, utt))?);
            text.push('\n');
        }
    }
    Ok(text)
}

fn event_line(t_us: i64, event: serde_json::Value) -> String {
    json!({ "t_us": t_us, "event": event }).to_string()
}

/// The events file of a run in which `langs_of` names the languages of the
/// finals that utterance `utt` of `repeat` was given, in the order they came.
/// The finals are numbered from 1 in the order of the utterances, as one
/// process numbers them.
fn events_text(langs_of: impl Fn(u32, u32) -> Vec<&'static str>) -> String {
    let mut text = String::new();
    let mut number = 0;
    for repeat in 1..=REPEATS {
        for utt in 1..=UTTERANCES {
            for (at, lang) in (0_i64..).zip(langs_of(repeat, utt)) {
                number += 1;
                let t_us = start_of(repeat, utt) + 6_000_000 + at * 1000;
                let event = json!({
                    "type": "final", "id": format!("file-{number}"), "lang": lang, "text": "x"
                });
                text.push_str(&event_line(t_us, event));
                text.push('\n');
            }
        }
    }
    text
}

/// A run in which every utterance was read in its manifest language (`ja`,
/// `en`, `ja`, `en`), whatever the repeat.
fn right_in(_repeat: u32, utt: u32) -> Vec<&'static str> {
    if utt % 2 == 1 { vec!["ja"] } else { vec!["en"] }
}

fn counted(events: &str) -> Result<Tally, Box<dyn Error>> {
    Ok(tally(&cells_of(&samples_text()?, events)?, &langs4())?)
}

fn scratch(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name)
}

/// T-IP-02-001
#[test]
#[ignore = "runs the hayamimi binary with the models, which are not in git"]
fn ip02_each_dialog_utterance_is_read_in_its_own_language() -> TestResult {
    let wav = find_above("testdata/lat-dialog.wav")?;
    let rows = manifest_rows(&find_above("testdata/manifests/lat-dialog.json")?)?;
    let langs = langs_of(&rows)?;
    let record = PathBuf::from(RECORD);
    std::fs::write(&record, "# IP-2 record\n")?;
    let results = record
        .parent()
        .ok_or("the record has no directory")?
        .to_path_buf();
    for mode in ["pair", "fast"] {
        let out = results.join(format!("ip02_{mode}.jsonl"));
        run_stack(mode, &wav, DIALOG_REPEATS, &out)?;
        let samples = read(&out)?;
        assert_eq!(
            check_complete(&samples, DIALOG_REPEATS, DIALOG_UTTERANCES)?,
            90
        );
        for suffix in ["events.jsonl", "summary.json", "machine.jsonl"] {
            assert!(out.with_extension(suffix).is_file(), "{mode}: no {suffix}");
        }
        let machine = read(&out.with_extension("machine.jsonl"))?;
        assert!(!machine.contains("stream.machine.refused"), "{mode}");
        let events = read(&out.with_extension("events.jsonl"))?;
        assert!(events.contains("\"type\":\"final\""), "{mode}: no final");
        record_dialog_run(&record, mode, &out, &langs, LINE)?;
    }
    Ok(())
}

/// T-IP-02-002
#[test]
fn a_record_of_three_repeats_of_every_utterance_is_complete() -> TestResult {
    assert_eq!(check_complete(&samples_text()?, REPEATS, UTTERANCES)?, 12);
    Ok(())
}

/// T-IP-02-003
#[test]
fn a_record_with_a_missing_line_or_no_line_is_refused() -> TestResult {
    let whole = samples_text()?;
    let lines: Vec<&str> = whole.lines().collect();
    let Some((_, kept)) = lines.split_last() else {
        return Err("the hand-made record has no line".into());
    };
    let without_last: String = kept.iter().flat_map(|line| [*line, "\n"]).collect();
    let said = check_complete(&without_last, REPEATS, UTTERANCES);
    assert!(
        matches!(&said, Err(message) if message.contains("repeat 3") && message.contains("utt 4")),
        "{said:?}"
    );
    assert!(check_complete("", REPEATS, UTTERANCES).is_err());
    Ok(())
}

/// A run in which utterance `utt` was read in `langs[0]`, `langs[1]` and
/// `langs[2]` in its three repeats, and every other one in its own language.
fn wrong_in_utt(utt: u32, langs: [&'static str; 3]) -> impl Fn(u32, u32) -> Vec<&'static str> {
    let [first, second, third] = langs;
    move |repeat, at| {
        if at != utt {
            return right_in(repeat, at);
        }
        match repeat {
            1 => vec![first],
            2 => vec![second],
            _ => vec![third],
        }
    }
}

/// T-IP-02-004
#[test]
fn an_utterance_read_wrongly_in_two_of_three_repeats_is_mixed_up() -> TestResult {
    let got = counted(&events_text(wrong_in_utt(2, ["en", "ja", "ja"])))?;
    assert_eq!((got.mixed_up, got.en_as_ja, got.ja_as_en), (1, 1, 0));
    Ok(())
}

/// T-IP-02-005
#[test]
fn an_utterance_read_wrongly_in_one_of_three_repeats_is_not_mixed_up() -> TestResult {
    let got = counted(&events_text(wrong_in_utt(3, ["en", "ja", "ja"])))?;
    assert_eq!(got.mixed_up, 0);
    Ok(())
}

/// T-IP-02-006
#[test]
fn a_run_where_every_final_has_the_utterances_language_has_no_mixed_up_utterance() -> TestResult {
    let got = counted(&events_text(right_in))?;
    assert_eq!((got.mixed_up, got.other, got.other_finals), (0, 0, 0));
    Ok(())
}

/// T-IP-02-007
#[test]
fn a_run_forced_to_ja_mixes_up_every_english_utterance() -> TestResult {
    let rows = 30_u32;
    let manifest: Vec<String> = (1..=rows)
        .map(|utt| String::from(if utt % 2 == 1 { "ja" } else { "en" }))
        .collect();
    let cells: Vec<Cell> = (1..=3)
        .flat_map(|repeat| {
            (1..=rows).map(move |utt| Cell {
                repeat,
                utt,
                langs: vec![String::from("ja")],
            })
        })
        .collect();
    let got = tally(&cells, &manifest)?;
    assert_eq!((got.mixed_up, got.en_as_ja, got.ja_as_en), (15, 15, 0));
    assert!(got.mixed_up > LINE);
    Ok(())
}

/// T-IP-02-008
#[test]
fn two_mixed_up_utterances_are_on_the_line_and_three_are_over_it() -> TestResult {
    let path = scratch("ip02_verdicts.txt");
    std::fs::write(&path, "")?;
    assert_eq!(
        record_verdict(&path, "mixed-up utterances", 2, LINE)?,
        Verdict::Inside
    );
    assert_eq!(
        record_verdict(&path, "mixed-up utterances", 3, LINE)?,
        Verdict::Outside
    );
    let written = read(&path)?;
    for word in ["線の内", "線の外", "(線 2)"] {
        assert!(written.contains(word), "{word} is not in: {written}");
    }
    Ok(())
}

/// T-IP-02-009
#[test]
fn a_final_in_a_third_language_is_counted() -> TestResult {
    let with_zh = |repeat: u32, utt: u32| {
        if (repeat, utt) == (1, 1) {
            vec!["zh"]
        } else {
            right_in(repeat, utt)
        }
    };
    assert_eq!(counted(&events_text(with_zh))?.other_finals, 1);
    assert_eq!(counted(&events_text(right_in))?.other_finals, 0);
    Ok(())
}

/// T-IP-02-010
#[test]
fn a_translation_into_its_own_source_language_is_counted() -> TestResult {
    let translation = |from: &str, into: &str| {
        event_line(
            1,
            json!({ "type": "translation", "id": "file-1", "src_lang": from, "lang": into, "text": "x" }),
        )
    };
    let mixed = [translation("ja", "ja"), translation("ja", "en")].join("\n");
    let clean = [translation("ja", "en"), translation("en", "ja")].join("\n");
    assert_eq!(same_language_translations(&mixed)?, 1);
    assert_eq!(same_language_translations(&clean)?, 0);
    Ok(())
}

fn fallback_line(id: u64, step: &str, level: &str) -> String {
    json!({ "event": "asr.lang.fallback", "level": level, "id": id, "step": step, "lid": null })
        .to_string()
}

/// T-IP-02-011
#[test]
fn language_fallback_events_are_counted_by_step_and_a_missing_log_is_an_error() -> TestResult {
    let log = [
        fallback_line(1, "sv_tag", "info"),
        fallback_line(2, "sv_tag", "info"),
        fallback_line(3, "default", "info"),
        fallback_line(4, "sv_failed", "warn"),
    ]
    .join("\n");
    assert_eq!(count_by_step(&log)?, [2, 0, 1, 1]);
    assert_eq!(count_by_step("")?, [0, 0, 0, 0]);
    let missing = scratch("ip02_no_such_run.log");
    let said = count_by_step_in(&missing);
    assert!(
        matches!(&said, Err(message) if message.contains("ip02_no_such_run.log")),
        "{said:?}"
    );
    let odd = fallback_line(5, "guessed", "info");
    let said = count_by_step(&odd);
    assert!(
        matches!(&said, Err(message) if message.contains("guessed")),
        "{said:?}"
    );
    Ok(())
}

/// T-IP-02-012
#[test]
fn an_utterance_split_into_two_finals_is_counted_once_and_a_wrong_one_among_them_makes_it_wrong()
-> TestResult {
    let split = |repeat: u32, utt: u32| {
        if utt == 1 {
            vec!["ja", "ja"]
        } else {
            right_in(repeat, utt)
        }
    };
    let got = counted(&events_text(split))?;
    assert_eq!((got.split, got.mixed_up), (1, 0));
    let split_wrong = |repeat: u32, utt: u32| {
        if utt == 1 {
            vec!["ja", "en"]
        } else {
            right_in(repeat, utt)
        }
    };
    let got = counted(&events_text(split_wrong))?;
    assert_eq!((got.mixed_up, got.ja_as_en), (1, 1));
    Ok(())
}

/// T-IP-02-013
#[test]
fn a_repeat_with_no_final_is_not_counted_as_right_or_wrong() -> TestResult {
    let silent_once = |repeat: u32, utt: u32| {
        if (repeat, utt) == (2, 4) {
            vec![]
        } else {
            right_in(repeat, utt)
        }
    };
    let got = counted(&events_text(silent_once))?;
    assert_eq!((got.mixed_up, got.no_final_repeats), (0, 1));
    assert!(got.undecided.is_empty());
    let never = |repeat: u32, utt: u32| {
        if utt == 4 {
            vec![]
        } else {
            right_in(repeat, utt)
        }
    };
    let got = counted(&events_text(never))?;
    assert_eq!((got.no_final_repeats, got.undecided), (3, vec![4]));
    Ok(())
}

/// T-IP-02-014
#[test]
fn a_manifest_that_alternates_has_one_switch_fewer_than_utterances() {
    let alternating: Vec<String> = (0..30)
        .map(|at| String::from(if at % 2 == 0 { "ja" } else { "en" }))
        .collect();
    assert_eq!(switches(&alternating), 29);
    let blocks: Vec<String> = (0..30)
        .map(|at| String::from(if at < 15 { "ja" } else { "en" }))
        .collect();
    assert_eq!(switches(&blocks), 1);
}

/// T-IP-02-015
#[test]
#[ignore = "reads testdata or run records, which are not in git"]
fn lat_dialog_has_thirty_utterances_and_twenty_nine_switches() -> TestResult {
    let rows = manifest_rows(&find_above("testdata/manifests/lat-dialog.json")?)?;
    let langs = langs_of(&rows)?;
    assert_eq!(langs.len(), 30);
    for lang in ["ja", "en"] {
        assert_eq!(
            langs.iter().filter(|got| *got == lang).count(),
            15,
            "{lang}"
        );
    }
    assert_eq!(switches(&langs), 29);
    Ok(())
}

/// A log with fallback lines for `ids`, and the events of one run of six finals.
fn run_of_six(ids: &[u64]) -> (String, String) {
    let log = ids
        .iter()
        .map(|id| fallback_line(*id, "sv_tag", "info"))
        .collect::<Vec<_>>()
        .join("\n");
    let events = (1..=6)
        .map(|number| {
            let event = json!({ "type": "final", "id": format!("file-{number}"), "lang": "ja", "text": "x" });
            event_line(number, event)
        })
        .collect::<Vec<_>>()
        .join("\n");
    (log, events)
}

/// T-IP-02-016
#[test]
fn a_fallback_event_is_matched_to_its_final_by_id_across_repeats() -> TestResult {
    let (log, events) = run_of_six(&[3, 5]);
    assert_eq!(match_fallbacks(&log, &events)?, 2);
    Ok(())
}

/// T-IP-02-017
#[test]
fn a_fallback_event_twice_for_one_final_or_for_no_final_is_an_error() -> TestResult {
    let (_, events) = run_of_six(&[]);
    let twice = [
        fallback_line(3, "sv_tag", "info"),
        fallback_line(3, "script", "info"),
    ]
    .join("\n");
    let said = match_fallbacks(&twice, &events);
    assert!(
        matches!(&said, Err(message) if message.contains("id 3") && message.contains("2")),
        "{said:?}"
    );
    let orphan = fallback_line(9, "sv_tag", "info");
    let said = match_fallbacks(&orphan, &events);
    assert!(
        matches!(&said, Err(message) if message.contains("id 9") && message.contains("no final")),
        "{said:?}"
    );
    assert_eq!(match_fallbacks("", &events)?, 0);
    Ok(())
}

/// T-IP-02-018
#[test]
fn records_of_two_runs_mixed_together_are_refused_and_one_run_is_matched() -> TestResult {
    let (log, one_run) = run_of_six(&[3]);
    let mixed = format!("{one_run}\n{one_run}");
    let said = match_fallbacks(&log, &mixed);
    assert!(
        matches!(&said, Err(message) if message.contains("file-3") && message.contains("2 finals")),
        "{said:?}"
    );
    let (log, events) = run_of_six(&[3, 5]);
    assert_eq!(match_fallbacks(&log, &events)?, 2);
    Ok(())
}

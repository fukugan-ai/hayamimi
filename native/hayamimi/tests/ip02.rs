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
use std::path::{Path, PathBuf};

use hayamimi_eval::latency::Sample;
use serde_json::json;
use support::dialog::{
    Cell, Silent, Tally, Verdict, cells_of, check_complete, count_by_reason, count_by_step,
    count_by_step_in, given_of, langs_of, manifest_rows, match_fallbacks, record_dialog_run,
    record_verdict, same_language_translations, split_undecided, switches, tally, unread_rows,
};
use support::dialog_run::{find_llama, run_stack};
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

/// An `asr.segment.unread` line that ends at `end_s` on the stream.
fn unread_line(end_s: f64, reason: &str) -> String {
    json!({
        "event": "asr.segment.unread", "level": "info", "start_s": end_s - 1.0,
        "end_s": end_s, "reason": reason, "lang": "ja", "tier": "rz"
    })
    .to_string()
}

/// A run in which every utterance but those in `silent` was read right.
fn right_but(silent: &'static [u32]) -> impl Fn(u32, u32) -> Vec<&'static str> {
    move |repeat, utt| {
        if silent.contains(&utt) {
            vec![]
        } else {
            right_in(repeat, utt)
        }
    }
}

/// How the utterances that got no final in a run (those in `silent`) are
/// sorted when `unread` lines ended at `ends`, in seconds on the stream.
fn split_for(silent: &'static [u32], ends: &[f64]) -> Result<Silent, Box<dyn Error>> {
    let counted = counted(&events_text(right_but(silent)))?;
    let log = ends
        .iter()
        .map(|end_s| unread_line(*end_s, "empty_decode"))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(split_undecided(
        &samples_text()?,
        &counted.undecided,
        &unread_rows(&log)?,
    )?)
}

/// IP-2 (design 10.9.2, 6): an utterance with no final in any repeat is
/// "read as nothing" when a segment that read as nothing ended in its window
/// in some repeat, and has "no segment" when none did in any repeat.
#[test]
fn an_utterance_with_no_final_has_a_segment_only_when_one_ended_in_its_window() -> TestResult {
    // Utterance 4 of repeat 1 starts at 30 s and its window ends at 40 s.
    let inside = split_for(&[4], &[33.0])?;
    assert_eq!((inside.no_segment, inside.read_empty), (vec![], vec![4]));
    let nowhere = split_for(&[4], &[23.0, 47.0])?;
    assert_eq!((nowhere.no_segment, nowhere.read_empty), (vec![4], vec![]));
    let none_at_all = split_for(&[4], &[])?;
    assert_eq!(none_at_all.no_segment, vec![4]);
    // Repeat 3 starts utterance 4 at 110 s, and the last window has no end.
    let in_a_later_repeat = split_for(&[4], &[112.0, 500.0])?;
    assert_eq!(in_a_later_repeat.read_empty, vec![4]);
    Ok(())
}

/// The window is closed at its start and open at the next start.
#[test]
fn a_segment_ending_where_the_next_utterance_starts_belongs_to_the_next_one() -> TestResult {
    let at_start = split_for(&[4], &[30.0])?;
    assert_eq!(at_start.read_empty, vec![4]);
    let at_next_start = split_for(&[4], &[40.0])?;
    assert_eq!(at_next_start.no_segment, vec![4]);
    Ok(())
}

/// Two utterances with no final are sorted one by one.
#[test]
fn utterances_with_no_final_are_sorted_into_the_two_kinds_one_by_one() -> TestResult {
    let got = split_for(&[3, 4], &[15.0, 31.0])?;
    assert_eq!((got.no_segment, got.read_empty), (vec![3], vec![4]));
    // An utterance that has a final is in neither list, whatever ended in its window.
    let got = split_for(&[4], &[3.0, 13.0])?;
    assert_eq!((got.no_segment, got.read_empty), (vec![4], vec![]));
    Ok(())
}

/// IP-2: the lines of `asr.segment.unread` are counted by reason, other
/// events are not read, and a reason outside the two or a span that is not a
/// number is an error.
#[test]
fn unread_segments_are_counted_by_reason_and_an_odd_line_is_an_error() -> TestResult {
    let log = [
        unread_line(3.0, "empty_decode"),
        unread_line(4.0, "empty_text"),
        unread_line(5.0, "empty_decode"),
        fallback_line(1, "sv_tag", "info"),
    ]
    .join("\n");
    assert_eq!(count_by_reason(&unread_rows(&log)?), [2, 1]);
    assert_eq!(count_by_reason(&unread_rows("")?), [0, 0]);
    let said = unread_rows(&unread_line(3.0, "guessed"));
    assert!(
        matches!(&said, Err(message) if message.contains("guessed")),
        "{said:?}"
    );
    let no_end = json!({ "event": "asr.segment.unread", "reason": "empty_text" }).to_string();
    let said = unread_rows(&no_end);
    assert!(
        matches!(&said, Err(message) if message.contains("end_s")),
        "{said:?}"
    );
    Ok(())
}

/// IP-2 and IP-5: a final is kept with the id the server gave it, in the
/// order the run placed the finals of an utterance, and `cells_of` reads the
/// languages of the same placement.
#[test]
fn the_finals_of_an_utterance_keep_their_ids_in_placement_order() -> TestResult {
    let split = |repeat: u32, utt: u32| {
        if utt == 1 {
            vec!["ja", "en"]
        } else {
            right_in(repeat, utt)
        }
    };
    let events = events_text(split);
    let given = given_of(&samples_text()?, &events)?;
    let first = given.first().ok_or("no utterance was given")?;
    let placed: Vec<(&str, Option<&str>)> = first
        .finals
        .iter()
        .map(|done| (done.lang.as_str(), done.id.as_deref()))
        .collect();
    assert_eq!(
        (first.repeat, first.utt, placed),
        (1, 1, vec![("ja", Some("file-1")), ("en", Some("file-2"))])
    );
    let cells = cells_of(&samples_text()?, &events)?;
    assert_eq!(cells.len(), given.len());
    let langs: Vec<&str> = cells
        .first()
        .ok_or("no cell")?
        .langs
        .iter()
        .map(String::as_str)
        .collect();
    assert_eq!(langs, ["ja", "en"]);
    Ok(())
}

/// The record of a run names the counts by reason and both kinds of
/// utterance with no final; a log that cannot be read is written down as it
/// is said and does not stop the record.
#[test]
fn the_record_of_a_run_names_the_unread_segments_and_the_two_kinds_of_silent_utterance()
-> TestResult {
    let out = scratch("ip02_unread.jsonl");
    std::fs::write(&out, samples_text()?)?;
    std::fs::write(
        out.with_extension("events.jsonl"),
        events_text(right_but(&[3, 4])),
    )?;
    std::fs::write(
        out.with_extension("log"),
        [
            unread_line(31.0, "empty_decode"),
            unread_line(32.5, "empty_text"),
        ]
        .join("\n"),
    )?;
    let record = scratch("ip02_unread.md");
    std::fs::write(&record, "")?;
    record_dialog_run(&record, "unread", &out, &langs4(), LINE)?;
    let written = read(&record)?;
    for line in [
        "asr.segment.unread empty_decode: 1",
        "asr.segment.unread empty_text: 1",
        "utterances with no segment: [3]",
        "utterances read as nothing: [4]",
    ] {
        assert!(written.contains(line), "`{line}` is not in: {written}");
    }
    std::fs::remove_file(out.with_extension("log"))?;
    std::fs::write(&record, "")?;
    record_dialog_run(&record, "no log", &out, &langs4(), LINE)?;
    let written = read(&record)?;
    assert!(written.contains("asr.segment.unread: cannot read the log"));
    assert!(!written.contains("utterances read as nothing"));
    Ok(())
}

/// The command line of a `llama-server` started with `--port <port>`.
fn server_words(port: &str) -> Vec<&str> {
    vec!["llama-server", "-m", "model.gguf", "--port", port]
}

/// A proc root of its own under the scratch directory: each `(pid, words)` is a
/// `llama-server` with that command line, words ended by a NUL as the kernel
/// writes them. Nothing is left from an earlier run of the same name.
fn llama_root(name: &str, servers: &[(u32, Vec<&str>)]) -> Result<PathBuf, Box<dyn Error>> {
    let root = scratch(name);
    if root.exists() {
        std::fs::remove_dir_all(&root)?;
    }
    for (pid, words) in servers {
        let dir = root.join(pid.to_string());
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("comm"), "llama-server\n")?;
        let cmdline: Vec<u8> = words
            .iter()
            .flat_map(|word| word.bytes().chain([0]))
            .collect();
        std::fs::write(dir.join("cmdline"), cmdline)?;
    }
    Ok(root)
}

/// What `find_llama` says when it refuses the servers of `root` on port 18084.
fn refusal_on_18084(root: &Path) -> Result<String, Box<dyn Error>> {
    match find_llama(root, 18084) {
        Err(said) => Ok(said),
        Ok(llama) => Err(format!("llama-server {} was picked", llama.pid).into()),
    }
}

/// T-IP-02-019
#[test]
fn the_run_picks_the_llama_server_on_its_own_port_beside_another_one() -> TestResult {
    let other = (4001, server_words("18081"));
    let own = (4002, server_words("18084"));
    let beside = llama_root("ip02_llama_beside", &[other, own.clone()])?;
    let alone = llama_root("ip02_llama_alone", &[own])?;
    for root in [&beside, &alone] {
        let llama = find_llama(root, 18084)?;
        assert_eq!((llama.pid, llama.port), (4002, 18084));
    }
    Ok(())
}

/// T-IP-02-020
#[test]
fn a_run_without_exactly_one_llama_server_on_its_own_port_is_refused() -> TestResult {
    let on_other = llama_root("ip02_llama_other", &[(4001, server_words("18081"))])?;
    let padded = llama_root("ip02_llama_padded", &[(4001, server_words("018084"))])?;
    let two_flags = vec!["llama-server", "--port", "8080", "--port", "18084"];
    let doubled = llama_root("ip02_llama_flags", &[(4001, two_flags)])?;
    for root in [&on_other, &padded, &doubled] {
        let said = refusal_on_18084(root)?;
        assert!(said.contains("18084"), "{said}");
        assert!(said.contains("hayamimi-llama-e2e.service"), "{said}");
    }
    let twice = [(4003, server_words("18084")), (4004, server_words("18084"))];
    let said = refusal_on_18084(&llama_root("ip02_llama_twice", &twice)?)?;
    assert!(said.contains("4003") && said.contains("4004"), "{said}");
    // The same function picks the one server of a root that has exactly one.
    let one = llama_root("ip02_llama_one", &[(4003, server_words("18084"))])?;
    assert_eq!(find_llama(&one, 18084)?.pid, 4003);
    Ok(())
}

/// A child of the stack that writes more than a pipe holds is read from while it runs.
#[cfg(unix)]
mod drained {
    use std::process::Child;
    use std::time::{Duration, Instant};

    use crate::support::dialog_run::drain_output;
    use crate::support::{TestResult, spawn_binary};

    /// Writes 4 000 000 bytes to each of stdout and stderr: several times what a pipe
    /// holds (64 KiB by default on Linux; a little over 1 MiB on the WSL2 kernel).
    const WRITES_MUCH: &str = "head -c 4000000 /dev/zero; head -c 4000000 /dev/zero >&2";

    /// Whether `child` ended within `within`.
    fn ends_within(child: &mut Child, within: Duration) -> Result<bool, std::io::Error> {
        let deadline = Instant::now() + within;
        while Instant::now() < deadline {
            if child.try_wait()?.is_some() {
                return Ok(true);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Ok(false)
    }

    /// T-IP-02-022
    #[test]
    fn a_child_whose_output_is_drained_never_stops_on_a_full_pipe() -> TestResult {
        let mut drained = spawn_binary("sh", &["-c", WRITES_MUCH], None, &[])?;
        drain_output(&mut drained);
        assert!(
            ends_within(&mut drained, Duration::from_secs(20))?,
            "the drained child did not end"
        );
        let mut unread = spawn_binary("sh", &["-c", WRITES_MUCH], None, &[])?;
        let stopped = !ends_within(&mut unread, Duration::from_secs(2))?;
        unread.kill()?;
        unread.wait()?;
        assert!(
            stopped,
            "a child nobody reads from ended, so the test cannot tell a full pipe"
        );
        Ok(())
    }
}

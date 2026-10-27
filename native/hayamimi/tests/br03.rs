//! BR-3: the budget of the wait from the end of speech to the page (design 10.13.3).
//!
//! The real run (T-BR-03-001) plays `lat-dialog` three times through the stack
//! of IP-2 with the translation server the window's unit starts (port 18084),
//! with a page open that times itself (BR-2), and puts the stages of BR-1 and
//! the page stage together. It is `#[ignore]`d because it needs the models,
//! `testdata/` (not in git), the page bundle, a browser and that server. Until
//! the speed measures of B2 are in (`JUDGE_AFTER_B2`), it writes the values and
//! the files of the run to `docs/results/in-person/<step>.*` (each step of the
//! speed work has files of its own) and judges nothing.
//!
//! Two more real runs (T-BR-03-017 and 018) start the programs with the
//! measures of IP-S8 and of IP-S7 switched off by their flags, so that the
//! values of the programs as they are can be set beside the values without each
//! measure, taken on the same machine. Two more (T-BR-36-012 and 013) play the
//! dialog with a kept sentence used as it is, and with that measure of IP-S17
//! switched off, to be set beside each other the same way. The rows T-BR-36-014,
//! 015 and 019 read the log and the events of those two records: the plans the
//! `silence` closes were read by, the finals that differ between the runs, and
//! the end of the finals made from a kept sentence. Two more (T-BR-37-012 and
//! 013) play it with the language of a final decided in the middle of an
//! utterance, and with that measure of IP-S20 switched off. The row T-BR-37-014
//! reads the log and the events of those two records: the finals of the two
//! runs, the finals whose language was the answer of the question put when the
//! voice reached four seconds, and those questions. Two more (T-BR-38-018 and
//! 019) play it with the drafts read by a worker thread of IP-S19, and with
//! them read on the thread that feeds the detector.
//!
//! The rows 003 to 016 test the function that judges the budget, on waits
//! written by hand; the row 002 reads the pair run of IP-4, which no later run
//! rewrites. The rows T-BR-04-015 to 017 test the record's split of the draft
//! translation by language, on utterances written by hand, and the rows
//! T-BR-04-018 to 022 the line of the draft translation, on the same kind of
//! utterances. The row T-BR-04-009 reads a copy of the recorded run with IP-S7
//! and IP-S8 on, which no later run rewrites. The row 019 reads the sources of
//! the tests: a record that a real run rewrites is named only by the test that
//! owns the run, so a run recorded again cannot turn the hand-counted values of
//! another test red.

mod support;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use hayamimi_eval::latency::{Stage, StageReport, StageRow, stage_report};
use serde_json::Value;
use support::dialog::{
    Wait, draft_by_language_text, join_page, judge_budget, render_baseline, spoken_in,
};
use support::dialog_run::{Flags, LLAMA_PORT, Playback, Translator, run_paged_with};
use support::{Failure, REPO, TestResult, find_above, read};

/// Whether the run judges the budget. The speed measures of B2 (design 10.13.4,
/// IP-S7 and IP-S8) are not in, so the lines are recorded and not applied.
const JUDGE_AFTER_B2: bool = false;
/// How many times the real run plays `lat-dialog`.
const REPEATS: u32 = 3;

/// How a real run starts the two programs, and the files its record goes to
/// (design 10.13.9). Each step has files of its own, so the values of the steps
/// before it stay.
struct Variant {
    step: &'static str,
    title: &'static str,
    flags: Flags<'static>,
    /// Whether the run is judged against the line of the draft translation
    /// (see [`judge_draft_p50`]): only the run that has IP-S7 on is.
    judges_draft: bool,
}

/// The programs as they are, with IP-S7 and IP-S8 on.
const CURRENT: Variant = Variant {
    step: "br03_ip_s8",
    title: "BR-3 の IP-S7 と IP-S8 の後の値",
    flags: Flags {
        hayamimi: &[],
        relay: &[],
    },
    judges_draft: true,
};

/// IP-S7 on and IP-S8 off.
const LID_ONLY: Variant = Variant {
    step: "br03_ip_s8_lid_only",
    title: "BR-3 の IP-S7 だけの値（IP-S8 を旗で外した run）",
    flags: Flags {
        hayamimi: &[],
        relay: &["--partial-yield", "off"],
    },
    judges_draft: false,
};

/// IP-S7 and IP-S8 both off: the programs as they were before IP-S7.
const BOTH_OFF: Variant = Variant {
    step: "br03_ip_s8_both_off",
    title: "BR-3 の IP-S7 と IP-S8 を旗で外した値",
    flags: Flags {
        hayamimi: &["--partial-lid", "off"],
        relay: &["--partial-yield", "off"],
    },
    judges_draft: false,
};

/// The programs as they are after IP-S17: a kept sentence is used when it was
/// read to a point inside the silence that closed the segment.
const HELD_COVERING: Variant = Variant {
    step: "br03_ip_s17",
    title: "BR-3 の IP-S17 の後の値（持った文を無音の中まで使う）",
    flags: Flags {
        hayamimi: &[],
        relay: &[],
    },
    judges_draft: false,
};

/// IP-S17 off: a kept sentence is used only when it ends within one window of
/// the end of the segment.
const HELD_STRICT: Variant = Variant {
    step: "br03_ip_s17_strict",
    title: "BR-3 の IP-S17 を旗で外した値（持った文を 1 窓の内でだけ使う）",
    flags: Flags {
        hayamimi: &["--held-reuse", "strict"],
        relay: &[],
    },
    judges_draft: false,
};

/// The programs as they are after IP-S20: the language of a final is decided
/// when the voice reaches four seconds, not when the segment closes.
const FINAL_LID_MIDWAY: Variant = Variant {
    step: "br03_ip_s20",
    title: "BR-3 の IP-S20 の後の値（確定文の言語を声が 4 秒に届いた時に決める）",
    flags: Flags {
        hayamimi: &[],
        relay: &[],
    },
    judges_draft: false,
};

/// IP-S20 off: the language of a final is asked when the segment closes.
const FINAL_LID_CLOSE: Variant = Variant {
    step: "br03_ip_s20_close",
    title: "BR-3 の IP-S20 を旗で外した値（確定文の言語を閉じる時に決める）",
    flags: Flags {
        hayamimi: &["--final-lid", "close"],
        relay: &[],
    },
    judges_draft: false,
};

/// The programs with the drafts read by a worker thread of their own (IP-S19).
const DRAFT_WORKER_ON: Variant = Variant {
    step: "br03_ip_s19",
    title: "BR-3 の IP-S19 の後の値（速報を専用の thread で読む）",
    flags: Flags {
        hayamimi: &["--draft-worker", "on"],
        relay: &[],
    },
    judges_draft: false,
};

/// IP-S19 off: the drafts are read on the thread that feeds the detector.
const DRAFT_WORKER_OFF: Variant = Variant {
    step: "br03_ip_s19_off",
    title: "BR-3 の IP-S19 を旗で外した値（速報を音を渡す thread で読む）",
    flags: Flags {
        hayamimi: &["--draft-worker", "off"],
        relay: &[],
    },
    judges_draft: false,
};

/// The file of the variant's record that ends in `suffix`.
fn record_of(variant: &Variant, suffix: &str) -> PathBuf {
    Path::new(REPO)
        .join("docs/results/in-person")
        .join(format!("{}{suffix}", variant.step))
}

/// Writes `bytes` to `path`, naming the path when it cannot.
fn keep(path: &Path, bytes: &[u8]) -> Result<(), Failure> {
    std::fs::write(path, bytes).map_err(|error| format!("{}: {error}", path.display()))
}

fn scratch() -> Result<PathBuf, Failure> {
    let work = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("br03");
    std::fs::create_dir_all(&work).map_err(|error| format!("{}: {error}", work.display()))?;
    Ok(work)
}

/// The events of the two logs of the run that the speed work reads: where the
/// detector closed a segment and how far it lagged, the question put to the
/// identifier in the middle of an utterance, a wait for a slot of the
/// translation server, a text in progress let go, the translation of a text in
/// progress taken over by the line that closed it, how each draft was read and
/// whether the draft worker came up and stayed up, and (see
/// [`ENDPOINT_EVENTS`]) how each segment was closed and read.
const EVENTS_KEPT: [&str; 12] = [
    "asr.vad.closed",
    "asr.partial.lang.asked",
    "asr.partial.read",
    "asr.draft.ready",
    "asr.draft.lost",
    "translate.slot.waited",
    "translate.partial.skipped",
    "translate.draft.reused",
    "asr.endpoint.decided",
    "asr.endpoint.held",
    "asr.utterance.timed",
    "asr.feed.waited",
];

/// The events of [`EVENTS_KEPT`] that every run has: the rule and the plan that
/// made each final, a sentence the early detector kept, the stages of the
/// reading of an utterance, and the wait for the audio to reach the engine.
const ENDPOINT_EVENTS: [&str; 4] = [
    "asr.endpoint.decided",
    "asr.endpoint.held",
    "asr.utterance.timed",
    "asr.feed.waited",
];

/// The lines of the logs of the run in `work` that name one of [`EVENTS_KEPT`].
fn kept_lines(work: &Path) -> Result<String, Failure> {
    let mut kept = String::new();
    for name in ["hayamimi.log", "relay.log"] {
        let text = read(&work.join(name))?;
        for line in text.lines() {
            if EVENTS_KEPT.iter().any(|event| line.contains(event)) {
                kept.push_str(line);
                kept.push('\n');
            }
        }
    }
    Ok(kept)
}

/// The line of the draft translation of a pair run: the `L_tr` of the warm
/// utterances has a p50 no greater than this (design 10.13.4, hand 1).
const DRAFT_P50_LINE_MS: i64 = 100;

/// T-BR-04-009: the p50 of the draft translation is inside its line, and each
/// language of the pair has a warm value of its own. A report with no warm
/// utterance, or in which a language has none, is no pass.
fn judge_draft_p50(report: &StageReport) -> Result<String, Failure> {
    let found = report
        .stat(Stage::Draft)
        .map_err(|error| error.to_string())?;
    let Some(all) = found else {
        return Err(String::from("no warm value for L_tr"));
    };
    if all.p50_ms > DRAFT_P50_LINE_MS {
        return Err(format!(
            "L_tr p50 is {} ms, over the line of {DRAFT_P50_LINE_MS} ms",
            all.p50_ms
        ));
    }
    for lang in ["ja", "en"] {
        let kept = spoken_in(report, lang)
            .stat(Stage::Draft)
            .map_err(|error| error.to_string())?;
        if kept.is_none() {
            return Err(format!("no warm value for L_tr in {lang}"));
        }
    }
    Ok(format!(
        "L_tr p50 {} ms, inside the line of {DRAFT_P50_LINE_MS} ms",
        all.p50_ms
    ))
}

/// Plays `lat-dialog` through the stack started the way `variant` says and
/// keeps the record of the run under the variant's step.
fn record_run(variant: &Variant) -> TestResult {
    let work = scratch()?;
    let wav = find_above("testdata/lat-dialog.wav")?;
    let manifest = find_above("testdata/manifests/lat-dialog.json")?;
    let run = run_paged_with(
        &Translator::Real(LLAMA_PORT),
        &Playback {
            wav: &wav,
            manifest: Some(&manifest),
            repeat: REPEATS,
        },
        &variant.flags,
        &work,
        false,
    )?;
    let samples = read(&run.played)?;
    let events = read(&run.played.with_extension("events.jsonl"))?;
    let report = stage_report(&samples, &events).map_err(|error| error.to_string())?;
    let joined = join_page(&report, &run.with_relay.stamps)?;
    keep(
        &record_of(variant, ".md"),
        render_baseline(variant.title, &report, &joined)?.as_bytes(),
    )?;
    keep(&record_of(variant, ".jsonl"), samples.as_bytes())?;
    keep(&record_of(variant, ".events.jsonl"), events.as_bytes())?;
    let kept = kept_lines(&work)?;
    keep(&record_of(variant, ".log.jsonl"), kept.as_bytes())?;
    if let Some(missing) = ENDPOINT_EVENTS.iter().find(|event| !kept.contains(**event)) {
        return Err(format!("the logs of the run have no line of {missing}").into());
    }
    if JUDGE_AFTER_B2 {
        judge_budget(&joined.warm, &joined.cold_l_page_ms)?;
    }
    if variant.judges_draft {
        judge_draft_p50(&report)?;
    }
    Ok(())
}

/// T-BR-03-001
#[test]
#[ignore = "starts the hayamimi binary, the relay and chrome-headless-shell, and needs the translation server on port 18084"]
fn br03_the_wait_from_the_end_of_speech_to_the_page_is_inside_the_budget() -> TestResult {
    record_run(&CURRENT)
}

/// T-BR-03-017
#[test]
#[ignore = "starts the hayamimi binary, the relay and chrome-headless-shell, and needs the translation server on port 18084"]
fn the_run_with_the_final_translation_not_yielding_is_recorded() -> TestResult {
    record_run(&LID_ONLY)
}

/// T-BR-03-018
#[test]
#[ignore = "starts the hayamimi binary, the relay and chrome-headless-shell, and needs the translation server on port 18084"]
fn the_run_with_both_measures_switched_off_is_recorded() -> TestResult {
    record_run(&BOTH_OFF)
}

/// T-BR-36-012
#[test]
#[ignore = "starts the hayamimi binary, the relay and chrome-headless-shell, and needs the translation server on port 18084"]
fn the_run_with_covering_held_reuse_is_recorded() -> TestResult {
    record_run(&HELD_COVERING)
}

/// T-BR-36-013
#[test]
#[ignore = "starts the hayamimi binary, the relay and chrome-headless-shell, and needs the translation server on port 18084"]
fn the_run_with_strict_held_reuse_is_recorded() -> TestResult {
    record_run(&HELD_STRICT)
}

/// T-BR-37-012
#[test]
#[ignore = "starts the hayamimi binary, the relay and chrome-headless-shell, and needs the translation server on port 18084"]
fn the_run_with_the_final_language_decided_midway_is_recorded() -> TestResult {
    record_run(&FINAL_LID_MIDWAY)?;
    let log = read(&record_of(&FINAL_LID_MIDWAY, ".log.jsonl"))?;
    assert!(
        log.contains("asr.partial.lang.asked"),
        "the logs of the run have no line of asr.partial.lang.asked"
    );
    Ok(())
}

/// T-BR-37-013
#[test]
#[ignore = "starts the hayamimi binary, the relay and chrome-headless-shell, and needs the translation server on port 18084"]
fn the_run_with_the_final_language_decided_at_the_close_is_recorded() -> TestResult {
    record_run(&FINAL_LID_CLOSE)
}

/// T-BR-38-018
#[test]
#[ignore = "starts the hayamimi binary, the relay and chrome-headless-shell, and needs the translation server on port 18084"]
fn the_run_with_the_draft_worker_on_is_recorded() -> TestResult {
    record_run(&DRAFT_WORKER_ON)?;
    let log = read(&record_of(&DRAFT_WORKER_ON, ".log.jsonl"))?;
    for event in ["asr.partial.read", "asr.draft.ready"] {
        assert!(
            log.contains(event),
            "the logs of the run have no line of {event}"
        );
    }
    Ok(())
}

/// T-BR-38-019
#[test]
#[ignore = "starts the hayamimi binary, the relay and chrome-headless-shell, and needs the translation server on port 18084"]
fn the_run_with_the_draft_worker_off_is_recorded() -> TestResult {
    record_run(&DRAFT_WORKER_OFF)?;
    let log = read(&record_of(&DRAFT_WORKER_OFF, ".log.jsonl"))?;
    assert!(
        log.contains("asr.partial.read"),
        "the logs of the run have no line of asr.partial.read"
    );
    Ok(())
}

/// How the segments the `silence` rule closed in a run were turned into text,
/// counted from the run's `asr.endpoint.decided` lines (design 10.13.4, hand 9).
#[derive(Debug, Default, PartialEq, Eq)]
struct HeldCounts {
    /// Segments the `silence` rule closed that made a final.
    silence: usize,
    /// Of them, the sentence the early detector kept was used as it was.
    held: usize,
    /// The whole segment was read.
    decode: usize,
    /// What follows the early line was read.
    rest: usize,
    /// Of the segments read again although a sentence had been kept, those
    /// whose kept sentence was read to a point at or after the segment's end.
    over: usize,
    /// Of the same, those whose kept sentence was read to a point before it.
    short: usize,
}

/// Counts the `silence` closes of a run's log lines by the plan that made their
/// final. A plan the design does not name, or a `silence` close with none, is
/// an error: the lines would no longer mean what the count says.
fn held_counts(log: &str) -> Result<HeldCounts, Failure> {
    let mut counts = HeldCounts::default();
    for line in log
        .lines()
        .filter(|line| line.contains("asr.endpoint.decided"))
    {
        let row: Value = serde_json::from_str(line).map_err(|error| error.to_string())?;
        if row["rule"] != "silence" {
            continue;
        }
        counts.silence += 1;
        match row["plan"].as_str() {
            Some("held") => counts.held += 1,
            Some("decode") => counts.decode += 1,
            Some("rest") => counts.rest += 1,
            Some(other) => return Err(format!("a plan the design does not name: {other}")),
            None => return Err(String::from("a silence close with no plan")),
        }
        match row["held_gap_ms"].as_i64() {
            Some(gap) if gap >= 0 => counts.over += 1,
            Some(_) => counts.short += 1,
            None => {}
        }
    }
    Ok(counts)
}

/// T-BR-36-014
#[test]
fn the_held_plan_counts_and_signed_gaps_of_a_run_are_counted_from_its_log() -> TestResult {
    let decided = |fields: &str| format!("{{\"event\":\"asr.endpoint.decided\",{fields}}}\n");
    let log = [
        decided("\"id\":1,\"rule\":\"silence\",\"silence_ms\":300,\"plan\":\"decode\",\"held_gap_ms\":120"),
        decided("\"id\":2,\"rule\":\"silence\",\"silence_ms\":300,\"plan\":\"decode\",\"held_gap_ms\":-40"),
        decided("\"id\":3,\"rule\":\"silence\",\"silence_ms\":300,\"plan\":\"held\""),
        decided("\"id\":4,\"rule\":\"early\",\"silence_ms\":120"),
        String::from("{\"event\":\"asr.endpoint.held\",\"reason\":\"incomplete\"}\n"),
    ]
    .concat();
    assert_eq!(
        held_counts(&log)?,
        HeldCounts {
            silence: 3,
            held: 1,
            decode: 2,
            rest: 0,
            over: 1,
            short: 1,
        }
    );
    let unnamed = decided("\"id\":5,\"rule\":\"silence\",\"plan\":\"guess\"");
    assert!(held_counts(&unnamed).is_err());
    let planless = decided("\"id\":6,\"rule\":\"silence\"");
    assert!(held_counts(&planless).is_err());
    assert_eq!(held_counts("")?, HeldCounts::default());
    Ok(())
}

/// The `(id, lang, text)` of every `final` of a record's events, in the order
/// the record has them.
fn finals_of(events: &str) -> Result<Vec<(String, String, String)>, Failure> {
    let mut finals = Vec::new();
    for line in events.lines() {
        let row: Value = serde_json::from_str(line).map_err(|error| error.to_string())?;
        let event = &row["event"];
        if event["type"] != "final" {
            continue;
        }
        let field = |key: &str| event[key].as_str().map(str::to_string);
        match (field("id"), field("lang"), field("text")) {
            (Some(id), Some(lang), Some(text)) => finals.push((id, lang, text)),
            _ => return Err(format!("a final with no id, lang or text: {line}")),
        }
    }
    Ok(finals)
}

/// A line of a record's events: a `final` of `lang` that says `text`.
fn final_line(id: &str, lang: &str, text: &str) -> String {
    let event = serde_json::json!({"type": "final", "id": id, "lang": lang, "text": text});
    format!("{}\n", serde_json::json!({"event": event, "t_us": 1}))
}

/// A `final` of a record: its id, its language and its text.
type Final = (String, String, String);

/// The plan of every final of a run (`held`, `decode`, `rest`; none for an
/// early close), by the number of the final.
type Plans = BTreeMap<u64, Option<String>>;

/// The number in the id of a final (`file-3` is 3), which is the number the
/// `asr.endpoint.decided` line of that final carries.
fn number_of(id: &str) -> Result<u64, Failure> {
    id.strip_prefix("file-")
        .and_then(|number| number.parse().ok())
        .ok_or_else(|| format!("the id {id} is not file-<number>"))
}

/// The plans of a run, from its `asr.endpoint.decided` lines.
fn plans_of(log: &str) -> Result<Plans, Failure> {
    let mut plans = Plans::new();
    for line in log
        .lines()
        .filter(|line| line.contains("asr.endpoint.decided"))
    {
        let row: Value = serde_json::from_str(line).map_err(|error| error.to_string())?;
        let Some(number) = row["id"].as_u64() else {
            return Err(format!("an endpoint line with no number: {line}"));
        };
        plans.insert(number, row["plan"].as_str().map(str::to_string));
    }
    Ok(plans)
}

/// Whether the run whose plans are `plans` made the final `number` from the
/// sentence the early detector kept.
fn made_from_kept(plans: &Plans, number: u64) -> bool {
    plans
        .get(&number)
        .is_some_and(|plan| plan.as_deref() == Some("held"))
}

/// The pause marks of English and Japanese that a sentence closed by the
/// `silence` rule does not end with when its final was made from a kept sentence.
const PAUSE_MARKS: [char; 5] = [',', ';', ':', '、', '，'];

/// A text without the pause marks and the full stops at its end: the one place
/// where `covering` writes a mark that `strict` does not, for a sentence both
/// made from the kept one.
fn without_end_marks(text: &str) -> &str {
    text.trim_end()
        .trim_end_matches(|mark: char| mark == '.' || mark == '。' || PAUSE_MARKS.contains(&mark))
}

/// The ids of the finals whose text differs between a covering run and a strict
/// run (`plans` of the two, in that order), apart from the marks at its end,
/// although covering did not make the final from a kept sentence that strict
/// read again.
fn unexplained(
    covering: &[Final],
    strict: &[Final],
    plans: [&Plans; 2],
) -> Result<Vec<String>, Failure> {
    let mut ids = Vec::new();
    for (covered, read_again) in covering.iter().zip(strict) {
        if without_end_marks(&covered.2) == without_end_marks(&read_again.2) {
            continue;
        }
        let number = number_of(&covered.0)?;
        if !made_from_kept(plans[0], number) || made_from_kept(plans[1], number) {
            ids.push(covered.0.to_owned());
        }
    }
    Ok(ids)
}

/// T-BR-36-015
#[test]
fn the_two_held_reuse_runs_differ_only_in_the_finals_covering_made_from_kept_sentences()
-> TestResult {
    let partial = "{\"event\":{\"type\":\"partial\",\"text\":\"a\"},\"t_us\":0}\n";
    let first = final_line("file-1", "ja", "あ");
    let second = final_line("file-2", "en", "b");
    let sample = [partial, first.as_str(), second.as_str()].concat();
    assert_eq!(
        finals_of(&sample)?,
        vec![
            (
                String::from("file-1"),
                String::from("ja"),
                String::from("あ")
            ),
            (
                String::from("file-2"),
                String::from("en"),
                String::from("b")
            ),
        ]
    );
    let changed = final_line("file-1", "ja", "い");
    let other = [partial, changed.as_str(), second.as_str()].concat();
    assert_ne!(finals_of(&sample)?, finals_of(&other)?);
    assert!(finals_of("{\"event\":{\"type\":\"final\",\"id\":\"file-1\"}}\n").is_err());
    let line = |id: u64, plan: &str| {
        format!(
            "{{\"event\":\"asr.endpoint.decided\",\"id\":{id},\"rule\":\"silence\",\"plan\":\"{plan}\"}}\n"
        )
    };
    let kept = plans_of(&[line(1, "held"), line(2, "decode")].concat())?;
    let again = plans_of(&[line(1, "decode"), line(2, "decode")].concat())?;
    let (a, b) = (finals_of(&sample)?, finals_of(&other)?);
    assert!(unexplained(&a, &b, [&kept, &again])?.is_empty());
    assert_eq!(
        unexplained(&a, &b, [&again, &again])?,
        [String::from("file-1")]
    );
    assert_eq!(
        unexplained(&a, &b, [&kept, &kept])?,
        [String::from("file-1")]
    );
    let full_stop = final_line("file-2", "en", "b.");
    let comma = final_line("file-2", "en", "b,");
    let stopped = finals_of(&[partial, first.as_str(), full_stop.as_str()].concat())?;
    let paused = finals_of(&[partial, first.as_str(), comma.as_str()].concat())?;
    let marks_only = unexplained(&stopped, &paused, [&again, &again])?;
    assert!(marks_only.is_empty());
    assert!(plans_of("{\"event\":\"asr.endpoint.decided\",\"plan\":\"held\"}\n").is_err());

    let record = Path::new(REPO).join("docs/results/in-person");
    let recorded =
        |variant: &Variant, suffix: &str| read(&record.join(format!("{}{suffix}", variant.step)));
    let covering = finals_of(&recorded(&HELD_COVERING, ".events.jsonl")?)?;
    let strict = finals_of(&recorded(&HELD_STRICT, ".events.jsonl")?)?;
    assert!(!covering.is_empty(), "the covering run made no final");
    assert!(
        covering.len() == strict.len()
            && covering
                .iter()
                .zip(&strict)
                .all(|(covered, read_again)| covered.0 == read_again.0 && covered.1 == read_again.1),
        "the two runs did not make the same finals in the same languages"
    );
    let covering_log = recorded(&HELD_COVERING, ".log.jsonl")?;
    let strict_log = recorded(&HELD_STRICT, ".log.jsonl")?;
    let plans = [plans_of(&covering_log)?, plans_of(&strict_log)?];
    let stray = unexplained(&covering, &strict, [&plans[0], &plans[1]])?;
    assert!(
        stray.is_empty(),
        "finals differ with no kept sentence behind them: {stray:?}"
    );
    let covering_held = held_counts(&covering_log)?;
    let strict_held = held_counts(&strict_log)?;
    assert!(
        covering_held.held > strict_held.held,
        "covering used {} kept sentences and strict {}",
        covering_held.held,
        strict_held.held
    );
    Ok(())
}

/// The numbers of the finals a run made from the sentence the early detector
/// kept, when the `silence` rule closed the segment, from its
/// `asr.endpoint.decided` lines.
fn kept_by_silence(log: &str) -> Result<Vec<u64>, Failure> {
    let mut numbers = Vec::new();
    for line in log
        .lines()
        .filter(|line| line.contains("asr.endpoint.decided"))
    {
        let row: Value = serde_json::from_str(line).map_err(|error| error.to_string())?;
        if row["rule"] != "silence" || row["plan"] != "held" {
            continue;
        }
        let Some(number) = row["id"].as_u64() else {
            return Err(format!("an endpoint line with no number: {line}"));
        };
        numbers.push(number);
    }
    Ok(numbers)
}

/// Of the English and Japanese finals whose number is in `numbers`, how many
/// there are and the ids of those whose text ends with a pause mark.
fn ending_with_a_pause(finals: &[Final], numbers: &[u64]) -> Result<(usize, Vec<String>), Failure> {
    let mut checked = 0;
    let mut ids = Vec::new();
    for (id, lang, text) in finals {
        if !matches!(lang.as_str(), "en" | "ja") || !numbers.contains(&number_of(id)?) {
            continue;
        }
        checked += 1;
        if text.trim_end().ends_with(PAUSE_MARKS) {
            ids.push(id.to_owned());
        }
    }
    Ok((checked, ids))
}

/// T-BR-36-019
#[test]
fn the_finals_covering_made_from_kept_sentences_end_with_no_pause_mark() -> TestResult {
    let decided = |fields: &str| format!("{{\"event\":\"asr.endpoint.decided\",{fields}}}\n");
    let log = [
        decided("\"id\":1,\"rule\":\"silence\",\"plan\":\"held\""),
        decided("\"id\":2,\"rule\":\"silence\",\"plan\":\"held\""),
        decided("\"id\":3,\"rule\":\"silence\",\"plan\":\"decode\""),
        decided("\"id\":4,\"rule\":\"max_speech\",\"plan\":\"held\""),
        decided("\"id\":5,\"rule\":\"silence\",\"plan\":\"held\""),
    ]
    .concat();
    let kept = kept_by_silence(&log)?;
    assert_eq!(kept, [1, 2, 5]);
    let numberless = decided("\"rule\":\"silence\",\"plan\":\"held\"");
    assert!(kept_by_silence(&numberless).is_err());
    let made = |id: &str, lang: &str, text: &str| {
        (String::from(id), String::from(lang), String::from(text))
    };
    let finals = [
        made("file-1", "en", "He would pay."),
        made("file-2", "en", "He would pay, "),
        made("file-3", "en", "He would pay,"),
        made("file-4", "ja", "雨です、"),
        made("file-5", "zh", "你好，"),
    ];
    assert_eq!(
        ending_with_a_pause(&finals, &kept)?,
        (2, vec![String::from("file-2")])
    );
    assert!(ending_with_a_pause(&[made("final-1", "en", "a")], &[1]).is_err());

    let record = Path::new(REPO).join("docs/results/in-person");
    let recorded = |suffix: &str| read(&record.join(format!("{}{suffix}", HELD_COVERING.step)));
    let kept = kept_by_silence(&recorded(".log.jsonl")?)?;
    assert!(!kept.is_empty(), "the covering run kept no sentence");
    let finals = finals_of(&recorded(".events.jsonl")?)?;
    let (checked, ending) = ending_with_a_pause(&finals, &kept)?;
    assert_eq!(checked, kept.len(), "a kept sentence has no final");
    assert!(
        ending.is_empty(),
        "finals made from a kept sentence end with a pause mark: {ending:?}"
    );
    Ok(())
}

/// The numbers of the utterances whose `asr.utterance.timed` line says the
/// identifier took no time for the final (`lid_ms` is 0): the finals whose
/// language was the answer to the question put when the voice reached four
/// seconds.
fn without_a_question(log: &str) -> Result<Vec<u64>, Failure> {
    let mut numbers = Vec::new();
    for line in log
        .lines()
        .filter(|line| line.contains("asr.utterance.timed"))
    {
        let row: Value = serde_json::from_str(line).map_err(|error| error.to_string())?;
        let (Some(number), Some(lid_ms)) = (row["id"].as_u64(), row["lid_ms"].as_f64()) else {
            return Err(format!("a timed line with no id or lid_ms: {line}"));
        };
        if lid_ms == 0.0 {
            numbers.push(number);
        }
    }
    Ok(numbers)
}

/// The least voice, in milliseconds, of the question that is put for the final.
const FOUR_SECONDS_MS: u64 = 4000;

/// How many `asr.partial.lang.asked` lines of the log are for a voice of four
/// seconds or more: the question put for the final, not the one for the drafts.
fn four_second_questions(log: &str) -> Result<usize, Failure> {
    let mut asked = 0;
    for line in log
        .lines()
        .filter(|line| line.contains("asr.partial.lang.asked"))
    {
        let row: Value = serde_json::from_str(line).map_err(|error| error.to_string())?;
        let Some(voice_ms) = row["voice_ms"].as_u64() else {
            return Err(format!("an asked line with no voice_ms: {line}"));
        };
        if voice_ms >= FOUR_SECONDS_MS {
            asked += 1;
        }
    }
    Ok(asked)
}

/// The ids of the finals that are not the same in the two lists: a final whose
/// id, language or text differs, and a final that only one list has.
fn differing(first: &[Final], second: &[Final]) -> Vec<String> {
    let mut ids: Vec<String> = first
        .iter()
        .zip(second)
        .filter(|(one, other)| one != other)
        .map(|(one, _)| one.0.to_owned())
        .collect();
    let (long, short) = if first.len() > second.len() {
        (first, second)
    } else {
        (second, first)
    };
    ids.extend(long[short.len()..].iter().map(|extra| extra.0.to_owned()));
    ids
}

/// An `asr.utterance.timed` line of a run's log.
fn timed_line(id: u64, lid_ms: &str) -> String {
    format!("{{\"event\":\"asr.utterance.timed\",\"id\":{id},\"lid_ms\":{lid_ms}}}\n")
}

/// An `asr.partial.lang.asked` line of a run's log.
fn asked_line(voice_ms: u64) -> String {
    format!(
        "{{\"event\":\"asr.partial.lang.asked\",\"answer\":\"ja\",\"lid_ms\":98.5,\"voice_ms\":{voice_ms}}}\n"
    )
}

/// T-BR-37-014
#[test]
fn the_midway_and_close_runs_name_the_same_finals_and_only_midway_skips_the_question() -> TestResult
{
    let log = [
        timed_line(1, "113.176"),
        timed_line(2, "0.0"),
        timed_line(3, "88.5"),
        timed_line(4, "0.0"),
    ]
    .concat();
    assert_eq!(without_a_question(&log)?, [2, 4]);
    assert!(without_a_question(&timed_line(1, "38.0"))?.is_empty());
    assert!(without_a_question("{\"event\":\"asr.utterance.timed\",\"id\":1}\n").is_err());
    let asked = [
        asked_line(2016),
        asked_line(3999),
        asked_line(4000),
        asked_line(5200),
    ]
    .concat();
    assert_eq!(four_second_questions(&asked)?, 2);
    assert_eq!(four_second_questions("")?, 0);
    assert!(four_second_questions("{\"event\":\"asr.partial.lang.asked\"}\n").is_err());
    let made = |id: &str, lang: &str, text: &str| {
        (String::from(id), String::from(lang), String::from(text))
    };
    let same = vec![made("file-1", "ja", "あ"), made("file-2", "en", "b")];
    assert!(differing(&same, &same).is_empty());
    let other_lang = vec![made("file-1", "ja", "あ"), made("file-2", "ja", "b")];
    assert_eq!(differing(&same, &other_lang), [String::from("file-2")]);
    let other_text = vec![made("file-1", "ja", "い"), made("file-2", "en", "b")];
    assert_eq!(differing(&same, &other_text), [String::from("file-1")]);
    let longer = vec![same[0].clone(), same[1].clone(), made("file-3", "en", "c")];
    assert_eq!(differing(&same, &longer), [String::from("file-3")]);
    assert_eq!(differing(&longer, &same), [String::from("file-3")]);

    let midway_log = read(&record_of(&FINAL_LID_MIDWAY, ".log.jsonl"))?;
    let close_log = read(&record_of(&FINAL_LID_CLOSE, ".log.jsonl"))?;
    let midway = finals_of(&read(&record_of(&FINAL_LID_MIDWAY, ".events.jsonl"))?)?;
    let close = finals_of(&read(&record_of(&FINAL_LID_CLOSE, ".events.jsonl"))?)?;
    assert!(!midway.is_empty(), "the midway run made no final");
    let stray = differing(&midway, &close);
    assert!(
        stray.is_empty(),
        "the two runs differ in these finals: {stray:?}"
    );
    let skipped = without_a_question(&midway_log)?;
    let asked = four_second_questions(&midway_log)?;
    assert!(
        !skipped.is_empty(),
        "no final of the midway run took its language from the four-second question"
    );
    assert!(
        skipped.len() <= asked,
        "{} finals skipped the question and {asked} questions were put",
        skipped.len()
    );
    let kept = without_a_question(&close_log)?;
    assert!(
        kept.is_empty(),
        "finals of the close run that did not ask: {kept:?}"
    );
    assert_eq!(
        four_second_questions(&close_log)?,
        0,
        "the close run put a question for a final"
    );
    Ok(())
}

/// T-BR-03-002
#[test]
fn the_warm_stage_percentiles_of_the_pair_record_are_the_hand_counted_ones() -> TestResult {
    // The pair run of IP-4: a record that no later run rewrites.
    let record = Path::new(REPO).join("docs/results/in-person");
    let samples = read(&record.join("ip04_pair.jsonl"))?;
    let events = read(&record.join("ip04_pair.events.jsonl"))?;
    let report = stage_report(&samples, &events)?;
    assert_eq!(report.warm.len(), 76);
    let stat = |stage: Stage| -> Result<(i64, i64), Failure> {
        let found = report.stat(stage).map_err(|error| error.to_string())?;
        let found = found.ok_or_else(|| format!("no warm value for {}", stage.name()))?;
        Ok((found.p50_ms, found.p95_ms))
    };
    assert_eq!(stat(Stage::Wait)?, (445, 1081));
    assert_eq!(stat(Stage::Decode)?, (268, 472));
    assert_eq!(stat(Stage::Draft)?, (221, 363));
    let cold: Vec<(u32, u32)> = report
        .cold
        .iter()
        .map(|row| (row.repeat, row.utt))
        .collect();
    assert_eq!(cold, [(1, 1), (1, 2)]);
    Ok(())
}

/// T-BR-04-009
#[test]
fn the_warm_pair_run_has_its_draft_translation_p50_inside_100_ms_and_split_by_language()
-> TestResult {
    // The pair run with IP-S7 and IP-S8 on: a record that no later run rewrites.
    let record = Path::new(REPO).join("docs/results/in-person");
    let samples = read(&record.join("br04_ip_s8.jsonl"))?;
    let events = read(&record.join("br04_ip_s8.events.jsonl"))?;
    let report = stage_report(&samples, &events).map_err(|error| error.to_string())?;
    assert_eq!(report.warm.len(), 76);
    judge_draft_p50(&report)?;
    assert_eq!(
        draft_by_language_text(&report)?,
        "L_tr ja: n 32, p50 139 ms, p95 329 ms\nL_tr en: n 44, p50 4 ms, p95 258 ms\n"
    );
    Ok(())
}

/// The test file that owns each record a real run rewrites, and the record's
/// name as two parts. The name is joined at run time, so that this file does
/// not spell the name of a record it does not own.
const REWRITTEN: [(&str, &str, &str); 4] = [
    ("ip02.rs", "ip02", "pair"),
    ("ip02.rs", "ip02", "fast"),
    ("ip07.rs", "ip07", "pair"),
    ("br03.rs", "br03", "ip"),
];

/// The owner and the name of each record of [`REWRITTEN`].
fn rewritten() -> Vec<(&'static str, String)> {
    REWRITTEN
        .iter()
        .map(|(owner, step, tail)| (*owner, format!("{step}_{tail}")))
        .collect()
}

/// The names among `records` that `text`, the source of the file called `file`,
/// spells while the file is not their owner.
fn foreign_records<'a>(
    records: &'a [(&'static str, String)],
    file: &str,
    text: &str,
) -> Vec<&'a str> {
    records
        .iter()
        .filter(|(owner, name)| *owner != file && text.contains(name.as_str()))
        .map(|(_, name)| name.as_str())
        .collect()
}

/// The Rust files under `dir` at any depth, leaving out the build directories.
fn rust_files(dir: &Path, found: &mut Vec<PathBuf>) -> Result<(), Failure> {
    let said = |error: std::io::Error| format!("{}: {error}", dir.display());
    for entry in std::fs::read_dir(dir).map_err(said)? {
        let path = entry.map_err(said)?.path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("{} has no name", path.display()))?;
        if path.is_dir() {
            if !name.starts_with("target") {
                rust_files(&path, found)?;
            }
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
    Ok(())
}

/// T-BR-03-019
#[test]
fn only_the_owner_of_a_run_names_the_records_that_run_rewrites() -> TestResult {
    let records = rewritten();
    // The control: a file that is not the owner is found naming each record, its owner is not.
    for (owner, name) in &records {
        let text = format!("read(&record.join(\"{name}.jsonl\"))?");
        let outside = foreign_records(&records, "nobody.rs", &text);
        assert!(
            outside.contains(&name.as_str()),
            "{name} was not found in {text}"
        );
        let inside = foreign_records(&records, owner, &text);
        assert!(
            !inside.contains(&name.as_str()),
            "{name} was found in its owner {owner}"
        );
    }
    let mut files = Vec::new();
    rust_files(&Path::new(REPO).join("native"), &mut files)?;
    assert!(
        files.len() > 200,
        "only {} Rust files were found",
        files.len()
    );
    let mut foreign = Vec::new();
    for file in &files {
        let name = file
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("{} has no name", file.display()))?;
        for record in foreign_records(&records, name, &read(file)?) {
            foreign.push(format!("{} names {record}", file.display()));
        }
    }
    assert!(
        foreign.is_empty(),
        "only the test that owns a run names the records it rewrites; \
         read a copy under a name no run writes: {foreign:?}"
    );
    Ok(())
}

/// A wait whose stages are inside their lines, apart from the two the row varies.
fn wait(l_page_ms: i64, l_settled_page_ms: i64) -> Wait {
    Wait {
        d_ms: 200,
        dec_ms: 200,
        tr_ms: 50,
        l_page_ms,
        l_settled_page_ms,
    }
}

/// Twenty warm waits: `first` of them at `common`, the rest at `last`.
fn twenty(common: i64, last: i64, others: usize) -> Vec<i64> {
    let mut values = vec![common; 20 - others];
    values.extend(vec![last; others]);
    values
}

fn page_waits_of(values: &[i64]) -> Vec<Wait> {
    values.iter().map(|value| wait(*value, 1000)).collect()
}

fn settled_waits_of(values: &[i64]) -> Vec<Wait> {
    values.iter().map(|value| wait(700, *value)).collect()
}

fn says(result: Result<String, Failure>, words: &str) -> bool {
    result.err().is_some_and(|text| text.contains(words))
}

/// T-BR-03-003
#[test]
fn a_page_wait_p50_of_750_ms_is_on_the_line() {
    let warm = page_waits_of(&twenty(750, 750, 0));
    assert!(judge_budget(&warm, &[1000, 1000]).is_ok());
}

/// T-BR-03-004
#[test]
fn a_page_wait_p50_of_751_ms_is_over_the_line() {
    let warm = page_waits_of(&twenty(751, 751, 0));
    let said = judge_budget(&warm, &[1000, 1000]);
    assert!(says(said, "`L_page` の p50 が 751 ms で線 750 ms の外"));
}

/// T-BR-03-005
#[test]
fn a_page_wait_p95_of_1300_ms_is_on_the_line() {
    let warm = page_waits_of(&twenty(700, 1300, 2));
    assert!(judge_budget(&warm, &[1000, 1000]).is_ok());
}

/// T-BR-03-006
#[test]
fn a_page_wait_p95_of_1301_ms_is_over_the_line() {
    let warm = page_waits_of(&twenty(700, 1301, 2));
    let said = judge_budget(&warm, &[1000, 1000]);
    assert!(says(said, "`L_page` の p95 が 1301 ms で線 1300 ms の外"));
}

/// T-BR-03-007
#[test]
fn the_largest_sample_does_not_decide_the_page_wait_p95() {
    let mut values = twenty(700, 700, 0);
    values[18] = 1300;
    values[19] = 5000;
    assert!(judge_budget(&page_waits_of(&values), &[1000, 1000]).is_ok());
}

/// T-BR-03-008
#[test]
fn a_settled_page_wait_p50_of_1100_ms_is_on_the_line() {
    assert!(judge_budget(&settled_waits_of(&twenty(1100, 1100, 0)), &[1000]).is_ok());
}

/// T-BR-03-009
#[test]
fn a_settled_page_wait_p50_of_1101_ms_is_over_the_line() {
    let said = judge_budget(&settled_waits_of(&twenty(1101, 1101, 0)), &[1000]);
    assert!(says(
        said,
        "`L_settled_page` の p50 が 1101 ms で線 1100 ms の外"
    ));
}

/// T-BR-03-010
#[test]
fn a_settled_page_wait_p95_of_1700_ms_is_on_the_line() {
    assert!(judge_budget(&settled_waits_of(&twenty(1000, 1700, 2)), &[1000]).is_ok());
}

/// T-BR-03-011
#[test]
fn a_settled_page_wait_p95_of_1701_ms_is_over_the_line() {
    let said = judge_budget(&settled_waits_of(&twenty(1000, 1701, 2)), &[1000]);
    assert!(says(
        said,
        "`L_settled_page` の p95 が 1701 ms で線 1700 ms の外"
    ));
}

/// T-BR-03-012
#[test]
fn a_cold_sample_of_2500_ms_is_on_the_line() {
    let warm = page_waits_of(&twenty(700, 700, 0));
    assert!(judge_budget(&warm, &[2500, 100]).is_ok());
}

/// T-BR-03-013
#[test]
fn a_cold_sample_of_2501_ms_is_over_the_line() {
    let warm = page_waits_of(&twenty(700, 700, 0));
    let said = judge_budget(&warm, &[2501, 100]);
    assert!(says(
        said,
        "冷えた標本の `L_page` の最大が 2501 ms で線 2500 ms の外"
    ));
}

/// T-BR-03-014
#[test]
fn a_cold_sample_is_kept_out_of_the_warm_percentiles() {
    let warm = page_waits_of(&twenty(700, 700, 0));
    assert!(judge_budget(&warm, &[2400, 100]).is_ok());
    // Two cold samples at 2400 ms: mixed into 22 waits, the 21st is 2400 ms and
    // is over the p95 line of 1300 ms; kept apart they only meet the cold line.
    assert!(judge_budget(&warm, &[2400, 2400]).is_ok());
}

/// T-BR-03-015
#[test]
fn a_stage_outside_its_budget_is_recorded_and_does_not_fail_the_run() -> TestResult {
    let warm: Vec<Wait> = (0..20)
        .map(|_| Wait {
            d_ms: 301,
            dec_ms: 200,
            tr_ms: 50,
            l_page_ms: 700,
            l_settled_page_ms: 1000,
        })
        .collect();
    let record = judge_budget(&warm, &[1000, 1000])?;
    let wait_line = record
        .lines()
        .find(|line| line.starts_with("d:"))
        .ok_or("the record has no line for the wait")?;
    assert!(
        wait_line.contains("301") && wait_line.contains("線の外"),
        "{record}"
    );
    Ok(())
}

/// T-BR-03-016
#[test]
fn a_run_with_no_warm_sample_cannot_pass_the_budget() {
    let said = judge_budget(&[], &[1000, 1000]);
    assert!(says(said, "温まった標本 0"));
}

/// An utterance spoken in `lang` whose draft translation took `tr_ms`.
fn spoken(lang: &str, tr_ms: i64) -> StageRow {
    StageRow {
        repeat: 1,
        utt: 1,
        lang: Some(lang.to_owned()),
        final_id: None,
        d_us: None,
        dec_us: None,
        tr_us: Some(tr_ms * 1000),
        settle_us: None,
        l_us: None,
        settled_wait_us: None,
    }
}

fn report_of(cold: Vec<StageRow>, warm: Vec<StageRow>) -> StageReport {
    StageReport { cold, warm }
}

/// T-BR-04-015
#[test]
fn the_draft_translation_is_given_for_each_language_with_its_count() -> TestResult {
    let warm = vec![
        spoken("ja", 100),
        spoken("ja", 200),
        spoken("ja", 300),
        spoken("en", 50),
        spoken("en", 60),
    ];
    let text = draft_by_language_text(&report_of(Vec::new(), warm))?;
    assert_eq!(
        text,
        "L_tr ja: n 3, p50 200 ms, p95 300 ms\nL_tr en: n 2, p50 50 ms, p95 60 ms\n"
    );
    Ok(())
}

/// T-BR-04-016
#[test]
fn a_language_with_no_warm_utterance_is_said_to_have_no_value() -> TestResult {
    let warm = vec![spoken("ja", 100), spoken("ja", 200)];
    let text = draft_by_language_text(&report_of(Vec::new(), warm))?;
    let [ja, en] = text.lines().collect::<Vec<_>>()[..] else {
        return Err(format!("two lines were expected: {text}").into());
    };
    assert!(ja.starts_with("L_tr ja: n 2, p50 100 ms"), "{text}");
    assert_eq!(en, "L_tr en: 温まった標本が 0 件で、値は無い");
    Ok(())
}

/// T-BR-04-017
#[test]
fn a_cold_utterance_is_kept_out_of_the_split_by_language() -> TestResult {
    let cold = vec![spoken("en", 999)];
    let warm = vec![spoken("ja", 100), spoken("ja", 200)];
    let text = draft_by_language_text(&report_of(cold, warm))?;
    assert!(text.contains("L_tr ja: n 2"), "{text}");
    assert!(text.contains("L_tr en: 温まった標本が 0 件"), "{text}");
    assert!(!text.contains("999"), "{text}");
    Ok(())
}

/// T-BR-04-018
#[test]
fn a_draft_translation_p50_inside_the_line_passes_when_both_languages_have_a_value() -> TestResult {
    let warm = vec![
        spoken("ja", 150),
        spoken("ja", 60),
        spoken("en", 4),
        spoken("en", 5),
        spoken("en", 6),
    ];
    let said = judge_draft_p50(&report_of(Vec::new(), warm))?;
    assert!(said.contains("p50 6 ms"), "{said}");
    Ok(())
}

/// T-BR-04-019
#[test]
fn a_draft_translation_p50_over_the_line_fails() {
    let warm = vec![
        spoken("ja", 150),
        spoken("ja", 160),
        spoken("ja", 170),
        spoken("en", 101),
        spoken("en", 102),
    ];
    assert!(says(
        judge_draft_p50(&report_of(Vec::new(), warm)),
        "over the line"
    ));
}

/// T-BR-04-020
#[test]
fn a_draft_translation_p50_of_100_ms_is_on_the_line_and_101_ms_is_over_it() -> TestResult {
    let on = vec![spoken("ja", 100), spoken("en", 100)];
    judge_draft_p50(&report_of(Vec::new(), on))?;
    let over = vec![spoken("ja", 101), spoken("en", 101)];
    assert!(says(
        judge_draft_p50(&report_of(Vec::new(), over)),
        "101 ms"
    ));
    Ok(())
}

/// T-BR-04-021
#[test]
fn a_run_with_no_warm_draft_translation_cannot_pass_the_draft_line() {
    assert!(says(
        judge_draft_p50(&report_of(Vec::new(), Vec::new())),
        "no warm value for L_tr"
    ));
}

/// T-BR-04-022
#[test]
fn a_run_in_which_one_language_has_no_warm_draft_translation_cannot_pass() {
    let warm = vec![spoken("ja", 10), spoken("ja", 20)];
    assert!(says(
        judge_draft_p50(&report_of(Vec::new(), warm)),
        "no warm value for L_tr in en"
    ));
}

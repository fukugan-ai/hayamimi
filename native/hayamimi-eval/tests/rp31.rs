//! RP-31: how `L` is counted.
//!
//! Only a translation the subtitle shows is `t_tr` (`status` `ok`, `text` not
//! empty), and a `final` belongs to the utterance it ended (`t1_s`) and to the
//! translation with its `id` and the sha256 of its text.
//!
//! The slices are cut from the recorded runs when the test runs, with the
//! filters the design's table names, so no copy of a run is kept here:
//!
//! ```text
//! A   rp3_rust.events.jsonl  .t_us <= 29000000                      windows: repeat 1, utt 1..=3
//! B   rp3_rust.events.jsonl  301500000 <= .t_us <= 330000000        windows: repeat 2, utt 12..=14
//! PL  lat_ja_0.35_2.events   952265000 <= .t_us <= 977300000        windows: repeat 5, utt 20..=23
//! PO  lat_ja_0.35_1.events   the same range                         windows: the same
//! PB  lat_ja_0.35_2.events   91886063 <= .t_us <= 107500000         windows: repeat 1, utt 10..=11
//! PC  lat_ja_0.35_2.events   112873709 <= .t_us <= 134500000        windows: repeat 1, utt 13..=14
//! ```
//!
//! A and B are in `docs/results/r8e/`; the Python runs are under `testdata/baseline/`,
//! which git does not carry. A missing file is an `Err` that names its path.

mod support;

use std::ops::RangeInclusive;
use std::path::Path;

use hayamimi_eval::latency::{
    Event, Sample, Stat, Summary, Window, build_samples, parse_event, summarize_run,
};
use serde_json::{Value, json};
use support::base;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const R8E: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/results/r8e");
/// More references than any slice has utterances; the texts are not what these tests read.
const REFERENCES: usize = 64;
/// sha256 of the closing final of utterance 2 of slice A (`mic-2`).
const MIC_2_SHA: &str = "68972f77abf17f7011c960d9dfbe44519d08add6bf055a19c75123964b7edeb1";

// ---------------------------------------------------------------------------
// Reading and slicing
// ---------------------------------------------------------------------------

fn read_lines(path: &Path) -> Result<Vec<Value>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).map_err(|e| format!("{}: {e}", path.display())))
        .collect()
}

fn int_of(row: &Value, key: &str) -> Result<i64, String> {
    row.get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("a row has no integer `{key}`: {row}"))
}

fn events_between(rows: &[Value], low: i64, high: i64) -> Result<Vec<Value>, String> {
    let mut kept = Vec::new();
    for row in rows {
        let t_us = int_of(row, "t_us")?;
        if t_us >= low && t_us <= high {
            kept.push(row.clone());
        }
    }
    Ok(kept)
}

fn windows_of(
    rows: &[Value],
    repeat: i64,
    utts: RangeInclusive<i64>,
) -> Result<Vec<Value>, String> {
    let mut kept = Vec::new();
    for row in rows {
        if int_of(row, "repeat")? == repeat && utts.contains(&int_of(row, "utt")?) {
            kept.push(row.clone());
        }
    }
    Ok(kept)
}

/// A slice: the window rows, then the event rows.
type Slice = (Vec<Value>, Vec<Value>);

fn r8e_rows() -> Result<Slice, String> {
    let windows = read_lines(&Path::new(R8E).join("rp3_rust.jsonl"))?;
    let events = read_lines(&Path::new(R8E).join("rp3_rust.events.jsonl"))?;
    Ok((windows, events))
}

fn slice_a() -> Result<Slice, String> {
    let (windows, events) = r8e_rows()?;
    Ok((
        windows_of(&windows, 1, 1..=3)?,
        events_between(&events, i64::MIN, 29_000_000)?,
    ))
}

fn slice_b() -> Result<Slice, String> {
    let (windows, events) = r8e_rows()?;
    Ok((
        windows_of(&windows, 2, 12..=14)?,
        events_between(&events, 301_500_000, 330_000_000)?,
    ))
}

fn python_slice(
    run: &str,
    (repeat, utts): (i64, RangeInclusive<i64>),
    (low, high): (i64, i64),
) -> Result<Slice, String> {
    let dir = base()?;
    let windows = read_lines(&dir.join(format!("{run}.jsonl")))?;
    let events = read_lines(&dir.join(format!("{run}.events.jsonl")))?;
    Ok((
        windows_of(&windows, repeat, utts)?,
        events_between(&events, low, high)?,
    ))
}

fn slice_pl() -> Result<Slice, String> {
    python_slice("lat_ja_0.35_2", (5, 20..=23), (952_265_000, 977_300_000))
}

/// Utterance 1 of slice A alone: its window, and the events before the next window starts.
fn utterance_one() -> Result<Slice, String> {
    let (windows, events) = slice_a()?;
    Ok((
        windows.into_iter().take(1).collect(),
        events_between(&events, i64::MIN, 11_000_000)?,
    ))
}

// ---------------------------------------------------------------------------
// Measuring through the functions under test (the one place that spells them)
// ---------------------------------------------------------------------------

struct Measured {
    samples: Vec<Sample>,
    summary: Summary,
}

fn window_of(row: &Value) -> Result<Window, String> {
    let narrow = |key: &str| u32::try_from(int_of(row, key)?).map_err(|e| format!("{key}: {e}"));
    Ok(Window {
        repeat: narrow("repeat")?,
        utt: narrow("utt")?,
        t_start_us: int_of(row, "t_start_us")?,
        t_end_us: int_of(row, "t_end_us")?,
    })
}

fn event_of(row: &Value) -> Result<Event, String> {
    let Some(event) = row.get("event") else {
        return Err(format!("a row has no `event`: {row}"));
    };
    parse_event(int_of(row, "t_us")?, &event.to_string()).map_err(|e| e.to_string())
}

fn measure((window_rows, event_rows): &Slice) -> Result<Measured, String> {
    let windows = window_rows
        .iter()
        .map(window_of)
        .collect::<Result<Vec<_>, _>>()?;
    let events = event_rows
        .iter()
        .map(event_of)
        .collect::<Result<Vec<_>, _>>()?;
    let references = vec![String::new(); REFERENCES];
    let samples = build_samples(&windows, &references, &events).map_err(|e| e.to_string())?;
    let summary = summarize_run(&samples, &events).map_err(|e| e.to_string())?;
    Ok(Measured { samples, summary })
}

fn sample_of(measured: &Measured, repeat: u32, utt: u32) -> Result<&Sample, String> {
    measured
        .samples
        .iter()
        .find(|s| s.repeat == repeat && s.utt == utt)
        .ok_or_else(|| format!("no sample for repeat {repeat} utterance {utt}"))
}

fn finals_of(measured: &Measured) -> Vec<u32> {
    measured.samples.iter().map(|s| s.finals).collect()
}

// ---------------------------------------------------------------------------
// Editing rows
// ---------------------------------------------------------------------------

fn held_row_index(events: &[Value]) -> Result<usize, String> {
    events
        .iter()
        .position(|row| {
            let event = &row["event"];
            event["type"] == "translation" && event["status"] == "held" && event["id"] == "mic-1"
        })
        .ok_or_else(|| String::from("slice A has no held translation of mic-1"))
}

/// Edits the `held` translation of `mic-1` in place.
fn edit_held(events: &mut [Value], edits: &[(&str, Value)]) -> Result<(), String> {
    let at = held_row_index(events)?;
    for (key, value) in edits {
        events[at]["event"][*key] = value.clone();
    }
    Ok(())
}

/// A copy of the held translation, at `t_us`, with `edits` applied, put in time order.
fn add_translation(
    events: &mut Vec<Value>,
    t_us: i64,
    edits: &[(&str, Value)],
) -> Result<(), String> {
    let mut row = events[held_row_index(events)?].clone();
    row["t_us"] = json!(t_us);
    for (key, value) in edits {
        row["event"][*key] = value.clone();
    }
    let at = events.partition_point(|r| int_of(r, "t_us").is_ok_and(|t| t <= t_us));
    events.insert(at, row);
    Ok(())
}

fn shown() -> Vec<(&'static str, Value)> {
    vec![
        ("status", json!("ok")),
        ("text", json!("a shown translation")),
    ]
}

fn stat(n: usize, p50_ms: i64, p95_ms: i64) -> Option<Stat> {
    Some(Stat { n, p50_ms, p95_ms })
}

// ---------------------------------------------------------------------------
// The acceptance test: the recount of the whole recorded run
// ---------------------------------------------------------------------------

#[test]
fn rp31_latency_counts_only_shown_translations_of_their_own_utterance() -> TestResult {
    let whole = measure(&r8e_rows()?)?;
    assert_eq!(whole.summary.l, stat(67, 1533, 2752));
    assert_eq!(
        whole.summary.with_translation + whole.summary.untranslated,
        whole.summary.samples
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// T-RP-31-001 .. 010: which translation is `t_tr`
// ---------------------------------------------------------------------------

#[test]
fn a_held_empty_translation_is_not_t_tr() -> TestResult {
    let slice = utterance_one()?;
    // Control: the held translation is in the events, at the time the old count used.
    let at = held_row_index(&slice.1)?;
    assert_eq!(int_of(&slice.1[at], "t_us")?, 9_802_292);
    let measured = measure(&slice)?;
    let sample = sample_of(&measured, 1, 1)?;
    assert_eq!(sample.finals, 1);
    assert_eq!(sample.t_final_us, Some(8_937_248));
    assert_eq!(sample.l_asr_us(), Some(1_302_281));
    assert_eq!(sample.t_tr_us, None);
    assert_eq!(sample.l_us(), None);
    assert_eq!(sample.l_tr_us(), None);
    assert_eq!(measured.summary.untranslated, 1);
    Ok(())
}

#[test]
fn an_ok_translation_after_a_held_one_is_t_tr() -> TestResult {
    let (windows, mut events) = utterance_one()?;
    add_translation(&mut events, 10_100_000, &shown())?;
    let measured = measure(&(windows, events))?;
    let sample = sample_of(&measured, 1, 1)?;
    assert_eq!(sample.t_tr_us, Some(10_100_000));
    assert_eq!(sample.l_us(), Some(2_465_033));
    assert_eq!(sample.l_tr_us(), Some(1_162_752));
    Ok(())
}

#[test]
fn a_later_held_translation_does_not_replace_the_first_ok() -> TestResult {
    let (windows, mut events) = utterance_one()?;
    add_translation(&mut events, 9_700_000, &shown())?;
    let measured = measure(&(windows, events))?;
    let sample = sample_of(&measured, 1, 1)?;
    assert_eq!(sample.t_tr_us, Some(9_700_000));
    assert_eq!(sample.l_us(), Some(2_065_033));
    assert_eq!(sample.l_tr_us(), Some(762_752));
    Ok(())
}

/// The held translation of utterance 1, edited, and nothing else added.
fn t_tr_after_editing_the_held_one(edits: &[(&str, Value)]) -> Result<Option<i64>, String> {
    let (windows, mut events) = utterance_one()?;
    edit_held(&mut events, edits)?;
    let measured = measure(&(windows, events))?;
    Ok(sample_of(&measured, 1, 1)?.t_tr_us)
}

#[test]
fn a_held_translation_with_text_is_not_t_tr() -> TestResult {
    assert_eq!(
        t_tr_after_editing_the_held_one(&[("text", json!("some words"))])?,
        None
    );
    Ok(())
}

#[test]
fn an_ok_translation_with_empty_text_is_not_t_tr() -> TestResult {
    assert_eq!(
        t_tr_after_editing_the_held_one(&[("status", json!("ok"))])?,
        None
    );
    // Control: the same edit with a text is the translation.
    let with_text = [("status", json!("ok")), ("text", json!("words"))];
    assert_eq!(
        t_tr_after_editing_the_held_one(&with_text)?,
        Some(9_802_292)
    );
    Ok(())
}

#[test]
fn an_unavailable_translation_is_not_t_tr() -> TestResult {
    assert_eq!(
        t_tr_after_editing_the_held_one(&[("status", json!("unavailable"))])?,
        None
    );
    Ok(())
}

#[test]
fn an_unavailable_translation_with_text_is_not_t_tr() -> TestResult {
    let edits = [("status", json!("unavailable")), ("text", json!("words"))];
    assert_eq!(t_tr_after_editing_the_held_one(&edits)?, None);
    Ok(())
}

#[test]
fn a_translation_of_another_source_text_is_not_t_tr() -> TestResult {
    let (windows, mut events) = utterance_one()?;
    let mut edits = shown();
    edits.push(("src_sha256", json!(MIC_2_SHA)));
    add_translation(&mut events, 10_100_000, &edits)?;
    let measured = measure(&(windows, events))?;
    assert_eq!(sample_of(&measured, 1, 1)?.t_tr_us, None);
    Ok(())
}

#[test]
fn a_translation_with_another_id_is_not_tied_to_the_utterance() -> TestResult {
    let (windows, mut events) = slice_a()?;
    let mut edits = shown();
    edits.push(("id", json!("mic-2")));
    add_translation(&mut events, 10_100_000, &edits)?;
    let measured = measure(&(windows, events))?;
    assert_eq!(sample_of(&measured, 1, 1)?.t_tr_us, None);
    assert_eq!(sample_of(&measured, 1, 2)?.t_tr_us, Some(18_483_305));
    Ok(())
}

#[test]
fn a_partial_translation_is_not_t_tr() -> TestResult {
    let (windows, mut events) = utterance_one()?;
    let mut edits = shown();
    edits.push(("of", json!("partial")));
    add_translation(&mut events, 10_100_000, &edits)?;
    let measured = measure(&(windows, events))?;
    assert_eq!(sample_of(&measured, 1, 1)?.t_tr_us, None);
    Ok(())
}

// ---------------------------------------------------------------------------
// T-RP-31-011 .. 017: tying finals and translations to their utterance
// ---------------------------------------------------------------------------

#[test]
fn utterances_without_a_held_translation_keep_their_l() -> TestResult {
    let a = measure(&slice_a()?)?;
    let third = sample_of(&a, 1, 3)?;
    assert_eq!(third.t_tr_us, Some(28_267_506));
    assert_eq!(third.l_us(), Some(1_532_532));
    let b = measure(&slice_b()?)?;
    let twelfth = sample_of(&b, 2, 12)?;
    assert_eq!(twelfth.t_tr_us, Some(308_008_448));
    assert_eq!(twelfth.l_us(), Some(1_535_855));
    Ok(())
}

#[test]
fn t_tr_is_the_first_ok_translation_of_the_closing_final() -> TestResult {
    let a = measure(&slice_a()?)?;
    let second = sample_of(&a, 1, 2)?;
    assert_eq!(second.t_tr_us, Some(18_483_305));
    assert_eq!(second.l_us(), Some(1_168_360));
    assert_eq!(second.l_tr_us(), Some(92_138));
    Ok(())
}

#[test]
fn a_split_utterance_is_closed_by_its_last_final_by_t1_s() -> TestResult {
    let a = measure(&slice_a()?)?;
    let second = sample_of(&a, 1, 2)?;
    assert_eq!(second.finals, 2);
    assert_eq!(second.t_final_us, Some(18_391_167));
    assert_eq!(second.l_asr_us(), Some(1_076_222));
    Ok(())
}

#[test]
fn a_late_final_belongs_to_its_own_utterance_by_t1_s() -> TestResult {
    let b = measure(&slice_b()?)?;
    let thirteenth = sample_of(&b, 2, 13)?;
    assert_eq!(thirteenth.finals, 1);
    assert_eq!(thirteenth.t_final_us, Some(318_628_342));
    assert_eq!(thirteenth.l_asr_us(), Some(2_031_482));
    assert_eq!(thirteenth.t_tr_us, Some(319_522_568));
    assert_eq!(thirteenth.l_us(), Some(2_925_708));
    assert_eq!(thirteenth.l_tr_us(), Some(894_226));
    Ok(())
}

#[test]
fn the_next_utterance_loses_the_late_final_it_never_owned() -> TestResult {
    let b = measure(&slice_b()?)?;
    let fourteenth = sample_of(&b, 2, 14)?;
    assert_eq!(fourteenth.finals, 3);
    assert_eq!(fourteenth.t_final_us, Some(327_445_514));
    assert_eq!(fourteenth.t_tr_us, Some(327_643_922));
    assert_eq!(fourteenth.l_us(), Some(1_068_930));
    assert_eq!(fourteenth.l_asr_us(), Some(870_522));
    Ok(())
}

/// Slice B with the start of utterance 14 moved to 318117000 and the `t1_s` of
/// `mic-38` set to `t1_s`.
fn slice_b_with_boundary(t1_s: f64) -> Result<Measured, String> {
    let (mut windows, mut events) = slice_b()?;
    for row in &mut windows {
        if int_of(row, "utt")? == 14 {
            row["t_start_us"] = json!(318_117_000);
        }
    }
    for row in &mut events {
        if row["event"]["id"] == "mic-38" && row["event"]["type"] == "final" {
            row["event"]["t1_s"] = json!(t1_s);
        }
    }
    measure(&(windows, events))
}

#[test]
fn t1_s_just_before_the_next_start_belongs_to_the_earlier_utterance() -> TestResult {
    let b = slice_b_with_boundary(318.116)?;
    assert_eq!(sample_of(&b, 2, 13)?.finals, 1);
    assert_eq!(sample_of(&b, 2, 14)?.finals, 3);
    Ok(())
}

#[test]
fn t1_s_equal_to_the_next_start_belongs_to_the_next_utterance() -> TestResult {
    let b = slice_b_with_boundary(318.117)?;
    assert_eq!(sample_of(&b, 2, 13)?.finals, 0);
    assert_eq!(sample_of(&b, 2, 14)?.finals, 4);
    Ok(())
}

// ---------------------------------------------------------------------------
// T-RP-31-018 / 019: the whole recorded run
// ---------------------------------------------------------------------------

#[test]
fn the_whole_r8e_record_gives_the_corrected_counts() -> TestResult {
    let summary = measure(&r8e_rows()?)?.summary;
    assert_eq!(summary.samples, 115);
    assert_eq!(summary.with_final, 101);
    assert_eq!(summary.with_translation, 67);
    assert_eq!(summary.untranslated, 48);
    assert_eq!(summary.finals_total, 125);
    assert_eq!(summary.l_tr_negative, 0);
    assert_eq!(summary.l, stat(67, 1533, 2752));
    assert_eq!(summary.l_asr, stat(101, 1302, 2050));
    assert_eq!(summary.l_tr, stat(67, 316, 1237));
    // Control: the count the record was written with.
    let (windows, _) = r8e_rows()?;
    let old_with_translation = windows
        .iter()
        .filter(|row| !row["t_tr_us"].is_null())
        .count();
    assert_eq!(old_with_translation, 93);
    Ok(())
}

#[test]
fn eight_samples_regain_their_late_final_and_none_changes_its_time() -> TestResult {
    let (old_rows, events) = r8e_rows()?;
    let new = measure(&(old_rows.clone(), events))?;
    let mut regained = Vec::new();
    let mut changed = 0;
    for (old, sample) in old_rows.iter().zip(&new.samples) {
        let old_final = old.get("t_final_us").and_then(Value::as_i64);
        match (old_final, sample.t_final_us) {
            (None, Some(_)) => regained.push((sample.repeat, sample.utt, sample.t_tr_us.is_some())),
            (Some(was), Some(now)) if was != now => changed += 1,
            _ => {}
        }
    }
    let expected = [
        (2, 13, true),
        (3, 11, false),
        (3, 22, false),
        (4, 12, true),
        (4, 13, true),
        (4, 22, false),
        (5, 12, true),
        (5, 13, true),
    ];
    assert_eq!(regained, expected);
    assert_eq!(changed, 0);
    Ok(())
}

// ---------------------------------------------------------------------------
// T-RP-31-020 .. 026: the Python server's events
// ---------------------------------------------------------------------------

#[test]
fn a_python_late_final_is_counted_in_its_own_column() -> TestResult {
    let pl = measure(&slice_pl()?)?;
    assert_eq!(finals_of(&pl), [1, 0, 2, 1]);
    assert_eq!(pl.summary.late_final_suspects, Some(1));
    Ok(())
}

#[test]
fn a_python_run_with_on_time_finals_counts_no_suspect() -> TestResult {
    let slice = python_slice("lat_ja_0.35_1", (5, 20..=23), (952_265_000, 977_300_000))?;
    let po = measure(&slice)?;
    assert_eq!(finals_of(&po), [1, 1, 1, 1]);
    assert_eq!(po.summary.late_final_suspects, Some(0));
    Ok(())
}

#[test]
fn one_final_in_the_next_window_is_not_a_suspect() -> TestResult {
    let slice = python_slice("lat_ja_0.35_2", (1, 10..=11), (91_886_063, 107_500_000))?;
    let pb = measure(&slice)?;
    assert_eq!(finals_of(&pb), [0, 1]);
    assert_eq!(pb.summary.late_final_suspects, Some(0));
    Ok(())
}

#[test]
fn a_previous_window_with_a_final_is_not_a_suspect() -> TestResult {
    let slice = python_slice("lat_ja_0.35_2", (1, 13..=14), (112_873_709, 134_500_000))?;
    let pc = measure(&slice)?;
    assert_eq!(finals_of(&pc), [1, 3]);
    assert_eq!(pc.summary.late_final_suspects, Some(0));
    Ok(())
}

#[test]
fn a_python_translation_without_status_still_counts() -> TestResult {
    let pl = measure(&slice_pl()?)?;
    for (utt, t_tr, l) in [
        (20, 957_098_241, 1_187_137),
        (22, 969_859_404, 1_247_548),
        (23, 975_384_593, 1_132_919),
    ] {
        let sample = sample_of(&pl, 5, utt)?;
        assert_eq!(sample.t_tr_us, Some(t_tr), "utterance {utt}");
        assert_eq!(sample.l_us(), Some(l), "utterance {utt}");
    }
    Ok(())
}

#[test]
fn a_python_translation_with_empty_text_is_skipped() -> TestResult {
    let (windows, mut events) = slice_pl()?;
    let empty =
        json!({"t_us": 969_800_000, "event": {"lang": "en", "text": "", "type": "translation"}});
    let at = events.partition_point(|r| int_of(r, "t_us").is_ok_and(|t| t <= 969_800_000));
    events.insert(at, empty);
    let pl = measure(&(windows, events))?;
    assert_eq!(sample_of(&pl, 5, 22)?.t_tr_us, Some(969_859_404));
    Ok(())
}

#[test]
fn the_suspect_column_is_not_counted_for_a_rust_run() -> TestResult {
    let b = measure(&slice_b()?)?;
    assert_eq!(b.summary.late_final_suspects, None);
    // Control: a Python run counts it, and a count of 0 is not "not counted".
    let po = measure(&python_slice(
        "lat_ja_0.35_1",
        (5, 20..=23),
        (952_265_000, 977_300_000),
    )?)?;
    assert_eq!(po.summary.late_final_suspects, Some(0));
    Ok(())
}

// ---------------------------------------------------------------------------
// T-RP-31-027: the counts are written in separate fields
// ---------------------------------------------------------------------------

#[test]
fn real_runs_write_the_counts_in_separate_fields() -> TestResult {
    let rust = serde_json::to_value(measure(&slice_a()?)?.summary)?;
    let python = serde_json::to_value(measure(&slice_pl()?)?.summary)?;
    for summary in [&rust, &python] {
        assert!(summary["with_translation"].is_u64(), "{summary}");
        assert!(summary["untranslated"].is_u64(), "{summary}");
    }
    assert!(rust["late_final_suspects"].is_null(), "{rust}");
    assert_eq!(python["late_final_suspects"], json!(1));
    Ok(())
}

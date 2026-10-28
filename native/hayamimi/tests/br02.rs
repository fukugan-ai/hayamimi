//! BR-2: the page stage `L_pg` is timed on the page's own clock (design 10.13.3).
//!
//! The real run (T-BR-02-001 to 003) starts `hayamimi` on a websocket input, the
//! relay, a stand-in translation server and `chrome-headless-shell`, opens the
//! dashboard with `?relay=`, puts the measuring script into the page over the
//! DevTools protocol, and plays rows 7 to 10 of `lat-dialog`. A second page
//! without `?relay=` is opened beside it. They are `#[ignore]`d because they
//! need the models, `testdata/` (not in git), the page bundle next to the
//! `hayamimi` binary and a browser.
//!
//! The rows 004 to 007 test the tools that turn the page's two times into a
//! stage and its percentiles, on stamps written by hand. The row 008 reads the
//! product's page code. The row 009, T-BR-14-014 to 020 and T-BR-15-008 to 013
//! test what the report of the page says about the refined paragraphs and the
//! listening strip, and how a run's page is judged, on values written by hand.

mod support;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use support::dialog::{PageStamp, page_percentiles, page_stage_text, page_waits};
use support::dialog_run::{
    PagedRun, Playback, Translator, dialog_window, page_record_of, run_paged,
};
use support::page_view::{
    AfterCheck, AtFinal, BeforeCheck, PAGE_ROW_LIMIT, PageArrival, PageRefines, PageStrip,
    PageView, RefineCheck, judge_after_last, judge_before_final, judge_page, judge_refines,
};
use support::{Failure, TestResult, find_above};

/// The first of the four rows of `lat-dialog` the run plays, counted from 1.
const FIRST_ROW: usize = 7;
/// How many utterances the run plays: ja, en, ja, en.
const UTTERANCES: usize = 4;

fn scratch() -> Result<PathBuf, Failure> {
    let work = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("br02");
    std::fs::create_dir_all(&work).map_err(|error| format!("{}: {error}", work.display()))?;
    Ok(work)
}

fn run_page() -> Result<PagedRun, Failure> {
    let work = scratch()?;
    let (wav, manifest) = dialog_window(&work, FIRST_ROW, UTTERANCES)?;
    run_paged(
        &Translator::Fake,
        &Playback {
            wav: &wav,
            manifest: Some(&manifest),
            repeat: 1,
        },
        &work,
        true,
    )
}

/// The run is made once; the three rows that judge it share what it saw.
fn observed() -> Result<&'static PagedRun, Failure> {
    static RUN: OnceLock<Result<PagedRun, Failure>> = OnceLock::new();
    RUN.get_or_init(run_page).as_ref().map_err(Clone::clone)
}

/// T-BR-02-001
#[test]
#[ignore = "starts the hayamimi binary, the relay, a stand-in translation server and chrome-headless-shell"]
fn br02_the_page_stage_is_timed_on_the_page_clock() -> TestResult {
    let run = observed()?;
    let page = &run.with_relay;
    let stamps = &page.stamps;
    // A line the relay withholds (row 9 of `lat-dialog` says a number) draws a
    // mark and no translation: every row is a measured translation or a mark.
    assert!(!stamps.is_empty(), "the page drew no translation");
    assert_eq!(
        stamps.len() + page.marks_in_rows,
        page.rows,
        "{} translations and {} marks for {} rows",
        stamps.len(),
        page.marks_in_rows,
        page.rows
    );
    assert!(
        page.rows >= UTTERANCES,
        "{} rows for {UTTERANCES} utterances",
        page.rows
    );
    let waits = page_waits(stamps)?;
    assert_eq!(waits.len(), stamps.len());
    let text = page_stage_text(&waits)?;
    assert!(text.contains("p50") && text.contains("p95"), "{text}");
    let record = scratch()?.join("record.txt");
    std::fs::write(&record, format!("{text}\n"))
        .map_err(|error| format!("{}: {error}", record.display()))?;
    Ok(())
}

/// T-BR-02-002
#[test]
#[ignore = "starts the hayamimi binary, the relay, a stand-in translation server and chrome-headless-shell"]
fn the_measuring_script_sees_as_many_draws_as_the_page_has_translation_rows() -> TestResult {
    let page = &observed()?.with_relay;
    let draws = page
        .stamps
        .iter()
        .filter(|stamp| stamp.p_draw.is_some())
        .count();
    assert!(page.trs_in_rows > 0, "{} tr in rows", page.trs_in_rows);
    assert_eq!(draws, page.trs_in_rows, "draws the script saw, tr in rows");
    Ok(())
}

/// T-BR-02-003
#[test]
#[ignore = "starts the hayamimi binary, the relay, a stand-in translation server and chrome-headless-shell"]
fn a_page_without_relay_in_its_address_gives_no_draw_and_an_error() -> TestResult {
    let page = observed()?
        .without_relay
        .as_ref()
        .ok_or("the run opened no page without a relay")?;
    assert!(page.rows >= UTTERANCES, "the page drew {} rows", page.rows);
    assert_eq!(page.trs_in_rows, 0);
    assert_eq!(page.marks_in_rows, 0);
    let draws = page
        .stamps
        .iter()
        .filter(|stamp| stamp.p_draw.is_some())
        .count();
    assert_eq!(draws, 0);
    assert!(page_waits(&page.stamps).is_err());
    Ok(())
}

fn stamp(id: &str, arrive: Option<f64>, draw: Option<f64>) -> PageStamp {
    PageStamp {
        id: id.to_owned(),
        p_arrive: arrive,
        p_draw: draw,
        tool_us: None,
    }
}

/// T-BR-02-004
#[test]
fn a_draw_before_its_arrival_on_the_page_clock_is_an_error() -> TestResult {
    assert_eq!(
        page_waits(&[stamp("file-1", Some(100.0), Some(140.0))])?,
        [40.0]
    );
    assert_eq!(
        page_waits(&[stamp("file-2", Some(200.0), Some(200.0))])?,
        [0.0]
    );
    let early = page_waits(&[stamp("file-3", Some(300.0), Some(299.0))]);
    let said = early.err();
    assert!(
        said.as_deref()
            .is_some_and(|text| text.contains("file-3") && text.contains("-1")),
        "{said:?}"
    );
    Ok(())
}

/// T-BR-02-005
#[test]
fn a_translation_without_a_draw_record_is_named_and_is_an_error() {
    let stamps = [
        stamp("file-1", Some(10.0), Some(20.0)),
        stamp("file-2", Some(30.0), Some(40.0)),
        stamp("file-3", Some(50.0), None),
        stamp("file-4", Some(70.0), Some(80.0)),
    ];
    let said = page_waits(&stamps).err();
    assert!(
        said.as_deref()
            .is_some_and(|text| text.contains("file-3") && !text.contains("file-4")),
        "{said:?}"
    );
}

#[test]
fn a_translation_without_an_arrival_record_is_named_and_is_an_error() {
    let said = page_waits(&[stamp("file-9", None, Some(5.0))]).err();
    assert!(
        said.as_deref().is_some_and(|text| text.contains("file-9")),
        "{said:?}"
    );
}

#[test]
fn a_page_with_no_translation_gives_no_stage() {
    let said = page_waits(&[]).err();
    assert!(said.is_some(), "an empty record gave a stage");
    let undrawn = page_waits(&[
        stamp("file-1", Some(1.0), None),
        stamp("file-2", Some(2.0), None),
    ]);
    assert!(undrawn.is_err());
}

/// T-BR-02-006
#[test]
fn the_page_stage_percentiles_are_nearest_rank() -> TestResult {
    let (p50, p95) = page_percentiles(&[40.0, 10.0, 30.0, 20.0])?;
    assert_eq!((p50, p95), (20.0, 40.0));
    let said = page_percentiles(&[]).err();
    assert!(said.is_some(), "an empty list gave percentiles");
    Ok(())
}

/// T-BR-02-007
#[test]
fn the_page_stage_does_not_depend_on_the_tool_clock() -> TestResult {
    let pairs = [(10.0, 20.0), (100.0, 120.0), (500.0, 530.0), (900.0, 940.0)];
    let with_tool_clock = |shift_us: i64| -> Vec<PageStamp> {
        pairs
            .iter()
            .enumerate()
            .map(|(at, (arrive, draw))| PageStamp {
                id: format!("file-{at}"),
                p_arrive: Some(*arrive),
                p_draw: Some(*draw),
                tool_us: Some(1_000_000 * (at as i64 + 1) + shift_us),
            })
            .collect()
    };
    let first = page_waits(&with_tool_clock(0))?;
    let second = page_waits(&with_tool_clock(1_000_000))?;
    assert_eq!(first, second);
    assert_eq!(page_percentiles(&first)?, page_percentiles(&second)?);
    Ok(())
}

/// A report of the shape the measuring script sends: three lines, one of them
/// without a translation, and the refined paragraphs and the strip of a page.
const PAGE_REPORT: &str = r#"{"finals":["file-1","file-2","file-3"],"arrive":{"file-1":10.5,"file-3":30.0},"draw":[12.0,null,33.5],"rows":3,"trs_in_rows":2,"marks_in_rows":1,"refines":{"rows_before":2,"arrived_before":2,"rows":2,"arrived":[{"id":"r-1","speaker":"S1?","replayed":false},{"id":"r-2","speaker":null,"replayed":false},{"id":"r-1","speaker":"S1?","replayed":true}]},"strip":{"waiting":"Waiting for audio","at_final":[{"id":"file-1","strip":"hello","partial":"hello","gap_ms":300.5},{"id":"file-2","strip":null,"partial":null,"gap_ms":null}],"after_last":0,"text":"Waiting for audio","quiet":true}}"#;

#[test]
fn a_page_report_is_read_into_stamps_by_the_order_of_the_lines() -> TestResult {
    let record = page_record_of(PAGE_REPORT)?;
    assert_eq!(
        (record.rows, record.trs_in_rows, record.marks_in_rows),
        (3, 2, 1)
    );
    let ids: Vec<&str> = record
        .stamps
        .iter()
        .map(|stamp| stamp.id.as_str())
        .collect();
    assert_eq!(ids, ["file-1", "file-3"], "file-2 has neither time");
    assert_eq!(
        record
            .stamps
            .iter()
            .map(|stamp| stamp.p_draw)
            .collect::<Vec<_>>(),
        [Some(12.0), Some(33.5)]
    );
    Ok(())
}

#[test]
fn a_page_report_that_is_not_the_script_report_is_an_error() {
    assert!(page_record_of("{").is_err());
    assert!(page_record_of(r#"{"finals":[]}"#).is_err());
    // The report of a script that does not note the refined paragraphs or the strip.
    let before_the_strip =
        r#"{"finals":[],"arrive":{},"draw":[],"rows":0,"trs_in_rows":0,"marks_in_rows":0}"#;
    assert!(page_record_of(before_the_strip).is_err());
}

/// T-BR-02-009
#[test]
fn a_page_report_carries_the_refined_paragraphs_and_the_strip() -> TestResult {
    let record = page_record_of(PAGE_REPORT)?;
    assert_eq!(
        (
            record.refines.rows_before,
            record.refines.arrived_before,
            record.refines.rows
        ),
        (2, 2, 2)
    );
    let arrived: Vec<(Option<&str>, Option<&str>, bool)> = record
        .refines
        .arrived
        .iter()
        .map(|arrival| {
            (
                arrival.id.as_deref(),
                arrival.speaker.as_deref(),
                arrival.replayed,
            )
        })
        .collect();
    assert_eq!(
        arrived,
        [
            (Some("r-1"), Some("S1?"), false),
            (Some("r-2"), None, false),
            (Some("r-1"), Some("S1?"), true),
        ]
    );
    assert_eq!(record.strip.waiting.as_deref(), Some("Waiting for audio"));
    assert_eq!(record.strip.text.as_deref(), Some("Waiting for audio"));
    assert_eq!((record.strip.after_last, record.strip.quiet), (0, true));
    let gaps: Vec<Option<f64>> = record
        .strip
        .at_final
        .iter()
        .map(|found| found.gap_ms)
        .collect();
    assert_eq!(gaps, [Some(300.5), None]);
    Ok(())
}

fn arrival(id: &str, replayed: bool) -> PageArrival {
    PageArrival {
        id: Some(id.to_owned()),
        speaker: None,
        replayed,
    }
}

/// The refined paragraphs of a page, read before the replay when the
/// paragraphs that were not replayed had arrived. The measuring script
/// records two things about every page it reads, and a page written here that
/// lacks one is a wrong fixture, so it stops where it is built and not in the
/// judge: the arrivals that were not replayed come first, and a replayed
/// arrival has the id of one that came before it.
fn refines(rows_before: usize, rows: usize, arrived: Vec<PageArrival>) -> PageRefines {
    let mut replaying = false;
    for (at, arrival) in arrived.iter().enumerate() {
        if arrival.replayed {
            replaying = true;
            let came_before = arrived[..at]
                .iter()
                .any(|earlier| !earlier.replayed && earlier.id == arrival.id);
            assert!(
                came_before,
                "fixture: arrival {at} ({:?}) is replayed but no earlier arrival has its id",
                arrival.id
            );
        } else {
            assert!(
                !replaying,
                "fixture: arrival {at} ({:?}) was not replayed and follows one that was",
                arrival.id
            );
        }
    }
    PageRefines {
        rows_before,
        arrived_before: arrived.iter().filter(|arrival| !arrival.replayed).count(),
        rows,
        arrived,
    }
}

/// T-BR-14-014
#[test]
fn a_page_with_one_row_for_each_distinct_id_is_judged_and_the_repeats_are_counted() -> TestResult {
    let sent_again = refines(
        2,
        2,
        vec![
            arrival("r-1", false),
            arrival("r-2", false),
            arrival("r-1", true),
        ],
    );
    assert_eq!(
        judge_refines(&sent_again)?,
        RefineCheck {
            arrived: 3,
            paragraphs: 2,
            repeated: 1,
            replayed: 1
        }
    );
    Ok(())
}

/// T-BR-14-015
#[test]
fn a_page_with_a_row_for_every_arrival_is_not_a_page_that_replaces() {
    let arrived = vec![
        arrival("r-1", false),
        arrival("r-2", false),
        arrival("r-1", true),
    ];
    let appended_after = judge_refines(&refines(2, 3, arrived.clone())).err();
    assert!(
        appended_after.as_deref().is_some_and(
            |text| text.contains("and 3 after it") && text.contains("for 2 paragraphs")
        ),
        "{appended_after:?}"
    );
    let appended_before = judge_refines(&refines(3, 3, arrived)).err();
    assert!(
        appended_before.is_some(),
        "rows before the replay were not read"
    );
}

/// T-BR-14-016
#[test]
fn a_run_with_no_arrival_or_no_replay_is_no_pass_and_an_arrival_without_an_id_is_a_row()
-> TestResult {
    assert!(judge_refines(&refines(0, 0, Vec::new())).is_err());
    let natural = refines(2, 2, vec![arrival("r-1", false), arrival("r-2", false)]);
    let said = judge_refines(&natural).err();
    assert!(
        said.as_deref()
            .is_some_and(|text| text.contains("replayed no refined paragraph")),
        "{said:?}"
    );
    let anonymous = PageArrival {
        id: None,
        speaker: None,
        replayed: false,
    };
    let mixed = refines(
        3,
        3,
        vec![
            anonymous.clone(),
            anonymous,
            arrival("r-1", false),
            arrival("r-1", true),
        ],
    );
    assert_eq!(judge_refines(&mixed)?.paragraphs, 3);
    Ok(())
}

/// T-BR-14-017
#[test]
fn the_rows_the_page_keeps_are_the_limit_of_what_is_judged() -> TestResult {
    // `count` distinct paragraphs, then the first of them sent again.
    let distinct = |count: usize| -> Vec<PageArrival> {
        let mut arrived: Vec<PageArrival> = (0..count)
            .map(|at| arrival(&format!("r-{at}"), false))
            .collect();
        arrived.push(arrival("r-0", true));
        arrived
    };
    let at_the_limit = refines(PAGE_ROW_LIMIT, PAGE_ROW_LIMIT, distinct(PAGE_ROW_LIMIT));
    assert_eq!(judge_refines(&at_the_limit)?.paragraphs, PAGE_ROW_LIMIT);
    let over = refines(PAGE_ROW_LIMIT, PAGE_ROW_LIMIT, distinct(PAGE_ROW_LIMIT + 1));
    let said = judge_refines(&over).err();
    assert!(
        said.as_deref()
            .is_some_and(|text| text.contains("more than the 500 it keeps")),
        "{said:?}"
    );
    Ok(())
}

/// T-BR-14-018
#[test]
fn a_paragraph_that_arrived_after_the_rows_were_first_read_is_a_row_of_the_second_reading_only()
-> TestResult {
    let late = PageRefines {
        rows_before: 1,
        arrived_before: 1,
        rows: 2,
        arrived: vec![
            arrival("r-1", false),
            arrival("r-2", false),
            arrival("r-1", true),
        ],
    };
    assert_eq!(judge_refines(&late)?.paragraphs, 2);
    let drawn_early = PageRefines {
        rows_before: 2,
        ..late.clone()
    };
    assert!(judge_refines(&drawn_early).is_err());
    let miscounted = PageRefines {
        arrived_before: 4,
        ..late
    };
    let said = judge_refines(&miscounted).err();
    assert!(
        said.as_deref()
            .is_some_and(|text| text.contains("counts 4 arrivals before the replay")),
        "{said:?}"
    );
    Ok(())
}

/// T-BR-14-019
#[test]
#[should_panic(expected = "is replayed but no earlier arrival has its id")]
fn a_replayed_paragraph_whose_id_did_not_arrive_stops_where_the_page_is_built() {
    refines(2, 2, vec![arrival("r-1", false), arrival("r-2", true)]);
}

/// T-BR-14-020
#[test]
#[should_panic(expected = "was not replayed and follows one that was")]
fn an_arrival_that_was_not_replayed_after_a_replayed_one_stops_where_the_page_is_built() {
    refines(
        2,
        2,
        vec![
            arrival("r-1", false),
            arrival("r-1", true),
            arrival("r-2", false),
        ],
    );
}

const WAITING: &str = "Waiting for audio";

fn waiting_words() -> Vec<String> {
    vec![String::from(WAITING), String::from("音声を待っています")]
}

fn at_final(id: &str, strip: &str, partial: Option<&str>, gap_ms: Option<f64>) -> AtFinal {
    AtFinal {
        id: Some(id.to_owned()),
        strip: Some(strip.to_owned()),
        partial: partial.map(str::to_owned),
        gap_ms,
    }
}

fn strip_of(at_final: Vec<AtFinal>, after_last: usize, text: &str, quiet: bool) -> PageStrip {
    PageStrip {
        waiting: Some(String::from(WAITING)),
        at_final,
        after_last,
        text: Some(text.to_owned()),
        quiet,
    }
}

/// T-BR-15-008
#[test]
fn a_strip_that_shows_the_partial_before_a_final_and_waits_after_the_last_is_judged() -> TestResult
{
    let finals = vec![
        at_final("file-1", "こん", Some("こん"), Some(400.0)),
        at_final("file-2", WAITING, None, None),
        at_final("file-3", "次", Some("次"), Some(149.0)),
        at_final("file-4", "last", Some("last"), Some(150.0)),
    ];
    assert_eq!(
        judge_before_final(&finals)?,
        BeforeCheck {
            compared: 2,
            left_out: 2
        }
    );
    let strip = strip_of(finals, 0, WAITING, true);
    assert_eq!(
        judge_after_last(&strip, &waiting_words())?,
        AfterCheck::Waiting { emptied: 1 }
    );
    Ok(())
}

/// A final with a partial, then a final that no partial came before.
fn emptied_pair() -> Vec<AtFinal> {
    vec![
        at_final("file-1", "こん", Some("こん"), Some(400.0)),
        at_final("file-2", WAITING, None, None),
    ]
}

/// T-BR-15-009
#[test]
fn a_strip_that_keeps_the_last_partial_after_the_last_final_is_an_error() -> TestResult {
    let words = waiting_words();
    let kept = strip_of(emptied_pair(), 0, "こん", false);
    let said = judge_after_last(&kept, &words).err();
    assert!(
        said.as_deref()
            .is_some_and(|text| text.contains("after the last final the strip read")),
        "{said:?}"
    );
    let not_quiet = strip_of(emptied_pair(), 0, WAITING, false);
    let said = judge_after_last(&not_quiet, &words).err();
    assert!(
        said.as_deref().is_some_and(|text| text.contains("quiet")),
        "{said:?}"
    );
    let mut unknown = strip_of(emptied_pair(), 0, WAITING, true);
    unknown.waiting = Some(String::from("Listening"));
    assert!(judge_after_last(&unknown, &words).is_err());
    let no_final = strip_of(Vec::new(), 0, WAITING, true);
    let said = judge_after_last(&no_final, &words).err();
    assert!(
        said.as_deref()
            .is_some_and(|text| text.contains("no final reached the page")),
        "{said:?}"
    );
    // The control: a partial that came after the last final is the strip's to show.
    let late = strip_of(emptied_pair(), 2, "次", false);
    assert_eq!(
        judge_after_last(&late, &words)?,
        AfterCheck::NotJudged {
            partials: 2,
            emptied: 1
        }
    );
    Ok(())
}

/// T-BR-15-012
#[test]
fn a_final_that_finds_the_strip_still_showing_the_partial_before_it_is_an_error() {
    let stale = vec![
        at_final("file-1", "こん", Some("こん"), Some(400.0)),
        at_final("file-2", "こん", None, None),
    ];
    let said = judge_after_last(&strip_of(stale, 0, WAITING, true), &waiting_words()).err();
    assert!(
        said.as_deref()
            .is_some_and(|text| text.contains("file-2") && text.contains("こん")),
        "{said:?}"
    );
    // The control: a partial that came before the second final is the strip's to show.
    let shown = vec![
        at_final("file-1", "こん", Some("こん"), Some(400.0)),
        at_final("file-2", "次", Some("次"), Some(300.0)),
        at_final("file-3", WAITING, None, None),
    ];
    assert!(judge_after_last(&strip_of(shown, 0, WAITING, true), &waiting_words()).is_ok());
}

/// T-BR-15-013
#[test]
fn a_run_in_which_no_final_follows_a_final_with_a_partial_without_one_of_its_own_is_no_pass() {
    let words = waiting_words();
    let alone = vec![at_final("file-1", "こん", Some("こん"), Some(400.0))];
    let covered = vec![
        at_final("file-1", "こん", Some("こん"), Some(400.0)),
        at_final("file-2", "次", Some("次"), Some(300.0)),
    ];
    let uncovered = vec![
        at_final("file-1", WAITING, None, None),
        at_final("file-2", WAITING, None, None),
    ];
    for finals in [alone, covered, uncovered] {
        let said = judge_after_last(&strip_of(finals, 0, WAITING, true), &words).err();
        assert!(
            said.as_deref()
                .is_some_and(|text| text.contains("whether a final empties the strip")),
            "{said:?}"
        );
    }
}

/// T-BR-15-010
#[test]
fn a_strip_that_differs_from_a_settled_partial_before_a_final_is_an_error() {
    let differing = vec![at_final("file-1", "こ", Some("こん"), Some(400.0))];
    let said = judge_before_final(&differing).err();
    assert!(
        said.as_deref()
            .is_some_and(|text| text.contains("file-1") && text.contains("こん")),
        "{said:?}"
    );
    // A strip that never showed anything has no final that compared equal.
    let waiting_only = vec![
        at_final("file-1", WAITING, Some("こん"), Some(100.0)),
        at_final("file-2", WAITING, None, None),
    ];
    assert!(judge_before_final(&waiting_only).is_err());
    assert!(judge_before_final(&[]).is_err());
}

/// T-BR-15-011
#[test]
fn the_record_of_a_page_says_which_judgement_failed() -> TestResult {
    let strip = strip_of(emptied_pair(), 0, WAITING, true);
    let good = PageView {
        refines: refines(1, 1, vec![arrival("r-1", false), arrival("r-1", true)]),
        strip: strip.clone(),
    };
    let judged = judge_page(&good, &waiting_words());
    assert_eq!(judged.first_failure(), None);
    let lines = judged.lines();
    assert!(lines.contains("清書の行: 頁の行は 1 行"), "{lines}");
    assert!(lines.contains("待ちの文言"), "{lines}");
    let bad = PageView {
        refines: refines(1, 2, vec![arrival("r-1", false), arrival("r-1", true)]),
        strip,
    };
    let judged = judge_page(&bad, &waiting_words());
    assert!(
        judged
            .first_failure()
            .is_some_and(|text| text.contains("refine-text")),
        "{:?}",
        judged.first_failure()
    );
    assert!(judged.lines().contains("清書の行: 不可"));
    Ok(())
}

/// The text of every file under `directory`, as `(path, text)`. A directory
/// that cannot be read is an error and not an empty list.
fn files_under(directory: &Path) -> Result<Vec<(PathBuf, String)>, Failure> {
    let mut found = Vec::new();
    let entries = std::fs::read_dir(directory)
        .map_err(|error| format!("{}: {error}", directory.display()))?;
    for entry in entries {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.is_dir() {
            found.extend(files_under(&path)?);
        } else {
            let text = std::fs::read_to_string(&path)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            found.push((path, text));
        }
    }
    Ok(found)
}

/// The files whose text names a measuring point of the page.
fn files_with_the_hook(files: &[(PathBuf, String)]) -> Vec<&Path> {
    files
        .iter()
        .filter(|(_, text)| text.contains("p_arrive") || text.contains("p_draw"))
        .map(|(path, _)| path.as_path())
        .collect()
}

/// T-BR-02-008
#[test]
fn no_product_page_code_carries_the_measuring_hook() -> TestResult {
    let root = find_above("native/hayamimi-ui/src")?;
    let native = root.ancestors().nth(2).ok_or("no native directory")?;
    let mut product = files_under(&root)?;
    product.extend(files_under(&native.join("hayamimi/data/pages"))?);
    assert!(
        product.len() >= 10,
        "only {} product files were read",
        product.len()
    );
    let carrying = files_with_the_hook(&product);
    assert!(carrying.is_empty(), "{carrying:?}");
    // The control: the same walk finds the hook in this test.
    let control = files_under(&native.join("hayamimi/tests"))?;
    let here: Vec<&Path> = files_with_the_hook(&control)
        .into_iter()
        .filter(|path| path.ends_with("br02.rs"))
        .collect();
    assert_eq!(here.len(), 1, "{here:?}");
    Ok(())
}

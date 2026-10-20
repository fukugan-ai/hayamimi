//! OV-6: what the two lanes of the overlay show, as the page that draws them
//! comes out (design 11.3, 11.4.1). No window and no GPU: it runs anywhere.
//!
//! The test is a user of the crate. It feeds `LanesPage` the way the host does
//! (a model that messages are applied to, and a clock it moves to the times
//! `next_change` gives), renders the page, and reads the result the way the
//! overlay window would show it: which line elements stand under
//! `lane-upper` and `lane-lower`, in which order, with which text, mark and
//! class. The rules of `view` itself are the unit tests of the library.
//!
//! Each check is run on the page it is meant for, which must pass, and on a
//! page that a wrong implementation would draw, which it must reject, so that
//! a check that accepts everything is caught:
//!
//! - the translation drawn on the newest line instead of its own;
//! - a `file-` line drawn on a lane;
//! - a line still drawn after its time, and a line gone before its time.

use std::rc::Rc;

use dioxus::prelude::*;
use hayamimi_ui::lanes::{LANE_HOLD_MS, LaneRows, LanesPage, TR_WAIT_MS, next_change};
use hayamimi_ui::model::Model;
use hayamimi_ui::parts::Words;
use hayamimi_ui::strings::{Locale, Strings};
use hayamimi_ui::wire::parse;

type Failure = Box<dyn std::error::Error>;
type Verdict = Result<(), String>;

const ROWS: LaneRows = LaneRows { upper: 2, lower: 1 };

// ---- the page, as the host renders it -----------------------------------

#[derive(Props, Clone, PartialEq)]
struct Host {
    model: Model,
    now_ms: u64,
    words: Strings,
}

fn host(props: Host) -> Element {
    let words = props.words.clone();
    use_context_provider(|| -> Words { Rc::new(words) });
    let model = use_signal(|| props.model.clone());
    let now_ms = use_signal(|| props.now_ms);
    rsx! { LanesPage { model, rows: ROWS, now_ms } }
}

fn page(model: &Model, now_ms: u64) -> Result<String, Failure> {
    let props = Host {
        model: model.clone(),
        now_ms,
        words: Strings::embedded(Locale::En)?,
    };
    let mut dom = VirtualDom::new_with_props(host, props);
    dom.rebuild_in_place();
    Ok(dioxus_ssr::render(&dom))
}

// ---- the stream, as the wire carries it ---------------------------------

fn final_data(id: &str, lang: &str, text: &str) -> String {
    format!(
        r#"{{"type":"final","id":"{id}","text":"{text}","lang":"{lang}","speaker":"","latency_ms":300.0}}"#
    )
}

fn translation_data(id: &str, lang: &str, text: &str, settled: bool) -> String {
    format!(
        r#"{{"type":"translation","of":"final","id":"{id}","lang":"{lang}","text":"{text}","status":"ok","settled":{settled}}}"#
    )
}

fn withheld_data(id: &str, lang: &str) -> String {
    format!(
        r#"{{"type":"translation","of":"final","id":"{id}","lang":"{lang}","text":"","status":"held","hold_reason":"number","settled":true}}"#
    )
}

/// The model after each message arrived at its time (milliseconds).
fn model_of(messages: &[(u64, String)]) -> Result<Model, Failure> {
    let mut model = Model::default();
    for (at, data) in messages {
        model.apply(parse(data)?, *at);
    }
    Ok(model)
}

/// One review meeting: a presenter on the call, the user on the microphone, a
/// file line that no lane shows and a line with no id.
fn meeting() -> Vec<(u64, String)> {
    vec![
        (
            0,
            final_data("loopback-41", "en", "Shall we begin the review"),
        ),
        (0, final_data("loopback-42", "ja", "メディアの音声です")),
        (0, final_data("mic-7", "ja", "お願いします")),
        (0, final_data("file-3", "en", "Over the table")),
        (0, translation_data("file-3", "ja", "机の上", true)),
        (
            0,
            String::from(r#"{"type":"final","text":"no id","lang":"en"}"#),
        ),
        (
            1200,
            translation_data("loopback-41", "ja", "見直しを始めましょうか", true),
        ),
        (1300, translation_data("mic-7", "en", "Please", false)),
        (
            1900,
            translation_data("mic-7", "en", "Please go ahead.", true),
        ),
        (2100, final_data("mic-8", "ja", "予算は五百万円です")),
        (2200, withheld_data("mic-8", "en")),
        (2500, final_data("loopback-43", "en", "Next item please")),
        (
            6000,
            translation_data("loopback-43", "ja", "次の議題です", true),
        ),
    ]
}

/// The page at `now_ms` of a meeting in which only the messages that have
/// arrived by then have been applied.
fn page_at(script: &[(u64, String)], now_ms: u64) -> Result<String, Failure> {
    let arrived: Vec<(u64, String)> = script
        .iter()
        .filter(|(at, _)| *at <= now_ms)
        .cloned()
        .collect();
    page(&model_of(&arrived)?, now_ms)
}

// ---- reading the page ----------------------------------------------------

/// The part of the page under one lane: the upper lane comes first.
fn lane<'a>(html: &'a str, name: &str) -> Result<&'a str, String> {
    let (Some(upper), Some(lower)) = (
        html.find(r#"id="lane-upper""#),
        html.find(r#"id="lane-lower""#),
    ) else {
        return Err(format!("a lane is missing from the page: {html}"));
    };
    match name {
        "upper" => html.get(upper..lower),
        _ => html.get(lower..),
    }
    .ok_or_else(|| format!("the lanes are out of order in the page: {html}"))
}

/// The ids of the line elements under a lane, in page order.
fn line_ids(lane_html: &str) -> Vec<&str> {
    lane_html
        .split(r#"id="line-"#)
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .collect()
}

/// The element of one line: from its id to the next line or the end of the lane.
fn line<'a>(lane_html: &'a str, id: &str) -> Result<&'a str, String> {
    let start = format!(r#"id="line-{id}""#);
    let Some((_, from)) = lane_html.split_once(&start) else {
        return Err(format!("line-{id} is not under the lane: {lane_html}"));
    };
    Ok(from.split_once(r#"id="line-"#).map_or(from, |(own, _)| own))
}

fn expect(condition: bool, said: impl FnOnce() -> String) -> Verdict {
    if condition { Ok(()) } else { Err(said()) }
}

fn expect_ids(lane_html: &str, wanted: &[&str], what: &str) -> Verdict {
    let found = line_ids(lane_html);
    expect(found == wanted, || {
        format!("{what} lane holds {found:?}, not {wanted:?}: {lane_html}")
    })
}

fn expect_text(element: &str, id: &str, has: &[&str], hasnt: &[&str]) -> Verdict {
    for word in has {
        expect(element.contains(word), || {
            format!("line-{id} lacks {word:?}: {element}")
        })?;
    }
    for word in hasnt {
        expect(!element.contains(word), || {
            format!("line-{id} has {word:?}: {element}")
        })?;
    }
    Ok(())
}

// ---- what each page must show --------------------------------------------

/// A line that needs no translation is drawn at once; a line that waits for
/// one, a `file-` line and a line with no id are not drawn.
fn check_arrival(html: &str) -> Verdict {
    let upper = lane(html, "upper")?;
    expect_ids(upper, &["loopback-42"], "the upper")?;
    expect_text(
        line(upper, "loopback-42")?,
        "loopback-42",
        &["メディアの音声です", "source"],
        &[],
    )?;
    expect_ids(lane(html, "lower")?, &[], "the lower")?;
    expect(
        !html.contains("file-3") && !html.contains("Over the table"),
        || format!("a file- line is drawn: {html}"),
    )
}

/// The translation that came 1200 ms late stands on its own line, which is
/// not the newest.
fn check_late_translation(html: &str) -> Verdict {
    let upper = lane(html, "upper")?;
    expect_ids(upper, &["loopback-41", "loopback-42"], "the upper")?;
    expect_text(
        line(upper, "loopback-41")?,
        "loopback-41",
        &["見直しを始めましょうか", "settled", "t-ja"],
        &["Shall we begin"],
    )?;
    expect_text(
        line(upper, "loopback-42")?,
        "loopback-42",
        &["メディアの音声です"],
        &["見直し"],
    )?;
    expect_ids(lane(html, "lower")?, &[], "the lower")?;
    expect(!html.contains("file-3"), || {
        format!("a file- line is drawn: {html}")
    })
}

/// The early translation is drawn as a draft.
fn check_draft(html: &str) -> Verdict {
    let lower = lane(html, "lower")?;
    expect_ids(lower, &["mic-7"], "the lower")?;
    expect_text(
        line(lower, "mic-7")?,
        "mic-7",
        &["Please", "draft", "t-en"],
        &["お願いします", "go ahead"],
    )
}

/// The deciding translation stands in the same element, once.
fn check_decided(html: &str) -> Verdict {
    let lower = lane(html, "lower")?;
    expect_ids(lower, &["mic-7"], "the lower")?;
    expect_text(
        line(lower, "mic-7")?,
        "mic-7",
        &["Please go ahead.", "settled"],
        &["draft", "お願いします"],
    )?;
    expect(html.matches(r#"id="line-mic-7""#).count() == 1, || {
        format!("mic-7 is drawn more than once: {html}")
    })
}

/// A withheld translation: the source with the mark, and the lower lane keeps
/// only its newest line.
fn check_held(html: &str) -> Verdict {
    let lower = lane(html, "lower")?;
    expect_ids(lower, &["mic-8"], "the lower")?;
    expect_text(
        line(lower, "mic-8")?,
        "mic-8",
        &["Original", "予算は五百万円です", "held", "t-ja"],
        &["Please"],
    )
}

fn check_waiting(html: &str) -> Verdict {
    let upper = lane(html, "upper")?;
    expect_ids(upper, &["loopback-41", "loopback-42"], "the upper")?;
    expect(!html.contains("loopback-43"), || {
        format!("loopback-43 is drawn before its wait ends: {html}")
    })
}

/// The wait ended: the newest two lines, the new one as its source with the mark.
fn check_untranslated(html: &str) -> Verdict {
    let upper = lane(html, "upper")?;
    expect_ids(upper, &["loopback-42", "loopback-43"], "the upper")?;
    expect_text(
        line(upper, "loopback-43")?,
        "loopback-43",
        &["Next item please", "No translation", "untranslated", "t-en"],
        &["次の議題です"],
    )
}

fn check_taken_late(html: &str) -> Verdict {
    let upper = lane(html, "upper")?;
    expect_ids(upper, &["loopback-42", "loopback-43"], "the upper")?;
    expect_text(
        line(upper, "loopback-43")?,
        "loopback-43",
        &["次の議題です", "settled", "t-ja"],
        &["Next item please", "No translation"],
    )
}

/// A line is drawn with its translation until it has been `LANE_HOLD_MS` since
/// it last changed.
fn check_present(html: &str, id: &str, translation: &str) -> Verdict {
    let upper = lane(html, "upper")?;
    expect_ids(upper, &[id], "the upper")?;
    expect_text(line(upper, id)?, id, &[translation], &[])
}

fn check_gone(html: &str) -> Verdict {
    expect_ids(lane(html, "upper")?, &[], "the upper")?;
    expect_ids(lane(html, "lower")?, &[], "the lower")
}

// ---- the pages that a wrong implementation would draw --------------------

/// Two ids exchanged: the texts stay where they were, on the other line.
fn with_ids_exchanged(html: &str, a: &str, b: &str) -> String {
    let id_a = format!(r#"id="line-{a}""#);
    let id_b = format!(r#"id="line-{b}""#);
    let held = r#"id="line-@@""#;
    html.replace(&id_a, held)
        .replace(&id_b, &id_a)
        .replace(held, &id_b)
}

fn with_id_renamed(html: &str, from: &str, to: &str) -> String {
    html.replace(
        &format!(r#"id="line-{from}""#),
        &format!(r#"id="line-{to}""#),
    )
}

// ---- the host's clock ----------------------------------------------------

/// The times a host that follows `next_change` wakes at, from `start_ms` on.
fn wake_times(model: &Model, start_ms: u64) -> Vec<u64> {
    let mut times = Vec::new();
    let mut now = start_ms;
    while let Some(at) = next_change(model, ROWS, now) {
        times.push(at);
        now = at;
    }
    times
}

#[test]
fn ov06_lanes_split_join_replace_and_expire() -> Result<(), Failure> {
    // The constants of the contract are written out, so that a changed one is a failure.
    assert_eq!((LANE_HOLD_MS, TR_WAIT_MS), (8000, 3000));

    let script = meeting();
    // A line that needs no translation is drawn at once and nothing else is.
    check_arrival(&page_at(&script, 0)?)?;

    // The late translation joins its own line (positive control), and the
    // page with the translation on the newest line is rejected (negative control).
    let joined = page_at(&script, 1250)?;
    check_late_translation(&joined)?;
    let on_the_newest = with_ids_exchanged(&joined, "loopback-41", "loopback-42");
    assert!(
        check_late_translation(&on_the_newest).is_err(),
        "a translation drawn on the newest line was accepted"
    );
    // A file line drawn on a lane is rejected.
    let with_a_file_line = with_id_renamed(&joined, "loopback-42", "file-3");
    assert!(
        check_late_translation(&with_a_file_line).is_err(),
        "a file- line drawn on a lane was accepted"
    );

    // An early translation is replaced in the same element.
    check_draft(&page_at(&script, 1400)?)?;
    check_decided(&page_at(&script, 2000)?)?;
    assert!(
        check_decided(&page_at(&script, 1400)?).is_err(),
        "the draft was accepted as the deciding translation"
    );

    // A withheld translation shows the source with its mark.
    check_held(&page_at(&script, 2300)?)?;

    // The wait for a translation ends at TR_WAIT_MS, and a translation that
    // comes later replaces the source.
    check_waiting(&page_at(&script, 2500 + TR_WAIT_MS - 1)?)?;
    check_untranslated(&page_at(&script, 2500 + TR_WAIT_MS)?)?;
    check_taken_late(&page_at(&script, 6100)?)?;
    assert!(
        check_waiting(&page_at(&script, 2500 + TR_WAIT_MS)?).is_err(),
        "a line whose wait had ended was accepted as still waiting"
    );

    // A host that follows `next_change` sees a line leave exactly at LANE_HOLD_MS.
    let closing = model_of(&[
        (500, final_data("loopback-61", "en", "Closing words")),
        (
            900,
            translation_data("loopback-61", "ja", "終わりの言葉", true),
        ),
    ])?;
    let leaves = 900 + LANE_HOLD_MS;
    assert_eq!(wake_times(&closing, 900), [leaves]);
    check_present(&page(&closing, leaves - 1)?, "loopback-61", "終わりの言葉")?;
    check_gone(&page(&closing, leaves)?)?;
    // A line gone before its time (6000 ms), or one still there after it, is rejected.
    assert!(
        check_present(&page(&closing, leaves)?, "loopback-61", "終わりの言葉").is_err(),
        "a line gone before its time was accepted"
    );
    assert!(
        check_gone(&page(&closing, leaves - 1)?).is_err(),
        "a line still drawn after its time was accepted as gone"
    );

    // A line that never gets a translation: the wake-ups are the end of the
    // wait and the leaving, and the page at each is the one that is expected.
    let silent = model_of(&[(500, final_data("loopback-62", "en", "Anyone there"))])?;
    assert_eq!(
        wake_times(&silent, 500),
        [500 + TR_WAIT_MS, 500 + LANE_HOLD_MS]
    );
    let waiting_ends = page(&silent, 500 + TR_WAIT_MS)?;
    expect_text(
        line(lane(&waiting_ends, "upper")?, "loopback-62")?,
        "loopback-62",
        &["Anyone there", "No translation"],
        &[],
    )?;
    check_gone(&page(&silent, 500 + LANE_HOLD_MS)?)?;
    Ok(())
}

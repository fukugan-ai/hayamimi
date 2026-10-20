//! OV-6: the rules of the two lanes of the overlay, through the public API of
//! `hayamimi_ui::lanes` (design 11.3). No window and no GPU: it runs anywhere.
//!
//! Each rule is a check on the `LaneView` that `view` returns at a time. A
//! check is run on the real view, which must pass it, and on the view that a
//! wrong implementation would return, which it must reject, so that a check
//! that accepts everything is caught:
//!
//! - a translation pasted onto the newest line instead of its own;
//! - a `file-` line shown on a lane;
//! - lines removed after 6000 ms instead of 8000 ms.

use hayamimi_ui::lanes::{
    LANE_HOLD_MS, Lane, LaneLine, LaneRows, LaneView, LineState, TR_WAIT_MS, next_change, view,
};
use hayamimi_ui::model::Model;
use hayamimi_ui::tr::{TrRow, TrState};
use hayamimi_ui::wire::parse;

type Failure = Box<dyn std::error::Error>;
type Verdict = Result<(), String>;

const ROWS: LaneRows = LaneRows { upper: 2, lower: 1 };

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

fn withheld_data(id: &str, lang: &str, reason: &str) -> String {
    format!(
        r#"{{"type":"translation","of":"final","id":"{id}","lang":"{lang}","text":"","status":"held","hold_reason":"{reason}","settled":true}}"#
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

// ---- reading a view ----------------------------------------------------

fn ids(lines: &[LaneLine]) -> Vec<&str> {
    lines.iter().map(|line| line.id.as_str()).collect()
}

/// The text a line draws: its translation when it draws one, else its source.
fn drawn(line: &LaneLine) -> &str {
    line.tr
        .as_ref()
        .map_or(line.source_text.as_str(), |tr| tr.text.as_str())
}

fn find<'a>(lines: &'a [LaneLine], id: &str) -> Result<&'a LaneLine, String> {
    lines
        .iter()
        .find(|line| line.id == id)
        .ok_or_else(|| format!("{id} is not on the lane; the lane holds {:?}", ids(lines)))
}

fn expect(condition: bool, said: impl FnOnce() -> String) -> Verdict {
    if condition { Ok(()) } else { Err(said()) }
}

// ---- the views a wrong implementation would return ----------------------

/// A line as a wrong implementation would hand it back.
fn made(
    id: &str,
    lane: Lane,
    state: LineState,
    source: &str,
    translation: Option<&str>,
) -> LaneLine {
    let drawn_in = match lane {
        Lane::Upper => "ja",
        Lane::Lower => "en",
    };
    LaneLine {
        id: id.to_owned(),
        lane,
        state,
        lang: String::from(if lane == Lane::Upper { "en" } else { "ja" }),
        source_text: source.to_owned(),
        tr: translation.map(|text| TrRow {
            lang: drawn_in.to_owned(),
            text: text.to_owned(),
            annotations: Vec::new(),
            state: TrState::Confirmed,
        }),
    }
}

// ---- the rules -----------------------------------------------------------

/// Rules 1 and 2: a line goes to the lane its id begins with, the upper lane
/// keeps the newest two and the lower the newest one, oldest first, and no
/// other line is on a lane.
fn split_model() -> Result<Model, Failure> {
    let mut messages = Vec::new();
    for number in 1..=3_u64 {
        let id = format!("loopback-{number}");
        messages.push((number * 10, final_data(&id, "en", "x")));
        messages.push((number * 10 + 1, translation_data(&id, "ja", "訳", true)));
    }
    for number in 1..=2_u64 {
        let id = format!("mic-{number}");
        messages.push((number * 10 + 5, final_data(&id, "ja", "え")));
        messages.push((number * 10 + 6, translation_data(&id, "en", "tr", true)));
    }
    messages.push((50, final_data("file-1", "en", "face to face")));
    messages.push((51, translation_data("file-1", "ja", "対面", true)));
    messages.push((
        52,
        String::from(r#"{"type":"final","text":"no id","lang":"en"}"#),
    ));
    model_of(&messages)
}

fn check_split(found: &LaneView) -> Verdict {
    expect(ids(&found.upper) == ["loopback-2", "loopback-3"], || {
        format!(
            "the upper lane holds {:?}, not the newest two loopback lines",
            ids(&found.upper)
        )
    })?;
    expect(ids(&found.lower) == ["mic-2"], || {
        format!(
            "the lower lane holds {:?}, not the newest mic line",
            ids(&found.lower)
        )
    })
}

/// Rule 3, the point of OV-6: the translation of the first line arrives 1200 ms
/// late, after the second line, and still belongs to the first.
fn late_model() -> Result<Model, Failure> {
    model_of(&[
        (0, final_data("loopback-1", "en", "first")),
        (500, final_data("loopback-2", "en", "second")),
        (1200, translation_data("loopback-1", "ja", "一つめ", true)),
    ])
}

fn check_late_translation(found: &LaneView) -> Verdict {
    expect(ids(&found.upper) == ["loopback-1"], || {
        format!(
            "the upper lane holds {:?}; only the first line has its translation",
            ids(&found.upper)
        )
    })?;
    let first = find(&found.upper, "loopback-1")?;
    expect(
        first.state == LineState::Settled && drawn(first) == "一つめ",
        || format!("loopback-1 is {:?} drawing {:?}", first.state, drawn(first)),
    )
}

/// Rule 4: an early translation is replaced, in the same row, by the deciding one.
fn check_replaced(early: &LaneView, decided: &LaneView) -> Verdict {
    let draft = find(&early.upper, "loopback-1")?;
    expect(
        draft.state == LineState::Draft && drawn(draft) == "やあ",
        || {
            format!(
                "the early translation is {:?} drawing {:?}",
                draft.state,
                drawn(draft)
            )
        },
    )?;
    expect(ids(&decided.upper) == ["loopback-1"], || {
        format!(
            "the deciding translation left the lane with {:?}",
            ids(&decided.upper)
        )
    })?;
    let settled = find(&decided.upper, "loopback-1")?;
    expect(
        settled.state == LineState::Settled && drawn(settled) == "こんにちは",
        || {
            format!(
                "the deciding translation is {:?} drawing {:?}",
                settled.state,
                drawn(settled)
            )
        },
    )
}

/// Rule 5: a withheld translation shows the source, marked, and draws no translation.
fn check_held(found: &LaneView) -> Verdict {
    let line = find(&found.lower, "mic-1")?;
    expect(line.state == LineState::Held, || {
        format!("mic-1 is {:?}, not Held", line.state)
    })?;
    expect(
        line.tr.is_none() && line.source_text == "予算は五百万円です",
        || format!("a held line draws {:?}", drawn(line)),
    )
}

/// Rule 6: a line that is not translated shows its source when it arrives.
fn sources_model() -> Result<Model, Failure> {
    model_of(&[
        (0, final_data("loopback-1", "ja", "メディアの日本語")),
        (0, final_data("loopback-2", "zh", "第三の言語")),
        (0, final_data("mic-1", "en", "spoken in english")),
        // A translation into the language the lane does not translate into is not drawn.
        (
            50,
            translation_data("loopback-1", "en", "media japanese", true),
        ),
    ])
}

fn check_sources(found: &LaneView) -> Verdict {
    expect(ids(&found.upper) == ["loopback-1", "loopback-2"], || {
        format!("the upper lane holds {:?}", ids(&found.upper))
    })?;
    expect(ids(&found.lower) == ["mic-1"], || {
        format!("the lower lane holds {:?}", ids(&found.lower))
    })?;
    for line in found.upper.iter().chain(&found.lower) {
        expect(line.state == LineState::Source && line.tr.is_none(), || {
            format!(
                "{} is {:?} with a translation of {:?}",
                line.id, line.state, line.tr
            )
        })?;
    }
    expect(drawn(&found.upper[0]) == "メディアの日本語", || {
        format!("loopback-1 draws {:?}", drawn(&found.upper[0]))
    })
}

/// Rule 7: a line that is translated waits `TR_WAIT_MS` for its translation, then
/// shows its source marked, and takes a translation that comes later.
fn check_wait(model: &Model, replaced: &Model) -> Verdict {
    let before = view(model, ROWS, 1000 + TR_WAIT_MS - 1);
    expect(before.upper.is_empty(), || {
        format!(
            "a line was drawn {} ms before its wait ends: {:?}",
            1,
            ids(&before.upper)
        )
    })?;
    let after = view(model, ROWS, 1000 + TR_WAIT_MS);
    let line = find(&after.upper, "loopback-1")?;
    expect(
        line.state == LineState::Untranslated && line.source_text == "hello",
        || {
            format!(
                "after the wait loopback-1 is {:?} drawing {:?}",
                line.state,
                drawn(line)
            )
        },
    )?;
    let late = view(replaced, ROWS, 5000);
    let taken = find(&late.upper, "loopback-1")?;
    expect(
        taken.state == LineState::Settled && drawn(taken) == "こんにちは",
        || {
            format!(
                "a late translation left loopback-1 {:?} drawing {:?}",
                taken.state,
                drawn(taken)
            )
        },
    )
}

/// Rule 8: a line leaves `LANE_HOLD_MS` after it last changed.
fn check_present(found: &LaneView, id: &str, at_ms: u64) -> Verdict {
    find(&found.upper, id)
        .map(|_| ())
        .map_err(|said| format!("at {at_ms} ms: {said}"))
}

fn check_gone(found: &LaneView, at_ms: u64) -> Verdict {
    expect(found.upper.is_empty() && found.lower.is_empty(), || {
        format!(
            "at {at_ms} ms the lanes still hold {:?} and {:?}",
            ids(&found.upper),
            ids(&found.lower)
        )
    })
}

#[test]
fn ov06_lanes_split_join_replace_and_expire() -> Result<(), Failure> {
    // The constants of the contract are written out, so that a changed one is a failure.
    assert_eq!((LANE_HOLD_MS, TR_WAIT_MS), (8000, 3000));

    // Rules 1 and 2, with the positive control (two loopback lines and one mic
    // line come out on their own lanes) and the negative control (a `file-` line shown).
    let split = split_model()?;
    check_split(&view(&split, ROWS, 100))?;
    let with_a_file_line = LaneView {
        upper: vec![
            made(
                "loopback-3",
                Lane::Upper,
                LineState::Settled,
                "x",
                Some("訳"),
            ),
            made(
                "file-1",
                Lane::Upper,
                LineState::Settled,
                "face to face",
                Some("対面"),
            ),
        ],
        lower: vec![made(
            "mic-2",
            Lane::Lower,
            LineState::Settled,
            "え",
            Some("tr"),
        )],
    };
    assert!(
        check_split(&with_a_file_line).is_err(),
        "a file- line on a lane was accepted"
    );

    // Rule 3, with the negative control (the translation pasted onto the newest line).
    check_late_translation(&view(&late_model()?, ROWS, 1250))?;
    let pasted_on_the_newest = LaneView {
        upper: vec![made(
            "loopback-2",
            Lane::Upper,
            LineState::Settled,
            "second",
            Some("一つめ"),
        )],
        lower: Vec::new(),
    };
    assert!(
        check_late_translation(&pasted_on_the_newest).is_err(),
        "a translation pasted onto the newest line was accepted"
    );

    // Rule 4.
    let mut replacing = model_of(&[
        (0, final_data("loopback-1", "en", "hello")),
        (100, translation_data("loopback-1", "ja", "やあ", false)),
    ])?;
    let early = view(&replacing, ROWS, 200);
    replacing.apply(
        parse(&translation_data("loopback-1", "ja", "こんにちは", true))?,
        900,
    );
    check_replaced(&early, &view(&replacing, ROWS, 1000))?;

    // Rule 5.
    let held = model_of(&[
        (0, final_data("mic-1", "ja", "予算は五百万円です")),
        (100, withheld_data("mic-1", "en", "number")),
    ])?;
    check_held(&view(&held, ROWS, 200))?;

    // Rule 6.
    check_sources(&view(&sources_model()?, ROWS, 0))?;

    // Rule 7.
    let waiting = model_of(&[(1000, final_data("loopback-1", "en", "hello"))])?;
    let mut answered = waiting.clone();
    answered.apply(
        parse(&translation_data("loopback-1", "ja", "こんにちは", true))?,
        5000,
    );
    check_wait(&waiting, &answered)?;

    // Rule 8, with the negative control (lines removed after 6000 ms).
    let expiring = model_of(&[
        (1000, final_data("loopback-1", "en", "hello")),
        (
            2000,
            translation_data("loopback-1", "ja", "こんにちは", true),
        ),
    ])?;
    let last_change = 2000;
    let still = last_change + LANE_HOLD_MS - 1;
    check_present(&view(&expiring, ROWS, still), "loopback-1", still)?;
    check_gone(
        &view(&expiring, ROWS, last_change + LANE_HOLD_MS),
        last_change + LANE_HOLD_MS,
    )?;
    let removed_after_six_seconds = LaneView::default();
    assert!(
        check_present(&removed_after_six_seconds, "loopback-1", last_change + 6000).is_err(),
        "a line removed after 6000 ms was accepted"
    );

    // The times at which the caller has to look again are the end of the wait and the leaving.
    assert_eq!(next_change(&waiting, ROWS, 1000), Some(1000 + TR_WAIT_MS));
    assert_eq!(
        next_change(&waiting, ROWS, 1000 + TR_WAIT_MS),
        Some(1000 + LANE_HOLD_MS)
    );
    assert_eq!(next_change(&waiting, ROWS, 1000 + LANE_HOLD_MS), None);
    Ok(())
}

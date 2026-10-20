//! The two lanes of the subtitle band: which lines of the model each lane shows
//! at a given time, and the page that draws them.
//!
//! The upper lane holds the lines of the other side (`loopback-<n>`), the lower
//! lane the lines of the speaker (`mic-<n>`). A line that is translated shows
//! the translation into the opposite language; a line that is not shows its
//! source when it arrives. `view` is pure: it reads no clock and does no I/O,
//! and `next_change` says when the caller has to call it again, so a window and
//! a test apply the same rules.

use dioxus::prelude::*;

use crate::css;
use crate::model::{FinalRow, Model};
use crate::parts::Words;
use crate::tr::{Tone, TrRow, TrState, pieces};

/// A line leaves its lane this long after it last changed.
pub const LANE_HOLD_MS: u64 = 8000;
/// A line that is translated waits this long for its translation; after that
/// its source is drawn, and a translation that comes later replaces it.
pub const TR_WAIT_MS: u64 = 3000;

/// The key of the mark drawn at the head of a line shown in its source because
/// the translation was withheld.
pub const HELD_MARK_KEY: &str = "ui.lanes.held_mark";
/// The key of the mark drawn on a line shown in its source because no
/// translation came.
pub const UNTRANSLATED_MARK_KEY: &str = "ui.lanes.untranslated_mark";

const UPPER_PREFIX: &str = "loopback-";
const LOWER_PREFIX: &str = "mic-";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lane {
    Upper,
    Lower,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineState {
    /// The source of a line that is not translated.
    Source,
    /// A translation that a later one is going to replace.
    Draft,
    /// The translation that decides the line.
    Settled,
    /// The translation was withheld: the source, with a mark.
    Held,
    /// No translation came, or it cannot be made: the source, with a mark.
    Untranslated,
}

/// How many lines each lane shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LaneRows {
    pub upper: usize,
    pub lower: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LaneLine {
    pub id: String,
    pub lane: Lane,
    pub state: LineState,
    /// The language of the source line.
    pub lang: String,
    /// Drawn for `Source`, `Held` and `Untranslated`.
    pub source_text: String,
    /// Drawn for `Draft` and `Settled`; the opposite language only.
    pub tr: Option<TrRow>,
}

/// The lines of the two lanes, oldest first.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LaneView {
    pub upper: Vec<LaneLine>,
    pub lower: Vec<LaneLine>,
}

fn lane_of(id: &str) -> Option<Lane> {
    if id.starts_with(UPPER_PREFIX) {
        Some(Lane::Upper)
    } else if id.starts_with(LOWER_PREFIX) {
        Some(Lane::Lower)
    } else {
        None
    }
}

/// The language a line of `lang` in `lane` is translated into; `None` when the
/// lane draws such a line as it is.
fn target_lang(lane: Lane, lang: &str) -> Option<&'static str> {
    match (lane, lang) {
        (Lane::Upper, "en") => Some("ja"),
        (Lane::Lower, "ja") => Some("en"),
        _ => None,
    }
}

/// A row that is on a lane at some time, and what it shows there.
struct Shown<'a> {
    row: &'a FinalRow,
    id: &'a str,
    lane: Lane,
    state: LineState,
    tr: Option<&'a TrRow>,
}

fn state_of_translation(tr: &TrRow) -> (LineState, Option<&TrRow>) {
    match tr.state {
        TrState::Partial { .. } | TrState::First => (LineState::Draft, Some(tr)),
        TrState::Confirmed => (LineState::Settled, Some(tr)),
        TrState::Held { .. } => (LineState::Held, None),
        TrState::Unavailable => (LineState::Untranslated, None),
    }
}

/// What a line that is translated into `target` shows at `now_ms`; `None`
/// while it is still waiting for its translation.
fn translated_state<'a>(
    row: &'a FinalRow,
    target: &str,
    now_ms: u64,
) -> Option<(LineState, Option<&'a TrRow>)> {
    match row.trs.iter().find(|tr| tr.lang == target) {
        Some(tr) => Some(state_of_translation(tr)),
        None if now_ms >= row.updated_ms.saturating_add(TR_WAIT_MS) => {
            Some((LineState::Untranslated, None))
        }
        None => None,
    }
}

/// The lane and the state of `row` at `now_ms`; `None` when the row is on no
/// lane then: it belongs to neither, it has left, or it is waiting.
fn shown(row: &FinalRow, now_ms: u64) -> Option<Shown<'_>> {
    let id = row.id.as_deref()?;
    let lane = lane_of(id)?;
    if now_ms >= row.updated_ms.saturating_add(LANE_HOLD_MS) {
        return None;
    }
    let (state, tr) = match target_lang(lane, &row.lang) {
        Some(target) => translated_state(row, target, now_ms)?,
        None => (LineState::Source, None),
    };
    Some(Shown {
        row,
        id,
        lane,
        state,
        tr,
    })
}

fn line_of(found: &Shown<'_>) -> LaneLine {
    LaneLine {
        id: found.id.to_owned(),
        lane: found.lane,
        state: found.state,
        lang: found.row.lang.clone(),
        source_text: found.row.text.clone(),
        tr: found.tr.cloned(),
    }
}

/// The lines the two lanes show at `now_ms`: the newest `rows` of each lane,
/// oldest first. Pure: no clock, no I/O.
pub fn view(model: &Model, rows: LaneRows, now_ms: u64) -> LaneView {
    let mut out = LaneView::default();
    for row in model.finals.iter().rev() {
        if out.upper.len() >= rows.upper && out.lower.len() >= rows.lower {
            break;
        }
        let Some(found) = shown(row, now_ms) else {
            continue;
        };
        let (lines, room) = match found.lane {
            Lane::Upper => (&mut out.upper, rows.upper),
            Lane::Lower => (&mut out.lower, rows.lower),
        };
        if lines.len() < room {
            lines.push(line_of(&found));
        }
    }
    out.upper.reverse();
    out.lower.reverse();
    out
}

/// The times at which a row can start or stop being shown, or end its wait.
fn thresholds(row: &FinalRow) -> [u64; 2] {
    [
        row.updated_ms.saturating_add(TR_WAIT_MS),
        row.updated_ms.saturating_add(LANE_HOLD_MS),
    ]
}

/// The earliest time after `now_ms` at which `view` returns something else (a
/// wait of `TR_WAIT_MS` ends or a line reaches `LANE_HOLD_MS`); `None` when
/// nothing waits. A time at which only a line that is not shown changes is
/// skipped.
pub fn next_change(model: &Model, rows: LaneRows, now_ms: u64) -> Option<u64> {
    let now = view(model, rows, now_ms);
    let mut due: Vec<u64> = model
        .finals
        .iter()
        .flat_map(thresholds)
        .filter(|at| *at > now_ms)
        .collect();
    due.sort_unstable();
    due.dedup();
    due.into_iter().find(|at| view(model, rows, *at) != now)
}

/// The key of the mark a state draws, if it draws one.
fn mark_key(state: LineState) -> Option<&'static str> {
    match state {
        LineState::Held => Some(HELD_MARK_KEY),
        LineState::Untranslated => Some(UNTRANSLATED_MARK_KEY),
        LineState::Source | LineState::Draft | LineState::Settled => None,
    }
}

fn state_class(state: LineState) -> &'static str {
    match state {
        LineState::Source => "source",
        LineState::Draft => "draft",
        LineState::Settled => "settled",
        LineState::Held => "held",
        LineState::Untranslated => "untranslated",
    }
}

/// The class that picks the font of the language a line is drawn in.
fn font_class(line: &LaneLine) -> &'static str {
    let drawn = line
        .tr
        .as_ref()
        .map_or(line.lang.as_str(), |tr| tr.lang.as_str());
    match drawn {
        "ja" => "t-ja",
        "en" => "t-en",
        _ => "t-xx",
    }
}

fn piece_class(tone: Tone) -> Option<&'static str> {
    match tone {
        Tone::Plain => None,
        Tone::Faint => Some("faint"),
        Tone::Note => Some("note"),
    }
}

#[component]
fn LineView(line: LaneLine) -> Element {
    let words = use_context::<Words>();
    let class = format!("line {} {}", state_class(line.state), font_class(&line));
    rsx! {
        div { id: "line-{line.id}", class: "{class}",
            if let Some(key) = mark_key(line.state) {
                span { class: "mark", "{words.text(key)}" }
            }
            match &line.tr {
                Some(tr) => rsx! {
                    for piece in pieces(tr) {
                        span { class: piece_class(piece.tone), "{piece.text}" }
                    }
                },
                None => rsx! { "{line.source_text}" },
            }
        }
    }
}

#[component]
fn LaneColumn(dom_id: &'static str, rows: usize, lines: Vec<LaneLine>) -> Element {
    rsx! {
        div { id: "{dom_id}", class: "lane", style: "--rows: {rows}",
            for line in lines {
                LineView { key: "{line.id}", line }
            }
        }
    }
}

/// Draws `view(&model.read(), rows, now_ms())`. The host provides `Words` and
/// moves `now_ms` at the times `next_change` gives.
#[component]
pub fn LanesPage(model: Signal<Model>, rows: LaneRows, now_ms: Signal<u64>) -> Element {
    let sheet = css::lanes();
    let lines = view(&model.read(), rows, now_ms());
    rsx! {
        style { "{sheet}" }
        LaneColumn { dom_id: "lane-upper", rows: rows.upper, lines: lines.upper }
        LaneColumn { dom_id: "lane-lower", rows: rows.lower, lines: lines.lower }
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::*;
    use crate::strings::{Locale, Strings};
    use crate::wire::parse;

    type Failure = Box<dyn std::error::Error>;

    const ROWS: LaneRows = LaneRows { upper: 2, lower: 1 };

    fn final_data(id: &str, lang: &str, text: &str) -> String {
        format!(
            r#"{{"type":"final","id":"{id}","text":"{text}","lang":"{lang}","speaker":"","latency_ms":300.0}}"#
        )
    }

    fn tr_data(id: &str, lang: &str, text: &str, settled: bool) -> String {
        format!(
            r#"{{"type":"translation","of":"final","id":"{id}","lang":"{lang}","text":"{text}","status":"ok","settled":{settled}}}"#
        )
    }

    fn withheld_data(id: &str, lang: &str, reason: &str) -> String {
        format!(
            r#"{{"type":"translation","of":"final","id":"{id}","lang":"{lang}","text":"","status":"held","hold_reason":"{reason}","settled":true}}"#
        )
    }

    /// The model after each message arrived at its time.
    fn model_of(messages: &[(u64, String)]) -> Result<Model, Failure> {
        let mut model = Model::default();
        for (at, data) in messages {
            model.apply(parse(data)?, *at);
        }
        Ok(model)
    }

    fn ids(lines: &[LaneLine]) -> Vec<&str> {
        lines.iter().map(|line| line.id.as_str()).collect()
    }

    fn drawn_text(line: &LaneLine) -> &str {
        line.tr
            .as_ref()
            .map_or(line.source_text.as_str(), |tr| tr.text.as_str())
    }

    #[test]
    fn a_line_belongs_to_the_lane_its_id_begins_with() {
        assert_eq!(lane_of("loopback-3"), Some(Lane::Upper));
        assert_eq!(lane_of("mic-3"), Some(Lane::Lower));
        for id in ["file-3", "loopbackx-3", "mic3", "", "xmic-3"] {
            assert_eq!(lane_of(id), None, "{id}");
        }
    }

    #[test]
    fn only_an_english_line_above_and_a_japanese_line_below_are_translated() {
        assert_eq!(target_lang(Lane::Upper, "en"), Some("ja"));
        assert_eq!(target_lang(Lane::Lower, "ja"), Some("en"));
        for (lane, lang) in [
            (Lane::Upper, "ja"),
            (Lane::Upper, "zh"),
            (Lane::Upper, ""),
            (Lane::Lower, "en"),
            (Lane::Lower, "zh"),
            (Lane::Lower, ""),
        ] {
            assert_eq!(target_lang(lane, lang), None, "{lane:?} {lang}");
        }
    }

    #[test]
    fn a_loopback_line_goes_up_a_mic_line_goes_down_and_no_other_line_goes_anywhere()
    -> Result<(), Failure> {
        let model = model_of(&[
            (0, final_data("loopback-1", "en", "up")),
            (0, final_data("mic-1", "ja", "down")),
            (0, final_data("file-1", "en", "face to face")),
            (
                0,
                String::from(r#"{"type":"final","text":"no id","lang":"en"}"#),
            ),
            (0, final_data("loopbackx-1", "en", "near miss")),
            (100, tr_data("loopback-1", "ja", "上", true)),
            (100, tr_data("mic-1", "en", "down", true)),
            (100, tr_data("file-1", "ja", "対面", true)),
        ])?;
        let found = view(&model, ROWS, 200);
        assert_eq!(ids(&found.upper), ["loopback-1"]);
        assert_eq!(ids(&found.lower), ["mic-1"]);
        assert_eq!(found.upper[0].lane, Lane::Upper);
        assert_eq!(found.lower[0].lane, Lane::Lower);
        Ok(())
    }

    #[test]
    fn the_upper_lane_keeps_the_newest_two_the_lower_the_newest_one_oldest_first()
    -> Result<(), Failure> {
        let mut messages = Vec::new();
        for number in 1..=3_u64 {
            let at = number * 10;
            messages.push((at, final_data(&format!("loopback-{number}"), "en", "x")));
            messages.push((
                at + 1,
                tr_data(&format!("loopback-{number}"), "ja", "訳", true),
            ));
        }
        for number in 1..=2_u64 {
            let at = number * 10 + 5;
            messages.push((at, final_data(&format!("mic-{number}"), "ja", "え")));
            messages.push((at + 1, tr_data(&format!("mic-{number}"), "en", "tr", true)));
        }
        let model = model_of(&messages)?;
        let found = view(&model, ROWS, 100);
        assert_eq!(ids(&found.upper), ["loopback-2", "loopback-3"]);
        assert_eq!(ids(&found.lower), ["mic-2"]);
        let wide = view(&model, LaneRows { upper: 3, lower: 2 }, 100);
        assert_eq!(ids(&wide.upper), ["loopback-1", "loopback-2", "loopback-3"]);
        assert_eq!(ids(&wide.lower), ["mic-1", "mic-2"]);
        let none = view(&model, LaneRows { upper: 0, lower: 0 }, 100);
        assert_eq!(none, LaneView::default());
        Ok(())
    }

    #[test]
    fn a_translation_that_is_late_joins_its_own_line_and_not_the_newest() -> Result<(), Failure> {
        let model = model_of(&[
            (0, final_data("loopback-1", "en", "first")),
            (500, final_data("loopback-2", "en", "second")),
            (1200, tr_data("loopback-1", "ja", "一つめ", true)),
        ])?;
        let found = view(&model, ROWS, 1250);
        // The second line has no translation yet, so only the first is drawn.
        assert_eq!(ids(&found.upper), ["loopback-1"]);
        assert_eq!(drawn_text(&found.upper[0]), "一つめ");
        let model = model_of(&[
            (0, final_data("loopback-1", "en", "first")),
            (500, final_data("loopback-2", "en", "second")),
            (1200, tr_data("loopback-1", "ja", "一つめ", true)),
            (1300, tr_data("loopback-2", "ja", "二つめ", true)),
        ])?;
        let found = view(&model, ROWS, 1400);
        assert_eq!(ids(&found.upper), ["loopback-1", "loopback-2"]);
        assert_eq!(drawn_text(&found.upper[0]), "一つめ");
        assert_eq!(drawn_text(&found.upper[1]), "二つめ");
        Ok(())
    }

    #[test]
    fn an_early_translation_is_replaced_in_the_same_row_by_the_deciding_one() -> Result<(), Failure>
    {
        let mut model = model_of(&[
            (0, final_data("loopback-1", "en", "hello")),
            (100, tr_data("loopback-1", "ja", "やあ", false)),
        ])?;
        let early = view(&model, ROWS, 200);
        assert_eq!(early.upper[0].state, LineState::Draft);
        assert_eq!(drawn_text(&early.upper[0]), "やあ");
        model.apply(
            parse(&tr_data("loopback-1", "ja", "こんにちは", true))?,
            900,
        );
        let decided = view(&model, ROWS, 1000);
        assert_eq!(ids(&decided.upper), ["loopback-1"]);
        assert_eq!(decided.upper[0].state, LineState::Settled);
        assert_eq!(drawn_text(&decided.upper[0]), "こんにちは");
        Ok(())
    }

    #[test]
    fn a_withheld_translation_shows_the_source_as_held_and_draws_no_translation()
    -> Result<(), Failure> {
        let model = model_of(&[
            (0, final_data("mic-1", "ja", "予算は五百万円です")),
            (100, withheld_data("mic-1", "en", "number")),
        ])?;
        let line = &view(&model, ROWS, 200).lower[0];
        assert_eq!(line.state, LineState::Held);
        assert_eq!(line.source_text, "予算は五百万円です");
        assert_eq!(line.tr, None);
        assert_eq!(mark_key(line.state), Some(HELD_MARK_KEY));
        Ok(())
    }

    #[test]
    fn a_translator_that_cannot_be_reached_shows_the_source_as_untranslated() -> Result<(), Failure>
    {
        let model = model_of(&[
            (0, final_data("loopback-1", "en", "hello")),
            (100, withheld_data("loopback-1", "ja", "unavailable")),
        ])?;
        let line = &view(&model, ROWS, 200).upper[0];
        assert_eq!(line.state, LineState::Untranslated);
        assert_eq!(line.source_text, "hello");
        assert_eq!(line.tr, None);
        assert_eq!(mark_key(line.state), Some(UNTRANSLATED_MARK_KEY));
        Ok(())
    }

    #[test]
    fn a_line_that_is_not_translated_shows_its_source_when_it_arrives() -> Result<(), Failure> {
        let model = model_of(&[
            (0, final_data("loopback-1", "ja", "メディアの日本語")),
            (0, final_data("loopback-2", "zh", "第三の言語")),
            (0, final_data("mic-1", "en", "spoken in english")),
            // A translation of a line the lane does not translate is not drawn.
            (50, tr_data("loopback-1", "en", "media japanese", true)),
        ])?;
        let found = view(&model, ROWS, 0);
        assert_eq!(ids(&found.upper), ["loopback-1", "loopback-2"]);
        assert_eq!(ids(&found.lower), ["mic-1"]);
        for line in found.upper.iter().chain(found.lower.iter()) {
            assert_eq!(line.state, LineState::Source, "{}", line.id);
            assert_eq!(line.tr, None, "{}", line.id);
        }
        assert_eq!(found.upper[0].source_text, "メディアの日本語");
        Ok(())
    }

    #[test]
    fn a_line_waits_three_seconds_for_its_translation_then_shows_its_source_and_takes_a_late_one()
    -> Result<(), Failure> {
        let mut model = model_of(&[(1000, final_data("loopback-1", "en", "hello"))])?;
        assert_eq!(view(&model, ROWS, 1000), LaneView::default());
        assert_eq!(view(&model, ROWS, 3999), LaneView::default());
        let waited = view(&model, ROWS, 4000);
        assert_eq!(waited.upper[0].state, LineState::Untranslated);
        assert_eq!(waited.upper[0].source_text, "hello");
        model.apply(
            parse(&tr_data("loopback-1", "ja", "こんにちは", true))?,
            5000,
        );
        let replaced = view(&model, ROWS, 5000);
        assert_eq!(ids(&replaced.upper), ["loopback-1"]);
        assert_eq!(replaced.upper[0].state, LineState::Settled);
        assert_eq!(drawn_text(&replaced.upper[0]), "こんにちは");
        Ok(())
    }

    #[test]
    fn only_the_translation_into_the_opposite_language_is_drawn() -> Result<(), Failure> {
        let wrong = model_of(&[
            (0, final_data("loopback-1", "en", "hello")),
            (100, tr_data("loopback-1", "en", "hello again", true)),
        ])?;
        // The only translation is not into Japanese, so the line still waits.
        assert_eq!(view(&wrong, ROWS, 200), LaneView::default());
        let both = model_of(&[
            (0, final_data("loopback-1", "en", "hello")),
            (100, tr_data("loopback-1", "en", "hello again", true)),
            (200, tr_data("loopback-1", "ja", "こんにちは", true)),
        ])?;
        assert_eq!(drawn_text(&view(&both, ROWS, 300).upper[0]), "こんにちは");
        Ok(())
    }

    /// The numbers are written out, so that a changed constant is a failure.
    #[test]
    fn the_hold_is_eight_seconds_and_the_wait_is_three() {
        assert_eq!(LANE_HOLD_MS, 8000);
        assert_eq!(TR_WAIT_MS, 3000);
    }

    #[test]
    fn a_line_leaves_eight_seconds_after_it_last_changed() -> Result<(), Failure> {
        let source = model_of(&[(0, final_data("loopback-1", "ja", "x"))])?;
        assert_eq!(view(&source, ROWS, 7999).upper.len(), 1);
        assert_eq!(view(&source, ROWS, 8000).upper.len(), 0);
        let translated = model_of(&[
            (0, final_data("mic-1", "ja", "x")),
            (1000, tr_data("mic-1", "en", "x", true)),
        ])?;
        assert_eq!(view(&translated, ROWS, 8999).lower.len(), 1);
        assert_eq!(view(&translated, ROWS, 9000).lower.len(), 0);
        Ok(())
    }

    #[test]
    fn ov06_lanes_split_join_replace_and_expire() -> Result<(), Failure> {
        let mut model = model_of(&[
            (0, final_data("loopback-1", "en", "first")),
            (400, final_data("loopback-2", "en", "second")),
            (500, final_data("mic-1", "ja", "自分")),
        ])?;
        // Join: the translation of the first line is 1200 ms late and still
        // goes to the first line.
        model.apply(parse(&tr_data("loopback-1", "ja", "早い訳", false))?, 1200);
        let early = view(&model, ROWS, 1300);
        assert_eq!(ids(&early.upper), ["loopback-1"]);
        assert_eq!(early.upper[0].state, LineState::Draft);
        // Replace: the deciding translation takes the place of the early one.
        model.apply(parse(&tr_data("loopback-1", "ja", "決めの訳", true))?, 1800);
        model.apply(parse(&tr_data("loopback-2", "ja", "二つめ", true))?, 1900);
        model.apply(parse(&tr_data("mic-1", "en", "myself", true))?, 2000);
        // Split: two lines above, one below.
        let found = view(&model, ROWS, 2100);
        assert_eq!(ids(&found.upper), ["loopback-1", "loopback-2"]);
        assert_eq!(ids(&found.lower), ["mic-1"]);
        assert_eq!(found.upper[0].state, LineState::Settled);
        assert_eq!(drawn_text(&found.upper[0]), "決めの訳");
        // Expire: eight seconds after each line last changed, not six.
        let late = view(&model, ROWS, 9799);
        assert_eq!(ids(&late.upper), ["loopback-1", "loopback-2"]);
        let gone = view(&model, ROWS, 10_000);
        assert_eq!(gone, LaneView::default());
        Ok(())
    }

    #[test]
    fn nothing_is_waiting_in_an_empty_model() {
        assert_eq!(next_change(&Model::default(), ROWS, 0), None);
    }

    #[test]
    fn the_change_after_a_line_arrives_is_the_end_of_its_wait_and_then_its_leaving()
    -> Result<(), Failure> {
        let model = model_of(&[(1000, final_data("loopback-1", "en", "hello"))])?;
        assert_eq!(next_change(&model, ROWS, 1000), Some(4000));
        assert_eq!(next_change(&model, ROWS, 4000), Some(9000));
        assert_eq!(next_change(&model, ROWS, 9000), None);
        Ok(())
    }

    #[test]
    fn a_wait_that_a_translation_already_ended_is_not_a_change() -> Result<(), Failure> {
        let model = model_of(&[
            (0, final_data("loopback-1", "en", "hello")),
            (1000, tr_data("loopback-1", "ja", "こんにちは", true)),
        ])?;
        // The wait would end at 4000; the translation at 1000 ended it, so
        // the next change is the line leaving.
        assert_eq!(next_change(&model, ROWS, 1500), Some(9000));
        Ok(())
    }

    #[test]
    fn a_line_that_is_not_shown_leaving_is_not_a_change() -> Result<(), Failure> {
        let one = LaneRows { upper: 1, lower: 0 };
        let model = model_of(&[
            (100, final_data("loopback-1", "en", "older")),
            (100, tr_data("loopback-1", "ja", "古い", true)),
            (5000, final_data("loopback-2", "en", "newer")),
            (5000, tr_data("loopback-2", "ja", "新しい", true)),
        ])?;
        assert_eq!(ids(&view(&model, one, 5001).upper), ["loopback-2"]);
        // The older line leaves at 8100 and the newer at 13000; only the
        // newer one is shown, so only 13000 changes what is drawn.
        assert_eq!(next_change(&model, one, 5001), Some(13_000));
        Ok(())
    }

    #[test]
    fn a_line_leaving_brings_back_an_older_one_that_is_still_in_its_time() -> Result<(), Failure> {
        let one = LaneRows { upper: 1, lower: 0 };
        let model = model_of(&[
            (0, final_data("loopback-1", "en", "older")),
            (100, final_data("loopback-2", "en", "newer")),
            (200, tr_data("loopback-2", "ja", "新しい", true)),
            (6000, tr_data("loopback-1", "ja", "古い", true)),
        ])?;
        assert_eq!(ids(&view(&model, one, 1000).upper), ["loopback-2"]);
        assert_eq!(next_change(&model, one, 1000), Some(8200));
        assert_eq!(ids(&view(&model, one, 8200).upper), ["loopback-1"]);
        Ok(())
    }

    #[test]
    fn the_font_follows_the_language_that_is_drawn() -> Result<(), Failure> {
        let model = model_of(&[
            (0, final_data("loopback-1", "en", "hello")),
            (0, final_data("loopback-2", "zh", "第三")),
            (0, final_data("mic-1", "ja", "自分")),
            (10, tr_data("loopback-1", "ja", "こんにちは", true)),
            (10, withheld_data("mic-1", "en", "number")),
        ])?;
        let found = view(&model, ROWS, 20);
        // A translation is drawn in the language it is into, a source in its own.
        assert_eq!(font_class(&found.upper[0]), "t-ja");
        assert_eq!(font_class(&found.upper[1]), "t-xx");
        assert_eq!(font_class(&found.lower[0]), "t-ja");
        let english_source = LaneLine {
            lang: String::from("en"),
            tr: None,
            ..found.lower[0].clone()
        };
        assert_eq!(font_class(&english_source), "t-en");
        Ok(())
    }

    #[test]
    fn a_state_draws_a_mark_only_when_it_shows_a_source_in_place_of_a_translation() {
        assert_eq!(mark_key(LineState::Held), Some(HELD_MARK_KEY));
        assert_eq!(
            mark_key(LineState::Untranslated),
            Some(UNTRANSLATED_MARK_KEY)
        );
        for state in [LineState::Source, LineState::Draft, LineState::Settled] {
            assert_eq!(mark_key(state), None, "{state:?}");
        }
    }

    #[test]
    fn both_marks_are_words_of_both_tables() -> Result<(), crate::strings::StringsError> {
        let ja = Strings::embedded(Locale::Ja)?;
        let en = Strings::embedded(Locale::En)?;
        for key in [HELD_MARK_KEY, UNTRANSLATED_MARK_KEY] {
            assert_ne!(ja.text(key), key, "{key}");
            assert_ne!(en.text(key), key, "{key}");
        }
        assert_eq!(ja.text(HELD_MARK_KEY), "原文");
        assert_eq!(en.text(HELD_MARK_KEY), "Original");
        Ok(())
    }

    #[derive(Props, Clone, PartialEq)]
    struct Root {
        model: Model,
        rows: LaneRows,
        now_ms: u64,
        words: Strings,
    }

    fn root(props: Root) -> Element {
        let words = props.words.clone();
        use_context_provider(|| -> Words { Rc::new(words) });
        let model = use_signal(|| props.model.clone());
        let now_ms = use_signal(|| props.now_ms);
        rsx! { LanesPage { model, rows: props.rows, now_ms } }
    }

    fn render(model: &Model, rows: LaneRows, now_ms: u64) -> Result<String, Failure> {
        let words = Strings::embedded(Locale::En)?;
        let mut dom = VirtualDom::new_with_props(
            root,
            Root {
                model: model.clone(),
                rows,
                now_ms,
                words,
            },
        );
        dom.rebuild_in_place();
        Ok(dioxus_ssr::render(&dom))
    }

    #[test]
    fn the_page_draws_each_lane_and_each_line_under_its_id_in_the_language_of_its_text()
    -> Result<(), Failure> {
        let model = model_of(&[
            (0, final_data("loopback-1", "en", "hello there")),
            (0, final_data("mic-1", "ja", "予算は五百万円です")),
            (10, tr_data("loopback-1", "ja", "こんにちは", true)),
            (10, withheld_data("mic-1", "en", "number")),
        ])?;
        let html = render(&model, ROWS, 20)?;
        assert!(html.contains(r#"id="lane-upper""#), "{html}");
        assert!(html.contains(r#"id="lane-lower""#), "{html}");
        assert!(html.contains("--rows: 2"), "{html}");
        assert!(html.contains("--rows: 1"), "{html}");
        assert!(html.contains(r#"id="line-loopback-1""#), "{html}");
        assert!(html.contains(r#"id="line-mic-1""#), "{html}");
        // The settled line draws its translation and not its source.
        assert!(html.contains("こんにちは"), "{html}");
        assert!(!html.contains("hello there"), "{html}");
        // The held line draws its source with the mark of the table.
        assert!(html.contains("予算は五百万円です"), "{html}");
        assert!(html.contains("Original"), "{html}");
        assert!(html.contains("t-ja"), "{html}");
        Ok(())
    }

    #[test]
    fn the_page_draws_the_lanes_and_no_line_while_nothing_has_arrived() -> Result<(), Failure> {
        let html = render(&Model::default(), ROWS, 0)?;
        assert!(html.contains(r#"id="lane-upper""#), "{html}");
        assert!(html.contains(r#"id="lane-lower""#), "{html}");
        assert!(!html.contains(r#"id="line-"#), "{html}");
        Ok(())
    }
}

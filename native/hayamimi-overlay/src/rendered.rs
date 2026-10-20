//! The `overlay.line.rendered` event: one for each time a line of a lane is
//! drawn in a state it was not drawn in before.
//!
//! The window asks `RenderLog::changes` after each time it has drawn. A line
//! drawn again in the state it already has says nothing; a line that has left
//! the lanes is forgotten. The text of the line is not written, only the
//! row's `id`, the lane and what was drawn of it.

use std::collections::HashMap;

use hayamimi_relay::logging::{Level, LogEvent};
use hayamimi_ui::lanes::{Lane, LaneLine, LaneView, LineState};

/// The lines last drawn, with the state each was drawn in.
#[derive(Default)]
pub struct RenderLog {
    drawn: HashMap<String, LineState>,
}

impl RenderLog {
    /// The events for the lines of `view` that are new or in a new state, in
    /// lane order (upper first, oldest first). Afterwards `view` is what was
    /// drawn last.
    pub fn changes(&mut self, view: &LaneView) -> Vec<LogEvent> {
        let mut events = Vec::new();
        let mut now_drawn = HashMap::with_capacity(self.drawn.len());
        for line in view.upper.iter().chain(&view.lower) {
            if self.drawn.get(&line.id) != Some(&line.state) {
                events.push(event_of(line));
            }
            now_drawn.insert(line.id.clone(), line.state);
        }
        self.drawn = now_drawn;
        events
    }
}

fn lane_name(lane: Lane) -> &'static str {
    match lane {
        Lane::Upper => "upper",
        Lane::Lower => "lower",
    }
}

fn state_name(state: LineState) -> &'static str {
    match state {
        LineState::Source => "source",
        LineState::Draft => "draft",
        LineState::Settled => "settled",
        LineState::Held => "held",
        LineState::Untranslated => "untranslated",
    }
}

/// There is no `stage` field: the wire carries the stage of a translation but
/// `hayamimi-ui` does not read it, so the field could only ever be empty. The
/// relay's log has the stage, by `id`.
fn event_of(line: &LaneLine) -> LogEvent {
    LogEvent::new(Level::Info, "overlay.line.rendered", "browser")
        .with("id", line.id.as_str())
        .with("lane", lane_name(line.lane))
        .with(
            "settled",
            matches!(line.state, LineState::Settled | LineState::Held),
        )
        .with("held", line.state == LineState::Held)
        .with("translated", line.tr.is_some())
        .with("state", state_name(line.state))
}

#[cfg(test)]
mod tests {
    use hayamimi_ui::lanes::{LaneRows, view};
    use hayamimi_ui::model::Model;
    use hayamimi_ui::wire::parse;
    use serde_json::Value;

    use super::*;

    type Failure = Box<dyn std::error::Error>;

    const ROWS: LaneRows = LaneRows { upper: 2, lower: 1 };

    fn apply(model: &mut Model, at: u64, data: &str) -> Result<(), Failure> {
        model.apply(parse(data)?, at);
        Ok(())
    }

    fn final_of(id: &str, lang: &str) -> String {
        format!(r#"{{"type":"final","id":"{id}","text":"x","lang":"{lang}"}}"#)
    }

    fn translation_of(id: &str, lang: &str, settled: bool) -> String {
        format!(
            r#"{{"type":"translation","of":"final","id":"{id}","lang":"{lang}","text":"y","status":"ok","settled":{settled}}}"#
        )
    }

    fn field<'a>(event: &'a LogEvent, key: &str) -> Option<&'a Value> {
        event.fields.get(key)
    }

    #[test]
    fn a_line_is_reported_when_it_first_shows_and_not_again_in_the_same_state()
    -> Result<(), Failure> {
        let mut model = Model::default();
        apply(&mut model, 0, &final_of("loopback-1", "ja"))?;
        let mut log = RenderLog::default();
        let first = log.changes(&view(&model, ROWS, 100));
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].event, "overlay.line.rendered");
        assert!(log.changes(&view(&model, ROWS, 200)).is_empty());
        Ok(())
    }

    #[test]
    fn a_draft_and_then_the_settled_translation_are_two_events() -> Result<(), Failure> {
        let mut model = Model::default();
        let mut log = RenderLog::default();
        apply(&mut model, 0, &final_of("loopback-1", "en"))?;
        assert!(log.changes(&view(&model, ROWS, 10)).is_empty());
        apply(&mut model, 300, &translation_of("loopback-1", "ja", false))?;
        let draft = log.changes(&view(&model, ROWS, 310));
        apply(&mut model, 900, &translation_of("loopback-1", "ja", true))?;
        let settled = log.changes(&view(&model, ROWS, 910));
        assert_eq!(draft.len(), 1);
        assert_eq!(settled.len(), 1);
        assert_eq!(field(&draft[0], "state"), Some(&Value::from("draft")));
        assert_eq!(field(&draft[0], "settled"), Some(&Value::from(false)));
        assert_eq!(field(&draft[0], "translated"), Some(&Value::from(true)));
        assert_eq!(field(&settled[0], "state"), Some(&Value::from("settled")));
        assert_eq!(field(&settled[0], "settled"), Some(&Value::from(true)));
        Ok(())
    }

    #[test]
    fn the_fields_name_the_row_the_lane_and_what_was_drawn() -> Result<(), Failure> {
        let mut model = Model::default();
        apply(&mut model, 0, &final_of("mic-4", "ja"))?;
        apply(&mut model, 5, &translation_of("mic-4", "en", true))?;
        let events = RenderLog::default().changes(&view(&model, ROWS, 10));
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(field(event, "id"), Some(&Value::from("mic-4")));
        assert_eq!(field(event, "lane"), Some(&Value::from("lower")));
        assert_eq!(field(event, "stage"), None);
        assert_eq!(field(event, "held"), Some(&Value::from(false)));
        assert_eq!(field(event, "translated"), Some(&Value::from(true)));
        Ok(())
    }

    #[test]
    fn a_line_with_no_translation_after_the_wait_is_reported_as_untranslated() -> Result<(), Failure>
    {
        let mut model = Model::default();
        let mut log = RenderLog::default();
        apply(&mut model, 0, &final_of("loopback-2", "en"))?;
        assert!(log.changes(&view(&model, ROWS, 2999)).is_empty());
        let events = log.changes(&view(&model, ROWS, 3000));
        assert_eq!(events.len(), 1);
        assert_eq!(
            field(&events[0], "state"),
            Some(&Value::from("untranslated"))
        );
        assert_eq!(field(&events[0], "translated"), Some(&Value::from(false)));
        Ok(())
    }

    #[test]
    fn a_withheld_translation_is_reported_as_held_and_not_translated() -> Result<(), Failure> {
        let mut model = Model::default();
        apply(&mut model, 0, &final_of("loopback-3", "en"))?;
        apply(
            &mut model,
            5,
            r#"{"type":"translation","of":"final","id":"loopback-3","lang":"ja","text":"","status":"held","hold_reason":"mismatch","settled":true}"#,
        )?;
        let events = RenderLog::default().changes(&view(&model, ROWS, 10));
        assert_eq!(events.len(), 1);
        assert_eq!(field(&events[0], "state"), Some(&Value::from("held")));
        assert_eq!(field(&events[0], "held"), Some(&Value::from(true)));
        assert_eq!(field(&events[0], "translated"), Some(&Value::from(false)));
        Ok(())
    }

    #[test]
    fn a_line_that_left_is_forgotten_and_a_new_row_is_reported() -> Result<(), Failure> {
        let mut model = Model::default();
        let mut log = RenderLog::default();
        apply(&mut model, 0, &final_of("loopback-1", "ja"))?;
        assert_eq!(log.changes(&view(&model, ROWS, 100)).len(), 1);
        assert!(log.changes(&view(&model, ROWS, 8000)).is_empty());
        apply(&mut model, 8100, &final_of("loopback-2", "ja"))?;
        let events = log.changes(&view(&model, ROWS, 8100));
        assert_eq!(events.len(), 1);
        assert_eq!(field(&events[0], "id"), Some(&Value::from("loopback-2")));
        Ok(())
    }

    #[test]
    fn two_lanes_are_reported_upper_first() -> Result<(), Failure> {
        let mut model = Model::default();
        apply(&mut model, 0, &final_of("mic-1", "en"))?;
        apply(&mut model, 1, &final_of("loopback-1", "ja"))?;
        let events = RenderLog::default().changes(&view(&model, ROWS, 10));
        let lanes: Vec<_> = events.iter().filter_map(|e| field(e, "lane")).collect();
        assert_eq!(lanes, [&Value::from("upper"), &Value::from("lower")]);
        Ok(())
    }
}

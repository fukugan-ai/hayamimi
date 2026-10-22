//! The layout probe (`--layout-probe`): three fixed rows, drawn as the lanes
//! draw them and measured by the page itself.
//!
//! The rows are the ones whose text reaches furthest down: Japanese in the
//! upper lane (a translation and a source) and English with descenders in the
//! lower. The page is asked, after its fonts have settled, for the height of
//! its window, the box and bottom edge of every row and lane, the state of
//! every face and the family each row is set in. What it answers is written
//! as one `overlay.layout.measured` event.

use hayamimi_relay::logging::{Level, LogEvent};
use hayamimi_ui::css::LANES_FONT_JA;
use hayamimi_ui::model::Model;
use hayamimi_ui::wire;
use serde::{Deserialize, Serialize};

use crate::config::Probe;

/// The messages of the three rows; they arrive at time 0 and the probe never
/// moves its clock, so they stay on the lanes.
const MESSAGES: [&str; 5] = [
    r#"{"type":"final","id":"loopback-1","text":"Could you send me the report by Friday?","lang":"en"}"#,
    r#"{"type":"translation","of":"final","id":"loopback-1","lang":"ja","text":"金曜日までにレポートを送っていただけますか。","status":"ok","settled":true}"#,
    r#"{"type":"final","id":"loopback-2","text":"次の議題は、予算の見直しです。","lang":"ja"}"#,
    r#"{"type":"final","id":"mic-1","text":"報告書は金曜日までに送ります。","lang":"ja"}"#,
    r#"{"type":"translation","of":"final","id":"mic-1","lang":"en","text":"I will send the report by Friday, typically by early afternoon.","status":"ok","settled":true}"#,
];

/// The characters whose faces the probe counts: the Japanese the upper lane draws.
const JAPANESE_SAMPLE: &str =
    "金曜日までにレポートを送っていただけますか。次の議題は、予算の見直しです。";

/// The model the probe draws.
pub fn model() -> Result<Model, serde_json::Error> {
    let mut model = Model::default();
    for data in MESSAGES {
        model.apply(wire::parse(data)?, 0);
    }
    Ok(model)
}

/// What the page answers.
#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub struct Measure {
    pub inner_height: f64,
    pub rows: Vec<RowBox>,
    pub lanes: Vec<LaneBox>,
    pub fonts: Vec<Face>,
    pub missing_glyphs: u64,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub struct RowBox {
    pub id: String,
    pub scroll_height: f64,
    pub client_height: f64,
    pub bottom: f64,
    pub font_family: String,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub struct LaneBox {
    pub lane: String,
    pub bottom: f64,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub struct Face {
    pub family: String,
    pub status: String,
}

/// The script the page runs: it waits for the fonts, measures, and sends the
/// answer back as an object of `Measure`'s shape. Lengths are CSS px.
///
/// `missing_glyphs` counts the sample characters that the family stack of the
/// upper lane draws exactly as the same stack without `cinecaption` does: the
/// advance and the ink box of the character (`measureText`) are the same in
/// both, so `cinecaption` supplied nothing, whether its face holds no glyph for
/// it or never loaded. `document.fonts.check` is not used: it answers whether
/// a face is loaded, not whether it holds a glyph.
pub fn script() -> String {
    let sample = serde_json::Value::from(JAPANESE_SAMPLE);
    let face = serde_json::Value::from(LANES_FONT_JA);
    format!(
        r#"
await document.fonts.ready;
const rows = [...document.querySelectorAll('.line')].map((el) => ({{
  id: el.id.replace(/^line-/, ''),
  scroll_height: el.scrollHeight,
  client_height: el.clientHeight,
  bottom: el.getBoundingClientRect().bottom,
  font_family: getComputedStyle(el).fontFamily,
}}));
const lanes = ['upper', 'lower'].map((lane) => ({{
  lane,
  bottom: document.getElementById('lane-' + lane).getBoundingClientRect().bottom,
}}));
const fonts = [...document.fonts].map((face) => ({{ family: face.family, status: face.status }}));
const canvas = document.createElement('canvas').getContext('2d');
const ink = (stack, ch) => {{
  canvas.font = '36px ' + stack;
  const m = canvas.measureText(ch);
  return [m.width, m.actualBoundingBoxLeft, m.actualBoundingBoxRight, m.actualBoundingBoxAscent, m.actualBoundingBoxDescent].join();
}};
const stack = getComputedStyle(document.querySelector('#lane-upper .line')).fontFamily;
const without = stack.split(',').map((name) => name.trim()).filter((name) => name.replace(/^["']|["']$/g, '') !== {face}).join(', ');
let missing = 0;
for (const ch of {sample}) {{
  if (ink(stack, ch) === ink(without, ch)) missing += 1;
}}
dioxus.send({{ inner_height: window.innerHeight, rows, lanes, fonts, missing_glyphs: missing }});
"#
    )
}

/// The `overlay.layout.measured` event for an answer of the page.
pub fn measured_event(measure: &Measure, probe: Probe) -> Result<LogEvent, serde_json::Error> {
    Ok(
        LogEvent::new(Level::Info, "overlay.layout.measured", "browser")
            .with("inner_height", measure.inner_height)
            .with("zoom", probe.zoom_percent)
            .with("probe_shrink", probe.shrink_px)
            .with("missing_glyphs", measure.missing_glyphs)
            .with("rows", serde_json::to_value(&measure.rows)?)
            .with("lanes", serde_json::to_value(&measure.lanes)?)
            .with("fonts", serde_json::to_value(&measure.fonts)?),
    )
}

#[cfg(test)]
mod tests {
    use hayamimi_ui::lanes::{LaneRows, view};

    use super::*;

    type Failure = Box<dyn std::error::Error>;

    #[test]
    fn the_probe_shows_two_rows_in_the_upper_lane_and_one_in_the_lower() -> Result<(), Failure> {
        let shown = view(&model()?, LaneRows { upper: 2, lower: 1 }, 0);
        let ids = |lines: &[hayamimi_ui::lanes::LaneLine]| -> Vec<String> {
            lines.iter().map(|line| line.id.clone()).collect()
        };
        assert_eq!(ids(&shown.upper), ["loopback-1", "loopback-2"]);
        assert_eq!(ids(&shown.lower), ["mic-1"]);
        Ok(())
    }

    #[test]
    fn the_probe_rows_stay_on_the_lanes_for_as_long_as_its_clock_is_at_zero() -> Result<(), Failure>
    {
        let shown = view(&model()?, LaneRows { upper: 2, lower: 1 }, 0);
        assert_eq!(shown.upper.len() + shown.lower.len(), 3);
        assert!(
            shown
                .upper
                .iter()
                .all(|line| line.tr.is_some() || line.lang == "ja")
        );
        Ok(())
    }

    #[test]
    fn the_script_waits_for_the_fonts_and_sends_the_keys_of_the_answer() {
        let text = script();
        assert!(text.contains("await document.fonts.ready"));
        for key in ["inner_height", "rows", "lanes", "fonts", "missing_glyphs"] {
            assert!(text.contains(key), "{key}");
        }
        assert!(text.contains("金曜日"));
    }

    #[test]
    fn the_glyph_count_compares_the_stack_with_and_without_the_face_and_never_asks_fonts_check() {
        let text = script();
        assert!(text.contains("measureText"));
        assert!(text.contains(r#"!== "cinecaption""#));
        assert!(text.contains("#lane-upper .line"));
        assert!(!text.contains("fonts.check"));
    }

    #[test]
    fn an_answer_of_the_page_reads_into_a_measure() -> Result<(), Failure> {
        let answer = r#"{"inner_height":157.8,"rows":[{"id":"mic-1","scroll_height":39,"client_height":39,"bottom":147,"font_family":"\"UDEV Gothic 35\", sans-serif"}],"lanes":[{"lane":"lower","bottom":147}],"fonts":[{"family":"\"UDEV Gothic 35\"","status":"loaded"}],"missing_glyphs":2}"#;
        let measure: Measure = serde_json::from_str(answer)?;
        assert_eq!(measure.rows[0].id, "mic-1");
        assert_eq!(measure.missing_glyphs, 2);
        Ok(())
    }

    #[test]
    fn an_answer_without_its_rows_is_refused() {
        let answer = r#"{"inner_height":157.8,"lanes":[],"fonts":[],"missing_glyphs":0}"#;
        assert!(serde_json::from_str::<Measure>(answer).is_err());
    }

    #[test]
    fn the_event_carries_the_probe_arguments_and_every_measure() -> Result<(), Failure> {
        let measure = Measure {
            inner_height: 157.8,
            rows: vec![],
            lanes: vec![LaneBox {
                lane: "lower".to_owned(),
                bottom: 147.0,
            }],
            fonts: vec![],
            missing_glyphs: 3,
        };
        let probe = Probe {
            zoom_percent: 125,
            shrink_px: 12,
            click_through_unset: false,
        };
        let line = measured_event(&measure, probe)?.to_json_line();
        let value: serde_json::Value = serde_json::from_str(&line)?;
        assert_eq!(value["event"], "overlay.layout.measured");
        assert_eq!(value["zoom"], 125);
        assert_eq!(value["probe_shrink"], 12);
        assert_eq!(value["missing_glyphs"], 3);
        assert_eq!(value["lanes"][0]["lane"], "lower");
        assert_eq!(value["inner_height"], 157.8);
        Ok(())
    }
}

//! Runs written out by hand, the inputs of the fixture cases.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::scenes::{SINK, Scene};
use crate::support::{Failure, Output, Served};

pub fn log_line(event: &str, fields: &str) -> String {
    format!("{{\"event\":\"{event}\"{fields}}}\n")
}

pub fn scene_line(scene: &Scene) -> String {
    log_line(
        scene.event,
        &format!(
            ",\"stage\":\"{}\",\"reason\":\"{}\"",
            scene.stage, scene.reason
        ),
    )
}

pub fn scene_wire(scene: &Scene) -> Value {
    serde_json::json!({"type": "fallback", "stage": scene.stage, "reason": scene.reason, "to": scene.to})
}

pub fn final_wire() -> Value {
    serde_json::json!({"type": "final", "text": "x"})
}

pub fn served_of(code: i32, stdout: String, wire: Vec<Value>) -> Served {
    Served {
        output: Output {
            code: Some(code),
            stdout,
            stderr: String::new(),
        },
        wire,
    }
}

/// The run of a scene with the resource taken away, as it must look.
pub fn fell(scene: &Scene) -> Served {
    served_of(0, scene_line(scene), vec![final_wire(), scene_wire(scene)])
}

/// A run with every resource present.
pub fn whole(code: i32) -> Served {
    served_of(code, String::new(), vec![final_wire()])
}

/// Whether the check failed with a message that names `what`.
pub fn names(said: Result<(), Failure>, what: &str) -> bool {
    said.is_err_and(|text| text.contains(what))
}

pub fn settings(retained: &[(&str, u64)]) -> BTreeMap<String, Value> {
    retained
        .iter()
        .map(|(key, value)| ((*key).to_owned(), Value::from(*value)))
        .collect()
}

/// A sink that fell and then came back.
pub fn cleared_run() -> Served {
    let cleared = log_line("sink.fallback.cleared", ",\"stage\":\"sink\"");
    let wire = vec![
        scene_wire(&SINK),
        serde_json::json!({"type": "fallback", "stage": "sink", "to": null}),
    ];
    served_of(0, scene_line(&SINK) + &cleared, wire)
}

/// The start of a stream a client got when it connected with two fallbacks in effect.
pub fn active_pair() -> Vec<Value> {
    let punct = serde_json::json!({"type": "fallback", "stage": "punct", "to": "cpu"});
    let vad = serde_json::json!({"type": "fallback", "stage": "vad", "to": "disabled"});
    vec![punct, vad, final_wire()]
}

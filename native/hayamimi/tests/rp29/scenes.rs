//! The scenes of the table in design 3.15, as data the checks read.

use serde_json::Value;

/// One scene of the table in design 3.15: the log event, the stage and the
/// destination the screen is told, and the reason.
#[derive(Debug, Clone, Copy)]
pub struct Scene {
    pub name: &'static str,
    pub event: &'static str,
    pub stage: &'static str,
    pub reason: &'static str,
    pub to: &'static str,
}

pub const MODEL_SMALLER: Scene = Scene {
    name: "model",
    event: "model.fallback.smaller",
    stage: "recognizer",
    reason: "missing",
    to: "smaller",
};
pub const MODEL_DISABLED: Scene = Scene {
    name: "model",
    event: "model.fallback.disabled",
    stage: "punct",
    reason: "missing",
    to: "disabled",
};
pub const SPEAKER: Scene = Scene {
    name: "speaker",
    event: "model.fallback.disabled",
    stage: "speaker",
    reason: "missing",
    to: "disabled",
};
/// A recognizer tier that is not installed, read by the next tier that is.
pub const MODEL_SUBSTITUTE: Scene = Scene {
    name: "recognizer substitute",
    event: "model.fallback.substitute",
    stage: "recognizer",
    reason: "missing",
    to: "substitute",
};
/// The language identifier's model is not there: the language is fixed instead.
pub const LID_DISABLED: Scene = Scene {
    name: "lid",
    event: "model.fallback.disabled",
    stage: "lid",
    reason: "missing",
    to: "disabled",
};
/// The Japanese second opinion has no model: it is turned off.
pub const SECOND_OPINION_DISABLED: Scene = Scene {
    name: "second opinion",
    event: "model.fallback.disabled",
    stage: "second_opinion",
    reason: "missing",
    to: "disabled",
};
pub const LIBRARY: Scene = Scene {
    name: "library",
    event: "asr.fallback.disabled",
    stage: "asr",
    reason: "library_unavailable",
    to: "disabled",
};
pub const TRANSLATE: Scene = Scene {
    name: "translate",
    event: "translate.fallback.held",
    stage: "translate",
    reason: "unreachable",
    to: "held",
};
pub const SINK: Scene = Scene {
    name: "sink",
    event: "sink.fallback.local",
    stage: "sink",
    reason: "unreachable",
    to: "local",
};
pub const PORT: Scene = Scene {
    name: "port",
    event: "server.fallback.port",
    stage: "events",
    reason: "in_use",
    to: "next_port",
};
pub const INGEST_PORT: Scene = Scene {
    name: "ingest port",
    event: "server.fallback.port",
    stage: "ingest",
    reason: "in_use",
    to: "next_port",
};
pub const CAPTURE_SINGLE: Scene = Scene {
    name: "capture single",
    event: "capture.fallback.single",
    stage: "capture",
    reason: "device_missing",
    to: "single",
};
pub const CAPTURE_NONE: Scene = Scene {
    name: "capture none",
    event: "capture.fallback.none",
    stage: "capture",
    reason: "device_missing",
    to: "none",
};
pub const CONFIG: Scene = Scene {
    name: "config",
    event: "config.fallback.default",
    stage: "replace",
    reason: "unreadable",
    to: "default",
};

/// The ports after a blocked one that the server may move to.
pub const NEXT_PORTS: u16 = 10;

pub fn text_of<'a>(event: &'a Value, key: &str) -> Option<&'a str> {
    event.get(key).and_then(Value::as_str)
}

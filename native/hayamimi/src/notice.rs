//! A stage that did not run as designed, as one value with three outputs: a
//! log event, an `/events` message for the screen, and the lines the user
//! reads on stderr. All three come from the same fields, so they cannot
//! disagree about which stage fell back or why.

use hayamimi_asr::{Fallback, Reason, To};
use serde_json::{Value, json};

/// Every stage name a notice can carry.
pub const STAGES: [&str; 12] = [
    "punct",
    "recognizer",
    "vad",
    "asr",
    "events",
    "replace",
    "speaker",
    "ingest",
    "capture",
    "translate",
    "lid",
    "second_opinion",
];
/// Every reason a notice can carry.
pub const REASONS: [&str; 9] = [
    "no_device",
    "driver_unreadable",
    "session_failed",
    "missing",
    "unreadable",
    "library_unavailable",
    "in_use",
    "device_missing",
    "provider_mismatch",
];
/// Every destination a notice can carry.
pub const DESTINATIONS: [&str; 6] = ["cpu", "disabled", "next_port", "default", "single", "none"];

/// The log event of a stage that moved to the CPU.
pub const GPU_FALLBACK: &str = "gpu.fallback.cpu";
/// The log event of a stage left out because its model is not usable.
pub const MODEL_DISABLED: &str = "model.fallback.disabled";
/// The log event of recognition left out because the inference libraries did not open.
pub const ASR_DISABLED: &str = "asr.fallback.disabled";
/// The log event of the subtitle server on another port.
pub const SERVER_PORT: &str = "server.fallback.port";
/// The log event of a settings file that was not used.
pub const CONFIG_DEFAULT: &str = "config.fallback.default";
/// The log event of a run that has one audio device of the two it asked for.
pub const CAPTURE_SINGLE: &str = "capture.fallback.single";
/// The log event of a run that has none of the audio devices it asked for.
pub const CAPTURE_NONE: &str = "capture.fallback.none";
/// The log event of an audio device that came back.
pub const CAPTURE_CLEARED: &str = "capture.fallback.cleared";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    /// The log event name, `<domain>.fallback.<destination>`.
    pub event: &'static str,
    pub error_code: &'static str,
    pub stage: &'static str,
    pub reason: &'static str,
    pub to: &'static str,
    /// Where the subtitle server listens now, for a port notice.
    pub url: Option<String>,
}

impl Notice {
    /// A stage the engine reported as moved to the CPU or left out.
    pub fn from_fallback(fallback: &Fallback) -> Notice {
        let (event, error_code) = match fallback.to {
            To::Cpu => (GPU_FALLBACK, "HAYAMIMI_GPU_FALLBACK"),
            To::Disabled => (MODEL_DISABLED, "HAYAMIMI_MODEL_FALLBACK"),
        };
        Notice {
            event,
            error_code,
            stage: fallback.stage.name(),
            reason: fallback.reason.name(),
            to: fallback.to.name(),
            url: None,
        }
    }

    /// The translation server runs on the CPU, with no layer on the GPU.
    pub fn translate_on_cpu(reason: Reason) -> Notice {
        Notice {
            event: GPU_FALLBACK,
            error_code: "HAYAMIMI_GPU_FALLBACK",
            stage: "translate",
            reason: reason.name(),
            to: "cpu",
            url: None,
        }
    }

    /// The inference libraries did not open, so there is no recognition.
    pub fn library_unavailable() -> Notice {
        Notice {
            event: ASR_DISABLED,
            error_code: "HAYAMIMI_ASR_FALLBACK",
            stage: "asr",
            reason: "library_unavailable",
            to: "disabled",
            url: None,
        }
    }

    /// The requested port was taken: the server is on the port of `url`, or,
    /// when none of the ports tried was free, is not running.
    pub fn port_in_use(url: Option<String>) -> Notice {
        Notice::port_taken("events", url)
    }

    /// The same for the port of `/ingest`.
    pub fn ingest_port_in_use(url: Option<String>) -> Notice {
        Notice::port_taken("ingest", url)
    }

    fn port_taken(stage: &'static str, url: Option<String>) -> Notice {
        let to = if url.is_some() {
            "next_port"
        } else {
            "disabled"
        };
        Notice {
            event: SERVER_PORT,
            error_code: "HAYAMIMI_SERVER_FALLBACK",
            stage,
            reason: "in_use",
            to,
            url,
        }
    }

    /// One of the two audio devices the input asked for is missing; the other one carries on.
    pub fn capture_single() -> Notice {
        Notice::capture(CAPTURE_SINGLE, "single")
    }

    /// No audio device the input asked for is usable; the run waits for `/ingest` and the wav file.
    pub fn capture_none() -> Notice {
        Notice::capture(CAPTURE_NONE, "none")
    }

    fn capture(event: &'static str, to: &'static str) -> Notice {
        Notice {
            event,
            error_code: "HAYAMIMI_CAPTURE_FALLBACK",
            stage: "capture",
            reason: "device_missing",
            to,
            url: None,
        }
    }

    /// The replacement dictionary could not be read; the run uses none.
    pub fn config_unreadable() -> Notice {
        Notice {
            event: CONFIG_DEFAULT,
            error_code: "HAYAMIMI_CONFIG_FALLBACK",
            stage: "replace",
            reason: "unreadable",
            to: "default",
            url: None,
        }
    }

    /// The domain of the log event, the part before the first dot.
    pub fn domain(&self) -> &'static str {
        match self.event.split_once('.') {
            Some((domain, _)) => domain,
            None => self.event,
        }
    }

    /// Whether the cause is the GPU, for which the user is told how to check it.
    pub fn is_gpu(&self) -> bool {
        self.event == GPU_FALLBACK
    }

    /// The message `/events` sends to the screen.
    pub fn wire(&self) -> Value {
        json!({
            "type": "fallback",
            "stage": self.stage,
            "reason": self.reason,
            "to": self.to,
        })
    }

    /// The keys of the `[cli]` table whose texts, in this order, tell the user.
    pub fn stderr_keys(&self) -> [String; 3] {
        [
            format!("stage_{}", self.stage),
            format!("reason_{}", self.reason),
            format!("to_{}", self.to),
        ]
    }
}

/// A resource that came back, so that the stage returns to its first road.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cleared {
    /// The log event, `<domain>.fallback.cleared`.
    pub event: &'static str,
    pub stage: &'static str,
}

impl Cleared {
    /// An audio device is back.
    pub fn capture() -> Cleared {
        Cleared {
            event: CAPTURE_CLEARED,
            stage: "capture",
        }
    }

    /// The domain of the log event, the part before the first dot.
    pub fn domain(&self) -> &'static str {
        match self.event.split_once('.') {
            Some((domain, _)) => domain,
            None => self.event,
        }
    }

    /// The message `/events` sends to the screen: the stage is no longer fallen back.
    pub fn wire(&self) -> Value {
        json!({"type": "fallback", "stage": self.stage, "to": null})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hayamimi_asr::Stage;

    #[test]
    fn a_missing_device_is_a_capture_notice_with_the_event_of_its_destination() {
        let single = Notice::capture_single();
        let none = Notice::capture_none();
        assert_eq!(
            (single.event, single.to),
            ("capture.fallback.single", "single")
        );
        assert_eq!((none.event, none.to), ("capture.fallback.none", "none"));
        for notice in [&single, &none] {
            assert_eq!(notice.domain(), "capture");
            assert_eq!((notice.stage, notice.reason), ("capture", "device_missing"));
            assert!(!notice.is_gpu());
        }
    }

    #[test]
    fn the_ingest_port_notice_names_its_own_stage() {
        let moved = Notice::ingest_port_in_use(Some(String::from("ws://127.0.0.1:8767/ingest")));
        assert_eq!((moved.stage, moved.to), ("ingest", "next_port"));
        assert_eq!(Notice::ingest_port_in_use(None).to, "disabled");
        assert_eq!(Notice::port_in_use(None).stage, "events");
    }

    #[test]
    fn a_cleared_device_tells_the_screen_with_a_null_destination() {
        let cleared = Cleared::capture();
        assert_eq!(cleared.domain(), "capture");
        let wire = cleared.wire();
        assert_eq!(wire["type"], "fallback");
        assert_eq!(wire["stage"], "capture");
        assert!(wire["to"].is_null());
        assert!(wire.get("to").is_some(), "the key is there, with null");
    }

    fn punct_on_cpu() -> Notice {
        Notice::from_fallback(&Fallback {
            stage: Stage::Punct,
            reason: Reason::NoDevice,
            to: To::Cpu,
        })
    }

    #[test]
    fn a_stage_on_the_cpu_is_a_gpu_fallback_with_its_stage_and_reason() {
        let notice = punct_on_cpu();
        assert_eq!(notice.event, GPU_FALLBACK);
        assert_eq!(notice.domain(), "gpu");
        assert!(notice.is_gpu());
        assert_eq!(
            (notice.stage, notice.reason, notice.to),
            ("punct", "no_device", "cpu")
        );
    }

    #[test]
    fn the_translation_server_on_the_cpu_is_a_gpu_fallback_of_the_translate_stage() {
        let notice = Notice::translate_on_cpu(Reason::SessionFailed);
        assert_eq!(notice.event, GPU_FALLBACK);
        assert!(notice.is_gpu());
        assert_eq!(
            (notice.stage, notice.reason, notice.to),
            ("translate", "session_failed", "cpu")
        );
        let wire = notice.wire();
        assert_eq!(
            (wire["stage"].as_str(), wire["to"].as_str()),
            (Some("translate"), Some("cpu"))
        );
    }

    #[test]
    fn a_stage_left_out_is_a_model_fallback() {
        let notice = Notice::from_fallback(&Fallback {
            stage: Stage::Recognizer,
            reason: Reason::Missing,
            to: To::Disabled,
        });
        assert_eq!(notice.event, MODEL_DISABLED);
        assert_eq!(notice.domain(), "model");
        assert!(!notice.is_gpu());
    }

    #[test]
    fn the_identifier_on_the_cpu_and_the_second_opinion_left_out_are_named_notices() {
        let moved = Notice::from_fallback(&Fallback {
            stage: Stage::Lid,
            reason: Reason::ProviderMismatch,
            to: To::Cpu,
        });
        assert_eq!(
            (moved.event, moved.stage, moved.reason, moved.to),
            (GPU_FALLBACK, "lid", "provider_mismatch", "cpu")
        );
        let left = Notice::from_fallback(&Fallback {
            stage: Stage::SecondOpinion,
            reason: Reason::Missing,
            to: To::Disabled,
        });
        assert_eq!(
            (left.event, left.stage, left.reason, left.to),
            (MODEL_DISABLED, "second_opinion", "missing", "disabled")
        );
        let no_model = Notice::from_fallback(&Fallback {
            stage: Stage::Lid,
            reason: Reason::Missing,
            to: To::Disabled,
        });
        assert_eq!(
            (no_model.event, no_model.stage, no_model.to),
            (MODEL_DISABLED, "lid", "disabled")
        );
        for notice in [moved, left, no_model] {
            assert!(STAGES.contains(&notice.stage), "{}", notice.stage);
            assert!(REASONS.contains(&notice.reason), "{}", notice.reason);
        }
    }

    #[test]
    fn the_wire_form_has_the_four_keys_of_the_design() {
        let wire = punct_on_cpu().wire();
        assert_eq!(wire["type"], "fallback");
        assert_eq!(wire["stage"], "punct");
        assert_eq!(wire["reason"], "no_device");
        assert_eq!(wire["to"], "cpu");
    }

    #[test]
    fn a_port_notice_goes_to_the_next_port_or_to_nothing() {
        let moved = Notice::port_in_use(Some(String::from("http://127.0.0.1:8834/events")));
        assert_eq!(moved.to, "next_port");
        assert_eq!(Notice::port_in_use(None).to, "disabled");
    }

    #[test]
    fn every_name_a_notice_can_carry_is_in_the_lists_the_tables_are_checked_against() {
        let notices = [
            punct_on_cpu(),
            Notice::library_unavailable(),
            Notice::port_in_use(None),
            Notice::port_in_use(Some(String::new())),
            Notice::config_unreadable(),
            Notice::translate_on_cpu(Reason::NoDevice),
            Notice::translate_on_cpu(Reason::DriverUnreadable),
            Notice::translate_on_cpu(Reason::SessionFailed),
        ];
        for notice in notices {
            assert!(STAGES.contains(&notice.stage), "{}", notice.stage);
            assert!(REASONS.contains(&notice.reason), "{}", notice.reason);
            assert!(DESTINATIONS.contains(&notice.to), "{}", notice.to);
        }
    }

    #[test]
    fn the_stderr_keys_follow_the_fields() {
        assert_eq!(
            punct_on_cpu().stderr_keys(),
            ["stage_punct", "reason_no_device", "to_cpu"]
        );
    }
}

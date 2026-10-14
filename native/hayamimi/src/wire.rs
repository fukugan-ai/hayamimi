//! The events of the subtitle stream, in the form the existing server sent.
//! Keys and value types are the contract with the pages and the apps that
//! read `/events`; they are not translated.

use hayamimi_asr::Final;
use serde_json::{Value, json};

/// A draft of the utterance in progress.
pub fn partial_event(text: &str) -> Value {
    json!({"type": "partial", "text": text})
}

/// A finished utterance.
pub fn final_event(done: &Final) -> Value {
    let t = &done.timings;
    json!({
        "type": "final",
        "text": done.text,
        "lang": "ja",
        "speaker": "",
        "latency_ms": t.l_dec_ms,
        "tier": "rz",
        "audio_s": done.audio_seconds,
        "lid_ms": t.lid_ms,
        "decode_ms": t.decode_ms + t.itn_ms + t.punct_ms,
        "switched": false,
    })
}

/// What a session made, sent when it ends and on a reset. The refine
/// counters and the speaker table belong to routes this build does not have;
/// they keep their place on the wire as zeros and null.
pub fn session_summary(
    total_audio_s: f64,
    segments: u64,
    mean_latency_ms: Option<f64>,
    max_latency_ms: Option<f64>,
) -> Value {
    json!({
        "type": "session_summary",
        "stats": {
            "total_audio_s": total_audio_s,
            "segments": segments,
            "mean_latency_ms": mean_latency_ms,
            "max_latency_ms": max_latency_ms,
            "refine_lang_corrections": 0,
            "refine_groups_closed": 0,
            "refine_lang_boundary_flushes": 0,
        },
        "speakers": null,
    })
}

/// A new session has begun.
pub fn session_reset() -> Value {
    json!({"type": "session_reset"})
}

/// The reply of `/ingest` once the client's config is accepted.
pub fn ready_event(sample_rate: u32) -> Value {
    json!({"type": "ready", "sr": sample_rate})
}

/// The reply of `/ingest` to a client it refuses.
pub fn error_event(message: &str) -> Value {
    json!({"type": "error", "message": message})
}

#[cfg(test)]
mod tests {
    use super::*;
    use hayamimi_asr::Timings;

    fn finished() -> Final {
        Final {
            id: 1,
            text: String::from("テスト"),
            audio_seconds: 1.5,
            timings: Timings {
                vad_ms: 1.0,
                lid_ms: 0.0,
                decode_ms: 100.0,
                punct_ms: 10.0,
                itn_ms: 1.0,
                l_dec_ms: 120.0,
            },
            punct_error: None,
        }
    }

    #[test]
    fn the_final_event_has_the_keys_of_the_existing_server() {
        let event = final_event(&finished());
        for key in [
            "type",
            "text",
            "lang",
            "speaker",
            "latency_ms",
            "tier",
            "audio_s",
            "lid_ms",
            "decode_ms",
            "switched",
        ] {
            assert!(event.get(key).is_some(), "{key} is missing");
        }
        assert_eq!(event["type"], "final");
        assert_eq!(event["latency_ms"], 120.0);
        assert_eq!(event["decode_ms"], 111.0);
    }

    #[test]
    fn a_partial_carries_only_its_type_and_text() {
        let event = partial_event("こんに");
        assert_eq!(event, json!({"type": "partial", "text": "こんに"}));
    }

    #[test]
    fn the_summary_has_the_seven_stats_and_a_null_speaker_table() {
        let event = session_summary(12.5, 3, Some(200.0), Some(310.0));
        let stats = &event["stats"];
        for key in [
            "total_audio_s",
            "segments",
            "mean_latency_ms",
            "max_latency_ms",
            "refine_lang_corrections",
            "refine_groups_closed",
            "refine_lang_boundary_flushes",
        ] {
            assert!(stats.get(key).is_some(), "{key} is missing");
        }
        assert!(event["speakers"].is_null());
        assert_eq!(stats["segments"], 3);
    }

    #[test]
    fn a_session_without_segments_reports_null_latencies() {
        let event = session_summary(0.0, 0, None, None);
        assert!(event["stats"]["mean_latency_ms"].is_null());
        assert!(event["stats"]["max_latency_ms"].is_null());
    }

    #[test]
    fn the_ingest_replies_have_the_keys_of_the_existing_server() {
        assert_eq!(ready_event(16_000), json!({"type": "ready", "sr": 16_000}));
        assert_eq!(error_event("busy")["type"], "error");
        assert_eq!(session_reset()["type"], "session_reset");
    }
}

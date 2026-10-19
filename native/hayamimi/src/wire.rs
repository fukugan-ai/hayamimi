//! The events of the subtitle stream, in the form the existing server sent.
//! Keys and value types are the contract with the pages and the apps that
//! read `/events`; they are not translated.

use hayamimi_asr::Final;
use serde_json::{Value, json};

/// A draft of the utterance in progress.
pub fn partial_event(text: &str) -> Value {
    json!({"type": "partial", "text": text})
}

/// A finished utterance. `id`, `source`, `t0_s`, `t1_s` and `end_wall_ms` let
/// the relay join a translation to this line and measure it.
pub fn final_event(done: &Final, source: &str) -> Value {
    let t = &done.timings;
    json!({
        "type": "final",
        "id": format!("{source}-{}", done.id),
        "source": source,
        "t0_s": done.t0_s,
        "t1_s": done.t1_s,
        "end_wall_ms": done.end_wall_ms,
        "text": done.text,
        "lang": done.lang,
        // The mark follows in a `speaker` event of the same id; the final does not wait for it.
        "speaker": "",
        "latency_ms": t.l_dec_ms,
        "tier": done.tier,
        "audio_s": done.audio_seconds,
        "lid_ms": t.lid_ms,
        "decode_ms": t.decode_ms + t.itn_ms + t.punct_ms,
        "switched": false,
    })
}

/// The speaker of an utterance already sent as a `final`, joined to it by `id`.
pub fn speaker_event(done: &Final, source: &str, label: &str) -> Value {
    json!({
        "type": "speaker",
        "id": format!("{source}-{}", done.id),
        "speaker": label,
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
            lang: String::from("ja"),
            tier: "rz",
            audio_seconds: 1.5,
            t0_s: 0.25,
            t1_s: 1.75,
            end_wall_ms: 1_790_000_000_123,
            timings: Timings {
                vad_ms: 1.0,
                lid_ms: 0.0,
                decode_ms: 100.0,
                punct_ms: 10.0,
                itn_ms: 1.0,
                l_dec_ms: 120.0,
            },
            punct_error: None,
            speaker_audio: Vec::new(),
        }
    }

    #[test]
    fn the_final_leaves_the_speaker_empty_and_the_speaker_event_names_the_same_id() {
        let done = finished();
        assert_eq!(final_event(&done, "file")["speaker"], "");
        let event = speaker_event(&done, "file", "S2");
        assert_eq!(event["type"], "speaker");
        assert_eq!(event["id"], final_event(&done, "file")["id"]);
        assert_eq!(event["speaker"], "S2");
    }

    #[test]
    fn the_final_event_has_the_keys_of_the_existing_server() {
        let event = final_event(&finished(), "file");
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
    fn the_final_event_carries_the_five_items_the_relay_joins_on() {
        let event = final_event(&finished(), "mic");
        assert_eq!(event["id"], "mic-1");
        assert_eq!(event["source"], "mic");
        assert_eq!(event["t0_s"], 0.25);
        assert_eq!(event["t1_s"], 1.75);
        assert_eq!(event["end_wall_ms"], 1_790_000_000_123_u64);
    }

    #[test]
    fn final_event_carries_audio_s_lid_ms_decode_ms() {
        let mut done = finished();
        done.text = String::from("hello in ja");
        done.audio_seconds = 1.0;
        done.timings.lid_ms = 12.0;
        done.timings.decode_ms = 34.0;
        done.timings.punct_ms = 0.0;
        done.timings.itn_ms = 0.0;
        let event = final_event(&done, "mic");
        assert_eq!(event["text"], "hello in ja");
        assert_eq!(event["lang"], "ja");
        assert_eq!(event["tier"], "rz");
        assert_eq!(event["audio_s"], 1.0);
        assert_eq!(event["lid_ms"], 12.0);
        assert_eq!(event["decode_ms"], 34.0);
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

//! Structured log events (`observability-log/v1`, one JSON object per line)
//! on stdout. Subtitle text, audio and file paths never appear in an event;
//! an event carries names, counts and times.

use std::io::Write;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use hayamimi_asr::audio::SAMPLE_RATE;
use hayamimi_asr::endpoint::Rule;
use hayamimi_asr::partial::{Midway, MidwayAsked};
use hayamimi_asr::pipeline::{ClosedSegment, PunctWarmup, Unread};
use hayamimi_asr::route::FallbackStep;
use hayamimi_asr::vad::EARLY_MIN_SEGMENT_MS;
use hayamimi_asr::{EarlyHold, Final, Reason, ResidentEvent, SessionReport, Warmup};
use hayamimi_relay::logging::{Level, LogEvent};
use serde_json::Value;

use crate::notice::{Cleared, Notice};
use crate::strings::Strings;

/// Identifies the running build in every line.
pub const BUILD_REVISION: &str = concat!("hayamimi-", env!("CARGO_PKG_VERSION"));
/// The error code of a start without a usable CUDA device.
pub const GPU_UNAVAILABLE_CODE: &str = "HAYAMIMI_GPU_DEVICE_UNAVAILABLE";

pub const SESSION_CREATED: &str = "gpu.session.created";
pub const SESSION_PLACED: &str = "gpu.session.placed";
pub const DEVICE_UNAVAILABLE: &str = "gpu.device.unavailable";
pub const UTTERANCE_TIMED: &str = "asr.utterance.timed";
pub const ENDPOINT_DECIDED: &str = "asr.endpoint.decided";
pub const ENDPOINT_HELD: &str = "asr.endpoint.held";
pub const PUNCT_FAILED: &str = "asr.punct.failed";
pub const SPEAKER_FAILED: &str = "asr.speaker.failed";
pub const FINAL_PUBLISHED: &str = "asr.final.published";
pub const FEED_WAITED: &str = "asr.feed.waited";
pub const VAD_CLOSED: &str = "asr.vad.closed";
pub const EMBEDDING_STARTED: &str = "speaker.embedding.started";
pub const ENGINE_FAILED: &str = "asr.engine.failed";
pub const MODEL_LOADED: &str = "asr.model.loaded";
pub const MODEL_EVICTED: &str = "asr.model.evicted";
pub const MEMORY_UNREADABLE: &str = "gpu.memory.unreadable";
pub const LANG_UNKNOWN: &str = "asr.lang.unknown";
pub const LANG_FALLBACK: &str = "asr.lang.fallback";
pub const SEGMENT_UNREAD: &str = "asr.segment.unread";
pub const WARMUP_DECODED: &str = "asr.warmup.decoded";
pub const WARMUP_FAILED: &str = "asr.warmup.failed";
pub const PARTIAL_FAILED: &str = "asr.partial.failed";
pub const PARTIAL_LANG_ASKED: &str = "asr.partial.lang.asked";
pub const DEVICE_OPENED: &str = "capture.device.opened";
pub const INGEST_CONNECTED: &str = "ingest.client.connected";
pub const INGEST_CLOSED: &str = "ingest.client.closed";
pub const INGEST_REJECTED: &str = "ingest.client.rejected";
pub const SERVER_LAN_OPENED: &str = "server.lan.opened";

/// A time in milliseconds to three decimals.
fn millis(value: f64) -> Value {
    Value::from((value * 1000.0).round() / 1000.0)
}

/// A time in seconds to three decimals.
fn seconds(value: f64) -> Value {
    Value::from((value * 1000.0).round() / 1000.0)
}

/// The one-line JSON form of an event.
pub fn event_line(
    strings: &Strings,
    level: Level,
    event: &str,
    component: &'static str,
    error_code: Option<&str>,
    fields: &[(&str, Value)],
) -> String {
    let mut built = LogEvent::new(level, event, component);
    built.build_revision = BUILD_REVISION;
    built.message = strings.message_for(event).to_owned();
    built.error_code = error_code.map(str::to_owned);
    for (key, value) in fields {
        built = built.with(key, value.clone());
    }
    built.to_json_line()
}

/// `gpu.session.created`: what a model session was asked for and what ran.
pub fn session_created_line(strings: &Strings, report: &SessionReport) -> String {
    event_line(
        strings,
        Level::Info,
        SESSION_CREATED,
        "gpu",
        None,
        &[
            ("model", Value::from(report.model.as_str())),
            ("requested_provider", Value::from(report.requested)),
            ("actual_provider", Value::from(report.actual)),
        ],
    )
}

/// `gpu.session.placed`: the node counts of a session, when they were measured.
pub fn session_placed_line(strings: &Strings, report: &SessionReport) -> Option<String> {
    let nodes = report.nodes?;
    Some(event_line(
        strings,
        Level::Info,
        SESSION_PLACED,
        "gpu",
        None,
        &[
            ("model", Value::from(report.model.as_str())),
            ("cuda_nodes", Value::from(nodes.cuda)),
            ("cpu_nodes", Value::from(nodes.cpu)),
        ],
    ))
}

/// `gpu.device.unavailable`: the start found no usable CUDA device. The run goes on.
pub fn device_unavailable_line(strings: &Strings, why: Reason) -> String {
    event_line(
        strings,
        Level::Warn,
        DEVICE_UNAVAILABLE,
        "gpu",
        Some(GPU_UNAVAILABLE_CODE),
        &[("reason", Value::from(why.name()))],
    )
}

/// `<domain>.fallback.<destination>`: one stage that did not run as designed.
pub fn fallback_line(strings: &Strings, notice: &Notice) -> String {
    let mut fields = vec![
        ("stage", Value::from(notice.stage)),
        ("reason", Value::from(notice.reason)),
    ];
    if let Some(url) = &notice.url {
        fields.push(("url", Value::from(url.as_str())));
    }
    if let Some((requested, used)) = notice.tiers {
        fields.push(("requested", Value::from(requested)));
        fields.push(("used", Value::from(used)));
    }
    event_line(
        strings,
        Level::Warn,
        notice.event,
        notice.domain(),
        Some(notice.error_code),
        &fields,
    )
}

/// `<domain>.fallback.cleared`: a resource came back and the stage returned to its first road.
pub fn cleared_line(strings: &Strings, cleared: &Cleared) -> String {
    event_line(
        strings,
        Level::Info,
        cleared.event,
        cleared.domain(),
        None,
        &[("stage", Value::from(cleared.stage))],
    )
}

/// `capture.device.opened`: which audio device a capture opened. A device name is data, not text of a table.
pub fn device_opened_line(strings: &Strings, kind: &str, device: &str) -> String {
    event_line(
        strings,
        Level::Info,
        DEVICE_OPENED,
        "capture",
        None,
        &[("kind", Value::from(kind)), ("device", Value::from(device))],
    )
}

/// `ingest.client.connected`: a client sent its config and audio is now taken from it.
pub fn ingest_connected_line(strings: &Strings, sample_rate: u32, channels: usize) -> String {
    event_line(
        strings,
        Level::Info,
        INGEST_CONNECTED,
        "transport",
        None,
        &[
            ("sample_rate", Value::from(sample_rate)),
            ("channels", Value::from(channels)),
        ],
    )
}

/// `ingest.client.closed`: the audio client left; `windows` is how many windows of audio it sent.
pub fn ingest_closed_line(strings: &Strings, windows: u64) -> String {
    event_line(
        strings,
        Level::Info,
        INGEST_CLOSED,
        "transport",
        None,
        &[("windows", Value::from(windows))],
    )
}

/// `ingest.client.rejected`: a second audio client was refused while one was streaming.
pub fn ingest_rejected_line(strings: &Strings) -> String {
    event_line(
        strings,
        Level::Warn,
        INGEST_REJECTED,
        "transport",
        Some("HAYAMIMI_TRANSPORT_BUSY"),
        &[],
    )
}

/// `server.lan.opened`: the subtitle server listens on an address other than the loopback, so
/// devices on the network can reach it. The page, `/config` and `/reset` have no authentication.
pub fn lan_opened_line(strings: &Strings, port: u16) -> String {
    event_line(
        strings,
        Level::Warn,
        SERVER_LAN_OPENED,
        "transport",
        None,
        &[("port", Value::from(port))],
    )
}

/// `asr.utterance.timed`: where the time of one utterance went.
pub fn utterance_timed_line(strings: &Strings, done: &Final) -> String {
    let t = &done.timings;
    event_line(
        strings,
        Level::Info,
        UTTERANCE_TIMED,
        "asr",
        None,
        &[
            ("id", Value::from(done.id)),
            ("vad_ms", millis(t.vad_ms)),
            ("lid_ms", millis(t.lid_ms)),
            ("decode_ms", millis(t.decode_ms)),
            ("punct_ms", millis(t.punct_ms)),
            ("itn_ms", millis(t.itn_ms)),
            ("l_dec_ms", millis(t.l_dec_ms)),
        ],
    )
}

/// `asr.endpoint.decided`: which rule ended one utterance, and the silence it waited for.
/// `plan` is how the main detector's segment became text (`held`: the sentence
/// the early detector kept was used; `decode`: the whole segment was read;
/// `rest`: what follows the early line was read). `held_gap_ms` is present
/// when a kept sentence existed but the segment was read again: how far the
/// ends of the two were apart. Both are absent for a sentence the early
/// detector finalised itself.
pub fn endpoint_decided_line(strings: &Strings, done: &Final) -> String {
    let endpoint = &done.endpoint;
    let mut fields = vec![
        ("id", Value::from(done.id)),
        ("rule", Value::from(endpoint.rule.name())),
        ("silence_ms", Value::from(endpoint.silence_ms)),
    ];
    if let Some(plan) = endpoint.plan {
        fields.push(("plan", Value::from(plan.name())));
    }
    if let Some(gap) = endpoint.held_gap_ms {
        fields.push(("held_gap_ms", Value::from(gap)));
    }
    event_line(strings, Level::Info, ENDPOINT_DECIDED, "asr", None, &fields)
}

/// `asr.lang.fallback`: the language of one utterance was not the identifier's
/// answer (`--mode pair`). `id` is the `final`'s, `step` is the answer that chose
/// the language (`sv_tag`, `script`, `default`, `sv_failed`) and `lid` is what the
/// identifier answered, or null when it answered nothing. `None` for a `final`
/// that did not fall back. A failed SenseVoice reading (`sv_failed`) is a warning.
pub fn lang_fallback_line(strings: &Strings, done: &Final) -> Option<String> {
    let fallback = done.fallback.as_ref()?;
    let level = if fallback.step == FallbackStep::SvFailed {
        Level::Warn
    } else {
        Level::Info
    };
    let lid = fallback.lid.as_deref().map_or(Value::Null, Value::from);
    Some(event_line(
        strings,
        level,
        LANG_FALLBACK,
        "asr",
        None,
        &[
            ("id", Value::from(done.id)),
            ("step", Value::from(fallback.step.name())),
            ("lid", lid),
        ],
    ))
}

/// `asr.segment.unread`: a segment of the main detector was read and made no
/// `final`. `start_s` and `end_s` are the span on the stream, the clock of a
/// `final`'s `t0_s` and `t1_s`; `reason` is `empty_decode` (the recognizer
/// answered with no text) or `empty_text` (the text was gone after ITN,
/// punctuation and the dictionary); `lang` and `tier` are what the recognizer
/// was asked for. The text is not written. It is `info`: an answer of no text
/// is common on near-silence.
pub fn segment_unread_line(strings: &Strings, unread: &Unread) -> String {
    event_line(
        strings,
        Level::Info,
        SEGMENT_UNREAD,
        "asr",
        None,
        &[
            ("start_s", seconds(unread.start_s)),
            ("end_s", seconds(unread.end_s)),
            ("reason", Value::from(unread.reason.name())),
            ("lang", Value::from(unread.lang.as_str())),
            ("tier", Value::from(unread.tier)),
        ],
    )
}

/// `asr.endpoint.held`: an early notice read a sentence from `hold.span_ms`
/// of speech and left the end to the main detector's silence, because the
/// sentence was not complete (`reason` is `incomplete`) or the speech was
/// shorter than `min_ms` (`too_short`). `tail` is the kind of form the sentence
/// ended in, and `shadow_complete` is whether a rule that counts a noun or an
/// honorific ending as a sentence end would have finalised it there. The
/// sentence is not written.
pub fn endpoint_held_line(strings: &Strings, hold: &EarlyHold) -> String {
    event_line(
        strings,
        Level::Info,
        ENDPOINT_HELD,
        "asr",
        None,
        &[
            ("reason", Value::from(hold.reason.name())),
            ("span_ms", millis(hold.span_ms)),
            ("min_ms", Value::from(EARLY_MIN_SEGMENT_MS)),
            ("tail", Value::from(hold.tail.name())),
            ("shadow_complete", Value::from(hold.shadow_complete)),
        ],
    )
}

/// `asr.feed.waited`: the window whose feed made utterance `id` had waited
/// `wait` on the queue before the loop took it, with `backlog` windows still
/// queued behind it.
pub fn feed_waited_line(strings: &Strings, id: u64, wait: Duration, backlog: usize) -> String {
    event_line(
        strings,
        Level::Info,
        FEED_WAITED,
        "asr",
        None,
        &[
            ("id", Value::from(id)),
            ("wait_ms", millis(wait.as_secs_f64() * 1000.0)),
            ("backlog", Value::from(backlog)),
        ],
    )
}

/// How far behind the audio the voice detector was, in whole milliseconds:
/// `received` samples had come in while it had been fed `fed`. The detector
/// is fed a window only after the window was received, so `fed` is not above
/// `received`.
pub fn vad_lag_ms(received: u64, fed: u64) -> u64 {
    received.saturating_sub(fed) * 1000 / u64::from(SAMPLE_RATE)
}

/// `asr.vad.closed`: the voice detector closed one segment. `id` is the id of
/// the `final` the segment made (`<source>-<number>`, the one `/events`
/// carries), or null when the segment read as nothing. `lag_ms` is how far
/// behind the audio the detector was when it closed the segment
/// ([`vad_lag_ms`]), and `rule` is what ended it: `silence`, `early`,
/// `max_speech`, or `flush` for the speech a leaving client cut short.
pub fn vad_closed_line(
    strings: &Strings,
    id: Option<&str>,
    received: u64,
    fed: u64,
    rule: Rule,
) -> String {
    event_line(
        strings,
        Level::Info,
        VAD_CLOSED,
        "asr",
        None,
        &[
            ("id", id.map_or(Value::Null, Value::from)),
            ("lag_ms", Value::from(vad_lag_ms(received, fed))),
            ("rule", Value::from(rule.name())),
        ],
    )
}

/// One `asr.vad.closed` line for each segment `closed` names, all closed
/// while the detector had been fed `fed` of the `received` samples.
pub fn vad_closed_lines(
    strings: &Strings,
    source: &str,
    closed: &[ClosedSegment],
    (received, fed): (u64, u64),
) -> Vec<String> {
    closed
        .iter()
        .map(|segment| {
            let id = segment.id.map(|number| format!("{source}-{number}"));
            vad_closed_line(strings, id.as_deref(), received, fed, segment.rule)
        })
        .collect()
}

/// What the identifier came to, in the words of the log: `ja` or `en` when it
/// named one of the pair, `outside` for any other language, `failed` when it
/// did not answer, and `waiting` for a question not asked yet.
fn midway_answer_name(answer: Midway) -> &'static str {
    match answer {
        Midway::Heard(lang) => lang,
        Midway::Outside => "outside",
        Midway::Failed => "failed",
        Midway::Waiting => "waiting",
    }
}

/// `asr.partial.lang.asked`: the language of the utterance in progress was
/// asked of the identifier, once, so that its drafts are read in that language.
/// `answer` is what it came to ([`midway_answer_name`]), `voice_ms` the voice it
/// was given and `lid_ms` how long the question took.
pub fn midway_asked_line(strings: &Strings, asked: &MidwayAsked) -> String {
    let voice_ms = (asked.voice_samples as u64) * 1000 / u64::from(SAMPLE_RATE);
    event_line(
        strings,
        Level::Info,
        PARTIAL_LANG_ASKED,
        "asr",
        None,
        &[
            ("answer", Value::from(midway_answer_name(asked.answer))),
            ("voice_ms", Value::from(voice_ms)),
            ("lid_ms", millis(asked.lid_ms)),
        ],
    )
}

/// Microseconds on the process's monotonic clock, from the first call.
pub fn mono_us() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    // A process that has run for 584,000 years would wrap; none does.
    START.get_or_init(Instant::now).elapsed().as_micros() as u64
}

/// `asr.final.published` (debug): the `final` of utterance `id` has been sent, at `mono_us`.
pub fn final_published_line(strings: &Strings, id: u64, mono_us: u64) -> String {
    event_line(
        strings,
        Level::Debug,
        FINAL_PUBLISHED,
        "asr",
        None,
        &[("id", Value::from(id)), ("mono_us", Value::from(mono_us))],
    )
}

/// `speaker.embedding.started` (debug): the embedding of utterance `id` begins, at `mono_us`.
pub fn embedding_started_line(strings: &Strings, id: u64, mono_us: u64) -> String {
    event_line(
        strings,
        Level::Debug,
        EMBEDDING_STARTED,
        "asr",
        None,
        &[("id", Value::from(id)), ("mono_us", Value::from(mono_us))],
    )
}

/// `asr.speaker.failed`: the speaker mark failed for one utterance and the text went on without it.
pub fn speaker_failed_line(strings: &Strings, done: &Final) -> String {
    event_line(
        strings,
        Level::Warn,
        SPEAKER_FAILED,
        "asr",
        None,
        &[("id", Value::from(done.id))],
    )
}

/// `asr.punct.failed`: punctuation failed for one utterance and the text went on without it.
pub fn punct_failed_line(strings: &Strings, done: &Final) -> String {
    event_line(
        strings,
        Level::Warn,
        PUNCT_FAILED,
        "asr",
        None,
        &[("id", Value::from(done.id))],
    )
}

/// `asr.partial.failed`: a draft of the utterance in progress failed and was skipped.
pub fn partial_failed_line(strings: &Strings) -> String {
    event_line(
        strings,
        Level::Warn,
        PARTIAL_FAILED,
        "asr",
        Some("HAYAMIMI_ASR_PARTIAL_FAILED"),
        &[],
    )
}

/// `asr.warmup.decoded`: the recognizer read the start-up silence once, before
/// the first utterance. `decode_ms` is what that first decode took. `punct_ms`
/// is what the punctuation model's one read took; it is null when that read
/// did not run or failed.
pub fn warmup_decoded_line(strings: &Strings, warmed: &Warmup) -> String {
    event_line(
        strings,
        Level::Info,
        WARMUP_DECODED,
        "asr",
        None,
        &[
            ("model", Value::from(warmed.tier.name())),
            ("decode_ms", millis(warmed.decode_ms)),
            ("audio_s", Value::from(warmed.audio_s)),
            ("punct_ms", warmed.punct.ms().map_or(Value::Null, millis)),
        ],
    )
}

/// The `stage` of a start-up read that failed: the recognizer's decode.
const WARMUP_STAGE_DECODE: &str = "decode";
/// The `stage` of a start-up read that failed: the punctuation model's read.
const WARMUP_STAGE_PUNCT: &str = "punct";

/// `asr.warmup.failed` for the read of `stage`; the run goes on without it.
fn warmup_failed_at(strings: &Strings, stage: &str) -> String {
    event_line(
        strings,
        Level::Warn,
        WARMUP_FAILED,
        "asr",
        Some("HAYAMIMI_ASR_WARMUP_FAILED"),
        &[("stage", Value::from(stage))],
    )
}

/// `asr.warmup.failed` with `stage` `decode`: the start-up decode failed; the
/// run goes on without it.
pub fn warmup_failed_line(strings: &Strings) -> String {
    warmup_failed_at(strings, WARMUP_STAGE_DECODE)
}

/// `asr.warmup.failed` with `stage` `punct`: the punctuation model's start-up
/// read failed; the run goes on without it.
pub fn warmup_punct_failed_line(strings: &Strings) -> String {
    warmup_failed_at(strings, WARMUP_STAGE_PUNCT)
}

/// The lines the start-up reads say, in order: `decoded` when the decode
/// worked, then `failed` (stage `punct`) when the punctuation read failed; a
/// failed decode says `failed` (stage `decode`) alone.
pub fn warmup_lines<E>(strings: &Strings, read: &Result<Warmup, E>) -> Vec<String> {
    match read {
        Ok(warmed) if warmed.punct == PunctWarmup::Failed => Vec::from([
            warmup_decoded_line(strings, warmed),
            warmup_punct_failed_line(strings),
        ]),
        Ok(warmed) => Vec::from([warmup_decoded_line(strings, warmed)]),
        Err(_) => Vec::from([warmup_failed_line(strings)]),
    }
}

/// `asr.engine.failed`: the route stopped on an error.
pub fn engine_failed_line(strings: &Strings) -> String {
    event_line(
        strings,
        Level::Error,
        ENGINE_FAILED,
        "asr",
        Some("HAYAMIMI_ASR_ENGINE_FAILED"),
        &[],
    )
}

/// `asr.model.loaded` and `asr.model.evicted`: a recognizer came into memory
/// or was dropped to make room. The other events of the recognizers are
/// fallbacks and have a [`Notice`].
pub fn resident_line(strings: &Strings, event: &ResidentEvent) -> Option<String> {
    match event {
        ResidentEvent::Loaded { tier, ms } => Some(event_line(
            strings,
            Level::Info,
            MODEL_LOADED,
            "asr",
            None,
            &[
                ("model", Value::from(tier.name())),
                ("load_ms", millis(*ms)),
            ],
        )),
        ResidentEvent::Evicted { tier } => Some(event_line(
            strings,
            Level::Info,
            MODEL_EVICTED,
            "asr",
            None,
            &[("model", Value::from(tier.name()))],
        )),
        ResidentEvent::LangUnknown => Some(event_line(
            strings,
            Level::Warn,
            LANG_UNKNOWN,
            "asr",
            Some("HAYAMIMI_ASR_LANG_UNKNOWN"),
            &[],
        )),
        ResidentEvent::Substituted { .. } | ResidentEvent::CutIntoChunks { .. } => None,
    }
}

/// `gpu.memory.unreadable`: the memory of the GPU cannot be read, so the run has no budget.
pub fn memory_unreadable_line(strings: &Strings) -> String {
    event_line(
        strings,
        Level::Warn,
        MEMORY_UNREADABLE,
        "gpu",
        Some("HAYAMIMI_GPU_MEMORY_UNREADABLE"),
        &[],
    )
}

/// Writes one line to stdout.
pub fn print(line: &str) {
    // A closed stdout leaves nowhere to report to.
    let _ = writeln!(std::io::stdout().lock(), "{line}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notice;
    use hayamimi_asr::endpoint::{Hold, PlanKind, Rule, Tail};
    use hayamimi_asr::ort_runtime::NodeCounts;
    use hayamimi_asr::pipeline::{ClosedSegment, UnreadReason};
    use hayamimi_asr::route::LangFallback;
    use hayamimi_asr::{Tier, Timings};
    use hayamimi_relay::lang::Lang;

    fn strings() -> Result<Strings, String> {
        Strings::embedded(Lang::En)
    }

    fn parsed(line: &str) -> Result<Value, serde_json::Error> {
        serde_json::from_str(line)
    }

    fn report(nodes: Option<NodeCounts>) -> SessionReport {
        SessionReport {
            model: String::from("punct_bert.onnx"),
            requested: "cuda",
            actual: "cuda",
            nodes,
        }
    }

    fn finished() -> Final {
        Final {
            id: 3,
            text: String::from("本文"),
            lang: String::from("ja"),
            tier: "rz",
            audio_seconds: 2.0,
            t0_s: 0.5,
            t1_s: 2.0,
            end_wall_ms: 1_790_000_000_000,
            timings: Timings {
                vad_ms: 1.0,
                lid_ms: 0.0,
                decode_ms: 120.4567,
                punct_ms: 9.0,
                itn_ms: 0.01,
                l_dec_ms: 130.0,
            },
            punct_error: None,
            speaker_audio: Vec::new(),
            endpoint: hayamimi_asr::endpoint::Decision {
                rule: hayamimi_asr::endpoint::Rule::Silence,
                silence_ms: 300,
                plan: None,
                held_gap_ms: None,
            },
            fallback: None,
        }
    }

    fn finished_by(plan: Option<PlanKind>, held_gap_ms: Option<u32>) -> Final {
        let mut done = finished();
        done.endpoint.plan = plan;
        done.endpoint.held_gap_ms = held_gap_ms;
        done
    }

    #[test]
    fn the_decided_event_names_the_plan_and_the_gap_of_a_segment_read_again()
    -> Result<(), Box<dyn std::error::Error>> {
        let held = parsed(&endpoint_decided_line(
            &strings()?,
            &finished_by(Some(PlanKind::Held), None),
        ))?;
        assert_eq!(held["plan"], "held");
        assert!(held.get("held_gap_ms").is_none());
        let decode = parsed(&endpoint_decided_line(
            &strings()?,
            &finished_by(Some(PlanKind::Decode), Some(140)),
        ))?;
        assert_eq!(decode["plan"], "decode");
        assert_eq!(decode["held_gap_ms"], 140);
        let nothing_kept = parsed(&endpoint_decided_line(
            &strings()?,
            &finished_by(Some(PlanKind::Decode), None),
        ))?;
        assert_eq!(nothing_kept["plan"], "decode");
        assert!(nothing_kept.get("held_gap_ms").is_none());
        let early = parsed(&endpoint_decided_line(
            &strings()?,
            &finished_by(None, None),
        ))?;
        assert!(early.get("plan").is_none());
        Ok(())
    }

    #[test]
    fn a_created_session_names_the_model_and_both_providers()
    -> Result<(), Box<dyn std::error::Error>> {
        let line = session_created_line(&strings()?, &report(None));
        let event = parsed(&line)?;
        assert_eq!(event["event"], SESSION_CREATED);
        assert_eq!(event["model"], "punct_bert.onnx");
        assert_eq!(event["requested_provider"], "cuda");
        assert_eq!(event["actual_provider"], "cuda");
        assert_eq!(event["message"], "Model session created");
        assert_eq!(event["build_revision"], BUILD_REVISION);
        Ok(())
    }

    #[test]
    fn a_placed_event_needs_counts() -> Result<(), Box<dyn std::error::Error>> {
        assert!(session_placed_line(&strings()?, &report(None)).is_none());
        let counts = NodeCounts { cuda: 453, cpu: 77 };
        let Some(line) = session_placed_line(&strings()?, &report(Some(counts))) else {
            return Err("no line for a session with counts".into());
        };
        let event = parsed(&line)?;
        assert_eq!(event["cuda_nodes"], 453);
        assert_eq!(event["cpu_nodes"], 77);
        Ok(())
    }

    #[test]
    fn the_unavailable_event_is_a_warning_with_the_error_code_and_the_reason()
    -> Result<(), Box<dyn std::error::Error>> {
        let event = parsed(&device_unavailable_line(&strings()?, Reason::NoDevice))?;
        assert_eq!(event["event"], DEVICE_UNAVAILABLE);
        assert_eq!(event["level"], "warn");
        assert_eq!(event["error_code"], GPU_UNAVAILABLE_CODE);
        assert_eq!(event["reason"], "no_device");
        let driver = parsed(&device_unavailable_line(
            &strings()?,
            Reason::DriverUnreadable,
        ))?;
        assert_eq!(driver["reason"], "driver_unreadable");
        Ok(())
    }

    #[test]
    fn a_fallback_event_names_the_stage_the_reason_and_its_error_code()
    -> Result<(), Box<dyn std::error::Error>> {
        let event = parsed(&fallback_line(&strings()?, &Notice::config_unreadable()))?;
        assert_eq!(event["event"], "config.fallback.default");
        assert_eq!(event["level"], "warn");
        assert_eq!(event["error_code"], "HAYAMIMI_CONFIG_FALLBACK");
        assert_eq!(event["stage"], "replace");
        assert_eq!(event["reason"], "unreadable");
        assert!(event.get("url").is_none());
        let port = Notice::port_in_use(Some(String::from("http://127.0.0.1:8834/events")));
        let moved = parsed(&fallback_line(&strings()?, &port))?;
        assert_eq!(moved["url"], "http://127.0.0.1:8834/events");
        Ok(())
    }

    #[test]
    fn an_utterance_event_has_the_timing_fields_and_no_text()
    -> Result<(), Box<dyn std::error::Error>> {
        let line = utterance_timed_line(&strings()?, &finished());
        assert!(!line.contains("本文"));
        let event = parsed(&line)?;
        for key in [
            "id",
            "vad_ms",
            "lid_ms",
            "decode_ms",
            "punct_ms",
            "itn_ms",
            "l_dec_ms",
        ] {
            assert!(event.get(key).is_some(), "{key} is missing");
        }
        assert_eq!(event["decode_ms"], 120.457);
        assert_eq!(event["id"], 3);
        assert!(event["error_code"].is_null());
        Ok(())
    }

    #[test]
    fn a_feed_wait_event_has_the_id_the_wait_and_the_backlog()
    -> Result<(), Box<dyn std::error::Error>> {
        let line = feed_waited_line(&strings()?, 7, Duration::from_micros(12_345), 3);
        assert!(!line.contains('\n'));
        let event = parsed(&line)?;
        assert_eq!(event["event"], "asr.feed.waited");
        assert_eq!(event["level"], "info");
        assert_eq!(event["id"], 7);
        assert_eq!(event["wait_ms"], 12.345);
        assert_eq!(event["backlog"], 3);
        assert!(event["error_code"].is_null());
        Ok(())
    }

    /// The keys a line carries beyond the standard ones of every event.
    fn extra_keys(line: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let bare = parsed(&event_line(
            &strings()?,
            Level::Info,
            VAD_CLOSED,
            "asr",
            None,
            &[],
        ))?;
        let full = parsed(line)?;
        let (Some(bare), Some(full)) = (bare.as_object(), full.as_object()) else {
            return Err("a line is not a JSON object".into());
        };
        let mut extra: Vec<String> = full
            .keys()
            .filter(|key| !bare.contains_key(*key))
            .cloned()
            .collect();
        extra.sort();
        Ok(extra)
    }

    /// T-BR-04-013
    #[test]
    fn the_question_asked_in_the_middle_of_an_utterance_is_logged_with_what_it_came_to()
    -> Result<(), Box<dyn std::error::Error>> {
        for lang in [Lang::En, Lang::Ja] {
            let table = Strings::embedded(lang)?;
            assert!(table.messages.contains_key(PARTIAL_LANG_ASKED), "{lang:?}");
        }
        let asked = MidwayAsked {
            answer: Midway::Heard("en"),
            voice_samples: 32_256,
            lid_ms: 91.25,
        };
        let line = midway_asked_line(&strings()?, &asked);
        assert!(!line.contains('\n'));
        let event = parsed(&line)?;
        assert_eq!(event["event"], "asr.partial.lang.asked");
        assert_eq!(event["component"], "asr");
        assert_eq!(event["level"], "info");
        assert_eq!(extra_keys(&line)?, ["answer", "lid_ms", "voice_ms"]);
        assert_eq!(event["answer"], "en");
        assert_eq!(event["voice_ms"], 2016);
        assert_eq!(event["lid_ms"], 91.25);
        assert!(event["error_code"].is_null());
        Ok(())
    }

    /// T-BR-04-014
    #[test]
    fn an_answer_outside_the_pair_and_a_failed_question_have_their_own_names()
    -> Result<(), Box<dyn std::error::Error>> {
        let strings = strings()?;
        let answers = [
            Midway::Heard("ja"),
            Midway::Heard("en"),
            Midway::Outside,
            Midway::Failed,
        ];
        let mut written: Vec<Value> = Vec::new();
        for answer in answers {
            let asked = MidwayAsked {
                answer,
                voice_samples: 32_000,
                lid_ms: 0.0,
            };
            written.push(parsed(&midway_asked_line(&strings, &asked))?["answer"].clone());
        }
        assert_eq!(written, ["ja", "en", "outside", "failed"]);
        Ok(())
    }

    /// T-BR-06-001
    #[test]
    fn br06_the_close_of_a_segment_logs_how_far_the_detector_lagged()
    -> Result<(), Box<dyn std::error::Error>> {
        let line = vad_closed_line(&strings()?, Some("file-3"), 48_000, 32_000, Rule::Silence);
        assert!(!line.contains('\n'));
        let event = parsed(&line)?;
        assert_eq!(event["event"], "asr.vad.closed");
        assert_eq!(event["level"], "info");
        assert_eq!(extra_keys(&line)?, ["id", "lag_ms", "rule"]);
        assert_eq!(event["id"], "file-3");
        assert_eq!(event["lag_ms"], 1000);
        assert_eq!(event["rule"], "silence");
        assert!(event["error_code"].is_null());
        Ok(())
    }

    /// T-BR-06-002
    #[test]
    fn the_lag_is_the_sample_difference_in_milliseconds_at_16000_hz() {
        let lags: Vec<u64> = [16, 160, 48_000, 15, 17, 31]
            .iter()
            .map(|samples| vad_lag_ms(*samples, 0))
            .collect();
        assert_eq!(lags, [1, 10, 3000, 0, 1, 1]);
    }

    /// T-BR-06-003
    #[test]
    fn a_lag_of_zero_is_written_as_the_number_zero() -> Result<(), Box<dyn std::error::Error>> {
        let line = vad_closed_line(&strings()?, Some("file-1"), 32_000, 32_000, Rule::Silence);
        let event = parsed(&line)?;
        assert!(event["lag_ms"].is_u64(), "{line}");
        assert_eq!(event["lag_ms"], 0);
        Ok(())
    }

    /// T-BR-06-004
    #[test]
    fn the_rule_names_which_detector_or_limit_closed_the_segment()
    -> Result<(), Box<dyn std::error::Error>> {
        let rules = [Rule::Silence, Rule::Early, Rule::MaxSpeech, Rule::Flush];
        let strings = strings()?;
        let mut written: Vec<Value> = Vec::new();
        for rule in rules {
            let event = parsed(&vad_closed_line(&strings, Some("file-1"), 0, 0, rule))?;
            written.push(event["rule"].clone());
        }
        assert_eq!(written, ["silence", "early", "max_speech", "flush"]);
        Ok(())
    }

    /// T-BR-06-005
    #[test]
    fn a_segment_that_made_no_final_is_logged_with_a_null_id()
    -> Result<(), Box<dyn std::error::Error>> {
        let silent = parsed(&vad_closed_line(&strings()?, None, 0, 0, Rule::Silence))?;
        let read = parsed(&vad_closed_line(
            &strings()?,
            Some("file-7"),
            0,
            0,
            Rule::Silence,
        ))?;
        assert!(silent.get("id").is_some_and(Value::is_null), "{silent}");
        assert_eq!(read["id"], "file-7");
        Ok(())
    }

    /// T-BR-06-006
    #[test]
    fn one_line_is_written_per_closed_segment() -> Result<(), Box<dyn std::error::Error>> {
        let closed = [
            ClosedSegment {
                id: Some(1),
                rule: Rule::Silence,
            },
            ClosedSegment {
                id: None,
                rule: Rule::Early,
            },
            ClosedSegment {
                id: Some(2),
                rule: Rule::MaxSpeech,
            },
        ];
        let feeds = [(48_000, 32_000), (320_016, 320_000), (32_000, 32_000)];
        let mut lines = Vec::new();
        for (received, fed) in feeds {
            lines.extend(vad_closed_lines(
                &strings()?,
                "file",
                &closed,
                (received, fed),
            ));
        }
        assert_eq!(lines.len(), closed.len() * feeds.len());
        let lags: Vec<Value> = lines
            .iter()
            .map(|line| parsed(line).map(|event| event["lag_ms"].clone()))
            .collect::<Result<_, _>>()?;
        assert_eq!(lags, [1000, 1000, 1000, 1, 1, 1, 0, 0, 0]);
        let first = parsed(&lines[0])?;
        let second = parsed(&lines[1])?;
        assert_eq!(first["id"], "file-1");
        assert_eq!(first["rule"], "silence");
        assert!(second["id"].is_null());
        assert_eq!(second["rule"], "early");
        Ok(())
    }

    #[test]
    fn opening_the_server_to_the_network_is_a_warn_line_with_the_port_and_no_address()
    -> Result<(), Box<dyn std::error::Error>> {
        let line = lan_opened_line(&strings()?, 8833);
        assert!(!line.contains('\n'));
        let event = parsed(&line)?;
        assert_eq!(event["event"], "server.lan.opened");
        assert_eq!(event["level"], "warn");
        assert_eq!(event["port"], 8833);
        assert!(event["error_code"].is_null());
        Ok(())
    }

    #[test]
    fn every_event_the_code_emits_has_a_message_in_both_tables() -> Result<(), String> {
        for lang in [Lang::En, Lang::Ja] {
            let table = Strings::embedded(lang)?;
            for event in [
                SESSION_CREATED,
                SESSION_PLACED,
                DEVICE_UNAVAILABLE,
                UTTERANCE_TIMED,
                ENDPOINT_DECIDED,
                ENDPOINT_HELD,
                PUNCT_FAILED,
                SPEAKER_FAILED,
                FINAL_PUBLISHED,
                FEED_WAITED,
                VAD_CLOSED,
                EMBEDDING_STARTED,
                ENGINE_FAILED,
                MODEL_LOADED,
                MODEL_EVICTED,
                MEMORY_UNREADABLE,
                LANG_UNKNOWN,
                LANG_FALLBACK,
                WARMUP_DECODED,
                WARMUP_FAILED,
                SERVER_LAN_OPENED,
                notice::GPU_FALLBACK,
                notice::MODEL_DISABLED,
                notice::MODEL_SUBSTITUTE,
                notice::MODEL_CHUNKS,
                notice::ASR_DISABLED,
                notice::SERVER_PORT,
                notice::CONFIG_DEFAULT,
            ] {
                assert!(
                    table.messages.contains_key(event),
                    "{lang:?} lacks `{event}`"
                );
            }
        }
        Ok(())
    }

    #[test]
    fn the_start_up_decode_is_an_info_line_with_the_model_the_time_and_the_seconds()
    -> Result<(), Box<dyn std::error::Error>> {
        let warmed = warmed_with(PunctWarmup::Read(448.4449));
        let event = parsed(&warmup_decoded_line(&strings()?, &warmed))?;
        assert_eq!(event["event"], WARMUP_DECODED);
        assert_eq!(event["level"], "info");
        assert_eq!(event["model"], "rz");
        assert_eq!(event["decode_ms"], 987.654);
        assert_eq!(event["audio_s"], 10);
        assert_eq!(event["punct_ms"], 448.445);
        assert!(event["error_code"].is_null());
        Ok(())
    }

    fn warmed_with(punct: PunctWarmup) -> Warmup {
        Warmup {
            tier: Tier::Rz,
            audio_s: 10,
            decode_ms: 987.65432,
            punct,
        }
    }

    #[test]
    fn a_punctuation_read_of_zero_milliseconds_is_the_number_zero()
    -> Result<(), Box<dyn std::error::Error>> {
        let warmed = warmed_with(PunctWarmup::Read(0.0));
        let event = parsed(&warmup_decoded_line(&strings()?, &warmed))?;
        assert!(event["punct_ms"].is_number(), "{event}");
        assert_eq!(event["punct_ms"], 0.0);
        Ok(())
    }

    #[test]
    fn a_punctuation_read_that_did_not_run_or_failed_is_a_null_punct_ms_that_is_still_there()
    -> Result<(), Box<dyn std::error::Error>> {
        for punct in [PunctWarmup::Skipped, PunctWarmup::Failed] {
            let line = warmup_decoded_line(&strings()?, &warmed_with(punct));
            let event = parsed(&line)?;
            assert_eq!(event.get("punct_ms"), Some(&Value::Null), "{punct:?}");
            assert_eq!(event["decode_ms"], 987.654, "{punct:?}");
        }
        Ok(())
    }

    #[test]
    fn a_failed_start_up_decode_is_a_warning_line_with_its_error_code_and_the_decode_stage()
    -> Result<(), Box<dyn std::error::Error>> {
        let event = parsed(&warmup_failed_line(&strings()?))?;
        assert_eq!(event["event"], WARMUP_FAILED);
        assert_eq!(event["level"], "warn");
        assert_eq!(event["error_code"], "HAYAMIMI_ASR_WARMUP_FAILED");
        assert_eq!(event["stage"], "decode");
        Ok(())
    }

    type Said = Vec<(String, Option<String>)>;

    /// The `event` of each line, and the `stage` of a failed one.
    fn said(lines: &[String]) -> Result<Said, Box<dyn std::error::Error>> {
        let mut found = Said::new();
        for line in lines {
            let event = parsed(line)?;
            let name = event["event"].as_str().ok_or("a line has no event")?;
            found.push((name.to_owned(), event["stage"].as_str().map(str::to_owned)));
        }
        Ok(found)
    }

    fn stage(name: &str, stage: &str) -> (String, Option<String>) {
        (name.to_owned(), Some(stage.to_owned()))
    }

    #[test]
    fn a_failed_punctuation_read_says_decoded_then_failed_with_the_punct_stage()
    -> Result<(), Box<dyn std::error::Error>> {
        let read: Result<Warmup, ()> = Ok(warmed_with(PunctWarmup::Failed));
        let lines = warmup_lines(&strings()?, &read);
        let want = [
            (WARMUP_DECODED.to_owned(), None),
            stage(WARMUP_FAILED, "punct"),
        ];
        assert_eq!(said(&lines)?, want);
        Ok(())
    }

    #[test]
    fn a_punctuation_read_that_worked_or_did_not_run_says_decoded_alone()
    -> Result<(), Box<dyn std::error::Error>> {
        for punct in [PunctWarmup::Read(12.0), PunctWarmup::Skipped] {
            let read: Result<Warmup, ()> = Ok(warmed_with(punct));
            let lines = warmup_lines(&strings()?, &read);
            let want = [(WARMUP_DECODED.to_owned(), None)];
            assert_eq!(said(&lines)?, want, "{punct:?}");
        }
        Ok(())
    }

    #[test]
    fn a_failed_decode_says_the_failed_line_of_the_decode_stage_alone()
    -> Result<(), Box<dyn std::error::Error>> {
        let read: Result<Warmup, ()> = Err(());
        let lines = warmup_lines(&strings()?, &read);
        assert_eq!(said(&lines)?, [stage(WARMUP_FAILED, "decode")]);
        Ok(())
    }

    #[test]
    fn a_failed_punctuation_read_is_the_same_warning_with_the_punct_stage()
    -> Result<(), Box<dyn std::error::Error>> {
        let event = parsed(&warmup_punct_failed_line(&strings()?))?;
        assert_eq!(event["event"], WARMUP_FAILED);
        assert_eq!(event["level"], "warn");
        assert_eq!(event["error_code"], "HAYAMIMI_ASR_WARMUP_FAILED");
        assert_eq!(event["stage"], "punct");
        Ok(())
    }

    #[test]
    fn a_model_loaded_or_dropped_is_an_info_line_with_the_tier_and_no_path()
    -> Result<(), Box<dyn std::error::Error>> {
        let loaded = resident_line(
            &strings()?,
            &ResidentEvent::Loaded {
                tier: Tier::Pz,
                ms: 1234.5678,
            },
        );
        let Some(loaded) = loaded else {
            return Err("a loaded model made no line".into());
        };
        let event = parsed(&loaded)?;
        assert_eq!(event["event"], MODEL_LOADED);
        assert_eq!(event["level"], "info");
        assert_eq!(event["model"], "pz");
        assert_eq!(event["load_ms"], 1234.568);
        let dropped = resident_line(&strings()?, &ResidentEvent::Evicted { tier: Tier::Sv });
        let Some(dropped) = dropped else {
            return Err("a dropped model made no line".into());
        };
        let event = parsed(&dropped)?;
        assert_eq!(event["event"], MODEL_EVICTED);
        assert_eq!(event["model"], "sv");
        Ok(())
    }

    #[test]
    fn a_language_tag_that_could_not_be_read_is_a_warning_line()
    -> Result<(), Box<dyn std::error::Error>> {
        let Some(line) = resident_line(&strings()?, &ResidentEvent::LangUnknown) else {
            return Err("an unreadable tag made no line".into());
        };
        let event = parsed(&line)?;
        assert_eq!(event["event"], LANG_UNKNOWN);
        assert_eq!(event["level"], "warn");
        assert_eq!(event["error_code"], "HAYAMIMI_ASR_LANG_UNKNOWN");
        Ok(())
    }

    #[test]
    fn a_fallback_of_the_recognizers_has_no_resident_line() -> Result<(), Box<dyn std::error::Error>>
    {
        let substituted = ResidentEvent::Substituted {
            requested: Tier::V2,
            used: Tier::V3,
            reason: Reason::Missing,
        };
        assert_eq!(resident_line(&strings()?, &substituted), None);
        Ok(())
    }

    #[test]
    fn a_substitution_names_the_tier_asked_for_and_the_tier_used()
    -> Result<(), Box<dyn std::error::Error>> {
        let Some(notice) = Notice::from_resident(&ResidentEvent::Substituted {
            requested: Tier::V2,
            used: Tier::V3,
            reason: Reason::Missing,
        }) else {
            return Err("a substitution made no notice".into());
        };
        let event = parsed(&fallback_line(&strings()?, &notice))?;
        assert_eq!(event["event"], "model.fallback.substitute");
        assert_eq!(event["requested"], "v2");
        assert_eq!(event["used"], "v3");
        assert_eq!(event["stage"], "recognizer");
        Ok(())
    }

    #[test]
    fn an_endpoint_event_names_the_rule_and_the_silence_without_the_text()
    -> Result<(), Box<dyn std::error::Error>> {
        let line = endpoint_decided_line(&strings()?, &finished());
        assert!(!line.contains("本文"));
        let event = parsed(&line)?;
        assert_eq!(event["event"], "asr.endpoint.decided");
        assert_eq!(event["rule"], "silence");
        assert_eq!(event["silence_ms"], 300);
        assert!(event.get("id").is_some());
        Ok(())
    }

    #[test]
    fn a_held_endpoint_event_names_the_reason_the_span_and_the_minimum_without_the_text()
    -> Result<(), Box<dyn std::error::Error>> {
        for (reason, name) in [
            (Hold::TooShort, "too_short"),
            (Hold::Incomplete, "incomplete"),
        ] {
            let hold = EarlyHold {
                reason,
                span_ms: 480.5,
                tail: Tail::Other,
                shadow_complete: false,
            };
            let line = endpoint_held_line(&strings()?, &hold);
            assert!(!line.contains('\n'));
            let event = parsed(&line)?;
            assert_eq!(event["event"], "asr.endpoint.held");
            assert_eq!(event["level"], "info");
            assert_eq!(event["reason"], name);
            assert_eq!(event["span_ms"], 480.5);
            assert_eq!(event["min_ms"], 1000);
            assert!(event["error_code"].is_null());
        }
        Ok(())
    }

    /// The fields an event line has whatever the event is.
    const ENVELOPE: [&str; 10] = [
        "schema_version",
        "project_slug",
        "timestamp",
        "level",
        "event",
        "component",
        "message",
        "error_code",
        "request_id",
        "build_revision",
    ];

    #[test]
    fn a_held_endpoint_event_names_the_tail_and_the_shadow_answer_and_no_sentence()
    -> Result<(), Box<dyn std::error::Error>> {
        for (tail, name) in [
            (Tail::Noun, "noun"),
            (Tail::Honorific, "honorific"),
            (Tail::Particle, "particle"),
            (Tail::Interjection, "interjection"),
            (Tail::Other, "other"),
        ] {
            for shadow_complete in [true, false] {
                let hold = EarlyHold {
                    reason: Hold::Incomplete,
                    span_ms: 2400.0,
                    tail,
                    shadow_complete,
                };
                let event = parsed(&endpoint_held_line(&strings()?, &hold))?;
                assert_eq!(event["tail"], name);
                assert_eq!(event["shadow_complete"], shadow_complete);
                let Some(fields) = event.as_object() else {
                    return Err("an event line is not an object".into());
                };
                let mut extra: Vec<&str> = fields
                    .keys()
                    .map(String::as_str)
                    .filter(|key| !ENVELOPE.contains(key))
                    .collect();
                extra.sort_unstable();
                assert_eq!(
                    extra,
                    ["min_ms", "reason", "shadow_complete", "span_ms", "tail"]
                );
            }
        }
        Ok(())
    }

    fn fell_back(id: u64, step: FallbackStep, lid: Option<&str>) -> Final {
        let mut done = finished();
        done.id = id;
        done.fallback = Some(LangFallback {
            step,
            lid: lid.map(str::to_owned),
        });
        done
    }

    /// T-IP-03-024
    #[test]
    fn a_language_fallback_event_has_the_fields_the_design_names()
    -> Result<(), Box<dyn std::error::Error>> {
        let cases = [
            (fell_back(7, FallbackStep::SvTag, Some("zh")), "info"),
            (fell_back(7, FallbackStep::Default, None), "info"),
            (fell_back(7, FallbackStep::SvFailed, Some("zh")), "warn"),
        ];
        for lang in [Lang::En, Lang::Ja] {
            let table = Strings::embedded(lang)?;
            assert!(table.messages.contains_key(LANG_FALLBACK), "{lang:?}");
        }
        for (done, level) in cases {
            let Some(line) = lang_fallback_line(&strings()?, &done) else {
                return Err("a final that fell back made no line".into());
            };
            let event = parsed(&line)?;
            assert_eq!(event["event"], "asr.lang.fallback");
            assert_eq!(event["component"], "asr");
            assert_eq!(event["level"], level);
            let Some(mark) = &done.fallback else {
                return Err("the final lost its mark".into());
            };
            assert_eq!(event["id"], 7);
            assert_eq!(event["step"], mark.step.name());
            assert_eq!(
                event["lid"],
                mark.lid.as_deref().map_or(Value::Null, Value::from)
            );
            let Some(fields) = event.as_object() else {
                return Err("an event line is not an object".into());
            };
            let mut extra: Vec<&str> = fields
                .keys()
                .map(String::as_str)
                .filter(|key| !ENVELOPE.contains(key))
                .collect();
            extra.sort_unstable();
            assert_eq!(extra, ["id", "lid", "step"]);
        }
        let steps = [
            FallbackStep::SvTag,
            FallbackStep::Script,
            FallbackStep::Default,
            FallbackStep::SvFailed,
        ];
        assert_eq!(
            steps.map(FallbackStep::name),
            ["sv_tag", "script", "default", "sv_failed"]
        );
        Ok(())
    }

    /// T-IP-03-035
    #[test]
    fn a_final_that_fell_back_makes_one_language_fallback_line_with_its_id_and_one_that_did_not_makes_none()
    -> Result<(), Box<dyn std::error::Error>> {
        let strings = strings()?;
        let kept = fell_back(7, FallbackStep::SvTag, Some("zh"));
        let line = lang_fallback_line(&strings, &kept);
        let Some(line) = line else {
            return Err("the first final made no line".into());
        };
        let event = parsed(&line)?;
        assert_eq!(event["event"], "asr.lang.fallback");
        assert_eq!(event["id"], 7);
        assert_eq!(event["step"], "sv_tag");
        assert_eq!(event["lid"], "zh");

        let mut plain = finished();
        plain.id = 8;
        assert_eq!(lang_fallback_line(&strings, &plain), None);

        let failed = fell_back(9, FallbackStep::SvFailed, None);
        let Some(line) = lang_fallback_line(&strings, &failed) else {
            return Err("the failed final made no line".into());
        };
        let event = parsed(&line)?;
        assert_eq!(event["level"], "warn");
        assert_eq!(event["id"], 9);
        assert!(event["lid"].is_null());
        Ok(())
    }

    fn unread_for(reason: UnreadReason) -> Unread {
        Unread {
            start_s: 12.3456,
            end_s: 13.5,
            reason,
            lang: String::from("en"),
            tier: "pz",
        }
    }

    /// IP-9: a segment that read as nothing leaves one `info` line with the
    /// five fields of the design, the span in stream seconds, and no text.
    #[test]
    fn ip09_a_segment_read_as_nothing_leaves_one_log_line() -> Result<(), Box<dyn std::error::Error>>
    {
        for lang in [Lang::En, Lang::Ja] {
            let table = Strings::embedded(lang)?;
            assert!(table.messages.contains_key(SEGMENT_UNREAD), "{lang:?}");
        }
        for (reason, name) in [
            (UnreadReason::EmptyDecode, "empty_decode"),
            (UnreadReason::EmptyText, "empty_text"),
        ] {
            let line = segment_unread_line(&strings()?, &unread_for(reason));
            assert!(!line.contains('\n'));
            let event = parsed(&line)?;
            assert_eq!(event["event"], "asr.segment.unread");
            assert_eq!(event["component"], "asr");
            assert_eq!(event["level"], "info");
            assert!(event["error_code"].is_null());
            assert_eq!(event["start_s"], 12.346);
            assert_eq!(event["end_s"], 13.5);
            assert_eq!(event["reason"], name);
            assert_eq!(event["lang"], "en");
            assert_eq!(event["tier"], "pz");
            let Some(fields) = event.as_object() else {
                return Err("an event line is not an object".into());
            };
            let mut extra: Vec<&str> = fields
                .keys()
                .map(String::as_str)
                .filter(|key| !ENVELOPE.contains(key))
                .collect();
            extra.sort_unstable();
            assert_eq!(extra, ["end_s", "lang", "reason", "start_s", "tier"]);
        }
        Ok(())
    }

    #[test]
    fn a_line_has_no_newline() -> Result<(), Box<dyn std::error::Error>> {
        assert!(!utterance_timed_line(&strings()?, &finished()).contains('\n'));
        Ok(())
    }
}

//! A record made by hand, for the tools of `quality.rs`: the text of a samples
//! file, of an events file and of a log. The clock is the tool's and the
//! stream's at once (a record with no `session_start`), and a time is in seconds.

use serde_json::json;

fn micros(seconds: f64) -> i64 {
    (seconds * 1_000_000.0).round() as i64
}

/// One line of a samples file: an utterance of a repeat whose voice spans
/// `[start_s, end_s)`. The tools assign finals by these two times alone.
pub fn sample_line(repeat: u32, utt: u32, start_s: f64, end_s: f64) -> String {
    json!({
        "repeat": repeat, "utt": utt,
        "t_start_us": micros(start_s), "t_end_us": micros(end_s),
        "t_partial_us": null, "t_partial_tr_us": null, "t_final_us": null, "t_tr_us": null,
        "finals": 0, "text": "", "translation": null, "final_latency_ms": null,
        "cer_dist": 0, "cer_denom": 0
    })
    .to_string()
}

/// One line of an events file: a final in `lang`, `id` its id, that stands for
/// `[t0_s, t1_s)` of the stream and was received at `arrival_s`. It belongs to
/// the latest utterance that had started at `t1_s`.
pub fn final_line(id: &str, lang: &str, (t0_s, t1_s): (f64, f64), arrival_s: f64) -> String {
    json!({
        "event": {
            "type": "final", "id": id, "lang": lang, "text": "x",
            "latency_ms": 250.5, "t0_s": t0_s, "t1_s": t1_s
        },
        "t_us": micros(arrival_s)
    })
    .to_string()
}

/// One line of a log: a segment that read as nothing and spans
/// `[start_s, end_s)` of the stream.
pub fn unread_line(start_s: f64, end_s: f64) -> String {
    json!({
        "event": "asr.segment.unread", "level": "info", "reason": "empty_decode",
        "start_s": start_s, "end_s": end_s, "lang": "ja", "tier": "rz"
    })
    .to_string()
}

/// One line of a log: the language route of final number `id` took `step`, and
/// whisper-tiny answered `lid` (`None` for null).
pub fn fallback_line(id: u64, step: &str, lid: Option<&str>) -> String {
    json!({"event": "asr.lang.fallback", "level": "info", "id": id, "step": step, "lid": lid})
        .to_string()
}

/// The lines joined, one for each line of the file.
pub fn file_of(lines: &[String]) -> String {
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

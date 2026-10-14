//! RP-15 (T-RP-15-012, T-RP-15-013): a `final` does not wait for the speaker.
//!
//! The `final` goes out with an empty `speaker`; once the embedding is done,
//! one `speaker` event of the same id follows. The wait is read from the log,
//! not from a difference of delays: for each id, `asr.final.published` is at
//! or before `speaker.embedding.started` on the process's monotonic clock.

mod support;

use serde_json::{Value, json};
use support::{Failure, Output, Served, TestResult, clip, events, models_dir, named, serve};

fn text_of<'a>(event: &'a Value, key: &str) -> Option<&'a str> {
    event.get(key).and_then(Value::as_str)
}

/// `mono_us` of every event named `name`, by utterance id.
fn times_of(all: &[Value], name: &str) -> Result<Vec<(u64, u64)>, Failure> {
    named(all, name)
        .into_iter()
        .map(|event| {
            let id = event.get("id").and_then(Value::as_u64);
            let at = event.get("mono_us").and_then(Value::as_u64);
            match (id, at) {
                (Some(id), Some(at)) => Ok((id, at)),
                _ => Err(format!("a {name} event has no id or no mono_us: {event}")),
            }
        })
        .collect()
}

/// Every `final` has an empty `speaker`, and each is followed by exactly one
/// `speaker` event of its id. Returns how many finals there are.
fn check_speaker_follows_final(wire: &[Value]) -> Result<usize, Failure> {
    let mut finals = 0;
    for (at, done) in wire.iter().enumerate() {
        if text_of(done, "type") != Some("final") {
            continue;
        }
        finals += 1;
        let id = text_of(done, "id").ok_or("a final has no id")?;
        if text_of(done, "speaker") != Some("") {
            return Err(format!(
                "final {id} waited for the speaker: it carries a mark"
            ));
        }
        let after = wire[at + 1..].iter().filter(|event| {
            text_of(event, "type") == Some("speaker") && text_of(event, "id") == Some(id)
        });
        let count = after.count();
        if count != 1 {
            return Err(format!("{count} speaker events follow final {id}, not 1"));
        }
    }
    Ok(finals)
}

/// For each embedding that started, its final had been published at or before that time.
fn check_published_before_embedding(stdout: &str) -> Result<usize, Failure> {
    let all = events(stdout)?;
    let published = times_of(&all, "asr.final.published")?;
    let started = times_of(&all, "speaker.embedding.started")?;
    for (id, began) in &started {
        let Some((_, sent)) = published.iter().find(|(known, _)| known == id) else {
            return Err(format!(
                "utterance {id} started an embedding but its final was never published"
            ));
        };
        if sent > began {
            return Err(format!(
                "utterance {id}: the final was published at {sent} us, after the embedding started at {began} us"
            ));
        }
    }
    Ok(started.len())
}

/// One run with the speaker stage on: no final waits for the speaker.
fn check_final_does_not_wait(run: &Served) -> Result<(), Failure> {
    if check_speaker_follows_final(&run.wire)? == 0 {
        return Err(String::from("the run made no final"));
    }
    if check_published_before_embedding(&run.output.stdout)? == 0 {
        return Err(String::from(
            "no speaker.embedding.started event in the log",
        ));
    }
    Ok(())
}

fn log_line(name: &str, id: u64, mono_us: u64) -> String {
    json!({"event": name, "id": id, "mono_us": mono_us}).to_string()
}

/// A run whose log has the two events at the given times, and whose `final`
/// carries `speaker_in_final`.
fn served(published_us: u64, started_us: u64, speaker_in_final: &str) -> Served {
    let stdout = [
        log_line("asr.final.published", 0, published_us),
        log_line("speaker.embedding.started", 0, started_us),
    ];
    let done = json!({"type": "final", "id": "file-0", "speaker": speaker_in_final});
    let mark = json!({"type": "speaker", "id": "file-0", "speaker": "S1"});
    Served {
        output: Output {
            code: Some(0),
            stdout: stdout.join("\n"),
            stderr: String::new(),
        },
        wire: vec![done, mark],
    }
}

fn path_text(path: &std::path::Path) -> Result<String, Failure> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("{} is not UTF-8", path.display()))
}

#[test]
#[ignore = "real run: needs the models directory with the speaker model"]
fn final_does_not_wait_for_the_speaker() -> TestResult {
    let (wav, models) = (path_text(&clip())?, path_text(&models_dir()?)?);
    let args = [
        "--wav",
        &wav,
        "--no-realtime",
        "--mode",
        "single",
        "--lang",
        "ja",
        "--models",
        &models,
    ];
    let run = serve(&args, Some("C.UTF-8"), &[])?;
    check_final_does_not_wait(&run)?;
    Ok(())
}

#[test]
fn final_waiting_for_the_speaker_is_rejected() {
    // The embedding started (at 100 us) before the final was published (at 400 us).
    let said = check_final_does_not_wait(&served(400, 100, ""));
    assert!(said.is_err_and(|text| {
        text.contains("utterance 0") && text.contains("400 us") && text.contains("100 us")
    }));
    // The same order with a mark inside the final is rejected too.
    assert!(check_final_does_not_wait(&served(400, 100, "S1")).is_err());
}

#[test]
fn a_final_published_before_the_embedding_started_passes() -> TestResult {
    check_final_does_not_wait(&served(100, 400, ""))?;
    check_final_does_not_wait(&served(400, 400, ""))?;
    Ok(())
}

#[test]
fn a_final_with_no_speaker_event_after_it_is_rejected() {
    let done = json!({"type": "final", "id": "file-0", "speaker": ""});
    assert!(check_speaker_follows_final(&[done]).is_err());
}

#[test]
fn a_log_without_an_embedding_start_is_rejected() {
    let mut run = served(100, 400, "");
    run.output.stdout = log_line("asr.final.published", 0, 100);
    assert!(check_final_does_not_wait(&run).is_err());
}

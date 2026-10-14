//! RP-15 (T-RP-15-012, T-RP-15-013): a `final` does not wait for the speaker.
//!
//! The `final` goes out with an empty `speaker`; once the embedding is done,
//! one `speaker` event of the same id follows. The time the text took
//! (`l_dec_ms` of `asr.utterance.timed`) must not grow by more than
//! `MAX_P50_GAP_MS` when the speaker stage is on.

mod support;

use serde_json::{Value, json};
use support::{
    Failure, Output, Served, TestResult, clip, events, models_dir, models_without, named, serve,
};

/// The most the p50 of `l_dec_ms` may grow with the speaker stage on. A
/// provisional value; R7 measures it again on two sources.
const MAX_P50_GAP_MS: f64 = 5.0;

/// Runs per side of the real comparison; their `l_dec_ms` values are pooled.
const RUNS: usize = 3;

fn text_of<'a>(event: &'a Value, key: &str) -> Option<&'a str> {
    event.get(key).and_then(Value::as_str)
}

/// `l_dec_ms` of every utterance a run logged.
fn l_dec_values(stdout: &str) -> Result<Vec<f64>, Failure> {
    let all = events(stdout)?;
    Ok(named(&all, "asr.utterance.timed")
        .into_iter()
        .filter_map(|event| event.get("l_dec_ms").and_then(Value::as_f64))
        .collect())
}

/// `l_dec_ms` of every utterance of several runs.
fn pooled_l_dec(runs: &[Served]) -> Result<Vec<f64>, Failure> {
    let mut pooled = Vec::new();
    for run in runs {
        pooled.extend(l_dec_values(&run.output.stdout)?);
    }
    Ok(pooled)
}

/// Nearest-rank median.
fn p50(mut values: Vec<f64>) -> Result<f64, Failure> {
    if values.is_empty() {
        return Err(String::from("no asr.utterance.timed event with l_dec_ms"));
    }
    values.sort_by(f64::total_cmp);
    let rank = values.len().div_ceil(2);
    Ok(values[rank - 1])
}

/// Every `final` has an empty `speaker`, and each is followed by exactly one
/// `speaker` event of its id.
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

/// The run with the speaker stage on against the run without it.
fn check_final_does_not_wait(with: &[Served], without: &[Served]) -> Result<f64, Failure> {
    let mut finals = 0;
    for run in with {
        finals += check_speaker_follows_final(&run.wire)?;
    }
    if finals == 0 {
        return Err(String::from("the run with the speaker stage made no final"));
    }
    let gap = p50(pooled_l_dec(with)?)? - p50(pooled_l_dec(without)?)?;
    if gap > MAX_P50_GAP_MS {
        return Err(format!(
            "the p50 of l_dec_ms is {gap:.1} ms higher with the speaker stage on (limit {MAX_P50_GAP_MS} ms)"
        ));
    }
    Ok(gap)
}

fn timed_line(l_dec_ms: f64) -> String {
    json!({"event": "asr.utterance.timed", "l_dec_ms": l_dec_ms}).to_string()
}

fn served(l_dec: &[f64], speaker_in_final: &str) -> Served {
    let stdout: Vec<String> = l_dec.iter().copied().map(timed_line).collect();
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

fn run_several(models: &std::path::Path) -> Result<Vec<Served>, Failure> {
    let wav = clip();
    let (wav, models) = (path_text(&wav)?, path_text(models)?);
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
    (0..RUNS)
        .map(|_| serve(&args, Some("C.UTF-8"), &[]))
        .collect()
}

fn path_text(path: &std::path::Path) -> Result<String, Failure> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("{} is not UTF-8", path.display()))
}

#[test]
#[ignore = "real run: needs the models directory with the speaker model"]
fn final_does_not_wait_for_the_speaker() -> TestResult {
    let bare = run_several(&models_without("campplus_sv.onnx")?)?;
    let marked = run_several(&models_dir()?)?;
    let gap = check_final_does_not_wait(&marked, &bare)?;
    println!("p50 of l_dec_ms, speaker on minus off: {gap:.2} ms (limit {MAX_P50_GAP_MS} ms)");
    Ok(())
}

#[test]
fn final_waiting_for_the_speaker_is_rejected() {
    let waiting = [served(&[200.0, 200.0], "S1")];
    let bare = [served(&[130.0, 130.0], "")];
    let said = check_final_does_not_wait(&waiting, &bare);
    assert!(said.is_err_and(|text| text.contains("speaker")));
    let said = check_final_does_not_wait(&[served(&[200.0, 200.0], "")], &bare);
    assert!(said.is_err_and(|text| text.contains("70.0 ms")));
}

#[test]
fn a_final_that_sent_its_speaker_after_the_text_passes() -> TestResult {
    let gap = check_final_does_not_wait(&[served(&[132.0], "")], &[served(&[130.0], "")])?;
    assert!((gap - 2.0).abs() < 1e-9);
    Ok(())
}

#[test]
fn a_final_with_no_speaker_event_after_it_is_rejected() {
    let done = json!({"type": "final", "id": "file-0", "speaker": ""});
    assert!(check_speaker_follows_final(&[done]).is_err());
}

#[test]
fn the_median_of_an_even_count_is_the_lower_middle() -> TestResult {
    assert!((p50(vec![4.0, 1.0, 3.0, 2.0])? - 2.0).abs() < 1e-9);
    assert!(p50(Vec::new()).is_err());
    Ok(())
}

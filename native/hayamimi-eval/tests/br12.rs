//! BR-12: the clips of IP-4 are ranked by the delta `--mode pair` adds to the
//! wait for the closing `final`, the clips over the line of the 95th percentile
//! are marked, and every clip carries the marks of the path its language took
//! (design 10.13.13, 22). The tool is `hayamimi_eval::clip_path`.
//!
//! The rows that read the record read the three runs of IP-4 in
//! `docs/results/in-person/` (`ip04_pair.*`, `ip04_lang_ja.jsonl`,
//! `ip04_lang_en.jsonl`, `ip04.md`) and the three manifests they were made with,
//! which are copied there as `br12_*_manifest.json`. The other rows feed the
//! tool a record made by hand: clips whose three repeats waited the same time,
//! and finals that closed each sample 1.2 s after it began.
//!
//! The record of the tool is `docs/results/in-person/br12_paths.md`; the row that
//! writes it is `#[ignore]`d and runs with `-- --ignored`.

mod support;

use std::path::Path;

use hayamimi_eval::clip_path::{
    OVER_US, RankedClip, Reread, RunInput, clip_text, delta_p95_us, rank_clips,
};
use hayamimi_eval::compare::read_samples;
use hayamimi_eval::latency::Sample;
use hayamimi_eval::manifest::read_rows;
use hayamimi_eval::percentile::micros_to_ms;
use serde_json::{Value, json};
use support::{Failure, REPO, TestResult, read, window_sample};

fn records() -> std::path::PathBuf {
    Path::new(REPO).join("docs/results/in-person")
}

fn shas_of(manifest: &str) -> Result<Vec<String>, Failure> {
    read_rows(&records().join(manifest))
        .map_err(|error| error.to_string())?
        .iter()
        .enumerate()
        .map(|(at, row)| {
            row.get("sha256")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| format!("row {at} of {manifest} has no `sha256`"))
        })
        .collect()
}

fn samples_in(name: &str) -> Result<Vec<Sample>, Failure> {
    read_samples(&records().join(name)).map_err(|error| error.to_string())
}

/// The ranked clips of the three runs of IP-4.
fn ranked_real() -> Result<Vec<RankedClip>, Failure> {
    let (pair, ja, en) = (
        samples_in("ip04_pair.jsonl")?,
        samples_in("ip04_lang_ja.jsonl")?,
        samples_in("ip04_lang_en.jsonl")?,
    );
    let (pair_shas, ja_shas, en_shas) = (
        shas_of("br12_dialog_manifest.json")?,
        shas_of("br12_ja_manifest.json")?,
        shas_of("br12_en_manifest.json")?,
    );
    let forced = [
        RunInput {
            samples: &ja,
            shas: &ja_shas,
        },
        RunInput {
            samples: &en,
            shas: &en_shas,
        },
    ];
    let events = read(&records().join("ip04_pair.events.jsonl"))?;
    let log = read(&records().join("ip04_pair.log"))?;
    rank_clips(
        RunInput {
            samples: &pair,
            shas: &pair_shas,
        },
        &forced,
        &events,
        &log,
    )
    .map_err(|error| error.to_string())
}

// ---- a record made by hand ------------------------------------------------------

/// The samples of a pair run of one clip for each entry of `waits_ms`, three
/// repeats each, the repeat of a clip waiting that long for its closing final.
/// A sample begins 10 s after the one before it.
fn pair_samples(waits_ms: &[i64]) -> Vec<Sample> {
    let clips = waits_ms.len() as u32;
    let mut samples = Vec::new();
    for repeat in 1..=3_u32 {
        for (at, wait_ms) in waits_ms.iter().enumerate() {
            let utt = at as u32 + 1;
            let start = i64::from((repeat - 1) * clips + at as u32) * 10_000_000;
            let mut sample = window_sample(repeat, utt, start, start + 1_000_000);
            sample.t_final_us = Some(start + 1_000_000 + wait_ms * 1000);
            samples.push(sample);
        }
    }
    samples
}

/// A run told the language: each clip waited `waits_ms[clip]` in all its repeats.
fn forced_samples(waits_ms: &[i64]) -> Vec<Sample> {
    pair_samples(waits_ms)
}

fn shas(count: usize) -> Vec<String> {
    (1..=count).map(|at| format!("sha-{at}")).collect()
}

/// The events of the pair run: one final for each sample, `file-1`, `file-2`, and
/// so on in the order of the samples, standing for 0.1 s to 1.2 s after it began.
fn pair_events(samples: &[Sample]) -> String {
    let lines: Vec<String> = samples
        .iter()
        .enumerate()
        .map(|(at, sample)| {
            let start = sample.t_start_us as f64 / 1_000_000.0;
            json!({
                "event": {
                    "type": "final", "id": format!("file-{}", at + 1), "text": "x",
                    "latency_ms": 250.0, "t0_s": start + 0.1, "t1_s": start + 1.2
                },
                "t_us": sample.t_end_us + 400_000
            })
            .to_string()
        })
        .collect();
    format!("{}\n", lines.join("\n"))
}

/// The number of the final of the first repeat of clip `utt` (1-based) in a run of `clips` clips.
fn first_final(utt: usize) -> usize {
    utt
}

fn log_of(lines: &[Value]) -> String {
    let mut text = lines
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    text.push('\n');
    text
}

fn timed(id: usize, lid_ms: f64) -> Value {
    json!({"event": "asr.utterance.timed", "id": id, "lid_ms": lid_ms})
}

fn fallback(id: usize, step: &str) -> Value {
    json!({"event": "asr.lang.fallback", "id": id, "step": step, "lid": "zh"})
}

fn reread(id: usize) -> Value {
    json!({"event": "asr.lang.reread", "id": id})
}

/// The ranked clips of a hand-made pair run against a hand-made `--lang` run.
fn ranked_by_hand(
    pair_ms: &[i64],
    forced_ms: &[i64],
    log: &str,
) -> Result<Vec<RankedClip>, Failure> {
    let (pair, forced) = (pair_samples(pair_ms), forced_samples(forced_ms));
    let events = pair_events(&pair);
    let shas = shas(pair_ms.len());
    rank_clips(
        RunInput {
            samples: &pair,
            shas: &shas,
        },
        &[RunInput {
            samples: &forced,
            shas: &shas,
        }],
        &events,
        log,
    )
    .map_err(|error| error.to_string())
}

/// T-BR-12-001
#[test]
fn br12_the_clips_over_the_ip04_line_are_named_with_their_language_path() -> TestResult {
    let ranked = ranked_real()?;
    assert_eq!(ranked.len(), 26);
    assert!(
        ranked
            .windows(2)
            .all(|pair| pair[0].delta_us >= pair[1].delta_us),
        "the clips are not in order of the delta"
    );
    for clip in &ranked {
        assert_eq!(clip.over, clip.delta_us > OVER_US, "utt {}", clip.utt);
        assert!(
            clip.marks.lid_ms.is_some(),
            "utt {} has no lid_ms",
            clip.utt
        );
        assert!(!clip_text(clip).is_empty());
    }
    let over = ranked.iter().filter(|clip| clip.over).count();
    assert!(over >= 2, "{over} clips are over the line");
    Ok(())
}

/// T-BR-12-002
#[test]
fn the_delta_p95_of_the_named_clips_is_the_381_ms_of_the_record() -> TestResult {
    let ranked = ranked_real()?;
    let record = read(&records().join("ip04.md"))?;
    let line = record
        .lines()
        .find_map(|line| line.strip_prefix("delta p95:"))
        .ok_or("ip04.md has no line `delta p95:`")?;
    let digits: String = line
        .trim_start()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    let recorded: i64 = digits.parse()?;
    assert_eq!(recorded, 381);
    assert_eq!(micros_to_ms(delta_p95_us(&ranked)?), recorded);
    Ok(())
}

/// T-BR-12-003
#[test]
fn a_clip_without_any_path_mark_is_written_as_no_mark() -> TestResult {
    let ranked = ranked_by_hand(&[600], &[100], "")?;
    assert_eq!(ranked.len(), 1);
    let text = clip_text(&ranked[0]);
    assert!(text.contains("道 印なし"), "{text}");
    assert!(ranked[0].marks.steps.is_empty());
    assert_eq!(ranked[0].marks.reread, Reread::NoMark);
    // The control: a clip with a fallback line is not written as having no mark.
    let log = log_of(&[fallback(first_final(1), "sv_tag")]);
    let marked = clip_text(&ranked_by_hand(&[600], &[100], &log)?[0]);
    assert!(!marked.contains("道 印なし"), "{marked}");
    Ok(())
}

/// T-BR-12-004
#[test]
fn the_clips_are_listed_from_the_largest_delta_down() -> TestResult {
    let ranked = ranked_by_hand(&[150, 500, 220, 400], &[100, 100, 100, 100], "")?;
    let deltas: Vec<i64> = ranked.iter().map(|clip| clip.delta_us / 1000).collect();
    assert_eq!(deltas, vec![400, 300, 120, 50]);
    Ok(())
}

/// T-BR-12-005
#[test]
fn each_mark_of_a_clip_is_written_and_a_missing_reread_is_written_as_none() -> TestResult {
    // Clip 1: step sv_tag, read again, lid_ms 91. Clip 2: a step, no re-read.
    // The log has a re-read line, so clip 2 was not read again.
    let log = log_of(&[
        fallback(1, "sv_tag"),
        reread(1),
        timed(1, 91.0),
        fallback(2, "script"),
        timed(2, 40.0),
    ]);
    let ranked = ranked_by_hand(&[700, 600], &[100, 100], &log)?;
    let first = &ranked[0];
    assert_eq!(first.marks.steps, vec![String::from("sv_tag")]);
    assert_eq!(first.marks.reread, Reread::Seen);
    assert_eq!(first.marks.lid_ms, Some(91));
    let text = clip_text(first);
    assert!(
        text.contains("sv_tag") && text.contains("読み直し あり") && text.contains("lid_ms 91"),
        "{text}"
    );
    let second = &ranked[1];
    assert_eq!(second.marks.reread, Reread::NotSeen);
    let text = clip_text(second);
    assert!(
        text.contains("script") && text.contains("読み直し なし"),
        "{text}"
    );
    Ok(())
}

/// T-BR-12-006
#[test]
fn only_a_delta_over_300_ms_is_marked_over_and_every_clip_keeps_its_path_marks() -> TestResult {
    let log = log_of(&[
        fallback(1, "sv_tag"),
        fallback(2, "sv_tag"),
        fallback(3, "sv_tag"),
    ]);
    let ranked = ranked_by_hand(&[399, 400, 401], &[100, 100, 100], &log)?;
    let by_delta: Vec<(i64, bool)> = ranked
        .iter()
        .map(|clip| (clip.delta_us / 1000, clip.over))
        .collect();
    assert_eq!(by_delta, vec![(301, true), (300, false), (299, false)]);
    for clip in &ranked {
        assert_eq!(
            clip.marks.steps,
            vec![String::from("sv_tag")],
            "utt {}",
            clip.utt
        );
    }
    Ok(())
}

/// The record of the tool on the three runs of IP-4.
#[test]
#[ignore = "writes docs/results/in-person/br12_paths.md"]
fn the_record_of_br12_is_written() -> TestResult {
    let ranked = ranked_real()?;
    let mut text = String::from(
        "# BR-12: IP-4 の線の外の clip の道\n\n\
         記録: ip04_pair.*、ip04_lang_ja.jsonl、ip04_lang_en.jsonl、ip04.md、br12_*_manifest.json\n\
         結べた clip: ",
    );
    text.push_str(&format!("{}\n", ranked.len()));
    text.push_str(&format!(
        "差の p95: {} ms（線 300 ms）\n",
        micros_to_ms(delta_p95_us(&ranked)?)
    ));
    text.push_str(
        "読み直し: 記録の log に `asr.lang.reread` の行が無い。生きた経路は読み直しを log に出さないので、\
         どの clip も「印なし」である。\n\n",
    );
    for clip in &ranked {
        text.push_str(&clip_text(clip));
        text.push('\n');
    }
    std::fs::write(records().join("br12_paths.md"), text)?;
    Ok(())
}

//! BR-10: the utterances a run mixed up are named, with the answers of the two
//! language identifiers where the log keeps them (design 10.13.13, 1 and 21).
//! The tool is in `support/quality.rs`.
//!
//! The rows that read a record read the copies under
//! `docs/results/in-person/br10_*`, names no run writes: the records of IP-2
//! (`br10_ip2_pair.*`, `br10_ip2_fast.*`, `br10_ip2.md`) and of IP-7
//! (`br10_ip7_pair.*`, `br10_ip7.md`), and the manifests they were made with
//! (`br10_ip2_manifest.json`, `br10_ip7_manifest.json`). The other rows feed the
//! tool a record made by hand (`support/hand.rs`).
//!
//! The record of the tool is `docs/results/in-person/br10_named.md`; the row that
//! writes it is `#[ignore]`d and runs with `-- --ignored`.

mod support;

use std::error::Error;
use std::path::Path;

use hayamimi_eval::manifest::Row;
use serde_json::json;
use support::dialog::manifest_rows;
use support::hand::{fallback_line, file_of, final_line, sample_line};
use support::quality::{MixedUp, mixed_up_text, mixed_up_utterances, numbers_after};
use support::{Failure, TestResult, read};

const RECORDS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/results/in-person");

/// What a run left: its samples, its events and its log.
struct Run {
    samples: String,
    events: String,
    log: String,
}

fn run_of(prefix: &str) -> Result<Run, Failure> {
    let copy = |extension: &str| read(&Path::new(RECORDS).join(format!("{prefix}.{extension}")));
    Ok(Run {
        samples: copy("jsonl")?,
        events: copy("events.jsonl")?,
        log: copy("log")?,
    })
}

fn manifest_of(name: &str) -> Result<Vec<Row>, Failure> {
    manifest_rows(&Path::new(RECORDS).join(name))
}

fn named(run: &Run, manifest: &[Row]) -> Result<Vec<MixedUp>, Failure> {
    mixed_up_utterances(&run.samples, &run.events, &run.log, manifest)
}

fn row(lang: &str, samples: u64) -> Result<Row, Box<dyn Error>> {
    Ok(serde_json::from_value(json!({
        "lang": lang, "start_sample": 0, "end_sample": samples
    }))?)
}

/// A run of `spans.len()` utterances in Japanese, each read in two repeats 3 s
/// apart; an utterance listed in `wrong` is read as English in both repeats and
/// the others as Japanese. The finals are `file-1`, `file-2`, and so on.
fn mixed_record(
    spans: &[u64],
    wrong: &[u32],
) -> Result<(Vec<Row>, String, String), Box<dyn Error>> {
    let manifest = spans
        .iter()
        .map(|span| row("ja", *span))
        .collect::<Result<Vec<_>, _>>()?;
    let utterances = u32::try_from(spans.len())?;
    let mut samples = Vec::new();
    let mut events = Vec::new();
    for repeat in 1..=2_u32 {
        for utt in 1..=utterances {
            let start = f64::from((repeat - 1) * utterances + (utt - 1)) * 3.0;
            let lang = if wrong.contains(&utt) { "en" } else { "ja" };
            let id = format!("file-{}", samples.len() + 1);
            samples.push(sample_line(repeat, utt, start, start + 1.0));
            events.push(final_line(
                &id,
                lang,
                (start + 0.1, start + 1.2),
                start + 1.6,
            ));
        }
    }
    Ok((manifest, file_of(&samples), file_of(&events)))
}

/// T-BR-10-001
#[test]
fn br10_each_mixed_up_utterance_is_named_with_both_identifiers_answers() -> TestResult {
    let dialog = manifest_of("br10_ip2_manifest.json")?;
    let short = manifest_of("br10_ip7_manifest.json")?;
    assert!(named(&run_of("br10_ip2_pair")?, &dialog)?.is_empty());
    let fast = named(&run_of("br10_ip2_fast")?, &dialog)?;
    let pair = named(&run_of("br10_ip7_pair")?, &short)?;
    // Measured on the records: in the fast run of IP-2, utterance 13
    // (Japanese, 2.4 s) was read as Chinese; in the pair run of IP-7, utterance 3
    // (Japanese, 460 ms) was read as English. Neither has a fallback line, so
    // the log keeps no answer of the identifiers for it.
    let want = |utt, read_as: &str, voice_ms| MixedUp {
        utt,
        manifest_lang: String::from("ja"),
        read_as: String::from(read_as),
        lid: None,
        sv_mark: None,
        step: None,
        voice_ms,
    };
    assert_eq!(fast, vec![want(13, "zh", 2400)]);
    assert_eq!(pair, vec![want(3, "en", 460)]);
    Ok(())
}

/// T-BR-10-002
#[test]
fn the_named_rows_are_as_many_as_the_mixed_up_utterances_of_the_records() -> TestResult {
    let dialog = manifest_of("br10_ip2_manifest.json")?;
    let short = manifest_of("br10_ip7_manifest.json")?;
    let label = "mixed-up utterances";
    // The record of IP-2 holds the pair run, then the fast run.
    let ip2 = numbers_after(&read(&Path::new(RECORDS).join("br10_ip2.md"))?, label);
    let ip7 = numbers_after(&read(&Path::new(RECORDS).join("br10_ip7.md"))?, label);
    assert_eq!(ip2, vec![0, 1]);
    assert_eq!(ip7, vec![1]);
    assert_eq!(named(&run_of("br10_ip2_pair")?, &dialog)?.len(), ip2[0]);
    assert_eq!(named(&run_of("br10_ip2_fast")?, &dialog)?.len(), ip2[1]);
    assert_eq!(named(&run_of("br10_ip7_pair")?, &short)?.len(), ip7[0]);
    Ok(())
}

/// T-BR-10-003
#[test]
fn a_record_without_mix_ups_names_no_utterance() -> TestResult {
    let (manifest, samples, events) = mixed_record(&[16_000, 16_000], &[])?;
    let rows = mixed_up_utterances(&samples, &events, "", &manifest)?;
    assert!(rows.is_empty());
    assert_eq!(mixed_up_text(&rows), "取り違えなし\n");
    // The control: the same record with an utterance read as English is named.
    let (manifest, samples, events) = mixed_record(&[16_000, 16_000], &[2])?;
    let rows = mixed_up_utterances(&samples, &events, "", &manifest)?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].utt, 2);
    Ok(())
}

/// T-BR-10-004
#[test]
fn a_missing_sensevoice_tag_is_written_as_none() -> TestResult {
    let (manifest, samples, events) = mixed_record(&[24_000], &[1])?;
    let rows = mixed_up_utterances(&samples, &events, "", &manifest)?;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].step.as_deref(), rows[0].sv_mark.as_deref()),
        (None, None)
    );
    let text = mixed_up_text(&rows);
    assert!(
        text.contains("SenseVoice なし") && text.contains("step なし"),
        "{text}"
    );
    // The control: a fallback line of the first wrong final gives the step, the
    // answer of whisper-tiny and the language the tag chose.
    let log = file_of(&[fallback_line(1, "sv_tag", Some("zh"))]);
    let rows = mixed_up_utterances(&samples, &events, &log, &manifest)?;
    assert_eq!(rows[0].step.as_deref(), Some("sv_tag"));
    assert_eq!(rows[0].lid.as_deref(), Some("zh"));
    assert_eq!(rows[0].sv_mark.as_deref(), Some("en"));
    Ok(())
}

/// T-BR-10-005
#[test]
fn the_voice_length_is_written_in_milliseconds() -> TestResult {
    let (manifest, samples, events) = mixed_record(&[15_999, 16_000, 32_000], &[1, 2, 3])?;
    let rows = mixed_up_utterances(&samples, &events, "", &manifest)?;
    let lengths: Vec<i64> = rows.iter().map(|row| row.voice_ms).collect();
    assert_eq!(lengths, vec![999, 1000, 2000]);
    Ok(())
}

/// The record of the tool on the records of IP-2 and IP-7.
#[test]
#[ignore = "writes docs/results/in-person/br10_named.md"]
fn the_record_of_br10_is_written() -> TestResult {
    let dialog = manifest_of("br10_ip2_manifest.json")?;
    let short = manifest_of("br10_ip7_manifest.json")?;
    let sections = [
        (
            "IP-2 pair（br10_ip2_pair.*、br10_ip2_manifest.json）",
            "br10_ip2_pair",
            &dialog,
        ),
        (
            "IP-2 fast（br10_ip2_fast.*、br10_ip2_manifest.json）",
            "br10_ip2_fast",
            &dialog,
        ),
        (
            "IP-7 pair（br10_ip7_pair.*、br10_ip7_manifest.json）",
            "br10_ip7_pair",
            &short,
        ),
    ];
    let mut text = String::from("# BR-10: 取り違えた発話の名指し\n\n");
    for (title, prefix, manifest) in sections {
        let rows = named(&run_of(prefix)?, manifest)?;
        text.push_str(&format!("## {title}\n\n{}\n", mixed_up_text(&rows)));
    }
    std::fs::write(Path::new(RECORDS).join("br10_named.md"), text)?;
    Ok(())
}

//! IP-7: replies shorter than a second are read in their own language. This is
//! only recorded: a count over the line is written down as "線の外" and does not
//! fail the test.
//!
//! The real run (T-IP-07-001) plays `lat-short` through the stack of IP-2 and is
//! `#[ignore]`d. The rows that read the real manifest and the real wav files are
//! `#[ignore]`d too, because `testdata/` is not in git. The others test the
//! counting tools (`support/dialog.rs`, the same ones IP-2 uses) on records made
//! by hand. The record file is `docs/results/in-person/ip07.md`.

mod support;

use std::error::Error;
use std::path::PathBuf;

use hayamimi_asr::lid::has_kana;
use hayamimi_eval::error::EvalError;
use hayamimi_eval::manifest::{Order, Row, concat, parse_part, write_rows};
use hayamimi_eval::sha::sha256_hex;
use hayamimi_eval::wav::read_wav;
use serde_json::{Value, json};
use support::dialog::{
    Cell, Verdict, append_line, check_complete, langs_of, manifest_rows, record_dialog_run,
    record_verdict, span_of, switches, tally,
};
use support::dialog_run::run_stack;
use support::{Failure, TestResult, find_above};

/// The provisional line of mixed-up utterances out of 20.
const LINE: usize = 6;
const UTTERANCES: u32 = 20;
const REPEATS: u32 = 3;
/// A reply is short when it spans fewer samples than this: one second at 16 kHz.
const ONE_SECOND: u64 = 16_000;
const RECORD: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/results/in-person/ip07.md"
);

fn is_short(span: u64) -> bool {
    span < ONE_SECOND
}

/// Every row of a manifest spans less than a second, or the first that does not is named.
fn check_all_short(rows: &[Row]) -> Result<(), Failure> {
    for (at, row) in rows.iter().enumerate() {
        let span = span_of(row, at)?;
        if !is_short(span) {
            return Err(format!(
                "row {at} spans {span} samples, not under one second"
            ));
        }
    }
    Ok(())
}

fn row(lang: &str, start: u64, end: u64) -> Result<Row, Box<dyn Error>> {
    Ok(serde_json::from_value(json!({
        "lang": lang, "start_sample": start, "end_sample": end
    }))?)
}

fn manifest() -> Result<Vec<Row>, Failure> {
    manifest_rows(&find_above("testdata/manifests/lat-short.json")?)
}

fn text<'a>(row: &'a Row, key: &str) -> Result<&'a str, Failure> {
    row.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("a row has no string `{key}`"))
}

/// T-IP-07-001
#[test]
#[ignore = "runs the hayamimi binary with the models, which are not in git"]
fn ip07_short_replies_are_read_in_their_own_language() -> TestResult {
    let wav = find_above("testdata/lat-short.wav")?;
    let langs = langs_of(&manifest()?)?;
    let record = PathBuf::from(RECORD);
    std::fs::write(&record, "# IP-7 record\n")?;
    let out = record
        .parent()
        .ok_or("the record has no directory")?
        .join("ip07_pair.jsonl");
    run_stack("pair", &wav, REPEATS, &out)?;
    let samples = support::read(&out)?;
    assert_eq!(check_complete(&samples, REPEATS, UTTERANCES)?, 60);
    record_dialog_run(&record, "pair", &out, &langs, LINE)?;
    Ok(())
}

/// T-IP-07-002
#[test]
#[ignore = "reads testdata or run records, which are not in git"]
fn lat_short_has_twenty_replies_each_shorter_than_one_second() -> TestResult {
    let rows = manifest()?;
    assert_eq!(rows.len(), 20);
    let langs = langs_of(&rows)?;
    for lang in ["ja", "en"] {
        assert_eq!(
            langs.iter().filter(|got| *got == lang).count(),
            10,
            "{lang}"
        );
    }
    check_all_short(&rows)?;
    let mut spans = rows
        .iter()
        .enumerate()
        .map(|(at, row)| span_of(row, at))
        .collect::<Result<Vec<_>, _>>()?;
    spans.sort_unstable();
    let (Some(least), Some(most)) = (spans.first(), spans.last()) else {
        return Err("lat-short has no row".into());
    };
    append_line(
        &PathBuf::from(RECORD),
        &format!(
            "lat-short spans (samples): min {least}, median {}, max {most}",
            spans[spans.len() / 2]
        ),
    )?;
    Ok(())
}

/// T-IP-07-003
#[test]
fn a_reply_of_15999_samples_is_short_and_one_of_16000_is_not() -> TestResult {
    assert!(is_short(15_999));
    assert!(!is_short(16_000));
    assert!(check_all_short(&[row("ja", 0, 15_999)?]).is_ok());
    let said = check_all_short(&[row("ja", 0, 15_999)?, row("en", 20_000, 36_000)?]);
    assert!(
        matches!(&said, Err(message) if message.contains("row 1") && message.contains("16000")),
        "{said:?}"
    );
    Ok(())
}

/// T-IP-07-004
#[test]
fn a_run_forced_to_ja_mixes_up_ten_short_english_replies() -> TestResult {
    let manifest: Vec<String> = (1..=UTTERANCES)
        .map(|utt| String::from(if utt % 2 == 1 { "ja" } else { "en" }))
        .collect();
    let cells: Vec<Cell> = (1..=REPEATS)
        .flat_map(|repeat| {
            (1..=UTTERANCES).map(move |utt| Cell {
                repeat,
                utt,
                langs: vec![String::from("ja")],
            })
        })
        .collect();
    let got = tally(&cells, &manifest)?;
    assert_eq!((got.mixed_up, got.en_as_ja, got.ja_as_en), (10, 10, 0));
    assert!(got.mixed_up > LINE);
    Ok(())
}

/// T-IP-07-005
#[test]
#[ignore = "reads testdata or run records, which are not in git"]
fn lat_short_alternates_ja_and_en_ten_each() -> TestResult {
    let langs = langs_of(&manifest()?)?;
    assert_eq!(langs.first().map(String::as_str), Some("ja"));
    assert_eq!(switches(&langs), 19);
    Ok(())
}

/// Whether a reply is written in the script of its language: Japanese has a
/// kana, English has a Latin letter and neither a kana nor a Han character.
fn written_in(lang: &str, reply: &str) -> bool {
    let han = |c: char| ('\u{4E00}'..='\u{9FFF}').contains(&c);
    match lang {
        "ja" => has_kana(reply),
        "en" => {
            reply.chars().any(|c| c.is_ascii_alphabetic())
                && !has_kana(reply)
                && !reply.chars().any(han)
        }
        _ => false,
    }
}

/// T-IP-07-006
#[test]
#[ignore = "reads testdata or run records, which are not in git"]
fn every_reply_of_lat_short_is_written_in_the_script_of_its_language() -> TestResult {
    assert!(written_in("ja", "はい") && written_in("en", "Yeah"));
    assert!(!written_in("ja", "Yes") && !written_in("en", "はい") && !written_in("en", "OK です"));
    for (at, row) in manifest()?.iter().enumerate() {
        let (lang, reply) = (text(row, "lang")?, text(row, "ref")?);
        assert!(written_in(lang, reply), "row {at}: {reply:?} is not {lang}");
    }
    Ok(())
}

/// A copy of the manifest whose wav paths are absolute (a join reads them from
/// the directory it runs in), with the digest of its first row changed when asked.
fn copy_of_manifest(name: &str, break_first: bool) -> Result<PathBuf, Box<dyn Error>> {
    let mut rows = manifest()?;
    for (at, row) in rows.iter_mut().enumerate() {
        let wav = find_above(text(row, "wav")?)?;
        row.insert(String::from("wav"), json!(wav.to_string_lossy()));
        if break_first && at == 0 {
            row.insert(String::from("sha256"), json!("0".repeat(64)));
        }
    }
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    write_rows(&path, &rows)?;
    Ok(path)
}

/// Joins the replies of a copy of the manifest, interleaved.
fn join_copy(name: &str, break_first: bool) -> Result<(), Box<dyn Error>> {
    let copy = copy_of_manifest(&format!("ip07_{name}.json"), break_first)?;
    let parts = ["ja", "en"].map(|lang| parse_part(&format!("manifest:{}:{lang}", copy.display())));
    let [ja, en] = parts;
    let (out_manifest, out_wav) = (
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("ip07_{name}_joined.json")),
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("ip07_{name}_joined.wav")),
    );
    concat(
        &[ja?, en?],
        800,
        Order::Interleaved,
        &out_manifest,
        &out_wav,
    )?;
    Ok(())
}

/// T-IP-07-007
#[test]
#[ignore = "reads testdata or run records, which are not in git"]
fn every_wav_of_lat_short_matches_its_sha256_and_is_16_khz() -> TestResult {
    for (at, row) in manifest()?.iter().enumerate() {
        let path = find_above(text(row, "wav")?)?;
        let bytes = std::fs::read(&path)?;
        assert_eq!(sha256_hex(&bytes), text(row, "sha256")?, "row {at}");
        let wav = read_wav(&path)?;
        assert_eq!(wav.sample_rate, 16_000, "row {at}");
    }
    join_copy("intact", false)?;
    let said = join_copy("broken", true);
    assert!(
        matches!(said.as_ref().err().and_then(|e| e.downcast_ref::<EvalError>()),
            Some(EvalError::Manifest(message)) if message.contains("sha256")),
        "{said:?}"
    );
    Ok(())
}

/// T-IP-07-008
#[test]
fn six_mixed_up_replies_are_on_the_line_and_seven_are_over_it() -> TestResult {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("ip07_verdicts.txt");
    std::fs::write(&path, "")?;
    assert_eq!(
        record_verdict(&path, "mixed-up replies", 6, LINE)?,
        Verdict::Inside
    );
    assert_eq!(
        record_verdict(&path, "mixed-up replies", 7, LINE)?,
        Verdict::Outside
    );
    let written = support::read(&path)?;
    for word in ["線の内", "線の外", "(線 6)"] {
        assert!(written.contains(word), "{word} is not in: {written}");
    }
    Ok(())
}

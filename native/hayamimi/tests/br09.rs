//! BR-9: the repeats of IP-7 that got no final are told apart by why, and the
//! utterances the SenseVoice tag decided are given their `L_asr` and the answer
//! of whisper-tiny (design 10.13.13, 21). The tools are in `support/quality.rs`.
//!
//! The rows that read a record read the copy of the record of IP-7 under
//! `docs/results/in-person/br09_ip7*`, a name no run writes. The other rows feed
//! the tools a record made by hand (`support/hand.rs`): utterances 3 s apart, and
//! finals that stand for a stretch of the stream and are placed by the end of it.
//! Nothing here changes what a run does: the numbers are counted from its text.
//!
//! The record of the tools is `docs/results/in-person/br09_reasons.md`; the row
//! that writes it is `#[ignore]`d and runs with `-- --ignored`.

mod support;

use std::path::Path;

use support::hand::{fallback_line, file_of, final_line, sample_line, unread_line};
use support::quality::{
    NoFinal, count_of, no_final_text, numbers_after, repeats_without_final, split_no_final,
    sv_tag_rows, sv_tag_text, the_number_after,
};
use support::{Failure, TestResult, read};

const RECORDS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/results/in-person");

/// The copy of the record of IP-7: what the run left, and the record file that
/// counted it.
struct Ip7 {
    samples: String,
    events: String,
    log: String,
    record: String,
}

fn ip7() -> Result<Ip7, Failure> {
    let copy = |name: &str| read(&Path::new(RECORDS).join(name));
    Ok(Ip7 {
        samples: copy("br09_ip7_pair.jsonl")?,
        events: copy("br09_ip7_pair.events.jsonl")?,
        log: copy("br09_ip7_pair.log")?,
        record: copy("br09_ip7.md")?,
    })
}

/// Four utterances of one repeat, 3 s apart and 1 s long. One final stands for
/// 0.5 s to 3.2 s: it is placed by 3.2 s, so it belongs to utterance 2, and it
/// overlaps the voice of utterance 1. One segment read as nothing, 6.2 s to
/// 6.8 s, overlaps utterance 3. Nothing is near utterance 4.
fn reasons_record() -> (String, String, String) {
    let samples: Vec<String> = (1..=4)
        .map(|utt| {
            let start = f64::from(utt - 1) * 3.0;
            sample_line(1, utt, start, start + 1.0)
        })
        .collect();
    let events = vec![final_line("file-1", "ja", (0.5, 3.2), 4.5)];
    let log = vec![unread_line(6.2, 6.8)];
    (file_of(&samples), file_of(&events), file_of(&log))
}

/// `count` utterances of one repeat, 3 s apart, and nothing else: each is a
/// repeat with no final.
fn silent_samples(count: u32) -> String {
    let lines: Vec<String> = (1..=count)
        .map(|utt| {
            let start = f64::from(utt) * 3.0;
            sample_line(1, utt, start, start + 1.0)
        })
        .collect();
    file_of(&lines)
}

/// T-BR-09-001
#[test]
fn br09_each_repeat_without_a_final_is_given_its_reason() -> TestResult {
    let ip7 = ip7()?;
    let reported = the_number_after(&ip7.record, "repeats with no final")?;
    let repeats = split_no_final(&ip7.samples, &ip7.events, &ip7.log, reported)?;
    assert_eq!(repeats.len(), 16);
    let mut named: Vec<(u32, u32)> = repeats.iter().map(|got| (got.repeat, got.utt)).collect();
    named.sort_unstable();
    named.dedup();
    assert_eq!(named.len(), 16, "each of the 16 is named by repeat and utt");
    // Measured on the record: the 12 repeats of utterances 1, 5, 7
    // and 14 and the 4 of utterance 19 (repeats 1 and 2), utterance 2 (repeat 3)
    // and utterance 18 (repeat 3) have no final that overlaps their voice.
    assert_eq!(count_of(&repeats, NoFinal::NoSegment), 16);
    assert_eq!(count_of(&repeats, NoFinal::ReadEmpty), 0);
    assert_eq!(count_of(&repeats, NoFinal::Elsewhere), 0);
    Ok(())
}

/// T-BR-09-002
#[test]
fn the_three_reasons_add_up_to_the_repeats_with_no_final_of_the_record() -> TestResult {
    let ip7 = ip7()?;
    let reported = the_number_after(&ip7.record, "repeats with no final")?;
    assert_eq!(reported, 16);
    let repeats = repeats_without_final(&ip7.samples, &ip7.events, &ip7.log)?;
    let sum: usize = NoFinal::ALL
        .iter()
        .map(|why| count_of(&repeats, *why))
        .sum();
    assert_eq!(sum, reported);
    Ok(())
}

/// T-BR-09-003
#[test]
fn reasons_that_do_not_add_up_to_the_repeats_without_a_final_are_an_error() -> TestResult {
    let samples = silent_samples(15);
    let error = split_no_final(&samples, "", "", 16)
        .err()
        .ok_or("reasons that do not add up must be an error")?;
    assert!(error.contains("15") && error.contains("16"), "{error}");
    // The control: the same record, told that it has 15, is accepted.
    assert_eq!(split_no_final(&samples, "", "", 15)?.len(), 15);
    Ok(())
}

/// T-BR-09-004
#[test]
fn each_reason_is_told_apart_on_a_hand_made_record() -> TestResult {
    let (samples, events, log) = reasons_record();
    let repeats = repeats_without_final(&samples, &events, &log)?;
    let named: Vec<(u32, u32, NoFinal)> = repeats
        .iter()
        .map(|got| (got.repeat, got.utt, got.why))
        .collect();
    assert_eq!(
        named,
        vec![
            (1, 1, NoFinal::Elsewhere),
            (1, 3, NoFinal::ReadEmpty),
            (1, 4, NoFinal::NoSegment),
        ]
    );
    Ok(())
}

/// T-BR-09-005
#[test]
fn each_sv_tag_fallback_is_written_with_its_wait_and_its_lid_answer() -> TestResult {
    let ip7 = ip7()?;
    let rows = sv_tag_rows(&ip7.samples, &ip7.events, &ip7.log)?;
    let recorded = the_number_after(&ip7.record, "asr.lang.fallback sv_tag")?;
    assert_eq!(rows.len(), 14);
    assert_eq!(rows.len(), recorded);
    for row in &rows {
        assert!(row.l_asr_ms > 0.0, "{} has L_asr {}", row.id, row.l_asr_ms);
        assert!(row.lid.is_some(), "{} has no lid answer", row.id);
    }
    let mut named: Vec<(u32, u32)> = rows.iter().map(|row| (row.repeat, row.utt)).collect();
    named.sort_unstable();
    named.dedup();
    assert_eq!(
        named.len(),
        14,
        "each row is a different repeat of an utterance"
    );
    Ok(())
}

/// T-BR-09-006
#[test]
fn a_record_without_sv_tag_lines_says_so() -> TestResult {
    let (samples, events, log) = reasons_record();
    let rows = sv_tag_rows(&samples, &events, &log)?;
    assert!(rows.is_empty());
    assert_eq!(sv_tag_text(&rows), "sv_tag なし\n");
    // The controls: a record with a fallback line names it, and so does the real one.
    let tagged = file_of(&[fallback_line(1, "sv_tag", Some("zh"))]);
    let named = sv_tag_rows(&samples, &events, &tagged)?;
    assert_eq!(named.len(), 1);
    assert_eq!(named[0].lid.as_deref(), Some("zh"));
    let ip7 = ip7()?;
    let real = sv_tag_rows(&ip7.samples, &ip7.events, &ip7.log)?;
    assert!(sv_tag_text(&real).contains("sv_tag: 14"));
    Ok(())
}

/// T-BR-09-007
#[test]
fn the_first_matching_reason_in_order_is_the_only_one_counted() -> TestResult {
    // Utterance 1 has the final that was given elsewhere and a segment that read
    // as nothing, both over its voice: the reason is the first one.
    let (samples, events, _) = reasons_record();
    let log = file_of(&[unread_line(0.2, 0.8)]);
    let repeats = repeats_without_final(&samples, &events, &log)?;
    let first = repeats
        .iter()
        .find(|got| got.utt == 1)
        .ok_or("utterance 1 has a final")?;
    assert_eq!(first.why, NoFinal::Elsewhere);
    assert_eq!(count_of(&repeats, NoFinal::ReadEmpty), 0);
    Ok(())
}

/// T-BR-09-008
#[test]
fn reasons_that_add_up_above_the_repeats_without_a_final_are_an_error() -> TestResult {
    let samples = silent_samples(17);
    let error = split_no_final(&samples, "", "", 16)
        .err()
        .ok_or("reasons that do not add up must be an error")?;
    assert!(error.contains("17") && error.contains("16"), "{error}");
    // The record of 004 says 3 and adds up to 3, and counts three reasons only.
    let (samples, events, log) = reasons_record();
    let repeats = split_no_final(&samples, &events, &log, 3)?;
    assert_eq!(repeats.len(), 3);
    assert_eq!(NoFinal::ALL.len(), 3);
    assert_eq!(no_final_text(&repeats).matches("和: 3").count(), 1);
    Ok(())
}

/// A line of a record file that is not there is an error, and is not zero.
#[test]
fn a_record_line_that_is_not_there_is_an_error_and_not_a_zero() {
    let record = "repeats with no final: 16\nmixed-up utterances: 0 (線 2) 線の内\nmixed-up utterances: 1 (線 2) 線の内\n";
    assert_eq!(the_number_after(record, "repeats with no final"), Ok(16));
    assert!(the_number_after(record, "utterances with no final").is_err());
    assert!(the_number_after(record, "mixed-up utterances").is_err());
    assert_eq!(numbers_after(record, "mixed-up utterances"), vec![0, 1]);
}

/// The record of the tools on the record of IP-7.
#[test]
#[ignore = "writes docs/results/in-person/br09_reasons.md"]
fn the_record_of_br09_is_written() -> TestResult {
    let ip7 = ip7()?;
    let reported = the_number_after(&ip7.record, "repeats with no final")?;
    let repeats = split_no_final(&ip7.samples, &ip7.events, &ip7.log, reported)?;
    let rows = sv_tag_rows(&ip7.samples, &ip7.events, &ip7.log)?;
    let text = format!(
        "# BR-9: IP-7 の final の無い繰り返しと sv_tag の内訳\n\n\
         記録の複写: br09_ip7_pair.jsonl、br09_ip7_pair.events.jsonl、br09_ip7_pair.log、br09_ip7.md\n\
         repeats with no final（記録の値）: {reported}\n\n\
         ## final の無い繰り返し（設計 10.13.13 の 21 の順）\n\n{}\n\
         ## sv_tag（発話ごとの L_asr と whisper-tiny の答え）\n\n{}",
        no_final_text(&repeats),
        sv_tag_text(&rows)
    );
    std::fs::write(Path::new(RECORDS).join("br09_reasons.md"), text)?;
    Ok(())
}

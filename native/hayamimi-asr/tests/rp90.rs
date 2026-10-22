//! RP-90: with a cap on the length of one decode, the Japanese tier keeps the
//! characters of a long buffer that it drops when it reads the buffer in one
//! call.
//!
//! The long material is the one of RP-89 (`testdata/long_ja`, six files of 20
//! to 21 s). Each file is decoded under a cap of 10 s, which cuts it into three
//! pieces, and with no cap; the three clips of its group are decoded one by one
//! as the reference of what the model can read. The test also checks the
//! functions that join the sentences of the pieces and judge the numbers. The
//! material, the models and the audio are not in git; a worktree reads them
//! from the main checkout.
//!
//!     cargo test --release -p hayamimi-asr --test rp90 -- --ignored --nocapture

mod support;

use std::error::Error;
use std::path::Path;

use hayamimi_asr::audio::read_wav;
use hayamimi_asr::cap::join_sentences;
use hayamimi_asr::recognizer::Recognizer;
use hayamimi_asr::{DecodeCap, LangTag, Recognizers, Tier};
use hayamimi_eval::cer::{Cer, normalize_ja};
use support::long::{GAPS, GROUPS, Gap, Group, Side, THREADS, measure, measure_each};
use support::{Failure, TestResult, models_dir, testdata_dir};

/// The cap and the overlap of the acceptance run, in seconds. They are given
/// on purpose: the run does not depend on the default of the build.
const CAP_SECONDS: f64 = 10.0;
const OVERLAP_SECONDS: f64 = 0.0;

/// RP-90: the capped error is at most this much above the clips decoded one
/// by one (2.0 pt, in millionths).
const WITHIN_MICROS: i128 = 20_000;

fn capped() -> Result<DecodeCap, Box<dyn Error>> {
    Ok(DecodeCap::from_seconds(CAP_SECONDS, OVERLAP_SECONDS)?)
}

// --- the judgements of RP-90 --------------------------------------------------

/// The capped error is within `WITHIN_MICROS` of the clips decoded one by one
/// and strictly below the uncapped error.
fn judge_error(capped: u64, each: u64, uncapped: u64) -> Result<(), Failure> {
    let over = i128::from(capped) - i128::from(each);
    if over > WITHIN_MICROS {
        return Err(format!(
            "the capped decode ({capped} micros) is {over} micros above the clips decoded one \
             by one ({each} micros); {WITHIN_MICROS} are allowed"
        ));
    }
    match capped.cmp(&uncapped) {
        std::cmp::Ordering::Less => Ok(()),
        std::cmp::Ordering::Equal => Err(format!(
            "the capped decode ({capped} micros) is equal to the uncapped decode ({uncapped} micros)"
        )),
        std::cmp::Ordering::Greater => Err(format!(
            "the capped decode ({capped} micros) is above the uncapped decode ({uncapped} micros)"
        )),
    }
}

/// Every buffer keeps at least 0.8 of the characters of its reference. A
/// row is the name of the buffer, the characters it read and its reference's.
fn judge_characters(rows: &[(String, usize, usize)]) -> Result<(), Failure> {
    for (label, read, reference) in rows {
        if read * 10 < reference * 8 {
            return Err(format!(
                "{label} keeps {read} of {reference} characters; 0.8 are needed"
            ));
        }
    }
    Ok(())
}

// --- decoding and counting ----------------------------------------------------

/// One buffer of the long material read under the cap, with no cap, and the
/// clips of its group one by one.
struct Row {
    label: String,
    capped: Side,
    uncapped: Side,
    each: Side,
    reference_chars: usize,
    /// The first sample of every piece but the first.
    cuts: Vec<usize>,
}

fn recognizers(cap: DecodeCap) -> Result<Recognizers, Failure> {
    Ok(Recognizers::new(&models_dir()?, THREADS, None, cap))
}

/// Where the cap cuts `samples`; an error when it does not cut them.
fn cut_points(cap: DecodeCap, samples: &[f32]) -> Result<Vec<usize>, Box<dyn Error>> {
    let Some(ranges) = cap.plan(Tier::Rz, samples) else {
        return Err("the buffer is within the cap, so nothing is cut".into());
    };
    Ok(ranges.iter().skip(1).map(|range| range.start).collect())
}

struct Readers {
    capped: Recognizers,
    uncapped: Recognizers,
}

fn measure_row(
    readers: &mut Readers,
    testdata: &Path,
    group: &Group,
    gap: &Gap,
    each: Side,
) -> Result<Row, Box<dyn Error>> {
    let samples = read_wav(&group.material(testdata, gap))?;
    let reference = group.reference();
    Ok(Row {
        label: group.label(gap),
        capped: measure(&mut readers.capped, &samples, &reference)?,
        uncapped: measure(&mut readers.uncapped, &samples, &reference)?,
        each,
        reference_chars: group.chars,
        cuts: cut_points(capped()?, &samples)?,
    })
}

fn measure_all(testdata: &Path) -> Result<Vec<Row>, Box<dyn Error>> {
    let mut readers = Readers {
        capped: recognizers(capped()?)?,
        uncapped: recognizers(DecodeCap::Off)?,
    };
    let mut rows = Vec::new();
    for group in &GROUPS {
        let each = measure_each(&mut readers.uncapped, testdata, group)?;
        for gap in &GAPS {
            rows.push(measure_row(&mut readers, testdata, group, gap, each)?);
        }
    }
    Ok(rows)
}

fn micros_of(side: Side) -> String {
    let cer = side.cer;
    format!("{} micros ({}/{})", cer.micros(), cer.dist, cer.denom)
}

fn report(row: &Row) {
    println!(
        "{}: uncapped {}, one by one {}, capped {}; {} of {} characters; cuts at {:?}",
        row.label,
        micros_of(row.uncapped),
        micros_of(row.each),
        micros_of(row.capped),
        row.capped.chars,
        row.reference_chars,
        row.cuts,
    );
}

/// The summed measurements: capped, uncapped, one by one (dist and denom
/// added, then divided).
fn totals(rows: &[Row]) -> (Cer, Cer, Cer) {
    rows.iter().fold(
        (Cer::default(), Cer::default(), Cer::default()),
        |(capped, uncapped, each), row| {
            (
                capped.plus(row.capped.cer),
                uncapped.plus(row.uncapped.cer),
                each.plus(row.each.cer),
            )
        },
    )
}

// --- acceptance ---------------------------------------------------------------

#[test]
#[ignore = "reads testdata/long_ja and the models, which are not in git"]
fn rp90_a_capped_decode_keeps_the_characters_of_a_long_buffer() -> TestResult {
    let rows = measure_all(&testdata_dir()?)?;
    rows.iter().for_each(report);
    let (capped, uncapped, each) = totals(&rows);
    println!(
        "total: capped {} micros ({}/{}); uncapped {} micros ({}/{}); one by one {} micros ({}/{})",
        capped.micros(),
        capped.dist,
        capped.denom,
        uncapped.micros(),
        uncapped.dist,
        uncapped.denom,
        each.micros(),
        each.dist,
        each.denom,
    );
    judge_error(capped.micros(), each.micros(), uncapped.micros())?;
    let characters: Vec<(String, usize, usize)> = rows
        .iter()
        .map(|row| (row.label.clone(), row.capped.chars, row.reference_chars))
        .collect();
    judge_characters(&characters)?;
    Ok(())
}

#[test]
#[ignore = "reads testdata/long_ja and the models, which are not in git"]
fn the_uncapped_decode_fails_the_same_judgement() -> TestResult {
    let rows = measure_all(&testdata_dir()?)?;
    let (_, uncapped, each) = totals(&rows);
    let said = judge_error(uncapped.micros(), each.micros(), uncapped.micros()).err();
    println!(
        "uncapped {} micros against one by one {} micros: {said:?}",
        uncapped.micros(),
        each.micros()
    );
    assert!(
        said.as_deref()
            .is_some_and(|text| text.contains("above the clips decoded one by one")),
        "{said:?}"
    );
    Ok(())
}

#[test]
#[ignore = "reads testdata/long_ja and the models, which are not in git"]
fn decode_and_decode_tagged_return_the_same_joined_text() -> TestResult {
    let testdata = testdata_dir()?;
    let samples = read_wav(&GROUPS[2].material(&testdata, &GAPS[1]))?;
    let cap = capped()?;
    cut_points(cap, &samples)?;
    let recognizer = Recognizer::open_tier(&models_dir()?, Tier::Rz, THREADS, cap)?;
    let plain = recognizer.decode(&samples)?;
    let (tagged, tag) = recognizer.decode_tagged(&samples)?;
    assert_eq!(plain, tagged);
    assert_eq!(tag, LangTag::NotApplicable);
    assert!(!plain.trim().is_empty());
    Ok(())
}

// --- the checks that need neither the models nor the audio ---------------------

fn says(said: Option<String>, part: &str) -> bool {
    said.as_deref().is_some_and(|text| text.contains(part))
}

#[test]
fn a_capped_error_2_points_above_the_clips_is_accepted() {
    assert_eq!(judge_error(120_000, 100_000, 200_000), Ok(()));
}

#[test]
fn a_capped_error_one_millionth_above_the_line_is_rejected() {
    let said = judge_error(120_001, 100_000, 200_000).err();
    assert!(says(said, "20001"));
}

#[test]
fn a_capped_error_equal_to_the_uncapped_error_is_rejected() {
    let said = judge_error(120_000, 100_000, 120_000).err();
    assert!(says(said, "equal to the uncapped decode"));
    assert_eq!(judge_error(120_000, 100_000, 120_001), Ok(()));
}

fn rows_with(label: &str, read: usize) -> Vec<(String, usize, usize)> {
    let mut rows: Vec<(String, usize, usize)> = GROUPS
        .iter()
        .flat_map(|group| {
            GAPS.iter()
                .map(|gap| (group.label(gap), group.chars, group.chars))
        })
        .collect();
    for row in rows.iter_mut().filter(|row| row.0 == label) {
        row.1 = read;
    }
    rows
}

#[test]
fn a_group_that_keeps_45_of_56_characters_passes() {
    assert_eq!(judge_characters(&[(String::from("L1"), 45, 56)]), Ok(()));
}

#[test]
fn one_group_at_44_of_56_characters_is_named() {
    assert_eq!(judge_characters(&rows_with("L1 gap 500 ms", 56)), Ok(()));
    let said = judge_characters(&rows_with("L1 gap 200 ms", 44)).err();
    assert!(says(said, "L1 gap 200 ms keeps 44 of 56 characters"));
}

#[test]
fn a_join_that_drops_a_piece_is_caught() {
    let [first, second, third] = GROUPS[2].refs;
    let characters = |sentences: &[&str]| normalize_ja(&sentences.concat()).len();
    let dropped = characters(&[first, third]);
    assert_eq!(dropped, 45);
    let said = judge_characters(&[(String::from("L3"), dropped, GROUPS[2].chars)]).err();
    assert!(says(said, "L3 keeps 45 of 62 characters"));
    let kept = characters(&[first, second, third]);
    assert_eq!(judge_characters(&[(String::from("L3"), kept, 62)]), Ok(()));
}

#[test]
fn japanese_sentences_are_joined_without_a_space() {
    assert_eq!(
        join_sentences(&["こんにちは", "世界"], false),
        "こんにちは世界"
    );
}

#[test]
fn english_words_at_a_piece_boundary_get_one_space() {
    assert_eq!(join_sentences(&["hello", "world"], false), "hello world");
}

#[test]
fn a_space_needs_ascii_alphanumerics_on_both_sides() {
    assert_eq!(join_sentences(&["hello", "世界"], false), "hello世界");
    assert_eq!(
        join_sentences(&["こんにちは", "world"], false),
        "こんにちはworld"
    );
}

#[test]
fn blank_and_empty_sentences_are_dropped_before_joining() {
    assert_eq!(
        join_sentences(&["  hello ", "", "world  "], false),
        "hello world"
    );
    assert_eq!(join_sentences(&["a", "", "b"], false), "a b");
}

#[test]
fn only_ascii_letters_and_digits_count_at_a_piece_boundary() {
    assert_eq!(join_sentences(&["hello.", "world"], false), "hello.world");
    assert_eq!(join_sentences(&["abc", "123"], false), "abc 123");
    assert_eq!(join_sentences(&["10", "人"], false), "10人");
}

#[test]
fn a_two_character_overlap_is_folded() {
    assert_eq!(
        join_sentences(&["今日は晴れ", "晴れです"], true),
        "今日は晴れです"
    );
    assert_eq!(
        join_sentences(&["今日は晴れ", "晴れです"], false),
        "今日は晴れ晴れです"
    );
}

#[test]
fn a_one_character_overlap_is_not_folded() {
    assert_eq!(join_sentences(&["今日は", "はい"], true), "今日ははい");
}

#[test]
fn an_overlap_of_six_characters_is_folded_and_one_of_seven_is_not() {
    assert_eq!(
        join_sentences(&["んあいうえおか", "あいうえおかす"], true),
        "んあいうえおかす"
    );
    assert_eq!(
        join_sentences(&["んあいうえおかき", "あいうえおかきす"], true),
        "んあいうえおかきあいうえおかきす"
    );
}

#[test]
fn the_longest_overlap_is_the_one_folded() {
    assert_eq!(join_sentences(&["xxabab", "ababyy"], true), "xxababyy");
}

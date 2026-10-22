//! RP-89: the Japanese tier, decoding a long buffer in one call with no cap,
//! drops characters that it reads when the same sentences are decoded one clip
//! at a time.
//!
//! The long material is three groups of three clips of `testdata/eval_real`,
//! joined with 0.2 s and with 0.5 s of silence between the clips (six files in
//! `testdata/long_ja`). For each file the test decodes the whole buffer once
//! and the three clips one by one, scores both against the same reference, and
//! compares the two error rates with the line of RP-89. The material, the
//! models and the audio are not in git; a worktree reads them from the main
//! checkout.
//!
//!     cargo test --release -p hayamimi-asr --test rp89 -- --ignored --nocapture

mod support;

use std::error::Error;
use std::path::Path;

use hayamimi_asr::audio::{SAMPLE_RATE, read_wav};
use hayamimi_asr::{Recognizers, Tier};
use hayamimi_eval::cer::{Cer, cer, normalize_ja};
use hayamimi_eval::wav::read_wav as read_pcm;
use serde_json::Value;
use support::{Failure, TARGET, TestResult, models_dir, read_json, testdata_dir};

const THREADS: i32 = 2;

/// RP-89: the whole decode is at least this much worse (5.0 pt, in millionths).
const DROP_MICROS: i128 = 50_000;

/// RP-89, a buffer within the cap: the difference stays below this (1.0 pt).
const NO_DROP_BELOW_MICROS: i128 = 10_000;

/// A group of three clips, in the order they are joined.
struct Group {
    name: &'static str,
    clips: [&'static str; 3],
    refs: [&'static str; 3],
    /// Reference characters after normalisation.
    chars: usize,
    /// The sample count of the joined file for each gap of `GAPS`.
    lengths: [usize; 2],
}

const GROUPS: [Group; 3] = [
    Group {
        name: "L1",
        clips: ["ja_10", "ja_13", "ja_15"],
        refs: [
            "勝利はしたものの１０万人以上の戦死者を出していた。",
            "高校生２００人に聞きました。",
            "模擬裁判に参加した大学１年生の本音とは。",
        ],
        chars: 56,
        lengths: [323_218, 332_818],
    },
    Group {
        name: "L2",
        clips: ["ja_05", "ja_08", "ja_07"],
        refs: [
            "でも敦賀さん、９１歳力強い走りに変わっていきます。",
            "けさの東京、風はなさそうですけどね、体感はどうでしょうか、近藤さん。",
            "しっかり打ち切れるかです。",
        ],
        chars: 65,
        lengths: [321_116, 330_716],
    },
    Group {
        name: "L3",
        clips: ["ja_03", "ja_09", "ja_04"],
        refs: [
            "そこの上流から丸太を組んでいかだにしてどんどん流したんですこれを。",
            "そしてもう一つ大きな勢力があります。",
            "今、１００ｍのターンを迎える。",
        ],
        chars: 62,
        lengths: [329_519, 339_119],
    },
];

/// A silence between clips: its length in milliseconds (it names the file)
/// and in samples.
struct Gap {
    millis: u32,
    samples: usize,
}

const GAPS: [Gap; 2] = [
    Gap {
        millis: 200,
        samples: 3_200,
    },
    Gap {
        millis: 500,
        samples: 8_000,
    },
];

impl Group {
    /// The reference of the joined buffer: the three references, in order,
    /// with nothing between them.
    fn reference(&self) -> String {
        self.refs.concat()
    }
}

// --- the shape of the material ------------------------------------------------

/// The `gap` samples at the start of `rest` (the silence that comes before the
/// clip `next`) are all zero and exactly `gap` long.
fn check_gap(rest: &[i16], gap: usize, number: usize, next: &[i16]) -> Result<(), Failure> {
    let window = &rest[..gap.min(rest.len())];
    let Some(offset) = window.iter().position(|sample| *sample != 0) else {
        if window.len() == gap {
            return Ok(());
        }
        return Err(format!("the buffer ends inside gap {number}"));
    };
    if rest[offset..].starts_with(next) {
        return Err(format!("gap {number} is {offset} samples long, not {gap}"));
    }
    Err(format!(
        "gap {number} sample {} is {}, not 0",
        offset + 1,
        window[offset]
    ))
}

/// The samples of `joined` from `at` are the clip `clip`; the position after it.
fn check_clip(joined: &[i16], at: usize, clip: &[i16], number: usize) -> Result<usize, Failure> {
    let end = at + clip.len();
    let Some(found) = joined.get(at..end) else {
        return Err(format!("the buffer ends inside clip {number}"));
    };
    if let Some(index) = found.iter().zip(clip).position(|(got, want)| got != want) {
        return Err(format!(
            "clip {number} sample {} is {}, not {}",
            index + 1,
            found[index],
            clip[index]
        ));
    }
    Ok(end)
}

/// `joined` is the clips in order with `gap` zero samples between them and
/// nothing else, front to back.
fn walk(parts: &[&[i16]], gap: usize, joined: &[i16]) -> Result<(), Failure> {
    let mut at = 0;
    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            check_gap(&joined[at.min(joined.len())..], gap, index, part)?;
            at += gap;
        }
        at = check_clip(joined, at, part, index + 1)?;
    }
    if at != joined.len() {
        return Err(format!(
            "{} samples follow the last clip",
            joined.len() - at
        ));
    }
    Ok(())
}

/// `joined` is `parts` with exactly `gap` zero samples between neighbours, no
/// silence before the first clip and none after the last, and every clip
/// sample equal to its source.
fn check_join(parts: &[&[i16]], gap: usize, joined: &[i16]) -> Result<(), Failure> {
    let body =
        parts.iter().map(|part| part.len()).sum::<usize>() + gap * parts.len().saturating_sub(1);
    if joined.len() > body {
        let extra = joined.len() - body;
        if walk(parts, gap, &joined[extra..]).is_ok() {
            return Err(format!("{extra} samples before the first clip"));
        }
        if walk(parts, gap, &joined[..body]).is_ok() {
            return Err(format!("{extra} samples after the last clip"));
        }
    }
    walk(parts, gap, joined)
}

/// The samples of one joined file of `dir`; the error names the path.
fn read_material(dir: &Path, group: &Group, gap: &Gap) -> Result<Vec<i16>, Failure> {
    let path = dir.join(format!("{}_g{}.wav", group.name, gap.millis));
    read_pcm(&path)
        .map(|wav| wav.samples)
        .map_err(|error| format!("{}: {error}", path.display()))
}

// --- the judgement of RP-89 ---------------------------------------------------

/// How much worse the whole decode is than the one-by-one decode, in millionths.
fn difference(whole: u64, each: u64) -> i128 {
    i128::from(whole) - i128::from(each)
}

/// RP-89: the whole decode is worse by `DROP_MICROS` or more.
fn judge_drop(whole: u64, each: u64) -> Result<(), Failure> {
    let diff = difference(whole, each);
    if diff >= DROP_MICROS {
        return Ok(());
    }
    Err(format!(
        "the whole decode ({whole} micros) is {diff} micros worse than the one-by-one decode \
         ({each} micros); {DROP_MICROS} are needed"
    ))
}

/// RP-89, a buffer within the cap: the difference stays below `NO_DROP_BELOW_MICROS`.
fn judge_no_drop(whole: u64, each: u64) -> Result<(), Failure> {
    let diff = difference(whole, each);
    if diff < NO_DROP_BELOW_MICROS {
        return Ok(());
    }
    Err(format!(
        "the whole decode ({whole} micros) is {diff} micros worse than the one-by-one decode \
         ({each} micros); a difference of {NO_DROP_BELOW_MICROS} or more shows a drop"
    ))
}

// --- decoding and counting ----------------------------------------------------

/// An error measurement and the number of characters the decoded text holds.
#[derive(Clone, Copy, Default)]
struct Side {
    cer: Cer,
    chars: usize,
}

impl Side {
    fn plus(self, other: Side) -> Side {
        Side {
            cer: self.cer.plus(other.cer),
            chars: self.chars + other.chars,
        }
    }
}

/// One buffer of the long material: its whole decode against the one-by-one
/// decode of its clips, both scored against the reference of the group.
struct Pair {
    label: String,
    whole: Side,
    each: Side,
    reference_chars: usize,
}

fn recognizers() -> Result<Recognizers, Failure> {
    Ok(Recognizers::new(&models_dir()?, THREADS, None))
}

/// The error of the whole-buffer decode of `samples` against `reference`.
fn measure(
    recognizers: &mut Recognizers,
    samples: &[f32],
    reference: &str,
) -> Result<Side, Box<dyn Error>> {
    let decoded = recognizers.decode_whole(Tier::Rz, samples)?;
    if decoded.tier != Tier::Rz {
        return Err(format!("the Japanese tier did not answer; {:?} did", decoded.tier).into());
    }
    Ok(Side {
        cer: cer(reference, &decoded.text),
        chars: normalize_ja(&decoded.text).len(),
    })
}

/// The samples of the clip `name` of `testdata/eval_real`.
fn eval_clip(testdata: &Path, name: &str) -> Result<Vec<f32>, Box<dyn Error>> {
    Ok(read_wav(
        &testdata.join("eval_real").join(format!("{name}.wav")),
    )?)
}

/// The clips decoded one by one, each against its own reference, summed.
fn measure_each(
    recognizers: &mut Recognizers,
    testdata: &Path,
    group: &Group,
) -> Result<Side, Box<dyn Error>> {
    let mut total = Side::default();
    for (name, reference) in group.clips.iter().zip(group.refs) {
        let samples = eval_clip(testdata, name)?;
        total = total.plus(measure(recognizers, &samples, reference)?);
    }
    Ok(total)
}

/// Both joined buffers of a group, each against the one-by-one decode.
fn measure_group(
    recognizers: &mut Recognizers,
    testdata: &Path,
    group: &Group,
) -> Result<Vec<Pair>, Box<dyn Error>> {
    let each = measure_each(recognizers, testdata, group)?;
    let mut pairs = Vec::with_capacity(GAPS.len());
    for gap in &GAPS {
        let path = testdata
            .join("long_ja")
            .join(format!("{}_g{}.wav", group.name, gap.millis));
        let whole = measure(recognizers, &read_wav(&path)?, &group.reference())?;
        pairs.push(Pair {
            label: format!("{} gap {} ms", group.name, gap.millis),
            whole,
            each,
            reference_chars: group.chars,
        });
    }
    Ok(pairs)
}

/// The summed measurements of the whole decodes and of the one-by-one decodes
/// (dist and denom added, then divided).
fn totals(pairs: &[Pair]) -> (Cer, Cer) {
    pairs
        .iter()
        .fold((Cer::default(), Cer::default()), |(whole, each), pair| {
            (whole.plus(pair.whole.cer), each.plus(pair.each.cer))
        })
}

fn report(pair: &Pair) {
    let (whole, each) = (pair.whole.cer, pair.each.cer);
    println!(
        "{}: whole {} micros ({}/{}), {} of {} characters; one by one {} micros ({}/{}); \
         difference {}",
        pair.label,
        whole.micros(),
        whole.dist,
        whole.denom,
        pair.whole.chars,
        pair.reference_chars,
        each.micros(),
        each.dist,
        each.denom,
        difference(whole.micros(), each.micros()),
    );
}

// --- acceptance ---------------------------------------------------------------

#[test]
#[ignore = "reads testdata/long_ja and the models, which are not in git"]
fn rp89_the_whole_decode_drops_characters_on_a_long_buffer() -> TestResult {
    let testdata = testdata_dir()?;
    let mut recognizers = recognizers()?;
    let mut pairs = Vec::new();
    for group in &GROUPS {
        pairs.extend(measure_group(&mut recognizers, &testdata, group)?);
    }
    pairs.iter().for_each(report);
    let (whole, each) = totals(&pairs);
    println!(
        "total: whole {} micros ({}/{}); one by one {} micros ({}/{}); difference {}",
        whole.micros(),
        whole.dist,
        whole.denom,
        each.micros(),
        each.dist,
        each.denom,
        difference(whole.micros(), each.micros()),
    );
    judge_drop(whole.micros(), each.micros())?;
    Ok(())
}

/// The sentences of `ja_01` to `ja_08` of the evaluation set, in order.
fn first_eight_references(testdata: &Path) -> Result<Vec<String>, Box<dyn Error>> {
    let Value::Array(rows) = read_json(&testdata.join("eval_real").join("manifest.json"))? else {
        return Err("eval_real/manifest.json is not a list".into());
    };
    let mut references = Vec::new();
    for number in 1..=8 {
        let name = format!("ja_{number:02}.wav");
        let Some(row) = rows.iter().find(|row| row["wav"] == name.as_str()) else {
            return Err(format!("eval_real/manifest.json has no {name}").into());
        };
        let Some(reference) = row["ref"].as_str() else {
            return Err(format!("{name} has no `ref`").into());
        };
        references.push(reference.to_owned());
    }
    Ok(references)
}

#[test]
#[ignore = "reads testdata/eval_real and the models, which are not in git"]
fn the_existing_51_second_buffer_shows_the_same_drop() -> TestResult {
    let testdata = testdata_dir()?;
    let references = first_eight_references(&testdata)?;
    let gap = vec![0.0_f32; (SAMPLE_RATE / 2) as usize];
    let mut buffer = Vec::new();
    let mut recognizers = recognizers()?;
    let mut each = Side::default();
    for (number, reference) in (1..=8).zip(&references) {
        let samples = eval_clip(&testdata, &format!("ja_{number:02}"))?;
        each = each.plus(measure(&mut recognizers, &samples, reference)?);
        buffer.extend(&samples);
        buffer.extend(&gap);
    }
    assert_eq!(buffer.len(), 827_938);
    let whole = measure(&mut recognizers, &buffer, &references.concat())?;
    println!(
        "ja_01..ja_08: whole {} micros, one by one {} micros",
        whole.cer.micros(),
        each.cer.micros()
    );
    judge_drop(whole.cer.micros(), each.cer.micros())?;
    Ok(())
}

#[test]
#[ignore = "reads testdata/eval_real and the models, which are not in git"]
fn a_single_clip_group_shows_no_drop() -> TestResult {
    let testdata = testdata_dir()?;
    let mut recognizers = recognizers()?;
    for group in &GROUPS {
        for (name, reference) in group.clips.iter().zip(group.refs) {
            let buffer = eval_clip(&testdata, name)?;
            let whole = measure(&mut recognizers, &buffer, reference)?;
            let each = measure(&mut recognizers, &eval_clip(&testdata, name)?, reference)?;
            println!(
                "{name}: whole {} micros, one by one {} micros",
                whole.cer.micros(),
                each.cer.micros()
            );
            judge_no_drop(whole.cer.micros(), each.cer.micros())?;
        }
    }
    Ok(())
}

// --- the checks that need neither the models nor the audio ---------------------

#[test]
fn a_drop_of_exactly_5_points_is_enough() {
    assert!(judge_drop(150_000, 100_000).is_ok());
}

#[test]
fn a_drop_of_4_9999_points_is_not_enough() {
    let said = judge_drop(149_999, 100_000).err();
    assert!(
        said.as_deref().is_some_and(|text| text.contains("49999")),
        "{said:?}"
    );
}

#[test]
fn a_difference_of_9999_millionths_is_no_drop() {
    assert!(judge_no_drop(109_999, 100_000).is_ok());
}

#[test]
fn a_difference_of_10000_millionths_is_a_drop() {
    let said = judge_no_drop(110_000, 100_000).err();
    assert!(
        said.as_deref().is_some_and(|text| text.contains("10000")),
        "{said:?}"
    );
}

const FIRST: &[i16] = &[1, 2, 3];
const SECOND: &[i16] = &[4, 5];
const THIRD: &[i16] = &[6, 7, 8, 9];

/// Why `joined` is not the three clips joined by two zero samples.
fn why_not_joined(joined: &[i16]) -> Option<String> {
    check_join(&[FIRST, SECOND, THIRD], 2, joined).err()
}

fn says(said: Option<String>, part: &str) -> bool {
    said.as_deref().is_some_and(|text| text.contains(part))
}

#[test]
fn a_joined_buffer_with_exact_gaps_is_accepted() {
    assert_eq!(
        why_not_joined(&[1, 2, 3, 0, 0, 4, 5, 0, 0, 6, 7, 8, 9]),
        None
    );
}

#[test]
fn a_gap_one_sample_short_is_rejected() {
    let said = why_not_joined(&[1, 2, 3, 0, 4, 5, 0, 0, 6, 7, 8, 9]);
    assert!(says(said, "gap 1 is 1 samples long, not 2"));
}

#[test]
fn a_nonzero_sample_in_a_gap_is_rejected() {
    let said = why_not_joined(&[1, 2, 3, 0, 0, 4, 5, 0, 9, 6, 7, 8, 9]);
    assert!(says(said, "gap 2 sample 2 is 9, not 0"));
}

#[test]
fn a_silence_before_the_first_or_after_the_last_clip_is_rejected() {
    let after = why_not_joined(&[1, 2, 3, 0, 0, 4, 5, 0, 0, 6, 7, 8, 9, 0, 0]);
    assert!(says(after, "after the last clip"));
    let before = why_not_joined(&[0, 0, 1, 2, 3, 0, 0, 4, 5, 0, 0, 6, 7, 8, 9]);
    assert!(says(before, "before the first clip"));
}

#[test]
fn a_clip_sample_that_differs_from_the_source_is_rejected() {
    let said = why_not_joined(&[1, 2, 3, 0, 0, 4, 6, 0, 0, 6, 7, 8, 9]);
    assert!(says(said, "clip 2 sample 2 is 6, not 5"));
}

#[test]
fn a_missing_long_material_names_its_path() {
    let missing = Path::new(TARGET).join("rp89_no_such_directory");
    let said = read_material(&missing, &GROUPS[0], &GAPS[0]).err();
    let path = missing.display().to_string();
    assert!(says(said, &path));
}

#[test]
fn the_reference_of_a_group_is_its_three_references_joined_in_order() {
    let expected = [
        "勝利はしたものの１０万人以上の戦死者を出していた。高校生２００人に聞きました。模擬裁判に参加した大学１年生の本音とは。",
        "でも敦賀さん、９１歳力強い走りに変わっていきます。けさの東京、風はなさそうですけどね、体感はどうでしょうか、近藤さん。しっかり打ち切れるかです。",
        "そこの上流から丸太を組んでいかだにしてどんどん流したんですこれを。そしてもう一つ大きな勢力があります。今、１００ｍのターンを迎える。",
    ];
    for (group, want) in GROUPS.iter().zip(expected) {
        let reference = group.reference();
        assert_eq!(reference, want, "{}", group.name);
        assert_eq!(
            cer(&reference, &reference),
            Cer {
                dist: 0,
                denom: group.chars
            }
        );
        assert_eq!(
            cer(&reference, ""),
            Cer {
                dist: group.chars,
                denom: group.chars
            }
        );
    }
}

// --- the material on disk -----------------------------------------------------

#[test]
#[ignore = "reads testdata/long_ja, which is not in git"]
fn the_long_material_is_the_three_clips_joined_by_zero_gaps() -> TestResult {
    let testdata = testdata_dir()?;
    for group in &GROUPS {
        let sources = group
            .clips
            .iter()
            .map(|name| {
                let path = testdata.join("eval_real").join(format!("{name}.wav"));
                read_pcm(&path).map(|wav| wav.samples)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let parts: Vec<&[i16]> = sources.iter().map(Vec::as_slice).collect();
        for (gap, length) in GAPS.iter().zip(group.lengths) {
            let joined = read_material(&testdata.join("long_ja"), group, gap)?;
            assert_eq!(joined.len(), length, "{} gap {} ms", group.name, gap.millis);
            check_join(&parts, gap.samples, &joined)?;
        }
    }
    Ok(())
}

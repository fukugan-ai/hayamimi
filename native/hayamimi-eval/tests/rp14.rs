//! RP-14: the Rust chrF gives the value sacrebleu 2.6.0 gave, to within one
//! hundredth of a point, on the 50 sentences of each of four languages.
//!
//! `chrf_fixture.tsv` of the baseline holds `<lang>\t<n>\t<score>` for each
//! sentence and `<lang>\tcorpus\t<score>` for the corpus, scored by sacrebleu
//! over the reference and the degraded hypothesis of `pairs/`. The detector is
//! held to a control: a scorer that counts n-grams to order 5 and not 6 is
//! more than one hundredth away on some sentence.

mod support;

use std::collections::HashMap;

use hayamimi_eval::chrf::{corpus_chrf, sentence_chrf};
use support::{Failure, TestResult, base, micros, read};

const LANGUAGES: [&str; 4] = ["en", "zh", "ko", "es"];

/// One hundredth of a point, in units of 10^-6 points.
const TOLERANCE: i64 = 10_000;

/// A row of the fixture: a sentence number or `corpus`, and the score.
struct Row {
    key: String,
    score: i64,
}

fn fixture_rows(lang: &str) -> Result<Vec<Row>, Failure> {
    let text = read(&base()?.join("chrf_fixture.tsv"))?;
    let mut rows = Vec::new();
    for line in text.lines() {
        let cells: Vec<&str> = line.split('\t').collect();
        let [code, key, score] = cells.as_slice() else {
            return Err(format!(
                "the fixture row {line:?} does not have three columns"
            ));
        };
        if *code == lang {
            rows.push(Row {
                key: (*key).to_owned(),
                score: micros(score)?,
            });
        }
    }
    Ok(rows)
}

fn pair_file(lang: &str, kind: &str) -> Result<String, Failure> {
    read(&base()?.join("pairs").join(format!("{lang}.{kind}.txt")))
}

/// A score in units of 10^-6 points.
fn micro(score: f64) -> i64 {
    (score * 1_000_000.0).round() as i64
}

/// Whether two scores, in units of 10^-6, are within one hundredth.
fn within_a_hundredth(ours: i64, recorded: i64) -> Result<(), Failure> {
    let gap = (ours - recorded).abs();
    if gap <= TOLERANCE {
        Ok(())
    } else {
        Err(format!("{ours} against {recorded}: {gap} apart"))
    }
}

/// The sentence rows of a language with the reference and the hypothesis each
/// was scored over.
fn sentence_cases(lang: &str) -> Result<Vec<(Row, String, String)>, Failure> {
    let (references, hypotheses) = (pair_file(lang, "ref")?, pair_file(lang, "hyp")?);
    let rows: Vec<Row> = fixture_rows(lang)?
        .into_iter()
        .filter(|row| row.key != "corpus")
        .collect();
    let (references, hypotheses): (Vec<&str>, Vec<&str>) =
        (references.lines().collect(), hypotheses.lines().collect());
    if rows.len() != references.len() || rows.len() != hypotheses.len() {
        return Err(format!(
            "{lang}: {} fixture rows, {} references, {} hypotheses",
            rows.len(),
            references.len(),
            hypotheses.len()
        ));
    }
    Ok(rows
        .into_iter()
        .zip(references.into_iter().zip(hypotheses))
        .map(|(row, (reference, hypothesis))| (row, reference.to_owned(), hypothesis.to_owned()))
        .collect())
}

/// T-RP-14-001.
#[test]
fn sentence_scores_match_sacrebleu() -> TestResult {
    let mut compared = 0;
    for lang in LANGUAGES {
        for (row, reference, hypothesis) in sentence_cases(lang)? {
            let ours = micro(sentence_chrf(&reference, &hypothesis));
            within_a_hundredth(ours, row.score)
                .map_err(|why| format!("{lang} {}: {why}", row.key))?;
            compared += 1;
        }
    }
    assert_eq!(compared, 200);
    Ok(())
}

/// T-RP-14-002.
#[test]
fn corpus_scores_match_sacrebleu() -> TestResult {
    for lang in LANGUAGES {
        let (references, hypotheses) = (pair_file(lang, "ref")?, pair_file(lang, "hyp")?);
        let (references, hypotheses): (Vec<&str>, Vec<&str>) =
            (references.lines().collect(), hypotheses.lines().collect());
        let ours = micro(corpus_chrf(&references, &hypotheses)?.score);
        let corpus_rows: Vec<Row> = fixture_rows(lang)?
            .into_iter()
            .filter(|row| row.key == "corpus")
            .collect();
        let [recorded] = corpus_rows.as_slice() else {
            return Err(format!("{lang} has {} corpus rows", corpus_rows.len()).into());
        };
        within_a_hundredth(ours, recorded.score).map_err(|why| format!("{lang}: {why}"))?;
    }
    Ok(())
}

/// T-RP-14-003.
#[test]
fn difference_boundary_is_0_01() -> TestResult {
    assert_eq!(
        within_a_hundredth(micros("47.57")?, micros("47.58")?),
        Ok(())
    );
    assert!(within_a_hundredth(micros("47.57")?, micros("47.59")?).is_err());
    Ok(())
}

/// T-RP-14-004.
#[test]
fn first_fixture_sentence_matches() -> TestResult {
    let cases = sentence_cases("en")?;
    let Some((row, reference, hypothesis)) = cases.first() else {
        return Err("the fixture has no English sentence".into());
    };
    assert_eq!(row.key, "1");
    within_a_hundredth(micro(sentence_chrf(reference, hypothesis)), row.score)?;
    Ok(())
}

/// T-RP-14-005.
#[test]
fn identical_sentences_score_100() {
    for sentence in [
        "The cat sat on the mat.",
        "猫が寝ている。",
        "El gato duerme.",
    ] {
        assert!((sentence_chrf(sentence, sentence) - 100.0).abs() < 1e-9);
    }
}

/// T-RP-14-006.
#[test]
fn disjoint_characters_score_zero() {
    assert_eq!(sentence_chrf("aaaa", "bbbb"), 0.0);
}

fn grams(chars: &[char], length: usize) -> HashMap<&[char], u64> {
    let mut counts = HashMap::new();
    for gram in chars.windows(length) {
        *counts.entry(gram).or_insert(0) += 1;
    }
    counts
}

/// The sentence chrF with n-grams up to `order` characters, beta 2, white
/// space left out: the scorer of the library with the order as a parameter.
fn chrf_to_order(reference: &str, hypothesis: &str, order: usize) -> f64 {
    let chars = |text: &str| -> Vec<char> { text.chars().filter(|c| !c.is_whitespace()).collect() };
    let (reference, hypothesis) = (chars(reference), chars(hypothesis));
    let (mut precision, mut recall, mut orders) = (0.0, 0.0, 0.0);
    for length in 1..=order {
        let (theirs, ours) = (grams(&reference, length), grams(&hypothesis, length));
        let (reference_total, hypothesis_total): (u64, u64) =
            (theirs.values().sum(), ours.values().sum());
        let matching: u64 = ours
            .iter()
            .map(|(gram, count)| theirs.get(gram).map_or(0, |other| (*count).min(*other)))
            .sum();
        if reference_total > 0 && hypothesis_total > 0 {
            precision += matching as f64 / hypothesis_total as f64;
            recall += matching as f64 / reference_total as f64;
            orders += 1.0;
        }
    }
    if orders == 0.0 {
        return 0.0;
    }
    let (precision, recall) = (precision / orders, recall / orders);
    if 4.0 * precision + recall == 0.0 {
        return 0.0;
    }
    100.0 * 5.0 * precision * recall / (4.0 * precision + recall)
}

/// T-RP-14-007: the detector sees a scorer of the wrong order. The same
/// function at order 6 is the control: it agrees with the library on every
/// sentence, so a gap at order 5 is the order and not the function.
#[test]
fn order_five_implementation_differs_on_some_sentence() -> TestResult {
    let (mut far_at_five, mut far_at_six) = (0, 0);
    for lang in LANGUAGES {
        for (row, reference, hypothesis) in sentence_cases(lang)? {
            let at_five = micro(chrf_to_order(&reference, &hypothesis, 5));
            let at_six = micro(chrf_to_order(&reference, &hypothesis, 6));
            far_at_five += usize::from(within_a_hundredth(at_five, row.score).is_err());
            far_at_six += usize::from(within_a_hundredth(at_six, row.score).is_err());
        }
    }
    assert_eq!(far_at_six, 0);
    assert!(far_at_five >= 1, "order 5 was never told from order 6");
    Ok(())
}

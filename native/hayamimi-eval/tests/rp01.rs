//! RP-1: the recognition error of the Rust engine is no worse than the Python
//! engine's in any language, and it detects languages as often. The cases here
//! cover the comparison (`check_scorecard`), the scoring and the Python
//! baseline; the two cases that read a Rust run are `#[ignore]`d.

mod support;

use std::path::Path;

use hayamimi_eval::score::{Scorer, wer};
use hayamimi_eval::scorecard::{
    LangRow, Lid, ScoreError, Scorecard, check_scorecard, parse_markdown, python_baseline,
    read_json,
};
use support::{RESULTS, TestResult, find_up, read};

/// The Python figures of `docs/results/scorecard.md`, in millionths, with the
/// right detections and the clips of each language.
const PYTHON: [(&str, u64, u32, u32); 5] = [
    ("ja", 38_000, 15, 15),
    ("en", 23_000, 15, 15),
    ("zh", 66_000, 12, 12),
    ("ko", 81_000, 12, 12),
    ("yue", 61_000, 12, 12),
];

/// How far a Python run may be from the recorded figure, in millionths.
const TOLERANCE: u64 = 5_000;

fn row(lang: &str, err: u64, lid: u32, clips: u32) -> LangRow {
    LangRow {
        lang: lang.to_owned(),
        clips,
        lid_correct: Some(lid),
        tiers: String::from("x"),
        mean_err_micros: err,
        mean_rtf_micros: 0,
    }
}

fn card(rows: Vec<LangRow>) -> Scorecard {
    Scorecard {
        engine: String::from("fixture"),
        lid: Lid::Detected,
        rows,
        fallbacks: Vec::new(),
    }
}

fn python() -> Scorecard {
    card(
        PYTHON
            .iter()
            .map(|(lang, err, lid, clips)| row(lang, *err, *lid, *clips))
            .collect(),
    )
}

/// The Python card with one language changed.
fn python_with(lang: &str, change: impl Fn(&mut LangRow)) -> Scorecard {
    let mut changed = python();
    for one in changed.rows.iter_mut().filter(|one| one.lang == lang) {
        change(one);
    }
    changed
}

/// T-RP-1-001
#[test]
fn equal_to_python_passes() {
    assert_eq!(check_scorecard(&python(), &python()), Ok(()));
}

/// T-RP-1-002
#[test]
fn ja_one_thousandth_worse_is_rejected() {
    let rust = python_with("ja", |one| one.mean_err_micros = 39_000);
    assert!(matches!(
        check_scorecard(&python(), &rust),
        Err(ScoreError::Worse { lang, .. }) if lang == "ja"
    ));
}

/// T-RP-1-003
#[test]
fn en_one_thousandth_worse_is_rejected() {
    let rust = python_with("en", |one| one.mean_err_micros = 24_000);
    assert!(matches!(
        check_scorecard(&python(), &rust),
        Err(ScoreError::Worse { lang, .. }) if lang == "en"
    ));
}

/// T-RP-1-004
#[test]
fn all_languages_better_pass() {
    let mut rust = python();
    for one in &mut rust.rows {
        one.mean_err_micros -= 1_000;
    }
    assert_eq!(check_scorecard(&python(), &rust), Ok(()));
}

/// T-RP-1-005
#[test]
fn one_fewer_correct_lid_is_rejected() {
    let rust = python_with("ja", |one| one.lid_correct = Some(14));
    assert!(matches!(
        check_scorecard(&python(), &rust),
        Err(ScoreError::LidWorse { lang, python: 15, rust: 14 }) if lang == "ja"
    ));
}

/// T-RP-1-006
#[test]
fn missing_language_is_rejected() {
    let mut rust = python();
    rust.rows.retain(|one| one.lang != "yue");
    assert_eq!(
        check_scorecard(&python(), &rust),
        Err(ScoreError::MissingLanguage(String::from("yue")))
    );
}

/// The baseline of the two Python runs, read from the files they left.
fn python_runs() -> Result<Scorecard, Box<dyn std::error::Error>> {
    let base = find_up("testdata/baseline")?;
    let first = read(&base.join("scorecard_run1.md"))?;
    let second = read(&base.join("scorecard_run2.md"))?;
    Ok(python_baseline(&[&first, &second])?)
}

/// T-RP-1-007
#[test]
fn python_baseline_matches_scorecard_md_within_0_005() -> TestResult {
    let baseline = python_runs()?;
    let record = parse_markdown(&read(&Path::new(RESULTS).join("scorecard.md"))?)?;
    assert_eq!(record.len(), PYTHON.len());
    for recorded in &record {
        let Some(found) = baseline.rows.iter().find(|one| one.lang == recorded.lang) else {
            return Err(format!("{} is not in the baseline", recorded.lang).into());
        };
        let apart = found.mean_err_micros.abs_diff(recorded.mean_err_micros);
        assert!(apart <= TOLERANCE, "{}: {apart} apart", recorded.lang);
    }
    Ok(())
}

/// T-RP-1-008
#[test]
fn wer_ignores_case_and_punctuation() {
    assert_eq!(wer("Hello, World.", "hello world").micros(), 0);
}

/// T-RP-1-009
#[test]
fn wer_one_substitution_in_six_words() {
    let measured = wer("the cat sat on the mat", "the cat sat on a mat");
    assert_eq!(measured.micros(), 166_666);
}

/// T-RP-1-010
#[test]
fn wer_of_empty_reference_is_0() {
    assert_eq!(wer(" , . ", "x").micros(), 0);
}

/// T-RP-1-011
#[test]
fn yue_cer_normalises_traditional_to_simplified() -> TestResult {
    let scorer = Scorer::new()?;
    assert_eq!(scorer.score("yue", "學習", "学习").micros(), 0);
    Ok(())
}

/// T-RP-1-012: the scorecard run of the shipped build on Windows, `<ev>
/// scorecard --engine hayamimi --sets
/// testdata/eval_real,testdata/eval_real_zhko,testdata/eval_real_yue --out
/// docs/results/r3c/win_routed.json`. It passes only when the route detects
/// languages; a run that used the true language leaves the detections empty and
/// is refused.
#[test]
#[ignore = "reads the scorecard of a Rust run"]
fn hayamimi_scorecard_not_worse_than_python() -> TestResult {
    let rust = read_json(&Path::new(RESULTS).join("r3c").join("win_routed.json"))?;
    check_scorecard(&python_runs()?, &rust)?;
    Ok(())
}

/// The smallest Japanese error, in millionths, that rounds to more than the
/// recorded 0.038 at three decimals.
const JA_WORSE_FROM: u64 = 38_500;

/// T-RP-1-013: the run of a build that routes the Japanese clips to the
/// SenseVoice tier, written to `docs/results/r3/rp01_sensevoice_ja.json`; the
/// other languages take the same path as in 012. The negative control: the
/// comparison must name Japanese, whose error rounds above 0.038 and whose
/// detections fall below 15, so the comparison in 012 is shown to be able to
/// fail. A record that is not there is an error naming its path.
#[test]
#[ignore = "reads the scorecard of a Rust run that routes the ja clips to SenseVoice"]
fn sensevoice_routed_ja_run_is_worse_than_python_in_ja() -> TestResult {
    let rust = read_json(
        &Path::new(RESULTS)
            .join("r3")
            .join("rp01_sensevoice_ja.json"),
    )?;
    let said = check_scorecard(&python_runs()?, &rust);
    // The error must be the Japanese error itself: a run with no detections also fails, for another reason.
    assert!(
        matches!(&said, Err(ScoreError::Worse { lang, .. }) if lang == "ja"),
        "{said:?}"
    );
    let Some(ja) = rust.rows.iter().find(|one| one.lang == "ja") else {
        return Err("the record has no ja row".into());
    };
    assert!(
        ja.mean_err_micros >= JA_WORSE_FROM,
        "{}",
        ja.mean_err_micros
    );
    assert!(
        ja.lid_correct.is_some_and(|right| right < 15),
        "{:?}",
        ja.lid_correct
    );
    Ok(())
}

//! RP-13: the corpus chrF of the translations from a language model beats the
//! recorded chrF of the models it replaces, in each of four languages.
//!
//! `check_chrf` is the comparison: strictly above the record, in every
//! language, and a language that was not measured is named. The scorer under it
//! is held to the controls of the acceptance table: a reference given as the
//! hypothesis scores 100, the source given as the hypothesis falls below the
//! record, an empty hypothesis stays in the corpus, and a hypothesis file of
//! another length is refused.
//!
//! The sentences are the 50 pairs per language of the baseline
//! (`fleurs-pairs --n 50 --seed 0`). The run against a language model reads
//! them too, so it needs a llama-server and runs with `--ignored`.

mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use hayamimi_eval::chrf::{Corpus, corpus_chrf, score_directories, sentence_chrf};
use hayamimi_relay::eval::{self, EvalOptions};
use hayamimi_relay::llm::HttpLlm;
use hayamimi_relay::logging::NullSink;
use support::{Failure, RESULTS, TestResult, base, micros, read, testdata};

/// The recorded corpus chrF per language, in units of 10^-6.
const CHRF_RECORD: [(&str, i64); 4] = [
    ("en", 47_570_000),
    ("zh", 20_190_000),
    ("ko", 25_200_000),
    ("es", 42_090_000),
];

/// One hundredth of a point, in units of 10^-6 points.
const HUNDREDTH: i64 = 10_000;

/// Where the translator of the live pair listens: the address the relay's
/// `live` asks.
const LLAMA_SERVER: &str = "http://127.0.0.1:8080";
/// Where the translator of the other languages listens, when
/// `testdata/rp13_other_server.txt` does not name another address.
const LLAMA_OTHER_SERVER: &str = "http://127.0.0.1:8081";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
/// The result file of the run, under `docs/results/`.
const RESULT_FILE: &str = "rp13_translation.md";

/// Strictly above the record in every language. Names the first language
/// that is not, and any language of the record that was not measured.
fn check_chrf(record: &[(&str, i64)], measured: &[(&str, i64)]) -> Result<(), Failure> {
    for (lang, floor) in record {
        let Some((_, got)) = measured.iter().find(|(name, _)| name == lang) else {
            return Err(format!("{lang} was not measured"));
        };
        if got <= floor {
            return Err(format!(
                "{lang}: chrF {got} does not beat the record {floor}"
            ));
        }
    }
    Ok(())
}

fn pairs_dir() -> Result<PathBuf, Failure> {
    Ok(base()?.join("pairs"))
}

/// A file of the pairs directory: `<lang>.ref.txt`, `<lang>.ja.txt` or the
/// degraded `<lang>.hyp.txt`.
fn pair_file(lang: &str, kind: &str) -> Result<String, Failure> {
    read(&pairs_dir()?.join(format!("{lang}.{kind}.txt")))
}

fn score_text(references: &str, hypotheses: &str) -> Result<Corpus, Failure> {
    let references: Vec<&str> = references.lines().collect();
    let hypotheses: Vec<&str> = hypotheses.lines().collect();
    corpus_chrf(&references, &hypotheses).map_err(|error| error.to_string())
}

/// A score in units of 10^-6 points.
fn micro(score: f64) -> i64 {
    (score * 1_000_000.0).round() as i64
}

fn all_languages(record: &[(&str, i64)], delta: i64) -> Vec<(&'static str, i64)> {
    CHRF_RECORD
        .iter()
        .zip(record)
        .map(|((lang, floor), _)| (*lang, floor + delta))
        .collect()
}

/// T-RP-13-001.
#[test]
fn values_one_hundredth_above_record_pass() {
    let measured = all_languages(&CHRF_RECORD, HUNDREDTH);
    assert_eq!(check_chrf(&CHRF_RECORD, &measured), Ok(()));
}

/// T-RP-13-002.
#[test]
fn value_equal_to_record_is_rejected() {
    let mut measured = all_languages(&CHRF_RECORD, HUNDREDTH);
    measured[0] = ("en", 47_570_000);
    let said = check_chrf(&CHRF_RECORD, &measured);
    assert!(
        matches!(said, Err(ref text) if text.starts_with("en:")),
        "{said:?}"
    );
}

/// T-RP-13-003.
#[test]
fn value_below_record_is_rejected() {
    let mut measured = all_languages(&CHRF_RECORD, HUNDREDTH);
    measured[1] = ("zh", 20_180_000);
    let said = check_chrf(&CHRF_RECORD, &measured);
    assert!(
        matches!(said, Err(ref text) if text.starts_with("zh:")),
        "{said:?}"
    );
}

#[test]
fn a_language_that_was_not_measured_is_named() {
    let measured = all_languages(&CHRF_RECORD, HUNDREDTH);
    let said = check_chrf(&CHRF_RECORD, &measured[..3]);
    assert!(
        matches!(said, Err(ref text) if text.starts_with("es ")),
        "{said:?}"
    );
}

/// T-RP-13-004: three sentences that verification withheld in both of their
/// translations are empty lines. They stay in the corpus: 50 pairs are
/// counted, and the score is the lower one that counts their references.
#[test]
fn empty_hypotheses_are_counted_not_dropped() -> TestResult {
    let references = pair_file("en", "ref")?;
    let degraded = pair_file("en", "hyp")?;
    let reference_lines: Vec<&str> = references.lines().collect();
    let mut hypothesis_lines: Vec<&str> = degraded.lines().collect();
    for withheld in [4, 17, 30] {
        hypothesis_lines[withheld] = "";
    }
    let counted = corpus_chrf(&reference_lines, &hypothesis_lines)?;
    assert_eq!(counted.pairs, 50);
    let (kept_references, kept_hypotheses): (Vec<&str>, Vec<&str>) = reference_lines
        .iter()
        .zip(&hypothesis_lines)
        .filter(|(_, hypothesis)| !hypothesis.is_empty())
        .unzip();
    let dropped = corpus_chrf(&kept_references, &kept_hypotheses)?;
    assert_eq!(dropped.pairs, 47);
    assert!(counted.score < dropped.score, "{counted:?} {dropped:?}");
    Ok(())
}

/// T-RP-13-005.
#[test]
fn all_empty_hypotheses_score_zero() -> TestResult {
    let references = pair_file("en", "ref")?;
    let empty = "\n".repeat(50);
    let corpus = score_text(&references, &empty)?;
    assert_eq!(corpus.pairs, 50);
    assert_eq!(corpus.score, 0.0);
    Ok(())
}

/// T-RP-13-006.
#[test]
fn hypothesis_line_count_mismatch_is_rejected() -> TestResult {
    let references = pair_file("en", "ref")?;
    let short: String = references
        .lines()
        .take(49)
        .map(|line| format!("{line}\n"))
        .collect();
    let said = score_text(&references, &short);
    assert!(
        matches!(said, Err(ref text) if text.contains("49") && text.contains("50")),
        "{said:?}"
    );
    Ok(())
}

/// T-RP-13-007.
#[test]
fn reference_as_hypothesis_scores_100() -> TestResult {
    for (lang, _) in CHRF_RECORD {
        let references = pair_file(lang, "ref")?;
        let corpus = score_text(&references, &references)?;
        assert_eq!(corpus.pairs, 50, "{lang}");
        assert!(
            (corpus.score - 100.0).abs() < 1e-9,
            "{lang}: {}",
            corpus.score
        );
    }
    Ok(())
}

/// T-RP-13-008: Chinese shares its characters with the Japanese source, and
/// still falls below the record.
#[test]
fn source_as_hypothesis_stays_below_record() -> TestResult {
    for (lang, record) in CHRF_RECORD {
        let corpus = score_text(&pair_file(lang, "ref")?, &pair_file(lang, "ja")?)?;
        assert!(micro(corpus.score) < record, "{lang}: {}", corpus.score);
    }
    Ok(())
}

/// The scorer gives the value sacrebleu gave, to the four decimals the
/// baseline recorded: the sentence scores of the degraded hypotheses, and the
/// corpus scores of the three controls.
#[test]
fn the_scorer_gives_the_recorded_sacrebleu_scores() -> TestResult {
    let scores = base()?.join("chrf_scores");
    for (lang, _) in CHRF_RECORD {
        let references = pair_file(lang, "ref")?;
        let degraded = pair_file(lang, "hyp")?;
        let recorded = read(&scores.join(format!("{lang}.hyp.sentence.txt")))?;
        let pairs = references.lines().zip(degraded.lines());
        let mut compared = 0;
        for ((reference, hypothesis), want) in pairs.zip(recorded.lines()) {
            let got = micro(sentence_chrf(reference, hypothesis));
            assert!(
                (got - micros(want)?).abs() <= 100,
                "{lang}: {got} against {want}"
            );
            compared += 1;
        }
        assert_eq!(compared, 50, "{lang}");
        for (hypotheses, name) in [
            (pair_file(lang, "hyp")?, "hyp"),
            (pair_file(lang, "ja")?, "ja"),
            (references.clone(), "ref"),
        ] {
            let want = micros(&read(&scores.join(format!("{lang}.{name}.corpus.txt")))?)?;
            let got = micro(score_text(&references, &hypotheses)?.score);
            assert!(
                (got - want).abs() <= 100,
                "{lang} {name}: {got} against {want}"
            );
        }
    }
    Ok(())
}

/// The address of the second translator: the one line of
/// `testdata/rp13_other_server.txt` when the file is there, the design's
/// address when it is not.
fn other_server() -> Result<String, Failure> {
    let file = testdata()?.join("rp13_other_server.txt");
    if file.is_file() {
        Ok(read(&file)?.trim().to_owned())
    } else {
        Ok(String::from(LLAMA_OTHER_SERVER))
    }
}

/// The translations of the language models, scored.
fn translate_and_score(pairs: &Path, out: &Path) -> Result<Vec<(String, Corpus)>, Failure> {
    let llm = HttpLlm::with_timeout(LLAMA_SERVER, REQUEST_TIMEOUT);
    let other = HttpLlm::with_timeout(&other_server()?, REQUEST_TIMEOUT);
    eval::run(&llm, &other, &EvalOptions { pairs, out }, &mut NullSink)
        .map_err(|error| error.to_string())?;
    score_directories(pairs, out).map_err(|error| error.to_string())
}

/// One language of the result file: the corpus chrF with every held line
/// counted as empty, the corpus chrF of the lines that were not held, and how
/// many were held.
struct Row {
    lang: String,
    all: Corpus,
    kept: Option<Corpus>,
    held: usize,
}

/// The score of `lang` over the lines whose hypothesis is not empty; `None`
/// when every line was held.
fn row_of(lang: &str, all: Corpus, pairs: &Path, out: &Path) -> Result<Row, Failure> {
    let references = read(&pairs.join(format!("{lang}.ref.txt")))?;
    let hypotheses = read(&out.join(format!("{lang}.hyp.txt")))?;
    let (kept_references, kept_hypotheses): (Vec<&str>, Vec<&str>) = references
        .lines()
        .zip(hypotheses.lines())
        .filter(|(_, hypothesis)| !hypothesis.is_empty())
        .unzip();
    let held = hypotheses.lines().count() - kept_hypotheses.len();
    let kept = if kept_hypotheses.is_empty() {
        None
    } else {
        Some(corpus_chrf(&kept_references, &kept_hypotheses).map_err(|error| error.to_string())?)
    };
    Ok(Row {
        lang: lang.to_owned(),
        all,
        kept,
        held,
    })
}

/// A table cell for a score; a language with no record, or with every line
/// held, has no number.
fn cell(score: Option<f64>) -> String {
    match score {
        Some(value) => format!("{value:.4}"),
        // DEFAULT: a language outside the record, or one whose every line was held, has no score, and the table cell is a dash.
        None => String::from("-"),
    }
}

/// The result file: per language the record, the score over all lines (held
/// ones empty), the score over the lines that were not held, and the number
/// held.
fn result_text(rows: &[Row]) -> String {
    let mut text = String::from(
        "# RP-13 translation chrF\n\n\
         Written by `translation_chrf_beats_current_models` in \
         `native/hayamimi-eval/tests/rp13.rs`.\n\
         `all lines` counts a line that V held as empty; `lines kept` scores only the \
         lines that were not held.\n\n\
         | lang | record | all lines | lines kept | lines | held |\n|---|---|---|---|---|---|\n",
    );
    for row in rows {
        let record = CHRF_RECORD
            .iter()
            .find(|(lang, _)| *lang == row.lang)
            .map(|(_, floor)| *floor as f64 / 1_000_000.0);
        text.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            row.lang,
            cell(record),
            cell(Some(row.all.score)),
            cell(row.kept.map(|corpus| corpus.score)),
            row.all.pairs,
            row.held,
        ));
    }
    text
}

/// T-RP-13-009: needs a llama-server at `LLAMA_SERVER` with the translation
/// model. The sentences are the baseline's 50 per language.
#[test]
#[ignore = "needs a llama-server with the translation model"]
fn translation_chrf_beats_current_models() -> TestResult {
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("rp13-hyps");
    // A directory left by an earlier run holds files this run may not write.
    if out.exists() {
        fs::remove_dir_all(&out)?;
    }
    let pairs = pairs_dir()?;
    let scores = translate_and_score(&pairs, &out)?;
    let measured: Vec<(&str, i64)> = scores
        .iter()
        .map(|(lang, corpus)| (lang.as_str(), micro(corpus.score)))
        .collect();
    let rows = scores
        .iter()
        .map(|(lang, corpus)| row_of(lang, *corpus, &pairs, &out))
        .collect::<Result<Vec<Row>, Failure>>()?;
    let text = result_text(&rows);
    print!("{text}");
    // The file is written before the comparison, so a run that falls short
    // still leaves its numbers.
    fs::write(Path::new(RESULTS).join(RESULT_FILE), &text)?;
    check_chrf(&CHRF_RECORD, &measured)?;
    Ok(())
}

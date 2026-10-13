//! Japanese to target-language sentence pairs from FLEURS, chosen the way
//! the Python translation evaluation chooses them, plus a rule-made
//! degraded copy of each reference for scorer cross-checks.
//!
//! FLEURS is n-way parallel: one `id` names the same sentence in every
//! language. The pairs are the ids both languages have, sorted, shuffled
//! with Python's generator seeded by `--seed`, and cut to `--n`.

use std::collections::BTreeMap;
use std::path::Path;

use crate::error::EvalError;
use crate::log;
use crate::manifest::write_text;
use crate::mt::Mt19937;

/// The FLEURS configuration of each language code the evaluation uses.
const CONFIGS: [(&str, &str); 5] = [
    ("ja", "ja_jp"),
    ("zh", "cmn_hans_cn"),
    ("ko", "ko_kr"),
    ("es", "es_419"),
    ("en", "en_us"),
];

/// Languages written without spaces between words.
const UNSPACED: [&str; 3] = ["zh", "ja", "yue"];

/// One chosen sentence: its FLEURS id, the Japanese source and the
/// target-language reference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pair {
    pub id: i64,
    pub ja: String,
    pub reference: String,
}

/// The FLEURS configuration of a language code.
pub fn config_for(lang: &str) -> Result<&'static str, EvalError> {
    CONFIGS
        .iter()
        .find(|(code, _)| *code == lang)
        .map(|(_, config)| *config)
        .ok_or_else(|| {
            let known: Vec<&str> = CONFIGS.iter().map(|(code, _)| *code).collect();
            EvalError::Invalid(format!(
                "no FLEURS configuration for {lang:?}; known: {known:?}"
            ))
        })
}

/// Where the validation split's text table of a configuration lives.
pub fn tsv_url(config: &str) -> String {
    format!("https://huggingface.co/datasets/google/fleurs/resolve/main/data/{config}/dev.tsv")
}

/// `id` to `raw_transcription` from a split's tab-separated table. Rows
/// whose text is blank are left out; for an id that appears twice the later
/// row wins.
pub fn parse_tsv(text: &str) -> Result<BTreeMap<i64, String>, EvalError> {
    let mut sentences = BTreeMap::new();
    for (number, line) in text.lines().enumerate() {
        let mut columns = line.split('\t');
        let (Some(id_text), Some(_file), Some(raw)) =
            (columns.next(), columns.next(), columns.next())
        else {
            return Err(EvalError::Manifest(format!(
                "FLEURS table line {} has fewer than three columns",
                number + 1
            )));
        };
        let Ok(id) = id_text.parse::<i64>() else {
            return Err(EvalError::Manifest(format!(
                "FLEURS table line {}: {id_text:?} is not an id",
                number + 1
            )));
        };
        if !raw.trim().is_empty() {
            sentences.insert(id, raw.to_owned());
        }
    }
    Ok(sentences)
}

/// Chooses up to `n` pairs the way `build_pairs` of the Python evaluation
/// does.
pub fn choose_pairs(
    ja: &BTreeMap<i64, String>,
    target: &BTreeMap<i64, String>,
    n: usize,
    seed: u32,
) -> Result<Vec<Pair>, EvalError> {
    let mut ids: Vec<i64> = ja
        .keys()
        .filter(|id| target.contains_key(id))
        .copied()
        .collect();
    if ids.is_empty() {
        return Err(EvalError::Manifest(String::from(
            "the two languages have no FLEURS id in common",
        )));
    }
    Mt19937::from_python_seed(seed).shuffle(&mut ids);
    ids.truncate(n);
    ids.into_iter()
        .map(|id| match (ja.get(&id), target.get(&id)) {
            (Some(source), Some(reference)) => Ok(Pair {
                id,
                ja: source.clone(),
                reference: reference.clone(),
            }),
            _ => Err(EvalError::Manifest(format!(
                "id {id} vanished from a table"
            ))),
        })
        .collect()
}

/// The reference with its first word dropped, or its first character for a
/// language written without spaces. A sentence of one word becomes empty.
pub fn degrade(reference: &str, lang: &str) -> String {
    if UNSPACED.contains(&lang) {
        return reference.chars().skip(1).collect();
    }
    match reference.split_once(char::is_whitespace) {
        Some((_, rest)) => rest.trim_start().to_owned(),
        // DEFAULT: dropping the only word of a one-word sentence leaves nothing; the spec says it degrades to the empty text.
        None => String::new(),
    }
}

/// The text of one language's validation table.
fn download(config: &str) -> Result<String, EvalError> {
    let url = tsv_url(config);
    let http_error = |message: String| EvalError::Http {
        url: url.clone(),
        message,
    };
    let mut response = ureq::get(&url)
        .call()
        .map_err(|e| http_error(e.to_string()))?;
    response
        .body_mut()
        .read_to_string()
        .map_err(|e| http_error(e.to_string()))
}

/// The lines of one output file, one sentence per line.
fn lines_file(sentences: impl Iterator<Item = String>) -> String {
    let mut text = String::new();
    for sentence in sentences {
        text.push_str(&sentence);
        text.push('\n');
    }
    text
}

/// What `run` wrote for one language.
#[derive(Debug, PartialEq, Eq)]
pub struct Written {
    pub lang: String,
    pub pairs: usize,
}

/// Writes `<lang>.ids.txt`, `<lang>.ja.txt` and `<lang>.ref.txt` (and
/// `<lang>.hyp.txt` with `degrade`) for each target into `out`.
pub fn run(
    targets: &[String],
    n: usize,
    seed: u32,
    out: &Path,
    with_degraded: bool,
) -> Result<Vec<Written>, EvalError> {
    let ja = parse_tsv(&download(config_for("ja")?)?)?;
    let mut written = Vec::with_capacity(targets.len());
    for lang in targets {
        let target = parse_tsv(&download(config_for(lang)?)?)?;
        let pairs = choose_pairs(&ja, &target, n, seed)?;
        let file = |suffix: &str| out.join(format!("{lang}.{suffix}.txt"));
        write_text(
            &file("ids"),
            &lines_file(pairs.iter().map(|p| p.id.to_string())),
        )?;
        write_text(&file("ja"), &lines_file(pairs.iter().map(|p| p.ja.clone())))?;
        write_text(
            &file("ref"),
            &lines_file(pairs.iter().map(|p| p.reference.clone())),
        )?;
        if with_degraded {
            write_text(
                &file("hyp"),
                &lines_file(pairs.iter().map(|p| degrade(&p.reference, lang))),
            )?;
        }
        log::info(
            "eval.fleurs.written",
            "storage",
            "FLEURS pairs written",
            &[
                ("lang", lang.as_str().into()),
                ("pairs", pairs.len().into()),
            ],
        );
        written.push(Written {
            lang: lang.clone(),
            pairs: pairs.len(),
        });
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(rows: &[(i64, &str)]) -> BTreeMap<i64, String> {
        rows.iter()
            .map(|(id, text)| (*id, (*text).to_owned()))
            .collect()
    }

    #[test]
    fn a_known_language_has_a_configuration() -> Result<(), EvalError> {
        assert_eq!(config_for("zh")?, "cmn_hans_cn");
        assert_eq!(config_for("es")?, "es_419");
        Ok(())
    }

    #[test]
    fn an_unknown_language_is_refused() {
        assert!(matches!(config_for("fr"), Err(EvalError::Invalid(_))));
    }

    #[test]
    fn the_table_url_names_the_configuration() {
        assert!(tsv_url("ja_jp").ends_with("/data/ja_jp/dev.tsv"));
    }

    #[test]
    fn a_table_is_read_by_id_and_raw_text() -> Result<(), EvalError> {
        let text = "1516\ta.wav\tこんにちは。\tこんにちは\tx\t100\tMALE\n7\tb.wav\thello\thello\tx\t1\tFEMALE\n";
        assert_eq!(
            parse_tsv(text)?,
            table(&[(7, "hello"), (1516, "こんにちは。")])
        );
        Ok(())
    }

    #[test]
    fn blank_text_is_left_out_and_a_repeated_id_keeps_the_later_row() -> Result<(), EvalError> {
        let text = "1\ta.wav\t \t\n2\tb.wav\tfirst\t\n2\tc.wav\tsecond\t\n";
        assert_eq!(parse_tsv(text)?, table(&[(2, "second")]));
        Ok(())
    }

    #[test]
    fn a_line_that_is_not_a_table_row_is_refused() {
        assert!(matches!(
            parse_tsv("only\ttwo\n"),
            Err(EvalError::Manifest(_))
        ));
        assert!(matches!(
            parse_tsv("x\ta.wav\ttext\n"),
            Err(EvalError::Manifest(_))
        ));
    }

    #[test]
    fn pairs_come_only_from_ids_both_languages_have() -> Result<(), EvalError> {
        let ja = table(&[(1, "a"), (2, "b"), (3, "c")]);
        let target = table(&[(2, "B"), (3, "C"), (4, "D")]);
        let mut ids: Vec<i64> = choose_pairs(&ja, &target, 10, 0)?
            .iter()
            .map(|p| p.id)
            .collect();
        ids.sort_unstable();
        assert_eq!(ids, vec![2, 3]);
        Ok(())
    }

    #[test]
    fn at_most_n_pairs_are_returned() -> Result<(), EvalError> {
        let rows: Vec<(i64, &str)> = (0..30).map(|i| (i, "x")).collect();
        let ja = table(&rows);
        assert_eq!(choose_pairs(&ja, &ja, 12, 0)?.len(), 12);
        Ok(())
    }

    #[test]
    fn the_same_seed_chooses_the_same_pairs() -> Result<(), EvalError> {
        let rows: Vec<(i64, &str)> = (0..30).map(|i| (i, "x")).collect();
        let ja = table(&rows);
        assert_eq!(
            choose_pairs(&ja, &ja, 12, 0)?,
            choose_pairs(&ja, &ja, 12, 0)?
        );
        assert_ne!(
            choose_pairs(&ja, &ja, 12, 0)?,
            choose_pairs(&ja, &ja, 12, 1)?
        );
        Ok(())
    }

    #[test]
    fn languages_with_nothing_in_common_are_refused() {
        let ja = table(&[(1, "a")]);
        let target = table(&[(2, "B")]);
        assert!(matches!(
            choose_pairs(&ja, &target, 5, 0),
            Err(EvalError::Manifest(_))
        ));
    }

    #[test]
    fn a_spaced_language_loses_its_first_word() {
        assert_eq!(
            degrade("The cat sat on the mat.", "en"),
            "cat sat on the mat."
        );
        assert_eq!(degrade("Hola mundo cruel.", "es"), "mundo cruel.");
    }

    #[test]
    fn zh_loses_its_first_character() {
        assert_eq!(degrade("你好世界。", "zh"), "好世界。");
    }

    #[test]
    fn a_one_word_sentence_degrades_to_nothing() {
        assert_eq!(degrade("Yes", "en"), "");
        assert_eq!(degrade("好", "zh"), "");
    }

    #[test]
    fn lines_end_with_a_newline_each() {
        assert_eq!(
            lines_file(["a", "b"].into_iter().map(String::from)),
            "a\nb\n"
        );
        assert_eq!(lines_file(std::iter::empty()), "");
    }
}

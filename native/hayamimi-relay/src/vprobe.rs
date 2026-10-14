//! `vprobe`: how verification (V) treats the reference translations of
//! sentence pairs, and what T alone writes with and without V.
//!
//! Input is a pairs directory (`<lang>.ids.txt`, `<lang>.ja.txt` and
//! `<lang>.ref.txt`, as `hayamimi-eval fleurs-pairs` writes them) and one
//! language. The report has one tab-separated line per finding:
//!
//! - `reference`: the ids of the references V withholds as they stand.
//! - `break:number`, `break:negation`, `break:uncertainty`: the same for the
//!   references after one fixed rule broke them. `rows` counts the references
//!   the rule could break; the others are left out of that line. V should
//!   withhold every one of them.
//! - `t_only` (with `--out`): the ids V withholds from the translator's own
//!   answers. The answers go to `<out>/noV/<lang>.hyp.txt` as they are and to
//!   `<out>/V/<lang>.hyp.txt` with the withheld lines empty.
//!
//! The rules need no server. Only V's public entry points are used, and V's
//! own code is not read or changed, so the tool builds against an older V.
//!
//! The rules read the marker table of V (`DEFAULT_MARKERS`) with their own
//! scan: words for English and Spanish, substrings for Chinese and Korean,
//! after the `exclude` phrases are blanked. A line is broken only when this
//! scan finds a marker, which can differ from V's scan on a rare line.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::cli::{DEFAULT_LLM, DEFAULT_LLM_OTHER};
use crate::lang::Lang;
use crate::llm::{Completion, HttpLlm, Llm};
use crate::logging::NullSink;
use crate::pipeline::{MAX_TOKENS, translate_prompt_to};
use crate::target::Target;
use crate::verify::{
    DEFAULT_MARKERS, LangMarkers, MarkerSet, Markers, Verdict, Verifier, contains_eos,
};

/// The languages the tool reads.
const LANGS: [&str; 4] = ["en", "zh", "ko", "es"];

/// How long one request to the translator may take.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

const USAGE: &str = "usage: hayamimi-relay vprobe --pairs <dir> --lang <en|zh|ko|es> \
[--out <dir> [--llm <url>] [--llm-other <url>]]";

/// What the command line asked for.
#[derive(Debug, PartialEq, Eq)]
pub struct Options {
    pub pairs: PathBuf,
    pub lang: String,
    pub out: Option<PathBuf>,
    pub llm: String,
    pub llm_other: String,
}

/// One sentence of a pairs directory.
#[derive(Debug, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub ja: String,
    pub reference: String,
}

/// One line of the report: a name, the references looked at and the ids V
/// withheld.
#[derive(Debug, PartialEq, Eq)]
pub struct Finding {
    pub name: String,
    pub rows: usize,
    pub held: Vec<String>,
}

/// The fixed ways to break a reference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Break {
    /// The first ASCII digit becomes the next digit.
    Number,
    /// Every negation marker is removed.
    Negation,
    /// Every uncertainty marker is removed.
    Uncertainty,
}

impl Break {
    pub const ALL: [Break; 3] = [Break::Number, Break::Negation, Break::Uncertainty];

    fn name(self) -> &'static str {
        match self {
            Break::Number => "break:number",
            Break::Negation => "break:negation",
            Break::Uncertainty => "break:uncertainty",
        }
    }
}

/// The translator's answers to every row, as written and as V lets them
/// through, and the ids V withheld.
#[derive(Debug, PartialEq, Eq)]
pub struct TOnly {
    pub raw: Vec<String>,
    pub gated: Vec<String>,
    pub held: Vec<String>,
}

/// Parses the arguments after `vprobe`.
pub fn parse_options(args: &[String]) -> Result<Options, String> {
    let (mut pairs, mut lang, mut out) = (None, None, None);
    let mut llm = DEFAULT_LLM.to_owned();
    let mut llm_other = DEFAULT_LLM_OTHER.to_owned();
    let mut rest = args.iter();
    while let Some(option) = rest.next() {
        let Some(value) = rest.next() else {
            return Err(format!("{option} needs a value"));
        };
        match option.as_str() {
            "--pairs" => pairs = Some(PathBuf::from(value)),
            "--lang" => lang = Some(value.clone()),
            "--out" => out = Some(PathBuf::from(value)),
            "--llm" => llm.clone_from(value),
            "--llm-other" => llm_other.clone_from(value),
            _ => return Err(format!("unknown option {option}")),
        }
    }
    let (Some(pairs), Some(lang)) = (pairs, lang) else {
        return Err(String::from("--pairs and --lang are needed"));
    };
    if !LANGS.contains(&lang.as_str()) {
        return Err(format!("--lang must be one of {LANGS:?}, not {lang:?}"));
    }
    Ok(Options {
        pairs,
        lang,
        out,
        llm,
        llm_other,
    })
}

fn read_lines(path: &Path) -> Result<Vec<String>, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(text.lines().map(str::to_owned).collect())
}

/// The rows of `<dir>/<lang>.{ids,ja,ref}.txt`; the three files must have the
/// same number of lines.
pub fn read_rows(dir: &Path, lang: &str) -> Result<Vec<Row>, String> {
    let read = |suffix: &str| read_lines(&dir.join(format!("{lang}.{suffix}.txt")));
    let (ids, ja, references) = (read("ids")?, read("ja")?, read("ref")?);
    if ids.len() != ja.len() || ids.len() != references.len() {
        return Err(format!(
            "{lang}: ids, ja and ref have {}, {} and {} lines",
            ids.len(),
            ja.len(),
            references.len()
        ));
    }
    Ok(ids
        .into_iter()
        .zip(ja)
        .zip(references)
        .map(|((id, ja), reference)| Row { id, ja, reference })
        .collect())
}

/// `text` with its first ASCII digit replaced by the next digit (9 by 0);
/// `None` when it holds no digit.
fn shift_first_digit(text: &str) -> Option<String> {
    let (at, digit) = text.char_indices().find(|(_, c)| c.is_ascii_digit())?;
    let next = char::from_digit((digit.to_digit(10)? + 1) % 10, 10)?;
    let mut shifted = String::with_capacity(text.len());
    shifted.push_str(&text[..at]);
    shifted.push(next);
    shifted.push_str(&text[at + 1..]);
    Some(shifted)
}

/// `text` lower-cased with every `exclude` phrase overwritten by spaces of
/// the same length, so offsets still point into `text`. `None` when
/// lower-casing changes the length.
fn masked_lowercase(text: &str, exclude: &[String]) -> Option<String> {
    let mut masked = text.to_lowercase();
    if masked.len() != text.len() {
        return None;
    }
    for phrase in exclude.iter().filter(|phrase| !phrase.is_empty()) {
        masked = masked.replace(phrase.as_str(), &" ".repeat(phrase.len()));
    }
    Some(masked)
}

/// Whether the match `start..end` of `text` stands between non-letters.
fn at_word_edges(text: &str, start: usize, end: usize) -> bool {
    let before = text[..start].chars().next_back();
    let after = text[end..].chars().next();
    before.is_none_or(|c| !c.is_alphanumeric()) && after.is_none_or(|c| !c.is_alphanumeric())
}

/// The byte ranges of the markers of `set` in `masked`, ordered and without
/// overlaps. Suffixes count wherever they occur.
fn marker_spans(masked: &str, set: &MarkerSet, words: bool) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let markers = set.markers.iter().map(|marker| (marker, words));
    let suffixes = set.suffixes.iter().map(|suffix| (suffix, false));
    for (marker, whole) in markers.chain(suffixes).filter(|(m, _)| !m.is_empty()) {
        for (start, found) in masked.match_indices(marker.as_str()) {
            let end = start + found.len();
            if !whole || at_word_edges(masked, start, end) {
                spans.push((start, end));
            }
        }
    }
    spans.sort_unstable();
    let mut kept: Vec<(usize, usize)> = Vec::new();
    for (start, end) in spans {
        if kept.last().is_none_or(|&(_, last_end)| start >= last_end) {
            kept.push((start, end));
        }
    }
    kept
}

/// `text` without the markers of `set`; `None` when the scan finds none.
fn drop_markers(text: &str, set: &MarkerSet, words: bool) -> Option<String> {
    let masked = masked_lowercase(text, &set.exclude)?;
    let spans = marker_spans(&masked, set, words);
    if spans.is_empty()
        || spans
            .iter()
            .any(|&(s, e)| !text.is_char_boundary(s) || !text.is_char_boundary(e))
    {
        return None;
    }
    let mut kept = String::with_capacity(text.len());
    let mut at = 0;
    for (start, end) in spans {
        kept.push_str(&text[at..start]);
        at = end;
    }
    kept.push_str(&text[at..]);
    Some(kept)
}

/// The marker table of a language and whether it is scanned by words.
fn marker_table<'a>(markers: &'a Markers, lang: &str) -> Option<(&'a LangMarkers, bool)> {
    match lang {
        "en" => Some((&markers.en, true)),
        "es" => markers.es.as_ref().map(|table| (table, true)),
        "zh" => markers.zh.as_ref().map(|table| (table, false)),
        "ko" => markers.ko.as_ref().map(|table| (table, false)),
        _ => None,
    }
}

/// `text` broken by `kind`, or `None` when the rule has nothing to break.
pub fn break_text(kind: Break, text: &str, table: &LangMarkers, words: bool) -> Option<String> {
    match kind {
        Break::Number => shift_first_digit(text),
        Break::Negation => drop_markers(text, &table.negation, words),
        Break::Uncertainty => drop_markers(text, &table.uncertainty, words),
    }
}

/// V with the target it judges for.
struct Judge {
    verifier: Verifier,
    target: Target,
}

impl Judge {
    fn open(lang: &str) -> Result<Judge, String> {
        let Some(target) = Target::from_code(lang) else {
            return Err(format!("{lang}: not a language of the table"));
        };
        let verifier = Verifier::open(None, &mut NullSink)?;
        Ok(Judge { verifier, target })
    }

    fn holds(&mut self, src: &str, text: &str) -> bool {
        let verdict = self
            .verifier
            .verify_into(src, text, Lang::Ja, &self.target, &mut NullSink);
        verdict != Verdict::Ok
    }

    /// The finding for `cases`, each a row and the text V is asked about.
    fn finding(&mut self, name: &str, cases: Vec<(&Row, String)>) -> Finding {
        let held = cases
            .iter()
            .filter(|(row, text)| self.holds(&row.ja, text))
            .map(|(row, _)| row.id.clone())
            .collect();
        Finding {
            name: name.to_owned(),
            rows: cases.len(),
            held,
        }
    }
}

/// The references as they stand, then each way of breaking them, through V.
pub fn probe(rows: &[Row], lang: &str) -> Result<Vec<Finding>, String> {
    let markers = Markers::parse(DEFAULT_MARKERS.as_bytes())?;
    let Some((table, words)) = marker_table(&markers, lang) else {
        return Err(format!("{lang}: no marker table"));
    };
    let mut judge = Judge::open(lang)?;
    let standing = rows
        .iter()
        .map(|row| (row, row.reference.clone()))
        .collect();
    let mut findings = vec![judge.finding("reference", standing)];
    for kind in Break::ALL {
        let broken = rows
            .iter()
            .filter_map(|row| Some((row, break_text(kind, &row.reference, table, words)?)))
            .collect();
        findings.push(judge.finding(kind.name(), broken));
    }
    Ok(findings)
}

/// One line without line breaks.
fn one_line(text: &str) -> String {
    text.replace(['\r', '\n'], " ")
}

/// T alone for one row: the language model's own answer.
fn translate<L: Llm>(llm: &L, target: &Target, src: &str) -> Result<String, String> {
    let answer = if target.lang() == Some(Lang::Ja.other()) {
        let prompt = translate_prompt_to(Lang::Ja, target, src);
        llm.complete(&Completion::new(&prompt, MAX_TOKENS))
    } else {
        llm.complete(&Completion::coded(
            src,
            Lang::Ja.code(),
            target.code,
            MAX_TOKENS,
        ))
    };
    answer.map_err(|error| format!("{}: {error}", target.code))
}

/// T alone for every row, without V and with V.
pub fn t_only<L: Llm>(llm: &L, rows: &[Row], lang: &str) -> Result<TOnly, String> {
    let mut judge = Judge::open(lang)?;
    let mut done = TOnly {
        raw: Vec::with_capacity(rows.len()),
        gated: Vec::with_capacity(rows.len()),
        held: Vec::new(),
    };
    for row in rows {
        let answer = translate(llm, &judge.target, &row.ja)?;
        let text = answer.trim();
        let withheld = contains_eos(text) || judge.holds(&row.ja, text);
        done.raw.push(one_line(text));
        done.gated.push(if withheld {
            String::new()
        } else {
            one_line(text)
        });
        if withheld {
            done.held.push(row.id.clone());
        }
    }
    Ok(done)
}

fn hypothesis_text(lines: &[String]) -> String {
    lines.iter().map(|line| format!("{line}\n")).collect()
}

/// Writes `<dir>/<lang>.hyp.txt`.
fn write_hypotheses(dir: &Path, lang: &str, lines: &[String]) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    let path = dir.join(format!("{lang}.hyp.txt"));
    fs::write(&path, hypothesis_text(lines)).map_err(|error| format!("{}: {error}", path.display()))
}

/// The report line of a finding.
pub fn report_line(lang: &str, finding: &Finding) -> String {
    format!(
        "{}\t{lang}\trows={}\theld={}\tids={}",
        finding.name,
        finding.rows,
        finding.held.len(),
        finding.held.join(",")
    )
}

/// Runs the probe and, with `--out`, the translator; returns the report.
fn execute(options: &Options) -> Result<Vec<String>, String> {
    let rows = read_rows(&options.pairs, &options.lang)?;
    let mut report: Vec<String> = probe(&rows, &options.lang)?
        .iter()
        .map(|finding| report_line(&options.lang, finding))
        .collect();
    let Some(out) = &options.out else {
        return Ok(report);
    };
    let url = if options.lang == "en" {
        &options.llm
    } else {
        &options.llm_other
    };
    let done = t_only(
        &HttpLlm::with_timeout(url, REQUEST_TIMEOUT),
        &rows,
        &options.lang,
    )?;
    write_hypotheses(&out.join("noV"), &options.lang, &done.raw)?;
    write_hypotheses(&out.join("V"), &options.lang, &done.gated)?;
    let finding = Finding {
        name: String::from("t_only"),
        rows: rows.len(),
        held: done.held,
    };
    report.push(report_line(&options.lang, &finding));
    Ok(report)
}

/// `vprobe`: returns the exit code, 0 done, 1 failed, 2 the command line was
/// refused.
pub fn run(args: &[String]) -> u8 {
    let options = match parse_options(args) {
        Ok(options) => options,
        Err(why) => {
            eprintln!("vprobe: {why}\n{USAGE}");
            return 2;
        }
    };
    match execute(&options) {
        Ok(report) => {
            for line in report {
                println!("{line}");
            }
            0
        }
        Err(why) => {
            eprintln!("vprobe: {why}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn markers() -> Result<Markers, String> {
        Markers::parse(DEFAULT_MARKERS.as_bytes())
    }

    fn broken(lang: &str, kind: Break, text: &str) -> Result<Option<String>, String> {
        let markers = markers()?;
        let Some((table, words)) = marker_table(&markers, lang) else {
            return Err(format!("no table for {lang}"));
        };
        Ok(break_text(kind, text, table, words))
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_owned()).collect()
    }

    #[test]
    fn the_number_rule_moves_the_first_digit_by_one() {
        assert_eq!(
            shift_first_digit("at 3 and 5"),
            Some(String::from("at 4 and 5"))
        );
        assert_eq!(
            shift_first_digit("会議は9時"),
            Some(String::from("会議は0時"))
        );
        assert_eq!(shift_first_digit("no digit"), None);
    }

    #[test]
    fn the_negation_rule_removes_a_word_marker_only_as_a_whole_word() -> Result<(), String> {
        let said = broken("en", Break::Negation, "It is not raining.")?;
        assert_eq!(said.as_deref(), Some("It is  raining."));
        assert_eq!(broken("en", Break::Negation, "A knot of rope.")?, None);
        Ok(())
    }

    #[test]
    fn a_phrase_the_table_excludes_is_not_a_negation() -> Result<(), String> {
        assert_eq!(broken("en", Break::Negation, "Not only cats sleep.")?, None);
        Ok(())
    }

    #[test]
    fn a_suffix_marker_counts_inside_a_word() -> Result<(), String> {
        let said = broken("en", Break::Negation, "He doesn't run.")?;
        assert_eq!(said.as_deref(), Some("He does run."));
        Ok(())
    }

    #[test]
    fn a_substring_language_loses_its_marker_wherever_it_stands() -> Result<(), String> {
        let said = broken("zh", Break::Negation, "我不去。")?;
        assert_eq!(said.as_deref(), Some("我去。"));
        Ok(())
    }

    #[test]
    fn the_uncertainty_rule_reads_the_uncertainty_table() -> Result<(), String> {
        let said = broken("en", Break::Uncertainty, "It will probably rain.")?;
        assert_eq!(said.as_deref(), Some("It will  rain."));
        assert_eq!(
            broken("en", Break::Uncertainty, "It is not raining.")?,
            None
        );
        Ok(())
    }

    #[test]
    fn the_options_need_pairs_and_a_language_of_the_table() {
        assert!(parse_options(&args(&["--lang", "en"])).is_err());
        assert!(parse_options(&args(&["--pairs", "p", "--lang", "fr"])).is_err());
        assert!(parse_options(&args(&["--pairs", "p", "--lang", "en", "--serve", "1"])).is_err());
        assert!(parse_options(&args(&["--pairs"])).is_err());
    }

    #[test]
    fn the_options_default_to_the_relays_translators_and_no_output() -> Result<(), String> {
        let options = parse_options(&args(&["--pairs", "p", "--lang", "ko"]))?;
        assert_eq!(options.out, None);
        assert_eq!(options.llm, DEFAULT_LLM);
        assert_eq!(options.llm_other, DEFAULT_LLM_OTHER);
        Ok(())
    }

    #[test]
    fn the_file_has_one_line_per_row_and_the_report_names_the_ids() {
        let lines = [String::from("a"), String::new(), String::from("c")];
        assert_eq!(hypothesis_text(&lines), "a\n\nc\n");
        assert_eq!(hypothesis_text(&[]), "");
        let finding = Finding {
            name: String::from("reference"),
            rows: 50,
            held: vec![String::from("7"), String::from("9")],
        };
        assert_eq!(
            report_line("zh", &finding),
            "reference\tzh\trows=50\theld=2\tids=7,9"
        );
    }
}

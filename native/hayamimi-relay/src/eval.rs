//! `eval`: the Japanese side of sentence pairs, translated into the language
//! each file names, for a scorer to read.
//!
//! `<pairs>/<code>.ja.txt` holds one sentence per line. `<out>/<code>.hyp.txt`
//! gets the settled translation of each line, in the same order. A sentence
//! that verification withheld in both of its translations is an empty line, so
//! the lines stay aligned with the reference file.
//!
//! A code the translator does not write is left out with a warning and the
//! other languages go on. A translator that cannot be reached stops the run: a
//! score over empty answers would pass for a measurement. The tables are the
//! built-in ones, so that a table file on the machine cannot change a score.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use crate::lang::Lang;
use crate::llm::{Completion, Llm, LlmError};
use crate::logging::{Level, LogEvent, LogSink};
use crate::pipeline::{MAX_TOKENS, edit_prompt_to, translate_prompt_to};
use crate::reach::{ERROR_CODE, STAGE};
use crate::style::{Calques, JaStyle};
use crate::target::Target;
use crate::verify::{Verdict, Verifier, contains_eos};

/// The log event of a language that is left out.
pub const SKIPPED_EVENT: &str = "translate.fallback.skipped";
/// The log event of one language's file written.
pub const WRITTEN_EVENT: &str = "translate.eval.written";
/// The reason of the skipped event.
pub const UNSUPPORTED: &str = "unsupported_target";

const SOURCE_SUFFIX: &str = ".ja.txt";
const HYPOTHESIS_SUFFIX: &str = ".hyp.txt";

/// Why the run stopped.
#[derive(Debug)]
pub enum EvalError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    /// A built-in table could not be read.
    Table(String),
    /// The translator did not answer for a line of one language.
    Llm {
        code: &'static str,
        line: usize,
        source: LlmError,
    },
    /// The directory holds no `<code>.ja.txt`.
    NoPairs(PathBuf),
}

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EvalError::Io { path, source } => write!(f, "{}: {source}", path.display()),
            EvalError::Table(why) => write!(f, "table: {why}"),
            EvalError::Llm { code, line, source } => {
                write!(f, "{code} line {line}: {source}")
            }
            EvalError::NoPairs(dir) => write!(f, "{}: no *{SOURCE_SUFFIX} file", dir.display()),
        }
    }
}

impl std::error::Error for EvalError {}

/// Where `eval` reads and writes.
pub struct EvalOptions<'a> {
    pub pairs: &'a Path,
    pub out: &'a Path,
}

/// What was written for one language.
#[derive(Debug, PartialEq, Eq)]
pub struct Written {
    pub code: &'static str,
    pub lines: usize,
    /// Lines that are empty because verification withheld them.
    pub held: usize,
}

/// What a run did: the languages written and the codes left out.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub written: Vec<Written>,
    pub skipped: Vec<String>,
}

struct PairFile {
    code: String,
    path: PathBuf,
}

fn io_error(path: &Path, source: std::io::Error) -> EvalError {
    EvalError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// The `<code>.ja.txt` files of `dir`, ordered by code.
fn pair_files(dir: &Path) -> Result<Vec<PairFile>, EvalError> {
    let entries = fs::read_dir(dir).map_err(|source| io_error(dir, source))?;
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| io_error(dir, source))?;
        let name = entry.file_name();
        let Some(code) = name
            .to_str()
            .and_then(|name| name.strip_suffix(SOURCE_SUFFIX))
        else {
            continue;
        };
        found.push(PairFile {
            code: code.to_owned(),
            path: entry.path(),
        });
    }
    found.sort_by(|left, right| left.code.cmp(&right.code));
    Ok(found)
}

/// The text a line settles on: the edited translation when it passed
/// verification, otherwise the first translation when that passed and the
/// edit did not carry the end marker, otherwise nothing.
fn settled_text(draft: &str, draft_ok: bool, edited: &str, edited_ok: bool) -> Option<String> {
    if edited_ok {
        return Some(one_line(edited));
    }
    (draft_ok && !contains_eos(edited)).then(|| one_line(draft))
}

/// `text` without line breaks, so that one translation stays one line.
fn one_line(text: &str) -> String {
    text.replace(['\r', '\n'], " ")
}

/// The file text of translations: one line each, empty for a withheld one.
fn hypothesis_text(lines: &[Option<String>]) -> String {
    let mut text = String::new();
    for line in lines {
        if let Some(translation) = line {
            text.push_str(translation);
        }
        text.push('\n');
    }
    text
}

/// Writes `text` to a sibling of `path` and renames it over `path`.
fn write_by_rename(path: &Path, text: &str) -> Result<(), EvalError> {
    let swap = path.with_extension("swap");
    fs::write(&swap, text).map_err(|source| io_error(&swap, source))?;
    fs::rename(&swap, path).map_err(|source| io_error(path, source))
}

/// T, E and V around a translator.
struct Translator<'a, L: Llm> {
    llm: &'a L,
    verifier: Verifier,
    calques: Calques,
    ja_style: JaStyle,
}

impl<'a, L: Llm> Translator<'a, L> {
    fn open(llm: &'a L, sink: &mut dyn LogSink) -> Result<Translator<'a, L>, EvalError> {
        Ok(Translator {
            llm,
            verifier: Verifier::open(None, sink).map_err(EvalError::Table)?,
            calques: Calques::embedded().map_err(EvalError::Table)?,
            ja_style: JaStyle::embedded().map_err(EvalError::Table)?,
        })
    }

    fn ask(&self, prompt: &str) -> Result<String, LlmError> {
        self.llm.complete(&Completion::new(prompt, MAX_TOKENS))
    }

    fn passes(&mut self, src: &str, text: &str, target: &Target, sink: &mut dyn LogSink) -> bool {
        self.verifier.verify_into(src, text, Lang::Ja, target, sink) == Verdict::Ok
    }

    /// The settled translation of one Japanese sentence, or `None` when it
    /// is withheld.
    fn settle(
        &mut self,
        target: &Target,
        src: &str,
        sink: &mut dyn LogSink,
    ) -> Result<Option<String>, LlmError> {
        let first = self.ask(&translate_prompt_to(Lang::Ja, target, src))?;
        if contains_eos(&first) {
            return Ok(None);
        }
        let draft = first.trim();
        let draft_ok = self.passes(src, draft, target, sink);
        let prompt = edit_prompt_to(Lang::Ja, target, src, draft, &self.calques, &self.ja_style);
        let second = self.ask(&prompt)?;
        let edited = second.trim();
        let edited_ok = self.passes(src, edited, target, sink);
        Ok(settled_text(draft, draft_ok, edited, edited_ok))
    }

    /// The settled translations of every line of one pairs file.
    fn translate_file(
        &mut self,
        target: &Target,
        file: &PairFile,
        sink: &mut dyn LogSink,
    ) -> Result<Vec<Option<String>>, EvalError> {
        let text = fs::read_to_string(&file.path).map_err(|source| io_error(&file.path, source))?;
        let mut lines = Vec::new();
        for (index, src) in text.lines().enumerate() {
            let line = self
                .settle(target, src, sink)
                .map_err(|source| EvalError::Llm {
                    code: target.code,
                    line: index + 1,
                    source,
                })?;
            lines.push(line);
        }
        Ok(lines)
    }
}

fn skipped_event(code: &str) -> LogEvent {
    let mut event = LogEvent::new(Level::Warn, SKIPPED_EVENT, "service")
        .with("stage", STAGE)
        .with("reason", UNSUPPORTED)
        .with("target", code);
    event.error_code = Some(ERROR_CODE.to_owned());
    event
}

fn written_event(written: &Written) -> LogEvent {
    LogEvent::new(Level::Info, WRITTEN_EVENT, "service")
        .with("target", written.code)
        .with("lines", written.lines)
        .with("held", written.held)
}

/// Translates every `<code>.ja.txt` of `options.pairs` and writes the
/// `<code>.hyp.txt` files into `options.out`.
pub fn run<L: Llm>(
    llm: &L,
    options: &EvalOptions<'_>,
    sink: &mut dyn LogSink,
) -> Result<Report, EvalError> {
    let files = pair_files(options.pairs)?;
    if files.is_empty() {
        return Err(EvalError::NoPairs(options.pairs.to_path_buf()));
    }
    fs::create_dir_all(options.out).map_err(|source| io_error(options.out, source))?;
    let mut translator = Translator::open(llm, sink)?;
    let mut report = Report::default();
    for file in &files {
        let Some(target) = Target::from_code(&file.code) else {
            sink.emit(skipped_event(&file.code));
            report.skipped.push(file.code.clone());
            continue;
        };
        let lines = translator.translate_file(&target, file, sink)?;
        let path = options
            .out
            .join(format!("{}{HYPOTHESIS_SUFFIX}", target.code));
        write_by_rename(&path, &hypothesis_text(&lines))?;
        let written = Written {
            code: target.code,
            lines: lines.len(),
            held: lines.iter().filter(|line| line.is_none()).count(),
        };
        sink.emit(written_event(&written));
        report.written.push(written);
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_edit_stands_when_it_passed() {
        let said = settled_text("draft", true, "edited", true);
        assert_eq!(said.as_deref(), Some("edited"));
        // A draft that failed does not matter once the edit passed.
        let said = settled_text("draft", false, "edited", true);
        assert_eq!(said.as_deref(), Some("edited"));
    }

    #[test]
    fn the_draft_stands_when_only_the_edit_failed() {
        let said = settled_text("draft", true, "edited", false);
        assert_eq!(said.as_deref(), Some("draft"));
    }

    #[test]
    fn a_line_is_withheld_when_both_failed() {
        assert_eq!(settled_text("draft", false, "edited", false), None);
    }

    #[test]
    fn an_edit_with_the_end_marker_does_not_fall_back_to_the_draft() {
        assert_eq!(settled_text("draft", true, "edited</s>", false), None);
    }

    #[test]
    fn a_translation_stays_on_one_line() {
        let said = settled_text("d", true, "first\nsecond\r\nthird", true);
        assert_eq!(said.as_deref(), Some("first second  third"));
    }

    #[test]
    fn the_file_has_one_line_per_sentence_and_an_empty_line_for_a_held_one() {
        let lines = [Some(String::from("a")), None, Some(String::from("c"))];
        assert_eq!(hypothesis_text(&lines), "a\n\nc\n");
        assert_eq!(hypothesis_text(&[]), "");
    }

    #[test]
    fn the_skipped_event_names_the_code_and_the_reason() {
        let event = skipped_event("xx");
        assert_eq!(event.event, SKIPPED_EVENT);
        assert_eq!(event.level, Level::Warn);
        assert_eq!(
            event.error_code.as_deref(),
            Some("HAYAMIMI_TRANSLATE_FALLBACK")
        );
        assert_eq!(
            event.fields.get("reason"),
            Some(&serde_json::json!("unsupported_target"))
        );
        assert_eq!(event.fields.get("target"), Some(&serde_json::json!("xx")));
    }

    #[test]
    fn the_written_event_carries_counts_and_no_text() {
        let event = written_event(&Written {
            code: "es",
            lines: 50,
            held: 3,
        });
        assert_eq!(event.fields.get("lines"), Some(&serde_json::json!(50)));
        assert_eq!(event.fields.get("held"), Some(&serde_json::json!(3)));
    }
}

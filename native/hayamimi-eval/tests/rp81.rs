//! RP-81: documents and comments do not name a removed Python file.
//!
//! `check_python_names` reads every tracked text file line by line and looks
//! for two things: a path of the shape `scripts/<name>.py`, `demo/<name>.py` or
//! `tests/test_<name>.py` (with `/` or `\` as the separator), and the bare name
//! of one of the removed files (`download_models.py`). A hit passes only when
//! its file is a record, a checking tool, an owned exception that lists the
//! line, or a file that keeps one string a program prints. Every other hit is
//! an `Err` that names the file and the line.
//!
//! The removed files are listed in `rp81_removed_python.txt`; the owned
//! exceptions are listed in `rp81_owned_lines.tsv` (file, line, owner slice,
//! separated by tabs). Each list is pinned by its sha256 and its row count, so
//! a list cannot change without the pin changing in the same commit.

mod support;

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use support::{Failure, REPO, TestResult, sha256_hex};

const REMOVED_LIST: &str = include_str!("rp81_removed_python.txt");
const OWNED_LIST: &str = include_str!("rp81_owned_lines.tsv");
const README_BEFORE_R11: &str = include_str!("rp81_before_r11/README.md.txt");

/// The sha256 and the row count of `rp81_removed_python.txt`.
const REMOVED_PYTHON_SHA256: &str =
    "f223d62505d89d662b88c8ef5f822e1edf589af961949197bdcac7ecd9579220";
const REMOVED_PYTHON_ROWS: usize = 78;
/// The sha256 and the row count of `rp81_owned_lines.tsv`.
const OWNED_LINES_SHA256: &str = "f6ca926ced47803722788526e97b5b781c3362b87db8d9db4bf216614354258a";
const OWNED_LINES_ROWS: usize = 18;

/// Directories that hold records, which are not rewritten afterwards.
const RECORD_DIRS: [&str; 3] = ["docs/eval/", "docs/design/", "docs/results/"];
/// Files that hold records, named by their whole path.
const RECORD_FILES: [&str; 4] = [
    "mobile/hayamimi_core/CHANGELOG.md",
    "tests/golden/ja/golden.json",
    "mobile/hayamimi_core/test/fixtures/punct_ja_parity.json",
    "docs/verify/android_emulator.md",
];
/// The checking tool: three files and the fixture directories `rp81_*/`.
const TOOL_FILES: [&str; 3] = [
    "native/hayamimi-eval/tests/rp81.rs",
    "native/hayamimi-eval/tests/rp81_removed_python.txt",
    "native/hayamimi-eval/tests/rp81_owned_lines.tsv",
];
const TOOL_DIR_PARENT: &str = "native/hayamimi-eval/tests/";
const TOOL_DIR_PREFIX: &str = "rp81_";
/// Files that another slice rewrites; a hit passes only when the tsv lists it.
const OWNED_FILES: [&str; 4] = [
    "docs/guide/embedding.md",
    "docs/guide/embedding.ja.md",
    "docs/guide/tuning.md",
    "docs/guide/tuning.ja.md",
];
/// Strings that a program prints or runs, kept by file: a hit passes when its
/// line holds the string that its file keeps.
const KEPT_STRINGS: [(&str, &str); 5] = [
    (
        "mobile/hayamimi_core/test/punct/punct_ja_fixture_tokens_test.dart",
        "differently from scripts/punct_ja.py",
    ),
    (
        "mobile/hayamimi_core/test/punct/punct_ja_parity_test.dart",
        "PunctuatorJa matches scripts/punct_ja.py",
    ),
    (
        "mobile/hayamimi_core/test/punct/punct_ja_parity_test.dart",
        "python scripts/quantize_punct.py --variant fp16",
    ),
    (
        "mobile/hayamimi_core/test/punct/punct_worker_load_test.dart",
        "python scripts/quantize_punct.py --variant fp16",
    ),
    ("mobile/lib/main.dart", "scripts/eval_accuracy.py"),
];
/// The bytes of a file that git reads to decide that it is binary.
const BINARY_PROBE_BYTES: usize = 8000;

/// A tracked file: its path and its text.
type Tracked = (String, String);

// ---------------------------------------------------------------------------
// The pattern: `(scripts|demo)[/\\][a-z_0-9]+\.py|tests[/\\]test_[a-z_0-9]+\.py`,
// and a bare name that no letter, digit, `_`, `/` or `\` stands in front of.
// ---------------------------------------------------------------------------

fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'
}

fn is_separator(byte: u8) -> bool {
    byte == b'/' || byte == b'\\'
}

/// Whether `rest` starts with a non-empty run of name bytes and then `.py`.
fn name_then_py(rest: &[u8]) -> bool {
    let run = rest.iter().take_while(|byte| is_name_byte(**byte)).count();
    run > 0 && rest[run..].starts_with(b".py")
}

/// Whether `rest` starts with `<dir><separator>` and then what `after` accepts.
fn dir_then(rest: &[u8], dir: &[u8], after: impl Fn(&[u8]) -> bool) -> bool {
    rest.strip_prefix(dir)
        .and_then(<[u8]>::split_first)
        .is_some_and(|(separator, name)| is_separator(*separator) && after(name))
}

fn path_shape_at(rest: &[u8]) -> bool {
    dir_then(rest, b"scripts", name_then_py)
        || dir_then(rest, b"demo", name_then_py)
        || dir_then(rest, b"tests", |name| {
            name.strip_prefix(b"test_").is_some_and(name_then_py)
        })
}

fn names_a_path(line: &str) -> bool {
    let bytes = line.as_bytes();
    (0..bytes.len()).any(|at| path_shape_at(&bytes[at..]))
}

/// The name of each removed file, without its directory (`download_models.py`).
fn bare_names<'a>(removed: &[&'a str]) -> Vec<&'a str> {
    removed
        .iter()
        .map(|path| match path.rsplit_once('/') {
            Some((_, name)) => name,
            None => path,
        })
        .collect()
}

fn names_a_bare(line: &str, names: &[&str]) -> bool {
    let bytes = line.as_bytes();
    names.iter().any(|name| {
        line.match_indices(name)
            .any(|(at, _)| at == 0 || !(is_name_byte(bytes[at - 1]) || is_separator(bytes[at - 1])))
    })
}

fn line_hits(line: &str, names: &[&str]) -> bool {
    line.contains(".py") && (names_a_path(line) || names_a_bare(line, names))
}

// ---------------------------------------------------------------------------
// The lists and their pins.
// ---------------------------------------------------------------------------

/// One row of `rp81_owned_lines.tsv`.
struct OwnedRow<'a> {
    file: &'a str,
    line: &'a str,
    owner: &'a str,
}

fn parse_owned(text: &str) -> Result<Vec<OwnedRow<'_>>, Failure> {
    let mut rows = Vec::new();
    for row in text.lines().filter(|row| !row.is_empty()) {
        let mut fields = row.split('\t');
        let (Some(file), Some(line), Some(owner), None) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            return Err(format!(
                "an owned-lines row is not file, line, owner: {row:?}"
            ));
        };
        rows.push(OwnedRow { file, line, owner });
    }
    Ok(rows)
}

fn row_count(text: &str) -> usize {
    text.lines().filter(|row| !row.is_empty()).count()
}

/// The text has the pinned row count and the pinned sha256.
fn check_pins(what: &str, text: &str, sha256: &str, rows: usize) -> Result<(), Failure> {
    let mut problems = Vec::new();
    let found_rows = row_count(text);
    if found_rows != rows {
        problems.push(format!("{what}: {found_rows} rows, the pin says {rows}"));
    }
    let found_sha256 = sha256_hex(text.as_bytes());
    if found_sha256 != sha256 {
        problems.push(format!(
            "{what}: sha256 {found_sha256}, the pin says {sha256}"
        ));
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("\n"))
    }
}

/// The removed-file list matches its pins, and every row has the path shape.
fn check_removed_list(text: &str, sha256: &str, rows: usize) -> Result<(), Failure> {
    check_pins("rp81_removed_python.txt", text, sha256, rows)?;
    let missed: Vec<&str> = text
        .lines()
        .filter(|row| !row.is_empty() && !names_a_path(row))
        .collect();
    if missed.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "rows the pattern does not match: {}",
            missed.join(", ")
        ))
    }
}

fn check_owned_lines(text: &str, sha256: &str, rows: usize) -> Result<(), Failure> {
    check_pins("rp81_owned_lines.tsv", text, sha256, rows)
}

/// The owned-lines list is empty; otherwise the number of rows per owner.
fn check_owned_exceptions_gone(text: &str) -> Result<(), Failure> {
    let rows = parse_owned(text)?;
    let mut per_owner: BTreeMap<&str, usize> = BTreeMap::new();
    for row in &rows {
        *per_owner.entry(row.owner).or_insert(0) += 1;
    }
    if per_owner.is_empty() {
        return Ok(());
    }
    let counts: Vec<String> = per_owner
        .iter()
        .map(|(owner, count)| format!("{owner}: {count}"))
        .collect();
    Err(format!(
        "owned exceptions remain, per owner: {}",
        counts.join(", ")
    ))
}

// ---------------------------------------------------------------------------
// The check over a tree.
// ---------------------------------------------------------------------------

fn is_record(path: &str) -> bool {
    RECORD_DIRS.iter().any(|dir| path.starts_with(dir)) || RECORD_FILES.contains(&path)
}

/// `native/hayamimi-eval/tests/rp81_<x>/...`: a fixture directory of the tool.
fn is_tool_directory(path: &str) -> bool {
    path.strip_prefix(TOOL_DIR_PARENT)
        .and_then(|rest| rest.split_once('/'))
        .is_some_and(|(directory, _)| directory.starts_with(TOOL_DIR_PREFIX))
}

fn is_tool(path: &str) -> bool {
    TOOL_FILES.contains(&path) || is_tool_directory(path)
}

fn is_allowed_line(path: &str, line: &str, owned: &[OwnedRow<'_>]) -> bool {
    let listed =
        OWNED_FILES.contains(&path) && owned.iter().any(|row| row.file == path && row.line == line);
    let kept = KEPT_STRINGS
        .iter()
        .any(|(file, text)| *file == path && line.contains(*text));
    listed || kept
}

fn check_python_names(
    files: &[Tracked],
    removed: &[&str],
    owned: &[OwnedRow<'_>],
) -> Result<(), Failure> {
    let names = bare_names(removed);
    let mut hits = Vec::new();
    for (path, text) in files {
        if is_record(path) || is_tool(path) {
            continue;
        }
        for (index, line) in text.lines().enumerate() {
            if line_hits(line, &names) && !is_allowed_line(path, line, owned) {
                hits.push(format!("{path}:{}: {}", index + 1, line.trim()));
            }
        }
    }
    if hits.is_empty() {
        Ok(())
    } else {
        Err(hits.join("\n"))
    }
}

/// Every tracked text file of the checkout, with its text.
fn tracked_files(root: &Path) -> Result<Vec<Tracked>, Failure> {
    let listing = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z"])
        .output()
        .map_err(|error| format!("cannot run git: {error}"))?;
    if !listing.status.success() {
        return Err("git ls-files failed".to_owned());
    }
    let mut files = Vec::new();
    for raw in listing
        .stdout
        .split(|byte| *byte == 0)
        .filter(|raw| !raw.is_empty())
    {
        let path =
            std::str::from_utf8(raw).map_err(|_| "a tracked path is not UTF-8".to_owned())?;
        let bytes = std::fs::read(root.join(path))
            .map_err(|error| format!("cannot read {path}: {error}"))?;
        if bytes.iter().take(BINARY_PROBE_BYTES).any(|byte| *byte == 0) {
            continue;
        }
        files.push((
            path.to_owned(),
            String::from_utf8_lossy(&bytes).into_owned(),
        ));
    }
    Ok(files)
}

// ---------------------------------------------------------------------------
// The base fixture (F81) and the edits the cases make to it.
// ---------------------------------------------------------------------------

const FIXTURE_REMOVED: [&str; 3] = [
    "scripts/download_models.py",
    "demo/capture_events.py",
    "tests/test_units.py",
];
const FIXTURE_REMOVED_TEXT: &str =
    "scripts/download_models.py\ndemo/capture_events.py\ntests/test_units.py\n";
const FIXTURE_REMOVED_SHA256: &str =
    "497be68c42681746804ebf7cd23722d6a98b49ee3d95993711604aaf76b20d0c";
const OWNED_LINE: &str = "the engine is scripts/asr_engine.py today";
const FIXTURE_OWNED_SHA256: &str =
    "c2fdb416546f4900f50bd26160a82ad44c579f6c59560332b43d1a170abef48b";
const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

const PARITY_TEST: &str = "mobile/hayamimi_core/test/punct/punct_ja_parity_test.dart";
const TUNING_GUIDE: &str = "docs/guide/tuning.md";
const SPEC_JSON: &str = "docs/spec/ja_pipeline_spec.json";
const TESTS_DIR: &str = "native/hayamimi-eval/tests";

fn file(path: &str, text: &str) -> Tracked {
    (path.to_owned(), text.to_owned())
}

fn f81() -> Vec<Tracked> {
    vec![
        file("README.md", "```\ncargo run --release -p hayamimi\n```\n"),
        file("docs/guide/embedding.md", "run scripts/x.sh once\n"),
        file("native/hayamimi/src/lib.rs", "//! the library\n"),
        file(
            "mobile/lib/main.dart",
            "'with scripts/eval_accuracy.py)'\n'scripts/eval_accuracy.py\\'s cer_ja)'\n",
        ),
        file(
            "docs/design/rust_port.ja.md",
            "the old engine was scripts/eval_engine.py\n",
        ),
    ]
}

/// `base` with one line added to `path`; the file is made when it is not there.
fn with_line(base: &[Tracked], path: &str, line: &str) -> Vec<Tracked> {
    let mut files = base.to_vec();
    match files.iter_mut().find(|entry| entry.0 == path) {
        Some(entry) => {
            entry.1.push_str(line);
            entry.1.push('\n');
        }
        None => files.push(file(path, &format!("{line}\n"))),
    }
    files
}

fn with_text(base: &[Tracked], path: &str, text: &str) -> Vec<Tracked> {
    let mut files: Vec<Tracked> = base
        .iter()
        .filter(|entry| entry.0 != path)
        .cloned()
        .collect();
    files.push(file(path, text));
    files
}

fn judge(files: &[Tracked]) -> Result<(), Failure> {
    check_python_names(files, &FIXTURE_REMOVED, &[])
}

fn accepted(verdict: Result<(), Failure>) -> TestResult {
    verdict.map_err(Into::into)
}

/// The verdict is an `Err` that names every needle.
fn rejected(verdict: Result<(), Failure>, needles: &[&str]) -> TestResult {
    let Err(said) = verdict else {
        return Err("the check accepted a fixture it should reject".into());
    };
    match needles.iter().find(|needle| !said.contains(*needle)) {
        Some(missing) => Err(format!("{missing:?} is not named in: {said}").into()),
        None => Ok(()),
    }
}

/// Each line added to `path` of the base fixture is rejected, naming `path`
/// and the line.
fn each_added_line_is_rejected(path: &str, lines: &[&str]) -> TestResult {
    for line in lines {
        let files = with_line(&f81(), path, line);
        rejected(judge(&files), &[path, line])?;
    }
    Ok(())
}

fn each_added_line_passes(path: &str, line: &str) -> TestResult {
    accepted(judge(&with_line(&f81(), path, line)))
}

/// The owned files, each holding `line`, and the tsv that lists them.
fn owned_fixture(line: &str) -> (Vec<Tracked>, String) {
    let files = OWNED_FILES
        .iter()
        .map(|path| file(path, &format!("{line}\n")))
        .collect();
    let rows = OWNED_FILES
        .iter()
        .map(|path| format!("{path}\t{OWNED_LINE}\tR13-14\n"))
        .collect();
    (files, rows)
}

#[test]
fn a_tree_without_a_removed_python_name_passes() -> TestResult {
    accepted(judge(&f81()))
}

#[test]
fn a_guide_that_names_a_removed_script_is_rejected_by_name() -> TestResult {
    each_added_line_is_rejected("docs/guide/embedding.md", &["scripts/x.py"])
}

#[test]
fn the_readme_before_r11_has_sixteen_hits_and_is_rejected() -> TestResult {
    let files = with_text(&f81(), "README.md", README_BEFORE_R11);
    let Err(said) = check_python_names(&files, &[], &[]) else {
        return Err("the README before R11 was accepted".into());
    };
    let hits = said
        .lines()
        .filter(|line| line.starts_with("README.md:"))
        .count();
    if hits == 16 {
        Ok(())
    } else {
        Err(format!("{hits} hits in README.md, not 16: {said}").into())
    }
}

#[test]
fn the_three_record_directories_may_name_a_python_file() -> TestResult {
    for path in ["docs/eval/a.md", "docs/design/a.md", "docs/results/a.md"] {
        each_added_line_passes(path, "scripts/x.py")?;
    }
    Ok(())
}

#[test]
fn a_directory_that_only_shares_a_prefix_is_not_a_record_directory() -> TestResult {
    for path in [
        "docs/evaluation/a.md",
        "docs/designs/a.md",
        "docs/results2/a.md",
    ] {
        each_added_line_is_rejected(path, &["scripts/x.py"])?;
    }
    Ok(())
}

#[test]
fn the_four_record_files_may_name_a_python_file() -> TestResult {
    for path in RECORD_FILES {
        each_added_line_passes(path, "scripts/x.py")?;
    }
    Ok(())
}

#[test]
fn a_changelog_at_another_path_is_not_a_record_file() -> TestResult {
    for path in ["mobile/CHANGELOG.md", "CHANGELOG.md"] {
        each_added_line_is_rejected(path, &["scripts/x.py"])?;
    }
    Ok(())
}

/// The three Dart test files and `main.dart`, each holding only what it keeps.
fn kept_strings_only() -> Vec<Tracked> {
    let mut files = f81();
    for (path, text) in KEPT_STRINGS {
        files = with_line(&files, path, text);
    }
    files
}

#[test]
fn the_six_kept_dart_strings_pass() -> TestResult {
    accepted(judge(&kept_strings_only()))
}

#[test]
fn an_extra_hit_in_a_file_with_a_kept_string_is_rejected() -> TestResult {
    let comment = "/// the port of scripts/punct_ja.py";
    let files = with_line(&kept_strings_only(), PARITY_TEST, comment);
    rejected(judge(&files), &["punct_ja_parity_test.dart", comment])
}

#[test]
fn a_kept_string_in_another_file_is_rejected() -> TestResult {
    let moved = "mobile/hayamimi_core/test/punct/punct_ja_text_test.dart";
    let files = with_line(&f81(), moved, "PunctuatorJa matches scripts/punct_ja.py");
    rejected(judge(&files), &[moved])
}

#[test]
fn names_that_only_resemble_a_removed_python_file_pass() -> TestResult {
    each_added_line_passes("README.md", "see scripts/x.sh")?;
    each_added_line_passes("README.md", "see tests/test_a.rs")
}

#[test]
fn both_removed_path_forms_are_matched() -> TestResult {
    each_added_line_is_rejected("README.md", &["scripts/a1_b2.py", "tests/test_a.py"])
}

#[test]
fn a_source_comment_naming_a_removed_script_is_rejected() -> TestResult {
    each_added_line_is_rejected(
        "native/hayamimi/src/lib.rs",
        &["// ported from scripts/asr_engine.py"],
    )
}

#[test]
fn rp81_no_document_names_a_removed_python_file() -> TestResult {
    let removed: Vec<&str> = REMOVED_LIST.lines().filter(|row| !row.is_empty()).collect();
    check_removed_list(REMOVED_LIST, REMOVED_PYTHON_SHA256, REMOVED_PYTHON_ROWS)?;
    check_owned_lines(OWNED_LIST, OWNED_LINES_SHA256, OWNED_LINES_ROWS)?;
    let owned = parse_owned(OWNED_LIST)?;
    let files = tracked_files(Path::new(REPO))?;
    let python: Vec<&str> = files
        .iter()
        .map(|(path, _)| path.as_str())
        .filter(|path| path.ends_with(".py"))
        .collect();
    if !python.is_empty() {
        return Err(format!(
            "git still tracks {} Python files: {}",
            python.len(),
            python.join(", ")
        )
        .into());
    }
    accepted(check_python_names(&files, &removed, &owned))
}

#[test]
fn the_removed_python_list_matches_its_two_pins() -> TestResult {
    accepted(check_removed_list(
        FIXTURE_REMOVED_TEXT,
        FIXTURE_REMOVED_SHA256,
        FIXTURE_REMOVED.len(),
    ))
}

#[test]
fn a_removed_python_list_that_drifted_from_its_pins_is_rejected() -> TestResult {
    let drifted = format!("{FIXTURE_REMOVED_TEXT}scripts/extra.py\n");
    let verdict = check_removed_list(&drifted, FIXTURE_REMOVED_SHA256, FIXTURE_REMOVED.len());
    rejected(verdict, &["4 rows", "pin says 3", "sha256"])
}

#[test]
fn every_line_of_the_removed_python_list_is_matched_by_the_pattern() -> TestResult {
    accepted(check_removed_list(
        FIXTURE_REMOVED_TEXT,
        FIXTURE_REMOVED_SHA256,
        FIXTURE_REMOVED.len(),
    ))?;
    let real_rows = REMOVED_LIST.lines().filter(|row| !row.is_empty()).count();
    let matched = REMOVED_LIST.lines().filter(|row| names_a_path(row)).count();
    if real_rows == matched {
        Ok(())
    } else {
        Err(format!("{matched} of {real_rows} listed paths match the pattern").into())
    }
}

#[test]
fn a_removed_list_line_the_pattern_misses_is_rejected() -> TestResult {
    let text = format!("{FIXTURE_REMOVED_TEXT}tools/x.py\n");
    let verdict = check_removed_list(&text, &sha256_hex(text.as_bytes()), 4);
    rejected(verdict, &["tools/x.py"])
}

#[test]
fn a_backslash_path_to_a_removed_script_is_rejected() -> TestResult {
    each_added_line_is_rejected(
        "README.md",
        &[
            "scripts\\x.py",
            ".venv\\Scripts\\python scripts\\realtime_transcribe.py",
            "tests\\test_a.py",
        ],
    )
}

#[test]
fn a_demo_script_path_is_rejected() -> TestResult {
    each_added_line_is_rejected(
        "README.md",
        &["demo/x.py", "demo\\x.py", "demo/capture_events.py"],
    )
}

#[test]
fn a_bare_removed_file_name_is_rejected() -> TestResult {
    each_added_line_is_rejected("README.md", &["python download_models.py"])
}

#[test]
fn a_bare_python_name_outside_the_removed_list_passes() -> TestResult {
    each_added_line_passes("README.md", "python my_tool.py")
}

#[test]
fn an_unlisted_line_in_an_owned_file_is_rejected() -> TestResult {
    each_added_line_is_rejected(TUNING_GUIDE, &[OWNED_LINE])
}

#[test]
fn the_listed_lines_of_an_owned_file_pass() -> TestResult {
    let (files, rows) = owned_fixture(OWNED_LINE);
    let owned = parse_owned(&rows)?;
    accepted(check_python_names(&files, &FIXTURE_REMOVED, &owned))
}

#[test]
fn a_listed_line_whose_text_changed_is_rejected() -> TestResult {
    let changed = "the engine is scripts/asr_engine.py tomorrow";
    let (files, rows) = owned_fixture(OWNED_LINE);
    let files = with_text(&files, TUNING_GUIDE, &format!("{changed}\n"));
    let owned = parse_owned(&rows)?;
    let verdict = check_python_names(&files, &FIXTURE_REMOVED, &owned);
    rejected(verdict, &[TUNING_GUIDE, changed])
}

#[test]
fn the_spec_json_is_owned_and_no_longer_a_record_file() -> TestResult {
    each_added_line_is_rejected(SPEC_JSON, &[OWNED_LINE])
}

#[test]
fn an_owned_lines_file_that_drifted_from_its_pins_is_rejected() -> TestResult {
    let one = format!("{TUNING_GUIDE}\t{OWNED_LINE}\tR13-14\n");
    accepted(check_owned_lines(&one, FIXTURE_OWNED_SHA256, 1))?;
    let two = format!("{one}{TUNING_GUIDE}\tanother scripts/x.py line\tR13-14\n");
    let verdict = check_owned_lines(&two, FIXTURE_OWNED_SHA256, 1);
    rejected(verdict, &["2 rows", "pin says 1", "sha256"])
}

#[test]
fn the_checking_tool_files_may_name_a_python_file() -> TestResult {
    for path in [
        format!("{TESTS_DIR}/rp81.rs"),
        format!("{TESTS_DIR}/rp81_removed_python.txt"),
        format!("{TESTS_DIR}/rp81_owned_lines.tsv"),
        format!("{TESTS_DIR}/rp81_a/x.md"),
    ] {
        each_added_line_passes(&path, "scripts/x.py")?;
    }
    Ok(())
}

#[test]
fn a_sibling_of_the_tool_files_is_not_exempt() -> TestResult {
    for path in [
        format!("{TESTS_DIR}/rp80.rs"),
        format!("{TESTS_DIR}/rp81_a.rs"),
        "native/hayamimi/tests/rp81_a/x.md".to_owned(),
        format!("{TESTS_DIR}/rp81a/x.md"),
    ] {
        each_added_line_is_rejected(&path, &["scripts/x.py"])?;
    }
    Ok(())
}

#[test]
fn rp81_owned_exceptions_are_gone() -> TestResult {
    accepted(check_owned_exceptions_gone(OWNED_LIST))
}

#[test]
fn a_remaining_owned_row_is_counted_per_owner() -> TestResult {
    let rows = "a.md\tone scripts/x.py\tR13-10\nb.md\ttwo scripts/x.py\tR13-14\nc.md\tthree scripts/x.py\tR13-14\n";
    rejected(
        check_owned_exceptions_gone(rows),
        &["R13-10: 1", "R13-14: 2"],
    )
}

#[test]
fn an_empty_owned_lines_file_passes() -> TestResult {
    accepted(check_owned_lines("", EMPTY_SHA256, 0))?;
    accepted(check_owned_exceptions_gone(""))
}

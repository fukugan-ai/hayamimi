//! RP-19: the cases of the Python tests are ported to Rust. The table in
//! section 5.3 of `docs/design/rust_port_tests.ja.md` gives, for each Python
//! case, the name of the Rust test that holds it, or the reason it is not
//! ported. `check_port_table` sets those names against the test names that
//! `cargo test --release --workspace -- --list` prints.
//!
//! The cases on fixtures run everywhere. The cases that need the real list
//! start a nested `cargo test --list`, which builds every test of the
//! workspace, so they are `#[ignore]`: run them with
//! `cargo test --release --test rp19 -- --ignored`. A slice that ports the
//! cases of one Python file checks only its rows by naming the file:
//!
//! ```text
//! cargo test --release --test rp19 -- --ignored rp19_file_embed_runtime
//! ```
//!
//! `rp19_python_cases_are_ported` checks the whole table and names every row
//! whose test is not in the list, with the count per Python file.

mod support;

use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

use hayamimi_eval::mdtable::{first_table, section};
use support::{Failure, REPO, TestResult, read};

/// The heading of the table in the test design.
const TABLE_HEADING: &str = "### 5.3";
/// The prefix of a reason in the column of the place, for a case that is not ported.
const NOT_PORTED: &str = "写さない：";
/// The number of cases the Python tests hold, counted when the table was made.
const PYTHON_CASES: usize = 386;

/// One row of the table.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PortRow {
    file: String,
    case: String,
    /// `None` for a case that is not ported.
    rust: Option<String>,
    place: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PortError {
    /// The row's test is not in the list.
    Missing {
        file: String,
        case: String,
        name: String,
    },
    /// Two rows name the same Rust test.
    Duplicate(String),
    /// A case that is not ported gives no reason.
    NoReason { file: String, case: String },
}

impl std::fmt::Display for PortError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PortError::Missing { file, case, name } => {
                write!(f, "{file}: {case} -> `{name}` is not in the list")
            }
            PortError::Duplicate(name) => write!(f, "`{name}` is named by two rows"),
            PortError::NoReason { file, case } => {
                write!(f, "{file}: {case} is not ported and gives no reason")
            }
        }
    }
}

fn unticked(cell: &str) -> &str {
    cell.trim_matches('`')
}

/// The rows of the table in the text of the test design.
fn parse_port_table(document: &str) -> Result<Vec<PortRow>, Failure> {
    let found = section(document, TABLE_HEADING).map_err(|e| e.to_string())?;
    let table = first_table(found).map_err(|e| e.to_string())?;
    let mut rows = Vec::new();
    for cells in &table.rows {
        let [file, case, rust, place] = cells.as_slice() else {
            return Err(format!(
                "a row of the table has {} cells, want 4",
                cells.len()
            ));
        };
        let rust = unticked(rust);
        rows.push(PortRow {
            file: unticked(file).to_owned(),
            case: unticked(case).to_owned(),
            rust: (rust != "-").then(|| rust.to_owned()),
            place: unticked(place).to_owned(),
        });
    }
    Ok(rows)
}

/// Sets the table against the test names of the list: every named row is
/// listed, no name is held by two rows, and every row that is not ported gives
/// its reason.
fn check_port_table(rows: &[PortRow], listed: &[String]) -> Result<(), Vec<PortError>> {
    let listed: HashSet<&str> = listed.iter().map(String::as_str).collect();
    let mut seen: HashSet<&str> = HashSet::new();
    let mut errors = Vec::new();
    for row in rows {
        let Some(name) = row.rust.as_deref() else {
            let reason = row.place.strip_prefix(NOT_PORTED).map(str::trim);
            if reason.is_none_or(str::is_empty) {
                errors.push(PortError::NoReason {
                    file: row.file.clone(),
                    case: row.case.clone(),
                });
            }
            continue;
        };
        if !seen.insert(name) {
            errors.push(PortError::Duplicate(name.to_owned()));
        }
        if !listed.contains(name) {
            errors.push(PortError::Missing {
                file: row.file.clone(),
                case: row.case.clone(),
                name: name.to_owned(),
            });
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// The rows that are not in the list, counted per Python file.
fn missing_per_file(errors: &[PortError]) -> BTreeMap<&str, usize> {
    let mut counts = BTreeMap::new();
    for error in errors {
        if let PortError::Missing { file, .. } = error {
            *counts.entry(file.as_str()).or_insert(0) += 1;
        }
    }
    counts
}

/// The text a failing check shows: the count, the count per file, every error.
fn report(errors: &[PortError]) -> String {
    let missing: usize = missing_per_file(errors).values().sum();
    let mut text = format!("{} errors, {missing} rows not in the list\n", errors.len());
    for (file, count) in missing_per_file(errors) {
        text.push_str(&format!("  {file}: {count}\n"));
    }
    for error in errors {
        text.push_str(&format!("{error}\n"));
    }
    text
}

const DESIGN_TESTS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/design/rust_port_tests.ja.md"
);

fn port_rows() -> Result<Vec<PortRow>, Failure> {
    parse_port_table(&read(Path::new(DESIGN_TESTS))?)
}

/// The test names in the output of `cargo test -- --list`, without their
/// module paths. A doc test (its name holds spaces) is not a name of the table.
fn parse_list(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| line.strip_suffix(": test"))
        .filter(|path| !path.contains(' '))
        .filter_map(|path| path.rsplit("::").next())
        .map(str::to_owned)
        .collect()
}

/// The names `cargo test --release --workspace -- --list` prints, listed once per process.
fn listed_names() -> Result<&'static [String], Failure> {
    static LISTED: OnceLock<Result<Vec<String>, Failure>> = OnceLock::new();
    LISTED
        .get_or_init(|| {
            let output = Command::new(env!("CARGO"))
                .args(["test", "--release", "--workspace", "--", "--list"])
                .current_dir(env!("CARGO_MANIFEST_DIR"))
                .output()
                .map_err(|e| format!("cannot run cargo: {e}"))?;
            if !output.status.success() {
                return Err(format!(
                    "cargo test --list failed:\n{}",
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
            Ok(parse_list(&String::from_utf8_lossy(&output.stdout)))
        })
        .as_deref()
        .map_err(Clone::clone)
}

/// The cases the Python tests hold: the `def test_` lines of `tests/*.py`.
fn python_case_count() -> Result<usize, Failure> {
    let dir = Path::new(REPO).join("tests");
    let mut total = 0;
    for entry in std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().is_some_and(|extension| extension == "py") {
            total += read(&path)?
                .lines()
                .filter(|line| line.starts_with("def test_") || line.starts_with("    def test_"))
                .count();
        }
    }
    Ok(total)
}

fn named(rows: &[PortRow]) -> Vec<String> {
    rows.iter().filter_map(|row| row.rust.clone()).collect()
}

fn row(file: &str, case: &str, rust: Option<&str>, place: &str) -> PortRow {
    PortRow {
        file: file.to_owned(),
        case: case.to_owned(),
        rust: rust.map(str::to_owned),
        place: place.to_owned(),
    }
}

/// T-RP-19-001: the table has one row per case of the Python tests.
#[test]
fn table_row_count_equals_python_case_count() -> TestResult {
    let rows = port_rows()?;
    assert_eq!(rows.len(), PYTHON_CASES);
    assert_eq!(python_case_count()?, PYTHON_CASES);
    Ok(())
}

/// T-RP-19-002
#[test]
fn table_names_all_listed_pass() -> TestResult {
    let rows = port_rows()?;
    assert!(rows.iter().any(|row| row.rust.is_some()));
    assert_eq!(check_port_table(&rows, &named(&rows)), Ok(()));
    Ok(())
}

/// T-RP-19-003, with its control: the same row is missing from an empty list.
#[test]
fn first_table_row_name_is_found() -> TestResult {
    let rows = port_rows()?;
    let first = &rows[..1];
    assert_eq!(check_port_table(first, &named(first)), Ok(()));
    assert!(check_port_table(first, &[]).is_err());
    Ok(())
}

/// T-RP-19-004
#[test]
fn renamed_test_in_list_is_named() -> TestResult {
    let rows = port_rows()?;
    let mut listed = named(&rows);
    let renamed = std::mem::replace(&mut listed[0], String::from("renamed_in_the_list"));
    let said = check_port_table(&rows, &listed);
    assert!(
        matches!(&said, Err(errors) if errors.len() == 1
            && matches!(&errors[0], PortError::Missing { name, .. } if *name == renamed)),
        "{said:?}"
    );
    Ok(())
}

/// T-RP-19-005
#[test]
fn duplicate_rust_name_in_table_is_rejected() {
    let rows = [
        row("a.py", "test_x", Some("x"), "p"),
        row("b.py", "test_y", Some("x"), "p"),
    ];
    let said = check_port_table(&rows, &[String::from("x")]);
    assert_eq!(said, Err(vec![PortError::Duplicate(String::from("x"))]));
}

/// T-RP-19-006
#[test]
fn unported_row_without_reason_is_rejected() {
    let rows = [row("a.py", "test_x", None, "")];
    let said = check_port_table(&rows, &[]);
    assert_eq!(
        said,
        Err(vec![PortError::NoReason {
            file: String::from("a.py"),
            case: String::from("test_x")
        }])
    );
}

/// T-RP-19-007
#[test]
fn unported_row_with_reason_is_accepted() {
    let rows = [row("a.py", "test_x", None, "写さない：この設計で無くなる")];
    assert_eq!(check_port_table(&rows, &[]), Ok(()));
}

#[test]
fn a_reason_with_nothing_after_the_colon_is_no_reason() {
    let rows = [row("a.py", "test_x", None, "写さない：")];
    assert!(check_port_table(&rows, &[]).is_err());
}

/// The table in the document has 334 named rows and 52 that are not ported.
#[test]
fn the_document_table_has_334_named_rows_and_52_reasons() -> TestResult {
    let rows = port_rows()?;
    assert_eq!(rows.iter().filter(|row| row.rust.is_some()).count(), 334);
    assert_eq!(rows.iter().filter(|row| row.rust.is_none()).count(), 52);
    assert_eq!(check_port_table(&rows, &named(&rows)), Ok(()));
    Ok(())
}

#[test]
fn the_list_is_read_without_module_paths_and_doc_tests() {
    let output = "a::tests::one: test\ntwo: test\nsrc/lib.rs - f (line 3): test\nb::bench: benchmark\n\n2 tests, 0 benchmarks\n";
    assert_eq!(parse_list(output), ["one", "two"]);
}

#[test]
fn missing_rows_are_counted_per_python_file() {
    let missing = |file: &str| PortError::Missing {
        file: file.to_owned(),
        case: String::new(),
        name: String::new(),
    };
    let errors = [
        missing("a.py"),
        missing("b.py"),
        missing("a.py"),
        PortError::Duplicate(String::new()),
    ];
    let counts = missing_per_file(&errors);
    assert_eq!(counts.get("a.py"), Some(&2));
    assert_eq!(counts.get("b.py"), Some(&1));
}

/// The Python files of the table, one slice test each, so that a slice names its file.
macro_rules! slice_tests {
    ($($name:ident => $file:literal),* $(,)?) => {
        const SLICE_FILES: &[&str] = &[$($file),*];
        $(
            #[test]
            #[ignore = "needs the real test list: a nested cargo test --list"]
            fn $name() -> TestResult {
                check_slice($file)
            }
        )*
    };
}

slice_tests! {
    rp19_file_asr_segment => "test_asr_segment.py",
    rp19_file_audio_capture => "test_audio_capture.py",
    rp19_file_diar_eval => "test_diar_eval.py",
    rp19_file_diar_osd => "test_diar_osd.py",
    rp19_file_diar_overlap => "test_diar_overlap.py",
    rp19_file_diarize => "test_diarize.py",
    rp19_file_doc_links => "test_doc_links.py",
    rp19_file_download_models => "test_download_models.py",
    rp19_file_embed_runtime => "test_embed_runtime.py",
    rp19_file_itn_cjk => "test_itn_cjk.py",
    rp19_file_ja_golden => "test_ja_golden.py",
    rp19_file_ja_pipeline_spec => "test_ja_pipeline_spec.py",
    rp19_file_lifecycle => "test_lifecycle.py",
    rp19_file_overlay_parity => "test_overlay_parity.py",
    rp19_file_process_safety => "test_process_safety.py",
    rp19_file_punct_4class => "test_punct_4class.py",
    rp19_file_second_opinion => "test_second_opinion.py",
    rp19_file_speaker_id => "test_speaker_id.py",
    rp19_file_units => "test_units.py",
    rp19_file_ws_ingest => "test_ws_ingest.py",
}

/// The rows of one Python file against the real list.
fn check_slice(file: &str) -> TestResult {
    let rows: Vec<PortRow> = port_rows()?
        .into_iter()
        .filter(|row| row.file == file)
        .collect();
    assert!(!rows.is_empty(), "the table has no row for {file}");
    if let Err(errors) = check_port_table(&rows, listed_names()?) {
        return Err(report(&errors).into());
    }
    Ok(())
}

/// Every Python file in the table has a slice test, and every slice test names a file in the table.
#[test]
fn every_python_file_of_the_table_has_a_slice_test() -> TestResult {
    let mut files: Vec<String> = port_rows()?.into_iter().map(|row| row.file).collect();
    files.sort_unstable();
    files.dedup();
    let mut sliced: Vec<&str> = SLICE_FILES.to_vec();
    sliced.sort_unstable();
    assert_eq!(files, sliced);
    Ok(())
}

/// The whole table against the real list.
fn check_whole_table() -> TestResult {
    let rows = port_rows()?;
    if let Err(errors) = check_port_table(&rows, listed_names()?) {
        return Err(report(&errors).into());
    }
    Ok(())
}

/// The acceptance test: every named row of the table is in the list.
#[test]
#[ignore = "needs the real test list: a nested cargo test --list"]
fn rp19_python_cases_are_ported() -> TestResult {
    check_whole_table()
}

/// T-RP-19-010 under its case-table name.
#[test]
#[ignore = "needs the real test list: a nested cargo test --list"]
fn python_cases_are_ported() -> TestResult {
    check_whole_table()
}

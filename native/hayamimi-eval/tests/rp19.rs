//! RP-19: the cases of the Python tests are ported to Rust.
//!
//! The table in section 5.3 of `docs/design/rust_port_tests.ja.md` gives, for
//! each Python case, the name of the Rust test that holds it, or the reason it
//! is not ported. Three kinds of function do the work: `parse_port_table`
//! reads the table, `parse_list` reads the output of `cargo test -- --list`,
//! and the `check_*` functions set one against the other.
//!
//! The rows whose test is not in the list are the cases not yet implemented.
//! They are recounted on every run. The set must equal the fixed list
//! `rp19_not_yet_at_r11.tsv` less the names that the slices record in
//! `rp19_ported/<owner>.txt` or `rp19_ported/<owner>.<slice>.txt` (the owner
//! label is what precedes the first `.` once `.txt` is dropped) and less the
//! names the table closes with a reason (the rows whose Rust test is `-`), so
//! a port that is deleted again, a port that is not recorded, a row added to
//! the fixed list, a name written in another owner's file and a name that is
//! both closed and recorded all fail
//! `rp19_ported_rows_pass_and_the_not_yet_list_only_shrinks`.
//! `rp19_python_cases_are_ported` fails while any row is missing from the
//! list and prints the count per slice.

mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use hayamimi_eval::mdtable::{first_table, section};
use support::{FIXTURES, Failure, REPO, TestResult, read, sha256_hex};

/// The heading of the table in the test design.
const TABLE_HEADING: &str = "### 5.3";
/// The prefix of a reason in the column of the place, for a case that is not ported.
const NOT_PORTED: &str = "写さない：";
/// The reason that must name a place or a mechanism in parentheses.
const PYTHON_MOUTH: &str = "Python の口";
/// The number of cases the Python tests held when the table was made.
const PYTHON_CASES: usize = 386;
/// The label of the missing rows that the fixed list does not know.
const NOT_IN_FIXED_LIST: &str = "(not in the fixed list)";

/// The sha256 of the content of `rp19_not_yet_at_r11.tsv`.
const NOT_YET_AT_R11_SHA256: &str =
    "5b9f4ddb8b7ad69917db2b4967ff5714511b6fcfeb9b83ac4cd2051d36c764a9";
/// The number of rows of `rp19_not_yet_at_r11.tsv`.
const NOT_YET_AT_R11_ROWS: usize = 172;

const DESIGN_TESTS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/design/rust_port_tests.ja.md"
);
const FIXED_LIST: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/rp19_not_yet_at_r11.tsv");
const PORTED_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/rp19_ported");

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
    /// A Python mouth names neither a place nor a mechanism.
    EmptyPythonMouth { file: String, case: String },
}

impl fmt::Display for PortError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PortError::Missing { file, case, name } => {
                write!(f, "{file}: {case} -> `{name}` is not in the list")
            }
            PortError::Duplicate(name) => write!(f, "`{name}` is named by two rows"),
            PortError::NoReason { file, case } => {
                write!(f, "{file}: {case} is not ported and gives no reason")
            }
            PortError::EmptyPythonMouth { file, case } => write!(
                f,
                "{file}: {case} is a Python mouth and names no place or mechanism"
            ),
        }
    }
}

/// One row of the fixed list: a test name and the slice that owns it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Owned {
    name: String,
    slice: String,
}

/// One file of `rp19_ported/`: the names its owner has ported.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PortedFile {
    /// The file name, `<owner>.txt` or `<owner>.<slice>.txt`.
    file: String,
    /// The owner label: what precedes the first `.` of the name without `.txt`.
    slice: String,
    names: Vec<String>,
}

impl PortedFile {
    fn from_text(file: &str, text: &str) -> Result<Self, Failure> {
        let Some(stem) = file.strip_suffix(".txt") else {
            return Err(format!("{file} is not an <owner>[.<slice>].txt file"));
        };
        let slice = stem.split_once('.').map_or(stem, |(owner, _)| owner);
        let names = text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect();
        Ok(Self {
            file: file.to_owned(),
            slice: slice.to_owned(),
            names,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum NotYetError {
    /// A row is missing from the list, and no file records it as ported and
    /// the fixed list does not hold it.
    NotInFixedList(String),
    /// A ported file names a test that is not in the list.
    PortedNotListed(String),
    /// A test is in the list, and no ported file records it.
    ListedNotPorted(String),
    /// A ported file names a test that the fixed list does not hold.
    UnknownName { file: String, name: String },
    /// A ported file names a test that another slice owns.
    OtherSlice {
        file: String,
        name: String,
        owner: String,
    },
    /// A ported file names a test that the table closes with a reason.
    ClosedAlsoPorted { file: String, name: String },
}

impl fmt::Display for NotYetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NotYetError::NotInFixedList(name) => write!(
                f,
                "`{name}` is a row of the table missing from the list, and the fixed list does not hold it"
            ),
            NotYetError::PortedNotListed(name) => write!(
                f,
                "`{name}` is recorded in rp19_ported/ but is not in the list (deleted or renamed after the port)"
            ),
            NotYetError::ListedNotPorted(name) => write!(
                f,
                "`{name}` is in the fixed list, in no rp19_ported/ file and not closed by the table, but it is not a missing row (write it in its slice's file)"
            ),
            NotYetError::UnknownName { file, name } => {
                write!(f, "{file}: `{name}` is not a name of the fixed list")
            }
            NotYetError::OtherSlice { file, name, owner } => {
                write!(f, "{file}: `{name}` is a row of slice {owner}")
            }
            NotYetError::ClosedAlsoPorted { file, name } => write!(
                f,
                "{file}: `{name}` is closed by the table with a reason and is also recorded as ported"
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FixedListError {
    Rows { found: usize, pinned: usize },
    Digest { found: String, pinned: String },
}

impl fmt::Display for FixedListError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FixedListError::Rows { found, pinned } => {
                write!(f, "the fixed list has {found} rows, pinned {pinned}")
            }
            FixedListError::Digest { found, pinned } => {
                write!(f, "the fixed list has sha256 {found}, pinned {pinned}")
            }
        }
    }
}

/// A failure whose text is printed as it stands, line breaks included.
struct Report(String);

impl fmt::Debug for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Report {}

// ---------------------------------------------------------------------------
// Reading the table
// ---------------------------------------------------------------------------

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

fn port_rows() -> Result<Vec<PortRow>, Failure> {
    parse_port_table(&read(Path::new(DESIGN_TESTS))?)
}

/// The Rust test names of the rows that give one.
fn named(rows: &[PortRow]) -> Vec<String> {
    rows.iter().filter_map(|row| row.rust.clone()).collect()
}

/// A case name without its leading `test_`.
fn without_test_prefix(case: &str) -> &str {
    match case.strip_prefix("test_") {
        Some(rest) => rest,
        None => case,
    }
}

/// The names the table closes: the case of each row that gives no Rust test,
/// without its leading `test_`.
fn closed_names(rows: &[PortRow]) -> Vec<String> {
    rows.iter()
        .filter(|row| row.rust.is_none())
        .map(|row| without_test_prefix(&row.case).to_owned())
        .collect()
}

// ---------------------------------------------------------------------------
// Reading the list, the fixed list and the ported files
// ---------------------------------------------------------------------------

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

/// The variables cargo sets for a test process. A cargo that inherits them
/// reads a changed environment: it reruns the build scripts of the workspace
/// (`ring` is rebuilt), and the cargo that ran this test reruns them again
/// on its next build.
const SET_BY_CARGO: [&str; 22] = [
    "CARGO_BIN_EXE_hayamimi-eval",
    "CARGO_BIN_NAME",
    "CARGO_CRATE_NAME",
    "CARGO_MANIFEST_DIR",
    "CARGO_MANIFEST_PATH",
    "CARGO_PKG_AUTHORS",
    "CARGO_PKG_DESCRIPTION",
    "CARGO_PKG_HOMEPAGE",
    "CARGO_PKG_LICENSE",
    "CARGO_PKG_LICENSE_FILE",
    "CARGO_PKG_NAME",
    "CARGO_PKG_README",
    "CARGO_PKG_REPOSITORY",
    "CARGO_PKG_RUST_VERSION",
    "CARGO_PKG_VERSION",
    "CARGO_PKG_VERSION_MAJOR",
    "CARGO_PKG_VERSION_MINOR",
    "CARGO_PKG_VERSION_PATCH",
    "CARGO_PKG_VERSION_PRE",
    "CARGO_PRIMARY_PACKAGE",
    "CARGO_TARGET_TMPDIR",
    "OUT_DIR",
];

/// The target directory of the nested cargo: `rp19-list` under the target
/// directory that holds this test binary (`<target>/<profile>/deps/<binary>`).
/// The nested build never writes the libraries the outer target shares with
/// the tests that run beside this one.
fn list_target_dir() -> Result<PathBuf, Failure> {
    let binary =
        std::env::current_exe().map_err(|e| format!("cannot find the test binary: {e}"))?;
    let Some(target) = binary.ancestors().nth(3) else {
        return Err(format!(
            "{} is not under <target>/<profile>/deps",
            binary.display()
        ));
    };
    Ok(target.join("rp19-list"))
}

/// `cargo test --release --workspace -- --list`, run in its own target
/// directory and in an environment that holds none of the variables cargo set
/// for this test.
fn list_command() -> Result<Command, Failure> {
    let mut command = Command::new(env!("CARGO"));
    command
        .args([
            "test",
            "--release",
            "--workspace",
            "--lib",
            "--bins",
            "--tests",
        ])
        .args(["--", "--list"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("CARGO_TARGET_DIR", list_target_dir()?);
    for name in SET_BY_CARGO {
        command.env_remove(name);
    }
    Ok(command)
}

/// The names `cargo test --release --workspace -- --list` prints, listed once per process.
fn listed_names() -> Result<&'static [String], Failure> {
    static LISTED: OnceLock<Result<Vec<String>, Failure>> = OnceLock::new();
    LISTED
        .get_or_init(|| {
            let output = list_command()?
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

/// The rows of the fixed list: `name<TAB>slice`, one per line.
fn parse_fixed_list(text: &str) -> Result<Vec<Owned>, Failure> {
    let mut owned = Vec::new();
    let mut seen = BTreeSet::new();
    for (index, line) in text.lines().enumerate() {
        let number = index + 1;
        let Some((name, slice)) = line.split_once('\t') else {
            return Err(format!("fixed list line {number}: want `name<TAB>slice`"));
        };
        if name.is_empty() || slice.is_empty() || slice.contains('\t') {
            return Err(format!("fixed list line {number}: want `name<TAB>slice`"));
        }
        if !seen.insert(name) {
            return Err(format!(
                "fixed list line {number}: `{name}` is listed twice"
            ));
        }
        owned.push(Owned {
            name: name.to_owned(),
            slice: slice.to_owned(),
        });
    }
    Ok(owned)
}

/// The paths in `dir`, sorted. A directory that does not exist has none: the
/// first slice that ports a name creates `rp19_ported/`.
fn paths_in(dir: &Path) -> Result<Vec<PathBuf>, Failure> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("{}: {error}", dir.display())),
    };
    let mut paths = Vec::new();
    for entry in entries {
        match entry {
            Ok(found) => paths.push(found.path()),
            Err(error) => return Err(format!("{}: {error}", dir.display())),
        }
    }
    paths.sort();
    Ok(paths)
}

/// The files of `rp19_ported/`. A file that does not end in `.txt` is refused.
fn read_ported(dir: &Path) -> Result<Vec<PortedFile>, Failure> {
    let mut files = Vec::new();
    for path in paths_in(dir)? {
        let Some(file) = path.file_name().and_then(|name| name.to_str()) else {
            return Err(format!("{} has no UTF-8 name", path.display()));
        };
        files.push(PortedFile::from_text(file, &read(&path)?)?);
    }
    Ok(files)
}

/// The cases the Python tests hold: the `def test_` lines of `tests/*.py`.
/// `None` once the Python tests are removed.
fn python_case_count() -> Result<Option<usize>, Failure> {
    let mut files = 0;
    let mut total = 0;
    for path in paths_in(&Path::new(REPO).join("tests"))? {
        if path.extension().is_some_and(|extension| extension == "py") {
            files += 1;
            total += read(&path)?
                .lines()
                .filter(|line| line.starts_with("def test_") || line.starts_with("    def test_"))
                .count();
        }
    }
    Ok((files > 0).then_some(total))
}

// ---------------------------------------------------------------------------
// Setting one against the other
// ---------------------------------------------------------------------------

/// Whether a reason of the Python mouth kind names a place or a mechanism
/// between its parentheses.
fn python_mouth_names_a_place(reason: &str) -> bool {
    reason
        .strip_prefix(PYTHON_MOUTH)
        .and_then(|rest| rest.strip_prefix('（'))
        .and_then(|rest| rest.strip_suffix('）'))
        .is_some_and(|inner| !inner.trim().is_empty())
}

/// What is wrong with a row that is not ported, if anything.
fn unported_error(row: &PortRow) -> Option<PortError> {
    let reason = row
        .place
        .strip_prefix(NOT_PORTED)
        .map(str::trim)
        .filter(|reason| !reason.is_empty());
    let (file, case) = (row.file.clone(), row.case.clone());
    match reason {
        None => Some(PortError::NoReason { file, case }),
        Some(reason) if reason.starts_with(PYTHON_MOUTH) && !python_mouth_names_a_place(reason) => {
            Some(PortError::EmptyPythonMouth { file, case })
        }
        Some(_) => None,
    }
}

/// Sets the table against the test names of the list: every named row is
/// listed, no name is held by two rows, and every row that is not ported gives
/// its reason.
fn check_port_table(rows: &[PortRow], listed: &[String]) -> Result<(), Vec<PortError>> {
    let listed: BTreeSet<&str> = listed.iter().map(String::as_str).collect();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut errors = Vec::new();
    for row in rows {
        let Some(name) = row.rust.as_deref() else {
            errors.extend(unported_error(row));
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

/// The owner slice of each name of the fixed list.
fn owner_map(fixed: &[Owned]) -> BTreeMap<&str, &str> {
    fixed
        .iter()
        .map(|owned| (owned.name.as_str(), owned.slice.as_str()))
        .collect()
}

/// The set errors: the rows missing from the list, against the fixed list
/// less the names the ported files record and the names the table closes.
fn set_errors(
    table_names: &[String],
    listed: &[String],
    fixed: &[Owned],
    ported: &[PortedFile],
    closed: &[String],
) -> Vec<NotYetError> {
    let listed: BTreeSet<&str> = listed.iter().map(String::as_str).collect();
    let closed: BTreeSet<&str> = closed.iter().map(String::as_str).collect();
    let claimed: BTreeSet<&str> = ported
        .iter()
        .flat_map(|file| file.names.iter().map(String::as_str))
        .collect();
    let actual: BTreeSet<&str> = table_names
        .iter()
        .map(String::as_str)
        .filter(|name| !listed.contains(name))
        .collect();
    let expected: BTreeSet<&str> = fixed
        .iter()
        .map(|owned| owned.name.as_str())
        .filter(|name| !claimed.contains(name) && !closed.contains(name))
        .collect();
    let mut errors = Vec::new();
    for name in actual.difference(&expected) {
        let name = (*name).to_owned();
        errors.push(if claimed.contains(name.as_str()) {
            NotYetError::PortedNotListed(name)
        } else {
            NotYetError::NotInFixedList(name)
        });
    }
    errors.extend(
        expected
            .difference(&actual)
            .map(|name| NotYetError::ListedNotPorted((*name).to_owned())),
    );
    errors
}

/// The ownership errors: each ported name is a row of the file's own slice
/// and is not a name the table closes.
fn ownership_errors(fixed: &[Owned], ported: &[PortedFile], closed: &[String]) -> Vec<NotYetError> {
    let owners = owner_map(fixed);
    let closed: BTreeSet<&str> = closed.iter().map(String::as_str).collect();
    let mut errors = Vec::new();
    for file in ported {
        for name in &file.names {
            if closed.contains(name.as_str()) {
                errors.push(NotYetError::ClosedAlsoPorted {
                    file: file.file.clone(),
                    name: name.clone(),
                });
            }
            match owners.get(name.as_str()) {
                None => errors.push(NotYetError::UnknownName {
                    file: file.file.clone(),
                    name: name.clone(),
                }),
                Some(owner) if *owner != file.slice => errors.push(NotYetError::OtherSlice {
                    file: file.file.clone(),
                    name: name.clone(),
                    owner: (*owner).to_owned(),
                }),
                Some(_) => {}
            }
        }
    }
    errors
}

/// The rows of the table missing from the list (the cases not yet
/// implemented) are exactly the fixed list less the names the ported files
/// record and the names the table closes, each recorded name is a row of its
/// own slice, and no recorded name is a closed one.
fn check_not_yet_set(
    table_names: &[String],
    listed: &[String],
    fixed: &[Owned],
    ported: &[PortedFile],
    closed: &[String],
) -> Result<(), Vec<NotYetError>> {
    let mut errors = set_errors(table_names, listed, fixed, ported, closed);
    errors.extend(ownership_errors(fixed, ported, closed));
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// The content of the fixed list has the pinned number of rows and the pinned sha256.
fn check_fixed_list(
    text: &str,
    pinned_sha256: &str,
    pinned_rows: usize,
) -> Result<(), Vec<FixedListError>> {
    let mut errors = Vec::new();
    let rows = text.lines().count();
    if rows != pinned_rows {
        errors.push(FixedListError::Rows {
            found: rows,
            pinned: pinned_rows,
        });
    }
    let digest = sha256_hex(text.as_bytes());
    if digest != pinned_sha256 {
        errors.push(FixedListError::Digest {
            found: digest,
            pinned: pinned_sha256.to_owned(),
        });
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

// ---------------------------------------------------------------------------
// What a failing check shows
// ---------------------------------------------------------------------------

/// The missing rows counted per slice. A row the fixed list does not hold
/// is counted under [`NOT_IN_FIXED_LIST`].
fn missing_per_slice<'a>(errors: &[PortError], fixed: &'a [Owned]) -> BTreeMap<&'a str, usize> {
    let owners = owner_map(fixed);
    let mut counts = BTreeMap::new();
    for error in errors {
        let PortError::Missing { name, .. } = error else {
            continue;
        };
        let slice = match owners.get(name.as_str()) {
            Some(slice) => *slice,
            None => NOT_IN_FIXED_LIST,
        };
        *counts.entry(slice).or_insert(0) += 1;
    }
    counts
}

/// The text a failing `rp19_python_cases_are_ported` shows: the count, the
/// count per slice, every error.
fn report_not_yet(errors: &[PortError], fixed: &[Owned]) -> String {
    let per_slice = missing_per_slice(errors, fixed);
    let total: usize = per_slice.values().sum();
    let mut text = format!("{total} rows of the table are not in the list yet\n");
    for (slice, count) in &per_slice {
        text.push_str(&format!("  {slice}: {count}\n"));
    }
    for error in errors {
        text.push_str(&format!("{error}\n"));
    }
    text
}

/// The lines of a failed check, none for a check that passed.
fn lines_of<E: fmt::Display>(checked: Result<(), Vec<E>>) -> Vec<String> {
    match checked {
        Ok(()) => Vec::new(),
        Err(errors) => errors.iter().map(ToString::to_string).collect(),
    }
}

/// A passing test for no problems, a failure that prints them otherwise.
fn finish(problems: Vec<String>) -> TestResult {
    if problems.is_empty() {
        return Ok(());
    }
    Err(Report(problems.join("\n")).into())
}

// ---------------------------------------------------------------------------
// The acceptance tests
// ---------------------------------------------------------------------------

/// T-RP-19-010: the whole table against the real list. It fails while any
/// row is missing and prints the count per slice.
#[test]
fn rp19_python_cases_are_ported() -> TestResult {
    let fixed = parse_fixed_list(&read(Path::new(FIXED_LIST))?)?;
    match check_port_table(&port_rows()?, listed_names()?) {
        Ok(()) => Ok(()),
        Err(errors) => Err(Report(report_not_yet(&errors, &fixed)).into()),
    }
}

/// T-RP-19-025: the three checks of the fixed list on the real files.
#[test]
fn rp19_ported_rows_pass_and_the_not_yet_list_only_shrinks() -> TestResult {
    let fixed_text = read(Path::new(FIXED_LIST))?;
    let mut problems = lines_of(check_fixed_list(
        &fixed_text,
        NOT_YET_AT_R11_SHA256,
        NOT_YET_AT_R11_ROWS,
    ));
    let fixed = parse_fixed_list(&fixed_text)?;
    let ported = read_ported(Path::new(PORTED_DIR))?;
    let rows = port_rows()?;
    problems.extend(lines_of(check_not_yet_set(
        &named(&rows),
        listed_names()?,
        &fixed,
        &ported,
        &closed_names(&rows),
    )));
    finish(problems)
}

// ---------------------------------------------------------------------------
// The checks on the table
// ---------------------------------------------------------------------------

fn row(file: &str, case: &str, rust: Option<&str>, place: &str) -> PortRow {
    PortRow {
        file: file.to_owned(),
        case: case.to_owned(),
        rust: rust.map(str::to_owned),
        place: place.to_owned(),
    }
}

/// T-RP-19-001: the table has one row per case of the Python tests. While the
/// Python tests stand their cases are counted too.
#[test]
fn table_row_count_equals_python_case_count() -> TestResult {
    assert_eq!(port_rows()?.len(), PYTHON_CASES);
    if let Some(found) = python_case_count()? {
        assert_eq!(found, PYTHON_CASES);
    }
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
    assert!(!rows.is_empty());
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

fn mouth_error(place: &str) -> Result<(), Vec<PortError>> {
    check_port_table(&[row("a.py", "test_x", None, place)], &[])
}

/// T-RP-19-011: the heading alone names no place and no mechanism.
#[test]
fn python_mouth_row_without_place_or_mechanism_is_rejected() {
    let said = mouth_error("写さない：Python の口");
    assert_eq!(
        said,
        Err(vec![PortError::EmptyPythonMouth {
            file: String::from("a.py"),
            case: String::from("test_x")
        }])
    );
}

/// T-RP-19-012: the parentheses are there and empty.
#[test]
fn python_mouth_row_with_empty_parentheses_is_rejected() {
    let said = mouth_error("写さない：Python の口（）");
    assert!(
        matches!(&said, Err(errors) if matches!(errors.as_slice(), [PortError::EmptyPythonMouth { .. }])),
        "{said:?}"
    );
}

/// T-RP-19-013: the mechanism of the real row of `run_stream_without_control_is_unaffected`.
#[test]
fn python_mouth_row_with_a_mechanism_is_accepted() -> TestResult {
    let rows = port_rows()?;
    let case = "test_run_stream_without_control_is_unaffected";
    let Some(found) = rows.iter().find(|row| row.case == case) else {
        return Err(Report(format!("the table has no row of {case}")).into());
    };
    assert_eq!(mouth_error(&found.place), Ok(()));
    Ok(())
}

fn is_python_mouth(row: &PortRow) -> bool {
    row.rust.is_none()
        && row
            .place
            .strip_prefix(NOT_PORTED)
            .map(str::trim)
            .is_some_and(python_mouth_names_a_place)
}

/// T-RP-19-023
#[test]
fn the_table_has_exactly_twenty_python_mouth_rows() -> TestResult {
    let rows = port_rows()?;
    assert_eq!(rows.iter().filter(|row| is_python_mouth(row)).count(), 20);
    Ok(())
}

fn rows_placed_at(rows: &[PortRow], file: &str, place: &str) -> usize {
    rows.iter()
        .filter(|row| row.file == file && row.place == place)
        .count()
}

/// T-RP-19-024
#[test]
fn the_ingest_codec_rows_are_placed_in_eval_ws_rs() -> TestResult {
    let rows = port_rows()?;
    let owned = ["test_ws_ingest", ".py"].concat();
    let file = owned.as_str();
    assert_eq!(rows_placed_at(&rows, file, "hayamimi-eval/src/ws.rs"), 8);
    assert_eq!(rows_placed_at(&rows, file, "hayamimi-asr/src/audio.rs"), 4);
    assert_eq!(
        rows_placed_at(&rows, file, "hayamimi/src/capture/convert.rs"),
        4
    );
    assert_eq!(rows_placed_at(&rows, file, "hayamimi/src/ingest.rs"), 0);
    let mouths = rows
        .iter()
        .filter(|row| row.file == file && is_python_mouth(row))
        .count();
    assert_eq!(mouths, 5);
    assert_eq!(rows.iter().filter(|row| row.file == file).count(), 21);
    Ok(())
}

/// The table holds 314 named rows and 72 that are not ported (the 52 of the
/// four ordinary reasons and the 20 Python mouths).
#[test]
fn the_document_table_has_314_named_rows_and_72_reasons() -> TestResult {
    let rows = port_rows()?;
    assert_eq!(rows.iter().filter(|row| row.rust.is_some()).count(), 314);
    assert_eq!(rows.iter().filter(|row| row.rust.is_none()).count(), 72);
    assert_eq!(check_port_table(&rows, &named(&rows)), Ok(()));
    Ok(())
}

#[test]
fn the_list_is_read_without_module_paths_and_doc_tests() {
    let output = "a::tests::one: test\ntwo: test\nsrc/lib.rs - f (line 3): test\nb::bench: benchmark\n\n2 tests, 0 benchmarks\n";
    assert_eq!(parse_list(output), ["one", "two"]);
}

// ---------------------------------------------------------------------------
// The checks on the fixed list, on fixtures
// ---------------------------------------------------------------------------

const BASELINE_FIXED: &str = include_str!("fixtures/rp19_r11/fixed.tsv");
const FIXED_WITH_DELTA: &str = include_str!("fixtures/rp19_r11/fixed_with_delta.tsv");
const FIXED_CHANGED_SLICE: &str = include_str!("fixtures/rp19_r11/fixed_changed_slice.tsv");
const LIST_ALPHA: &str = include_str!("fixtures/rp19_r11/list_alpha.txt");
const LIST_EMPTY: &str = include_str!("fixtures/rp19_r11/list_empty.txt");
const LIST_ALL: &str = include_str!("fixtures/rp19_r11/list_all.txt");
const PORTED_ALPHA: &str = include_str!("fixtures/rp19_r11/ported_alpha.txt");
const PORTED_BLANK: &str = include_str!("fixtures/rp19_r11/ported_blank.txt");
const PORTED_BETA: &str = include_str!("fixtures/rp19_r11/ported_beta.txt");
const PORTED_BETA_GAMMA: &str = include_str!("fixtures/rp19_r11/ported_beta_gamma.txt");
const PORTED_ALPHA_ZETA: &str = include_str!("fixtures/rp19_r11/ported_alpha_zeta.txt");

/// The sha256 and the rows of `fixed.tsv`, the baseline fixed list.
const BASELINE_SHA256: &str = "22f60128b350e30585e8a3d7e3e495c9c4d80657f811bc188e6e8dc95c994010";
const BASELINE_ROWS: usize = 3;
/// The three rows of the table in the baseline state.
const TABLE_NAMES: [&str; 3] = ["alpha", "beta", "gamma"];

fn owned_names(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

/// The not-yet check on a state: the fixed list, the text of the list, the
/// ported files as `(file name, text)`, the names of the table's rows that
/// give a Rust test, and the names the table closes.
fn judge(
    fixed_text: &str,
    list_text: &str,
    ported: &[(&str, &str)],
    table: &[&str],
    closed: &[&str],
) -> Result<Result<(), Vec<NotYetError>>, Failure> {
    let fixed = parse_fixed_list(fixed_text)?;
    let mut files = Vec::new();
    for (file, text) in ported {
        files.push(PortedFile::from_text(file, text)?);
    }
    Ok(check_not_yet_set(
        &owned_names(table),
        &parse_list(list_text),
        &fixed,
        &files,
        &owned_names(closed),
    ))
}

/// T-RP-19-014: `alpha` is recorded as ported and is gone from the list.
#[test]
fn a_name_ported_and_then_deleted_from_the_list_is_rejected() -> TestResult {
    let ported = [("R12b.txt", PORTED_ALPHA)];
    let said = judge(BASELINE_FIXED, LIST_EMPTY, &ported, &TABLE_NAMES, &[])?;
    assert_eq!(
        said,
        Err(vec![NotYetError::PortedNotListed(String::from("alpha"))])
    );
    Ok(())
}

/// T-RP-19-015: `alpha` is in the list and no ported file records it.
#[test]
fn a_ported_name_missing_from_the_ported_files_is_rejected() -> TestResult {
    let ported = [("R12b.txt", PORTED_BLANK)];
    let said = judge(BASELINE_FIXED, LIST_ALPHA, &ported, &TABLE_NAMES, &[])?;
    assert_eq!(
        said,
        Err(vec![NotYetError::ListedNotPorted(String::from("alpha"))])
    );
    Ok(())
}

/// T-RP-19-016: a row is added to the fixed list, the pinned values stay.
#[test]
fn a_row_added_to_the_fixed_list_is_rejected() {
    let said = check_fixed_list(FIXED_WITH_DELTA, BASELINE_SHA256, BASELINE_ROWS);
    assert!(
        matches!(&said, Err(errors) if matches!(
            errors.as_slice(),
            [FixedListError::Rows { found: 4, pinned: 3 }, FixedListError::Digest { .. }]
        )),
        "{said:?}"
    );
}

/// T-RP-19-017: `alpha` is written in the file of owner `R12e`, which does not own it.
#[test]
fn a_name_written_in_another_slices_file_is_rejected() -> TestResult {
    let ported = [("R12b.txt", PORTED_BLANK), ("R12e.txt", PORTED_ALPHA)];
    let said = judge(BASELINE_FIXED, LIST_ALPHA, &ported, &TABLE_NAMES, &[])?;
    assert_eq!(
        said,
        Err(vec![NotYetError::OtherSlice {
            file: String::from("R12e.txt"),
            name: String::from("alpha"),
            owner: String::from("R12b")
        }])
    );
    Ok(())
}

/// T-RP-19-018: a name the fixed list does not hold, in a ported file.
#[test]
fn a_name_unknown_to_the_fixed_list_is_rejected_in_a_ported_file() -> TestResult {
    let ported = [("R12b.txt", PORTED_ALPHA_ZETA)];
    let said = judge(BASELINE_FIXED, LIST_ALPHA, &ported, &TABLE_NAMES, &[])?;
    assert_eq!(
        said,
        Err(vec![NotYetError::UnknownName {
            file: String::from("R12b.txt"),
            name: String::from("zeta")
        }])
    );
    Ok(())
}

/// T-RP-19-019: a changed row with the same row count changes the digest only.
#[test]
fn a_changed_row_with_the_same_count_changes_the_digest() {
    let said = check_fixed_list(FIXED_CHANGED_SLICE, BASELINE_SHA256, BASELINE_ROWS);
    assert!(
        matches!(&said, Err(errors) if matches!(errors.as_slice(), [FixedListError::Digest { .. }])),
        "{said:?}"
    );
}

/// T-RP-19-020: the baseline state passes both checks.
#[test]
fn the_baseline_state_passes() -> TestResult {
    let ported = [("R12b.txt", PORTED_ALPHA)];
    let said = judge(BASELINE_FIXED, LIST_ALPHA, &ported, &TABLE_NAMES, &[])?;
    assert_eq!(said, Ok(()));
    let fixed = check_fixed_list(BASELINE_FIXED, BASELINE_SHA256, BASELINE_ROWS);
    assert_eq!(fixed, Ok(()));
    Ok(())
}

/// T-RP-19-021: no ported directory reads as no ported name.
#[test]
fn an_absent_ported_directory_reads_as_empty() -> TestResult {
    let absent = Path::new(FIXTURES)
        .join("rp19_r11")
        .join("no_ported_dir_here");
    assert_eq!(read_ported(&absent)?, Vec::<PortedFile>::new());
    let said = judge(BASELINE_FIXED, LIST_EMPTY, &[], &TABLE_NAMES, &[])?;
    assert_eq!(said, Ok(()));
    Ok(())
}

/// T-RP-19-022: every name is in the list and recorded by its slice.
#[test]
fn a_fully_ported_state_passes() -> TestResult {
    let ported = [("R12b.txt", PORTED_ALPHA), ("R12e.txt", PORTED_BETA_GAMMA)];
    let said = judge(BASELINE_FIXED, LIST_ALL, &ported, &TABLE_NAMES, &[])?;
    assert_eq!(said, Ok(()));
    Ok(())
}

/// T-RP-19-026: `R12b.R13-1.txt` reads the owner `R12b` before the first `.`.
#[test]
fn a_file_named_owner_dot_slice_reads_the_owner_before_the_first_dot() -> TestResult {
    let ported = [("R12b.R13-1.txt", PORTED_ALPHA)];
    let said = judge(BASELINE_FIXED, LIST_ALPHA, &ported, &TABLE_NAMES, &[])?;
    assert_eq!(said, Ok(()));
    Ok(())
}

/// T-RP-19-027: `R12e.R13-1.txt` has the owner `R12e`, which does not own `alpha`.
#[test]
fn a_name_in_a_file_of_another_owner_with_a_slice_suffix_is_rejected() -> TestResult {
    let ported = [("R12e.R13-1.txt", PORTED_ALPHA)];
    let said = judge(BASELINE_FIXED, LIST_ALPHA, &ported, &TABLE_NAMES, &[])?;
    assert_eq!(
        said,
        Err(vec![NotYetError::OtherSlice {
            file: String::from("R12e.R13-1.txt"),
            name: String::from("alpha"),
            owner: String::from("R12b")
        }])
    );
    Ok(())
}

/// The state of T-RP-19-028 to 030: the row of `beta` is closed by the table,
/// so the table names are `alpha` and `gamma`.
const TABLE_WITHOUT_BETA: [&str; 2] = ["alpha", "gamma"];

/// T-RP-19-028: `beta` is closed by the table and is left out of the expected set.
#[test]
fn a_name_closed_in_the_table_is_left_out_of_the_expected_not_yet_set() -> TestResult {
    let ported = [("R12b.txt", PORTED_ALPHA)];
    let said = judge(
        BASELINE_FIXED,
        LIST_ALPHA,
        &ported,
        &TABLE_WITHOUT_BETA,
        &["beta"],
    )?;
    assert_eq!(said, Ok(()));
    Ok(())
}

/// T-RP-19-029: the same state with no closed name passed; `beta` stays in the expected set.
#[test]
fn a_closed_name_that_is_not_subtracted_is_named_as_listed_not_ported() -> TestResult {
    let ported = [("R12b.txt", PORTED_ALPHA)];
    let said = judge(
        BASELINE_FIXED,
        LIST_ALPHA,
        &ported,
        &TABLE_WITHOUT_BETA,
        &[],
    )?;
    assert_eq!(
        said,
        Err(vec![NotYetError::ListedNotPorted(String::from("beta"))])
    );
    Ok(())
}

/// T-RP-19-030: `beta` is closed by the table and also recorded in the file of its owner.
#[test]
fn a_name_both_closed_in_the_table_and_ported_is_rejected() -> TestResult {
    let ported = [("R12b.txt", PORTED_ALPHA), ("R12e.txt", PORTED_BETA)];
    let said = judge(
        BASELINE_FIXED,
        LIST_ALPHA,
        &ported,
        &TABLE_WITHOUT_BETA,
        &["beta"],
    )?;
    assert_eq!(
        said,
        Err(vec![NotYetError::ClosedAlsoPorted {
            file: String::from("R12e.txt"),
            name: String::from("beta")
        }])
    );
    Ok(())
}

#[test]
fn a_file_that_is_not_a_slice_file_is_refused() {
    let said = PortedFile::from_text("R12b.md", "alpha\n");
    assert!(said.is_err());
}

#[test]
fn a_malformed_or_repeated_row_of_the_fixed_list_is_refused() {
    let texts = [
        "alpha\n",
        "alpha\t\n",
        "\tR12b\n",
        "a\tb\tc\n",
        "a\tR1\na\tR2\n",
        "a\tR1\n\n",
    ];
    for text in texts {
        assert!(parse_fixed_list(text).is_err(), "{text:?}");
    }
}

#[test]
fn missing_rows_are_counted_per_slice() {
    let missing = |name: &str| PortError::Missing {
        file: String::new(),
        case: String::new(),
        name: name.to_owned(),
    };
    let owned = |name: &str, slice: &str| Owned {
        name: name.to_owned(),
        slice: slice.to_owned(),
    };
    let fixed = [owned("a", "R1"), owned("b", "R2"), owned("c", "R1")];
    let errors = [
        missing("a"),
        missing("b"),
        missing("c"),
        missing("z"),
        PortError::Duplicate(String::new()),
    ];
    let counts = missing_per_slice(&errors, &fixed);
    assert_eq!(counts.get("R1"), Some(&2));
    assert_eq!(counts.get("R2"), Some(&1));
    assert_eq!(counts.get(NOT_IN_FIXED_LIST), Some(&1));
}

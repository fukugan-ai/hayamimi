//! RP-30: the `stage` and `reason` values of a `fallback` event that the design
//! lists (the paragraph "`fallback` の event の `stage` の値は" of 3.15) are the
//! values `STAGES` and `REASONS` of `src/notice.rs` hold, as sets. The test
//! reads the design and the source as text; it needs no binary and no GPU.
//! The counterexamples are files in `tests/rp30/`.

use std::collections::BTreeSet;
use std::fmt;
use std::fs;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const DESIGN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/design/rust_port.ja.md"
);
const NOTICE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/notice.rs");
const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/rp30");

/// The start of the line that lists the stages.
const STAGE_LEAD: &str = "`fallback` の event の `stage` の値は";
/// The start of the line that lists the reasons.
const REASON_LEAD: &str = "`reason` の値は";
/// The words before the values `hayamimi` itself sends.
const OWN: &str = "`hayamimi` が";

#[derive(Debug, PartialEq, Eq)]
enum ListError {
    /// The design has no list of that kind (or the list is empty).
    NoList(&'static str),
    /// The source has no constant of that name.
    NoConstant(&'static str),
    /// The design lists a value the code does not hold.
    OnlyInDesign { list: &'static str, value: String },
    /// The code holds a value the design does not list.
    OnlyInCode { list: &'static str, value: String },
}

impl fmt::Display for ListError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ListError::NoList(list) => write!(f, "the design has no list of {list} values"),
            ListError::NoConstant(name) => write!(f, "notice.rs has no constant {name}"),
            ListError::OnlyInDesign { list, value } => {
                write!(f, "`{value}` is a {list} in the design only")
            }
            ListError::OnlyInCode { list, value } => {
                write!(f, "`{value}` is a {list} in the code only")
            }
        }
    }
}

impl std::error::Error for ListError {}

/// The odd-numbered pieces of `text` split at `mark`: the parts between a pair of marks.
fn between(text: &str, mark: char) -> BTreeSet<String> {
    text.split(mark)
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

/// The values `hayamimi` sends, from the line of the design that starts with `lead`.
/// The list ends at the count ("13 個"), so the values named after it for the relay are left out.
fn design_values(
    design: &str,
    lead: &str,
    list: &'static str,
) -> Result<BTreeSet<String>, ListError> {
    let line = design
        .lines()
        .find(|line| line.starts_with(lead))
        .ok_or(ListError::NoList(list))?;
    let (_, after) = line.split_once(OWN).ok_or(ListError::NoList(list))?;
    let (listed, _) = after.split_once('個').ok_or(ListError::NoList(list))?;
    let values = between(listed, '`');
    if values.is_empty() {
        return Err(ListError::NoList(list));
    }
    Ok(values)
}

/// The string literals of `pub const <name>: [&str; N] = [ ... ];` in the source.
fn code_values(code: &str, name: &'static str) -> Result<BTreeSet<String>, ListError> {
    let head = format!("pub const {name}: ");
    let start = code.find(&head).ok_or(ListError::NoConstant(name))?;
    let rest = &code[start..];
    let open = rest.find("= [").ok_or(ListError::NoConstant(name))?;
    let body = &rest[open..];
    let close = body.find("];").ok_or(ListError::NoConstant(name))?;
    Ok(between(&body[..close], '"'))
}

fn same_set(
    list: &'static str,
    design: &BTreeSet<String>,
    code: &BTreeSet<String>,
) -> Result<(), ListError> {
    if let Some(value) = code.difference(design).next() {
        return Err(ListError::OnlyInCode {
            list,
            value: value.clone(),
        });
    }
    if let Some(value) = design.difference(code).next() {
        return Err(ListError::OnlyInDesign {
            list,
            value: value.clone(),
        });
    }
    Ok(())
}

/// The two lists of the design are the two constants of the code, as sets.
fn check_fallback_lists(design: &str, code: &str) -> Result<(), ListError> {
    let stages = design_values(design, STAGE_LEAD, "stage")?;
    let reasons = design_values(design, REASON_LEAD, "reason")?;
    same_set("stage", &stages, &code_values(code, "STAGES")?)?;
    same_set("reason", &reasons, &code_values(code, "REASONS")?)
}

fn real_design() -> Result<String, std::io::Error> {
    fs::read_to_string(DESIGN)
}

fn real_code() -> Result<String, std::io::Error> {
    fs::read_to_string(NOTICE)
}

fn fixture(name: &str) -> Result<String, std::io::Error> {
    fs::read_to_string(format!("{FIXTURES}/{name}"))
}

/// The acceptance test: the real design and the real source agree.
#[test]
fn rp30_fallback_stage_and_reason_lists_match_the_code() -> TestResult {
    assert_eq!(check_fallback_lists(&real_design()?, &real_code()?), Ok(()));
    Ok(())
}

/// T-RP-30-001
#[test]
fn design_lists_equal_the_code_constants() -> TestResult {
    assert_eq!(check_fallback_lists(&real_design()?, &real_code()?), Ok(()));
    Ok(())
}

/// T-RP-30-002: the control that makes the `Ok` above a comparison of something.
#[test]
fn the_real_paragraph_yields_13_stages_and_10_reasons() -> TestResult {
    let design = real_design()?;
    assert_eq!(design_values(&design, STAGE_LEAD, "stage")?.len(), 13);
    assert_eq!(design_values(&design, REASON_LEAD, "reason")?.len(), 10);
    Ok(())
}

/// T-RP-30-003
#[test]
fn values_named_for_the_relay_are_not_read_as_hayamimi_values() -> TestResult {
    let design = real_design()?;
    let reasons = design_values(&design, REASON_LEAD, "reason")?;
    for outside in ["memory_budget", "unreachable", "unsupported_target"] {
        assert!(!reasons.contains(outside), "{outside}");
    }
    let stages = design_values(&design, STAGE_LEAD, "stage")?;
    assert!(!stages.contains("sink"));
    assert!(
        stages.contains("translate"),
        "the control: `translate` is also a hayamimi stage"
    );
    Ok(())
}

/// T-RP-30-004
#[test]
fn a_stage_removed_from_the_design_is_named() -> TestResult {
    let said = check_fallback_lists(&fixture("stage_without_lid.md")?, &real_code()?);
    assert_eq!(
        said,
        Err(ListError::OnlyInCode {
            list: "stage",
            value: String::from("lid")
        })
    );
    Ok(())
}

/// T-RP-30-005
#[test]
fn a_reason_removed_from_the_design_is_named() -> TestResult {
    let said = check_fallback_lists(&fixture("reason_without_bad_value.md")?, &real_code()?);
    assert_eq!(
        said,
        Err(ListError::OnlyInCode {
            list: "reason",
            value: String::from("bad_value")
        })
    );
    Ok(())
}

/// T-RP-30-006
#[test]
fn a_stage_added_to_the_design_only_is_named() -> TestResult {
    let said = check_fallback_lists(&fixture("stage_with_diarize.md")?, &real_code()?);
    assert_eq!(
        said,
        Err(ListError::OnlyInDesign {
            list: "stage",
            value: String::from("diarize")
        })
    );
    Ok(())
}

/// T-RP-30-007
#[test]
fn a_reason_added_to_the_design_only_is_named() -> TestResult {
    let said = check_fallback_lists(&fixture("reason_with_timeout.md")?, &real_code()?);
    assert_eq!(
        said,
        Err(ListError::OnlyInDesign {
            list: "reason",
            value: String::from("timeout")
        })
    );
    Ok(())
}

/// T-RP-30-008
#[test]
fn a_stage_added_to_the_code_only_is_named() -> TestResult {
    let said = check_fallback_lists(&real_design()?, &fixture("notice_with_extra_stage.txt")?);
    assert_eq!(
        said,
        Err(ListError::OnlyInCode {
            list: "stage",
            value: String::from("extra_stage")
        })
    );
    Ok(())
}

/// T-RP-30-009
#[test]
fn list_order_does_not_matter() -> TestResult {
    assert_eq!(
        check_fallback_lists(&fixture("reversed.md")?, &real_code()?),
        Ok(())
    );
    Ok(())
}

/// T-RP-30-010
#[test]
fn a_design_without_the_paragraph_is_rejected() -> TestResult {
    let said = check_fallback_lists(&fixture("no_paragraph.md")?, &real_code()?);
    assert_eq!(said, Err(ListError::NoList("stage")));
    Ok(())
}

/// The code side: a source without the constants is refused by name, not read as empty.
#[test]
fn a_source_without_the_constants_is_refused() -> TestResult {
    let said = check_fallback_lists(&real_design()?, "fn main() {}");
    assert_eq!(said, Err(ListError::NoConstant("STAGES")));
    assert!(said.err().is_some_and(|e| e.to_string().contains("STAGES")));
    Ok(())
}

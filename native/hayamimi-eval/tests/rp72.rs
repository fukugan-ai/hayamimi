//! RP-72: a run whose server moved its port is invalid.
//!
//! `hayamimi` moves to a free port when its own is taken, and the measuring
//! tool keeps sending to the port it was given. A Rust run of the RP-32
//! directory whose `rs<N>.events.jsonl` holds a `fallback` with the reason
//! `in_use` is refused by name, and so is a run whose events file is missing.
//! Python's events are not read.
//!
//! The directories are built here from F32 (the formula of RP-32) in the build
//! tree, as the pair-count tests of `rp32.rs` do.

mod support;

use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use support::rp32::{check_dir, rp32_dir, scratch_dir, write_f32_dir};
use support::{Failure, REPO, TestResult, read};

/// A directory of three F32 pairs with the clean `rs<N>.events.jsonl` files.
fn f72_dir(name: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let dir = scratch_dir("rp72-test-dirs", name)?;
    write_f32_dir(&dir, 3, None)?;
    Ok(dir)
}

/// A `fallback`-shaped event line: the one `ingest_port_in_use` writes when
/// `kind` is `fallback`, `reason` is `in_use` and `stage` is `ingest`.
fn event_line(kind: &str, stage: &str, reason: &str) -> String {
    json!({
        "event": {
            "type": kind,
            "stage": stage,
            "reason": reason,
            "to": "next_port",
            "url": "http://127.0.0.1:8767/ingest",
        },
        "t_us": 3,
    })
    .to_string()
}

fn in_use_line() -> String {
    event_line("fallback", "ingest", "in_use")
}

/// Appends `line` to the events file `file` of `dir`.
fn append_event(dir: &Path, file: &str, line: &str) -> TestResult {
    let mut events = std::fs::OpenOptions::new()
        .append(true)
        .open(dir.join(file))?;
    writeln!(events, "{line}")?;
    Ok(())
}

/// The refusal of `check_dir` as text; a pass is an error of the test.
fn refusal_of(dir: &Path) -> Result<Failure, Box<dyn std::error::Error>> {
    match check_dir(dir) {
        Ok(_) => Err(format!("{} was not refused", dir.display()).into()),
        Err(why) => Ok(why),
    }
}

/// A refusal for a port or a missing file says nothing of the values.
fn assert_no_values(why: &str) {
    assert!(
        !why.contains("p50") && !why.contains("p95") && !why.contains("utterances"),
        "{why}"
    );
}

/// T-RP-72-001: no `fallback` anywhere, and no `ports<N>.txt`.
#[test]
fn a_dir_without_a_port_move_passes() -> TestResult {
    check_dir(&f72_dir("t001")?)?;
    Ok(())
}

/// T-RP-72-002: one `in_use` line in the middle run refuses the directory by
/// that run's name, and no value is printed.
#[test]
fn a_run_whose_ingest_port_moved_is_refused_by_name() -> TestResult {
    let dir = f72_dir("t002")?;
    append_event(&dir, "rs2.events.jsonl", &in_use_line())?;
    let why = refusal_of(&dir)?;
    assert!(why.contains("rs2") && why.contains("in_use"), "{why}");
    assert_no_values(&why);
    Ok(())
}

/// T-RP-72-003: the first and the last run are read as well.
#[test]
fn the_first_and_the_last_run_are_read_too() -> TestResult {
    for (index, (file, run)) in [("rs1.events.jsonl", "rs1"), ("rs3.events.jsonl", "rs3")]
        .into_iter()
        .enumerate()
    {
        let dir = f72_dir(&format!("t003-{index}"))?;
        append_event(&dir, file, &in_use_line())?;
        let why = refusal_of(&dir)?;
        assert!(why.contains(run), "{run}: {why}");
    }
    Ok(())
}

/// T-RP-72-004: the port of `/events` moving is a move too (`stage` is not read).
#[test]
fn a_moved_events_port_is_refused_too() -> TestResult {
    let dir = f72_dir("t004")?;
    append_event(
        &dir,
        "rs2.events.jsonl",
        &event_line("fallback", "events", "in_use"),
    )?;
    let why = refusal_of(&dir)?;
    assert!(why.contains("rs2"), "{why}");
    Ok(())
}

/// T-RP-72-005: every run that moved is named; the first is not the only one.
#[test]
fn every_run_that_moved_is_named() -> TestResult {
    let dir = f72_dir("t005")?;
    append_event(&dir, "rs1.events.jsonl", &in_use_line())?;
    append_event(&dir, "rs3.events.jsonl", &in_use_line())?;
    let why = refusal_of(&dir)?;
    assert!(why.contains("rs1") && why.contains("rs3"), "{why}");
    assert!(!why.contains("rs2"), "{why}");
    Ok(())
}

/// T-RP-72-006: a `fallback` for another reason is not a port move.
#[test]
fn a_fallback_for_another_reason_is_not_a_port_move() -> TestResult {
    let dir = f72_dir("t006")?;
    append_event(
        &dir,
        "rs2.events.jsonl",
        &event_line("fallback", "ingest", "unreadable"),
    )?;
    check_dir(&dir)?;
    Ok(())
}

/// T-RP-72-007: `in_use` in an event of another type is not a port move.
#[test]
fn an_event_of_another_type_with_in_use_is_not_a_port_move() -> TestResult {
    let dir = f72_dir("t007")?;
    append_event(
        &dir,
        "rs2.events.jsonl",
        &event_line("notice", "ingest", "in_use"),
    )?;
    check_dir(&dir)?;
    Ok(())
}

/// T-RP-72-008: a run without its events file cannot be checked and is refused.
#[test]
fn a_run_without_its_events_file_is_refused_by_name() -> TestResult {
    let dir = f72_dir("t008")?;
    std::fs::remove_file(dir.join("rs2.events.jsonl"))?;
    let why = refusal_of(&dir)?;
    assert!(why.contains("rs2.events.jsonl"), "{why}");
    assert_no_values(&why);
    Ok(())
}

/// T-RP-72-009: Python's events are not read, and need not stand.
#[test]
fn only_the_rust_events_are_read() -> TestResult {
    let dir = f72_dir("t009")?;
    std::fs::write(dir.join("py1.events.jsonl"), in_use_line() + "\n")?;
    check_dir(&dir)?;
    Ok(())
}

/// T-RP-72-010: two recorded pairs need the events of two runs only.
#[test]
fn a_two_pair_dir_needs_the_events_of_two_runs_only() -> TestResult {
    let dir = scratch_dir("rp72-test-dirs", "t010")?;
    write_f32_dir(&dir, 2, Some("2\n"))?;
    check_dir(&dir)?;
    std::fs::remove_file(dir.join("rs2.events.jsonl"))?;
    let why = refusal_of(&dir)?;
    assert!(why.contains("rs2.events.jsonl"), "{why}");
    assert!(!why.contains("rs3"), "{why}");
    Ok(())
}

/// The events of run `number` of the recorded directory as JSON rows.
fn recorded_events(number: usize) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    let text = read(&rp32_dir().join(format!("rs{number}.events.jsonl")))?;
    let mut rows = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        rows.push(serde_json::from_str(line)?);
    }
    Ok(rows)
}

fn count_of_type(rows: &[Value], kind: &str) -> usize {
    rows.iter()
        .filter(|row| {
            row.get("event")
                .and_then(|event| event.get("type"))
                .and_then(Value::as_str)
                == Some(kind)
        })
        .count()
}

/// The `final` events of each recorded Rust run `rs1` to `rs3`, in run order.
const RECORDED_FINALS: [usize; 3] = [149, 149, 149];

/// T-RP-72-011: the recorded RP-32 runs hold `RECORDED_FINALS` `final` events
/// and no `fallback` each, and the directory's refusal (it has one, for the
/// paired comparison) does not name a port move.
#[test]
fn the_recorded_rp32_dir_has_no_port_move() -> TestResult {
    for (number, finals) in (1..=3).zip(RECORDED_FINALS) {
        let rows = recorded_events(number)?;
        assert_eq!(count_of_type(&rows, "final"), finals, "rs{number}");
        assert_eq!(count_of_type(&rows, "fallback"), 0, "rs{number}");
    }
    let said = check_dir(&rp32_dir());
    assert!(
        said.as_ref()
            .err()
            .is_none_or(|why| !why.contains("in_use")),
        "{said:?}"
    );
    Ok(())
}

/// The text of the section `heading` of a Markdown file: from the heading to
/// the next heading of the same or a higher level.
fn section_of(text: &str, heading: &str) -> Option<String> {
    let level = |line: &str| line.chars().take_while(|c| *c == '#').count();
    let mut lines = text.lines();
    let title = lines.find(|line| line.starts_with('#') && line.contains(heading))?;
    let mut section = vec![title];
    for line in lines {
        if line.starts_with('#') && level(line) <= level(title) {
            break;
        }
        section.push(line);
    }
    Some(section.join("\n"))
}

/// T-RP-72-012: the E2E README's conditions name the step that waits for the
/// ports to be free.
// The README still lacks the step, so this stays ignored until the seat that
// owns `docs/results/e2e/README.md` writes it; the control (one heading) runs
// either way in the test below.
#[test]
#[ignore = "docs/results/e2e/README.md has no port wait step yet: the seat that owns the results docs writes it"]
fn the_e2e_readme_names_the_port_wait_step() -> TestResult {
    let text = read(&Path::new(REPO).join("docs/results/e2e/README.md"))?;
    let Some(section) = section_of(&text, "Conditions of the run") else {
        return Err("no section `Conditions of the run`".into());
    };
    for word in [
        "ss -tanH",
        "Get-NetTCPConnection",
        "8766",
        "8833",
        "ports<N>.txt",
    ] {
        assert!(section.contains(word), "the section lacks {word:?}");
    }
    Ok(())
}

/// The positive control of T-RP-72-012: the heading it reads stands once.
#[test]
fn the_e2e_readme_has_one_conditions_section() -> TestResult {
    let text = read(&Path::new(REPO).join("docs/results/e2e/README.md"))?;
    let headings = text
        .lines()
        .filter(|line| line.contains("Conditions of the run"))
        .count();
    assert_eq!(headings, 1);
    assert!(section_of(&text, "Conditions of the run").is_some());
    Ok(())
}

/// T-RP-72-013: the three directories in turn: the moved port and the missing
/// file are refused by name, the clean one passes.
#[test]
fn rp72_a_run_whose_server_moved_its_port_is_refused() -> TestResult {
    let moved = f72_dir("t013-moved")?;
    append_event(&moved, "rs2.events.jsonl", &in_use_line())?;
    assert!(refusal_of(&moved)?.contains("rs2"));
    let missing = f72_dir("t013-missing")?;
    std::fs::remove_file(missing.join("rs2.events.jsonl"))?;
    assert!(refusal_of(&missing)?.contains("rs2.events.jsonl"));
    check_dir(&f72_dir("t013-clean")?)?;
    Ok(())
}

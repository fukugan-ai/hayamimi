//! RP-83: the gate of an E2E run (T 4.75).
//!
//! Every row reads a fixture and starts nothing outside the build tree. The
//! rows on the judgement (001 to 029, 042, 045 to 052) call
//! `support::machine::check_runs`. The rows on `stream` (030 to 040, 043, 044)
//! start the `hayamimi-eval stream` process against a fake `/ingest` and a
//! `/proc` of their own (`support::s83`), because the real `/proc` of a test
//! that runs under `cargo` always holds a `cargo`.
//!
//!     cd native && cargo test --release --jobs 2 -p hayamimi-eval --test rp83

mod support;

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use support::machine::{GateReport, RunInput, check_runs, f83_rows, f83_text, text_of};
use support::rp32::scratch_dir;
use support::s83::{Ended, FakeIngest, Setup, run, wait_for};
use support::{Failure, REPO, RESULTS, TestResult};

/// The events `stream` writes, and the keys of a refusal. The design names the
/// events and the values; the spelling of the keys is this file's.
const REFUSED: &str = "stream.machine.refused";
const ABORTED: &str = "stream.machine.aborted";
const KEY_REASON: &str = "reason";
const KEY_LOAD1: &str = "load1";
const KEY_LOAD15: &str = "load15";
const KEY_CARGO: &str = "cargo";
const KEY_RUSTC: &str = "rustc";

/// The audio bytes of a 12 second recording at 16 kHz, 16 bits: all of it sent.
const FULL_BYTES: usize = 12 * 16_000 * 2;
/// The `load15` of a first row on the limit, as `/proc/loadavg` writes it.
const LOAD15_ON_THE_LIMIT: &str = "2.50";

fn judged(runs: &[RunInput]) -> Result<GateReport, Failure> {
    check_runs(Path::new(REPO), runs)
}

/// `result` is an `Err` that names every word in `named` and none in `unnamed`.
fn assert_names(result: &Result<GateReport, Failure>, named: &[&str], unnamed: &[&str]) {
    let holds = result.as_ref().err().is_some_and(|why| {
        named.iter().all(|word| why.contains(word))
            && !unnamed.iter().any(|word| why.contains(word))
    });
    assert!(holds, "named {named:?}, not {unnamed:?}: {result:?}");
}

/// A directory of the build tree with an empty-looking `<name>.jsonl` for each name.
fn run_dir(test: &str, names: &[&str]) -> Result<PathBuf, Box<dyn Error>> {
    let dir = scratch_dir("rp83-runs", test)?;
    for name in names {
        std::fs::write(dir.join(format!("{name}.jsonl")), "{}\n")?;
    }
    Ok(dir)
}

/// F83 with `change` applied to its rows, as the text of a file.
fn f83_with(change: impl FnOnce(&mut Vec<Value>)) -> String {
    let mut rows = f83_rows();
    change(&mut rows);
    text_of(&rows)
}

/// The Rust run `rs1` with this machine record.
fn rs1(test: &str, machine: String) -> Result<RunInput, Box<dyn Error>> {
    Ok(RunInput::rust(
        &run_dir(test, &["rs1"])?,
        "rs1",
        Some(machine),
    ))
}

/// `rs1` with F83 changed by `change`.
fn rs1_with(test: &str, change: impl FnOnce(&mut Vec<Value>)) -> Result<RunInput, Box<dyn Error>> {
    rs1(test, f83_with(change))
}

fn passes(run: &RunInput) {
    let result = judged(std::slice::from_ref(run));
    assert!(result.is_ok(), "{result:?}");
}

fn rejected(run: &RunInput, named: &[&str]) {
    assert_names(&judged(std::slice::from_ref(run)), named, &[]);
}

// ---- the judgement on one run (001 to 018) ----

#[test]
fn a_clean_run_passes() -> TestResult {
    passes(&rs1("t001", f83_text())?);
    Ok(())
}

#[test]
fn a_first_row_load1_of_1_99_passes() -> TestResult {
    passes(&rs1_with("t002", |rows| rows[0]["load1"] = json!("1.99"))?);
    Ok(())
}

#[test]
fn a_first_row_load1_of_2_00_is_rejected() -> TestResult {
    let run = rs1_with("t003", |rows| rows[0]["load1"] = json!("2.00"))?;
    rejected(&run, &["rs1", "load1", "2.00"]);
    Ok(())
}

#[test]
fn a_first_row_load15_of_2_49_passes() -> TestResult {
    passes(&rs1_with("t004", |rows| rows[0]["load15"] = json!("2.49"))?);
    Ok(())
}

#[test]
fn a_first_row_load15_of_2_50_is_rejected() -> TestResult {
    let run = rs1_with("t005", |rows| rows[0]["load15"] = json!("2.50"))?;
    rejected(&run, &["rs1", "load15", "2.50"]);
    Ok(())
}

#[test]
fn a_last_row_load5_of_2_49_passes() -> TestResult {
    passes(&rs1_with("t006", |rows| rows[4]["load5"] = json!("2.49"))?);
    Ok(())
}

#[test]
fn a_last_row_load5_of_2_50_is_rejected() -> TestResult {
    let run = rs1_with("t007", |rows| rows[4]["load5"] = json!("2.50"))?;
    rejected(&run, &["rs1", "load5", "2.50"]);
    Ok(())
}

/// Every load the gate does not look at is 9.99: row 1 `load5`, row 5 `load1`
/// and `load15`, and all three loads of rows 2 to 4.
#[test]
fn loads_the_gate_does_not_judge_may_be_high() -> TestResult {
    let run = rs1_with("t008", |rows| {
        rows[0]["load5"] = json!("9.99");
        rows[4]["load1"] = json!("9.99");
        rows[4]["load15"] = json!("9.99");
        for row in &mut rows[1..4] {
            for key in ["load1", "load5", "load15"] {
                row[key] = json!("9.99");
            }
        }
    })?;
    passes(&run);
    Ok(())
}

/// `tag` names the scratch directory, so that two tests never share one.
fn run_009(tag: &str) -> Result<RunInput, Box<dyn Error>> {
    rs1_with(tag, |rows| rows[2]["cargo"] = json!(1))
}

#[test]
fn a_cargo_in_one_middle_row_is_rejected() -> TestResult {
    rejected(&run_009("t009")?, &["rs1", "cargo", "1"]);
    Ok(())
}

#[test]
fn a_rustc_in_one_middle_row_is_rejected() -> TestResult {
    let run = rs1_with("t010", |rows| rows[2]["rustc"] = json!(1))?;
    rejected(&run, &["rs1", "rustc", "1"]);
    Ok(())
}

#[test]
fn a_cargo_in_the_first_row_is_rejected() -> TestResult {
    let run = rs1_with("t011", |rows| rows[0]["cargo"] = json!(1))?;
    rejected(&run, &["rs1", "cargo", "1"]);
    Ok(())
}

#[test]
fn a_rustc_in_the_last_row_is_rejected() -> TestResult {
    let run = rs1_with("t012", |rows| rows[4]["rustc"] = json!(1))?;
    rejected(&run, &["rs1", "rustc", "1"]);
    Ok(())
}

#[test]
fn two_cargo_processes_are_rejected_as_well() -> TestResult {
    let run = rs1_with("t013", |rows| rows[2]["cargo"] = json!(2))?;
    rejected(&run, &["cargo", "2"]);
    Ok(())
}

/// F83 with its `t_us` replaced.
fn at_times(test: &str, times: [i64; 5]) -> Result<RunInput, Box<dyn Error>> {
    rs1_with(test, |rows| {
        for (row, t_us) in rows.iter_mut().zip(times) {
            row["t_us"] = json!(t_us);
        }
    })
}

#[test]
fn a_gap_of_exactly_15_seconds_passes() -> TestResult {
    passes(&at_times(
        "t014",
        [0, 5_000_000, 10_000_000, 25_000_000, 30_000_000],
    )?);
    Ok(())
}

#[test]
fn a_gap_of_15_seconds_and_one_microsecond_is_rejected() -> TestResult {
    let run = at_times("t015", [0, 5_000_000, 10_000_000, 25_000_001, 30_000_001])?;
    rejected(&run, &["rs1", "15000001"]);
    Ok(())
}

fn run_016(tag: &str) -> Result<RunInput, Box<dyn Error>> {
    at_times(tag, [0, 5_000_000, 10_000_000, 30_000_000, 35_000_000])
}

#[test]
fn a_gap_of_20_seconds_is_rejected() -> TestResult {
    rejected(&run_016("t016")?, &["rs1", "20000000"]);
    Ok(())
}

#[test]
fn a_gap_at_either_end_of_the_file_is_rejected() -> TestResult {
    let first_pair = at_times("t017a", [0, 15_000_001, 20_000_001, 25_000_001, 30_000_001])?;
    let last_pair = at_times("t017b", [0, 5_000_000, 10_000_000, 15_000_000, 30_000_001])?;
    rejected(&first_pair, &["15000001"]);
    rejected(&last_pair, &["15000001"]);
    Ok(())
}

#[test]
fn two_rows_at_the_same_time_pass() -> TestResult {
    passes(&at_times(
        "t018",
        [0, 5_000_000, 10_000_000, 10_000_000, 15_000_000],
    )?);
    Ok(())
}

// ---- which runs are judged, and the runs together (019 to 024) ----

#[test]
fn a_python_run_without_a_machine_file_is_not_judged() -> TestResult {
    let dir = run_dir("t019", &["py1", "rs1"])?;
    let runs = [
        RunInput::python(&dir, "py1", None),
        RunInput::rust(&dir, "rs1", Some(f83_text())),
    ];
    assert!(judged(&runs).is_ok());
    Ok(())
}

fn run_020(tag: &str) -> Result<RunInput, Box<dyn Error>> {
    Ok(RunInput::rust(&run_dir(tag, &["rs1"])?, "rs1", None))
}

#[test]
fn a_rust_run_without_a_machine_file_is_rejected() -> TestResult {
    rejected(&run_020("t020")?, &["rs1", "no machine file"]);
    Ok(())
}

#[test]
fn a_python_run_is_not_judged_even_when_it_has_a_file() -> TestResult {
    let dir = run_dir("t021", &["py1"])?;
    let busy = f83_with(|rows| rows[0]["load15"] = json!("3.14"));
    assert!(judged(&[RunInput::python(&dir, "py1", Some(busy))]).is_ok());
    Ok(())
}

#[test]
fn only_the_failing_run_is_named() -> TestResult {
    let dir = run_dir("t022", &["rs1", "rs2", "rs3"])?;
    let busy = f83_with(|rows| rows[0]["load15"] = json!(LOAD15_ON_THE_LIMIT));
    let runs = [
        RunInput::rust(&dir, "rs1", Some(f83_text())),
        RunInput::rust(&dir, "rs2", Some(busy)),
        RunInput::rust(&dir, "rs3", Some(f83_text())),
    ];
    assert_names(&judged(&runs), &["rs2", "load15", "2.50"], &["rs1", "rs3"]);
    Ok(())
}

/// Three runs whose rows 1 to 4 hold `first` and row 5 holds `last`, each as
/// `load1 load5 load15`.
fn runs_with_loads(
    test: &str,
    shapes: [([&str; 3], [&str; 3]); 3],
) -> Result<Vec<RunInput>, Box<dyn Error>> {
    let dir = run_dir(test, &["rs1", "rs2", "rs3"])?;
    let mut runs = Vec::new();
    for (index, (first, last)) in shapes.into_iter().enumerate() {
        let machine = f83_with(|rows| {
            for (at, row) in rows.iter_mut().enumerate() {
                let [load1, load5, load15] = if at == 4 { last } else { first };
                row["load1"] = json!(load1);
                row["load5"] = json!(load5);
                row["load15"] = json!(load15);
            }
        });
        runs.push(RunInput::rust(
            &dir,
            &format!("rs{}", index + 1),
            Some(machine),
        ));
    }
    Ok(runs)
}

/// The three Rust runs of `docs/results/rp32/`, as `load1 load5 load15`.
fn input_023(tag: &str) -> Result<Vec<RunInput>, Box<dyn Error>> {
    runs_with_loads(
        tag,
        [
            (["1.36", "1.99", "2.02"], ["1.27", "1.80", "1.84"]),
            (["0.69", "1.51", "1.73"], ["1.81", "1.66", "1.64"]),
            (["1.17", "1.48", "1.58"], ["2.05", "2.18", "1.97"]),
        ],
    )
}

/// The three runs of the e-card-form measurement, as `load1 load5 load15`.
fn input_024(tag: &str) -> Result<Vec<RunInput>, Box<dyn Error>> {
    runs_with_loads(
        tag,
        [
            (["1.77", "2.48", "3.14"], ["3.01", "3.45", "3.49"]),
            (["1.78", "2.53", "3.03"], ["3.40", "3.17", "3.15"]),
            (["1.89", "2.65", "2.95"], ["3.28", "3.59", "3.48"]),
        ],
    )
}

#[test]
fn the_recorded_rp32_rust_loads_pass() -> TestResult {
    let result = judged(&input_023("t023")?);
    assert!(result.is_ok(), "{result:?}");
    Ok(())
}

#[test]
fn the_recorded_ecard_form_loads_are_rejected_at_load15() -> TestResult {
    let result = judged(&input_024("t024")?);
    assert_names(
        &result,
        &["rs1", "rs2", "rs3", "load15", "3.14", "3.03", "2.95"],
        &[],
    );
    Ok(())
}

// ---- a file that cannot be read as a record (025 to 029, 042, 049, 050) ----

#[test]
fn an_empty_machine_file_is_rejected() -> TestResult {
    rejected(&rs1("t025", String::new())?, &["rs1", "no rows"]);
    Ok(())
}

#[test]
fn a_machine_file_cut_in_its_last_row_is_rejected() -> TestResult {
    let mut text = text_of(&f83_rows()[..4]);
    text.push_str(r#"{"t_us":20000000,"load1":"0.5"#);
    rejected(&rs1("t026", text)?, &["rs1", "row 5", "not readable"]);
    Ok(())
}

#[test]
fn a_row_without_a_rustc_field_is_rejected() -> TestResult {
    let run = rs1_with("t027", |rows| {
        if let Value::Object(row) = &mut rows[2] {
            row.remove("rustc");
        }
    })?;
    rejected(&run, &["rs1", "rustc"]);
    Ok(())
}

#[test]
fn a_load_written_as_a_json_number_is_rejected() -> TestResult {
    let text = f83_text().replacen(r#""load15":"0.50""#, r#""load15":0.50"#, 1);
    assert!(
        text.contains(r#""load15":0.50"#),
        "the fixture was not changed"
    );
    rejected(&rs1("t028", text)?, &["rs1", "load15", "string"]);
    Ok(())
}

#[test]
fn a_machine_file_of_two_rows_passes() -> TestResult {
    passes(&rs1("t029", text_of(&f83_rows()[..2]))?);
    Ok(())
}

#[test]
fn a_machine_file_of_one_row_is_rejected() -> TestResult {
    rejected(
        &rs1("t042", text_of(&f83_rows()[..1]))?,
        &["rs1", "fewer than 2 rows"],
    );
    Ok(())
}

#[test]
fn an_unreadable_load_is_rejected_not_read_as_zero() -> TestResult {
    let run = rs1_with("t049", |rows| rows[0]["load15"] = json!("n/a"))?;
    rejected(&run, &["rs1", "load15"]);
    Ok(())
}

#[test]
fn a_null_load_is_rejected_not_read_as_zero() -> TestResult {
    let run = rs1_with("t050", |rows| rows[0]["load1"] = Value::Null)?;
    rejected(&run, &["rs1", "load1"]);
    Ok(())
}

// ---- the records measured before the gate (045 to 048, 051, 052) ----

/// The files of the records measured before the gate, as (directory, run).
const RECORDS: [(&str, &str); 5] = [
    ("docs/results/rp32", "rs1"),
    ("docs/results/rp32", "rs2"),
    ("docs/results/rp32", "rs3"),
    ("docs/results/e2e/e2e1", "e2e1"),
    ("docs/results/e2e/e2e2", "e2e2"),
];

/// Copies `<dir>/<run>.jsonl` of the repository to `to` under the same
/// relative directory, and returns the directory made.
fn copy_record(
    root: &Path,
    from: (&str, &str),
    to: (&str, &str),
) -> Result<PathBuf, Box<dyn Error>> {
    let source = Path::new(RESULTS)
        .join("..")
        .join("..")
        .join(from.0)
        .join(format!("{}.jsonl", from.1));
    let dir = root.join(to.0);
    std::fs::create_dir_all(&dir)?;
    std::fs::copy(&source, dir.join(format!("{}.jsonl", to.1)))?;
    Ok(dir)
}

/// A copy of the five records on a directory standing in for the repository
/// root, and the five Rust runs without a machine file.
fn stand_records(test: &str) -> Result<(PathBuf, Vec<RunInput>), Box<dyn Error>> {
    let root = scratch_dir("rp83-root", test)?;
    let mut runs = Vec::new();
    for record in RECORDS {
        let dir = copy_record(&root, record, record)?;
        runs.push(RunInput::rust(&dir, record.1, None));
    }
    Ok((root, runs))
}

fn input_045(tag: &str) -> Result<(PathBuf, Vec<RunInput>), Box<dyn Error>> {
    stand_records(tag)
}

/// 045 with one byte (a newline) added to the copy of `rs1.jsonl`.
fn input_046(tag: &str) -> Result<(PathBuf, Vec<RunInput>), Box<dyn Error>> {
    let (root, runs) = stand_records(tag)?;
    let copy = root.join("docs/results/rp32/rs1.jsonl");
    let mut bytes = std::fs::read(&copy)?;
    bytes.push(b'\n');
    std::fs::write(&copy, bytes)?;
    Ok((root, runs))
}

#[test]
fn the_pre_gate_records_are_exempt_and_the_output_says_so() -> TestResult {
    let (root, runs) = input_045("t045")?;
    let report = check_runs(&root, &runs)?;
    let said = report.render();
    assert_eq!(report.exempt, ["rs1", "rs2", "rs3", "e2e1", "e2e2"]);
    assert!(said.contains("pre-gate"), "{said}");
    for (_, name) in RECORDS {
        assert!(said.contains(name), "{name}: {said}");
    }
    Ok(())
}

#[test]
fn a_run_with_the_same_name_and_a_different_sha256_is_not_exempt() -> TestResult {
    let (root, runs) = input_046("t046")?;
    assert_names(
        &check_runs(&root, &runs),
        &["rs1", "no machine file"],
        &["rs2", "rs3", "e2e1", "e2e2"],
    );
    Ok(())
}

#[test]
fn an_exemption_needs_the_dir_and_the_name_as_well_as_the_sha256() -> TestResult {
    let root = scratch_dir("rp83-root", "t047")?;
    let other_dir = copy_record(&root, RECORDS[0], ("docs/results/other", "rs1"))?;
    let other_name = copy_record(&root, RECORDS[0], ("docs/results/rp32", "rs4"))?;
    let in_another_dir = [RunInput::rust(&other_dir, "rs1", None)];
    let under_another_name = [RunInput::rust(&other_name, "rs4", None)];
    assert_names(
        &check_runs(&root, &in_another_dir),
        &["rs1", "no machine file"],
        &[],
    );
    assert_names(
        &check_runs(&root, &under_another_name),
        &["rs4", "no machine file"],
        &[],
    );
    Ok(())
}

#[test]
fn only_the_run_off_the_list_is_named_when_a_listed_run_is_present() -> TestResult {
    let root = scratch_dir("rp83-root", "t048")?;
    let dir = copy_record(&root, RECORDS[0], RECORDS[0])?;
    copy_record(&root, RECORDS[0], ("docs/results/rp32", "rs4"))?;
    let runs = [
        RunInput::rust(&dir, "rs1", None),
        RunInput::rust(&dir, "rs4", None),
    ];
    assert_names(
        &check_runs(&root, &runs),
        &["rs4", "no machine file"],
        &["rs1"],
    );
    Ok(())
}

/// One listed run, `rs1`, on a directory standing in for the repository root.
fn listed_rs1(test: &str, machine: Option<String>) -> Result<(PathBuf, RunInput), Box<dyn Error>> {
    let root = scratch_dir("rp83-root", test)?;
    let dir = copy_record(&root, RECORDS[0], RECORDS[0])?;
    Ok((root, RunInput::rust(&dir, "rs1", machine)))
}

#[test]
fn a_listed_run_with_a_machine_file_is_judged_like_any_other() -> TestResult {
    let (root, clean) = listed_rs1("t051a", Some(f83_text()))?;
    let report = check_runs(&root, &[clean])?;
    assert!(report.exempt.is_empty());
    assert!(!report.render().contains("rs1"), "{}", report.render());
    let busy = f83_with(|rows| rows[0]["load15"] = json!(LOAD15_ON_THE_LIMIT));
    let (root, busy) = listed_rs1("t051b", Some(busy))?;
    assert_names(&check_runs(&root, &[busy]), &["rs1", "load15", "2.50"], &[]);
    Ok(())
}

#[test]
fn a_listed_run_without_a_machine_file_is_exempt() -> TestResult {
    let (root, listed) = listed_rs1("t052", None)?;
    let report = check_runs(&root, &[listed])?;
    assert_eq!(report.exempt, ["rs1"]);
    assert!(report.render().contains("rs1"), "{}", report.render());
    Ok(())
}

// ---- `stream` (030 to 040, 043, 044) ----

/// What one `stream` run did: the process, the audio the fake `/ingest` got,
/// and the machine record it left.
struct Observed {
    ended: Ended,
    audio: usize,
    machine: Option<String>,
}

/// Runs `stream` for `name` on a 1 second recording, under this `loadavg` and
/// these processes (`<dir>`, `comm`). `extra` are more arguments.
fn stream_once(
    name: &str,
    loadavg: &str,
    processes: &[(&str, &str)],
    extra: &[&str],
) -> Result<Observed, Box<dyn Error>> {
    let setup = Setup::new(name, 1)?;
    setup.proc_root.set_loadavg(loadavg)?;
    for (dir, comm) in processes {
        setup.proc_root.add_process(dir, comm)?;
    }
    let ingest = FakeIngest::start()?;
    let ended = run(&setup, ingest.port, 1, extra)?;
    Ok(Observed {
        ended,
        audio: ingest.finish()?,
        machine: setup.machine_text(),
    })
}

/// The single refusal event of a run, with the values it carries.
struct Expected {
    reason: &'static str,
    load1: &'static str,
    load15: &'static str,
    cargo: u64,
    rustc: u64,
}

fn assert_refused(seen: &Observed, expected: &Expected) {
    let events = seen.ended.events(REFUSED);
    assert_eq!(events.len(), 1, "{}", seen.ended.stderr);
    let event = &events[0];
    assert_eq!(event["level"], "warn");
    assert_eq!(event[KEY_REASON], expected.reason);
    assert_eq!(event[KEY_LOAD1], expected.load1);
    assert_eq!(event[KEY_LOAD15], expected.load15);
    assert_eq!(event[KEY_CARGO], expected.cargo);
    assert_eq!(event[KEY_RUSTC], expected.rustc);
    assert_eq!(seen.ended.code, Some(2), "{}", seen.ended.stderr);
    assert_eq!(seen.audio, 0, "a refused run sent audio");
    assert!(seen.ended.events(ABORTED).is_empty());
}

/// The rows of the machine record as JSON.
fn rows_of(seen: &Observed) -> Result<Vec<Value>, Box<dyn Error>> {
    let Some(text) = &seen.machine else {
        return Err("the run left no machine record".into());
    };
    Ok(text
        .lines()
        .map(serde_json::from_str::<Value>)
        .collect::<Result<_, _>>()?)
}

#[test]
fn a_first_row_load1_of_2_00_stops_the_stream_before_any_audio() -> TestResult {
    let seen = stream_once("t030", "2.00 0.50 0.50 1/300 1000", &[], &[])?;
    assert_refused(
        &seen,
        &Expected {
            reason: "load1",
            load1: "2.00",
            load15: "0.50",
            cargo: 0,
            rustc: 0,
        },
    );
    let rows = rows_of(&seen)?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["load1"], "2.00");
    Ok(())
}

#[test]
fn a_first_row_one_step_inside_every_line_starts_the_stream() -> TestResult {
    let seen = stream_once("t031", "1.99 9.99 2.49 1/300 1000", &[], &[])?;
    assert!(
        !matches!(seen.ended.code, Some(2 | 3)),
        "{:?}",
        seen.ended.code
    );
    assert!(seen.ended.events(REFUSED).is_empty());
    assert!(seen.ended.events(ABORTED).is_empty());
    assert!(seen.audio >= 1, "no audio was sent");
    let rows = rows_of(&seen)?;
    assert_eq!(
        (&rows[0]["load1"], &rows[0]["load15"]),
        (&json!("1.99"), &json!("2.49"))
    );
    Ok(())
}

#[test]
fn a_first_row_load15_of_2_50_stops_the_stream_before_any_audio() -> TestResult {
    let seen = stream_once("t032", "0.50 0.50 2.50 1/300 1000", &[], &[])?;
    assert_refused(
        &seen,
        &Expected {
            reason: "load15",
            load1: "0.50",
            load15: "2.50",
            cargo: 0,
            rustc: 0,
        },
    );
    Ok(())
}

const CLEAN_LOADAVG: &str = "0.50 0.50 0.50 1/300 1000";

#[test]
fn a_cargo_process_before_the_run_stops_the_stream() -> TestResult {
    let seen = stream_once("t033", CLEAN_LOADAVG, &[("4242", "cargo")], &[])?;
    assert_refused(
        &seen,
        &Expected {
            reason: "build",
            load1: "0.50",
            load15: "0.50",
            cargo: 1,
            rustc: 0,
        },
    );
    Ok(())
}

#[test]
fn a_rustc_process_before_the_run_stops_the_stream() -> TestResult {
    let seen = stream_once("t034", CLEAN_LOADAVG, &[("4242", "rustc")], &[])?;
    assert_refused(
        &seen,
        &Expected {
            reason: "build",
            load1: "0.50",
            load15: "0.50",
            cargo: 0,
            rustc: 1,
        },
    );
    Ok(())
}

#[test]
fn names_that_only_resemble_cargo_are_not_counted() -> TestResult {
    let lookalikes = [
        ("4242", "cargo-nextest"),
        ("4243", "mycargo"),
        ("4244", "Cargo"),
        ("sys", "cargo"),
    ];
    let seen = stream_once("t035", CLEAN_LOADAVG, &lookalikes, &[])?;
    assert!(
        !matches!(seen.ended.code, Some(2 | 3)),
        "{}",
        seen.ended.stderr
    );
    assert!(seen.ended.events(REFUSED).is_empty());
    assert!(seen.audio >= 1);
    let rows = rows_of(&seen)?;
    assert_eq!(
        (&rows[0][KEY_CARGO], &rows[0][KEY_RUSTC]),
        (&json!(0), &json!(0))
    );
    Ok(())
}

#[test]
fn two_cargo_processes_before_the_run_are_counted_as_two() -> TestResult {
    let two = [("4242", "cargo"), ("4243", "cargo")];
    let seen = stream_once("t036", CLEAN_LOADAVG, &two, &[])?;
    assert_refused(
        &seen,
        &Expected {
            reason: "build",
            load1: "0.50",
            load15: "0.50",
            cargo: 2,
            rustc: 0,
        },
    );
    Ok(())
}

/// A wrong flag is a usage error with the exit code 2 too, so the refusal is
/// told by its event and not by the code.
#[test]
fn a_usage_error_is_not_read_as_a_machine_refusal() -> TestResult {
    let seen = stream_once("t037", CLEAN_LOADAVG, &[], &["--proc-rootx", "x"])?;
    assert_eq!(seen.ended.code, Some(2), "{}", seen.ended.stderr);
    assert!(seen.ended.events(REFUSED).is_empty());
    assert_eq!(seen.audio, 0);
    Ok(())
}

#[test]
fn proc_root_defaults_to_proc() -> TestResult {
    let output = Command::new(env!("CARGO_BIN_EXE_hayamimi-eval"))
        .args(["stream", "--help"])
        .output()?;
    let help = String::from_utf8_lossy(&output.stdout);
    assert!(help.contains("--proc-root"), "{help}");
    assert!(help.contains("[default: /proc]"), "{help}");
    Ok(())
}

#[test]
fn a_cargo_process_during_the_run_aborts_the_stream_with_exit_code_3() -> TestResult {
    let setup = Setup::new("t039", 12)?;
    let ingest = FakeIngest::start()?;
    let child = setup.spawn(ingest.port, 1, &[])?;
    let waited = Instant::now();
    while ingest.audio_bytes() == 0 {
        assert!(
            waited.elapsed() < Duration::from_secs(30),
            "no audio was sent"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    setup.proc_root.add_process("4242", "cargo")?;
    let ended = wait_for(child, Instant::now())?;
    let audio = ingest.finish()?;
    assert_eq!(ended.code, Some(3), "{}", ended.stderr);
    assert!(
        ended.elapsed < Duration::from_secs(8),
        "{:?}",
        ended.elapsed
    );
    let aborted = ended.events(ABORTED);
    assert_eq!(aborted.len(), 1, "{}", ended.stderr);
    assert_eq!(aborted[0]["level"], "warn");
    assert_eq!(
        (&aborted[0][KEY_CARGO], &aborted[0][KEY_RUSTC]),
        (&json!(1), &json!(0))
    );
    assert!(ended.events(REFUSED).is_empty());
    assert!(audio < FULL_BYTES, "all the audio was sent: {audio} bytes");
    let Some(text) = setup.machine_text() else {
        return Err("the run left no machine record".into());
    };
    let last = text.lines().last().map(serde_json::from_str::<Value>);
    assert!(
        matches!(&last, Some(Ok(row)) if row[KEY_CARGO].as_u64() >= Some(1)),
        "{last:?}"
    );
    let dir = setup.machine_file().parent().map(Path::to_path_buf);
    let Some(dir) = dir else {
        return Err("the record has no directory".into());
    };
    assert_names(
        &judged(&[RunInput::rust(&dir, "s83", Some(text))]),
        &["s83", "cargo"],
        &[],
    );
    Ok(())
}

#[test]
fn a_run_beside_no_build_writes_rows_through_the_drain_and_a_file_that_passes() -> TestResult {
    let setup = Setup::new("t040", 12)?;
    let ingest = FakeIngest::start()?;
    let ended = run(&setup, ingest.port, 11, &[])?;
    let audio = ingest.finish()?;
    assert!(!matches!(ended.code, Some(2 | 3)), "{}", ended.stderr);
    assert!(ended.events(ABORTED).is_empty());
    assert_eq!(audio, FULL_BYTES);
    let Some(text) = setup.machine_text() else {
        return Err("the run left no machine record".into());
    };
    let times = text
        .lines()
        .map(|line| {
            serde_json::from_str::<Value>(line)?["t_us"]
                .as_i64()
                .ok_or("no t_us".into())
        })
        .collect::<Result<Vec<i64>, Box<dyn Error>>>()?;
    assert!(times.len() >= 5, "{times:?}");
    assert!(
        times
            .windows(2)
            .all(|pair| pair[0] < pair[1] && pair[1] - pair[0] <= 15_000_000),
        "{times:?}"
    );
    let in_the_drain = times.iter().filter(|t_us| **t_us > 12_000_000).count();
    assert!(in_the_drain >= 2, "{times:?}");
    let Some(dir) = setup.machine_file().parent().map(Path::to_path_buf) else {
        return Err("the record has no directory".into());
    };
    let result = judged(&[RunInput::rust(&dir, "s83", Some(text))]);
    assert!(result.is_ok(), "{result:?}");
    Ok(())
}

#[test]
fn the_refusal_reason_is_load1_when_all_three_fail() -> TestResult {
    let seen = stream_once(
        "t043",
        "2.00 0.50 2.50 1/300 1000",
        &[("4242", "cargo")],
        &[],
    )?;
    assert_refused(
        &seen,
        &Expected {
            reason: "load1",
            load1: "2.00",
            load15: "2.50",
            cargo: 1,
            rustc: 0,
        },
    );
    assert_eq!(rows_of(&seen)?.len(), 1);
    Ok(())
}

#[test]
fn the_refusal_reason_is_load15_when_load15_and_build_fail() -> TestResult {
    let seen = stream_once(
        "t044",
        "0.50 0.50 2.50 1/300 1000",
        &[("4242", "cargo")],
        &[],
    )?;
    assert_refused(
        &seen,
        &Expected {
            reason: "load15",
            load1: "0.50",
            load15: "2.50",
            cargo: 1,
            rustc: 0,
        },
    );
    Ok(())
}

// ---- the pressure columns (proposed T-RP-83-053 and 054) ----

const PRESSURE_UNAVAILABLE: &str = "stream.machine.pressure_unavailable";

#[test]
fn rows_carry_the_io_pressure_totals_when_the_kernel_shows_them() -> TestResult {
    let setup = Setup::new("t053", 1)?;
    setup.proc_root.set_pressure(12_345, 678)?;
    let ingest = FakeIngest::start()?;
    let ended = run(&setup, ingest.port, 1, &[])?;
    ingest.finish()?;
    assert_eq!(ended.code, Some(0), "{}", ended.stderr);
    assert!(
        ended.events(PRESSURE_UNAVAILABLE).is_empty(),
        "{}",
        ended.stderr
    );
    let Some(text) = setup.machine_text() else {
        return Err("the run left no machine record".into());
    };
    for line in text.lines() {
        let row: Value = serde_json::from_str(line)?;
        assert_eq!(
            (&row["io_some_total"], &row["io_full_total"]),
            (&json!(12_345), &json!(678))
        );
    }
    Ok(())
}

#[test]
fn rows_carry_null_and_one_event_says_so_when_the_kernel_hides_the_pressure() -> TestResult {
    let seen = stream_once("t054", CLEAN_LOADAVG, &[], &[])?;
    assert_eq!(seen.ended.code, Some(0), "{}", seen.ended.stderr);
    let events = seen.ended.events(PRESSURE_UNAVAILABLE);
    assert_eq!(events.len(), 1, "{}", seen.ended.stderr);
    assert_eq!(events[0]["level"], "warn");
    let rows = rows_of(&seen)?;
    assert!(rows.len() >= 2);
    assert!(
        rows.iter()
            .all(|row| row["io_some_total"].is_null() && row["io_full_total"].is_null())
    );
    // The judge reads no pressure: the same rows pass.
    let dir = run_dir("t054", &["rs1"])?;
    let Some(text) = &seen.machine else {
        return Err("the run left no machine record".into());
    };
    let result = judged(&[RunInput::rust(&dir, "rs1", Some(text.clone()))]);
    assert!(result.is_ok(), "{result:?}");
    Ok(())
}

// ---- the acceptance (041) ----

/// 023 and 024, 009, 016, 020, 042, 045 and 046 handed to the judgement in
/// that order: pass, fail, fail, fail, fail, fail, pass, fail.
#[test]
fn rp83_a_run_under_a_busy_machine_or_beside_a_build_is_refused() -> TestResult {
    assert!(judged(&input_023("t041-023")?).is_ok());
    assert_names(&judged(&input_024("t041-024")?), &["load15"], &[]);
    assert_names(&judged(&[run_009("t041-009")?]), &["cargo"], &[]);
    assert_names(&judged(&[run_016("t041-016")?]), &["20000000"], &[]);
    assert_names(&judged(&[run_020("t041-020")?]), &["no machine file"], &[]);
    let one_row = rs1("t041-042", text_of(&f83_rows()[..1]))?;
    assert_names(&judged(&[one_row]), &["fewer than 2 rows"], &[]);
    let (root, runs) = input_045("t041-045")?;
    let listed = check_runs(&root, &runs)?;
    assert_eq!(listed.exempt.len(), 5, "{}", listed.render());
    for (_, name) in RECORDS {
        assert!(listed.render().contains(name), "{}", listed.render());
    }
    let (root, runs) = input_046("t041-046")?;
    assert_names(&check_runs(&root, &runs), &["rs1"], &[]);
    Ok(())
}

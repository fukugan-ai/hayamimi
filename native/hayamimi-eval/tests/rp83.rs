//! RP-83: the gate of an E2E run (T 4.75).
//!
//! Every row reads a fixture and starts nothing outside the build tree. The
//! rows on the judgement (001 to 029, 042, 045 to 052, 055 to 067, 080 (c),
//! 089) call `support::machine::check_runs`. The rows on `stream` (030 to 040,
//! 043, 044, 053, 054, 068 to 088, 090) start the `hayamimi-eval stream`
//! process against a fake `/ingest` and a `/proc` of their own
//! (`support::s83`), because the real `/proc` of a test that runs under
//! `cargo` always holds a `cargo`. That `/proc` is built by the test in the
//! build tree, `cmdline` bytes included.
//!
//!     cd native && cargo test --release --jobs 2 -p hayamimi-eval --test rp83

mod support;

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use hayamimi_eval::machine::ticks_of_stat;
use serde_json::{Value, json};
use support::machine::{
    GateReport, RunInput, check_runs, f83_rows, f83_text, gate_lines, o83_rows, text_of,
    two_o83_rows, without_own_fields,
};
use support::rp32::scratch_dir;
use support::s83::{
    Ended, FakeIngest, OWN_PIDS, OWN_TICKS, ProcRoot, SERVER_WORDS, Setup, own_args, run, wait_for,
};
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

// ---- the pressure columns are not judged (055) ----

#[test]
fn the_judgement_ignores_the_values_of_the_pressure_columns() -> TestResult {
    let odd = f83_with(|rows| {
        rows[1]["io_some_total"] = json!("x");
        rows[2]["io_full_total"] = json!(-1);
        rows[3]["io_some_total"] = json!(1.5);
    });
    passes(&rs1("t055a", odd)?);
    let real = f83_with(|rows| {
        for row in rows.iter_mut() {
            row["io_some_total"] = json!(12_345);
            row["io_full_total"] = json!(678);
        }
    });
    passes(&rs1("t055b", real)?);
    Ok(())
}

// ---- the run's own share of load5 (056 to 067, 089) ----

/// O83 with `per_row` ticks more in each row, as the Rust run `rs1`.
fn o83_run(test: &str, per_row: u64) -> Result<RunInput, Box<dyn Error>> {
    rs1(test, text_of(&o83_rows(per_row)))
}

/// A file of the window E3' that holds no `own`, as the Rust run `name`.
fn recorded_run(test: &str, name: &str) -> Result<RunInput, Box<dyn Error>> {
    let path = Path::new(RESULTS)
        .join("r8x")
        .join(format!("{name}.machine.jsonl"));
    Ok(RunInput::rust(
        &run_dir(test, &[name])?,
        name,
        Some(std::fs::read_to_string(path)?),
    ))
}

/// The load5 of the last row of O83 and the limit a judgement reports.
const O83_LOAD5: &str = "3.10";

#[test]
fn a_load5_of_3_10_with_an_own_share_of_0_61_passes() -> TestResult {
    passes(&o83_run("t056", 370)?);
    Ok(())
}

#[test]
fn a_load5_of_3_10_with_an_own_share_of_0_60_is_rejected() -> TestResult {
    rejected(&o83_run("t057", 364)?, &["rs1", "load5", O83_LOAD5]);
    Ok(())
}

#[test]
fn an_own_share_of_0_609_is_cut_to_60_not_rounded_to_61() -> TestResult {
    rejected(&o83_run("t058", 369)?, &["rs1", "load5", O83_LOAD5]);
    Ok(())
}

#[test]
fn a_two_row_file_with_an_own_share_of_0_48_and_a_load5_of_2_97_passes() -> TestResult {
    passes(&rs1("t059", text_of(&two_o83_rows("2.97")))?);
    Ok(())
}

#[test]
fn a_two_row_file_with_an_own_share_of_0_48_and_a_load5_of_2_98_is_rejected() -> TestResult {
    rejected(
        &rs1("t060", text_of(&two_o83_rows("2.98")))?,
        &["rs1", "load5", "2.98"],
    );
    Ok(())
}

#[test]
fn a_load5_of_3_10_with_no_own_work_is_rejected() -> TestResult {
    rejected(&o83_run("t061", 0)?, &["rs1", "load5", O83_LOAD5]);
    Ok(())
}

#[test]
fn a_load5_of_2_11_passes_with_an_own_share_of_zero() -> TestResult {
    let mut rows = o83_rows(0);
    rows[105]["load5"] = json!("2.11");
    passes(&rs1("t062a", text_of(&rows))?);
    passes(&recorded_run("t062b", "e3p1.e2e1")?);
    Ok(())
}

#[test]
fn a_file_without_own_is_judged_without_subtracting() -> TestResult {
    rejected(
        &recorded_run("t063a", "e3p1.e2e2")?,
        &["e3p1.e2e2", "load5", O83_LOAD5],
    );
    let old_form = text_of(&without_own_fields(o83_rows(370)));
    rejected(&rs1("t063b", old_form)?, &["rs1", "load5", O83_LOAD5]);
    Ok(())
}

#[test]
fn an_own_cpu_ticks_below_the_previous_row_is_named_by_its_row() -> TestResult {
    let mut rows = o83_rows(370);
    rows[49]["own_cpu_ticks"] = json!(48 * 370 - 1);
    let result = judged(&[rs1("t064", text_of(&rows))?]);
    assert_names(&result, &["rs1", "row 50", "own_cpu_ticks"], &["row 51"]);
    Ok(())
}

/// A change to one row of a file.
type RowChange = fn(&mut Value);

#[test]
fn an_own_cpu_ticks_that_is_missing_or_not_an_integer_is_rejected() -> TestResult {
    let broken: [(&str, RowChange); 3] = [
        ("t065a", |row| {
            if let Value::Object(map) = row {
                map.remove("own_cpu_ticks");
            }
        }),
        ("t065b", |row| row["own_cpu_ticks"] = json!("22200")),
        ("t065c", |row| row["own_cpu_ticks"] = Value::Null),
    ];
    for (test, change) in broken {
        let mut rows = o83_rows(370);
        change(&mut rows[59]);
        let result = judged(&[rs1(test, text_of(&rows))?]);
        assert_names(&result, &["rs1", "row 60", "own_cpu_ticks"], &[]);
    }
    Ok(())
}

#[test]
fn a_first_row_with_own_but_without_clk_tck_is_rejected() -> TestResult {
    let mut rows = o83_rows(370);
    if let Value::Object(map) = &mut rows[0] {
        map.remove("clk_tck");
    }
    let result = judged(&[rs1("t066", text_of(&rows))?]);
    assert_names(&result, &["rs1", "clk_tck"], &[]);
    Ok(())
}

#[test]
fn own_cpu_ticks_of_one_stat_text_is_utime_plus_stime() -> TestResult {
    let tail = " S 1 4242 4242 0 -1 4194560 100 0 0 0 1000 250 0 0 20 0 8 0 12345 1000000 100 \
                18446744073709551615";
    assert_eq!(ticks_of_stat(&format!("4242 (hayamimi){tail}"))?, 1250);
    assert_eq!(ticks_of_stat(&format!("4242 (a b) c){tail}"))?, 1250);
    let short = "4242 (hayamimi) S 1 4242 4242 0 -1 4194560 100 0 0 0";
    assert!(ticks_of_stat(short).is_err());
    Ok(())
}

/// The lines the judgement reports for `runs`, `expected` among them once.
fn assert_reports_once(runs: &[RunInput], expected: &str) {
    let lines = gate_lines(runs);
    let count = lines
        .iter()
        .filter(|line| line.as_str() == expected)
        .count();
    assert_eq!(count, 1, "{expected:?} in {lines:?}");
}

#[test]
fn the_judgement_prints_one_line_per_run_with_own5_and_the_judged_value() -> TestResult {
    let passing = o83_run("t089a", 370)?;
    let failing = o83_run("t089b", 364)?;
    let on_the_limit = "rs1 load5 3.10 own5 0.601493 (60) judged 250 limit 250";
    let inside = "rs1 load5 3.10 own5 0.611407 (61) judged 249 limit 250";
    assert_reports_once(std::slice::from_ref(&passing), inside);
    assert!(judged(std::slice::from_ref(&passing)).is_ok());
    assert_reports_once(std::slice::from_ref(&failing), on_the_limit);
    assert!(judged(std::slice::from_ref(&failing)).is_err());
    let old_form = recorded_run("t089c", "e3p1.e2e2")?;
    let absent = "e3p1.e2e2 load5 3.10 own absent judged 310 limit 250";
    assert_reports_once(std::slice::from_ref(&old_form), absent);
    assert!(judged(std::slice::from_ref(&old_form)).is_err());
    let second = RunInput {
        name: String::from("rs2"),
        ..o83_run("t089d", 364)?
    };
    let both = [passing, second];
    assert_reports_once(&both, inside);
    assert_reports_once(
        &both,
        "rs2 load5 3.10 own5 0.601493 (60) judged 250 limit 250",
    );
    Ok(())
}

// ---- `stream` with `--own-pid` (068 to 088, 090) ----

const KEY_OWN_FAULT: &str = "own_fault";
const KEY_OWN_PID: &str = "own_pid";
/// The `/proc` of such a test: what a test adds to the three processes of a run.
type Prepare = fn(&ProcRoot) -> Result<(), Box<dyn Error>>;

fn nothing_more(_: &ProcRoot) -> Result<(), Box<dyn Error>> {
    Ok(())
}

/// Runs `stream` on a 1 second recording in the `/proc` of S83o after
/// `prepare` has changed it, naming `pids` by `--own-pid` and adding `extra`.
fn own_once(
    name: &str,
    pids: &[u32],
    prepare: impl FnOnce(&ProcRoot) -> Result<(), Box<dyn Error>>,
    extra: &[&str],
) -> Result<Observed, Box<dyn Error>> {
    let setup = Setup::new_own(name, 1)?;
    prepare(&setup.proc_root)?;
    let named = own_args(pids);
    let mut arguments: Vec<&str> = named.iter().map(String::as_str).collect();
    arguments.extend_from_slice(extra);
    let ingest = FakeIngest::start()?;
    let ended = run(&setup, ingest.port, 1, &arguments)?;
    Ok(Observed {
        ended,
        audio: ingest.finish()?,
        machine: setup.machine_text(),
    })
}

/// The three processes of a run and one more, named last.
fn with_one_more(pid: u32) -> [u32; 4] {
    [OWN_PIDS[0], OWN_PIDS[1], OWN_PIDS[2], pid]
}

/// The run was refused for `--own-pid` with this fault and pid (`None`: a fault
/// of the set, whose `own_pid` is `null`), before any audio.
fn assert_own_refused(seen: &Observed, fault: &str, pid: Option<u32>) {
    let events = seen.ended.events(REFUSED);
    assert_eq!(events.len(), 1, "{}", seen.ended.stderr);
    let event = &events[0];
    assert_eq!(event["level"], "warn");
    assert_eq!(event[KEY_REASON], "own_pid");
    assert_eq!(event[KEY_OWN_FAULT], fault);
    match pid {
        Some(pid) => assert_eq!(event[KEY_OWN_PID], pid),
        None => assert!(
            event.get(KEY_OWN_PID).is_some_and(Value::is_null),
            "{event}"
        ),
    }
    assert!(event[KEY_LOAD1].is_string() && event[KEY_LOAD15].is_string());
    assert!(event[KEY_CARGO].is_u64() && event[KEY_RUSTC].is_u64());
    assert_eq!(seen.ended.code, Some(2), "{}", seen.ended.stderr);
    assert_eq!(seen.audio, 0, "a refused run sent audio");
    assert!(seen.ended.events(ABORTED).is_empty());
}

/// `stream` is refused for `fault` of `pid` when `pids` are named after `prepare`.
fn assert_refused_for(
    name: &str,
    pids: &[u32],
    prepare: impl FnOnce(&ProcRoot) -> Result<(), Box<dyn Error>>,
    (fault, pid): (&str, Option<u32>),
) -> TestResult {
    let seen = own_once(name, pids, prepare, &[])?;
    assert_own_refused(&seen, fault, pid);
    Ok(())
}

#[test]
fn an_own_pid_set_of_the_three_processes_starts_the_stream_and_is_recorded() -> TestResult {
    let seen = own_once("t068", &OWN_PIDS, nothing_more, &[])?;
    assert!(
        !matches!(seen.ended.code, Some(2 | 3)),
        "{}",
        seen.ended.stderr
    );
    assert!(seen.ended.events(REFUSED).is_empty());
    assert!(seen.ended.events(ABORTED).is_empty());
    assert!(seen.audio >= 1, "no audio was sent");
    let rows = rows_of(&seen)?;
    let named = json!([
        {"pid": 4242, "comm": "hayamimi"},
        {"pid": 4243, "comm": "hayamimi-relay"},
        {"pid": 4244, "comm": "llama-server"},
    ]);
    assert_eq!(rows[0]["own"], named);
    assert_eq!(rows[0]["clk_tck"], 100);
    assert!(
        rows.iter()
            .all(|row| row["own_cpu_ticks"] == json!(OWN_TICKS))
    );
    let Some(text) = &seen.machine else {
        return Err("the run left no machine record".into());
    };
    passes(&rs1("t068", text.clone())?);
    Ok(())
}

#[test]
fn a_sudo_pid_stops_the_stream_before_any_audio() -> TestResult {
    let prepare = |root: &ProcRoot| root.add_own(4246, "sudo", 10, 10);
    assert_refused_for("t069", &with_one_more(4246), prepare, ("comm", Some(4246)))
}

#[test]
fn a_pid_with_another_comm_stops_the_stream_before_any_audio() -> TestResult {
    let prepare = |root: &ProcRoot| root.add_own(4246, "bash", 10, 10);
    assert_refused_for("t070", &with_one_more(4246), prepare, ("comm", Some(4246)))
}

/// The server's command line replaced by `words`, for a pid check on the port.
fn server_started_with(
    words: &'static [&'static str],
) -> impl FnOnce(&ProcRoot) -> Result<(), Box<dyn Error>> {
    move |root| root.set_cmdline(4244, words)
}

#[test]
fn a_llama_server_on_port_8080_stops_the_stream_before_any_audio() -> TestResult {
    let words = &["llama-server", "-m", "model.gguf", "--port", "8080"];
    let port = ("port", Some(4244));
    assert_refused_for("t071", &OWN_PIDS, server_started_with(words), port)
}

#[test]
fn a_llama_server_on_a_port_that_only_contains_18084_stops_the_stream() -> TestResult {
    let port = ("port", Some(4244));
    let beyond = &["llama-server", "-m", "model.gguf", "--port", "180840"];
    assert_refused_for("t072a", &OWN_PIDS, server_started_with(beyond), port)?;
    let before = &["llama-server", "-m", "model.gguf", "--port", "118084"];
    assert_refused_for("t072b", &OWN_PIDS, server_started_with(before), port)
}

#[test]
fn a_port_flag_with_no_value_stops_the_stream() -> TestResult {
    let words = &["llama-server", "-m", "model.gguf", "--port"];
    let port = ("port", Some(4244));
    assert_refused_for("t073", &OWN_PIDS, server_started_with(words), port)
}

#[test]
fn a_18084_that_is_not_the_value_of_port_stops_the_stream() -> TestResult {
    let port = ("port", Some(4244));
    let later = &["llama-server", "--port", "8080", "--parallel", "18084"];
    assert_refused_for("t074a", &OWN_PIDS, server_started_with(later), port)?;
    let first = &["llama-server", "18084", "--port", "8080"];
    assert_refused_for("t074b", &OWN_PIDS, server_started_with(first), port)
}

#[test]
fn an_unreadable_or_empty_cmdline_stops_the_stream() -> TestResult {
    let port = ("port", Some(4244));
    assert_refused_for(
        "t075a",
        &OWN_PIDS,
        |root| root.remove(4244, "cmdline"),
        port,
    )?;
    assert_refused_for(
        "t075b",
        &OWN_PIDS,
        |root| root.set_cmdline_bytes(4244, b""),
        port,
    )
}

#[test]
fn a_pid_that_cannot_be_read_stops_the_stream() -> TestResult {
    let pids = with_one_more(4250);
    let unreadable = ("unreadable", Some(4250));
    assert_refused_for("t076a", &pids, nothing_more, unreadable)?;
    let comm_only = |root: &ProcRoot| root.add_process("4250", "hayamimi");
    assert_refused_for("t076b", &pids, comm_only, unreadable)?;
    let stat_only = |root: &ProcRoot| {
        std::fs::create_dir_all(root.path().join("4250"))?;
        root.set_stat(4250, "hayamimi", 10, 10)
    };
    assert_refused_for("t076c", &pids, stat_only, unreadable)
}

#[test]
fn a_set_missing_one_of_the_three_processes_stops_the_stream() -> TestResult {
    let set = ("set", None);
    assert_refused_for("t077a", &[4243, 4244], nothing_more, set)?;
    assert_refused_for("t077b", &[4242, 4244], nothing_more, set)?;
    assert_refused_for("t077c", &[4242, 4243], nothing_more, set)
}

#[test]
fn one_bad_pid_beside_a_complete_set_stops_the_stream() -> TestResult {
    let prepare = |root: &ProcRoot| root.add_server(4247, &["llama-server", "--port", "8080"]);
    assert_refused_for("t078", &with_one_more(4247), prepare, ("port", Some(4247)))
}

#[test]
fn a_non_numeric_own_pid_is_a_usage_error_not_a_refusal() -> TestResult {
    let seen = own_once("t079", &OWN_PIDS, nothing_more, &["--own-pid", "abc"])?;
    assert_eq!(seen.ended.code, Some(2), "{}", seen.ended.stderr);
    assert!(seen.ended.events(REFUSED).is_empty());
    assert_eq!(seen.audio, 0);
    Ok(())
}

/// The `cpu_some_total` of every row of a run of S83o.
fn cpu_totals(seen: &Observed) -> Result<Vec<Value>, Box<dyn Error>> {
    Ok(rows_of(seen)?
        .into_iter()
        .map(|mut row| row["cpu_some_total"].take())
        .collect())
}

#[test]
fn rows_carry_the_cpu_pressure_total_and_the_judgement_ignores_it() -> TestResult {
    let with = own_once("t080a", &OWN_PIDS, |root| root.set_cpu_pressure(777), &[])?;
    let totals = cpu_totals(&with)?;
    assert!(totals.len() >= 2 && totals.iter().all(|total| *total == json!(777)));
    let without = own_once("t080b", &OWN_PIDS, nothing_more, &[])?;
    let totals = cpu_totals(&without)?;
    assert!(totals.len() >= 2 && totals.iter().all(Value::is_null));
    let mut rows = o83_rows(370);
    for row in &mut rows {
        row["cpu_some_total"] = json!("x");
    }
    passes(&rs1("t080c", text_of(&rows))?);
    Ok(())
}

#[test]
fn a_comm_that_only_starts_like_ours_stops_the_stream() -> TestResult {
    let comm = ("comm", Some(4246));
    let foo = |root: &ProcRoot| root.add_own(4246, "hayamimi-foo", 10, 10);
    assert_refused_for("t081a", &with_one_more(4246), foo, comm)?;
    let relay_two = |root: &ProcRoot| root.add_own(4246, "hayamimi-relay2", 10, 10);
    assert_refused_for("t081b", &with_one_more(4246), relay_two, comm)
}

#[test]
fn a_hayamimi_eval_pid_stops_the_stream_with_the_comm_fault() -> TestResult {
    let prepare = |root: &ProcRoot| root.add_own(4245, "hayamimi-eval", 10, 10);
    assert_refused_for("t082", &with_one_more(4245), prepare, ("comm", Some(4245)))
}

#[test]
fn a_port_flag_joined_with_equals_stops_the_stream() -> TestResult {
    let words = &["llama-server", "--port=18084"];
    let port = ("port", Some(4244));
    assert_refused_for("t083", &OWN_PIDS, server_started_with(words), port)
}

#[test]
fn two_port_flags_stop_the_stream() -> TestResult {
    let port = ("port", Some(4244));
    let twice = &["llama-server", "--port", "18084", "--port", "18084"];
    assert_refused_for("t084a", &OWN_PIDS, server_started_with(twice), port)?;
    let last = &["llama-server", "--port", "8080", "--port", "18084"];
    assert_refused_for("t084b", &OWN_PIDS, server_started_with(last), port)
}

/// A refused run of S83o with this `loadavg`, one row in its record, and the
/// event's `load1`, `load15` and `cargo`.
fn refused_with_load(
    name: &str,
    loadavg: &'static str,
    prepare: Prepare,
    seen: (&str, &str, u64),
) -> TestResult {
    let after = move |root: &ProcRoot| {
        root.set_loadavg(loadavg)?;
        prepare(root)
    };
    let observed = own_once(name, &with_one_more(4246), after, &[])?;
    assert_own_refused(&observed, "comm", Some(4246));
    let event = &observed.ended.events(REFUSED)[0];
    assert_eq!(
        (&event[KEY_LOAD1], &event[KEY_LOAD15], &event[KEY_CARGO]),
        (&json!(seen.0), &json!(seen.1), &json!(seen.2))
    );
    assert_eq!(rows_of(&observed)?.len(), 1);
    Ok(())
}

fn sudo_beside_the_run(root: &ProcRoot) -> Result<(), Box<dyn Error>> {
    root.add_own(4246, "sudo", 10, 10)
}

fn sudo_and_a_build(root: &ProcRoot) -> Result<(), Box<dyn Error>> {
    root.add_process("4260", "cargo")?;
    sudo_beside_the_run(root)
}

#[test]
fn the_refusal_reason_is_own_pid_when_load_fails_at_the_same_time() -> TestResult {
    refused_with_load(
        "t085a",
        "2.00 0.50 0.50 1/300 1000",
        sudo_beside_the_run,
        ("2.00", "0.50", 0),
    )?;
    refused_with_load(
        "t085b",
        "0.50 0.50 2.50 1/300 1000",
        sudo_beside_the_run,
        ("0.50", "2.50", 0),
    )?;
    refused_with_load(
        "t085c",
        CLEAN_LOADAVG,
        sudo_and_a_build,
        ("0.50", "0.50", 1),
    )?;
    // Without a fault of the pids the reason is `load1`, and no own field is named.
    let busy = |root: &ProcRoot| root.set_loadavg("2.00 0.50 0.50 1/300 1000");
    let seen = own_once("t085d", &OWN_PIDS, busy, &[])?;
    let events = seen.ended.events(REFUSED);
    assert_eq!(events.len(), 1, "{}", seen.ended.stderr);
    assert_eq!(events[0][KEY_REASON], "load1");
    assert!(events[0].get(KEY_OWN_FAULT).is_none() && events[0].get(KEY_OWN_PID).is_none());
    assert_eq!(seen.ended.code, Some(2));
    Ok(())
}

#[test]
fn the_first_failing_pid_in_the_given_order_names_the_fault() -> TestResult {
    let prepare = |root: &ProcRoot| root.add_own(4246, "sudo", 10, 10);
    let sudo_first = [4246, 4250, 4242, 4243, 4244];
    assert_refused_for("t086a", &sudo_first, prepare, ("comm", Some(4246)))?;
    let prepare = |root: &ProcRoot| root.add_own(4246, "sudo", 10, 10);
    let missing_first = [4250, 4246, 4242, 4243, 4244];
    assert_refused_for("t086b", &missing_first, prepare, ("unreadable", Some(4250)))
}

#[test]
fn a_stream_without_own_pid_starts_and_writes_rows_without_own_fields() -> TestResult {
    let setup = Setup::new("t087", 1)?;
    setup.proc_root.set_cpu_pressure(777)?;
    let ingest = FakeIngest::start()?;
    let ended = run(&setup, ingest.port, 1, &[])?;
    let audio = ingest.finish()?;
    assert!(!matches!(ended.code, Some(2 | 3)), "{}", ended.stderr);
    assert!(ended.events(REFUSED).is_empty());
    assert!(audio >= 1, "no audio was sent");
    let Some(text) = setup.machine_text() else {
        return Err("the run left no machine record".into());
    };
    for line in text.lines() {
        let row: Value = serde_json::from_str(line)?;
        for key in ["own", "clk_tck", "own_cpu_ticks", "cpu_some_total"] {
            assert!(row.get(key).is_none(), "{key} in {row}");
        }
    }
    passes(&rs1("t087", text)?);
    Ok(())
}

/// A 12 second run of S83o in which the `stat` of `gone` is taken away as
/// soon as audio flows. The run goes on, the rows from the next one on are
/// `null`, and the judgement names the first of them.
fn run_while_a_stat_vanishes(test: &str, gone: u32) -> TestResult {
    let setup = Setup::new_own(test, 12)?;
    let ingest = FakeIngest::start()?;
    let named = own_args(&OWN_PIDS);
    let arguments: Vec<&str> = named.iter().map(String::as_str).collect();
    let child = setup.spawn(ingest.port, 1, &arguments)?;
    let waited = Instant::now();
    while ingest.audio_bytes() == 0 {
        assert!(
            waited.elapsed() < Duration::from_secs(30),
            "no audio was sent"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    setup.proc_root.remove(gone, "stat")?;
    let ended = wait_for(child, Instant::now())?;
    ingest.finish()?;
    assert!(!matches!(ended.code, Some(2 | 3)), "{}", ended.stderr);
    assert!(ended.events(REFUSED).is_empty() && ended.events(ABORTED).is_empty());
    let Some(text) = setup.machine_text() else {
        return Err("the run left no machine record".into());
    };
    let rows = text
        .lines()
        .map(serde_json::from_str::<Value>)
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(rows[0]["own_cpu_ticks"], json!(OWN_TICKS));
    let Some(first_null) = rows.iter().position(|row| row["own_cpu_ticks"].is_null()) else {
        return Err("no row has a null own_cpu_ticks".into());
    };
    assert!(first_null >= 1, "{rows:?}");
    assert!(
        rows[first_null..]
            .iter()
            .all(|row| row["own_cpu_ticks"].is_null())
    );
    let Some(dir) = setup.machine_file().parent().map(Path::to_path_buf) else {
        return Err("the record has no directory".into());
    };
    let result = judged(&[RunInput::rust(&dir, "s83", Some(text))]);
    assert_names(
        &result,
        &["s83", &format!("row {}", first_null + 1), "own_cpu_ticks"],
        &[],
    );
    Ok(())
}

#[test]
fn a_pid_whose_stat_vanishes_mid_run_writes_a_null_row_and_does_not_stop_the_stream() -> TestResult
{
    run_while_a_stat_vanishes("t088a", OWN_PIDS[0])?;
    run_while_a_stat_vanishes("t088b", OWN_PIDS[2])
}

#[test]
fn two_pids_of_the_same_process_are_a_set_fault() -> TestResult {
    let set = ("set", None);
    let second_hayamimi = |root: &ProcRoot| root.add_own(4248, "hayamimi", 10, 10);
    assert_refused_for("t090a", &with_one_more(4248), second_hayamimi, set)?;
    let second_server = |root: &ProcRoot| root.add_server(4249, &SERVER_WORDS);
    assert_refused_for("t090b", &with_one_more(4249), second_server, set)?;
    let swapped = |root: &ProcRoot| root.add_own(4248, "hayamimi", 10, 10);
    assert_refused_for("t090c", &[4242, 4244, 4248], swapped, set)
}

// ---- the acceptance (041) ----

/// 023 and 024, 009, 016, 020, 042, 045, 046, 056, 057 and 063 (a) handed to the
/// judgement in that order: pass, fail, fail, fail, fail, fail, pass, fail,
/// pass, fail, fail.
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
    passes(&o83_run("t041-056", 370)?);
    let named = [O83_LOAD5, "load5"];
    assert_names(&judged(&[o83_run("t041-057", 364)?]), &named, &[]);
    let old_form = recorded_run("t041-063", "e3p1.e2e2")?;
    assert_names(&judged(&[old_form]), &named, &[]);
    Ok(())
}

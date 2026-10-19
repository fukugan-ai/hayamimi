//! RP-71: the llama-server with speculative decoding (arm S3) answers T and E
//! faster than the one without (arm N0), and the draft is used.
//!
//! The acceptance test reads the records of a run and starts nothing.
//! `docs/results/r8w/` holds four runs in the order they are made, N0, S3, N0,
//! S3, as two files each:
//!
//! - `<arm>_<run>.lines.jsonl`: the relay's stdout, or a replay of its requests
//!   in the same shape. A line counts when its `event` is `translate.line.ok`
//!   or `translate.line.held`; `t_ms` is the time of T and `e_ms` the time of E
//!   (0 when E was not asked). Other events are skipped.
//! - `<arm>_<run>.server.log`: the log of the llama-server of that run.
//!
//! It passes when, for T and for E, the p50 of the two S3 runs pooled is below
//! the p50 of the two N0 runs pooled minus the gap between the p50 of the two
//! N0 runs; when the log of every S3 run reports accepted draft tokens, and
//! their sum is above 0; and when no log of an N0 run has such a line.
//!
//! The other parts of the RP-71 row (the count of differing translations, the
//! lines V held, the chrF, the memory) need the output of `eval` and are not
//! read here.
//!
//!     cargo test --release -p hayamimi-relay --test rp71 -- --ignored --nocapture
//!
//! The other tests run on fixtures in this file: the percentile, the reading
//! of the lines and of the log, both ends of each verdict.

use std::fs;
use std::path::Path;

use serde_json::Value;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Where the records of the run are kept.
const RECORDS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/results/r8w");

/// The runs in the order they are made.
const RUNS: [&str; 4] = ["n0_1", "s3_1", "n0_2", "s3_2"];

/// The events of a settled or held line of the relay.
const LINE_EVENTS: [&str; 2] = ["translate.line.ok", "translate.line.held"];

/// One line of the relay: the time T took and the time E took (0: not asked).
struct Line {
    t_ms: u64,
    e_ms: u64,
}

/// What the log of a server says about the draft: how many lines report it and
/// how many tokens the main model took from the draft in all of them.
struct Acceptance {
    reports: usize,
    accepted: u64,
}

struct Run {
    name: String,
    lines: Vec<Line>,
    acceptance: Acceptance,
}

#[derive(Clone, Copy)]
enum Stage {
    T,
    E,
}

impl Stage {
    fn label(self) -> &'static str {
        match self {
            Stage::T => "T",
            Stage::E => "E",
        }
    }

    /// The time this stage took on `line`; E has none when it was not asked.
    fn time_of(self, line: &Line) -> Option<u64> {
        match self {
            Stage::T => Some(line.t_ms),
            Stage::E => (line.e_ms > 0).then_some(line.e_ms),
        }
    }
}

/// The verdict of one check: whether it holds, and the line that says why.
struct Judged {
    passed: bool,
    text: String,
}

/// Nearest-rank percentile (3.1); `None` when there is no sample.
fn percentile(values: &[u64], percent: usize) -> Option<u64> {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let rank = (sorted.len() * percent).div_ceil(100);
    sorted.get(rank.checked_sub(1)?).copied()
}

fn number_of(event: &Value, key: &str, at: usize) -> Result<u64, String> {
    event[key]
        .as_u64()
        .ok_or_else(|| format!("line {at}: `{key}` is missing or not a count"))
}

/// The relay lines among the events of a stdout, in order.
fn lines_of(text: &str) -> Result<Vec<Line>, String> {
    let mut lines = Vec::new();
    for (index, raw) in text
        .lines()
        .enumerate()
        .filter(|(_, raw)| !raw.trim().is_empty())
    {
        let at = index + 1;
        let event: Value =
            serde_json::from_str(raw).map_err(|error| format!("line {at}: {error}"))?;
        if !event["event"]
            .as_str()
            .is_some_and(|name| LINE_EVENTS.contains(&name))
        {
            continue;
        }
        lines.push(Line {
            t_ms: number_of(&event, "t_ms", at)?,
            e_ms: number_of(&event, "e_ms", at)?,
        });
    }
    Ok(lines)
}

/// The tokens a log line says the main model took from the draft: the count
/// before `accepted` after `draft acceptance`, or the count after `#acc tokens =`.
/// `None` for a line that does not report it; `Err` for a line that does and
/// carries no count.
fn accepted_in(line: &str) -> Result<Option<u64>, String> {
    let after = match line.split_once("#acc tokens =") {
        Some((_, after)) => after,
        None => match line.split_once("draft acceptance") {
            Some((_, after)) => after.split_once('(').map_or(after, |(_, inside)| inside),
            None => return Ok(None),
        },
    };
    let digits: String = after
        .trim_start()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    let count = digits.parse().ok();
    count
        .map(Some)
        .ok_or_else(|| format!("a draft line without a count: {line}"))
}

fn acceptance_of(log: &str) -> Result<Acceptance, String> {
    let mut found = Acceptance {
        reports: 0,
        accepted: 0,
    };
    for line in log.lines() {
        if let Some(count) = accepted_in(line)? {
            found.reports += 1;
            found.accepted += count;
        }
    }
    Ok(found)
}

fn parse_run(name: &str, lines: &str, log: &str) -> Result<Run, String> {
    Ok(Run {
        name: name.to_owned(),
        lines: lines_of(lines).map_err(|error| format!("{name}.lines.jsonl: {error}"))?,
        acceptance: acceptance_of(log).map_err(|error| format!("{name}.server.log: {error}"))?,
    })
}

fn read_run(dir: &Path, name: &str) -> Result<Run, String> {
    let read = |suffix: &str| {
        let path = dir.join(format!("{name}.{suffix}"));
        fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))
    };
    parse_run(name, &read("lines.jsonl")?, &read("server.log")?)
}

fn times_of(run: &Run, stage: Stage) -> Vec<u64> {
    run.lines
        .iter()
        .filter_map(|line| stage.time_of(line))
        .collect()
}

/// p50 and p95 of `values`, or the reason there are none.
fn p50_p95(stage: Stage, what: &str, values: &[u64]) -> Result<(u64, u64), String> {
    match (percentile(values, 50), percentile(values, 95)) {
        (Some(p50), Some(p95)) => Ok((p50, p95)),
        _ => Err(format!("{}: {what} has no sample", stage.label())),
    }
}

fn pooled(runs: [&Run; 2], stage: Stage) -> Vec<u64> {
    runs.iter().flat_map(|run| times_of(run, stage)).collect()
}

/// One stage: the S3 p50 must be below the N0 p50 less the gap between the
/// p50 of the two N0 runs. The p50 and p95 of every group are in the text.
fn judge_stage(stage: Stage, n0: [&Run; 2], s3: [&Run; 2]) -> Result<Judged, String> {
    let (first, _) = p50_p95(stage, &n0[0].name, &times_of(n0[0], stage))?;
    let (second, _) = p50_p95(stage, &n0[1].name, &times_of(n0[1], stage))?;
    let (n0_p50, n0_p95) = p50_p95(stage, "N0", &pooled(n0, stage))?;
    let (s3_p50, s3_p95) = p50_p95(stage, "S3", &pooled(s3, stage))?;
    let gap = first.abs_diff(second);
    let limit = n0_p50.saturating_sub(gap);
    let passed = s3_p50 < limit;
    let text = format!(
        "{label}: N0 p50 {n0_p50} p95 {n0_p95} ms (run p50 {first}, {second}; gap {gap}), \
         S3 p50 {s3_p50} p95 {s3_p95} ms; S3 p50 must be below {limit}: {verdict}",
        label = stage.label(),
        verdict = if passed { "holds" } else { "FAILS" },
    );
    Ok(Judged { passed, text })
}

/// What the log of `run` must say: draft tokens accepted (S3), or no draft line (N0).
fn judge_acceptance(run: &Run, draft_expected: bool) -> Judged {
    let Acceptance { reports, accepted } = run.acceptance;
    let (passed, wants) = if draft_expected {
        (
            reports > 0 && accepted > 0,
            "draft lines with accepted tokens above 0",
        )
    } else {
        (reports == 0, "no draft line")
    };
    Judged {
        passed,
        text: format!(
            "{}: {reports} draft lines, {accepted} accepted tokens; wants {wants}: {}",
            run.name,
            if passed { "holds" } else { "FAILS" }
        ),
    }
}

struct Report {
    lines: Vec<String>,
    failures: Vec<String>,
}

impl Report {
    fn note(&mut self, judged: Judged) {
        if !judged.passed {
            self.failures.push(judged.text.clone());
        }
        self.lines.push(judged.text);
    }

    fn note_stage(&mut self, verdict: Result<Judged, String>) {
        match verdict {
            Ok(judged) => self.note(judged),
            Err(reason) => {
                self.failures.push(reason.clone());
                self.lines.push(reason);
            }
        }
    }

    fn render(&self) -> String {
        let mut text = self.lines.join("\n");
        for failure in &self.failures {
            text.push_str(&format!("\nfailure: {failure}"));
        }
        text
    }
}

/// The verdict of the four runs, given in the order of [`RUNS`].
fn judge(runs: &[Run; 4]) -> Report {
    let [first_n0, first_s3, second_n0, second_s3] = runs;
    let (n0, s3) = ([first_n0, second_n0], [first_s3, second_s3]);
    let mut report = Report {
        lines: Vec::new(),
        failures: Vec::new(),
    };
    for stage in [Stage::T, Stage::E] {
        report.note_stage(judge_stage(stage, n0, s3));
    }
    for run in s3 {
        report.note(judge_acceptance(run, true));
    }
    for run in n0 {
        report.note(judge_acceptance(run, false));
    }
    report
}

fn read_runs(dir: &Path) -> Result<[Run; 4], String> {
    Ok([
        read_run(dir, RUNS[0])?,
        read_run(dir, RUNS[1])?,
        read_run(dir, RUNS[2])?,
        read_run(dir, RUNS[3])?,
    ])
}

#[test]
#[ignore = "reads the records in docs/results/r8w/"]
fn rp71_s3_answers_faster_than_n0_and_the_draft_is_used() -> TestResult {
    let report = judge(&read_runs(Path::new(RECORDS))?);
    println!("{}", report.render());
    assert!(report.failures.is_empty(), "{}", report.render());
    Ok(())
}

// ---- fixtures ---------------------------------------------------------------

/// A relay stdout with one line per `(t_ms, e_ms)` and an event that is not a line.
fn stdout_of(times: &[(u64, u64)]) -> String {
    let mut text = String::from("{\"event\":\"relay.serve.started\",\"port\":8835}\n");
    for (t_ms, e_ms) in times {
        text.push_str(&format!(
            "{{\"event\":\"translate.line.ok\",\"id\":\"mic-1\",\"t_ms\":{t_ms},\"e_ms\":{e_ms}}}\n"
        ));
    }
    text
}

/// A server log with a draft line per accepted count.
fn log_with_drafts(accepted: &[u64]) -> String {
    let mut text = String::from("slot launch_slot_: id 3 | task 0 | processing task\n");
    for count in accepted {
        text.push_str(&format!(
            "slot print_timing: id  3 | task 3 | draft acceptance = 0.47222 (   {count} accepted /    36 generated), mean len =  2.42\n"
        ));
    }
    text
}

fn run_of(name: &str, times: &[(u64, u64)], accepted: &[u64]) -> Result<Run, String> {
    parse_run(name, &stdout_of(times), &log_with_drafts(accepted))
}

/// N0 takes T 200 ms and E 400 ms in one run and T 210 ms and E 410 ms in the
/// other; S3 takes `s3_t` and `s3_e`.
fn four_runs(s3_t: u64, s3_e: u64, s3_accepted: &[u64]) -> Result<[Run; 4], String> {
    Ok([
        run_of("n0_1", &[(200, 400); 5], &[])?,
        run_of("s3_1", &[(s3_t, s3_e); 5], s3_accepted)?,
        run_of("n0_2", &[(210, 410); 5], &[])?,
        run_of("s3_2", &[(s3_t, s3_e); 5], s3_accepted)?,
    ])
}

// ---- tests ------------------------------------------------------------------

#[test]
fn a_percentile_of_no_sample_is_none() {
    assert_eq!(percentile(&[], 50), None);
    assert_eq!(percentile(&[], 95), None);
}

#[test]
fn a_percentile_is_the_nearest_rank() {
    assert_eq!(percentile(&[7], 95), Some(7));
    assert_eq!(percentile(&[40, 10, 30, 20], 50), Some(20));
    let twenty: Vec<u64> = (1..=20).collect();
    assert_eq!(percentile(&twenty, 95), Some(19));
    assert_eq!(percentile(&twenty, 50), Some(10));
}

#[test]
fn the_lines_of_the_relay_are_read_and_other_events_are_skipped() -> TestResult {
    let lines = lines_of(&stdout_of(&[(120, 0), (90, 300)]))?;
    let read: Vec<(u64, u64)> = lines.iter().map(|line| (line.t_ms, line.e_ms)).collect();
    assert_eq!(read, [(120, 0), (90, 300)]);
    assert!(lines_of("")?.is_empty());
    Ok(())
}

#[test]
fn a_held_line_counts_as_a_line() -> TestResult {
    let lines = lines_of("{\"event\":\"translate.line.held\",\"t_ms\":50,\"e_ms\":0}")?;
    assert_eq!(lines.len(), 1);
    Ok(())
}

#[test]
fn a_line_without_t_ms_is_an_error_that_names_the_line() {
    let said = lines_of("{\"event\":\"translate.line.ok\",\"e_ms\":0}");
    assert!(matches!(said, Err(text) if text.contains("line 1") && text.contains("t_ms")));
    assert!(lines_of("not json").is_err());
}

#[test]
fn an_e_time_of_zero_is_not_an_e_sample() -> TestResult {
    let run = run_of("n0_1", &[(100, 0), (100, 250)], &[])?;
    assert_eq!(times_of(&run, Stage::E), [250]);
    assert_eq!(times_of(&run, Stage::T), [100, 100]);
    Ok(())
}

#[test]
fn both_forms_of_the_acceptance_line_are_read() -> TestResult {
    let sum = |text: &str| acceptance_of(text).map(|found| (found.reports, found.accepted));
    let rate = "draft acceptance = 0.57576 (  171 accepted /   297 generated), mean len = 2.5";
    assert_eq!(sum(rate)?, (1, 171));
    assert_eq!(
        sum("statistics draft: #acc tokens = 98, #gen tokens = 120")?,
        (1, 98)
    );
    assert_eq!(sum("slot launch_slot_: processing task")?, (0, 0));
    assert_eq!(sum(&format!("{rate}\n{rate}\n"))?, (2, 342));
    Ok(())
}

#[test]
fn an_acceptance_line_without_a_count_is_an_error() {
    assert!(acceptance_of("draft acceptance = n/a").is_err());
    assert!(acceptance_of("#acc tokens =").is_err());
}

#[test]
fn an_s3_that_is_faster_with_the_draft_used_passes() -> TestResult {
    let report = judge(&four_runs(150, 300, &[40, 41])?);
    assert!(report.failures.is_empty(), "{}", report.render());
    Ok(())
}

/// The pooled N0 p50 is 200 ms (the fifth of ten values) and the gap between the
/// two N0 runs is 10: S3 must be below 190.
#[test]
fn an_s3_p50_at_the_limit_fails_and_one_below_it_passes() -> TestResult {
    let at_limit = judge(&four_runs(190, 300, &[40])?);
    assert_eq!(at_limit.failures.len(), 1, "{}", at_limit.render());
    assert!(
        at_limit.failures[0].starts_with("T:"),
        "{}",
        at_limit.render()
    );
    let below = judge(&four_runs(189, 300, &[40])?);
    assert!(below.failures.is_empty(), "{}", below.render());
    Ok(())
}

#[test]
fn an_s3_that_is_slower_fails_on_t_and_on_e() -> TestResult {
    let report = judge(&four_runs(290, 700, &[40])?);
    assert_eq!(report.failures.len(), 2, "{}", report.render());
    assert!(report.failures[0].starts_with("T:") && report.failures[1].starts_with("E:"));
    Ok(())
}

#[test]
fn a_stage_nobody_recorded_is_a_failure_and_not_a_pass() -> TestResult {
    let runs = [
        run_of("n0_1", &[(200, 0); 5], &[])?,
        run_of("s3_1", &[(150, 0); 5], &[40])?,
        run_of("n0_2", &[(210, 0); 5], &[])?,
        run_of("s3_2", &[(150, 0); 5], &[40])?,
    ];
    let report = judge(&runs);
    assert_eq!(report.failures.len(), 1, "{}", report.render());
    assert!(report.failures[0].contains("E:") && report.failures[0].contains("no sample"));
    Ok(())
}

#[test]
fn an_s3_log_that_took_no_draft_token_fails() -> TestResult {
    let none_accepted = judge(&four_runs(150, 300, &[0, 0])?);
    assert_eq!(
        none_accepted.failures.len(),
        2,
        "{}",
        none_accepted.render()
    );
    let no_line = judge(&four_runs(150, 300, &[])?);
    assert_eq!(no_line.failures.len(), 2, "{}", no_line.render());
    assert!(no_line.failures[0].starts_with("s3_1:"));
    Ok(())
}

#[test]
fn an_n0_log_with_a_draft_line_fails() -> TestResult {
    let runs = [
        run_of("n0_1", &[(200, 400); 5], &[3])?,
        run_of("s3_1", &[(150, 300); 5], &[40])?,
        run_of("n0_2", &[(210, 410); 5], &[])?,
        run_of("s3_2", &[(150, 300); 5], &[40])?,
    ];
    let report = judge(&runs);
    assert_eq!(report.failures.len(), 1, "{}", report.render());
    assert!(report.failures[0].starts_with("n0_1:"));
    Ok(())
}

#[test]
fn a_missing_record_names_its_path() {
    let said = read_run(Path::new(RECORDS), "no_such_run").err();
    assert!(said.is_some_and(|text| text.contains("no_such_run.lines.jsonl")));
}

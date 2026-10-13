//! CX-10: the daily fetch is scheduled, runs, and is removed, and the
//! production task is never touched.
//!
//! Every test uses its own task named `hayamimi-fx-fetch-test-<pid>-<n>`
//! and removes it when it ends, even when it fails. The production task is
//! only ever read with `/Query /XML`.
//!
//! Tests 002 to 008 need the Windows Task Scheduler and are ignored by
//! default: `cargo test --release --test cx10 -- --ignored` on Windows.

mod support;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use hayamimi_relay::calendar::parse_rfc3339;
use hayamimi_relay::fx::Rates;
use hayamimi_relay::schedule::DEFAULT_TASK_NAME;
use support::{FakeServer, Reply, TempDir, TestResult, ecb_xml, rates_json};

type Failure = Box<dyn std::error::Error>;

const TEST_PREFIX: &str = "hayamimi-fx-fetch-test-";
const PATH: &str = "/stats/eurofxref/eurofxref-daily.xml";
const POLL_LIMIT: Duration = Duration::from_secs(60);

static NEXT_NAME: AtomicU32 = AtomicU32::new(1);

/// What the name guard refuses.
#[derive(Debug, PartialEq)]
enum NameFault {
    NotATestName,
}

/// Only names under the test prefix, with something after it, may be used.
fn check_task_name_is_test(name: &str) -> Result<(), NameFault> {
    match name.strip_prefix(TEST_PREFIX) {
        Some(rest) if !rest.is_empty() && name != DEFAULT_TASK_NAME => Ok(()),
        _ => Err(NameFault::NotATestName),
    }
}

fn require_windows() -> TestResult {
    if cfg!(windows) {
        Ok(())
    } else {
        Err("this test needs the Windows Task Scheduler".into())
    }
}

/// The relay binary next to the test binary's `deps` directory.
fn relay_exe() -> Result<PathBuf, Failure> {
    let test_exe = std::env::current_exe()?;
    let dir = test_exe
        .parent()
        .and_then(Path::parent)
        .ok_or("the test binary has no grandparent directory")?;
    let file = if cfg!(windows) {
        "hayamimi-relay.exe"
    } else {
        "hayamimi-relay"
    };
    Ok(dir.join(file))
}

fn run(program: &Path, args: &[&str]) -> Result<Output, Failure> {
    Ok(Command::new(program).args(args).output()?)
}

fn schtasks(args: &[&str]) -> Result<Output, Failure> {
    run(Path::new("schtasks"), args)
}

/// Task Scheduler writes the XML in the console's encoding; NUL bytes of a
/// wide encoding are dropped.
fn decode_xml(bytes: &[u8]) -> String {
    let narrow: Vec<u8> = bytes.iter().copied().filter(|byte| *byte != 0).collect();
    String::from_utf8_lossy(&narrow).into_owned()
}

/// The text between `<tag>` and `</tag>`.
fn tag_text<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)? + start;
    Some(xml[start..end].trim())
}

/// Exit code, standard output and standard error of a query.
type Snapshot = (Option<i32>, Vec<u8>, Vec<u8>);

/// What the production task looks like to a read-only query.
fn production_snapshot() -> Result<Snapshot, Failure> {
    let output = schtasks(&["/Query", "/TN", DEFAULT_TASK_NAME, "/XML"])?;
    Ok((output.status.code(), output.stdout, output.stderr))
}

/// Removes the test task when the test ends, however it ends.
struct TaskGuard {
    name: String,
    exe: PathBuf,
}

impl Drop for TaskGuard {
    fn drop(&mut self) {
        if check_task_name_is_test(&self.name).is_err() {
            return;
        }
        let removed = run(
            &self.exe,
            &["fx-schedule", "--remove", "--task-name", &self.name],
        );
        if removed.is_err() {
            eprintln!("could not run the removal of {}", self.name);
        }
    }
}

/// A test task, a local server for it to fetch from, and a rate file that is
/// older than anything the fetch will write.
struct Scenario {
    guard: TaskGuard,
    server: FakeServer,
    out: PathBuf,
    _dir: TempDir,
}

fn scenario() -> Result<Scenario, Failure> {
    require_windows()?;
    let name = format!(
        "{TEST_PREFIX}{}-{}",
        std::process::id(),
        NEXT_NAME.fetch_add(1, Ordering::SeqCst)
    );
    check_task_name_is_test(&name).map_err(|fault| format!("{fault:?}: {name}"))?;
    let xml = ecb_xml("2026-10-12", "1.1181", "177.33");
    let server = FakeServer::start(move |_, _| Reply::xml(&xml))?;
    let dir = TempDir::new("cx10")?;
    let out = dir.join("fx_rates.json");
    std::fs::write(
        &out,
        rates_json(
            "2026-09-30",
            "2026-10-01T07:30:00+09:00",
            "1.1000",
            "176.00",
        ),
    )?;
    let guard = TaskGuard {
        name,
        exe: relay_exe()?,
    };
    Ok(Scenario {
        guard,
        server,
        out,
        _dir: dir,
    })
}

impl Scenario {
    fn name(&self) -> &str {
        &self.guard.name
    }

    fn install(&self) -> Result<Output, Failure> {
        let out = self.out.to_string_lossy().into_owned();
        let url = self.server.url(PATH);
        run(
            &self.guard.exe,
            &[
                "fx-schedule",
                "--install",
                "--task-name",
                self.name(),
                "--out",
                &out,
                "--source-url",
                &url,
            ],
        )
    }

    fn remove(&self) -> Result<Output, Failure> {
        run(
            &self.guard.exe,
            &["fx-schedule", "--remove", "--task-name", self.name()],
        )
    }

    fn query(&self) -> Result<Output, Failure> {
        schtasks(&["/Query", "/TN", self.name()])
    }

    fn query_xml(&self) -> Result<String, Failure> {
        let output = schtasks(&["/Query", "/TN", self.name(), "/XML"])?;
        assert_eq!(output.status.code(), Some(0), "schtasks /Query /XML");
        Ok(decode_xml(&output.stdout))
    }

    fn fetched_at(&self) -> Result<i64, Failure> {
        let rates = Rates::parse(&std::fs::read(&self.out)?)?;
        parse_rfc3339(&rates.fetched_at)
            .ok_or_else(|| format!("bad fetched_at {}", rates.fetched_at).into())
    }

    /// Runs the task and waits until the file's `fetched_at` moves forward.
    fn run_and_wait(&self) -> Result<(), Failure> {
        let before = self.fetched_at()?;
        let started = schtasks(&["/Run", "/TN", self.name()])?;
        assert_eq!(started.status.code(), Some(0), "schtasks /Run");
        let deadline = Instant::now() + POLL_LIMIT;
        while Instant::now() < deadline {
            if self.fetched_at()? > before {
                return Ok(());
            }
            std::thread::sleep(Duration::from_secs(1));
        }
        Err(format!("fetched_at did not move within {POLL_LIMIT:?}").into())
    }
}

/// T-CX-10-001
#[test]
fn cx10_test_task_is_scheduled_runs_and_removes_name_guard_refuses_the_production_name() {
    assert_eq!(
        check_task_name_is_test("hayamimi-fx-fetch-test-1"),
        Ok(()),
        "T-CX-10-001: test name"
    );
    assert_eq!(
        check_task_name_is_test("hayamimi-fx-fetch"),
        Err(NameFault::NotATestName),
        "T-CX-10-001: production name"
    );
    assert_eq!(
        check_task_name_is_test("hayamimi-fx-fetch-test-"),
        Err(NameFault::NotATestName)
    );
    assert_eq!(check_task_name_is_test(""), Err(NameFault::NotATestName));
}

/// T-CX-10-002
#[test]
#[ignore = "needs the Windows Task Scheduler"]
fn cx10_test_task_is_scheduled_runs_and_removes_install_registers_the_test_task() -> TestResult {
    let task = scenario()?;
    assert_eq!(
        task.install()?.status.code(),
        Some(0),
        "T-CX-10-002: install"
    );
    assert_eq!(task.query()?.status.code(), Some(0), "T-CX-10-002: query");
    Ok(())
}

/// T-CX-10-003
#[test]
#[ignore = "needs the Windows Task Scheduler"]
fn cx10_test_task_is_scheduled_runs_and_removes_xml_has_a_daily_0730_trigger() -> TestResult {
    let task = scenario()?;
    assert_eq!(
        task.install()?.status.code(),
        Some(0),
        "T-CX-10-003: install"
    );
    let xml = task.query_xml()?;
    assert!(
        xml.contains("<ScheduleByDay>"),
        "T-CX-10-003: ScheduleByDay in {xml}"
    );
    assert_eq!(
        tag_text(&xml, "DaysInterval"),
        Some("1"),
        "T-CX-10-003: DaysInterval"
    );
    let boundary = tag_text(&xml, "StartBoundary").ok_or("no StartBoundary")?;
    let time = boundary
        .split_once('T')
        .map(|(_, time)| time)
        .ok_or("StartBoundary has no time part")?;
    assert!(
        time.starts_with("07:30:00"),
        "T-CX-10-003: StartBoundary {boundary}"
    );
    Ok(())
}

/// T-CX-10-004
#[test]
#[ignore = "needs the Windows Task Scheduler"]
fn cx10_test_task_is_scheduled_runs_and_removes_xml_has_start_when_available_true() -> TestResult {
    let task = scenario()?;
    assert_eq!(
        task.install()?.status.code(),
        Some(0),
        "T-CX-10-004: install"
    );
    let xml = task.query_xml()?;
    assert_eq!(
        tag_text(&xml, "StartWhenAvailable"),
        Some("true"),
        "T-CX-10-004: {xml}"
    );
    Ok(())
}

/// T-CX-10-005
#[test]
#[ignore = "needs the Windows Task Scheduler"]
fn cx10_test_task_is_scheduled_runs_and_removes_run_refreshes_fetched_at() -> TestResult {
    let task = scenario()?;
    assert_eq!(
        task.install()?.status.code(),
        Some(0),
        "T-CX-10-005: install"
    );
    task.run_and_wait()
}

/// T-CX-10-006
#[test]
#[ignore = "needs the Windows Task Scheduler"]
fn cx10_test_task_is_scheduled_runs_and_removes_run_hit_the_test_server_not_the_network()
-> TestResult {
    let task = scenario()?;
    assert_eq!(
        task.install()?.status.code(),
        Some(0),
        "T-CX-10-006: install"
    );
    task.run_and_wait()?;
    assert!(
        !task.server.requests().is_empty(),
        "T-CX-10-006: the test server saw no request"
    );
    let rates = Rates::parse(&std::fs::read(&task.out)?)?;
    assert_eq!(
        rates.source_url,
        task.server.url(PATH),
        "T-CX-10-006: source_url"
    );
    assert_ne!(
        rates.source_url,
        hayamimi_relay::fx::DEFAULT_SOURCE_URL,
        "T-CX-10-006: the real ECB URL"
    );
    Ok(())
}

/// T-CX-10-007
#[test]
#[ignore = "needs the Windows Task Scheduler"]
fn cx10_test_task_is_scheduled_runs_and_removes_remove_deletes_the_test_task() -> TestResult {
    let task = scenario()?;
    assert_eq!(
        task.install()?.status.code(),
        Some(0),
        "T-CX-10-007: install"
    );
    assert_eq!(task.remove()?.status.code(), Some(0), "T-CX-10-007: remove");
    assert_eq!(
        task.query()?.status.code(),
        Some(1),
        "T-CX-10-007: query after remove"
    );
    Ok(())
}

/// T-CX-10-008
#[test]
#[ignore = "needs the Windows Task Scheduler"]
fn cx10_test_task_is_scheduled_runs_and_removes_production_task_query_output_is_unchanged()
-> TestResult {
    require_windows()?;
    let before = production_snapshot()?;
    let task = scenario()?;
    assert_eq!(
        task.install()?.status.code(),
        Some(0),
        "T-CX-10-008: install"
    );
    assert_eq!(task.remove()?.status.code(), Some(0), "T-CX-10-008: remove");
    let after = production_snapshot()?;
    assert_eq!(
        before, after,
        "T-CX-10-008: the production task query changed"
    );
    Ok(())
}

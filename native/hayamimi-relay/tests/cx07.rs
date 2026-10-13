//! CX-7: a failed fetch keeps the previous rate file.
//!
//! "Unchanged" is compared as identical bytes, which is stricter than the
//! digest the design names.

mod support;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use hayamimi_relay::annotate::SkipReason;
use hayamimi_relay::fx::{self, AtomicFileStore, FetchError, FetchOptions, RateStore, Rates};
use hayamimi_relay::lang::Lang;
use hayamimi_relay::logging::{Level, LogEvent, MemorySink};
use support::{FakeServer, Harness, Reply, TempDir, TestResult, ecb_xml, epoch, rates_json};

const AFTER_TRUNCATED: &str = include_str!("fixtures/cx07_after_truncated.json");
const NOT_XML: &str = include_str!("fixtures/cx07_not_xml.txt");
const TRUNCATED_XML: &str = include_str!("fixtures/cx07_truncated.xml");
const NO_TIME_XML: &str = include_str!("fixtures/cx07_no_time.xml");
const NO_USD_XML: &str = include_str!("fixtures/cx07_no_usd.xml");
const NO_JPY_XML: &str = include_str!("fixtures/cx07_no_jpy.xml");

const NOW: &str = "2026-10-13T07:30:00+09:00";
const PATH: &str = "/stats/eurofxref/eurofxref-daily.xml";

/// The file on disk before the fetch: older date, different rates.
fn previous_file() -> String {
    rates_json(
        "2026-10-09",
        "2026-10-12T07:30:00+09:00",
        "1.1000",
        "176.00",
    )
}

/// What a bytes check refuses.
#[derive(Debug)]
enum FileFault {
    Changed,
}

fn check_file_unchanged(before: &[u8], after: &[u8]) -> Result<(), FileFault> {
    if before == after {
        Ok(())
    } else {
        Err(FileFault::Changed)
    }
}

/// A store whose write dies half way through the temporary file.
struct HalfWriteStore;

impl RateStore for HalfWriteStore {
    fn replace(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        let mut name = path.as_os_str().to_os_string();
        name.push(".half");
        let half = PathBuf::from(name);
        fs::write(&half, &bytes[..bytes.len() / 2])?;
        Err(io::Error::other("the write stopped half way"))
    }
}

struct Run {
    _dir: TempDir,
    out: PathBuf,
    before: Option<Vec<u8>>,
    result: Result<Rates, FetchError>,
    sink: MemorySink,
}

impl Run {
    fn after(&self) -> io::Result<Vec<u8>> {
        fs::read(&self.out)
    }

    fn failures(&self) -> Vec<&LogEvent> {
        self.sink
            .events
            .iter()
            .filter(|event| event.event == "fx.fetch.failed")
            .collect()
    }
}

fn fetch_into_dir(
    url: &str,
    with_previous: bool,
    store: &dyn RateStore,
) -> Result<Run, Box<dyn std::error::Error>> {
    let dir = TempDir::new("cx07")?;
    let out = dir.join("fx_rates.json");
    let before = if with_previous {
        fs::write(&out, previous_file())?;
        Some(fs::read(&out)?)
    } else {
        None
    };
    let mut sink = MemorySink::default();
    let result = fx::fetch(
        &FetchOptions {
            source_url: url,
            out: &out,
        },
        epoch(NOW)?,
        store,
        &mut sink,
    );
    Ok(Run {
        _dir: dir,
        out,
        before,
        result,
        sink,
    })
}

fn fetch_reply(reply: Reply, with_previous: bool) -> Result<Run, Box<dyn std::error::Error>> {
    let mut reply = Some(reply);
    let server = FakeServer::start(move |_, _| match reply.take() {
        Some(first) => first,
        None => Reply::status(500),
    })?;
    fetch_into_dir(&server.url(PATH), with_previous, &AtomicFileStore)
}

fn fetch_status(code: u16) -> Result<Run, Box<dyn std::error::Error>> {
    fetch_reply(Reply::status(code), true)
}

fn fetch_body(body: &str) -> Result<Run, Box<dyn std::error::Error>> {
    fetch_reply(Reply::xml(body), true)
}

/// The previous bytes survive and exactly one error event was logged.
fn expect_kept(run: &Run, id: &str) -> TestResult {
    let before = run
        .before
        .as_deref()
        .ok_or("the run had no previous file")?;
    let after = run.after()?;
    assert!(
        check_file_unchanged(before, &after).is_ok(),
        "{id}: file changed"
    );
    assert!(run.result.is_err(), "{id}: the fetch reported success");
    let failures = run.failures();
    assert_eq!(failures.len(), 1, "{id}: fx.fetch.failed events");
    assert!(
        failures.iter().all(|event| event.level == Level::Error),
        "{id}: level"
    );
    Ok(())
}

/// T-CX-7-001
#[test]
fn cx07_failed_fetch_keeps_the_previous_file_ok_fetch_replaces_the_file() -> TestResult {
    let xml = ecb_xml("2026-10-12", "1.1181", "177.33");
    let run = fetch_reply(Reply::xml(&xml), true)?;
    let after = run.after()?;
    let rates = Rates::parse(&after)?;
    assert_eq!(rates.rate_date, "2026-10-12", "T-CX-7-001: rate_date");
    assert_eq!(rates.fetched_at, NOW, "T-CX-7-001: fetched_at");
    assert_eq!(rates.base, "EUR", "T-CX-7-001: base");
    assert_eq!(rates.rates.get("USD"), Some(&1.1181), "T-CX-7-001: USD");
    assert_eq!(rates.rates.get("JPY"), Some(&177.33), "T-CX-7-001: JPY");
    let before = run.before.as_deref().ok_or("no previous file")?;
    assert!(
        check_file_unchanged(before, &after).is_err(),
        "T-CX-7-001: bytes must differ"
    );
    assert!(run.failures().is_empty(), "T-CX-7-001: failure events");
    Ok(())
}

/// T-CX-7-002
#[test]
fn cx07_failed_fetch_keeps_the_previous_file_server_500_keeps_the_previous_bytes() -> TestResult {
    expect_kept(&fetch_status(500)?, "T-CX-7-002")
}

/// T-CX-7-003
#[test]
fn cx07_failed_fetch_keeps_the_previous_file_server_404_keeps_the_previous_bytes() -> TestResult {
    expect_kept(&fetch_status(404)?, "T-CX-7-003")
}

/// T-CX-7-004
#[test]
fn cx07_failed_fetch_keeps_the_previous_file_connection_refused_keeps_the_previous_bytes()
-> TestResult {
    // A listener that takes each connection and drops it unanswered refuses
    // the exchange at once, whether or not the host answers a closed port
    // with a reset.
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    let url = format!("http://{addr}{PATH}");
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stopped = std::sync::Arc::clone(&stop);
    let acceptor = std::thread::spawn(move || {
        while listener.accept().is_ok() {
            if stopped.load(std::sync::atomic::Ordering::SeqCst) {
                break;
            }
        }
    });
    let run = fetch_into_dir(&url, true, &AtomicFileStore)?;
    expect_kept(&run, "T-CX-7-004")?;
    assert!(
        matches!(run.result, Err(FetchError::Connect(_))),
        "T-CX-7-004: {:?}",
        run.result
    );
    // One more connection wakes the acceptor so that it sees the flag.
    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    let _ = std::net::TcpStream::connect(addr);
    let _ = acceptor.join();
    Ok(())
}

/// T-CX-7-005
#[test]
fn cx07_failed_fetch_keeps_the_previous_file_write_aborted_midway_keeps_the_previous_bytes()
-> TestResult {
    let xml = ecb_xml("2026-10-12", "1.1181", "177.33");
    let server = FakeServer::start(move |_, _| Reply::xml(&xml))?;
    let run = fetch_into_dir(&server.url(PATH), true, &HalfWriteStore)?;
    let before = run.before.as_deref().ok_or("no previous file")?;
    assert!(
        check_file_unchanged(before, &run.after()?).is_ok(),
        "T-CX-7-005: file changed"
    );
    assert!(
        matches!(run.result, Err(FetchError::Write(_))),
        "T-CX-7-005: {:?}",
        run.result
    );
    Ok(())
}

/// T-CX-7-006
#[test]
fn cx07_failed_fetch_keeps_the_previous_file_non_atomic_result_is_rejected_by_the_check() {
    let before = previous_file();
    let truncated = check_file_unchanged(before.as_bytes(), AFTER_TRUNCATED.as_bytes());
    assert!(
        matches!(truncated, Err(FileFault::Changed)),
        "T-CX-7-006: {truncated:?}"
    );
    assert!(
        check_file_unchanged(before.as_bytes(), before.as_bytes()).is_ok(),
        "T-CX-7-006: same bytes"
    );
}

/// T-CX-7-007
#[test]
fn cx07_failed_fetch_keeps_the_previous_file_ok_status_with_non_xml_body_keeps_the_previous_bytes()
-> TestResult {
    expect_kept(&fetch_body(NOT_XML)?, "T-CX-7-007")
}

/// T-CX-7-008
#[test]
fn cx07_failed_fetch_keeps_the_previous_file_truncated_xml_keeps_the_previous_bytes() -> TestResult
{
    expect_kept(&fetch_body(TRUNCATED_XML)?, "T-CX-7-008")
}

/// T-CX-7-009
#[test]
fn cx07_failed_fetch_keeps_the_previous_file_xml_without_time_keeps_the_previous_bytes()
-> TestResult {
    expect_kept(&fetch_body(NO_TIME_XML)?, "T-CX-7-009")
}

/// T-CX-7-010
#[test]
fn cx07_failed_fetch_keeps_the_previous_file_xml_without_usd_keeps_the_previous_bytes() -> TestResult
{
    expect_kept(&fetch_body(NO_USD_XML)?, "T-CX-7-010")
}

/// T-CX-7-011
#[test]
fn cx07_failed_fetch_keeps_the_previous_file_xml_without_jpy_keeps_the_previous_bytes() -> TestResult
{
    expect_kept(&fetch_body(NO_JPY_XML)?, "T-CX-7-011")
}

/// T-CX-7-012
#[test]
fn cx07_failed_fetch_keeps_the_previous_file_failure_without_a_previous_file_creates_no_file()
-> TestResult {
    let run = fetch_reply(Reply::status(500), false)?;
    assert!(!run.out.exists(), "T-CX-7-012: a file appeared");
    assert_eq!(
        run.failures().len(),
        1,
        "T-CX-7-012: fx.fetch.failed events"
    );
    Ok(())
}

/// T-CX-7-013
#[test]
fn cx07_failed_fetch_keeps_the_previous_file_failure_without_a_previous_file_gives_no_rate()
-> TestResult {
    let run = fetch_reply(Reply::status(500), false)?;
    let mut harness = Harness::open(Some(run.out.clone()), None)?;
    let line = harness.line(
        "I paid $100.",
        Lang::En,
        "私は100ドル払いました。",
        epoch(NOW)?,
    );
    assert!(line.annotations.is_empty(), "T-CX-7-013: annotations");
    assert!(
        line.skipped
            .iter()
            .any(|skipped| skipped.reason == SkipReason::NoRate),
        "T-CX-7-013: {:?}",
        line.skipped
    );
    Ok(())
}

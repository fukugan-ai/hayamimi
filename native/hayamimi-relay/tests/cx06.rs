//! CX-6: the rate fetch sends a bare GET and nothing from a conversation.

mod support;

use hayamimi_relay::fx::{self, AtomicFileStore, FetchOptions};
use hayamimi_relay::lang::Lang;
use hayamimi_relay::logging::NullSink;
use support::{FakeServer, Harness, Reply, TempDir, TestResult, ecb_xml, epoch};

const QUERY_FIXTURE: &str = include_str!("fixtures/cx06_request_with_subtitle_query.txt");
const PATH: &str = "/stats/eurofxref/eurofxref-daily.xml";

/// The subtitle strings in every shape they could take inside a request.
const SUBTITLE_SHAPES: [&str; 5] = [
    "こんにちは。",
    "%E3%81%93%E3%82%93%E3%81%AB%E3%81%A1%E3%81%AF%E3%80%82",
    "I paid $100.",
    "I%20paid%20%24100.",
    "I+paid+%24100.",
];

/// What a request check refuses.
#[derive(Debug)]
enum RequestFault {
    NotGet,
    Query,
    Body,
    Subtitle(usize),
}

fn head_and_body(raw: &[u8]) -> (&[u8], &[u8]) {
    if let Some(end) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
        return (&raw[..end], &raw[end + 4..]);
    }
    if let Some(end) = raw.windows(2).position(|window| window == b"\n\n") {
        return (&raw[..end], &raw[end + 2..]);
    }
    (raw, &[])
}

fn request_line(head: &[u8]) -> String {
    let Some(first) = head.split(|byte| *byte == b'\n').next() else {
        return String::new();
    };
    String::from_utf8_lossy(first)
        .trim_end_matches('\r')
        .to_owned()
}

/// A bare GET: method GET, no query, an empty body.
fn check_bare_get(raw: &[u8]) -> Result<(), RequestFault> {
    let (head, body) = head_and_body(raw);
    let line = request_line(head);
    let mut parts = line.split(' ');
    if parts.next() != Some("GET") {
        return Err(RequestFault::NotGet);
    }
    if parts.next().is_some_and(|target| target.contains('?')) {
        return Err(RequestFault::Query);
    }
    if !body.is_empty() {
        return Err(RequestFault::Body);
    }
    Ok(())
}

fn occurrences(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .filter(|window| *window == needle)
        .count()
}

fn subtitle_hits(raw: &[u8]) -> usize {
    SUBTITLE_SHAPES
        .iter()
        .map(|shape| occurrences(raw, shape.as_bytes()))
        .sum()
}

/// The request must not carry a subtitle in any shape.
fn check_no_subtitle_bytes(raw: &[u8]) -> Result<(), RequestFault> {
    match subtitle_hits(raw) {
        0 => Ok(()),
        found => Err(RequestFault::Subtitle(found)),
    }
}

/// Streams two lines, then fetches once from a local server; returns the
/// requests the server saw.
fn fetch_after_two_lines() -> Result<Vec<Vec<u8>>, Box<dyn std::error::Error>> {
    let now = epoch("2026-10-13T07:30:00+09:00")?;
    let mut harness = Harness::open(None, None)?;
    harness.line("こんにちは。", Lang::Ja, "Hello.", now);
    harness.line("I paid $100.", Lang::En, "私は100ドル払いました。", now);
    let xml = ecb_xml("2026-10-12", "1.1181", "177.33");
    let server = FakeServer::start(move |_, _| Reply::xml(&xml))?;
    let dir = TempDir::new("cx06")?;
    let out = dir.join("fx_rates.json");
    let url = server.url(PATH);
    fx::fetch(
        &FetchOptions {
            source_url: &url,
            out: &out,
        },
        now,
        &AtomicFileStore,
        &mut NullSink,
    )?;
    Ok(server.requests())
}

fn only_request(requests: &[Vec<u8>]) -> Result<&Vec<u8>, String> {
    match requests {
        [one] => Ok(one),
        other => Err(format!("expected one request, got {}", other.len())),
    }
}

/// T-CX-6-001
#[test]
fn cx06_rate_fetch_sends_a_bare_get_only_one_request_only() -> TestResult {
    let requests = fetch_after_two_lines()?;
    assert_eq!(requests.len(), 1, "T-CX-6-001");
    Ok(())
}

/// T-CX-6-002
#[test]
fn cx06_rate_fetch_sends_a_bare_get_only_method_get_and_path_matches() -> TestResult {
    let requests = fetch_after_two_lines()?;
    let raw = only_request(&requests)?;
    let (head, _) = head_and_body(raw);
    let line = request_line(head);
    let mut parts = line.split(' ');
    assert_eq!(parts.next(), Some("GET"), "T-CX-6-002: method");
    let target = parts.next().ok_or("no request target")?;
    assert_eq!(target, PATH, "T-CX-6-002: target");
    assert!(!target.contains('?'), "T-CX-6-002: query");
    Ok(())
}

/// T-CX-6-003
#[test]
fn cx06_rate_fetch_sends_a_bare_get_only_body_is_empty() -> TestResult {
    let requests = fetch_after_two_lines()?;
    let raw = only_request(&requests)?;
    let (head, body) = head_and_body(raw);
    assert!(body.is_empty(), "T-CX-6-003: body bytes {}", body.len());
    let head_text = String::from_utf8_lossy(head).to_lowercase();
    let length_line = head_text
        .lines()
        .find(|line| line.starts_with("content-length:"));
    assert!(
        length_line.is_none_or(|line| line.trim_start_matches("content-length:").trim() == "0"),
        "T-CX-6-003: {length_line:?}"
    );
    Ok(())
}

/// T-CX-6-004
#[test]
fn cx06_rate_fetch_sends_a_bare_get_only_subtitle_text_never_appears_in_request_bytes() -> TestResult
{
    let requests = fetch_after_two_lines()?;
    let raw = only_request(&requests)?;
    assert_eq!(subtitle_hits(raw), 0, "T-CX-6-004");
    assert_eq!(
        "こんにちは。".as_bytes(),
        [
            0xe3, 0x81, 0x93, 0xe3, 0x82, 0x93, 0xe3, 0x81, 0xab, 0xe3, 0x81, 0xa1, 0xe3, 0x81,
            0xaf, 0xe3, 0x80, 0x82
        ],
        "T-CX-6-004: the UTF-8 form searched for"
    );
    Ok(())
}

/// T-CX-6-005
#[test]
fn cx06_rate_fetch_sends_a_bare_get_only_default_source_url_is_the_ecb_daily_file() {
    assert_eq!(
        fx::DEFAULT_SOURCE_URL,
        "https://www.ecb.europa.eu/stats/eurofxref/eurofxref-daily.xml",
        "T-CX-6-005"
    );
}

/// T-CX-6-006
#[test]
fn cx06_rate_fetch_sends_a_bare_get_only_query_with_subtitle_is_rejected_by_the_check() -> TestResult
{
    let bare = check_bare_get(QUERY_FIXTURE.as_bytes());
    assert!(
        matches!(bare, Err(RequestFault::Query)),
        "T-CX-6-006: bare {bare:?}"
    );
    let subtitle = check_no_subtitle_bytes(QUERY_FIXTURE.as_bytes());
    assert!(
        matches!(subtitle, Err(RequestFault::Subtitle(1))),
        "T-CX-6-006: subtitle {subtitle:?}"
    );
    let requests = fetch_after_two_lines()?;
    let raw = only_request(&requests)?;
    assert!(
        check_bare_get(raw).is_ok(),
        "T-CX-6-006: real request is bare"
    );
    assert!(
        check_no_subtitle_bytes(raw).is_ok(),
        "T-CX-6-006: real request is clean"
    );
    assert!(matches!(
        check_bare_get(b"POST / HTTP/1.1\r\n\r\n"),
        Err(RequestFault::NotGet)
    ));
    assert!(matches!(
        check_bare_get(b"GET / HTTP/1.1\r\n\r\nx"),
        Err(RequestFault::Body)
    ));
    Ok(())
}

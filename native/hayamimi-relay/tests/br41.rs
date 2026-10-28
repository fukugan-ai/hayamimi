//! BR-41: the relay's `/events` names the page that may read it, and turns the
//! others away.
//!
//! The relay is started the way `cli::run` starts it (`start_live` with the
//! settings `parse_args` returned), and its `/events` is asked over TCP with
//! the `Host` and `Origin` heads a browser sends. A reply that lets a page
//! read the stream says that page's origin, never `*`; a page of another
//! origin and a host name that is not one of this machine get a 403 and no
//! stream. `--allow-origin` is what lets a page of another host in, so the
//! same request is tried with and without it.

mod support;

use std::error::Error;
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::time::Duration;

use hayamimi_relay::cli::{Command, parse_args, start_live};
use hayamimi_relay::lan::{LanUrl, lan_url};
use hayamimi_relay::lang::Lang;
use hayamimi_relay::logging::SharedSink;
use hayamimi_relay::relay::Relay;
use hayamimi_relay::strings::Strings;
use support::events::{FakeRecognizer, Logs};
use support::{FakeServer, Reply, TestResult, chat_reply};

/// A relay started from `live` and the two things it needs to be up.
struct Running {
    relay: Relay,
    logs: Logs,
    _recognizer: FakeRecognizer,
    _server: FakeServer,
}

fn start(extra: &[&str]) -> Result<Running, Box<dyn Error>> {
    let recognizer = FakeRecognizer::start()?;
    let server = FakeServer::start(|_, _| Reply::json(&chat_reply("x")))?;
    let mut args: Vec<String> = ["live", "--events", &recognizer.url(), "--llm"]
        .map(str::to_owned)
        .to_vec();
    args.push(server.base_url());
    args.push(String::from("--llm-other"));
    args.push(server.base_url());
    args.extend(["--serve", "0"].map(str::to_owned));
    args.extend(extra.iter().map(|word| (*word).to_owned()));
    let live = match parse_args(&args) {
        Ok(Command::Run(live)) => live,
        other => return Err(format!("live was not parsed: {other:?}").into()),
    };
    let strings = Strings::embedded(Lang::En)?;
    let logs = Logs::default();
    let sink = SharedSink::new(logs.clone());
    let relay = start_live(&live, 1_790_000_000, &strings, &sink)
        .map_err(|error| format!("live did not start: {error:?}"))?;
    Ok(Running {
        relay,
        logs,
        _recognizer: recognizer,
        _server: server,
    })
}

/// The `Host` and `Origin` lines of a request.
fn headers(host: &str, origin: Option<&str>) -> String {
    match origin {
        Some(origin) => format!("Host: {host}\r\nOrigin: {origin}\r\n"),
        None => format!("Host: {host}\r\n"),
    }
}

/// The head, in lower case, the relay answers a `GET /events` that carries
/// `headers` with.
fn asked(address: SocketAddr, headers: &str) -> Result<String, Box<dyn Error>> {
    let mut stream = TcpStream::connect(address)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    write!(stream, "GET /events HTTP/1.1\r\n{headers}\r\n")?;
    let mut seen = String::new();
    let mut buffer = [0_u8; 512];
    while !seen.contains("\r\n\r\n") {
        let count = stream.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        seen.push_str(&String::from_utf8_lossy(&buffer[..count]));
    }
    Ok(seen.to_lowercase())
}

fn let_in(head: &str, origin: &str) {
    assert!(head.starts_with("http/1.1 200"), "{head}");
    assert!(
        head.contains(&format!("access-control-allow-origin: {origin}\r\n")),
        "{head}"
    );
    assert!(head.contains("vary: origin\r\n"), "{head}");
    assert!(!head.contains("access-control-allow-origin: *"), "{head}");
}

fn turned_away(head: &str) {
    assert!(head.starts_with("http/1.1 403"), "{head}");
    assert!(!head.contains("access-control-allow-origin"), "{head}");
    assert!(!head.contains("text/event-stream"), "{head}");
}

/// T-BR-41-001: a page of this machine is told its own origin, a named page is
/// too, and a page of another origin and a rebound host name are turned away.
#[test]
fn br41_the_relay_events_reply_names_the_page_that_may_read_it_and_turns_the_others_away()
-> TestResult {
    let running = start(&["--allow-origin", "http://desk:8833"])?;
    let address = running.relay.address();
    let port = address.port();
    let loopback = format!("127.0.0.1:{port}");

    let page = "http://127.0.0.1:8833";
    let_in(&asked(address, &headers(&loopback, Some(page)))?, page);
    let named = "http://desk:8833";
    let_in(
        &asked(address, &headers(&format!("desk:{port}"), Some(named)))?,
        named,
    );

    turned_away(&asked(
        address,
        &headers(&loopback, Some("https://evil.test")),
    )?);
    turned_away(&asked(address, &headers(&loopback, Some("null")))?);
    turned_away(&asked(
        address,
        &headers(&format!("rebound.test:{port}"), None),
    )?);

    let plain = asked(address, &headers(&loopback, None))?;
    assert!(plain.starts_with("http/1.1 200"), "{plain}");
    assert!(!plain.contains("access-control-allow-origin"), "{plain}");

    let said = running.logs.named("relay.events.refused");
    assert_eq!(said.len(), 3, "{said:?}");
    Ok(())
}

/// T-BR-41-002: the same request that `--allow-origin` let in is turned away
/// when the relay is started without it, so the flag is what lets it in.
#[test]
fn a_page_of_another_host_is_turned_away_unless_the_relay_was_told_of_it() -> TestResult {
    let running = start(&[])?;
    let address = running.relay.address();
    let port = address.port();
    let named = "http://desk:8833";
    turned_away(&asked(
        address,
        &headers(&format!("desk:{port}"), Some(named)),
    )?);
    turned_away(&asked(
        address,
        &headers(&format!("127.0.0.1:{port}"), Some(named)),
    )?);
    Ok(())
}

/// The address a client on the LAN reaches a relay bound to every interface at.
fn lan_address(bound: SocketAddr) -> Result<SocketAddr, Box<dyn Error>> {
    match lan_url(bound, "/events") {
        LanUrl::At(url) => {
            let authority = url
                .strip_prefix("http://")
                .and_then(|rest| rest.split('/').next())
                .ok_or("the LAN URL has no authority")?;
            Ok(authority.parse()?)
        }
        other => Err(format!("this machine has no address on a network: {other:?}").into()),
    }
}

/// T-BR-41-003: a relay opened to the LAN tells a page of the same LAN address
/// its origin with no flag given, and turns away a page of another address.
#[test]
fn a_page_of_the_lan_address_the_relay_was_reached_at_is_let_in() -> TestResult {
    let running = start(&["--serve-host", "0.0.0.0"])?;
    let lan = lan_address(running.relay.address())?;
    let page = format!("http://{}:8833", lan.ip());
    let_in(&asked(lan, &headers(&lan.to_string(), Some(&page)))?, &page);
    let other = match lan.ip() {
        IpAddr::V4(_) => "http://192.0.2.77:8833",
        IpAddr::V6(_) => "http://[2001:db8::77]:8833",
    };
    turned_away(&asked(lan, &headers(&lan.to_string(), Some(other)))?);
    Ok(())
}

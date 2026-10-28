//! Which pages may read a server's streams from another origin.
//!
//! A page of the face-to-face mode reads the `/events` of a server on another
//! port (`?relay=<url>`), so that server has to name the origin it lets in.
//! It lets in the pages of this machine: a page under a loopback name or
//! address, and a page under the same host the request was made to (a page
//! and a relay on one LAN address). A page it was told of with
//! `--allow-origin` is let in as well. Every other origin is refused.
//!
//! A request whose `Host` is a DNS name that no page of this machine is
//! served under is refused too. A browser sends no `Origin` with a read of
//! its own origin, so a page that a rebound name made look like this
//! server's own would otherwise read the stream without being asked about.
//! An address, `localhost`, and the host of an `--allow-origin` are the
//! names let in.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// The longest host name DNS has.
const MAX_NAME_BYTES: usize = 253;
/// The most digits a port has.
const MAX_PORT_DIGITS: usize = 5;
/// The name every browser resolves to this machine.
const LOCALHOST: &str = "localhost";

#[derive(Clone, Debug, PartialEq, Eq)]
enum Host {
    Address(IpAddr),
    /// A DNS name in lower case.
    Name(String),
}

impl Host {
    fn is_loopback(&self) -> bool {
        match self {
            Host::Address(address) => address.to_canonical().is_loopback(),
            Host::Name(name) => name == LOCALHOST,
        }
    }
}

impl fmt::Display for Host {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Host::Address(IpAddr::V6(address)) => write!(out, "[{address}]"),
            Host::Address(address) => write!(out, "{address}"),
            Host::Name(name) => out.write_str(name),
        }
    }
}

/// The part of a URL after the scheme that names a server: a host and maybe
/// a port.
struct Authority {
    host: Host,
    port: Option<u16>,
}

/// The characters a DNS name is made of. A name with any other character is
/// not one a browser sends, so it is not read.
fn name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_')
}

fn parse_name(text: &str) -> Option<Host> {
    let well_formed = !text.is_empty()
        && text.len() <= MAX_NAME_BYTES
        && text.bytes().all(name_byte)
        && text.split('.').all(|label| !label.is_empty());
    well_formed.then(|| Host::Name(text.to_ascii_lowercase()))
}

fn parse_host(text: &str) -> Option<Host> {
    match text.parse::<Ipv4Addr>() {
        Ok(address) => Some(Host::Address(IpAddr::V4(address))),
        Err(_) => parse_name(text),
    }
}

/// A port written in digits, 1 to 65535.
fn parse_port(digits: &str) -> Option<u16> {
    let written = !digits.is_empty()
        && digits.len() <= MAX_PORT_DIGITS
        && digits.bytes().all(|byte| byte.is_ascii_digit());
    if !written {
        return None;
    }
    digits.parse().ok().filter(|port| *port != 0)
}

/// `[v6]` or `[v6]:port`, the form an IPv6 address has in a URL.
fn parse_bracketed(rest: &str) -> Option<Authority> {
    let (inner, after) = rest.split_once(']')?;
    let address: Ipv6Addr = inner.parse().ok()?;
    let port = match after.strip_prefix(':') {
        Some(digits) => Some(parse_port(digits)?),
        None if after.is_empty() => None,
        None => return None,
    };
    Some(Authority {
        host: Host::Address(IpAddr::V6(address)),
        port,
    })
}

/// `host` or `host:port`, as a `Host` header or the end of an origin has it.
fn parse_authority(text: &str) -> Option<Authority> {
    if let Some(rest) = text.strip_prefix('[') {
        return parse_bracketed(rest);
    }
    let (host_text, port_text) = match text.split_once(':') {
        Some((host_text, digits)) => (host_text, Some(digits)),
        None => (text, None),
    };
    let port = match port_text {
        Some(digits) => Some(parse_port(digits)?),
        None => None,
    };
    Some(Authority {
        host: parse_host(host_text)?,
        port,
    })
}

/// What follows `scheme://` when `text` starts so, in either case.
fn after_scheme<'a>(text: &'a str, scheme: &str) -> Option<&'a str> {
    let (head, rest) = text.split_once("://")?;
    head.eq_ignore_ascii_case(scheme).then_some(rest)
}

/// The origin of a page: scheme, host and port, as a browser writes it in an
/// `Origin` header.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Origin {
    secure: bool,
    host: Host,
    port: u16,
}

impl Origin {
    /// Reads `http://host[:port]` or `https://host[:port]` and nothing else:
    /// no path, query, user name or space, and not `null`, which the pages
    /// without an origin send.
    pub fn parse(text: &str) -> Option<Origin> {
        let (secure, rest) = match after_scheme(text, "http") {
            Some(rest) => (false, rest),
            None => (true, after_scheme(text, "https")?),
        };
        let authority = parse_authority(rest)?;
        // DEFAULT: a port left out is the default port of the scheme (RFC 6454, 6.2).
        let port = authority.port.unwrap_or(if secure { 443 } else { 80 });
        Some(Origin {
            secure,
            host: authority.host,
            port,
        })
    }
}

impl fmt::Display for Origin {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (scheme, default_port) = if self.secure {
            ("https", 443)
        } else {
            ("http", 80)
        };
        write!(out, "{scheme}://{}", self.host)?;
        if self.port != default_port {
            write!(out, ":{}", self.port)?;
        }
        Ok(())
    }
}

/// Why a request was turned away.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Its `Host` is a name no page of this machine is served under.
    Host,
    /// Its `Origin` is a page this machine does not serve.
    Origin,
}

impl Refusal {
    /// The word a log event carries for the reason.
    pub fn as_str(self) -> &'static str {
        match self {
            Refusal::Host => "host",
            Refusal::Origin => "origin",
        }
    }
}

/// What a server does with a request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Answer it. `echo` is what `Access-Control-Allow-Origin` says, the
    /// origin as the request wrote it, when the request named one.
    Serve { echo: Option<String> },
    /// Answer 403 and send nothing else.
    Refuse(Refusal),
}

/// The pages a server lets read its streams, besides the pages of the machine.
#[derive(Clone, Debug, Default)]
pub struct Guard {
    named: Vec<Origin>,
}

impl Guard {
    /// A guard that also lets in the pages of the `named` origins.
    pub fn new(named: Vec<Origin>) -> Guard {
        Guard { named }
    }

    fn knows_host(&self, host: &Host) -> bool {
        match host {
            Host::Address(_) => true,
            Host::Name(name) => {
                name == LOCALHOST
                    || self
                        .named
                        .iter()
                        .any(|origin| matches!(&origin.host, Host::Name(known) if known == name))
            }
        }
    }

    /// Whether the page of `page` is one of this machine: a loopback page, a
    /// page under the host the request was made to, or a named one.
    fn serves(&self, page: &Origin, request_host: Option<&Host>) -> bool {
        page.host.is_loopback() || request_host == Some(&page.host) || self.named.contains(page)
    }

    /// Judges a request by the values of its `Host` and `Origin` headers, in
    /// the order they came. A header given twice is refused, because a proxy
    /// and a server may read different ones.
    pub fn judge(&self, hosts: &[&str], origins: &[&str]) -> Verdict {
        let request_host = match hosts {
            [] => None,
            [text] => match parse_authority(text) {
                Some(authority) if self.knows_host(&authority.host) => Some(authority.host),
                _ => return Verdict::Refuse(Refusal::Host),
            },
            _ => return Verdict::Refuse(Refusal::Host),
        };
        match origins {
            [] => Verdict::Serve { echo: None },
            [text] => match Origin::parse(text) {
                Some(page) if self.serves(&page, request_host.as_ref()) => Verdict::Serve {
                    echo: Some((*text).to_owned()),
                },
                _ => Verdict::Refuse(Refusal::Origin),
            },
            _ => Verdict::Refuse(Refusal::Origin),
        }
    }

    /// [`Guard::judge`] for the head of a request as it came off the wire.
    pub fn judge_head(&self, head: &str) -> Verdict {
        self.judge(&header_values(head, "host"), &header_values(head, "origin"))
    }
}

/// What a request refused for `reason` showed, in a form that is safe to
/// write in a log: the origin or the host name as it was read, or a word
/// when it could not be read or came not once.
pub fn seen(head: &str, reason: Refusal) -> String {
    let name = match reason {
        Refusal::Host => "host",
        Refusal::Origin => "origin",
    };
    match header_values(head, name).as_slice() {
        [] => String::from("(none)"),
        [text] => shown(text, reason),
        _ => String::from("(repeated)"),
    }
}

fn shown(text: &str, reason: Refusal) -> String {
    let read = match reason {
        Refusal::Host => parse_authority(text).map(|authority| authority.host.to_string()),
        Refusal::Origin => Origin::parse(text).map(|origin| origin.to_string()),
    };
    match read {
        Some(read) => read,
        // DEFAULT: the log says the value could not be read and does not repeat it.
        None => String::from("(unreadable)"),
    }
}

/// The values of the header `name` in the head of a request, in order, with
/// the blanks around each taken off. The request line is not a header, and
/// what follows the blank line ending the head is not read.
pub fn header_values<'a>(head: &'a str, name: &str) -> Vec<&'a str> {
    let top = match head.split_once("\r\n\r\n") {
        Some((top, _)) => top,
        None => head,
    };
    top.lines()
        .skip(1)
        .filter_map(|line| line.split_once(':'))
        .filter(|(field, _)| field.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.trim())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(text: &str) -> Result<Origin, String> {
        Origin::parse(text).ok_or_else(|| format!("{text:?} was not read as an origin"))
    }

    fn guard_of(named: &[&str]) -> Result<Guard, String> {
        named
            .iter()
            .map(|text| parsed(text))
            .collect::<Result<Vec<_>, _>>()
            .map(Guard::new)
    }

    fn echoes(text: &str) -> Verdict {
        Verdict::Serve {
            echo: Some(text.to_owned()),
        }
    }

    const NOT_ECHOED: Verdict = Verdict::Serve { echo: None };

    #[test]
    fn an_origin_is_read_with_its_scheme_host_and_port() -> Result<(), String> {
        assert_eq!(
            parsed("http://127.0.0.1:8833")?.to_string(),
            "http://127.0.0.1:8833"
        );
        assert_eq!(
            parsed("https://Example.TEST:8443")?.to_string(),
            "https://example.test:8443"
        );
        assert_eq!(parsed("HTTP://LocalHost")?.to_string(), "http://localhost");
        assert_eq!(parsed("http://[::1]:9")?.to_string(), "http://[::1]:9");
        Ok(())
    }

    #[test]
    fn a_port_left_out_is_the_default_port_of_the_scheme() -> Result<(), String> {
        assert_eq!(parsed("http://a.test")?, parsed("http://a.test:80")?);
        assert_eq!(parsed("https://a.test")?, parsed("https://a.test:443")?);
        assert_ne!(parsed("https://a.test")?, parsed("http://a.test")?);
        assert_ne!(parsed("http://a.test")?, parsed("http://a.test:8080")?);
        Ok(())
    }

    #[test]
    fn what_is_not_an_origin_is_not_read() {
        for text in [
            "",
            "null",
            "http://",
            "http://a.test/",
            "http://a.test/x",
            "http://a.test?x=1",
            "http://user@a.test",
            "http://a.test:",
            "http://a.test:0",
            "http://a.test:65536",
            "http://a.test:+80",
            "http://a.test:8o",
            "http://a b.test",
            "http://a..test",
            "http://.a.test",
            "http://a.test.",
            "ftp://a.test",
            "file:///x",
            "a.test",
            "http://[::1",
            "http://[::1]x",
            "http://[::1]:",
            "http://[nope]",
            "http://::1",
            "http://a:b:c",
        ] {
            assert_eq!(Origin::parse(text), None, "{text:?}");
        }
    }

    #[test]
    fn a_name_of_the_longest_length_dns_has_is_read_and_one_byte_more_is_not() {
        // The two lengths are written out, not taken from the constant: the longest name DNS has is 253 bytes.
        let origin_of = |length: usize| format!("http://{}", "a".repeat(length));
        assert!(Origin::parse(&origin_of(253)).is_some());
        assert_eq!(Origin::parse(&origin_of(254)), None);
    }

    #[test]
    fn the_refusal_names_its_reason() {
        assert_eq!(Refusal::Host.as_str(), "host");
        assert_eq!(Refusal::Origin.as_str(), "origin");
    }

    #[test]
    fn a_request_naming_no_origin_is_served_without_an_echo() {
        let guard = Guard::default();
        assert_eq!(guard.judge(&["127.0.0.1:8835"], &[]), NOT_ECHOED);
        assert_eq!(guard.judge(&[], &[]), NOT_ECHOED);
    }

    #[test]
    fn a_loopback_page_is_echoed_whatever_its_port_and_scheme() {
        let guard = Guard::default();
        for page in [
            "http://127.0.0.1:8833",
            "http://localhost:8080",
            "http://[::1]:8833",
            "http://127.0.0.2",
            "https://localhost:8443",
            "http://[::ffff:127.0.0.1]:1",
        ] {
            assert_eq!(
                guard.judge(&["127.0.0.1:8835"], &[page]),
                echoes(page),
                "{page}"
            );
        }
    }

    #[test]
    fn the_origin_is_echoed_as_the_request_wrote_it() {
        let written = "HTTP://LOCALHOST:8833";
        assert_eq!(
            Guard::default().judge(&["localhost:8835"], &[written]),
            echoes(written)
        );
    }

    #[test]
    fn a_page_of_the_host_the_request_was_made_to_is_echoed() {
        let guard = Guard::default();
        let page = "http://192.168.1.10:8833";
        assert_eq!(guard.judge(&["192.168.1.10:8835"], &[page]), echoes(page));
        let v6 = "http://[fd00::10]:8833";
        assert_eq!(guard.judge(&["[fd00::10]:8835"], &[v6]), echoes(v6));
    }

    #[test]
    fn a_page_of_another_host_is_refused() {
        let guard = Guard::default();
        for page in [
            "https://evil.test",
            "http://192.168.1.11:8833",
            "http://evil.test:8833",
            "null",
            "file://",
            "http://127.0.0.1.evil.test",
        ] {
            assert_eq!(
                guard.judge(&["192.168.1.10:8835"], &[page]),
                Verdict::Refuse(Refusal::Origin),
                "{page}"
            );
        }
        assert_eq!(
            guard.judge(&["127.0.0.1:8835"], &["http://192.168.1.10:8833"]),
            Verdict::Refuse(Refusal::Origin)
        );
    }

    #[test]
    fn a_request_without_a_host_is_judged_by_its_origin_alone() {
        let guard = Guard::default();
        assert_eq!(
            guard.judge(&[], &["http://127.0.0.1:1"]),
            echoes("http://127.0.0.1:1")
        );
        assert_eq!(
            guard.judge(&[], &["http://192.168.1.10:1"]),
            Verdict::Refuse(Refusal::Origin)
        );
    }

    #[test]
    fn a_host_that_is_a_dns_name_is_refused_even_without_an_origin() {
        let guard = Guard::default();
        assert_eq!(
            guard.judge(&["rebound.test:8835"], &[]),
            Verdict::Refuse(Refusal::Host)
        );
        // The host is judged before the origin, so a page of the rebound name
        // is not let in by being the same host as the request.
        assert_eq!(
            guard.judge(&["rebound.test:8835"], &["http://rebound.test:8833"]),
            Verdict::Refuse(Refusal::Host)
        );
    }

    #[test]
    fn an_address_and_localhost_are_hosts_that_are_let_in() {
        let guard = Guard::default();
        for host in [
            "127.0.0.1",
            "127.0.0.1:8835",
            "localhost",
            "LOCALHOST:8835",
            "[::1]:8835",
            "10.0.0.7:80",
        ] {
            assert_eq!(guard.judge(&[host], &[]), NOT_ECHOED, "{host}");
        }
    }

    #[test]
    fn a_host_that_is_not_written_as_a_host_is_refused() {
        let guard = Guard::default();
        for host in [
            "",
            "localhost:",
            "localhost:0",
            "a b",
            "localhost/x",
            "[::1",
            "::1",
            "user@localhost",
        ] {
            assert_eq!(
                guard.judge(&[host], &[]),
                Verdict::Refuse(Refusal::Host),
                "{host:?}"
            );
        }
    }

    #[test]
    fn a_header_given_twice_is_refused() {
        let guard = Guard::default();
        assert_eq!(
            guard.judge(&["127.0.0.1", "localhost"], &[]),
            Verdict::Refuse(Refusal::Host)
        );
        assert_eq!(
            guard.judge(
                &["127.0.0.1"],
                &["http://127.0.0.1:1", "http://127.0.0.1:2"]
            ),
            Verdict::Refuse(Refusal::Origin)
        );
    }

    #[test]
    fn a_named_origin_is_let_in_and_so_is_its_host() -> Result<(), String> {
        let guard = guard_of(&["http://desk:8833"])?;
        let page = "http://desk:8833";
        assert_eq!(guard.judge(&["desk:8835"], &[page]), echoes(page));
        assert_eq!(guard.judge(&["DESK:8835"], &[]), NOT_ECHOED);
        // A name it was not told of is refused.
        assert_eq!(
            guard.judge(&["other:8835"], &[]),
            Verdict::Refuse(Refusal::Host)
        );
        assert_eq!(
            guard.judge(&["127.0.0.1:8835"], &[page]),
            echoes(page),
            "a named origin is let in whichever address the request came to"
        );
        assert_eq!(
            guard.judge(&["127.0.0.1:8835"], &["http://desk:9999"]),
            Verdict::Refuse(Refusal::Origin)
        );
        Ok(())
    }

    #[test]
    fn the_head_of_a_request_is_judged_by_its_host_and_origin_lines() {
        let guard = Guard::default();
        let served =
            "GET /events HTTP/1.1\r\nhost: 127.0.0.1:8835\r\nORIGIN: http://localhost:8833\r\n\r\n";
        assert_eq!(guard.judge_head(served), echoes("http://localhost:8833"));
        let foreign =
            "GET /events HTTP/1.1\r\nHost: 127.0.0.1:8835\r\nOrigin: https://evil.test\r\n\r\n";
        assert_eq!(guard.judge_head(foreign), Verdict::Refuse(Refusal::Origin));
        let bare = "GET /events HTTP/1.0\r\n\r\n";
        assert_eq!(guard.judge_head(bare), NOT_ECHOED);
    }

    #[test]
    fn a_refused_request_is_described_in_a_form_safe_for_a_log() {
        let foreign = "GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: HTTPS://Evil.Test:443\r\n\r\n";
        assert_eq!(seen(foreign, Refusal::Origin), "https://evil.test");
        assert_eq!(seen(foreign, Refusal::Host), "127.0.0.1");
        let rebound = "GET / HTTP/1.1\r\nHost: Rebound.Test:8835\r\n\r\n";
        assert_eq!(seen(rebound, Refusal::Host), "rebound.test");
        assert_eq!(seen(rebound, Refusal::Origin), "(none)");
        let odd = "GET / HTTP/1.1\r\nHost: a b\r\nOrigin: <script>\r\n\r\n";
        assert_eq!(seen(odd, Refusal::Host), "(unreadable)");
        assert_eq!(seen(odd, Refusal::Origin), "(unreadable)");
        let twice = "GET / HTTP/1.1\r\nOrigin: http://a.test\r\nOrigin: http://b.test\r\n\r\n";
        assert_eq!(seen(twice, Refusal::Origin), "(repeated)");
    }

    #[test]
    fn the_values_of_a_header_are_taken_from_the_head_only() {
        let head = "GET /events HTTP/1.1\r\nHost:  a.test \r\nx-origin: no\r\nOrigin: one\r\nORIGIN: two\r\n\r\nOrigin: body";
        assert_eq!(header_values(head, "origin"), ["one", "two"]);
        assert_eq!(header_values(head, "host"), ["a.test"]);
        assert!(header_values(head, "cookie").is_empty());
        // The request line is not a header, even when it has a colon.
        assert!(header_values("Origin: x HTTP/1.1\r\n\r\n", "origin").is_empty());
        assert!(header_values("", "origin").is_empty());
    }
}

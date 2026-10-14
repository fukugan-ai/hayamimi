//! RP-40: the subtitle page of `hayamimi` and the event stream of
//! `hayamimi-relay` are reached from a LAN address when `--serve-host
//! 0.0.0.0` is given, and are not reached from it when it is not.
//!
//! Both programs are started as processes with the command lines of the
//! design. The LAN address is the local address of a UDP socket connected
//! toward a public address (connecting sends nothing). The port of each
//! server is read from the URL line the process writes to stderr.
//!
//! `hayamimi-relay` is built by `cargo build --release -p hayamimi-relay`;
//! the test names the path it looked at when the file is missing.

mod support;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::path::Path;
use std::process::Child;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use support::{BINARY, Failure, TestResult, clip, collect, free_port, path_text, spawn_binary};

/// How long a process has to write its URL line or to start listening.
const STARTUP: Duration = Duration::from_secs(120);
/// How long one connection or one read may take.
const STEP: Duration = Duration::from_secs(5);

/// A process that is killed and collected when the test ends, whatever the
/// result, so that none is left running or defunct.
struct Running {
    child: Child,
    stderr_lines: Receiver<String>,
}

fn reap(child: &mut Child) {
    // The process may have ended already; then both calls only say so.
    let _ = child.kill();
    let _ = child.wait();
}

impl Drop for Running {
    fn drop(&mut self) {
        reap(&mut self.child);
    }
}

/// Sends each line of `pipe` to the receiver, and keeps reading after the
/// receiver is gone so that the process never blocks on a full pipe.
fn forward_lines(pipe: impl Read + Send + 'static) -> Receiver<String> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(pipe).lines().map_while(Result::ok) {
            // A dropped receiver only means nobody reads the line any more.
            let _ = sender.send(line);
        }
    });
    receiver
}

impl Running {
    fn start(binary: &str, args: &[&str]) -> Result<Running, Failure> {
        let mut child = spawn_binary(binary, args, None, &[])?;
        let (Some(stderr), Some(stdout)) = (child.stderr.take(), child.stdout.take()) else {
            reap(&mut child);
            return Err(format!("{binary} was started without its pipes"));
        };
        // The log on stdout is not read here; it is drained so the process never waits on it.
        drop(collect(stdout));
        Ok(Running {
            child,
            stderr_lines: forward_lines(stderr),
        })
    }

    /// The first line of stderr that contains `needle`.
    fn line_with(&self, needle: &str) -> Result<String, Failure> {
        let deadline = Instant::now() + STARTUP;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let line = self
                .stderr_lines
                .recv_timeout(left)
                .map_err(|error| format!("no line of stderr holds `{needle}`: {error}"))?;
            if line.contains(needle) {
                return Ok(line);
            }
        }
    }

    /// How many lines of stderr written since the last read hold `needle`.
    fn more_lines_with(&self, needle: &str) -> usize {
        self.stderr_lines
            .try_iter()
            .filter(|line| line.contains(needle))
            .count()
    }
}

/// The IPv4 address of this machine outside the loopback.
fn lan_address() -> Result<Ipv4Addr, Failure> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).map_err(|error| error.to_string())?;
    // Only the routing table is asked; the socket sends no packet.
    socket
        .connect((Ipv4Addr::new(8, 8, 8, 8), 80))
        .map_err(|error| format!("no route toward a network: {error}"))?;
    match socket.local_addr().map_err(|error| error.to_string())?.ip() {
        IpAddr::V4(address) if !address.is_loopback() && !address.is_unspecified() => Ok(address),
        other => Err(format!("no IPv4 address outside the loopback: {other}")),
    }
}

/// The `hayamimi-relay` binary next to the `hayamimi` binary.
fn relay_binary() -> Result<String, Failure> {
    let directory = Path::new(BINARY)
        .parent()
        .ok_or("the hayamimi binary has no directory")?;
    let name = if cfg!(windows) {
        "hayamimi-relay.exe"
    } else {
        "hayamimi-relay"
    };
    let path = directory.join(name);
    if !path.is_file() {
        return Err(format!(
            "{} is not built: run `cargo build --release -p hayamimi-relay`",
            path.display()
        ));
    }
    path_text(&path)
}

/// The port that follows `http://<host>:` in a line.
fn port_in(line: &str, host: Ipv4Addr) -> Result<u16, Failure> {
    let marker = format!("http://{host}:");
    let (_, rest) = line
        .split_once(&marker)
        .ok_or_else(|| format!("`{line}` holds no `{marker}`"))?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits
        .parse::<u16>()
        .map_err(|error| format!("no port after `{marker}` in `{line}`: {error}"))
}

/// The status and the content type of an answer.
struct Head {
    status: u16,
    content_type: Option<String>,
}

fn parse_head(text: &str) -> Result<Head, Failure> {
    let mut lines = text.lines();
    let status_line = lines.next().ok_or("the answer is empty")?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| format!("`{status_line}` has no status"))?
        .parse::<u16>()
        .map_err(|error| format!("`{status_line}`: {error}"))?;
    let content_type = lines.find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("content-type")
            .then(|| value.trim().to_ascii_lowercase())
    });
    Ok(Head {
        status,
        content_type,
    })
}

fn read_head(stream: &mut TcpStream) -> Result<String, Failure> {
    let mut raw = Vec::new();
    let mut chunk = [0_u8; 1024];
    while !raw.windows(4).any(|window| window == b"\r\n\r\n") {
        let count = stream
            .read(&mut chunk)
            .map_err(|error| format!("reading the head: {error}"))?;
        if count == 0 {
            return Err(String::from("the connection closed before the head ended"));
        }
        raw.extend_from_slice(&chunk[..count]);
    }
    Ok(String::from_utf8_lossy(&raw).into_owned())
}

/// `GET path` on `address`, read as far as the end of the head.
fn get_head(address: SocketAddr, path: &str) -> Result<Head, Failure> {
    let mut stream = TcpStream::connect_timeout(&address, STEP)
        .map_err(|error| format!("{address}: {error}"))?;
    stream
        .set_read_timeout(Some(STEP))
        .map_err(|error| error.to_string())?;
    let request = format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("{address}: {error}"))?;
    parse_head(&read_head(&mut stream)?)
}

fn expect_page(address: SocketAddr) -> Result<(), Failure> {
    let head = get_head(address, "/")?;
    if head.status == 200 {
        Ok(())
    } else {
        Err(format!("GET / on {address} answered {}", head.status))
    }
}

fn expect_events(address: SocketAddr) -> Result<(), Failure> {
    let head = get_head(address, "/events")?;
    let is_stream = head
        .content_type
        .as_deref()
        .is_some_and(|kind| kind.starts_with("text/event-stream"));
    if head.status == 200 && is_stream {
        Ok(())
    } else {
        Err(format!(
            "GET /events on {address} answered {} {:?}",
            head.status, head.content_type
        ))
    }
}

/// Waits until `address` accepts a connection.
fn wait_listening(address: SocketAddr) -> Result<(), Failure> {
    let deadline = Instant::now() + STARTUP;
    while TcpStream::connect_timeout(&address, STEP).is_err() {
        if Instant::now() > deadline {
            return Err(format!("nothing listens on {address}"));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

fn loopback(port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
}

fn on_lan(lan: Ipv4Addr, port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(lan), port)
}

/// Starts `hayamimi` with `--serve 0 --serve-host 0.0.0.0`.
fn start_hayamimi_on_the_lan() -> Result<Running, Failure> {
    let clip = path_text(&clip())?;
    Running::start(
        BINARY,
        &["--wav", &clip, "--serve", "0", "--serve-host", "0.0.0.0"],
    )
}

#[test]
#[ignore = "starts the real hayamimi and hayamimi-relay"]
fn rp40_pages_and_relay_events_reach_from_the_lan() -> TestResult {
    let lan = lan_address()?;
    let marker = format!("http://{lan}:");

    let hayamimi = start_hayamimi_on_the_lan()?;
    let page_port = port_in(&hayamimi.line_with(&marker)?, lan)?;
    for address in [on_lan(lan, page_port), loopback(page_port)] {
        expect_page(address)?;
        expect_events(address)?;
    }
    assert_eq!(hayamimi.more_lines_with(&marker), 0, "the URL line repeats");

    let relay = Running::start(
        &relay_binary()?,
        &["live", "--serve", "0", "--serve-host", "0.0.0.0"],
    )?;
    let events_port = port_in(&relay.line_with(&marker)?, lan)?;
    for address in [on_lan(lan, events_port), loopback(events_port)] {
        expect_events(address)?;
    }
    assert_eq!(relay.more_lines_with(&marker), 0, "the URL line repeats");
    Ok(())
}

/// The negative control: without `--serve-host` the servers answer on the
/// loopback only, and say no LAN URL.
#[test]
#[ignore = "starts the real hayamimi and hayamimi-relay"]
fn rp40_without_serve_host_the_lan_address_is_refused() -> TestResult {
    let lan = lan_address()?;
    let marker = format!("http://{lan}:");
    let clip = path_text(&clip())?;

    let page_port = free_port()?;
    let page = page_port.to_string();
    let hayamimi = Running::start(BINARY, &["--wav", &clip, "--serve", &page])?;
    wait_listening(loopback(page_port))?;
    expect_page(loopback(page_port))?;
    assert!(
        TcpStream::connect_timeout(&on_lan(lan, page_port), STEP).is_err(),
        "hayamimi answered on {lan} without --serve-host"
    );

    let events_port = free_port()?;
    let events = events_port.to_string();
    let relay = Running::start(&relay_binary()?, &["live", "--serve", &events])?;
    wait_listening(loopback(events_port))?;
    expect_events(loopback(events_port))?;
    assert!(
        TcpStream::connect_timeout(&on_lan(lan, events_port), STEP).is_err(),
        "hayamimi-relay answered on {lan} without --serve-host"
    );

    assert_eq!(hayamimi.more_lines_with(&marker), 0);
    assert_eq!(relay.more_lines_with(&marker), 0);
    Ok(())
}

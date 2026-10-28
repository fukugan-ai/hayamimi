//! BR-35: when `--serve-host` is not the loopback, the control routes of the
//! subtitle server answer a peer on this machine only, and `/events` of
//! `hayamimi` carries no `access-control-allow-origin` for a request that names
//! no page. The default `--serve-host` changes nothing about the control routes.
//!
//! BR-42 (the cases named `br42_` and those after them): the pages, `/events`
//! and the control routes answer only a page of this machine, a page of the LAN
//! address the request was made to, and a page the server was told of with
//! `--allow-origin`. A page of another origin and a host name that is not one of
//! this machine get a 403. The reply tells the page its own origin, never `*`.
//!
//! The routes are served in this process on a port the system chose, with a
//! stand-in for the recognition loop. A LAN peer is a connection to the
//! machine's own LAN address: the kernel then names that address as the
//! source, so the server sees a peer that is not the loopback. The LAN
//! address is the local address of a UDP socket connected toward a public
//! address (connecting sends nothing). No model is needed.

use std::error::Error;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream, UdpSocket};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use hayamimi_app::args::{self, Parsed};
use hayamimi_app::control::{Command, Controls, Initial, Live};
use hayamimi_app::routes::{self, AppState};
use hayamimi_app::server::{self, Hub};
use hayamimi_app::ui;
use hayamimi_asr::itn::Overrides;
use hayamimi_asr::replace::Replacements;
use hayamimi_asr::vad::Sensitivity;
use hayamimi_relay::origin::{Guard, Origin};
use serde_json::Value;

type TestResult = Result<(), Box<dyn Error>>;

/// How long one connection or one read may take.
const STEP: Duration = Duration::from_secs(15);

const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

/// `--serve-host 0.0.0.0`.
const ANY_HOST: IpAddr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);

/// The five control routes of the acceptance.
const ACCEPTANCE_ROUTES: [(&str, &str); 5] = [
    ("POST", "/config"),
    ("GET", "/config"),
    ("POST", "/reset"),
    ("POST", "/replacements"),
    ("POST", "/itn_overrides"),
];

/// The two reads that the design adds to them.
const READ_ROUTES: [&str; 2] = ["/replacements", "/itn_overrides"];

/// The routes whose answer shows the state a peer could have changed.
const STATE_ROUTES: [&str; 3] = ["/config", "/replacements", "/itn_overrides"];

/// A word that only a request of these tests puts into the state.
const MARKER: &str = "leakmarker";

/// The name of the file the `/ui/` case reads.
const PROBE_NAME: &str = "br35-probe.js";

/// The headers of an answer, in the order they came.
type Headers = Vec<(String, String)>;

/// What a request got back.
struct Reply {
    status: u16,
    headers: Headers,
    body: String,
}

impl Reply {
    /// The value of a header; names are compared without regard to case.
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(found, _)| found.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    fn is_event_stream(&self) -> bool {
        self.header("content-type")
            .is_some_and(|kind| kind.starts_with("text/event-stream"))
    }
}

/// The head of an answer: the status, then the headers up to the blank line.
fn read_head(reader: &mut impl BufRead) -> Result<(u16, Headers), Box<dyn Error>> {
    let mut status_line = String::new();
    reader.read_line(&mut status_line)?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .ok_or("the answer has no status line")?
        .parse::<u16>()?;
    let mut headers = Vec::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Err("the connection closed inside the head".into());
        }
        let line = line.trim_end();
        if line.is_empty() {
            return Ok((status, headers));
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.to_owned(), value.trim().to_owned()));
        }
    }
}

/// Sends one request and reads the answer. A server-sent stream never ends,
/// so its body is not read. `Host` is `localhost`.
fn send(
    address: SocketAddr,
    method: &str,
    path: &str,
    body: &str,
    extra: &[(&str, &str)],
) -> Result<Reply, Box<dyn Error>> {
    send_as(address, "localhost", method, path, body, extra)
}

/// [`send`] with the `Host` a page that reached `address` by that name sends.
fn send_as(
    address: SocketAddr,
    host: &str,
    method: &str,
    path: &str,
    body: &str,
    extra: &[(&str, &str)],
) -> Result<Reply, Box<dyn Error>> {
    let mut stream = TcpStream::connect_timeout(&address, STEP)?;
    stream.set_read_timeout(Some(STEP))?;
    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\
         Content-Length: {}\r\n",
        body.len()
    );
    for (name, value) in extra {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    request.push_str("\r\n");
    request.push_str(body);
    stream.write_all(request.as_bytes())?;
    let mut reader = BufReader::new(stream);
    let (status, headers) = read_head(&mut reader)?;
    let mut reply = Reply {
        status,
        headers,
        body: String::new(),
    };
    if !reply.is_event_stream() {
        reader.read_to_string(&mut reply.body)?;
    }
    Ok(reply)
}

/// The IPv4 address of this machine outside the loopback.
fn lan_address() -> Result<IpAddr, Box<dyn Error>> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
    // Only the routing table is asked; the socket sends no packet.
    socket.connect((Ipv4Addr::new(8, 8, 8, 8), 80))?;
    match socket.local_addr()?.ip() {
        IpAddr::V4(address) if !address.is_loopback() && !address.is_unspecified() => {
            Ok(IpAddr::V4(address))
        }
        other => Err(format!("no IPv4 address outside the loopback: {other}").into()),
    }
}

/// A server of the routes and the receiver of what the routes send to the loop.
struct Served {
    port: u16,
    commands: Receiver<Command>,
}

impl Served {
    fn at(&self, ip: IpAddr) -> SocketAddr {
        SocketAddr::new(ip, self.port)
    }
}

/// The routes of `hayamimi` for a listener on `serve_host`, on a port the system chose.
fn start(serve_host: IpAddr) -> Result<Served, Box<dyn Error>> {
    start_listening_on(serve_host, serve_host)
}

/// The routes for `serve_host`, served on `listen_on`. The routes decide by
/// `serve_host` alone, so a host this machine cannot listen on can still be
/// measured by listening on another.
fn start_listening_on(serve_host: IpAddr, listen_on: IpAddr) -> Result<Served, Box<dyn Error>> {
    start_guarded(serve_host, listen_on, Guard::default())
}

/// [`start_listening_on`] for the pages of this machine and those `guard` names.
fn start_guarded(
    serve_host: IpAddr,
    listen_on: IpAddr,
    guard: Guard,
) -> Result<Served, Box<dyn Error>> {
    let (commands, received) = mpsc::channel();
    let initial = Initial {
        replacements: Replacements::default(),
        overrides: Overrides::default(),
        punctuate: true,
        can_punctuate: true,
        vad: Sensitivity {
            threshold: 0.5,
            min_silence: 0.35,
            max_speech: 12.0,
        },
    };
    let state = AppState {
        hub: Hub::new(),
        controls: Arc::new(Controls::new(Live::new(initial), commands)),
    };
    let listener = server::bind_on(listen_on, 0)?;
    let address = server::serve(
        routes::router_for(state, serve_host, guard),
        listener,
        "br35",
    )?;
    Ok(Served {
        port: address.port(),
        commands: received,
    })
}

/// A stand-in loop that answers every reset.
fn answer_resets(commands: Receiver<Command>) {
    std::thread::spawn(move || {
        for command in commands {
            if let Command::Reset(done) = command {
                // The route may have stopped waiting; then nobody needs the answer.
                let _ = done.send(());
            }
        }
    });
}

/// The `--serve-host` of a command line that does not name one.
fn default_serve_host() -> Result<IpAddr, Box<dyn Error>> {
    let argv = [String::from("--wav"), String::from("a.wav")];
    match args::parse(&argv) {
        Ok(Parsed::Run(parsed)) => Ok(parsed.serve_host),
        Ok(Parsed::Help) => Err("the command line asked for help".into()),
        Err(error) => Err(format!("the command line was refused: {error:?}").into()),
    }
}

/// The body of a request that a peer on this machine gets a 200 for.
fn body_of(method: &str, path: &str) -> String {
    match (method, path) {
        ("POST", "/config") => String::from(r#"{"punctuate":false}"#),
        ("POST", "/replacements") => format!(r#"{{"{MARKER}":"right"}}"#),
        ("POST", "/itn_overrides") => format!(r#"{{"exclude":["{MARKER}"],"force":{{"a":"b"}}}}"#),
        _ => String::new(),
    }
}

/// The bodies of the three reads, each answered 200.
fn snapshot(address: SocketAddr) -> Result<Vec<String>, Box<dyn Error>> {
    STATE_ROUTES
        .iter()
        .map(|path| {
            let reply = send(address, "GET", path, "", &[])?;
            assert_eq!(reply.status, 200, "GET {path} from {address}");
            Ok(reply.body)
        })
        .collect()
}

/// Every route of the acceptance, asked of a refusing server, answers 403.
fn assert_refused(address: SocketAddr, routes: &[(&str, &str)]) -> TestResult {
    for (method, path) in routes {
        let reply = send(address, method, path, &body_of(method, path), &[])?;
        assert_eq!(reply.status, 403, "{method} {path} from {address}");
    }
    Ok(())
}

/// The seven control routes used by a peer that is accepted: the three
/// changes show in the reads that follow, and the reset is answered.
fn exercise_control_ports(address: SocketAddr) -> TestResult {
    for (method, path) in ACCEPTANCE_ROUTES {
        let reply = send(address, method, path, &body_of(method, path), &[])?;
        assert_eq!(reply.status, 200, "{method} {path} from {address}");
    }
    let config: Value = serde_json::from_str(&send(address, "GET", "/config", "", &[])?.body)?;
    assert_eq!(config["punctuate"], false);
    for path in READ_ROUTES {
        let shown = send(address, "GET", path, "", &[])?;
        assert_eq!(shown.status, 200, "GET {path} from {address}");
        assert!(shown.body.contains(MARKER), "{path}: {}", shown.body);
    }
    Ok(())
}

/// `GET /events`, asked the way a page that reached `address` by that address
/// asks, with `origin` as its page when it has one.
fn events_of(address: SocketAddr, origin: Option<&str>) -> Result<Reply, Box<dyn Error>> {
    let extra: Vec<(&str, &str)> = origin.map(|page| ("Origin", page)).into_iter().collect();
    send_as(address, &address.to_string(), "GET", "/events", "", &extra)
}

/// `GET /events` answers 200 as a stream, with `cors` as its allow-origin
/// header, and says that the reply varies with the origin.
fn assert_events(address: SocketAddr, origin: Option<&str>, cors: Option<&str>) -> TestResult {
    let reply = events_of(address, origin)?;
    assert_eq!(reply.status, 200, "GET /events from {address}");
    assert!(reply.is_event_stream(), "{:?}", reply.headers);
    assert_eq!(
        reply.header("access-control-allow-origin"),
        cors,
        "{:?}",
        reply.headers
    );
    assert!(
        reply
            .header("vary")
            .is_some_and(|vary| vary.eq_ignore_ascii_case("origin")),
        "{:?}",
        reply.headers
    );
    Ok(())
}

/// `GET /events` is turned away: 403, no stream and no allow-origin header.
fn assert_events_refused(address: SocketAddr, origin: Option<&str>) -> TestResult {
    let reply = events_of(address, origin)?;
    assert_eq!(reply.status, 403, "GET /events from {address}");
    assert!(!reply.is_event_stream(), "{:?}", reply.headers);
    assert_eq!(
        reply.header("access-control-allow-origin"),
        None,
        "{:?}",
        reply.headers
    );
    Ok(())
}

/// The relay's event stream on `0.0.0.0`, and the address of its port.
fn start_relay_hub() -> Result<u16, Box<dyn Error>> {
    let listener = hayamimi_relay::hub::bind_on(ANY_HOST, 0)?;
    let address = hayamimi_relay::hub::serve(listener, hayamimi_relay::hub::Hub::new())?;
    Ok(address.port())
}

/// A file in the `ui` directory next to the test executable, removed when dropped.
struct Probe(PathBuf);

impl Probe {
    fn create() -> Result<Probe, Box<dyn Error>> {
        let directory = ui::bundle_dir().ok_or("the test executable has no directory")?;
        std::fs::create_dir_all(&directory)?;
        let path = directory.join(PROBE_NAME);
        std::fs::write(&path, "// read by the BR-35 page case\n")?;
        Ok(Probe(path))
    }
}

impl Drop for Probe {
    fn drop(&mut self) {
        // The file or the directory may be gone already; a directory that
        // holds other files stays.
        let _ = std::fs::remove_file(&self.0);
        if let Some(directory) = self.0.parent() {
            let _ = std::fs::remove_dir(directory);
        }
    }
}

#[test]
fn br35_a_lan_serve_host_keeps_control_routes_to_loopback_and_events_to_its_origin() -> TestResult {
    let served = start(ANY_HOST)?;
    let lan = served.at(lan_address()?);
    let local = served.at(LOOPBACK);
    let before = snapshot(local)?;
    assert_refused(lan, &ACCEPTANCE_ROUTES)?;
    assert_eq!(
        snapshot(local)?,
        before,
        "a refused request changed the state"
    );
    assert_events(lan, None, None)?;

    answer_resets(served.commands);
    exercise_control_ports(local)?;

    // The relay no longer says `*` either (BR-41 and H69); a request with no
    // page named gets no allow-origin header from it.
    let relay = SocketAddr::new(lan.ip(), start_relay_hub()?);
    assert_events(relay, None, None)?;

    let by_default = start(default_serve_host()?)?;
    assert_events(by_default.at(LOOPBACK), None, None)
}

/// T-BR-35-001
#[test]
fn br35_the_control_ports_refuse_a_lan_peer_and_leave_the_state_alone() -> TestResult {
    let served = start(ANY_HOST)?;
    let lan = served.at(lan_address()?);
    let before = snapshot(served.at(LOOPBACK))?;
    assert_refused(lan, &ACCEPTANCE_ROUTES)?;
    assert_eq!(
        snapshot(served.at(LOOPBACK))?,
        before,
        "a refused request changed the state"
    );
    assert!(
        served.commands.try_recv().is_err(),
        "a refused request reached the loop"
    );
    Ok(())
}

/// T-BR-35-002
#[test]
fn a_lan_peer_cannot_read_the_replacements_or_the_overrides() -> TestResult {
    let served = start(ANY_HOST)?;
    let lan = served.at(lan_address()?);
    let local = served.at(LOOPBACK);
    for path in READ_ROUTES {
        let put = send(local, "POST", path, &body_of("POST", path), &[])?;
        assert_eq!(put.status, 200, "POST {path} from the loopback");
        let shown = send(local, "GET", path, "", &[])?;
        assert_eq!(shown.status, 200, "GET {path} from the loopback");
        assert!(shown.body.contains(MARKER), "{path}: {}", shown.body);
        let refused = send(lan, "GET", path, "", &[])?;
        assert_eq!(refused.status, 403, "GET {path} from the LAN");
        assert!(!refused.body.contains(MARKER), "{path}: {}", refused.body);
        assert!(!refused.body.contains(&shown.body), "{path}");
    }
    Ok(())
}

/// T-BR-35-003
#[test]
fn a_loopback_peer_still_uses_every_control_port() -> TestResult {
    let served = start(ANY_HOST)?;
    let local = served.at(LOOPBACK);
    answer_resets(served.commands);
    exercise_control_ports(local)
}

/// T-BR-35-005
#[test]
fn the_events_reply_has_no_cors_header_when_the_serve_host_is_not_loopback() -> TestResult {
    let served = start(ANY_HOST)?;
    assert_events(served.at(lan_address()?), None, None)?;
    assert_events(served.at(LOOPBACK), None, None)
}

/// T-BR-35-006
#[test]
fn with_the_default_serve_host_the_control_routes_work_and_events_has_no_star() -> TestResult {
    let host = default_serve_host()?;
    let served = start(host)?;
    let local = served.at(host);
    assert_events(local, None, None)?;
    answer_resets(served.commands);
    exercise_control_ports(local)
}

/// T-BR-35-007. `127.0.0.2` is listened on as it is. The IPv6 loopback is
/// named as the `--serve-host` and listened on at `127.0.0.1`, because a
/// machine without an IPv6 stack cannot bind `::1`. A loopback page is told
/// its own origin whichever way the loopback was named.
#[test]
fn any_loopback_serve_host_tells_a_loopback_page_its_own_origin() -> TestResult {
    let page = "http://127.0.0.1:8833";
    let second = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2));
    let served = start(second)?;
    assert_events(served.at(second), Some(page), Some(page))?;
    let named_v6 = start_listening_on(IpAddr::V6(Ipv6Addr::LOCALHOST), LOOPBACK)?;
    assert_events(named_v6.at(LOOPBACK), Some(page), Some(page))
}

/// T-BR-35-008
#[test]
fn the_pages_and_files_are_still_served_to_a_lan_peer() -> TestResult {
    let _probe = Probe::create()?;
    let served = start(ANY_HOST)?;
    let lan = served.at(lan_address()?);
    let file = format!("/ui/{PROBE_NAME}");
    for path in ["/", "/dashboard", "/transcript", file.as_str()] {
        let reply = send(lan, "GET", path, "", &[])?;
        assert_eq!(reply.status, 200, "GET {path} from the LAN");
    }
    Ok(())
}

/// T-BR-35-009
#[test]
fn a_lan_peer_cannot_pass_as_loopback_by_headers() -> TestResult {
    let served = start(ANY_HOST)?;
    let lan = served.at(lan_address()?);
    let before = snapshot(served.at(LOOPBACK))?;
    let origin = format!("http://127.0.0.1:{}", served.port);
    let headers = [
        ("X-Forwarded-For", "127.0.0.1"),
        ("Origin", origin.as_str()),
    ];
    let reply = send(
        lan,
        "POST",
        "/config",
        &body_of("POST", "/config"),
        &headers,
    )?;
    assert_eq!(reply.status, 403);
    assert_eq!(snapshot(served.at(LOOPBACK))?, before);
    Ok(())
}

/// T-BR-35-010. The relay is outside C6 but not outside BR-41: it tells a page
/// of the LAN address it was reached at that page's origin, and no page `*`.
#[test]
fn the_relay_events_reply_tells_the_page_its_origin_and_never_a_star() -> TestResult {
    let port = start_relay_hub()?;
    let lan = SocketAddr::new(lan_address()?, port);
    let page = format!("http://{}:8833", lan.ip());
    assert_events(lan, Some(&page), Some(&page))?;
    assert_events(lan, None, None)
}

/// T-BR-42-001. The origin of a page of this machine is told back; a page of
/// another origin gets a 403 and no stream.
#[test]
fn br42_a_page_of_this_machine_reads_events_and_a_page_of_another_origin_is_turned_away()
-> TestResult {
    let served = start(LOOPBACK)?;
    let local = served.at(LOOPBACK);
    let page = "http://127.0.0.1:8833";
    assert_events(local, Some(page), Some(page))?;
    let localhost = "http://localhost:8080";
    assert_events(local, Some(localhost), Some(localhost))?;
    assert_events(local, None, None)?;
    assert_events_refused(local, Some("https://evil.test"))?;
    assert_events_refused(local, Some("null"))?;
    assert_events_refused(local, Some("http://192.168.1.10:8833"))
}

/// T-BR-42-002. A page of another origin neither changes nor reads the
/// settings: the request is turned away before the route sees it.
#[test]
fn a_page_of_another_origin_cannot_change_or_read_the_settings() -> TestResult {
    let served = start(LOOPBACK)?;
    let local = served.at(LOOPBACK);
    let before = snapshot(local)?;
    let foreign = [("Origin", "https://evil.test")];
    for (method, path) in ACCEPTANCE_ROUTES {
        let reply = send_as(
            local,
            &local.to_string(),
            method,
            path,
            &body_of(method, path),
            &foreign,
        )?;
        assert_eq!(reply.status, 403, "{method} {path} from a foreign page");
        assert_eq!(reply.header("access-control-allow-origin"), None);
        assert!(!reply.body.contains(MARKER), "{path}: {}", reply.body);
    }
    assert_eq!(
        snapshot(local)?,
        before,
        "a turned-away request changed the state"
    );
    assert!(
        served.commands.try_recv().is_err(),
        "a turned-away request reached the loop"
    );
    Ok(())
}

/// T-BR-42-003. The reply of a route to a page of this machine carries that
/// page's origin and never `*`, whichever route it is.
#[test]
fn no_route_answers_a_loopback_page_with_a_star() -> TestResult {
    let served = start(LOOPBACK)?;
    let local = served.at(LOOPBACK);
    answer_resets(served.commands);
    let page = "http://127.0.0.1:8833";
    let origin = [("Origin", page)];
    for (method, path) in ACCEPTANCE_ROUTES {
        let reply = send_as(
            local,
            &local.to_string(),
            method,
            path,
            &body_of(method, path),
            &origin,
        )?;
        assert_eq!(reply.status, 200, "{method} {path}");
        assert_eq!(
            reply.header("access-control-allow-origin"),
            Some(page),
            "{method} {path}"
        );
    }
    for path in ["/", "/dashboard", "/transcript"] {
        let reply = send_as(local, &local.to_string(), "GET", path, "", &origin)?;
        assert_eq!(reply.status, 200, "GET {path}");
        assert_eq!(
            reply.header("access-control-allow-origin"),
            Some(page),
            "GET {path}"
        );
    }
    Ok(())
}

/// T-BR-42-004. A host name that is not one of this machine is turned away from
/// every route, even from a peer on the loopback and with no `Origin`: a page
/// a rebound name made look like this server's own reads nothing.
#[test]
fn a_rebound_host_name_is_turned_away_from_every_route() -> TestResult {
    let served = start(LOOPBACK)?;
    let local = served.at(LOOPBACK);
    let before = snapshot(local)?;
    for (method, path) in ACCEPTANCE_ROUTES {
        let reply = send_as(
            local,
            "rebound.test",
            method,
            path,
            &body_of(method, path),
            &[],
        )?;
        assert_eq!(reply.status, 403, "{method} {path} to a rebound name");
    }
    for path in ["/", "/events"] {
        let reply = send_as(local, "rebound.test:8833", "GET", path, "", &[])?;
        assert_eq!(reply.status, 403, "GET {path} to a rebound name");
    }
    assert_eq!(snapshot(local)?, before);
    for host in ["localhost", "127.0.0.1", "[::1]:8833", "LOCALHOST:8833"] {
        let reply = send_as(local, host, "GET", "/", "", &[])?;
        assert_eq!(reply.status, 200, "GET / to {host}");
    }
    Ok(())
}

/// T-BR-42-005. A server opened to the LAN tells a page of the LAN address it
/// was reached at that page's origin, and turns away a page of another address.
#[test]
fn a_page_of_the_lan_address_is_let_in_by_a_server_opened_to_the_lan() -> TestResult {
    let served = start(ANY_HOST)?;
    let lan = served.at(lan_address()?);
    let page = format!("http://{}:8833", lan.ip());
    assert_events(lan, Some(&page), Some(&page))?;
    assert_events_refused(lan, Some("http://192.0.2.77:8833"))?;
    assert_events_refused(lan, Some("https://evil.test"))
}

/// T-BR-42-006. A page the server was told of is let in, and so is its host
/// name; the same request is turned away when the server was not told.
#[test]
fn a_named_origin_is_let_in_and_an_unnamed_one_is_not() -> TestResult {
    let desk = Origin::parse("http://desk:8833").ok_or("not an origin")?;
    let told = start_guarded(LOOPBACK, LOOPBACK, Guard::new(vec![desk]))?;
    let named = "http://desk:8833";
    let host = format!("desk:{}", told.port);
    let reply = send_as(
        told.at(LOOPBACK),
        &host,
        "GET",
        "/events",
        "",
        &[("Origin", named)],
    )?;
    assert_eq!(reply.status, 200);
    assert_eq!(reply.header("access-control-allow-origin"), Some(named));

    let untold = start(LOOPBACK)?;
    let host = format!("desk:{}", untold.port);
    let reply = send_as(
        untold.at(LOOPBACK),
        &host,
        "GET",
        "/events",
        "",
        &[("Origin", named)],
    )?;
    assert_eq!(reply.status, 403);
    Ok(())
}

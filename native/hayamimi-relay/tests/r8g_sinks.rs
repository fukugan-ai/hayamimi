//! R8g: the Zoom and Teams destinations on the wire, against fake receivers
//! on 127.0.0.1, and the RP-29 scene with the real destinations.
//!
//! These stand in for CS-17 and CS-18 (a caption that reaches a live
//! meeting), which need a token or a link from a meeting and are not run
//! here. What they pin is the request: method, address, plain-text body, and
//! the number of the caption.
#![cfg(unix)]

mod support;

use std::time::{Duration, Instant};

use support::events::unused_port;
use support::private::PrivateDir;
use support::sinks::{Scene, start};
use support::{FakeServer, Reply, TestResult, request_body};

const ZOOM_PATH: &str = "/closedcaption?id=SECRETID0123&sig=SECRETSIG4567";

fn zoom_file(base: &str) -> String {
    format!("[zoom]\ntoken = \"{base}{ZOOM_PATH}\"\n")
}

fn teams_file(base: &str) -> String {
    format!("[teams]\nlink = \"{base}/cart/abc\"\n")
}

/// The first line of a raw request.
fn request_line(raw: &[u8]) -> Result<String, String> {
    let text = String::from_utf8_lossy(raw);
    let line = text
        .lines()
        .next()
        .ok_or_else(|| String::from("an empty request"))?;
    Ok(line.to_owned())
}

/// A header of a raw request, whatever the case of its name.
fn header(raw: &[u8], name: &str) -> Option<String> {
    let text = String::from_utf8_lossy(raw).into_owned();
    text.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.eq_ignore_ascii_case(name)
            .then(|| value.trim().to_owned())
    })
}

fn ok_server() -> std::io::Result<FakeServer> {
    FakeServer::start(|_, _| Reply::status(200))
}

fn scene_for(dir: &PrivateDir, text: &str) -> Result<Scene, Box<dyn std::error::Error>> {
    let path = dir.write("send.toml", text, 0o600)?;
    start(&path)
}

#[test]
fn cs17_fake_receiver_gets_the_zoom_wire_shape() -> TestResult {
    let zoom = ok_server()?;
    let dir = PrivateDir::new("zoom-wire")?;
    let scene = scene_for(&dir, &zoom_file(&zoom.base_url()))?;
    scene.say(1);
    scene.say(2);
    scene.wait_for_log("translate.send.ok", 2);
    let requests = zoom.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        request_line(&requests[0])?,
        format!("POST {ZOOM_PATH}&seq=1&lang=en-US HTTP/1.1")
    );
    assert!(request_line(&requests[1])?.contains("&seq=2&lang=en-US"));
    let kind = header(&requests[0], "content-type");
    assert!(kind.is_some_and(|value| value.starts_with("text/plain")));
    assert_eq!(request_body(&requests[0]), b"Hello.");
    Ok(())
}

#[test]
fn cs18_fake_receiver_gets_the_teams_wire_shape() -> TestResult {
    let teams = ok_server()?;
    let dir = PrivateDir::new("teams-wire")?;
    let scene = scene_for(&dir, &teams_file(&teams.base_url()))?;
    scene.say(1);
    scene.wait_for_log("translate.send.ok", 1);
    let requests = teams.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(request_line(&requests[0])?, "POST /cart/abc HTTP/1.1");
    assert_eq!(request_body(&requests[0]), b"Hello.");
    Ok(())
}

#[test]
fn a_destination_the_file_does_not_name_is_passed_over_without_a_word() -> TestResult {
    let teams = ok_server()?;
    let dir = PrivateDir::new("passed-over")?;
    let scene = scene_for(&dir, &teams_file(&teams.base_url()))?;
    scene.say(1);
    scene.wait_for_log("translate.send.ok", 1);
    // Zoom comes before Teams in the courier's order, so it has been passed over.
    let sent = scene.running.logs.named("translate.send.ok");
    assert!(
        sent.iter()
            .all(|event| event.fields["destination"] == "teams")
    );
    assert!(scene.running.logs.named("translate.send.failed").is_empty());
    assert!(scene.running.logs.named("sink.fallback.local").is_empty());
    Ok(())
}

#[test]
fn a_file_that_is_not_there_yet_sends_nothing_and_says_nothing() -> TestResult {
    let dir = PrivateDir::new("not-yet")?;
    let scene = start(&dir.join("send.toml"))?;
    scene.say(1);
    assert!(scene.running.logs.named("translate.send.ok").is_empty());
    assert!(scene.running.logs.named("translate.send.failed").is_empty());
    assert!(scene.running.logs.named("send.config.too_open").is_empty());
    // The meeting hands over its token while the relay runs.
    let zoom = ok_server()?;
    dir.write("send.toml", &zoom_file(&zoom.base_url()), 0o600)?;
    scene.say(2);
    scene.wait_for_log("translate.send.ok", 1);
    assert_eq!(zoom.requests().len(), 1);
    Ok(())
}

#[test]
fn a_refused_caption_is_said_with_its_status_and_the_line_is_kept() -> TestResult {
    let zoom = FakeServer::start(|_, _| Reply::status(500))?;
    let dir = PrivateDir::new("refused")?;
    let scene = scene_for(&dir, &zoom_file(&zoom.base_url()))?;
    scene.say(1);
    scene.wait_for_log("translate.send.failed", 1);
    let failed = scene.running.logs.named("translate.send.failed");
    assert_eq!(failed[0].fields["reason"], "refused");
    assert_eq!(failed[0].fields["status"], 500);
    assert!(scene.srt.text().contains("Hello."));
    assert_eq!(scene.running.logs.named("sink.fallback.local").len(), 1);
    Ok(())
}

#[test]
fn a_redirect_is_not_followed_so_the_credential_goes_nowhere_else() -> TestResult {
    let elsewhere = ok_server()?;
    let target = elsewhere.url("/stolen");
    let zoom = FakeServer::start(move |_, _| Reply::redirect(&target))?;
    let dir = PrivateDir::new("redirect")?;
    let scene = scene_for(&dir, &zoom_file(&zoom.base_url()))?;
    scene.say(1);
    scene.wait_for_log("translate.send.failed", 1);
    let failed = scene.running.logs.named("translate.send.failed");
    assert_eq!(failed[0].fields["reason"], "refused");
    assert_eq!(failed[0].fields["status"], 302);
    assert!(elsewhere.requests().is_empty());
    Ok(())
}

#[test]
fn rp29_real_destination_down_keeps_the_line_then_the_next_token_clears_it() -> TestResult {
    let dir = PrivateDir::new("rp29")?;
    let dead = format!("http://127.0.0.1:{}", unused_port()?);
    let scene = scene_for(&dir, &zoom_file(&dead))?;
    scene.say(1);
    scene.wait_for_log("translate.send.failed", 1);
    // The line is kept, and the fall is said once on each channel.
    assert!(scene.srt.text().contains("Hello."));
    assert_eq!(scene.running.logs.named("sink.fallback.local").len(), 1);
    assert!(scene.running.logs.named("sink.fallback.cleared").is_empty());
    // A new token arrives; the next caption goes, and the return is said once.
    let zoom = ok_server()?;
    dir.write("send.toml", &zoom_file(&zoom.base_url()), 0o600)?;
    scene.say(2);
    scene.wait_for_log("sink.fallback.cleared", 1);
    assert_eq!(zoom.requests().len(), 1);
    assert_eq!(scene.running.logs.named("sink.fallback.local").len(), 1);
    Ok(())
}

#[test]
fn a_destination_that_does_not_answer_is_given_up_on_and_the_line_is_kept() -> TestResult {
    let slow = FakeServer::start(|_, _| {
        std::thread::sleep(Duration::from_secs(5));
        Reply::status(200)
    })?;
    let dir = PrivateDir::new("slow")?;
    let scene = scene_for(&dir, &zoom_file(&slow.base_url()))?;
    let started = Instant::now();
    scene.say(1);
    scene.wait_for_log("translate.send.failed", 1);
    let failed = scene.running.logs.named("translate.send.failed");
    assert_eq!(failed[0].fields["reason"], "unreachable");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    assert!(scene.srt.text().contains("Hello."));
    Ok(())
}

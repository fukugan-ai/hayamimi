//! CS-28: the file that holds the Zoom token and the Teams link is read only
//! while no one but its owner can read it, and what it holds never reaches
//! the log.
//!
//! The destinations are the real ones, aimed at servers on 127.0.0.1. The
//! file's access is its mode here: owner only is private, and a mode that
//! lets the group or others read it is the "Users added" of the design. The
//! Windows access list is read through `icacls`; its parsing is tested on
//! recorded text in `access`, and the run against a real access list is not
//! done here.
#![cfg(unix)]

mod support;

use std::fs;

use hayamimi_relay::logging::Level;
use support::events::unused_port;
use support::private::PrivateDir;
use support::sinks::start;
use support::{FakeServer, Reply, TestResult};

const ID: &str = "SECRETID0123";
const SIG: &str = "SECRETSIG4567";

fn toml_with_token(address: &str) -> String {
    format!("[zoom]\ntoken = \"{address}\"\n")
}

fn token_at(base: &str) -> String {
    format!("{base}/closedcaption?id={ID}&sig={SIG}")
}

fn holds_no_secret(lines: &[String]) -> bool {
    lines
        .iter()
        .all(|line| !line.contains(ID) && !line.contains(SIG))
}

#[test]
fn cs28_send_config_is_private_and_never_logged() -> TestResult {
    let zoom = FakeServer::start(|_, _| Reply::status(200))?;
    let dir = PrivateDir::new("cs28")?;
    let path = dir.write(
        "send.toml",
        &toml_with_token(&token_at(&zoom.base_url())),
        0o600,
    )?;
    // The positive control: the file does hold the token, once.
    assert_eq!(fs::read_to_string(&path)?.matches(ID).count(), 1);
    let scene = start(&path)?;

    // Private: the caption goes, with the token in its address.
    scene.say(1);
    scene.wait_for_log("translate.send.ok", 1);
    let first = zoom.requests();
    assert_eq!(first.len(), 1);
    assert!(String::from_utf8_lossy(&first[0]).contains(ID));
    assert!(scene.running.logs.named("send.config.too_open").is_empty());

    // Others may read it now: nothing more is sent, and it is said as an error.
    dir.set_mode("send.toml", 0o644)?;
    scene.say(2);
    scene.wait_for_log("translate.send.failed", 1);
    scene.wait_for_log("send.config.too_open", 1);
    assert_eq!(zoom.requests().len(), 1);
    let said = scene.running.logs.named("send.config.too_open");
    assert!(!said.is_empty(), "no `send.config.too_open` was logged");
    for event in &said {
        assert_eq!(event.level, Level::Error);
        assert_eq!(event.component, "security");
        assert_eq!(event.fields["checked"], true);
        assert!(
            event
                .error_code
                .as_deref()
                .is_some_and(|code| code.ends_with("SEND_CONFIG_TOO_OPEN")),
            "error_code {:?}",
            event.error_code
        );
    }
    assert_eq!(scene.running.logs.named("sink.fallback.local").len(), 1);

    // Owner only again: the next caption goes, with no restart.
    dir.set_mode("send.toml", 0o600)?;
    scene.say(3);
    scene.wait_for_log("translate.send.ok", 2);
    assert_eq!(zoom.requests().len(), 2);
    scene.wait_for_log("sink.fallback.cleared", 1);

    // Whatever happened, the log never held the token.
    let lines = scene.running.logs.lines();
    assert!(lines.len() > 10, "the log held {} lines", lines.len());
    assert!(holds_no_secret(&lines));
    Ok(())
}

#[test]
fn cs28_a_failing_destination_never_writes_its_address_in_the_log() -> TestResult {
    let address = token_at(&format!("http://127.0.0.1:{}", unused_port()?));
    let dir = PrivateDir::new("cs28-fail")?;
    let path = dir.write("send.toml", &toml_with_token(&address), 0o600)?;
    let scene = start(&path)?;
    scene.say(1);
    scene.wait_for_log("translate.send.failed", 1);
    let failed = scene.running.logs.named("translate.send.failed");
    assert_eq!(failed[0].fields["reason"], "unreachable");
    assert!(holds_no_secret(&scene.running.logs.lines()));
    Ok(())
}

#[test]
fn cs28_a_value_that_cannot_be_parsed_is_not_quoted_by_the_log() -> TestResult {
    let zoom = FakeServer::start(|_, _| Reply::status(200))?;
    let dir = PrivateDir::new("cs28-bad")?;
    let good = toml_with_token(&token_at(&zoom.base_url()));
    let path = dir.write("send.toml", &good, 0o600)?;
    let scene = start(&path)?;
    scene.say(1);
    scene.wait_for_log("translate.send.ok", 1);
    // A broken edit that still holds the token: the old address stays in use.
    dir.write("send.toml", &format!("{good}oops oops\n"), 0o600)?;
    scene.say(2);
    scene.wait_for_log("translate.send.ok", 2);
    assert_eq!(zoom.requests().len(), 2);
    assert!(!scene.running.logs.named("table.load.failed").is_empty());
    assert!(holds_no_secret(&scene.running.logs.lines()));
    Ok(())
}

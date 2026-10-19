//! R12b: the control routes `GET /config`, `POST /config` and `POST /reset`
//! answer as the existing server did, over the real router served on a port
//! of its own with a stand-in for the recognition loop.
//!
//! Members the Rust build does not have (`lang` other than `ja`,
//! `dual_confirm`, `lid_switch_confirm`, `min_switch_s`, `translate`) are
//! null or refused by design, so the cases that set them on the Python
//! server set `punctuate` and `vad` here.

use std::error::Error;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use hayamimi_app::control::{Command, Controls, Initial, Live};
use hayamimi_app::routes::{self, AppState};
use hayamimi_app::server::{self, Hub};
use hayamimi_asr::itn::Overrides;
use hayamimi_asr::replace::Replacements;
use hayamimi_asr::vad::Sensitivity;
use serde_json::{Value, json};

type TestResult = Result<(), Box<dyn Error>>;

/// What a request got back.
struct Reply {
    status: u16,
    body: Value,
}

fn http(
    address: SocketAddr,
    method: &str,
    path: &str,
    body: &str,
) -> Result<Reply, Box<dyn Error>> {
    let mut stream = TcpStream::connect(address)?;
    stream.set_read_timeout(Some(Duration::from_secs(15)))?;
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\
         Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes())?;
    let mut text = String::new();
    stream.read_to_string(&mut text)?;
    let (head, payload) = text.split_once("\r\n\r\n").ok_or("no end of head")?;
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .ok_or("no status line")?
        .parse::<u16>()?;
    Ok(Reply {
        status,
        body: serde_json::from_str(payload)?,
    })
}

/// The routes served on a port of their own; the receiver is what the loop would read.
fn serve_routes() -> Result<(SocketAddr, Receiver<Command>), Box<dyn Error>> {
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
    let address = server::serve(routes::router(state), server::bind(0)?, "r12b-routes")?;
    Ok((address, received))
}

/// A stand-in loop that answers every reset and counts them on the returned receiver.
fn answer_and_count(commands: Receiver<Command>) -> Receiver<()> {
    let (counted, seen) = mpsc::channel();
    std::thread::spawn(move || {
        for command in commands {
            if let Command::Reset(done) = command {
                // The route may have stopped waiting; then nobody needs the answer.
                let _ = done.send(());
                let _ = counted.send(());
            }
        }
    });
    seen
}

fn error_text(reply: &Reply) -> Result<&str, Box<dyn Error>> {
    reply.body["error"]
        .as_str()
        .ok_or_else(|| "the reply carries no error text".into())
}

#[test]
fn get_config_reports_current_values() -> TestResult {
    let (address, _held) = serve_routes()?;
    let reply = http(address, "GET", "/config", "")?;
    assert_eq!(reply.status, 200);
    assert_eq!(
        reply.body,
        json!({
            "lang": "ja", "dual_confirm": null, "punctuate": true,
            "lid_switch_confirm": null, "min_switch_s": null, "translate": [],
            "vad": {"threshold": 0.5, "min_silence": 0.35, "max_speech": 12.0},
        })
    );
    Ok(())
}

#[test]
fn post_config_applies_a_subset_of_keys() -> TestResult {
    let (address, commands) = serve_routes()?;
    let reply = http(address, "POST", "/config", r#"{"punctuate":false}"#)?;
    assert_eq!(reply.status, 200);
    assert_eq!(reply.body, json!({"ok": true}));
    let shown = http(address, "GET", "/config", "")?.body;
    assert_eq!(shown["punctuate"], false);
    // A member that was not named stays as it was.
    assert_eq!(shown["vad"]["min_silence"], 0.35);
    assert!(matches!(commands.try_recv(), Ok(Command::Punctuate(false))));
    assert!(commands.try_recv().is_err());
    Ok(())
}

#[test]
fn post_config_invalid_value_returns_400_with_message() -> TestResult {
    let (address, commands) = serve_routes()?;
    let reply = http(address, "POST", "/config", r#"{"lang":"not-real"}"#)?;
    assert_eq!(reply.status, 400);
    assert!(error_text(&reply)?.contains("lang"), "{}", reply.body);
    // The refused value was never applied or handed to the loop.
    assert_eq!(http(address, "GET", "/config", "")?.body["lang"], "ja");
    assert!(commands.try_recv().is_err());
    Ok(())
}

#[test]
fn post_reset_calls_the_reset_function() -> TestResult {
    let (address, commands) = serve_routes()?;
    let seen = answer_and_count(commands);
    let reply = http(address, "POST", "/reset", "")?;
    assert_eq!(reply.status, 200);
    assert_eq!(reply.body, json!({"ok": true}));
    seen.recv_timeout(Duration::from_secs(5))?;
    assert!(seen.try_recv().is_err());
    Ok(())
}

#[test]
fn post_reset_pending_returns_202() -> TestResult {
    // Nobody reads the commands, as in a loop busy with a long decode.
    let (address, _held) = serve_routes()?;
    let reply = http(address, "POST", "/reset", "")?;
    assert_eq!(reply.status, 202);
    assert_eq!(reply.body, json!({"ok": false, "pending": true}));
    Ok(())
}

#[test]
fn post_config_invalid_vad_value_returns_400_not_a_crash() -> TestResult {
    let (address, commands) = serve_routes()?;
    let reply = http(address, "POST", "/config", r#"{"vad":{"threshold":"abc"}}"#)?;
    assert_eq!(reply.status, 400);
    assert!(error_text(&reply)?.contains("threshold"), "{}", reply.body);
    let unchanged = http(address, "GET", "/config", "")?.body;
    assert_eq!(unchanged["vad"]["threshold"], 0.5);

    let reply = http(address, "POST", "/config", r#"{"vad":{"min_silence":-1}}"#)?;
    assert_eq!(reply.status, 400);
    assert!(
        error_text(&reply)?.contains("min_silence"),
        "{}",
        reply.body
    );
    // Neither body reached the loop.
    assert!(commands.try_recv().is_err());
    Ok(())
}

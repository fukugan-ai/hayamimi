//! The checks of RP-29. Each reads a finished run, or a stream of events, and
//! returns an error that names what is wrong.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::scenes::{NEXT_PORTS, Scene, text_of};
use crate::support::{Failure, Output, Served, events, named};

fn check_exit_zero(output: &Output, scene: &Scene) -> Result<(), Failure> {
    if output.code == Some(0) {
        Ok(())
    } else {
        Err(format!(
            "{}: the run ended with {:?}, want 0",
            scene.name, output.code
        ))
    }
}

fn count_type(wire: &[Value], kind: &str) -> usize {
    wire.iter()
        .filter(|event| text_of(event, "type") == Some(kind))
        .count()
}

/// The log events of the scene, with its reason.
fn logged(all: &[Value], scene: &Scene) -> usize {
    named(all, scene.event)
        .into_iter()
        .filter(|event| text_of(event, "reason") == Some(scene.reason))
        .count()
}

/// The `fallback` events on `/events` for the scene's stage and destination.
fn sent(wire: &[Value], scene: &Scene) -> usize {
    wire.iter()
        .filter(|event| text_of(event, "type") == Some("fallback"))
        .filter(|event| text_of(event, "stage") == Some(scene.stage))
        .filter(|event| text_of(event, "to") == Some(scene.to))
        .count()
}

fn check_said_once(served: &Served, scene: &Scene) -> Result<(), Failure> {
    let all = events(&served.output.stdout)?;
    let in_log = logged(&all, scene);
    if in_log != 1 {
        return Err(format!(
            "{}: {in_log} `{}` events, want 1",
            scene.name, scene.event
        ));
    }
    let on_screen = sent(&served.wire, scene);
    if on_screen != 1 {
        return Err(format!(
            "{}: {on_screen} `fallback` events on /events, want 1",
            scene.name
        ));
    }
    Ok(())
}

/// The server is up: it sent `session_start` (or `ready`).
fn check_server_up(wire: &[Value], scene: &Scene) -> Result<(), Failure> {
    if count_type(wire, "session_start") + count_type(wire, "ready") == 0 {
        return Err(format!(
            "{}: the server sent neither session_start nor ready",
            scene.name
        ));
    }
    Ok(())
}

/// A run with the resource taken away: exit 0, the fall said once in both
/// places, and subtitles, or the server alone when `expects_final` is false.
pub fn check_resource_fallback(
    scene: &Scene,
    served: &Served,
    expects_final: bool,
) -> Result<(), Failure> {
    check_exit_zero(&served.output, scene)?;
    if expects_final && count_type(&served.wire, "final") == 0 {
        return Err(format!("{}: no `final` event came out", scene.name));
    }
    if !expects_final {
        check_server_up(&served.wire, scene)?;
    }
    check_said_once(served, scene)
}

/// A run with the resource present: nothing of the scene is said.
pub fn check_resource_present(scene: &Scene, served: &Served) -> Result<(), Failure> {
    check_exit_zero(&served.output, scene)?;
    let all = events(&served.output.stdout)?;
    let in_log = named(&all, scene.event).len();
    let on_screen = sent(&served.wire, scene);
    if in_log + on_screen != 0 {
        return Err(format!(
            "{}: the resource is present and {in_log} log and {on_screen} screen fallback events came out",
            scene.name
        ));
    }
    Ok(())
}

/// The actual port of a `server.fallback.port` event, read from its `url`.
pub fn port_of(event: &Value) -> Result<u16, Failure> {
    let url = text_of(event, "url").ok_or("the port event has no url")?;
    let after = url.rsplit(':').next().ok_or("the url has no port")?;
    let digits = after.trim_end_matches("/events");
    digits
        .parse::<u16>()
        .map_err(|error| format!("`{url}`: {error}"))
}

/// The server moved from the blocked port to one of the next ten.
pub fn check_port_move(blocked: u16, actual: u16) -> Result<(), Failure> {
    let last = blocked + NEXT_PORTS;
    if actual > blocked && actual <= last {
        Ok(())
    } else {
        Err(format!(
            "the server is on {actual}, outside {}..={last}",
            blocked + 1
        ))
    }
}

/// The settings in effect after a broken file equal the dump of the defaults.
pub fn check_defaults_match_dump(
    effective: &BTreeMap<String, Value>,
    dump: &BTreeMap<String, Value>,
) -> Result<(), Failure> {
    for (key, value) in dump {
        if effective.get(key) != Some(value) {
            return Err(format!("setting `{key}` differs from the dump"));
        }
    }
    Ok(())
}

/// A resource that came back is said cleared once in the log and once on the screen.
pub fn check_cleared(served: &Served, scene: &Scene) -> Result<(), Failure> {
    let cleared = scene.event.replace(".local", ".cleared");
    let all = events(&served.output.stdout)?;
    let in_log = named(&all, &cleared).len();
    let on_screen = served
        .wire
        .iter()
        .filter(|event| text_of(event, "type") == Some("fallback"))
        .filter(|event| text_of(event, "stage") == Some(scene.stage))
        .filter(|event| event.get("to").is_some_and(Value::is_null))
        .count();
    if in_log != 1 || on_screen != 1 {
        return Err(format!(
            "{}: {in_log} cleared log and {on_screen} cleared screen events, want 1 and 1",
            scene.name
        ));
    }
    Ok(())
}

/// A GPU stage stays on the CPU when the device returns: no cleared event, every session on the CPU.
pub fn check_gpu_stays_on_cpu(served: &Served) -> Result<(), Failure> {
    let all = events(&served.output.stdout)?;
    if !named(&all, "gpu.fallback.cleared").is_empty() {
        return Err(String::from("a `gpu.fallback.cleared` event came out"));
    }
    match named(&all, "gpu.session.created")
        .iter()
        .find(|e| text_of(e, "actual_provider") != Some("cpu"))
    {
        Some(event) => Err(format!("{:?} left the CPU", text_of(event, "model"))),
        None => Ok(()),
    }
}

/// A client that connects again receives the fallbacks in effect first.
pub fn check_replay(stream: &[Value], active: &[&str]) -> Result<(), Failure> {
    let head: Vec<&Value> = stream.iter().take(active.len()).collect();
    for stage in active {
        let found = head.iter().any(|event| {
            text_of(event, "type") == Some("fallback") && text_of(event, "stage") == Some(stage)
        });
        if !found {
            return Err(format!(
                "the active fallback of stage `{stage}` did not come first"
            ));
        }
    }
    Ok(())
}

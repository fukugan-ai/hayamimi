//! Saying what happened on the three channels of the design: the log (a
//! JSON event on stdout), the screen (`/events`) and the user (stderr, in the
//! reader's locale). Every channel gets the same fact from the same value.

use std::sync::mpsc::Receiver;
use std::thread::JoinHandle;

use hayamimi_asr::{Engine, SessionReport};

use crate::capture::supervise::Event;
use crate::log;
use crate::notice::{Cleared, Notice};
use crate::server::Hub;
use crate::strings::Strings;

/// Prints table texts to stderr, one per line.
pub fn say(strings: &Strings, keys: &[&str]) {
    for key in keys {
        eprintln!("{}", strings.cli_text(key));
    }
}

/// Says one fallback on the three channels.
pub fn say_notice(strings: &Strings, hub: &Hub, notice: &Notice) {
    log::print(&log::fallback_line(strings, notice));
    hub.publish_fallback(&notice.wire());
    for key in notice.stderr_keys() {
        eprintln!("{}", strings.cli_text(&key));
    }
    if let Some(url) = &notice.url {
        // An address is data, not a sentence of the table.
        eprintln!("{url}");
    }
}

/// Logs one session an engine created.
pub fn log_session(strings: &Strings, report: &SessionReport) {
    log::print(&log::session_created_line(strings, report));
    if let Some(line) = log::session_placed_line(strings, report) {
        log::print(&line);
    }
}

/// Logs the sessions an opened engine created.
pub fn log_sessions(strings: &Strings, engine: &Engine) {
    for report in engine.sessions() {
        log_session(strings, report);
    }
}

/// Says what the recognizers and the route did since the last call: models
/// loaded and dropped in the log, tiers that gave way on the three channels,
/// the sentences the early path kept, with the reason, and the segments that
/// read as nothing, in the log.
pub fn tell_resident(strings: &Strings, hub: &Hub, engine: &mut Engine) {
    for hold in engine.take_early_holds() {
        log::print(&log::endpoint_held_line(strings, &hold));
    }
    for unread in engine.take_unread() {
        log::print(&log::segment_unread_line(strings, &unread));
    }
    for event in engine.take_resident_events() {
        if let Some(line) = log::resident_line(strings, &event) {
            log::print(&line);
        }
        if let Some(notice) = Notice::from_resident(&event) {
            say_notice(strings, hub, &notice);
        }
    }
    for step in engine.take_route_notices() {
        say_notice(strings, hub, &Notice::from_route(&step));
    }
}

/// Says that a stage is back on its first road, on the three channels.
pub fn say_cleared(strings: &Strings, hub: &Hub, cleared: &Cleared) {
    log::print(&log::cleared_line(strings, cleared));
    hub.publish_fallback(&cleared.wire());
    eprintln!("{}", strings.message_for(cleared.event));
}

/// Says what the capture supervisor reports.
fn report_capture(strings: &Strings, hub: &Hub, event: Event) {
    match event {
        Event::Opened { kind, device } => {
            log::print(&log::device_opened_line(strings, kind.name(), &device));
        }
        Event::Single => say_notice(strings, hub, &Notice::capture_single()),
        Event::NoSource => say_notice(strings, hub, &Notice::capture_none()),
        Event::Cleared => say_cleared(strings, hub, &Cleared::capture()),
    }
}

/// Reports the capture events from a thread of its own, until the capture ends.
pub fn spawn_capture_reporter(
    strings: Strings,
    hub: Hub,
    events: Receiver<Event>,
) -> std::io::Result<JoinHandle<()>> {
    std::thread::Builder::new()
        .name(String::from("capture-report"))
        .spawn(move || {
            for event in events {
                report_capture(&strings, &hub, event);
            }
        })
}

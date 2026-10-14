//! Saying what happened on the three channels of the design: the log (a
//! JSON event on stdout), the screen (`/events`) and the user (stderr, in the
//! reader's locale). Every channel gets the same fact from the same value.

use std::sync::mpsc::Receiver;
use std::thread::JoinHandle;

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

//! The relay's translations, read from its `GET /events` while `stream` plays
//! a recording.
//!
//! The relay sends its translations on a stream of its own, not on the
//! recognizer's `/ingest` connection. This reads that stream beside the
//! ingest connection and stamps each event with the same clock, so that the
//! two sets of events can be put in time order.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use hayamimi_relay::follow::{Backoff, follow};
use hayamimi_relay::logging::{LogEvent, LogSink};

use crate::error::EvalError;
use crate::ingest::micros_since;
use crate::log;

/// How long the relay has to answer before the run is refused.
const OPEN_WITHIN: Duration = Duration::from_secs(5);

type Events = Arc<Mutex<Vec<(i64, String)>>>;

fn locked(events: &Mutex<Vec<(i64, String)>>) -> MutexGuard<'_, Vec<(i64, String)>> {
    match events.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Sets a flag once the stream is open.
struct OpenedFlag(Arc<AtomicBool>);

impl LogSink for OpenedFlag {
    fn emit(&mut self, event: LogEvent) {
        if event.event == "events.subscribe.opened" {
            self.0.store(true, Ordering::SeqCst);
        }
    }
}

/// The relay's stream, open and being read.
pub struct Watch {
    events: Events,
}

impl Watch {
    /// Opens `GET /events` of the relay at `host:port` and starts reading it
    /// from a thread that ends with the process. Times are measured from
    /// `origin`. A relay that is not there is a `Connect` error, before
    /// anything is played.
    pub fn open(host: &str, port: u16, origin: Instant) -> Result<Watch, EvalError> {
        let events = Events::default();
        let opened = Arc::new(AtomicBool::new(false));
        let (collected, flag) = (Arc::clone(&events), Arc::clone(&opened));
        let url = format!("http://{host}:{port}/events");
        std::thread::spawn(move || {
            let mut on_event = |data: &str, _: &mut dyn LogSink| {
                locked(&collected).push((micros_since(origin), data.to_owned()));
            };
            match follow(
                (0, &url),
                Backoff::STANDARD,
                &mut OpenedFlag(flag),
                &mut on_event,
            ) {}
        });
        let deadline = Instant::now() + OPEN_WITHIN;
        while !opened.load(Ordering::SeqCst) {
            if Instant::now() >= deadline {
                return Err(EvalError::Connect {
                    address: format!("{host}:{port}"),
                    source: io::Error::new(
                        io::ErrorKind::TimedOut,
                        "the relay's /events did not open",
                    ),
                });
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        log::info(
            "eval.relay.connected",
            "transport",
            "relay stream opened",
            &[],
        );
        Ok(Watch { events })
    }

    /// Every event received so far, with its arrival time in microseconds
    /// since the origin.
    pub fn events(&self) -> Vec<(i64, String)> {
        locked(&self.events).clone()
    }
}

/// The events of two streams in the order they arrived.
pub fn in_time_order(
    mut first: Vec<(i64, String)>,
    second: Vec<(i64, String)>,
) -> Vec<(i64, String)> {
    first.extend(second);
    first.sort_by_key(|(t_us, _)| *t_us);
    first
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// A relay that sends `body` after the head and then holds the stream open.
    fn serve(body: &'static str) -> io::Result<u16> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut request = [0_u8; 1024];
                if stream.read(&mut request).is_err() {
                    continue;
                }
                let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";
                if write!(stream, "{head}{body}").is_err() {
                    continue;
                }
                std::thread::sleep(Duration::from_secs(5));
            }
        });
        Ok(port)
    }

    fn wait_for(watch: &Watch, count: usize) -> Vec<(i64, String)> {
        let deadline = Instant::now() + Duration::from_secs(5);
        while watch.events().len() < count && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        watch.events()
    }

    #[test]
    fn the_translations_of_the_relay_are_read_with_the_time_they_came()
    -> Result<(), Box<dyn std::error::Error>> {
        let body = "data: {\"type\":\"translation\",\"id\":\"mic-1\",\"text\":\"Hi.\"}\n\ndata: {\"type\":\"translation\",\"id\":\"mic-2\",\"text\":\"Bye.\"}\n\n";
        let port = serve(body)?;
        let watch = Watch::open("127.0.0.1", port, Instant::now())?;
        let got = wait_for(&watch, 2);
        assert_eq!(got.len(), 2);
        assert!(got[0].1.contains("mic-1") && got[1].1.contains("mic-2"));
        assert!(got[0].0 >= 0 && got[0].0 <= got[1].0);
        Ok(())
    }

    #[test]
    fn a_relay_that_is_not_there_is_a_connect_error() -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        drop(listener);
        let said = Watch::open("127.0.0.1", port, Instant::now()).err();
        assert!(matches!(said, Some(EvalError::Connect { .. })), "{said:?}");
        Ok(())
    }

    #[test]
    fn two_streams_are_put_in_time_order() {
        let ingest = vec![(10, String::from("a")), (40, String::from("d"))];
        let relay = vec![
            (20, String::from("b")),
            (30, String::from("c")),
            (50, String::from("e")),
        ];
        let merged = in_time_order(ingest, relay);
        let order: Vec<&str> = merged.iter().map(|(_, text)| text.as_str()).collect();
        assert_eq!(order, ["a", "b", "c", "d", "e"]);
    }

    #[test]
    fn events_that_arrive_together_keep_the_order_of_the_first_stream_first() {
        let merged = in_time_order(
            vec![(5, String::from("ingest"))],
            vec![(5, String::from("relay"))],
        );
        assert_eq!(merged[0].1, "ingest");
    }
}

//! The three `/events` streams, read on native threads.
//!
//! Each stream has a thread of its own that follows it for as long as the feed
//! runs: a stream that ends or cannot be opened is opened again after a wait
//! that grows from 250 ms to 5 s (`follow_while`), and the lines the window
//! already shows stay where they are meanwhile. A message that parses is handed
//! to `deliver`, which is what carries it from the reading thread to the
//! window; one that does not is logged without its text and skipped.

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use hayamimi_relay::follow::{Backoff, follow_while};
use hayamimi_relay::logging::{Level, LogEvent, LogSink, SharedSink};
use hayamimi_ui::wire::{self, UiEvent};

/// What takes a message from a reading thread to the window.
pub type Deliver = Arc<dyn Fn(UiEvent) + Send + Sync>;

/// The readers of the streams. Dropping it stops them: each ends when its
/// connection next ends.
pub struct Feed {
    running: Arc<AtomicBool>,
}

/// What one reading thread works with.
struct Reader {
    endpoint: usize,
    url: String,
    backoff: Backoff,
    running: Arc<AtomicBool>,
    deliver: Deliver,
    sink: SharedSink,
}

impl Feed {
    /// Starts one reader for each of `urls`, numbered from 0 in the order
    /// given; the log names a stream by that number.
    pub fn start(
        urls: &[String],
        backoff: Backoff,
        sink: &SharedSink,
        deliver: &Deliver,
    ) -> io::Result<Feed> {
        let running = Arc::new(AtomicBool::new(true));
        // A reader that cannot be started drops the feed, which stops the ones before it.
        let feed = Feed {
            running: Arc::clone(&running),
        };
        for (endpoint, url) in urls.iter().enumerate() {
            spawn(Reader {
                endpoint,
                url: url.clone(),
                backoff,
                running: Arc::clone(&running),
                deliver: Arc::clone(deliver),
                sink: sink.clone(),
            })?;
        }
        Ok(feed)
    }
}

impl Drop for Feed {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

fn spawn(reader: Reader) -> io::Result<()> {
    std::thread::Builder::new()
        .name(format!("overlay-feed-{}", reader.endpoint))
        .spawn(move || run(reader))
        .map(drop)
}

fn run(reader: Reader) {
    let Reader {
        endpoint,
        url,
        backoff,
        running,
        deliver,
        mut sink,
    } = reader;
    let mut skips = SkipCount::default();
    let mut on_event = |data: &str, sink: &mut dyn LogSink| {
        if running.load(Ordering::SeqCst) {
            hand_over(data, endpoint, &deliver, &mut skips, sink);
        }
    };
    follow_while(
        &running,
        (endpoint, &url),
        backoff,
        &mut sink,
        &mut on_event,
    );
}

fn hand_over(
    data: &str,
    endpoint: usize,
    deliver: &Deliver,
    skips: &mut SkipCount,
    sink: &mut dyn LogSink,
) {
    match wire::parse(data) {
        Ok(event) => deliver(event),
        Err(_) => {
            if let Some(count) = skips.note(Instant::now()) {
                sink.emit(skipped(endpoint, count));
            }
        }
    }
}

/// A stream says that it skipped lines at most this often.
const SKIP_REPORT_EVERY: Duration = Duration::from_millis(60_000);

/// The lines of one stream that could not be read, counted between reports.
#[derive(Default)]
struct SkipCount {
    last_report: Option<Instant>,
    pending: u64,
}

impl SkipCount {
    /// Notes one line that could not be read, at `now`. When a report is due
    /// (the first, or one `SKIP_REPORT_EVERY` after the last) returns how
    /// many lines it covers: this one and those since the last report.
    fn note(&mut self, now: Instant) -> Option<u64> {
        self.pending += 1;
        let due = self
            .last_report
            .is_none_or(|at| now.duration_since(at) >= SKIP_REPORT_EVERY);
        if !due {
            return None;
        }
        self.last_report = Some(now);
        Some(std::mem::take(&mut self.pending))
    }
}

fn skipped(endpoint: usize, count: u64) -> LogEvent {
    LogEvent::new(Level::Warn, "events.line.skipped", "transport")
        .with("endpoint", endpoint)
        .with("reason", "not_a_message")
        .with("skipped", count)
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::Mutex;
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    type Failure = Box<dyn std::error::Error>;

    const QUICK: Backoff = Backoff {
        initial: Duration::from_millis(10),
        max: Duration::from_millis(20),
    };
    const PATIENCE: Duration = Duration::from_secs(10);

    const FINAL_A: &str = r#"{"type":"final","id":"mic-1","text":"hello","lang":"en"}"#;
    const FINAL_B: &str = r#"{"type":"final","id":"loopback-2","text":"world","lang":"en"}"#;

    /// Keeps what was logged where the test can read it.
    #[derive(Clone, Default)]
    struct Tap(Arc<Mutex<Vec<LogEvent>>>);

    impl Tap {
        fn kept(&self) -> std::sync::MutexGuard<'_, Vec<LogEvent>> {
            match self.0.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            }
        }

        fn named(&self, name: &str) -> usize {
            self.kept()
                .iter()
                .filter(|event| event.event == name)
                .count()
        }
    }

    impl LogSink for Tap {
        fn emit(&mut self, event: LogEvent) {
            self.kept().push(event);
        }
    }

    /// Answers each connection with the next of `bodies` as an event stream and
    /// closes it; after the last one nothing listens any more.
    fn serve(bodies: Vec<String>) -> Result<String, Failure> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let url = format!("http://{}/events", listener.local_addr()?);
        std::thread::spawn(move || {
            for body in bodies {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let mut head = BufReader::new(&stream);
                let mut line = String::new();
                while head.read_line(&mut line).is_ok_and(|n| n > 0) && !line.trim().is_empty() {
                    line.clear();
                }
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n{body}"
                );
                // A reader that went away leaves nobody to answer.
                let _ = stream.write_all(reply.as_bytes());
            }
        });
        Ok(url)
    }

    fn event_stream(messages: &[&str]) -> String {
        messages
            .iter()
            .map(|data| format!("data: {data}\n\n"))
            .collect()
    }

    fn channel() -> (Deliver, mpsc::Receiver<UiEvent>) {
        let (sender, receiver) = mpsc::channel();
        let deliver: Deliver = Arc::new(move |event| {
            // The test that listens may be over already.
            let _ = sender.send(event);
        });
        (deliver, receiver)
    }

    fn id_of(event: &UiEvent) -> Option<&str> {
        match event {
            UiEvent::Final(found) => found.id.as_deref(),
            _ => None,
        }
    }

    #[test]
    fn a_message_of_the_stream_reaches_deliver_parsed() -> Result<(), Failure> {
        let url = serve(vec![event_stream(&[FINAL_A])])?;
        let (deliver, receiver) = channel();
        let _feed = Feed::start(&[url], QUICK, &SharedSink::new(Tap::default()), &deliver)?;
        let event = receiver.recv_timeout(PATIENCE)?;
        assert_eq!(id_of(&event), Some("mic-1"));
        Ok(())
    }

    #[test]
    fn a_message_that_is_not_one_is_logged_and_skipped_and_the_next_arrives() -> Result<(), Failure>
    {
        let url = serve(vec![event_stream(&["not json at all", FINAL_B])])?;
        let (deliver, receiver) = channel();
        let tap = Tap::default();
        let _feed = Feed::start(&[url], QUICK, &SharedSink::new(tap.clone()), &deliver)?;
        let event = receiver.recv_timeout(PATIENCE)?;
        assert_eq!(id_of(&event), Some("loopback-2"));
        assert_eq!(tap.named("events.line.skipped"), 1);
        let line = tap
            .kept()
            .iter()
            .find(|event| event.event == "events.line.skipped")
            .map(LogEvent::to_json_line);
        assert!(
            line.as_deref()
                .is_some_and(|line| line.contains(r#""skipped":1"#)),
            "{line:?}"
        );
        Ok(())
    }

    #[test]
    fn the_first_unreadable_line_is_reported_at_once_with_a_count_of_one() {
        let start = Instant::now();
        assert_eq!(SkipCount::default().note(start), Some(1));
    }

    #[test]
    fn lines_within_a_minute_are_held_and_counted_into_the_next_report() {
        let start = Instant::now();
        let mut skips = SkipCount::default();
        assert_eq!(skips.note(start), Some(1));
        assert_eq!(skips.note(start + Duration::from_millis(10)), None);
        assert_eq!(skips.note(start + Duration::from_millis(59_999)), None);
        assert_eq!(skips.note(start + Duration::from_millis(60_000)), Some(3));
        assert_eq!(skips.note(start + Duration::from_millis(60_001)), None);
    }

    #[test]
    fn each_stream_has_its_own_count() {
        let start = Instant::now();
        let (mut one, mut two) = (SkipCount::default(), SkipCount::default());
        assert_eq!(one.note(start), Some(1));
        assert_eq!(two.note(start + Duration::from_millis(5)), Some(1));
    }

    #[test]
    fn a_stream_that_ends_is_opened_again_and_its_next_message_arrives() -> Result<(), Failure> {
        let url = serve(vec![event_stream(&[FINAL_A]), event_stream(&[FINAL_B])])?;
        let (deliver, receiver) = channel();
        let tap = Tap::default();
        let _feed = Feed::start(&[url], QUICK, &SharedSink::new(tap.clone()), &deliver)?;
        let first = receiver.recv_timeout(PATIENCE)?;
        let second = receiver.recv_timeout(PATIENCE)?;
        assert_eq!(id_of(&first), Some("mic-1"));
        assert_eq!(id_of(&second), Some("loopback-2"));
        assert!(tap.named("events.subscribe.lost") >= 1);
        Ok(())
    }

    #[test]
    fn a_stream_nobody_listens_on_is_retried_and_does_not_stop_the_others() -> Result<(), Failure> {
        // The address is bound and given up at once, so nothing answers there.
        let dead = {
            let listener = TcpListener::bind("127.0.0.1:0")?;
            format!("http://{}/events", listener.local_addr()?)
        };
        let live = serve(vec![event_stream(&[FINAL_A])])?;
        let (deliver, receiver) = channel();
        let tap = Tap::default();
        let _feed = Feed::start(
            &[dead, live],
            QUICK,
            &SharedSink::new(tap.clone()),
            &deliver,
        )?;
        let event = receiver.recv_timeout(PATIENCE)?;
        assert_eq!(id_of(&event), Some("mic-1"));
        Ok(())
    }

    #[test]
    fn a_dropped_feed_stops_delivering() -> Result<(), Failure> {
        let url = serve(vec![event_stream(&[FINAL_A]), event_stream(&[FINAL_B])])?;
        let (deliver, receiver) = channel();
        let feed = Feed::start(&[url], QUICK, &SharedSink::new(Tap::default()), &deliver)?;
        let first = receiver.recv_timeout(PATIENCE)?;
        assert_eq!(id_of(&first), Some("mic-1"));
        drop(feed);
        // The second connection may still be opened, but its message is not handed over.
        assert!(receiver.recv_timeout(Duration::from_millis(500)).is_err());
        Ok(())
    }
}

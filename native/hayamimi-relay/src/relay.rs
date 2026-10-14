//! The relay at work: it follows the event streams of the recognizers, runs
//! each line through the three stages in a lane of its own source, and
//! publishes every translation under the `id` of the line it belongs to.
//!
//! Lines of one source are translated in the order they arrive. Lines of
//! different sources do not wait for each other, so a slow line of the
//! microphone never holds back the loopback. A translation carries the `id`
//! of its own line, so one that arrives late cannot be read as another
//! line's.

use std::io;
use std::net::{SocketAddr, TcpListener};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Instant;

use crate::follow::{Backoff, follow};
use crate::hub::{self, Hub};
use crate::lang::Lang;
use crate::line::{Line, SOURCES, Source, parse_line};
use crate::llm::Llm;
use crate::logging::{Level, LogEvent, LogSink, SharedSink};
use crate::pipeline::{NoObserver, Pipeline};

/// How lines are shared out to translators.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lanes {
    /// A lane for each source.
    PerSource,
    /// One lane for every source.
    One,
}

impl Lanes {
    fn slot(self, source: Source) -> usize {
        match self {
            Lanes::PerSource => source.index(),
            Lanes::One => 0,
        }
    }
}

pub struct RelayConfig {
    /// The `/events` URLs to follow.
    pub events: Vec<String>,
    pub lanes: Lanes,
    pub backoff: Backoff,
    /// Unix seconds at the start of the run; later times count from it.
    pub started_epoch: i64,
}

/// Builds the pipeline of one lane. It runs in the lane's own thread.
pub type MakePipeline<L> =
    Arc<dyn Fn(&mut dyn LogSink) -> Result<Pipeline<L>, String> + Send + Sync>;

/// The time of day, counted from the start of the run so that it cannot step
/// backwards or fail while lines are being translated.
#[derive(Clone, Copy)]
struct RunClock {
    started_epoch: i64,
    started: Instant,
}

impl RunClock {
    fn new(started_epoch: i64) -> RunClock {
        RunClock {
            started_epoch,
            started: Instant::now(),
        }
    }

    fn now_epoch(&self) -> i64 {
        self.started_epoch
            .saturating_add_unsigned(self.started.elapsed().as_secs())
    }
}

/// A line and the language it is translated from.
struct Job {
    line: Line,
    src_lang: Lang,
}

fn line_event(level: Level, name: &str, component: &'static str, line: &Line) -> LogEvent {
    LogEvent::new(level, name, component)
        .with("id", line.id.as_str())
        .with("source", line.source.as_str())
}

/// Translates the jobs of one lane, one after another, until the router ends.
fn run_lane<L: Llm>(
    jobs: Receiver<Job>,
    make: &MakePipeline<L>,
    hub: &Hub,
    clock: RunClock,
    mut sink: SharedSink,
) {
    let mut pipeline = match make(&mut sink) {
        Ok(pipeline) => pipeline,
        Err(_) => {
            sink.emit(
                LogEvent::new(Level::Error, "translate.lane.failed", "worker").with_error("OPEN"),
            );
            return;
        }
    };
    for job in jobs {
        let request = job.line.request(job.src_lang, clock.now_epoch());
        let translation = pipeline.run(&request, &mut NoObserver, &mut sink);
        match serde_json::to_string(&translation) {
            Ok(json) => hub.publish(&json),
            Err(_) => sink.emit(
                line_event(
                    Level::Error,
                    "translate.publish.failed",
                    "service",
                    &job.line,
                )
                .with_error("PUBLISH"),
            ),
        }
    }
}

struct Router<L: Llm + 'static> {
    slots: [Option<Sender<Job>>; SOURCES],
    lanes: Lanes,
    make: MakePipeline<L>,
    hub: Hub,
    clock: RunClock,
    sink: SharedSink,
}

impl<L: Llm + 'static> Router<L> {
    fn open_lane(&self) -> Sender<Job> {
        let (jobs, receiver) = mpsc::channel();
        let (make, hub, clock, sink) = (
            Arc::clone(&self.make),
            self.hub.clone(),
            self.clock,
            self.sink.clone(),
        );
        std::thread::spawn(move || run_lane(receiver, &make, &hub, clock, sink));
        jobs
    }

    /// Hands a line to the lane of its source, opening the lane on first use.
    /// A line in a language the relay does not translate is only noted.
    fn dispatch(&mut self, line: Line) {
        let Some(src_lang) = line.source_language() else {
            self.sink.emit(
                line_event(Level::Info, "translate.line.skipped", "service", &line)
                    .with("reason", "language"),
            );
            return;
        };
        let slot = self.lanes.slot(line.source);
        let lane = match self.slots[slot].take() {
            Some(lane) => lane,
            None => self.open_lane(),
        };
        if let Err(refused) = lane.send(Job { line, src_lang }) {
            // The lane's pipeline could not be opened and its thread ended; the
            // next line opens the lane again.
            self.sink.emit(
                line_event(
                    Level::Error,
                    "translate.lane.failed",
                    "worker",
                    &refused.0.line,
                )
                .with_error("SEND"),
            );
            return;
        }
        self.slots[slot] = Some(lane);
    }

    fn run(mut self, incoming: Receiver<Line>) {
        for line in incoming {
            self.dispatch(line);
        }
    }
}

/// Hands a line of the stream to the router; reports an event that is a line
/// but cannot be used.
fn forward(data: &str, endpoint: usize, lines: &Sender<Line>, sink: &mut dyn LogSink) {
    match parse_line(data) {
        // The router ends only with the process, so a refused line has no one left to tell.
        Ok(Some(line)) => {
            let _ = lines.send(line);
        }
        Ok(None) => {}
        Err(reason) => sink.emit(
            LogEvent::new(Level::Warn, "events.line.skipped", "service")
                .with("endpoint", endpoint)
                .with("reason", reason.as_str()),
        ),
    }
}

fn spawn_follower(
    endpoint: usize,
    url: String,
    lines: Sender<Line>,
    backoff: Backoff,
    mut sink: SharedSink,
) -> io::Result<()> {
    std::thread::Builder::new()
        .name(format!("relay-follow-{endpoint}"))
        .spawn(move || {
            let mut on_event =
                |data: &str, sink: &mut dyn LogSink| forward(data, endpoint, &lines, sink);
            match follow((endpoint, &url), backoff, &mut sink, &mut on_event) {}
        })?;
    Ok(())
}

/// Starts the relay: serves the translations on `listener` and follows every
/// stream of `config.events`, all from threads of their own. Returns the
/// address the translations are served on.
pub fn start<L: Llm + 'static>(
    listener: TcpListener,
    config: RelayConfig,
    make: MakePipeline<L>,
    sink: SharedSink,
) -> io::Result<SocketAddr> {
    let hub = Hub::new();
    let address = hub::serve(listener, hub.clone())?;
    let (lines, incoming) = mpsc::channel();
    let router = Router {
        slots: [None, None, None],
        lanes: config.lanes,
        make,
        hub,
        clock: RunClock::new(config.started_epoch),
        sink: sink.clone(),
    };
    std::thread::Builder::new()
        .name(String::from("relay-router"))
        .spawn(move || router.run(incoming))?;
    for (endpoint, url) in config.events.into_iter().enumerate() {
        spawn_follower(endpoint, url, lines.clone(), config.backoff, sink.clone())?;
    }
    Ok(address)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_source_has_its_own_lane_unless_there_is_one_lane() {
        let sources = [Source::Mic, Source::Loopback, Source::File];
        let own: Vec<usize> = sources.iter().map(|s| Lanes::PerSource.slot(*s)).collect();
        assert_eq!(own, [0, 1, 2]);
        let shared: Vec<usize> = sources.iter().map(|s| Lanes::One.slot(*s)).collect();
        assert_eq!(shared, [0, 0, 0]);
    }

    #[test]
    fn every_lane_slot_is_inside_the_slot_array() {
        for source in [Source::Mic, Source::Loopback, Source::File] {
            assert!(Lanes::PerSource.slot(source) < SOURCES);
        }
    }

    #[test]
    fn the_clock_counts_from_the_start_and_never_goes_back() {
        let clock = RunClock::new(1_790_000_000);
        let (first, second) = (clock.now_epoch(), clock.now_epoch());
        assert!(first >= 1_790_000_000 && second >= first);
    }
}

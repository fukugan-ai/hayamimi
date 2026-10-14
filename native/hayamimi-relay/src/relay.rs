//! The relay at work: it follows the event streams of the recognizers, runs
//! each line through the three stages in a lane of its own source, and
//! publishes every translation under the `id` of the line it belongs to.
//!
//! Lines of one source are translated in the order they arrive. Lines of
//! different sources do not wait for each other, so a slow line of the
//! microphone never holds back the loopback. A translation carries the `id`
//! of its own line, so one that arrives late cannot be read as another
//! line's.
//!
//! A `final` line is published twice at most: its draft as soon as T's
//! translation passed V, and its settled translation at the end. Only the
//! settled one is handed to the outputs (`deliver`).

use std::io;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Instant;

use crate::deliver::{Delivery, Outputs, Words};
use crate::eval::skipped_event;
use crate::follow::{Backoff, follow_while};
use crate::hub::{self, Hub};
use crate::lang::Lang;
use crate::line::{Line, SOURCES, Source, parse_line};
use crate::llm::Llm;
use crate::logging::{Level, LogEvent, LogSink, SharedSink};
use crate::pipeline::{LineRequest, NoObserver, Pipeline, Translation};
use crate::strings::Strings;
use crate::target::Target;

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
    /// The `--translate` codes. `None` translates by the rule of each source
    /// (a Japanese line into English, an English line into Japanese). `Some`
    /// replaces the languages a Japanese line is translated into by these.
    /// A code outside the table is left out and said so.
    pub translate: Option<Vec<String>>,
}

/// Where a line of a source goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Route {
    /// The other side of the live pair, with unit notes.
    Pair,
    /// A language of `--translate` outside the live pair.
    Other(Target),
}

/// The routes of a line read in `src_lang`. An English line always goes to
/// the pair. A Japanese line goes to each language of `translate`, or to the
/// pair when there is no `--translate`.
fn routes(src_lang: Lang, translate: Option<&[Target]>) -> Vec<Route> {
    let (Lang::Ja, Some(targets)) = (src_lang, translate) else {
        return vec![Route::Pair];
    };
    targets
        .iter()
        .map(|target| match target.lang() {
            Some(_) => Route::Pair,
            None => Route::Other(*target),
        })
        .collect()
}

/// The languages of `codes` that the table has, once each and in the order
/// given, and the codes it has not.
fn split_codes(codes: &[String]) -> (Vec<Target>, Vec<String>) {
    let mut known: Vec<Target> = Vec::new();
    let mut unknown: Vec<String> = Vec::new();
    for code in codes {
        match Target::from_code(code) {
            Some(target) if known.contains(&target) => {}
            Some(target) => known.push(target),
            None if unknown.contains(code) => {}
            None => unknown.push(code.clone()),
        }
    }
    (known, unknown)
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
    route: Route,
}

fn line_event(level: Level, name: &str, component: &'static str, line: &Line) -> LogEvent {
    LogEvent::new(level, name, component)
        .with("id", line.id.as_str())
        .with("source", line.source.as_str())
}

/// Publishes one translation under the `id` of its line, and says so when it
/// cannot be written.
fn publish(hub: &Hub, translation: &Translation, line: &Line, sink: &mut dyn LogSink) {
    match serde_json::to_string(translation) {
        Ok(json) => hub.publish(&json),
        Err(_) => sink.emit(
            line_event(Level::Error, "translate.publish.failed", "service", line)
                .with_error("PUBLISH"),
        ),
    }
}

/// What a lane works with besides its jobs.
struct LaneParts<L: Llm> {
    make: MakePipeline<L>,
    hub: Hub,
    delivery: Delivery,
    clock: RunClock,
}

/// Translates the jobs of one lane, one after another, until the router ends.
fn run_lane<L: Llm>(jobs: Receiver<Job>, parts: &LaneParts<L>, mut sink: SharedSink) {
    let mut pipeline = match (parts.make)(&mut sink) {
        Ok(pipeline) => pipeline,
        Err(_) => {
            sink.emit(
                LogEvent::new(Level::Error, "translate.lane.failed", "worker").with_error("OPEN"),
            );
            return;
        }
    };
    let mut drafts = sink.clone();
    for job in jobs {
        let request = job.line.request(job.src_lang, parts.clock.now_epoch());
        let mut show_draft =
            |draft: Translation| publish(&parts.hub, &draft, &job.line, &mut drafts);
        let translation = translate(
            &mut pipeline,
            &request,
            job.route,
            &mut show_draft,
            &mut sink,
        );
        publish(&parts.hub, &translation, &job.line, &mut sink);
        // The subtitle file and the outside destinations carry the live pair
        // only; another language is for the screens.
        if job.route == Route::Pair {
            parts.delivery.submit(&job.line, &translation);
        }
    }
}

/// Runs one line along its route.
fn translate<L: Llm>(
    pipeline: &mut Pipeline<L>,
    request: &LineRequest<'_>,
    route: Route,
    early: &mut dyn FnMut(Translation),
    sink: &mut dyn LogSink,
) -> Translation {
    match route {
        Route::Pair => pipeline.run_staged(request, &mut NoObserver, early, sink),
        Route::Other(target) => pipeline.run_into(request, &target, early, sink),
    }
}

/// The lanes: one for each source (or one in all) for the live pair, and the
/// same again for the other languages, so that a slow Chinese translation
/// never holds back the English one.
const LANE_SLOTS: usize = SOURCES * 2;

struct Router<L: Llm + 'static> {
    slots: [Option<Sender<Job>>; LANE_SLOTS],
    lanes: Lanes,
    translate: Option<Vec<Target>>,
    make: MakePipeline<L>,
    hub: Hub,
    delivery: Delivery,
    clock: RunClock,
    sink: SharedSink,
}

impl<L: Llm + 'static> Router<L> {
    fn open_lane(&self) -> Sender<Job> {
        let (jobs, receiver) = mpsc::channel();
        let parts = LaneParts {
            make: Arc::clone(&self.make),
            hub: self.hub.clone(),
            delivery: self.delivery.clone(),
            clock: self.clock,
        };
        let sink = self.sink.clone();
        std::thread::spawn(move || run_lane(receiver, &parts, sink));
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
        for route in routes(src_lang, self.translate.as_deref()) {
            let job = Job {
                line: line.clone(),
                src_lang,
                route,
            };
            self.hand_over(job);
        }
    }

    /// Hands a job to the lane of its source and route.
    fn hand_over(&mut self, job: Job) {
        let slot = self.lanes.slot(job.line.source)
            + match job.route {
                Route::Pair => 0,
                Route::Other(_) => SOURCES,
            };
        let lane = match self.slots[slot].take() {
            Some(lane) => lane,
            None => self.open_lane(),
        };
        if let Err(refused) = lane.send(job) {
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

/// What a follower thread works with.
struct Follower {
    endpoint: usize,
    url: String,
    lines: Sender<Line>,
    backoff: Backoff,
    running: Arc<AtomicBool>,
}

fn spawn_follower(follower: Follower, mut sink: SharedSink) -> io::Result<()> {
    std::thread::Builder::new()
        .name(format!("relay-follow-{}", follower.endpoint))
        .spawn(move || {
            let mut on_event = |data: &str, sink: &mut dyn LogSink| {
                if follower.running.load(Ordering::SeqCst) {
                    forward(data, follower.endpoint, &follower.lines, sink);
                }
            };
            follow_while(
                &follower.running,
                (follower.endpoint, &follower.url),
                follower.backoff,
                &mut sink,
                &mut on_event,
            );
        })?;
    Ok(())
}

/// A relay that is running. Dropping it leaves the relay running, as the
/// process is what ends it; [`Relay::stop`] ends it earlier.
pub struct Relay {
    address: SocketAddr,
    running: Arc<AtomicBool>,
    skipped: Vec<String>,
}

impl Relay {
    /// The address the translations are served on.
    pub fn address(&self) -> SocketAddr {
        self.address
    }

    /// The `--translate` codes that the table has not and so are left out.
    pub fn skipped(&self) -> &[String] {
        &self.skipped
    }

    /// Stops the relay: the port is given up, no new line is read, and the
    /// lanes end once the lines they hold are translated. A stream that is
    /// open and silent is let go when it next ends.
    pub fn stop(self) {
        self.running.store(false, Ordering::SeqCst);
        // The accepting thread waits for a connection; this one is its last.
        // A refused connection means it has already gone.
        let _ = TcpStream::connect(self.address);
    }
}

/// The `fallback` message of a `--translate` code the table has not.
const SKIPPED_WIRE: &str =
    r#"{"type":"fallback","stage":"translate","reason":"unsupported_target","to":"skipped"}"#;

/// Says that `code` is left out: in the log, and on the stream as a
/// `fallback` that is kept for the clients that connect later. The kept
/// message is keyed by the code, so that each language is kept on its own and
/// none replaces the translator's own `translate` fallback.
fn say_skipped(code: &str, hub: &Hub, sink: &mut SharedSink) {
    sink.emit(skipped_event(code));
    hub.publish_fallback(&format!("translate:{code}"), true, SKIPPED_WIRE);
}

/// Starts the relay with no subtitle file and no outside destination.
pub fn start<L: Llm + 'static>(
    listener: TcpListener,
    config: RelayConfig,
    make: MakePipeline<L>,
    sink: SharedSink,
) -> io::Result<SocketAddr> {
    let strings = Strings::embedded(Lang::En).map_err(io::Error::other)?;
    let outputs = Outputs {
        destinations: Vec::new(),
        srt: None,
        words: Words::from_strings(&strings),
    };
    start_with(listener, config, make, sink, outputs)
}

/// Starts the relay like [`start_handle`] and returns only the address.
pub fn start_with<L: Llm + 'static>(
    listener: TcpListener,
    config: RelayConfig,
    make: MakePipeline<L>,
    sink: SharedSink,
    outputs: Outputs,
) -> io::Result<SocketAddr> {
    start_handle(listener, config, make, sink, outputs).map(|relay| relay.address)
}

/// Starts the relay: serves the translations on `listener` and follows every
/// stream of `config.events`, all from threads of their own. The settled
/// translations also go to `outputs`. Returns the handle of the running
/// relay.
pub fn start_handle<L: Llm + 'static>(
    listener: TcpListener,
    config: RelayConfig,
    make: MakePipeline<L>,
    mut sink: SharedSink,
    outputs: Outputs,
) -> io::Result<Relay> {
    let running = Arc::new(AtomicBool::new(true));
    let hub = Hub::new();
    let (translate, skipped) = match config.translate.as_deref().map(split_codes) {
        Some((known, unknown)) => (Some(known), unknown),
        None => (None, Vec::new()),
    };
    for code in &skipped {
        say_skipped(code, &hub, &mut sink);
    }
    let address = hub::serve_while(listener, hub.clone(), Arc::clone(&running))?;
    let delivery = Delivery::start(outputs, hub.clone(), sink.clone())?;
    let (lines, incoming) = mpsc::channel();
    let router = Router {
        slots: Default::default(),
        lanes: config.lanes,
        translate,
        make,
        hub,
        delivery,
        clock: RunClock::new(config.started_epoch),
        sink: sink.clone(),
    };
    std::thread::Builder::new()
        .name(String::from("relay-router"))
        .spawn(move || router.run(incoming))?;
    for (endpoint, url) in config.events.into_iter().enumerate() {
        let follower = Follower {
            endpoint,
            url,
            lines: lines.clone(),
            backoff: config.backoff,
            running: Arc::clone(&running),
        };
        spawn_follower(follower, sink.clone())?;
    }
    Ok(Relay {
        address,
        running,
        skipped,
    })
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

    fn codes(list: &[&str]) -> Vec<String> {
        list.iter().map(|code| (*code).to_owned()).collect()
    }

    fn targets(list: &[&str]) -> Vec<Target> {
        split_codes(&codes(list)).0
    }

    #[test]
    fn the_table_splits_the_codes_and_keeps_each_once_in_order() {
        let (known, unknown) = split_codes(&codes(&["zh", "xx", "ko", "zh", "ja", "xx"]));
        let known_codes: Vec<&str> = known.iter().map(|target| target.code).collect();
        assert_eq!(known_codes, ["zh", "ko"]);
        // Japanese is the source of a translation, never its target.
        assert_eq!(unknown, ["xx", "ja"]);
        assert_eq!(split_codes(&[]), (Vec::new(), Vec::new()));
    }

    #[test]
    fn without_translate_a_line_goes_to_the_pair() {
        assert_eq!(routes(Lang::Ja, None), [Route::Pair]);
        assert_eq!(routes(Lang::En, None), [Route::Pair]);
    }

    #[test]
    fn translate_replaces_the_targets_of_a_japanese_line() {
        let zh = targets(&["zh"]);
        let [Route::Other(only)] = routes(Lang::Ja, Some(&zh))[..] else {
            panic!("zh alone did not make one route to zh");
        };
        assert_eq!(only.code, "zh");
        let both = targets(&["en", "ko"]);
        let routed = routes(Lang::Ja, Some(&both));
        assert_eq!(routed.len(), 2);
        assert_eq!(routed[0], Route::Pair);
        assert!(matches!(routed[1], Route::Other(target) if target.code == "ko"));
    }

    #[test]
    fn an_english_line_is_not_moved_by_translate() {
        let zh = targets(&["zh"]);
        assert_eq!(routes(Lang::En, Some(&zh)), [Route::Pair]);
    }

    #[test]
    fn a_translate_with_nothing_the_table_has_routes_a_japanese_line_nowhere() {
        assert!(routes(Lang::Ja, Some(&[])).is_empty());
    }

    #[test]
    fn the_clock_counts_from_the_start_and_never_goes_back() {
        let clock = RunClock::new(1_790_000_000);
        let (first, second) = (clock.now_epoch(), clock.now_epoch());
        assert!(first >= 1_790_000_000 && second >= first);
    }
}

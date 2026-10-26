//! The relay at work: it follows the event streams of the recognizers, runs
//! each line through the three stages in a lane of its own source, and
//! publishes every translation under the `id` of the line it belongs to.
//!
//! Lines of one source are settled in the order they arrive. Lines of
//! different sources do not wait for each other, so a slow line of the
//! microphone never holds back the loopback. A translation carries the `id`
//! of its own line, so one that arrives late cannot be read as another
//! line's.
//!
//! A lane of the live pair has two threads: one asks T, the other asks E.
//! T of a line is asked while E of the line before it is still being
//! answered, so a draft is never queued behind an edit. Both ask the server
//! only while they hold one of its slots (`slots`), so no request is sent to
//! wait in the server's queue.
//!
//! A `final` line is published twice at most: its draft as soon as T's
//! translation passed V, and its settled translation at the end. Only the
//! settled one is handed to the outputs (`deliver`).
//!
//! When the translation of the text in progress is on, a worker of its own
//! translates what a recognizer's `partial` events say (`partial`). Those
//! translations are published to the screens and never handed to the outputs.

use std::io;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use crate::deliver::{Delivery, Outputs, Words};
use crate::eval::skipped_event;
use crate::follow::{Backoff, follow_while};
use crate::hub::{self, Hub};
use crate::lang::Lang;
use crate::line::{Heard, Line, SOURCES, Source, hear};
use crate::llm::Llm;
use crate::logging::{Level, LogEvent, LogSink, SharedSink};
use crate::partial::{Mailbox, PartialTranslation, Route as StreamRoute, Settling};
use crate::pipeline::{
    Drafted, LineKind, NoObserver, Observer, PartialOutcome, Pipeline, Reuse, Staged, Translation,
};
use crate::reach::{Change, Watch};
use crate::slots::{Held, LLAMA_SLOTS, ServerSlots};
use crate::strings::Strings;
use crate::target::Target;
use crate::verify_dump::VerifyDump;

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
    /// Where the translations handed to V are written for a developer, when
    /// the run was asked to.
    pub verify_dump: Option<VerifyDump>,
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

/// How the lanes build their pipelines: the live pair against one translator
/// and every other language against another.
pub struct Makers<L: Llm> {
    pub pair: MakePipeline<L>,
    pub other: MakePipeline<L>,
    /// What notices whether each translator can be reached, when the
    /// pipelines were built with translators wrapped through it.
    pub watches: Watches,
}

/// The watches on the two translators; a translator with none is not watched.
#[derive(Default)]
pub struct Watches {
    pub pair: Option<Watch>,
    pub other: Option<Watch>,
}

/// One translator for every language.
impl<L: Llm> From<MakePipeline<L>> for Makers<L> {
    fn from(make: MakePipeline<L>) -> Makers<L> {
        Makers {
            pair: Arc::clone(&make),
            other: make,
            watches: Watches::default(),
        }
    }
}

impl<L: Llm> Makers<L> {
    fn for_route(&self, route: Route) -> MakePipeline<L> {
        match route {
            Route::Pair => Arc::clone(&self.pair),
            Route::Other(_) => Arc::clone(&self.other),
        }
    }
}

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
    /// The hold the line keeps on its source's text in progress until its
    /// translation is made; only the live pair's job has one, as only the pair
    /// shares the server with the text in progress.
    settling: Option<Settling>,
}

/// A line of a stream, with the hold its `final` took when the stream said it
/// came. The hold travels with the line so that it is taken before the text
/// that follows the `final` on the stream is read.
struct Arrival {
    line: Line,
    settling: Option<Settling>,
}

fn line_event(level: Level, name: &str, component: &'static str, line: &Line) -> LogEvent {
    LogEvent::new(level, name, component)
        .with("id", line.id.as_str())
        .with("source", line.source.as_str())
}

/// Publishes one translation under the `id` of its line, and says so when it
/// cannot be written.
fn publish(hub: &Hub, translation: &Translation, sink: &mut dyn LogSink) {
    match serde_json::to_string(translation) {
        Ok(json) => hub.publish(&json),
        Err(_) => sink.emit(
            LogEvent::new(Level::Error, "translate.publish.failed", "service")
                .with("id", translation.id.as_str())
                .with("source", translation.source.as_str())
                .with_error("PUBLISH"),
        ),
    }
}

/// What the threads of a lane work with besides their jobs.
struct LaneParts<L: Llm> {
    make: MakePipeline<L>,
    hub: Hub,
    delivery: Delivery,
    clock: RunClock,
    verify_dump: Option<VerifyDump>,
    /// The slots of the server the live pair is translated on; `None` for a
    /// lane of another language, which has a server of its own.
    slots: Option<Arc<ServerSlots>>,
    /// Where the answer for a text in progress that a `final` only closes
    /// waits for the line; `None` when the text in progress is not translated.
    mailbox: Option<Arc<Mailbox>>,
}

/// A line that has been through T, on its way to the thread that asks E.
struct Pending {
    job: Job,
    now_epoch: i64,
    staged: Staged,
}

/// Opens the pipeline of a thread, and says so when it cannot be opened.
fn open_pipeline<L: Llm>(make: &MakePipeline<L>, sink: &mut SharedSink) -> Option<Pipeline<L>> {
    match make(sink) {
        Ok(pipeline) => Some(pipeline),
        Err(_) => {
            sink.emit(
                LogEvent::new(Level::Error, "translate.lane.failed", "worker").with_error("OPEN"),
            );
            None
        }
    }
}

/// How long a request waits for a slot before the log says so.
const SLOT_WAIT_NOTED: Duration = Duration::from_millis(1);

/// Takes a slot of the server, and notes in the log when that took a wait.
fn take_slot<'a>(
    slots: &'a ServerSlots,
    id: &str,
    stage: &'static str,
    sink: &mut dyn LogSink,
) -> Held<'a> {
    let (held, waited) = slots.take();
    if waited >= SLOT_WAIT_NOTED {
        sink.emit(
            LogEvent::new(Level::Debug, "translate.slot.waited", "worker")
                .with("id", id)
                .with("stage", stage)
                .with("duration_ms", u64::try_from(waited.as_millis()).ok()),
        );
    }
    held
}

/// Runs `run` with the observer of the line `id`: the developer's file when
/// the run was asked to write one, otherwise one that sees nothing.
fn with_observer<L: Llm, R>(
    parts: &LaneParts<L>,
    id: &str,
    run: impl FnOnce(&mut dyn Observer) -> R,
) -> R {
    match parts.verify_dump.as_ref() {
        Some(dump) => run(&mut dump.observer(id)),
        None => run(&mut NoObserver),
    }
}

/// The answer for the text in progress that the `final` of `job` only closes,
/// when the line is of the live pair and there is one. Waits for a request
/// that still runs; holds no slot of the server while it does.
fn reusable_answer<L: Llm>(parts: &LaneParts<L>, job: &Job) -> Option<Reuse> {
    if job.route != Route::Pair || job.line.kind != LineKind::Final {
        return None;
    }
    let mailbox = parts.mailbox.as_deref()?;
    mailbox.claim(job.line.source, &job.line.id)
}

/// Asks T for the line of `job`, along its route, and publishes the draft
/// when there is one. A line whose text only closes its last text in progress
/// takes the answer of that text over and does not ask T.
fn draft_line<L: Llm>(
    pipeline: &mut Pipeline<L>,
    parts: &LaneParts<L>,
    job: &Job,
    now_epoch: i64,
    sink: &mut SharedSink,
) -> Staged {
    let request = job.line.request(job.src_lang, now_epoch);
    let mut drafts = sink.clone();
    let mut show_draft = |draft: Translation| publish(&parts.hub, &draft, &mut drafts);
    if let Some(reuse) = reusable_answer(parts, job) {
        return with_observer(parts, &job.line.id, |observer| {
            pipeline.draft_staged_reusing(&request, observer, &mut show_draft, &reuse, sink)
        });
    }
    let _slot = parts
        .slots
        .as_deref()
        .map(|slots| take_slot(slots, &job.line.id, "t", sink));
    with_observer(parts, &job.line.id, |observer| match job.route {
        Route::Pair => pipeline.draft_staged(&request, observer, &mut show_draft, sink),
        Route::Other(target) => {
            Staged::Settled(pipeline.run_into(&request, &target, &mut show_draft, sink))
        }
    })
}

/// Asks E for a line T drafted and returns the settled translation.
fn edit_line<L: Llm>(
    pipeline: &mut Pipeline<L>,
    parts: &LaneParts<L>,
    job: &Job,
    now_epoch: i64,
    drafted: Drafted,
    sink: &mut SharedSink,
) -> Translation {
    let request = job.line.request(job.src_lang, now_epoch);
    let _slot = parts
        .slots
        .as_deref()
        .map(|slots| take_slot(slots, &job.line.id, "e", sink));
    with_observer(parts, &job.line.id, |observer| {
        pipeline.finish_staged(&request, drafted, observer, sink)
    })
}

/// Settles a line (asking E if T left it to E), publishes the settled
/// translation and hands it to the outputs.
fn settle_line<L: Llm>(
    pipeline: &mut Pipeline<L>,
    parts: &LaneParts<L>,
    pending: Pending,
    sink: &mut SharedSink,
) {
    let Pending {
        mut job,
        now_epoch,
        staged,
    } = pending;
    let translation = match staged {
        Staged::Settled(done) => done,
        Staged::Drafted(drafted) => edit_line(pipeline, parts, &job, now_epoch, drafted, sink),
    };
    // The server is free of the line once its translation is made, so the text
    // in progress may be asked for again before the screens are told.
    drop(job.settling.take());
    publish(&parts.hub, &translation, sink);
    // The subtitle file and the outside destinations carry the live pair
    // only; another language is for the screens.
    if job.route == Route::Pair {
        parts.delivery.submit(&job.line, &translation);
    }
}

/// Settles the lines the thread of T hands over, one after another, until
/// that thread ends. Every line of the lane passes through here, so the
/// settled translations of a lane come in the order its lines arrived.
fn run_edits<L: Llm>(pending: Receiver<Pending>, parts: &LaneParts<L>, mut sink: SharedSink) {
    let Some(mut pipeline) = open_pipeline(&parts.make, &mut sink) else {
        return;
    };
    for line in pending {
        settle_line(&mut pipeline, parts, line, &mut sink);
    }
}

/// Starts the thread that asks E for the lane. `None` when it cannot be
/// started; the lane then asks E itself.
fn spawn_editor<L: Llm + 'static>(
    parts: &Arc<LaneParts<L>>,
    sink: &mut SharedSink,
) -> Option<Sender<Pending>> {
    let (hand_over, pending) = mpsc::channel();
    let (parts, thread_sink) = (Arc::clone(parts), sink.clone());
    let started = std::thread::Builder::new()
        .name(String::from("relay-edit"))
        .spawn(move || run_edits(pending, &parts, thread_sink));
    if started.is_err() {
        sink.emit(
            LogEvent::new(Level::Error, "translate.lane.failed", "worker").with_error("SPAWN"),
        );
        return None;
    }
    Some(hand_over)
}

/// Hands a line to the thread that asks E. A line that cannot be handed over
/// (the lane has no such thread, or it has ended) is settled here.
fn pass_on<L: Llm>(
    pending: Pending,
    editor: Option<&Sender<Pending>>,
    pipeline: &mut Pipeline<L>,
    parts: &LaneParts<L>,
    sink: &mut SharedSink,
) {
    let unsent = match editor {
        Some(editor) => match editor.send(pending) {
            Ok(()) => return,
            Err(refused) => refused.0,
        },
        None => pending,
    };
    settle_line(pipeline, parts, unsent, sink);
}

/// Asks T for the jobs of one lane, one after another, until the router
/// ends. A lane of the live pair hands each line on to a thread of its own
/// that asks E, so that the next line's T is not queued behind this line's E.
fn run_lane<L: Llm + 'static>(
    jobs: Receiver<Job>,
    parts: Arc<LaneParts<L>>,
    route: Route,
    mut sink: SharedSink,
) {
    let Some(mut pipeline) = open_pipeline(&parts.make, &mut sink) else {
        return;
    };
    let editor = match route {
        Route::Pair => spawn_editor(&parts, &mut sink),
        Route::Other(_) => None,
    };
    for job in jobs {
        let now_epoch = parts.clock.now_epoch();
        let staged = draft_line(&mut pipeline, &parts, &job, now_epoch, &mut sink);
        let pending = Pending {
            job,
            now_epoch,
            staged,
        };
        pass_on(pending, editor.as_ref(), &mut pipeline, &parts, &mut sink);
    }
}

/// The lanes: one for each source (or one in all) for the live pair, and the
/// same again for the other languages, so that a slow Chinese translation
/// never holds back the English one.
const LANE_SLOTS: usize = SOURCES * 2;

/// Translates the text in progress, the oldest left first, until the process
/// ends. A translation is published only if its text is still the newest of
/// its source when it comes.
fn run_partials<L: Llm>(
    mailbox: &Mailbox,
    hub: &Hub,
    make: &MakePipeline<L>,
    slots: &ServerSlots,
    clock: RunClock,
    mut sink: SharedSink,
) {
    let Some(mut pipeline) = open_pipeline(make, &mut sink) else {
        return;
    };
    loop {
        let (job, wanted) = mailbox.take();
        let request = job.request(clock.now_epoch());
        let _slot = take_slot(slots, request.id, "partial", &mut sink);
        mailbox.begin(job.source, wanted.generation());
        let (outcome, answer) = pipeline.translate_partial_kept(&request, &wanted, &mut sink);
        // A line that took the request over waits for this; hand it the answer first.
        mailbox.finish(job.source, wanted.generation(), answer);
        if let PartialOutcome::Shown(translation) = outcome
            && wanted.newest()
        {
            show_partial(mailbox, hub, &job, &translation, &mut sink);
        }
    }
}

/// Publishes the translation of a text in progress unless it would take away
/// characters the screen already drew in the settled form; then the screen
/// keeps the translation it has.
fn show_partial(
    mailbox: &Mailbox,
    hub: &Hub,
    job: &crate::partial::Job,
    translation: &Translation,
    sink: &mut dyn LogSink,
) {
    if mailbox.admit(job.source, &translation.id, &translation.text) {
        publish(hub, translation, sink);
        return;
    }
    sink.emit(
        LogEvent::new(Level::Debug, "translate.partial.kept", "service")
            .with("id", translation.id.as_str())
            .with("source", translation.source.as_str()),
    );
}

fn spawn_partials<L: Llm + 'static>(
    mailbox: Arc<Mailbox>,
    hub: Hub,
    make: MakePipeline<L>,
    llama: Arc<ServerSlots>,
    clock: RunClock,
    sink: SharedSink,
) -> io::Result<()> {
    std::thread::Builder::new()
        .name(String::from("relay-partial"))
        .spawn(move || run_partials(&mailbox, &hub, &make, &llama, clock, sink))?;
    Ok(())
}

struct Router<L: Llm + 'static> {
    slots: [Option<Sender<Job>>; LANE_SLOTS],
    lanes: Lanes,
    translate: Option<Vec<Target>>,
    makers: Makers<L>,
    hub: Hub,
    delivery: Delivery,
    clock: RunClock,
    verify_dump: Option<VerifyDump>,
    /// The slots of the live pair's server, shared by its lanes and the
    /// translation of the text in progress.
    llama: Arc<ServerSlots>,
    /// The text in progress and its answers; `None` when it is not translated.
    mailbox: Option<Arc<Mailbox>>,
    sink: SharedSink,
}

impl<L: Llm + 'static> Router<L> {
    fn open_lane(&self, route: Route) -> Sender<Job> {
        let (jobs, receiver) = mpsc::channel();
        let parts = Arc::new(LaneParts {
            make: self.makers.for_route(route),
            hub: self.hub.clone(),
            delivery: self.delivery.clone(),
            clock: self.clock,
            verify_dump: self.verify_dump.clone(),
            slots: matches!(route, Route::Pair).then(|| Arc::clone(&self.llama)),
            mailbox: self.mailbox.clone(),
        });
        let sink = self.sink.clone();
        std::thread::spawn(move || run_lane(receiver, parts, route, sink));
        jobs
    }

    /// Hands a line to the lane of its source, opening the lane on first use.
    /// A line in a language the relay does not translate is only noted, and
    /// the hold its `final` took on the text in progress is given back.
    fn dispatch(&mut self, arrival: Arrival) {
        let Arrival { line, mut settling } = arrival;
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
                settling: if route == Route::Pair {
                    settling.take()
                } else {
                    None
                },
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
            None => self.open_lane(job.route),
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

    fn run(mut self, incoming: Receiver<Arrival>) {
        for arrival in incoming {
            self.dispatch(arrival);
        }
    }
}

/// Hands a line of the stream to the router and the text in progress to the
/// route of the stream; reports an event that is a line but cannot be used.
fn forward(
    data: &str,
    endpoint: usize,
    lines: &Sender<Arrival>,
    route: &mut StreamRoute,
    sink: &mut dyn LogSink,
) {
    match hear(data) {
        Ok(Heard::Line(line)) => {
            let settling = route.note_line(&line);
            // The router ends only with the process, so a refused line has no one left to tell.
            let _ = lines.send(Arrival { line, settling });
        }
        Ok(Heard::Partial(text)) => route.partial(&text, sink),
        Ok(Heard::Reset) => route.forget(),
        Ok(Heard::Other) => {}
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
    lines: Sender<Arrival>,
    route: StreamRoute,
    backoff: Backoff,
    running: Arc<AtomicBool>,
}

fn spawn_follower(follower: Follower, mut sink: SharedSink) -> io::Result<()> {
    std::thread::Builder::new()
        .name(format!("relay-follow-{}", follower.endpoint))
        .spawn(move || {
            let mut route = follower.route;
            let mut on_event = |data: &str, sink: &mut dyn LogSink| {
                if follower.running.load(Ordering::SeqCst) {
                    forward(data, follower.endpoint, &follower.lines, &mut route, sink);
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

/// The key under which the live pair's translator fallback is kept for the
/// clients that connect later, and the key of the other languages' translator.
const PAIR_FALLBACK_KEY: &str = "translate";
const OTHER_FALLBACK_KEY: &str = "translate:other";

/// How often the watches are read for a change.
const REACH_INTERVAL: Duration = Duration::from_millis(50);

/// Says that a translator became unreachable or reachable again: in the log,
/// and on the stream as a `fallback` that is kept while it is in effect.
fn say_reach(change: Change, key: &str, hub: &Hub, sink: &mut SharedSink) {
    sink.emit(change.event());
    hub.publish_fallback(key, change == Change::Held, &change.wire().to_string());
}

/// Reads the watches until the relay stops and says each change once. The
/// lanes of one translator share a watch, so an outage is said once however
/// many lanes meet it.
fn spawn_reach_notices(
    watches: Watches,
    hub: Hub,
    running: Arc<AtomicBool>,
    mut sink: SharedSink,
) -> io::Result<()> {
    let keyed: Vec<(&'static str, Watch)> = [
        (PAIR_FALLBACK_KEY, watches.pair),
        (OTHER_FALLBACK_KEY, watches.other),
    ]
    .into_iter()
    .filter_map(|(key, watch)| watch.map(|watch| (key, watch)))
    .collect();
    if keyed.is_empty() {
        return Ok(());
    }
    std::thread::Builder::new()
        .name(String::from("relay-reach"))
        .spawn(move || {
            while running.load(Ordering::SeqCst) {
                for (key, watch) in &keyed {
                    for change in watch.drain() {
                        say_reach(change, key, &hub, &mut sink);
                    }
                }
                std::thread::sleep(REACH_INTERVAL);
            }
        })?;
    Ok(())
}

/// Starts the relay with no subtitle file and no outside destination.
pub fn start<L: Llm + 'static>(
    listener: TcpListener,
    config: RelayConfig,
    make: impl Into<Makers<L>>,
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

/// Starts the relay like [`start_partials`], without translating the text of
/// an utterance in progress.
pub fn start_with<L: Llm + 'static>(
    listener: TcpListener,
    config: RelayConfig,
    make: impl Into<Makers<L>>,
    sink: SharedSink,
    outputs: Outputs,
) -> io::Result<SocketAddr> {
    start_partials(
        listener,
        config,
        make,
        sink,
        outputs,
        PartialTranslation::Off,
    )
}

/// Starts the relay like [`start_handle`] and returns only the address.
pub fn start_partials<L: Llm + 'static>(
    listener: TcpListener,
    config: RelayConfig,
    make: impl Into<Makers<L>>,
    sink: SharedSink,
    outputs: Outputs,
    partials: PartialTranslation,
) -> io::Result<SocketAddr> {
    start_handle(listener, config, make, sink, outputs, partials).map(|relay| relay.address)
}

/// The mailbox the followers leave the text in progress in, and the worker
/// that translates it, when the translation of that text is on.
fn open_partials<L: Llm + 'static>(
    partials: PartialTranslation,
    hub: &Hub,
    make: &MakePipeline<L>,
    llama: &Arc<ServerSlots>,
    clock: RunClock,
    sink: &SharedSink,
) -> io::Result<Option<Arc<Mailbox>>> {
    if partials == PartialTranslation::Off {
        return Ok(None);
    }
    let mailbox = Arc::new(Mailbox::with_yield(
        partials == PartialTranslation::Yielding,
    ));
    spawn_partials(
        Arc::clone(&mailbox),
        hub.clone(),
        Arc::clone(make),
        Arc::clone(llama),
        clock,
        sink.clone(),
    )?;
    Ok(Some(mailbox))
}

/// Starts the relay: serves the translations on `listener` and follows every
/// stream of `config.events`, all from threads of their own. The settled
/// translations also go to `outputs`; the translations of the text in
/// progress, when `partials` is on, go to the screens only. Returns the
/// handle of the running relay.
pub fn start_handle<L: Llm + 'static>(
    listener: TcpListener,
    config: RelayConfig,
    makers: impl Into<Makers<L>>,
    mut sink: SharedSink,
    outputs: Outputs,
    partials: PartialTranslation,
) -> io::Result<Relay> {
    let running = Arc::new(AtomicBool::new(true));
    let mut makers = makers.into();
    let hub = Hub::new();
    spawn_reach_notices(
        std::mem::take(&mut makers.watches),
        hub.clone(),
        Arc::clone(&running),
        sink.clone(),
    )?;
    let (translate, skipped) = match config.translate.as_deref().map(split_codes) {
        Some((known, unknown)) => (Some(known), unknown),
        None => (None, Vec::new()),
    };
    for code in &skipped {
        say_skipped(code, &hub, &mut sink);
    }
    let address = hub::serve_while(listener, hub.clone(), Arc::clone(&running))?;
    let delivery = Delivery::start(outputs, hub.clone(), sink.clone())?;
    let clock = RunClock::new(config.started_epoch);
    let llama = Arc::new(ServerSlots::new(LLAMA_SLOTS));
    let mailbox = open_partials(partials, &hub, &makers.pair, &llama, clock, &sink)?;
    let (lines, incoming) = mpsc::channel();
    let router = Router {
        slots: Default::default(),
        lanes: config.lanes,
        translate,
        makers,
        hub,
        delivery,
        clock,
        verify_dump: config.verify_dump,
        llama,
        mailbox: mailbox.clone(),
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
            route: StreamRoute::new(mailbox.clone()),
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
    use crate::logging::MemorySink;

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
    fn a_free_slot_is_taken_without_a_note() {
        let slots = ServerSlots::new(1);
        let mut sink = MemorySink::default();
        drop(take_slot(&slots, "mic-1", "t", &mut sink));
        assert!(sink.events.is_empty());
    }

    #[test]
    fn a_wait_for_a_slot_is_noted_with_the_line_and_the_stage() -> Result<(), String> {
        let slots = Arc::new(ServerSlots::new(1));
        let (held, _) = slots.take();
        let waiter = {
            let slots = Arc::clone(&slots);
            std::thread::spawn(move || {
                let mut sink = MemorySink::default();
                drop(take_slot(&slots, "mic-2", "e", &mut sink));
                sink.events
            })
        };
        std::thread::sleep(Duration::from_millis(50));
        drop(held);
        let events = waiter
            .join()
            .map_err(|_| String::from("the waiter panicked"))?;
        let [event] = events.as_slice() else {
            return Err(format!("{} events, not 1", events.len()));
        };
        assert_eq!(event.event, "translate.slot.waited");
        assert_eq!(event.fields.get("id"), Some(&serde_json::json!("mic-2")));
        assert_eq!(event.fields.get("stage"), Some(&serde_json::json!("e")));
        assert!(event.fields.contains_key("duration_ms"));
        Ok(())
    }

    #[test]
    fn the_slot_wait_is_worded_in_both_locales() -> Result<(), String> {
        for lang in [Lang::Ja, Lang::En] {
            let strings = Strings::embedded(lang)?;
            let said = strings.message_for("translate.slot.waited");
            assert_ne!(said, "translate.slot.waited", "{lang:?}");
        }
        Ok(())
    }

    #[test]
    fn the_clock_counts_from_the_start_and_never_goes_back() {
        let clock = RunClock::new(1_790_000_000);
        let (first, second) = (clock.now_epoch(), clock.now_epoch());
        assert!(first >= 1_790_000_000 && second >= first);
    }
}

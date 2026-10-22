//! The process and the root of its window.
//!
//! `main` reads the command line, opens the log and the settings, starts the
//! three readers and launches the desktop window. In the window, one `Model`
//! takes every message in the order it is handled; `LanesPage` draws the lines
//! of it that `lanes::view` picks at the time `now_ms` holds. One task keeps
//! that time (waiting no longer than `TICK` and no longer than the next change
//! of the lanes), reads the settings again, puts the band back where it
//! belongs and lifts the window to the front of the topmost band.

use std::cell::RefCell;
use std::collections::HashSet;
use std::process::ExitCode;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use dioxus::desktop::tao::event::{Event, WindowEvent};
use dioxus::desktop::tao::platform::windows::WindowExtWindows;
use dioxus::desktop::wry::http::header::CONTENT_TYPE;
use dioxus::desktop::wry::http::{HeaderValue, Response, StatusCode};
use dioxus::desktop::{DesktopContext, use_asset_handler, use_window, use_wry_event_handler};
use dioxus::document;
use dioxus::prelude::*;
use hayamimi_relay::follow::Backoff;
use hayamimi_relay::logging::{Level, LogEvent, LogSink, SharedSink};
use hayamimi_relay::watched::Watched;
use hayamimi_ui::lanes::{LaneRows, LanesPage, next_change, view};
use hayamimi_ui::model::Model;
use hayamimi_ui::parts::Words;
use hayamimi_ui::strings::{Locale, Strings};
use hayamimi_ui::wire::UiEvent;
use tokio::sync::Notify;
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

use crate::config::{self, Args, OverlayConfig, Probe};
use crate::feed::{Deliver, Feed};
use crate::fonts::{self, FontState, Reported, Slot};
use crate::log::{self, FileSink};
use crate::probe;
use crate::rendered::RenderLog;
use crate::win;
use crate::window::{self, Failure, Target};

/// The longest the window goes without reading the settings, lifting itself
/// to the front and looking at the clock.
const TICK: Duration = Duration::from_millis(1000);

/// What the process sets up before the window exists, handed to the root.
#[derive(Clone)]
pub struct Startup {
    pub args: Args,
    pub sink: SharedSink,
    pub initial: OverlayConfig,
    pub config: Arc<Mutex<Watched<OverlayConfig>>>,
    pub events: Arc<Mutex<Option<UnboundedReceiver<UiEvent>>>>,
    pub model: Model,
    pub words: Strings,
    /// Keeps the readers running for as long as the window does.
    pub feed: Option<Arc<Feed>>,
}

fn lock<T>(slot: &Mutex<T>) -> MutexGuard<'_, T> {
    match slot.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// The process: sets up, runs the window until it is closed, and returns the
/// code to exit with when it could not start (2 for the command line, 1 for
/// the rest).
pub fn main(argv: impl Iterator<Item = String>) -> ExitCode {
    let args = match config::parse_args(argv) {
        Ok(args) => args,
        Err(error) => {
            eprintln!("hayamimi-overlay: {error}");
            return ExitCode::from(2);
        }
    };
    let startup = match prepare(args) {
        Ok(startup) => startup,
        Err(reason) => {
            eprintln!("hayamimi-overlay: {reason}");
            return ExitCode::from(1);
        }
    };
    let head = fonts::head(&startup.initial);
    LaunchBuilder::desktop()
        .with_cfg(window::launch_config(head))
        .with_context(startup)
        .launch(Root);
    ExitCode::SUCCESS
}

fn open_sink(args: &Args) -> Result<SharedSink, String> {
    let Some(path) = args.log.clone().or_else(log::default_path) else {
        return Err(String::from("the machine names no directory for the log"));
    };
    let file =
        FileSink::open(&path).map_err(|error| format!("the log cannot be opened: {error}"))?;
    Ok(SharedSink::new(file))
}

/// The readers and the end of their channel; neither exists in a probe.
type Reading = (Option<Arc<Feed>>, Option<UnboundedReceiver<UiEvent>>);

fn start_feed(args: &Args, sink: &SharedSink) -> Result<Reading, String> {
    if args.probe.is_some() {
        return Ok((None, None));
    }
    let (sender, receiver) = unbounded_channel();
    let deliver: Deliver = Arc::new(move |event| {
        // The window is gone when the receiver is; nobody is left to tell.
        let _ = sender.send(event);
    });
    let feed = Feed::start(&args.streams, Backoff::STANDARD, sink, &deliver)
        .map_err(|error| format!("a stream cannot be followed: {error}"))?;
    Ok((Some(Arc::new(feed)), Some(receiver)))
}

fn prepare(args: Args) -> Result<Startup, String> {
    let mut sink = open_sink(&args)?;
    let opened = config::open(args.config.clone().or_else(config::default_path), &mut sink);
    let watched = opened.inspect_err(|reason| sink.emit(no_settings_event(reason)))?;
    let words = Strings::embedded(Locale::detect()).map_err(|error| error.to_string())?;
    let model = match args.probe {
        Some(_) => probe::model().map_err(|error| error.to_string())?,
        None => Model::default(),
    };
    let (feed, receiver) = start_feed(&args, &sink)?;
    Ok(Startup {
        initial: watched.current().clone(),
        config: Arc::new(Mutex::new(watched)),
        events: Arc::new(Mutex::new(receiver)),
        args,
        sink,
        model,
        words,
        feed,
    })
}

/// Milliseconds since the window started.
#[derive(Clone, Copy)]
struct Clock {
    start: Instant,
}

impl Clock {
    fn now_ms(self) -> u64 {
        let since = self.start.elapsed();
        since
            .as_secs()
            .saturating_mul(1000)
            .saturating_add(u64::from(since.subsec_millis()))
    }
}

/// What cuts the task's wait short: a message handled, or a window that moved
/// or changed scale.
#[derive(Default)]
struct Wake {
    notify: Notify,
    moved: AtomicBool,
}

impl Wake {
    fn rouse(&self) {
        self.notify.notify_one();
    }

    fn window_changed(&self) {
        self.moved.store(true, Ordering::SeqCst);
        self.rouse();
    }
}

fn rows_of(config: &OverlayConfig) -> LaneRows {
    LaneRows {
        upper: config.upper_rows,
        lower: config.lower_rows,
    }
}

/// The page's answer to a request for a font file.
fn response_of(reply: fonts::Answer) -> Response<Vec<u8>> {
    let mut response = Response::new(reply.body);
    *response.status_mut() = match StatusCode::from_u16(reply.status) {
        Ok(status) => status,
        Err(_) => StatusCode::NOT_FOUND,
    };
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static(reply.content_type));
    response
}

#[component]
fn Root() -> Element {
    let startup = use_context::<Startup>();
    use_context_provider(|| -> Words { Rc::new(startup.words.clone()) });
    let model = use_signal(|| startup.model.clone());
    let now_ms = use_signal(|| 0_u64);
    let rows = use_signal(|| rows_of(&startup.initial));
    let fonts_in_use = use_hook(|| Rc::new(RefCell::new(startup.initial.clone())));
    let wake = use_hook(|| Arc::new(Wake::default()));
    let clock = use_hook(|| Clock {
        start: Instant::now(),
    });
    let window = use_window();

    let for_handler = Rc::clone(&fonts_in_use);
    use_asset_handler(fonts::ASSET_NAME, move |request, responder| {
        let reply = fonts::answer(&for_handler.borrow(), request.uri().path());
        responder.respond(response_of(reply));
    });
    let for_events = Arc::clone(&wake);
    use_wry_event_handler(move |event, _| {
        if let Event::WindowEvent {
            event: WindowEvent::Moved(_) | WindowEvent::ScaleFactorChanged { .. },
            ..
        } = event
        {
            for_events.window_changed();
        }
    });

    let shell_startup = startup.clone();
    let shell_wake = Arc::clone(&wake);
    use_hook(move || {
        spawn(pump(
            Arc::clone(&shell_startup.events),
            model,
            now_ms,
            clock,
            Arc::clone(&shell_wake),
        ));
        let shell = Shell {
            window,
            startup: shell_startup,
            model,
            now_ms,
            rows,
            fonts_in_use,
            wake: shell_wake,
            clock,
            reported: Rc::new(RefCell::new(Reported::default())),
            said_monitors: HashSet::new(),
            topmost_failing: false,
            on_full_screen: false,
            font_version: 0,
        };
        spawn(shell.drive());
    });

    let mut sink = startup.sink.clone();
    let drawn = use_hook(|| Rc::new(RefCell::new(RenderLog::default())));
    use_effect(move || {
        let shown = view(&model.read(), rows(), now_ms());
        for event in drawn.borrow_mut().changes(&shown) {
            sink.emit(event);
        }
    });

    rsx! { LanesPage { model, rows: rows(), now_ms } }
}

/// Hands each message to the model at the time it is handled.
async fn pump(
    events: Arc<Mutex<Option<UnboundedReceiver<UiEvent>>>>,
    mut model: Signal<Model>,
    mut now_ms: Signal<u64>,
    clock: Clock,
    wake: Arc<Wake>,
) {
    let Some(mut receiver) = lock(&events).take() else {
        // A probe reads no stream.
        return;
    };
    while let Some(event) = receiver.recv().await {
        let at = clock.now_ms();
        model.write().apply(event, at);
        now_ms.set(at);
        wake.rouse();
    }
}

/// The window's keeper: everything the task does about the window and the
/// settings. It lives on the thread of the page.
struct Shell {
    window: DesktopContext,
    startup: Startup,
    model: Signal<Model>,
    now_ms: Signal<u64>,
    rows: Signal<LaneRows>,
    fonts_in_use: Rc<RefCell<OverlayConfig>>,
    wake: Arc<Wake>,
    clock: Clock,
    /// Shared with the tasks that wait for the page's answer to a font reload.
    reported: Rc<RefCell<Reported>>,
    said_monitors: HashSet<String>,
    topmost_failing: bool,
    /// The band stands on the whole monitor; said when it starts to.
    on_full_screen: bool,
    font_version: u32,
}

impl Shell {
    async fn drive(mut self) {
        self.open().await;
        loop {
            self.look_after();
            let wait = self.wait();
            // Whether the wait ended or was cut short, the next step is the same.
            let _ = tokio::time::timeout(wait, self.wake.notify.notified()).await;
            self.advance_clock();
        }
    }

    fn sink(&mut self) -> &mut SharedSink {
        &mut self.startup.sink
    }

    fn probe(&self) -> Option<Probe> {
        self.startup.args.probe
    }

    fn give_up(&mut self, event: LogEvent) -> ! {
        self.sink().emit(event);
        std::process::exit(1)
    }

    fn zoom_percent(&self) -> u32 {
        self.probe().map_or(100, |probe| probe.zoom_percent)
    }

    /// How much shorter than the band the window is made; only a probe shrinks it.
    fn shrink_px(&self) -> u32 {
        self.probe().map_or(0, |probe| probe.shrink_px)
    }

    fn current(&self) -> OverlayConfig {
        lock(&self.startup.config).current().clone()
    }

    /// Whether the window is click-through, read back from the system; the
    /// probe `--probe-click-through-unset` makes the read find it unset.
    async fn click_through_stands(&self) -> bool {
        let unset = self.probe().is_some_and(|probe| probe.click_through_unset);
        let hwnd = self.window.hwnd();
        let read = click_through_reader(unset);
        click_through_holds(|| read(hwnd), CLICK_THROUGH_GAP).await
    }

    /// Shows the window for the first time: click-through (read back), no
    /// border, in place, then visible, then said.
    async fn open(&mut self) {
        let zoom = self.zoom_percent();
        if zoom != 100 {
            self.window.set_zoom_level(f64::from(zoom) / 100.0);
        }
        if self.window.set_ignore_cursor_events(true).is_err() || !self.click_through_stands().await
        {
            self.give_up(failure_event(
                "overlay.window.click_through_failed",
                "CLICK_THROUGH_FAILED",
            ));
        }
        if !win::hide_border(self.window.hwnd()) {
            self.sink().emit(window::border_kept(win::windows_build()));
        }
        let target = match self.place() {
            Ok(target) => target,
            Err(failure) => self.give_up(window::place_failed(failure, true)),
        };
        self.window.set_visible(true);
        self.sink().emit(window::opened(&target));
        let config = self.current();
        self.reported
            .borrow_mut()
            .check(&config, &mut self.startup.sink);
        if let Some(probe) = self.probe() {
            spawn(measure(probe, self.startup.sink.clone()));
        }
    }

    /// Puts the window on the band the settings in use give, if it is not
    /// there. When the work area cannot be read the band goes on the whole
    /// monitor, said when that starts.
    fn place(&mut self) -> Result<Target, Failure> {
        let config = self.current();
        let zoom = self.zoom_percent();
        let shrink = self.shrink_px();
        let target = window::target(
            &self.window,
            &config,
            zoom,
            shrink,
            &mut self.said_monitors,
            &mut self.startup.sink,
        )?;
        if target.full_screen && !self.on_full_screen {
            self.sink().emit(window::work_area_unread());
        }
        self.on_full_screen = target.full_screen;
        if !window::stands_on(&self.window, target.rect) {
            window::place_on(&self.window, target.rect);
        }
        Ok(target)
    }

    /// Places the window again after it ran; a failure is said and the window
    /// keeps the rectangle it has.
    fn replace(&mut self) {
        if let Err(failure) = self.place() {
            self.sink().emit(window::place_failed(failure, false));
        }
    }

    /// One round of keeping: the settings, the band, the front of the screen.
    fn look_after(&mut self) {
        self.reread_settings();
        if self.wake.moved.swap(false, Ordering::SeqCst) {
            self.replace();
        }
        self.lift();
    }

    fn reread_settings(&mut self) {
        let (keys, config) = {
            let mut watched = lock(&self.startup.config);
            let keys = config::refresh(&mut watched, &mut self.startup.sink);
            (keys, watched.current().clone())
        };
        if keys.is_empty() {
            return;
        }
        if self.rows.peek().upper != config.upper_rows
            || self.rows.peek().lower != config.lower_rows
        {
            self.rows.set(rows_of(&config));
        }
        let fonts_changed = Slot::ALL.iter().any(|slot| keys.contains(&slot.key()));
        *self.fonts_in_use.borrow_mut() = config.clone();
        if fonts_changed {
            self.ask_for_fonts_again(config);
        }
        self.replace();
    }

    /// Runs the reload script in the page. What each slot became comes back
    /// from the page; a slot in `error` is reported once, and only an `eval`
    /// that never answers is a failed reload.
    fn ask_for_fonts_again(&mut self, config: OverlayConfig) {
        self.font_version += 1;
        let script = fonts::reload_script(&config, self.font_version);
        let reported = Rc::clone(&self.reported);
        let mut sink = self.startup.sink.clone();
        spawn(async move {
            let mut eval = document::eval(&script);
            match eval.recv::<Vec<FontState>>().await {
                Ok(states) => reported.borrow_mut().from_page(&states, &config, &mut sink),
                Err(error) => sink.emit(reload_failed_event(&error.to_string())),
            }
        });
    }

    /// Lifts the window to the front of the topmost band; a failure is said
    /// when it starts, not on every round it goes on.
    fn lift(&mut self) {
        match win::raise_topmost(self.window.hwnd()) {
            Ok(()) => self.topmost_failing = false,
            Err(code) => {
                if !self.topmost_failing {
                    self.sink().emit(window::topmost_failed(code));
                }
                self.topmost_failing = true;
            }
        }
    }

    /// How long to wait: until the lanes next change, but no longer than a tick.
    fn wait(&self) -> Duration {
        if self.probe().is_some() {
            return TICK;
        }
        let now = self.clock.now_ms();
        let rows = *self.rows.peek();
        let due = next_change(&self.model.peek(), rows, now);
        due.map_or(TICK, |at| {
            Duration::from_millis(at.saturating_sub(now).max(1)).min(TICK)
        })
    }

    /// Moves the time the lanes are cut at, and lets the model forget what
    /// has run out. A probe keeps its clock at zero so that its rows stay.
    fn advance_clock(&mut self) {
        if self.probe().is_some() {
            return;
        }
        let now = self.clock.now_ms();
        let mut model = self.model;
        if model.peek().next_deadline().is_some_and(|due| due <= now) {
            model.write().expire(now);
        }
        if *self.now_ms.peek() != now {
            self.now_ms.set(now);
        }
    }
}

/// An error event with no field but its code.
fn failure_event(event: &str, reason: &str) -> LogEvent {
    LogEvent::new(Level::Error, event, "browser").with_error(reason)
}

/// Not even the built-in settings could be made; a defect of the build.
fn no_settings_event(reason: &str) -> LogEvent {
    failure_event("overlay.window.no_settings", "NO_SETTINGS").with("reason", reason)
}

/// The page did not answer the font reload.
fn reload_failed_event(reason: &str) -> LogEvent {
    LogEvent::new(Level::Warn, "overlay.font.reload_failed", "browser")
        .with("reason", reason)
        .with_error("FONT_RELOAD_FAILED")
}

/// How many times the click-through styles are read back, and the wait between reads.
const CLICK_THROUGH_READS: u32 = 10;
const CLICK_THROUGH_GAP: Duration = Duration::from_millis(50);

/// The read-back of the styles; with `unset` it finds them unset, which is
/// what a window that cannot be clicked through would show.
fn click_through_reader(unset: bool) -> fn(isize) -> Result<bool, u32> {
    if unset {
        |_| Ok(false)
    } else {
        win::click_through_set
    }
}

/// Reads the styles back until they stand, at most `CLICK_THROUGH_READS`
/// times. The wait is a timer of the runtime, so that the thread of the event
/// loop goes on while the job tao posted to it runs.
async fn click_through_holds(read: impl Fn() -> Result<bool, u32>, gap: Duration) -> bool {
    for _ in 0..CLICK_THROUGH_READS {
        if window::click_through_read_ok(read()) {
            return true;
        }
        tokio::time::sleep(gap).await;
    }
    false
}

/// Asks the page for its measures and writes them as one event.
async fn measure(probe: Probe, mut sink: SharedSink) {
    let mut eval = document::eval(&probe::script());
    let event = match eval.recv::<probe::Measure>().await {
        Ok(measure) => probe::measured_event(&measure, probe).map_err(|error| error.to_string()),
        Err(error) => Err(error.to_string()),
    };
    match event {
        Ok(event) => sink.emit(event),
        Err(reason) => sink.emit(
            LogEvent::new(Level::Error, "overlay.layout.failed", "browser")
                .with("reason", reason)
                .with_error("LAYOUT_FAILED"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    const GAP: Duration = Duration::from_millis(1);

    fn run<F: Future>(work: F) -> Result<F::Output, std::io::Error> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()?;
        Ok(runtime.block_on(work))
    }

    /// A read that counts itself and finds the styles standing from the `stands_from`-th read on.
    fn counted(reads: &Cell<u32>, stands_from: u32) -> Result<bool, u32> {
        reads.set(reads.get() + 1);
        Ok(reads.get() >= stands_from)
    }

    #[test]
    fn styles_that_stand_at_the_first_read_end_the_wait_there() -> Result<(), std::io::Error> {
        let reads = Cell::new(0);
        assert!(run(click_through_holds(|| counted(&reads, 1), GAP))?);
        assert_eq!(reads.get(), 1);
        Ok(())
    }

    #[test]
    fn styles_that_stand_only_at_the_tenth_read_still_hold() -> Result<(), std::io::Error> {
        let reads = Cell::new(0);
        assert!(run(click_through_holds(|| counted(&reads, 10), GAP))?);
        assert_eq!(reads.get(), 10);
        Ok(())
    }

    #[test]
    fn styles_that_never_stand_do_not_hold_after_ten_reads() -> Result<(), std::io::Error> {
        let reads = Cell::new(0);
        assert!(!run(click_through_holds(|| counted(&reads, 11), GAP))?);
        assert_eq!(reads.get(), CLICK_THROUGH_READS);
        Ok(())
    }

    #[test]
    fn a_read_that_fails_every_time_does_not_hold() -> Result<(), std::io::Error> {
        let held = run(click_through_holds(|| Err(5), GAP))?;
        assert!(!held);
        Ok(())
    }

    #[test]
    fn the_unset_probe_reads_no_mark_whatever_the_window() {
        assert_eq!(click_through_reader(true)(1), Ok(false));
    }

    #[test]
    fn a_window_whose_styles_read_unset_ends_the_process_without_showing_it()
    -> Result<(), std::io::Error> {
        let unset = click_through_reader(true);
        let held = run(click_through_holds(|| unset(1), GAP))?;
        assert!(!held);
        Ok(())
    }

    #[test]
    fn no_settings_is_an_error_with_its_code_and_the_reason() {
        let line = no_settings_event("builtin failed").to_json_line();
        assert!(line.contains(r#""level":"error""#), "{line}");
        assert!(
            line.contains(r#""event":"overlay.window.no_settings""#),
            "{line}"
        );
        assert!(
            line.contains(r#""error_code":"HYM_BROWSER_NO_SETTINGS""#),
            "{line}"
        );
        assert!(line.contains(r#""reason":"builtin failed""#), "{line}");
    }

    #[test]
    fn a_reload_that_never_answers_is_a_warning_with_its_own_code() {
        let line = reload_failed_event("closed").to_json_line();
        assert!(line.contains(r#""level":"warn""#), "{line}");
        assert!(
            line.contains(r#""error_code":"HYM_BROWSER_FONT_RELOAD_FAILED""#),
            "{line}"
        );
    }
}

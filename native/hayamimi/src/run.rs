//! A run of the program: parse the command line, check what the machine has,
//! open the models, start the input and the servers, and recognise until the
//! input ends or the run is stopped.
//!
//! A missing GPU, model, library, port or audio device does not stop the run.
//! Each one is found at the start (devices also later), the stage that needs
//! it takes another road, and the change is said once in the log, on the
//! screen (`/events`) and on stderr.

use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread::JoinHandle;
use std::time::Instant;

use axum::Router;
use hayamimi_asr::audio::read_wav;
use hayamimi_asr::itn::{self, Overrides};
use hayamimi_asr::ort_runtime::{Device, OrtRuntime};
use hayamimi_asr::punct::Punctuator;
use hayamimi_asr::replace::Replacements;
use hayamimi_asr::route::{Policy, Refined, RouteError};
use hayamimi_asr::vad::{Sensitivity, VadSettings};
use hayamimi_asr::{AsrError, Engine, Gpu, Reason, Settings, check_library};
use hayamimi_gpu::memory::MemoryMeter;
use hayamimi_relay::lan::{self, LanUrl};
use hayamimi_relay::logging::Level;
use serde_json::Value;

#[cfg(test)]
use crate::args::Mode;
use crate::args::{self, Args, Parsed};
use crate::capture::SourceKind;
use crate::capture::supervise::{self, Capture, Spec};
use crate::control::{Command, Controls, Initial, Live};
use crate::feed::{self, AudioTx};
use crate::gpu_check;
use crate::ingest::{self, IngestState};
use crate::inputs::Input;
use crate::log;
use crate::memory_guard::MemoryGuard;
use crate::notice::Notice;
use crate::refine::{Refiner, Reread, Rereader, samples_ms};
use crate::report::{log_sessions, say, say_notice, spawn_capture_reporter, tell_resident};
use crate::routes::{self, AppState};
use crate::server::{self, Hub};
use crate::session::Session;
use crate::strings::{self, Strings, keys};

/// The run finished.
pub const EXIT_OK: u8 = 0;
/// The run stopped on an error that no other road avoids.
pub const EXIT_FAILED: u8 = 1;
/// The command line was refused.
pub const EXIT_REFUSED: u8 = 2;

/// Why a run stopped after it had started.
#[derive(Debug)]
enum RunError {
    Asr(AsrError),
    Io(std::io::Error),
    Signal(String),
}

impl From<AsrError> for RunError {
    fn from(error: AsrError) -> Self {
        RunError::Asr(error)
    }
}

impl From<std::io::Error> for RunError {
    fn from(error: std::io::Error) -> Self {
        RunError::Io(error)
    }
}

/// Reports a run that stopped on an error: the log event on stdout, and on
/// stderr the table's sentence followed by the detail, which is data (a file
/// name) and so is not in any table.
fn report_failure(strings: &Strings, error: &RunError) {
    log::print(&log::engine_failed_line(strings));
    let detail = match error {
        RunError::Asr(other) => other.to_string(),
        RunError::Io(other) => other.to_string(),
        RunError::Signal(other) => other.clone(),
    };
    eprintln!("{}", strings.cli_text(keys::RUN_FAILED));
    eprintln!("  {detail}");
}

pub use crate::wire::final_event;

/// The settings of the route. A replacement dictionary that cannot be read is
/// not used, and the run says so.
fn settings_of(args: &Args, gpu: Gpu) -> (Settings, Option<Notice>) {
    let mut settings = Settings::new(args.models.clone());
    settings.vad = VadSettings {
        min_silence: args.min_silence,
        max_speech: args.max_speech,
    };
    settings.diagnose = args.diagnose;
    settings.gpu = gpu;
    settings.max_resident = args.max_resident;
    settings.endpoint = args.endpoint;
    settings.policy = args.policy();
    let mut notice = None;
    if let Some(path) = &args.replace {
        match Replacements::load(path) {
            Ok(replacements) => settings.replacements = replacements,
            Err(_) => notice = Some(Notice::config_unreadable()),
        }
    }
    (settings, notice)
}

/// What the `GET` routes show before the engine has opened.
fn initial_of(settings: &Settings) -> Initial {
    Initial {
        replacements: settings.replacements.clone(),
        overrides: settings.itn_overrides.clone(),
        punctuate: settings.stages.punctuate,
        can_punctuate: settings.stages.punctuate,
        vad: Sensitivity::starting(settings.vad),
    }
}

/// The language the punctuation model restores marks for.
const PUNCTUATED_LANGUAGE: &str = "ja";
/// `refine.punct.failed`: a refined sentence the punctuation model could not read goes on without marks.
const PUNCT_FAILED: &str = "refine.punct.failed";

/// `refine.punct.failed`: the shape of the live `asr.punct.failed`. It names
/// the group by its length, never its text.
fn punct_failed_line(strings: &Strings, audio_ms: u64) -> String {
    log::event_line(
        strings,
        Level::Warn,
        PUNCT_FAILED,
        "asr",
        None,
        &[("audio_ms", Value::from(audio_ms))],
    )
}

/// What the refine pass has to say about its punctuation model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Told {
    /// The model could not be opened, for this reason. The pass goes on without marks.
    Unavailable(Reason),
    /// The model could not read one sentence of a group of `audio_ms`. The sentence went on without marks.
    Failed { audio_ms: u64 },
}

/// Says one thing the refine pass has to say.
type Say = Box<dyn FnMut(Told)>;

/// Says `told` where it belongs. A model that cannot be opened is a fallback
/// of the stage `refine_punct`, said on the three channels. A sentence that
/// failed is a log event only, as in the live pass.
fn tell(strings: &Strings, hub: &Hub, told: Told) {
    match told {
        Told::Unavailable(reason) => {
            say_notice(strings, hub, &Notice::refine_punct_disabled(reason));
        }
        Told::Failed { audio_ms } => log::print(&punct_failed_line(strings, audio_ms)),
    }
}

/// Why a punctuation model could not be opened: its file is not there, or it cannot be used.
fn refine_punct_reason(error: &AsrError) -> Reason {
    match error {
        AsrError::MissingModel(_) => Reason::Missing,
        _ => Reason::Unreadable,
    }
}

/// Puts marks into a refined sentence. [`Punctuator`] is the one of a run; a
/// test hands in a script, so that the refine pass is judged without a model.
trait Restore {
    fn restore(&mut self, text: &str) -> Result<String, AsrError>;
}

impl Restore for Punctuator {
    fn restore(&mut self, text: &str) -> Result<String, AsrError> {
        Punctuator::restore(self, text)
    }
}

/// Opens the punctuation model of the refine pass.
type Opener<P> = Box<dyn FnOnce() -> Result<P, AsrError>>;

/// The punctuation model of the refine pass: opened once, when it is first
/// wanted, and left out for good when it cannot be opened.
enum Model<P> {
    Closed(Opener<P>),
    Open(P),
    Unavailable,
}

/// What the live loop works by, read for one group: the refine pass follows a
/// change made through `/config`, `/replacements` and `/itn_overrides`.
struct Wanted {
    overrides: Overrides,
    replacements: Replacements,
    punctuate: bool,
}

fn wanted(controls: &Controls) -> Wanted {
    let live = controls.live();
    let (exclude, force) = live.itn();
    Wanted {
        overrides: Overrides { exclude, force },
        replacements: Replacements::from_pairs(live.replacements()),
        punctuate: live.config().0,
    }
}

/// A reader of the refine pass that gives a refined text what a final
/// sentence gets: ITN, then punctuation, then the dictionary.
struct Polish<R, P> {
    inner: R,
    controls: Arc<Controls>,
    model: Model<P>,
    /// Says that the model could not be opened, and that it could not read a sentence.
    say: Say,
}

impl<R: Reread, P: Restore> Polish<R, P> {
    fn new(inner: R, controls: Arc<Controls>, open: Opener<P>, say: Say) -> Self {
        Polish {
            inner,
            controls,
            model: Model::Closed(open),
            say,
        }
    }

    /// Opens the model now when punctuation is on, so that the first group
    /// does not wait for it.
    fn warm(&mut self) {
        if wanted(&self.controls).punctuate {
            self.punctuator();
        }
    }

    /// The model, opened on the first call. A model that cannot be opened is
    /// said once and not tried again.
    fn punctuator(&mut self) -> Option<&mut P> {
        if matches!(self.model, Model::Closed(_)) {
            self.open();
        }
        match &mut self.model {
            Model::Open(model) => Some(model),
            Model::Closed(_) | Model::Unavailable => None,
        }
    }

    fn open(&mut self) {
        let Model::Closed(open) = std::mem::replace(&mut self.model, Model::Unavailable) else {
            return;
        };
        match open() {
            Ok(model) => self.model = Model::Open(model),
            Err(error) => (self.say)(Told::Unavailable(refine_punct_reason(&error))),
        }
    }

    /// Marks for a Japanese sentence of a group of `audio_ms` when punctuation
    /// is on. A sentence the model cannot read goes on without marks, and the
    /// failure is said.
    fn punctuate(&mut self, text: String, lang: &str, on: bool, audio_ms: u64) -> String {
        if !on || lang != PUNCTUATED_LANGUAGE || text.trim().is_empty() {
            return text;
        }
        let Some(model) = self.punctuator() else {
            return text;
        };
        match model.restore(&text) {
            Ok(marked) => marked,
            Err(_) => {
                (self.say)(Told::Failed { audio_ms });
                text
            }
        }
    }

    /// The stages of a final sentence, in the order of the live path.
    fn finish(&mut self, text: String, lang: &str, audio_ms: u64) -> String {
        let now = wanted(&self.controls);
        let text = itn::convert(&text, lang, &now.overrides);
        let text = self.punctuate(text, lang, now.punctuate, audio_ms);
        now.replacements.apply(&text)
    }
}

impl<R: Reread, P: Restore> Reread for Polish<R, P> {
    /// The stages follow the language the second reading settled on.
    fn reread(&mut self, audio: &[f32], lang: &str) -> Result<Refined, RouteError> {
        let mut refined = self.inner.reread(audio, lang)?;
        let text = std::mem::take(&mut refined.text);
        refined.text = self.finish(text, &refined.lang, samples_ms(audio));
        Ok(refined)
    }

    fn release(&mut self) {
        self.inner.release();
    }
}

/// The device the punctuation model of the refine pass is opened on. It is a
/// second model, opened on the CPU: the live one is not shared, and the GPU is
/// not asked for more memory.
const REFINE_PUNCT_DEVICE: Device = Device::Cpu;

/// Opens the punctuation model of the refine pass through `open`, which is
/// told the device. This is the one place that decides it.
fn open_on_refine_device<P>(
    models: &Path,
    open: impl FnOnce(&Path, Device) -> Result<P, AsrError>,
) -> Result<P, AsrError> {
    open(models, REFINE_PUNCT_DEVICE)
}

/// The punctuation model of the refine pass.
fn open_cpu_punctuator(models: PathBuf) -> Result<Punctuator, AsrError> {
    open_on_refine_device(&models, |models, device| {
        Punctuator::open(OrtRuntime::shared()?, models, device, false)
    })
}

/// The reader of the refine pass, built on the thread of the worker. What the
/// pass has to say about its punctuation model is said through `hub`.
fn polished_rereader(
    models: PathBuf,
    policy: Policy,
    controls: Arc<Controls>,
    strings: Strings,
    hub: Hub,
) -> Polish<Rereader, Punctuator> {
    let reader = Rereader::new(&models, policy);
    let open: Opener<Punctuator> = Box::new(move || open_cpu_punctuator(models));
    let say: Say = Box::new(move |told| tell(&strings, &hub, told));
    let mut polish = Polish::new(reader, controls, open, say);
    polish.warm();
    polish
}

/// Builds the reader of the refine pass on the thread of its worker.
type Maker = Box<dyn FnOnce() -> Polish<Rereader, Punctuator> + Send>;

/// Starts the refine pass unless the command line turned it off.
fn start_refiner(
    args: &Args,
    strings: &Strings,
    hub: &Hub,
    controls: &Arc<Controls>,
) -> Option<Refiner> {
    start_refiner_with(args, strings, hub, controls, Refiner::start)
}

/// [`start_refiner`] with the way the worker is started. A worker that cannot
/// be started leaves the pass out: the run goes on without it, and says so once.
fn start_refiner_with(
    args: &Args,
    strings: &Strings,
    hub: &Hub,
    controls: &Arc<Controls>,
    start: impl FnOnce(Maker, Hub, Strings, &'static str) -> std::io::Result<Refiner>,
) -> Option<Refiner> {
    if !args.refine {
        return None;
    }
    let (models, policy) = (args.models.clone(), args.policy());
    let (live, own, told) = (Arc::clone(controls), strings.clone(), hub.clone());
    let make: Maker = Box::new(move || polished_rereader(models, policy, live, own, told));
    let source = args.io.source_name();
    match start(make, hub.clone(), strings.clone(), source) {
        Ok(refiner) => Some(refiner),
        Err(_) => {
            say_notice(strings, hub, &Notice::refine_disabled());
            None
        }
    }
}

/// One server the run starts: how it is called, how its address reads, and
/// the notice of a port that was taken.
struct Endpoint {
    name: &'static str,
    url: fn(SocketAddr) -> String,
    notice: fn(Option<String>) -> Notice,
}

fn events_url(address: SocketAddr) -> String {
    format!("http://{address}/events")
}

fn ingest_url(address: SocketAddr) -> String {
    format!("ws://{address}/ingest")
}

const EVENTS: Endpoint = Endpoint {
    name: "events",
    url: events_url,
    notice: Notice::port_in_use,
};

const INGEST: Endpoint = Endpoint {
    name: "ingest",
    url: ingest_url,
    notice: Notice::ingest_port_in_use,
};

/// Serves `router` on `port` or the first free port after it, and returns the
/// address it listens on. A server that moved, or could not be started because
/// every port in reach was taken (`None`), adds a notice.
fn start_listener(
    router: Router,
    ip: IpAddr,
    port: u16,
    endpoint: &Endpoint,
    notices: &mut Vec<Notice>,
) -> Result<Option<SocketAddr>, RunError> {
    let Ok(listener) = server::bind_on(ip, port) else {
        notices.push((endpoint.notice)(None));
        return Ok(None);
    };
    let address = server::serve(router, listener, endpoint.name)?;
    if port != 0 && address.port() != port {
        notices.push((endpoint.notice)(Some((endpoint.url)(address))));
    }
    Ok(Some(address))
}

/// Says that the subtitle server listens on the network, and where its page
/// opens, when the address it listens on is not the loopback. The URL is data
/// and follows the sentence of the table.
fn say_lan(strings: &Strings, address: SocketAddr) {
    let line = match lan::lan_url(address, "/") {
        LanUrl::Loopback => return,
        LanUrl::At(url) => format!("{} {url}", strings.cli_text(keys::LAN_URL)),
        LanUrl::Unknown => strings.cli_text(keys::LAN_URL_UNKNOWN).to_owned(),
    };
    log::print(&log::lan_opened_line(strings, address.port()));
    eprintln!("{line}");
}

/// Starts the servers the command line asks for.
fn start_servers(
    args: &Args,
    strings: &Strings,
    hub: &Hub,
    controls: &Arc<Controls>,
    audio: &AudioTx,
    notices: &mut Vec<Notice>,
) -> Result<(), RunError> {
    if let Some(port) = args.serve {
        let state = AppState {
            hub: hub.clone(),
            controls: Arc::clone(controls),
        };
        let ip = args.serve_host;
        let router = routes::router_for(state, ip);
        if let Some(address) = start_listener(router, ip, port, &EVENTS, notices)? {
            say_lan(strings, address);
        }
    }
    if let Some(port) = args.io.ingest_port() {
        let state = IngestState::new(hub.clone(), audio.clone(), Arc::new(strings.clone()));
        start_listener(
            ingest::router(state),
            args.io.ws_host,
            port,
            &INGEST,
            notices,
        )?;
    }
    Ok(())
}

/// The first GPU check; a machine without a device is logged and goes on.
fn check_gpu(strings: &Strings) -> Gpu {
    let gpu = gpu_check::preflight();
    if let Gpu::Absent(why) = gpu {
        log::print(&log::device_unavailable_line(strings, why));
    }
    gpu
}

/// Opens the reader of the GPU memory when there is a GPU to read. A reader
/// that cannot be opened is logged, and the run goes on without a budget.
fn open_guard(args: &Args, strings: &Strings, gpu: Gpu) -> Option<MemoryGuard<MemoryMeter>> {
    if !matches!(gpu, Gpu::Try) {
        return None;
    }
    match MemoryGuard::open(args.gpu_budget_mib, Instant::now()) {
        Ok(guard) => Some(guard),
        Err(_) => {
            log::print(&log::memory_unreadable_line(strings));
            None
        }
    }
}

/// Opens the engine, or notes that the inference libraries did not open and
/// there is none. The fallbacks of the opened stages are added to `notices`.
fn open_engine(
    settings: Settings,
    strings: &Strings,
    notices: &mut Vec<Notice>,
) -> Result<Option<Engine>, RunError> {
    if check_library().is_err() {
        notices.push(Notice::library_unavailable());
        return Ok(None);
    }
    let engine = Engine::open(settings)?;
    log_sessions(strings, &engine);
    notices.extend(engine.fallbacks().iter().map(Notice::from_fallback));
    Ok(Some(engine))
}

/// Reads silence once with the Japanese recognizer, so that the first
/// utterance does not pay for the first decode of the process. A decode that
/// fails is said, and the run goes on. The punctuation model is read once
/// after it; the lines to say are chosen by [`log::warmup_lines`].
fn warm_recognizer(strings: &Strings, engine: &mut Engine) {
    if let Some(read) = engine.warm_up() {
        for line in log::warmup_lines(strings, &read) {
            log::print(&line);
        }
    }
}

/// Says every notice, and once how to check the GPU when the GPU is the cause.
fn tell_notices(strings: &Strings, hub: &Hub, notices: &[Notice], gpu: Gpu) {
    for notice in notices {
        say_notice(strings, hub, notice);
    }
    if matches!(gpu, Gpu::Absent(_)) || notices.iter().any(Notice::is_gpu) {
        say(strings, &[keys::NO_GPU_CHECK]);
    }
}

/// The sources an input captures.
fn capture_specs(args: &Args) -> Vec<Spec> {
    let mic = Spec {
        kind: SourceKind::Mic,
        device: args.io.mic_device.clone(),
    };
    let speaker = Spec {
        kind: SourceKind::Speaker,
        device: args.io.speaker_device.clone(),
    };
    match args.input() {
        Input::Mic => vec![mic],
        Input::Speaker => vec![speaker],
        Input::Mix => vec![mic, speaker],
        Input::Wav | Input::Ws => Vec::new(),
    }
}

/// What feeds the queue; dropping it stops the capture.
struct InputGuard {
    _capture: Option<Capture>,
    _player: Option<JoinHandle<()>>,
}

fn start_capture(
    args: &Args,
    strings: &Strings,
    hub: &Hub,
    audio: AudioTx,
) -> Result<Capture, RunError> {
    let (events_tx, events_rx) = mpsc::channel();
    spawn_capture_reporter(strings.clone(), hub.clone(), events_rx)?;
    Ok(supervise::start(capture_specs(args), audio, events_tx)?)
}

/// Starts the input of the run. The wav file is played from a thread; a
/// device input is captured; `/ingest` needs nothing here, its server is up.
fn start_input(
    args: &Args,
    strings: &Strings,
    hub: &Hub,
    audio: AudioTx,
    samples: Option<Vec<f32>>,
) -> Result<InputGuard, RunError> {
    let mut guard = InputGuard {
        _capture: None,
        _player: None,
    };
    match (args.input(), samples) {
        (Input::Wav, Some(samples)) => {
            guard._player = Some(feed::play_wav(samples, args.realtime, audio));
        }
        (Input::Mic | Input::Speaker | Input::Mix, _) => {
            guard._capture = Some(start_capture(args, strings, hub, audio)?);
        }
        (Input::Wav | Input::Ws, _) => {}
    }
    Ok(guard)
}

/// Ends the run on Ctrl-C or a termination signal, with exit code 0.
fn install_stop() -> Result<Arc<AtomicBool>, RunError> {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    ctrlc::set_handler(move || flag.store(true, Ordering::Relaxed))
        .map_err(|error| RunError::Signal(error.to_string()))?;
    Ok(stop)
}

/// The wav file of a run that plays one, read before anything is started.
fn read_input_wav(args: &Args) -> Result<Option<Vec<f32>>, RunError> {
    match args.input() {
        Input::Wav => Ok(Some(read_wav(&args.wav)?)),
        _ => Ok(None),
    }
}

/// Whether the audio arrives at the speed of real time.
fn is_live(args: &Args) -> bool {
    args.input() != Input::Wav || args.realtime
}

/// Everything from the first check to the end of the input.
fn execute(args: &Args, strings: &Strings) -> Result<(), RunError> {
    let samples = read_input_wav(args)?;
    let gpu = check_gpu(strings);
    let (settings, config_notice) = settings_of(args, gpu);
    let hub = Hub::new();
    let (audio_tx, audio_rx) = feed::channel();
    let (command_tx, command_rx): (_, Receiver<Command>) = mpsc::channel();
    let controls = Arc::new(Controls::new(Live::new(initial_of(&settings)), command_tx));
    let mut notices = Vec::new();
    start_servers(args, strings, &hub, &controls, &audio_tx, &mut notices)?;
    notices.extend(config_notice);
    if args.io.source_refused {
        notices.push(Notice::source_unknown());
    }
    let mut engine = open_engine(settings, strings, &mut notices)?;
    controls
        .live()
        .engine_opened(engine.as_ref().is_some_and(Engine::can_punctuate));
    tell_notices(strings, &hub, &notices, gpu);
    // The models loaded while opening are said before `session_start`, which
    // is what a client of `/ingest` waits for before it is answered `ready`.
    // The start-up decode comes after them and before any audio is read.
    if let Some(opened) = engine.as_mut() {
        tell_resident(strings, &hub, opened);
        warm_recognizer(strings, opened);
    }
    let stop = install_stop()?;
    let refiner = start_refiner(args, strings, &hub, &controls);
    let _input = start_input(args, strings, &hub, audio_tx, samples)?;
    hub.publish_session_start();
    Session {
        engine,
        refiner,
        guard: open_guard(args, strings, gpu),
        hub: &hub,
        strings,
        controls: &controls,
        audio: audio_rx,
        commands: command_rx,
        stop: &stop,
        partial: args.io.partial,
        live: is_live(args),
        source: args.io.source_name(),
    }
    .run()?;
    Ok(())
}

/// Runs the program and returns its exit code.
pub fn run(raw: &[String], locale_tag: Option<&str>) -> u8 {
    let Ok(strings) = Strings::embedded(strings::resolve(locale_tag)) else {
        // The compiled-in tables are checked by a test; without one there is no text to print.
        return EXIT_FAILED;
    };
    let args = match args::parse(raw) {
        Ok(Parsed::Help) => {
            println!("{}", strings.cli_text(keys::HELP));
            return EXIT_OK;
        }
        Ok(Parsed::Run(args)) => args,
        Err(error) => {
            say(&strings, &[error.key(), keys::USAGE_HINT]);
            return EXIT_REFUSED;
        }
    };
    match execute(&args, &strings) {
        Ok(()) => EXIT_OK,
        Err(error) => {
            report_failure(&strings, &error);
            EXIT_FAILED
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{ConfigChange, ItnBody};
    use crate::inputs::IoArgs;
    use hayamimi_asr::{Fallback, Stage, To};
    use hayamimi_relay::lang::Lang;
    use std::cell::{Cell, RefCell};
    use std::net::Ipv4Addr;
    use std::rc::Rc;
    use tokio::sync::broadcast::{self, error::TryRecvError};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn args_with(input: Option<Input>, replace: Option<&str>) -> Args {
        Args {
            mode: Mode::Single,
            lang: None,
            wav: "a.wav".into(),
            realtime: false,
            models: "models".into(),
            min_silence: 0.35,
            max_speech: 12.0,
            endpoint: hayamimi_asr::endpoint::Mode::Early,
            partial_lid: true,
            replace: replace.map(Into::into),
            serve: None,
            serve_host: IpAddr::V4(Ipv4Addr::LOCALHOST),
            diagnose: false,
            refine: true,
            max_resident: None,
            gpu_budget_mib: 14_000,
            io: IoArgs {
                input,
                mic_device: Some(String::from("usb")),
                speaker_device: None,
                ..IoArgs::default()
            },
        }
    }

    #[test]
    fn the_exit_codes_are_distinct_and_only_ok_is_zero() {
        let codes = [EXIT_OK, EXIT_FAILED, EXIT_REFUSED];
        for (i, a) in codes.iter().enumerate() {
            for b in &codes[i + 1..] {
                assert_ne!(a, b);
            }
        }
        assert!(codes[1..].iter().all(|code| *code != 0));
    }

    #[test]
    fn a_command_line_without_input_is_refused_before_anything_else() {
        assert_eq!(run(&[], None), EXIT_REFUSED);
    }

    #[test]
    fn help_succeeds() {
        assert_eq!(run(&[String::from("--help")], None), EXIT_OK);
    }

    #[test]
    fn a_replacement_dictionary_that_cannot_be_read_is_not_used_and_is_named() {
        let (settings, notice) =
            settings_of(&args_with(None, Some("no-such-dictionary.txt")), Gpu::Try);
        assert!(settings.replacements.is_empty());
        assert_eq!(notice, Some(Notice::config_unreadable()));
    }

    #[test]
    fn the_source_flag_reaches_the_utterances_and_a_bad_one_keeps_the_input_source() {
        let mut args = args_with(Some(Input::Ws), None);
        args.io.source = crate::inputs::parse_source("loopback");
        assert_eq!(args.io.source_name(), "loopback");
        let mut bad = args_with(Some(Input::Ws), None);
        bad.io.source = crate::inputs::parse_source("line");
        bad.io.source_refused = true;
        assert_eq!(bad.io.source_name(), "mic");
        assert_eq!(Notice::source_unknown().stage, "source");
    }

    #[test]
    fn no_dictionary_asked_for_means_no_notice() {
        let (_, notice) = settings_of(&args_with(None, None), Gpu::Try);
        assert_eq!(notice, None);
    }

    #[test]
    fn the_state_of_the_gpu_reaches_the_settings() {
        let (settings, _) = settings_of(&args_with(None, None), Gpu::Absent(Reason::NoDevice));
        assert_eq!(settings.gpu, Gpu::Absent(Reason::NoDevice));
    }

    #[test]
    fn the_routes_start_from_the_settings_of_the_run() {
        let (settings, _) = settings_of(&args_with(None, None), Gpu::Try);
        let initial = initial_of(&settings);
        assert!(initial.punctuate && initial.can_punctuate);
        assert_eq!(initial.vad.min_silence, 0.35);
        assert_eq!(initial.vad.max_speech, 12.0);
    }

    #[test]
    fn each_input_captures_the_sources_it_names() {
        let kinds = |input| {
            capture_specs(&args_with(Some(input), None))
                .into_iter()
                .map(|spec| spec.kind)
                .collect::<Vec<_>>()
        };
        assert_eq!(kinds(Input::Mic), vec![SourceKind::Mic]);
        assert_eq!(kinds(Input::Speaker), vec![SourceKind::Speaker]);
        assert_eq!(
            kinds(Input::Mix),
            vec![SourceKind::Mic, SourceKind::Speaker]
        );
        assert!(kinds(Input::Wav).is_empty());
        assert!(kinds(Input::Ws).is_empty());
    }

    #[test]
    fn the_device_names_reach_their_sources() {
        let specs = capture_specs(&args_with(Some(Input::Mix), None));
        assert_eq!(specs[0].device.as_deref(), Some("usb"));
        assert_eq!(specs[1].device, None);
    }

    #[test]
    fn only_a_wav_file_played_without_pacing_is_not_live() {
        let mut args = args_with(None, None);
        assert!(!is_live(&args));
        args.realtime = true;
        assert!(is_live(&args));
        args.realtime = false;
        args.io.input = Some(Input::Ws);
        assert!(is_live(&args));
    }

    #[test]
    fn a_taken_port_moves_the_server_and_adds_a_notice() -> Result<(), Box<dyn std::error::Error>> {
        let taken = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let port = taken.local_addr()?.port();
        let mut notices = Vec::new();
        start_listener(
            Router::new(),
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            port,
            &EVENTS,
            &mut notices,
        )
        .map_err(|_| "the server did not start")?;
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].event, "server.fallback.port");
        assert_eq!(notices[0].reason, "in_use");
        assert_eq!(notices[0].stage, "events");
        assert!(
            notices[0]
                .url
                .as_deref()
                .is_some_and(|url| url.ends_with("/events"))
        );
        Ok(())
    }

    #[test]
    fn a_taken_ingest_port_names_its_own_stage_and_address()
    -> Result<(), Box<dyn std::error::Error>> {
        let taken = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let port = taken.local_addr()?.port();
        let mut notices = Vec::new();
        start_listener(
            Router::new(),
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            port,
            &INGEST,
            &mut notices,
        )
        .map_err(|_| "the server did not start")?;
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].stage, "ingest");
        assert!(
            notices[0]
                .url
                .as_deref()
                .is_some_and(|url| url.starts_with("ws://"))
        );
        assert!(
            notices[0]
                .url
                .as_deref()
                .is_some_and(|url| url.ends_with("/ingest"))
        );
        Ok(())
    }

    #[test]
    fn a_started_server_reports_the_address_it_listens_on() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut notices = Vec::new();
        let started = start_listener(
            Router::new(),
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            0,
            &EVENTS,
            &mut notices,
        )
        .map_err(|_| "the server did not start")?;
        let Some(address) = started else {
            return Err("a free port gave no address".into());
        };
        assert!(address.ip().is_loopback() && address.port() != 0);
        Ok(())
    }

    #[test]
    fn a_free_port_adds_no_notice() -> Result<(), Box<dyn std::error::Error>> {
        let mut notices = Vec::new();
        start_listener(
            Router::new(),
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            0,
            &EVENTS,
            &mut notices,
        )
        .map_err(|_| "the server did not start")?;
        assert!(notices.is_empty());
        Ok(())
    }

    /// A control surface whose live view has nothing set. The receiver keeps
    /// the commands the loop would read; a test holds it for as long as it
    /// changes a setting.
    fn controls_over(can_punctuate: bool) -> (Arc<Controls>, Receiver<Command>) {
        let (commands, received) = mpsc::channel();
        let vad = VadSettings {
            min_silence: 0.35,
            max_speech: 12.0,
        };
        let live = Live::new(Initial {
            replacements: Replacements::default(),
            overrides: Overrides::default(),
            punctuate: true,
            can_punctuate,
            vad: Sensitivity::starting(vad),
        });
        (Arc::new(Controls::new(live, commands)), received)
    }

    /// A reader that answers every group with one text in one language.
    struct Says(&'static str, &'static str);

    impl Reread for Says {
        fn reread(&mut self, _audio: &[f32], _lang: &str) -> Result<Refined, RouteError> {
            Ok(Refined {
                text: self.0.to_owned(),
                lang: self.1.to_owned(),
            })
        }
    }

    /// What the scripted model was asked, how often it was opened, and what the pass said about it.
    #[derive(Default)]
    struct Tally {
        opened: Rc<Cell<u32>>,
        said: Rc<RefCell<Vec<Told>>>,
        read: Rc<RefCell<Vec<String>>>,
    }

    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Behaviour {
        Marks,
        FailsToRead,
        /// The model file is not there.
        FailsToOpen,
        /// The model file is there and cannot be read.
        FailsToOpenUnreadable,
    }

    /// A model that adds a full stop, or fails to read.
    struct Marks {
        read: Rc<RefCell<Vec<String>>>,
        fails: bool,
    }

    impl Restore for Marks {
        fn restore(&mut self, text: &str) -> Result<String, AsrError> {
            self.read.borrow_mut().push(text.to_owned());
            if self.fails {
                Err(AsrError::NoResult)
            } else {
                Ok(format!("{text}。"))
            }
        }
    }

    /// A pass over a scripted model that says what it has to say through `say`.
    fn polish_saying(
        reader: Says,
        controls: &Arc<Controls>,
        tally: &Tally,
        how: Behaviour,
        say: Say,
    ) -> Polish<Says, Marks> {
        let (opened, read) = (Rc::clone(&tally.opened), Rc::clone(&tally.read));
        let open: Opener<Marks> = Box::new(move || {
            opened.set(opened.get() + 1);
            match how {
                Behaviour::FailsToOpen => Err(AsrError::MissingModel(PathBuf::new())),
                Behaviour::FailsToOpenUnreadable => {
                    Err(AsrError::Io(String::from("cannot be read")))
                }
                Behaviour::Marks | Behaviour::FailsToRead => Ok(Marks {
                    read,
                    fails: how == Behaviour::FailsToRead,
                }),
            }
        });
        Polish::new(reader, Arc::clone(controls), open, say)
    }

    /// A pass whose sayings are recorded in `tally`.
    fn polish_over(
        reader: Says,
        controls: &Arc<Controls>,
        tally: &Tally,
        how: Behaviour,
    ) -> Polish<Says, Marks> {
        let said = Rc::clone(&tally.said);
        let say: Say = Box::new(move |told| said.borrow_mut().push(told));
        polish_saying(reader, controls, tally, how, say)
    }

    /// A pass whose sayings are recorded in `tally` and also told on the
    /// channels of `hub`, the way a run tells them.
    fn polish_telling(
        reader: Says,
        controls: &Arc<Controls>,
        tally: &Tally,
        how: Behaviour,
        strings: &Strings,
        hub: &Hub,
    ) -> Polish<Says, Marks> {
        let (said, strings, hub) = (Rc::clone(&tally.said), strings.clone(), hub.clone());
        let say: Say = Box::new(move |told| {
            said.borrow_mut().push(told);
            tell(&strings, &hub, told);
        });
        polish_saying(reader, controls, tally, how, say)
    }

    /// The `fallback` events the hub keeps for a client that connects later.
    fn kept_fallbacks(hub: &Hub) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
        let mut kept = Vec::new();
        for text in hub.subscribe().0 {
            let event: Value = serde_json::from_str(&text)?;
            if event["type"] == "fallback" {
                kept.push(event);
            }
        }
        Ok(kept)
    }

    /// Everything published on the hub since `events` was subscribed.
    fn published(
        events: &mut broadcast::Receiver<String>,
    ) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
        let mut all = Vec::new();
        loop {
            match events.try_recv() {
                Ok(text) => all.push(serde_json::from_str(&text)?),
                Err(TryRecvError::Empty) => return Ok(all),
                Err(other) => return Err(other.into()),
            }
        }
    }

    /// The text a group comes to when it was asked for in `lang`.
    fn text_of<R: Reread>(
        reader: &mut R,
        lang: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let refined = reader.reread(&[], lang).map_err(|_| "the reader failed")?;
        Ok(refined.text)
    }

    fn yen_to_dollars() -> Vec<(String, String)> {
        vec![(String::from("円"), String::from("ドル"))]
    }

    #[test]
    fn a_refined_text_gets_itn_then_punctuation_then_the_dictionary() -> TestResult {
        let (controls, _commands) = controls_over(true);
        assert!(controls.set_replacements(yen_to_dollars()));
        let tally = Tally::default();
        let mut polish = polish_over(
            Says("三千二百円", "ja"),
            &controls,
            &tally,
            Behaviour::Marks,
        );
        assert_eq!(text_of(&mut polish, "ja")?, "3200ドル。");
        // The model read the sentence after ITN and before the dictionary.
        assert_eq!(*tally.read.borrow(), ["3200円"]);
        Ok(())
    }

    #[test]
    fn marks_follow_the_language_of_the_second_reading_and_only_japanese() -> TestResult {
        let (controls, _commands) = controls_over(true);
        // Asked for as English, read as Japanese: the second reading decides.
        let tally = Tally::default();
        let mut polish = polish_over(
            Says("こんにちは", "ja"),
            &controls,
            &tally,
            Behaviour::Marks,
        );
        assert_eq!(text_of(&mut polish, "en")?, "こんにちは。");
        // Asked for as Japanese, read as English: no marks and no model.
        let tally = Tally::default();
        let mut polish = polish_over(Says("hello", "en"), &controls, &tally, Behaviour::Marks);
        assert_eq!(text_of(&mut polish, "ja")?, "hello");
        assert_eq!(tally.opened.get(), 0);
        // Chinese gets ITN and no marks.
        let tally = Tally::default();
        let mut polish = polish_over(Says("三千二百", "zh"), &controls, &tally, Behaviour::Marks);
        assert_eq!(text_of(&mut polish, "zh")?, "3200");
        assert_eq!(tally.opened.get(), 0);
        Ok(())
    }

    #[test]
    fn a_blank_text_does_not_wake_the_model() -> TestResult {
        let (controls, _commands) = controls_over(true);
        let tally = Tally::default();
        let mut polish = polish_over(Says("  ", "ja"), &controls, &tally, Behaviour::Marks);
        assert_eq!(text_of(&mut polish, "ja")?, "  ");
        assert_eq!(tally.opened.get(), 0);
        Ok(())
    }

    #[test]
    fn a_model_that_cannot_be_opened_is_tried_once_and_said_once() -> TestResult {
        let (controls, _commands) = controls_over(true);
        let tally = Tally::default();
        let mut polish = polish_over(
            Says("こんにちは", "ja"),
            &controls,
            &tally,
            Behaviour::FailsToOpen,
        );
        for _ in 0..3 {
            assert_eq!(text_of(&mut polish, "ja")?, "こんにちは");
        }
        assert_eq!((tally.opened.get(), tally.said.borrow().len()), (1, 1));
        Ok(())
    }

    #[test]
    fn a_sentence_the_model_cannot_read_goes_on_without_marks() -> TestResult {
        let (controls, _commands) = controls_over(true);
        let tally = Tally::default();
        let mut polish = polish_over(
            Says("こんにちは", "ja"),
            &controls,
            &tally,
            Behaviour::FailsToRead,
        );
        assert_eq!(text_of(&mut polish, "ja")?, "こんにちは");
        assert_eq!(text_of(&mut polish, "ja")?, "こんにちは");
        // Each sentence is told as a sentence that failed; the model itself was opened.
        assert_eq!(tally.opened.get(), 1);
        assert_eq!(*tally.said.borrow(), [Told::Failed { audio_ms: 0 }; 2]);
        Ok(())
    }

    #[test]
    fn punctuation_that_is_off_opens_no_model_and_says_nothing() -> TestResult {
        // The live engine has no punctuation model: the view says it is off.
        let (controls, _commands) = controls_over(false);
        let tally = Tally::default();
        let mut polish = polish_over(
            Says("こんにちは", "ja"),
            &controls,
            &tally,
            Behaviour::FailsToOpen,
        );
        polish.warm();
        assert_eq!(text_of(&mut polish, "ja")?, "こんにちは");
        assert_eq!((tally.opened.get(), tally.said.borrow().len()), (0, 0));
        Ok(())
    }

    #[test]
    fn the_model_is_opened_at_the_start_when_punctuation_is_on() -> TestResult {
        let (controls, _commands) = controls_over(true);
        let tally = Tally::default();
        let mut polish = polish_over(
            Says("こんにちは", "ja"),
            &controls,
            &tally,
            Behaviour::Marks,
        );
        polish.warm();
        assert_eq!(tally.opened.get(), 1);
        assert_eq!(text_of(&mut polish, "ja")?, "こんにちは。");
        assert_eq!(tally.opened.get(), 1);
        Ok(())
    }

    /// H148: the dictionary set through `/replacements` after the pass began.
    #[test]
    fn the_refine_reader_uses_the_dictionary_set_after_it_started() -> TestResult {
        let (controls, _commands) = controls_over(false);
        let tally = Tally::default();
        let mut polish = polish_over(
            Says("三千二百円", "ja"),
            &controls,
            &tally,
            Behaviour::Marks,
        );
        assert_eq!(text_of(&mut polish, "ja")?, "3200円");
        assert!(controls.set_replacements(yen_to_dollars()));
        assert_eq!(text_of(&mut polish, "ja")?, "3200ドル");
        assert!(controls.set_replacements(Vec::new()));
        assert_eq!(text_of(&mut polish, "ja")?, "3200円");
        Ok(())
    }

    /// H148: the ITN overrides set through `/itn_overrides` after the pass began.
    #[test]
    fn the_refine_reader_uses_the_itn_overrides_set_after_it_started() -> TestResult {
        let (controls, _commands) = controls_over(false);
        let tally = Tally::default();
        let mut polish = polish_over(
            Says("三千二百人", "ja"),
            &controls,
            &tally,
            Behaviour::Marks,
        );
        assert_eq!(text_of(&mut polish, "ja")?, "3200人");
        let forced = ItnBody {
            exclude: Vec::new(),
            force: vec![(String::from("三千二百"), String::from("3,200"))],
        };
        assert!(controls.set_itn(forced));
        assert_eq!(text_of(&mut polish, "ja")?, "3,200人");
        let excluded = ItnBody {
            exclude: vec![String::from("三千二百人")],
            force: Vec::new(),
        };
        assert!(controls.set_itn(excluded));
        assert_eq!(text_of(&mut polish, "ja")?, "三千二百人");
        Ok(())
    }

    /// H148: punctuation switched through `/config` after the pass began.
    #[test]
    fn the_refine_reader_follows_punctuation_switched_after_it_started() -> TestResult {
        let (controls, _commands) = controls_over(true);
        let tally = Tally::default();
        let mut polish = polish_over(
            Says("こんにちは", "ja"),
            &controls,
            &tally,
            Behaviour::Marks,
        );
        let switch = |on| ConfigChange {
            punctuate: Some(on),
            vad: None,
        };
        assert!(controls.apply_config(switch(false)));
        assert_eq!(text_of(&mut polish, "ja")?, "こんにちは");
        assert_eq!(tally.opened.get(), 0);
        assert!(controls.apply_config(switch(true)));
        assert_eq!(text_of(&mut polish, "ja")?, "こんにちは。");
        Ok(())
    }

    fn args_of(list: &[&str]) -> Result<Args, Box<dyn std::error::Error>> {
        let raw: Vec<String> = list.iter().map(|word| (*word).to_owned()).collect();
        let Ok(Parsed::Run(args)) = args::parse(&raw) else {
            return Err("the command line did not ask for a run".into());
        };
        Ok(*args)
    }

    fn refiner_over(list: &[&str]) -> Result<Option<Refiner>, Box<dyn std::error::Error>> {
        let args = args_of(list)?;
        // No punctuation model: the worker opens nothing while it starts.
        let (controls, _commands) = controls_over(false);
        let strings = Strings::embedded(Lang::En)?;
        Ok(start_refiner(&args, &strings, &Hub::new(), &controls))
    }

    /// R13-20b (a), T-RP-29-029: a worker thread that cannot be started leaves
    /// the refine pass out, and the run says so once.
    #[test]
    fn a_refiner_that_cannot_start_falls_back_and_the_run_goes_on() -> TestResult {
        let args = args_of(&["--wav", "a.wav"])?;
        let (controls, _commands) = controls_over(false);
        let strings = Strings::embedded(Lang::En)?;
        let notice = Notice::refine_disabled();
        // No thread: no refiner, and the notice is sent to the screen once.
        let hub = Hub::new();
        let mut events = hub.subscribe_live();
        let refused = start_refiner_with(&args, &strings, &hub, &controls, |_, _, _, _| {
            Err(std::io::Error::other("no thread"))
        });
        assert!(refused.is_none());
        assert_eq!(kept_fallbacks(&hub)?, [notice.wire()]);
        assert_eq!(published(&mut events)?, [notice.wire()]);
        // The log event, its code, and the three stderr lines come from the same notice.
        let line: Value = serde_json::from_str(&log::fallback_line(&strings, &notice))?;
        assert_eq!(line["event"], "refine.fallback.disabled");
        assert_eq!(line["error_code"], "HAYAMIMI_REFINE_FALLBACK");
        assert_eq!(line["stage"], "refine");
        assert_eq!(line["reason"], "spawn_failed");
        assert_eq!(
            notice.stderr_keys(),
            ["stage_refine", "reason_spawn_failed", "to_disabled"]
        );
        // A thread that can be started: a refiner, and nothing is said.
        let hub = Hub::new();
        let Some(mut refiner) =
            start_refiner_with(&args, &strings, &hub, &controls, Refiner::start)
        else {
            return Err("a worker that can start gave no refiner".into());
        };
        refiner.close();
        assert!(kept_fallbacks(&hub)?.is_empty());
        Ok(())
    }

    /// RP-61: a run that does not say `--no-refine` reads the groups again.
    #[test]
    fn run_without_no_refine_gives_the_session_a_refiner() -> TestResult {
        let Some(mut refiner) = refiner_over(&["--wav", "a.wav"])? else {
            return Err("no refiner was made".into());
        };
        assert!(refiner.worker_running());
        refiner.close();
        assert!(!refiner.worker_running());
        Ok(())
    }

    #[test]
    fn run_with_no_refine_leaves_the_session_without_a_refiner() -> TestResult {
        assert!(refiner_over(&["--wav", "a.wav", "--no-refine"])?.is_none());
        Ok(())
    }

    /// The directory that holds `models/` at or above this checkout: model
    /// files are not in git, so a worktree reads them from the main checkout.
    fn models_above() -> Result<PathBuf, Box<dyn std::error::Error>> {
        let start = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).canonicalize()?;
        let found = start
            .ancestors()
            .map(|up| up.join("models"))
            .find(|models| models.join("silero_vad.onnx").is_file())
            .ok_or("no models/silero_vad.onnx at or above the checkout")?;
        Ok(found)
    }

    /// The middle of the sorted values.
    fn median(mut values: Vec<f64>) -> Result<f64, Box<dyn std::error::Error>> {
        values.sort_by(f64::total_cmp);
        let middle = values.get(values.len() / 2).ok_or("no values")?;
        Ok(*middle)
    }

    /// Japanese without marks, `chars` characters long.
    fn group_text(chars: usize) -> String {
        const BASE: &str = "今日は天気が良いので散歩に出かけましたそれから友達と昼ご飯を食べて午後は図書館で本を読みました";
        BASE.chars().cycle().take(chars).collect()
    }

    /// The seconds the punctuation model of the refine pass takes for the whole
    /// text of a group, on the CPU. Run by hand, from the built test binary:
    /// `-- --ignored --exact run::tests::refine_punctuation_seconds_of_a_group_on_the_cpu --nocapture`.
    #[test]
    #[ignore = "measures the refine pass's punctuation model on the CPU; reads the models"]
    fn refine_punctuation_seconds_of_a_group_on_the_cpu() -> TestResult {
        let began = Instant::now();
        let mut model = open_cpu_punctuator(models_above()?)?;
        eprintln!("open_s={:.3}", began.elapsed().as_secs_f64());
        for chars in [125_usize, 250, 500] {
            let text = group_text(chars);
            let mut runs = Vec::new();
            for _ in 0..5 {
                let began = Instant::now();
                let marked = model.restore(&text)?;
                runs.push(began.elapsed().as_secs_f64());
                assert!(!marked.is_empty());
            }
            eprintln!("chars={chars} runs_s={runs:.3?}");
            eprintln!("chars={chars} median_s={:.3}", median(runs)?);
        }
        Ok(())
    }

    /// The resident memory (VmRSS, KiB) of this process.
    #[cfg(target_os = "linux")]
    fn rss_kib() -> Result<u64, Box<dyn std::error::Error>> {
        let status = std::fs::read_to_string("/proc/self/status")?;
        let line = status
            .lines()
            .find(|line| line.starts_with("VmRSS:"))
            .ok_or("no VmRSS line")?;
        let kib = line.split_whitespace().nth(1).ok_or("no VmRSS value")?;
        Ok(kib.parse::<u64>()?)
    }

    /// Twenty-five seconds of the golden clips, joined.
    #[cfg(target_os = "linux")]
    fn group_audio() -> Result<Vec<f32>, Box<dyn std::error::Error>> {
        let directory =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden/ja");
        let wanted = 25 * hayamimi_asr::audio::SAMPLE_RATE as usize;
        let mut audio = Vec::new();
        for clip in ["ja_014.wav", "ja_024.wav", "ja_033.wav"] {
            audio.extend(read_wav(&directory.join(clip))?);
        }
        audio.truncate(wanted);
        Ok(audio)
    }

    /// The memory the refine pass adds beside a live engine, all on the CPU:
    /// after the engine opened and warmed up, after the first group was read
    /// again, after the first restore. A CPU engine's values, not the GPU
    /// production ones. Run by hand, from the built test binary.
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "measures the memory of the refine pass beside a live engine on the CPU; reads the models"]
    fn refine_pass_memory_beside_the_live_engine_on_the_cpu() -> TestResult {
        let models = models_above()?;
        let mut settings = Settings::new(models.clone());
        settings.gpu = Gpu::Absent(Reason::NoDevice);
        let mut engine = Engine::open(settings)?;
        assert!(engine.warm_up().is_some_and(|read| read.is_ok()));
        let live = rss_kib()?;
        let mut reader = Rereader::new(&models, Policy::balanced());
        let audio = group_audio()?;
        let began = Instant::now();
        let refined = reader
            .reread(&audio, "ja")
            .map_err(|_| "the second reading failed")?;
        let reread_s = began.elapsed().as_secs_f64();
        let after_reread = rss_kib()?;
        let mut model = open_cpu_punctuator(models)?;
        let began = Instant::now();
        let marked = model.restore(&refined.text)?;
        let restore_s = began.elapsed().as_secs_f64();
        let after_restore = rss_kib()?;
        eprintln!(
            "refined_chars={} marked_chars={}",
            refined.text.chars().count(),
            marked.chars().count()
        );
        eprintln!("reread_s={reread_s:.3} first_restore_s={restore_s:.3}");
        eprintln!("rss_kib live={live} after_reread={after_reread} after_restore={after_restore}");
        eprintln!(
            "rss_kib reread_adds={} restore_adds={}",
            after_reread.saturating_sub(live),
            after_restore.saturating_sub(after_reread)
        );
        Ok(())
    }

    /// R13-20b (b), T-RP-29-030: a sentence the model cannot read goes on
    /// without marks and is told once, as a log event only; a sentence it can
    /// read is not told.
    #[test]
    fn a_failed_refine_punctuation_is_said_as_an_event() -> TestResult {
        let (controls, _commands) = controls_over(true);
        let strings = Strings::embedded(Lang::En)?;
        // Two seconds of audio: the event names the length of the group.
        let audio = vec![0.0_f32; 2 * 16_000];
        for (how, want) in [
            (
                Behaviour::FailsToRead,
                vec![Told::Failed { audio_ms: 2000 }; 2],
            ),
            (Behaviour::Marks, Vec::new()),
        ] {
            let hub = Hub::new();
            let mut events = hub.subscribe_live();
            let tally = Tally::default();
            let reader = Says("こんにちは", "ja");
            let mut polish = polish_telling(reader, &controls, &tally, how, &strings, &hub);
            let first = polish
                .reread(&audio, "ja")
                .map_err(|_| "the reader failed")?;
            polish
                .reread(&audio, "ja")
                .map_err(|_| "the reader failed")?;
            let marks = if how == Behaviour::Marks { "。" } else { "" };
            assert_eq!(first.text, format!("こんにちは{marks}"), "{how:?}");
            assert_eq!(*tally.said.borrow(), want, "{how:?}");
            // Told in the log only: nothing reaches the screen.
            assert!(published(&mut events)?.is_empty(), "{how:?}");
        }
        Ok(())
    }

    /// `refine.punct.failed` has the shape of the live `asr.punct.failed`, and
    /// names the group by its length where the live line names the utterance.
    #[test]
    fn the_refine_punctuation_event_is_a_warning_that_names_the_group_and_not_its_text()
    -> TestResult {
        for lang in [Lang::En, Lang::Ja] {
            let line = punct_failed_line(&Strings::embedded(lang)?, 2000);
            let event: Value = serde_json::from_str(&line)?;
            assert_eq!(event["event"], "refine.punct.failed");
            assert_eq!(event["level"], "warn");
            assert_eq!(event["component"], "asr");
            assert!(event["error_code"].is_null());
            assert_eq!(event["audio_ms"], 2000);
            assert!(event.get("id").is_none());
            assert_ne!(
                event["message"], "refine.punct.failed",
                "{lang:?} has a message"
            );
        }
        Ok(())
    }

    /// R13-20b (c), T-RP-29-031: a model that cannot be opened is told once on
    /// the three channels as the stage `refine_punct`, however many groups
    /// follow, and the fallback of the live punctuation stays kept.
    #[test]
    fn an_unavailable_refine_punctuation_sends_one_fallback_event() -> TestResult {
        let (controls, _commands) = controls_over(true);
        let strings = Strings::embedded(Lang::En)?;
        let live = Notice::from_fallback(&Fallback {
            stage: Stage::Punct,
            reason: Reason::NoDevice,
            to: To::Cpu,
        })
        .wire();
        let unavailable = Notice::refine_punct_disabled(Reason::Missing).wire();
        for (how, want) in [
            (Behaviour::FailsToOpen, vec![unavailable.clone()]),
            (Behaviour::Marks, Vec::new()),
        ] {
            let hub = Hub::new();
            hub.publish_fallback(&live);
            let mut events = hub.subscribe_live();
            let tally = Tally::default();
            let reader = Says("こんにちは", "ja");
            let mut polish = polish_telling(reader, &controls, &tally, how, &strings, &hub);
            text_of(&mut polish, "ja")?;
            text_of(&mut polish, "ja")?;
            assert_eq!(published(&mut events)?, want, "{how:?}");
            let kept = kept_fallbacks(&hub)?;
            assert!(
                kept.contains(&live),
                "{how:?}: the live fallback stays kept"
            );
            assert_eq!(kept.contains(&unavailable), !want.is_empty(), "{how:?}");
        }
        Ok(())
    }

    /// An open that fails for another reason than a missing file is `unreadable`.
    #[test]
    fn an_unreadable_refine_punctuation_model_is_said_as_unreadable() -> TestResult {
        let (controls, _commands) = controls_over(true);
        for (how, reason) in [
            (Behaviour::FailsToOpenUnreadable, Reason::Unreadable),
            (Behaviour::FailsToOpen, Reason::Missing),
        ] {
            let tally = Tally::default();
            let mut polish = polish_over(Says("こんにちは", "ja"), &controls, &tally, how);
            text_of(&mut polish, "ja")?;
            assert_eq!(*tally.said.borrow(), [Told::Unavailable(reason)], "{how:?}");
        }
        Ok(())
    }

    /// R13-20b (j), T-RP-29-032: the punctuation model of the refine pass is
    /// opened on the CPU. The model is not opened here; the device asked for is.
    #[test]
    fn the_refine_punctuator_is_opened_on_the_cpu() -> TestResult {
        let asked: RefCell<Vec<Device>> = RefCell::new(Vec::new());
        open_on_refine_device(std::path::Path::new("models"), |_, device| {
            asked.borrow_mut().push(device);
            Ok(())
        })?;
        assert_eq!(*asked.borrow(), [Device::Cpu]);
        Ok(())
    }
}

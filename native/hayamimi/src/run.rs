//! A run of the program: parse the command line, check what the machine has,
//! open the models, start the input and the servers, and recognise until the
//! input ends or the run is stopped.
//!
//! A missing GPU, model, library, port or audio device does not stop the run.
//! Each one is found at the start (devices also later), the stage that needs
//! it takes another road, and the change is said once in the log, on the
//! screen (`/events`) and on stderr.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread::JoinHandle;
use std::time::Instant;

use axum::Router;
use hayamimi_asr::audio::read_wav;
use hayamimi_asr::replace::Replacements;
use hayamimi_asr::vad::{Sensitivity, VadSettings};
use hayamimi_asr::{AsrError, Engine, Gpu, Settings, check_library};
use hayamimi_gpu::memory::MemoryMeter;

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
use crate::report::{log_sessions, say, say_notice, spawn_capture_reporter};
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

/// Serves `router` on `port` or the first free port after it. A server that
/// moved, or could not be started because every port in reach was taken,
/// adds a notice.
fn start_listener(
    router: Router,
    ip: IpAddr,
    port: u16,
    endpoint: &Endpoint,
    notices: &mut Vec<Notice>,
) -> Result<(), RunError> {
    let Ok(listener) = server::bind_on(ip, port) else {
        notices.push((endpoint.notice)(None));
        return Ok(());
    };
    let address = server::serve(router, listener, endpoint.name)?;
    if port != 0 && address.port() != port {
        notices.push((endpoint.notice)(Some((endpoint.url)(address))));
    }
    Ok(())
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
        let ip = IpAddr::V4(Ipv4Addr::LOCALHOST);
        start_listener(routes::router(state), ip, port, &EVENTS, notices)?;
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
    let engine = open_engine(settings, strings, &mut notices)?;
    controls
        .live()
        .engine_opened(engine.as_ref().is_some_and(Engine::can_punctuate));
    tell_notices(strings, &hub, &notices, gpu);
    let stop = install_stop()?;
    let _input = start_input(args, strings, &hub, audio_tx, samples)?;
    hub.publish_session_start();
    Session {
        engine,
        guard: open_guard(args, strings, gpu),
        hub: &hub,
        strings,
        controls: &controls,
        audio: audio_rx,
        commands: command_rx,
        stop: &stop,
        partial: args.io.partial,
        live: is_live(args),
        source: args.input().source(),
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
    use crate::inputs::IoArgs;
    use hayamimi_asr::Reason;

    fn args_with(input: Option<Input>, replace: Option<&str>) -> Args {
        Args {
            mode: Mode::Single,
            lang: None,
            wav: "a.wav".into(),
            realtime: false,
            models: "models".into(),
            min_silence: 0.35,
            max_speech: 12.0,
            replace: replace.map(Into::into),
            serve: None,
            diagnose: false,
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
}

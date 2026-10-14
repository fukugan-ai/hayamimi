//! A run of the program: parse the command line, check what the machine has,
//! open the models, play the wav file through the Japanese route and report.
//!
//! A missing GPU, model, library or port does not stop the run. Each one is
//! found at the start, the stage that needs it takes another road, and the
//! change is said once in the log, on the screen (`/events`) and on stderr.

use std::time::{Duration, Instant};

use hayamimi_asr::audio::{SAMPLE_RATE, WINDOW, read_wav, windows};
use hayamimi_asr::replace::Replacements;
use hayamimi_asr::vad::VadSettings;
use hayamimi_asr::{AsrError, Engine, Final, Gpu, Settings, check_library};
use serde_json::{Value, json};

use crate::args::{self, Args, Parsed};
use crate::gpu_check;
use crate::log;
use crate::notice::Notice;
use crate::server::{self, Hub};
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
    Server(std::io::Error),
}

impl From<AsrError> for RunError {
    fn from(error: AsrError) -> Self {
        RunError::Asr(error)
    }
}

/// Prints table texts to stderr, one per line.
fn say(strings: &Strings, keys: &[&str]) {
    for key in keys {
        eprintln!("{}", strings.cli_text(key));
    }
}

/// Reports a run that stopped on an error: the log event on stdout, and on
/// stderr the table's sentence followed by the detail, which is data (a file
/// name) and so is not in any table.
fn report_failure(strings: &Strings, error: &RunError) {
    log::print(&log::engine_failed_line(strings));
    let detail = match error {
        RunError::Asr(other) => other.to_string(),
        RunError::Server(other) => other.to_string(),
    };
    eprintln!("{}", strings.cli_text(keys::RUN_FAILED));
    eprintln!("  {detail}");
}

/// The wire form of a finished utterance.
fn final_event(done: &Final) -> Value {
    let t = &done.timings;
    json!({
        "type": "final",
        "text": done.text,
        "lang": "ja",
        "speaker": done.speaker.label,
        "latency_ms": t.l_dec_ms,
        "tier": "rz",
        "audio_s": done.audio_seconds,
        "lid_ms": t.lid_ms,
        "decode_ms": t.decode_ms + t.itn_ms + t.punct_ms,
        "switched": false,
    })
}

/// Logs and publishes what an utterance made.
fn announce(strings: &Strings, hub: &Hub, finals: &[Final]) {
    for done in finals {
        log::print(&log::utterance_timed_line(strings, done));
        if done.punct_error.is_some() {
            log::print(&log::punct_failed_line(strings, done));
        }
        if done.speaker.error.is_some() {
            log::print(&log::speaker_failed_line(strings, done));
        }
        hub.publish(&final_event(done));
    }
}

/// Says one fallback on the three channels: the log, the screen and the user.
fn say_notice(strings: &Strings, hub: &Hub, notice: &Notice) {
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

/// Waits until `index` windows of audio have gone by since `start`.
fn pace(start: Instant, index: usize) {
    let seconds = (index * WINDOW) as f64 / f64::from(SAMPLE_RATE);
    let due = start + Duration::from_secs_f64(seconds);
    if let Some(wait) = due.checked_duration_since(Instant::now()) {
        std::thread::sleep(wait);
    }
}

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
    let mut notice = None;
    if let Some(path) = &args.replace {
        match Replacements::load(path) {
            Ok(replacements) => settings.replacements = replacements,
            Err(_) => notice = Some(Notice::config_unreadable()),
        }
    }
    (settings, notice)
}

/// The URL events are served at.
fn events_url(address: std::net::SocketAddr) -> String {
    format!("http://{address}/events")
}

/// Starts the event server on `port` or the first free port after it. A
/// server that moved, or could not be started because every port in reach was
/// taken, adds a notice.
fn start_server(hub: &Hub, port: u16, notices: &mut Vec<Notice>) -> Result<(), RunError> {
    let Ok(listener) = server::bind(port) else {
        notices.push(Notice::port_in_use(None));
        return Ok(());
    };
    let address = server::start(hub.clone(), listener).map_err(RunError::Server)?;
    if port != 0 && address.port() != port {
        notices.push(Notice::port_in_use(Some(events_url(address))));
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

/// Logs the sessions an opened engine created.
fn log_sessions(strings: &Strings, engine: &Engine) {
    for report in engine.sessions() {
        log::print(&log::session_created_line(strings, report));
        if let Some(line) = log::session_placed_line(strings, report) {
            log::print(&line);
        }
    }
}

/// Opens the engine, or notes that the inference libraries did not open and
/// there is none. The fallbacks of the opened stages are added to `notices`.
fn open_engine(
    args: &Args,
    strings: &Strings,
    gpu: Gpu,
    notices: &mut Vec<Notice>,
) -> Result<Option<Engine>, RunError> {
    let (settings, config_notice) = settings_of(args, gpu);
    notices.extend(config_notice);
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

/// Feeds the samples to the engine, window by window, and ends with what it holds.
fn play(
    args: &Args,
    strings: &Strings,
    hub: &Hub,
    samples: &[f32],
    mut engine: Option<Engine>,
) -> Result<(), RunError> {
    let start = Instant::now();
    for (index, window) in windows(samples).enumerate() {
        if args.realtime {
            pace(start, index);
        }
        if let Some(running) = engine.as_mut() {
            announce(strings, hub, &running.feed(&window)?);
        }
    }
    if let Some(running) = engine.as_mut() {
        announce(strings, hub, &running.finish()?);
    }
    Ok(())
}

/// Plays the wav file through the route.
fn transcribe_wav(args: &Args, strings: &Strings) -> Result<(), RunError> {
    let samples = read_wav(&args.wav)?;
    let hub = Hub::new();
    let mut notices = Vec::new();
    if let Some(port) = args.serve {
        start_server(&hub, port, &mut notices)?;
    }
    let gpu = check_gpu(strings);
    let engine = open_engine(args, strings, gpu, &mut notices)?;
    tell_notices(strings, &hub, &notices, gpu);
    hub.publish_session_start();
    play(args, strings, &hub, &samples, engine)
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
    match transcribe_wav(&args, &strings) {
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
    use hayamimi_asr::speaker_stage::Mark;
    use hayamimi_asr::{Reason, Timings};

    fn finished() -> Final {
        Final {
            id: 1,
            text: String::from("テスト"),
            audio_seconds: 1.5,
            timings: Timings {
                vad_ms: 1.0,
                lid_ms: 0.0,
                decode_ms: 100.0,
                punct_ms: 10.0,
                itn_ms: 1.0,
                l_dec_ms: 120.0,
            },
            punct_error: None,
            speaker: Mark::none(),
        }
    }

    fn args_with_replace(path: Option<&str>) -> Args {
        Args {
            wav: "a.wav".into(),
            realtime: false,
            models: "models".into(),
            min_silence: 0.35,
            max_speech: 12.0,
            replace: path.map(Into::into),
            serve: None,
            diagnose: false,
        }
    }

    #[test]
    fn the_wire_form_has_the_keys_of_the_existing_final_event() {
        let event = final_event(&finished());
        for key in [
            "type",
            "text",
            "lang",
            "speaker",
            "latency_ms",
            "tier",
            "audio_s",
            "lid_ms",
            "decode_ms",
            "switched",
        ] {
            assert!(event.get(key).is_some(), "{key} is missing");
        }
        assert_eq!(event["type"], "final");
        assert_eq!(event["latency_ms"], 120.0);
        assert_eq!(event["decode_ms"], 111.0);
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
            settings_of(&args_with_replace(Some("no-such-dictionary.txt")), Gpu::Try);
        assert!(settings.replacements.is_empty());
        assert_eq!(notice, Some(Notice::config_unreadable()));
    }

    #[test]
    fn no_dictionary_asked_for_means_no_notice() {
        let (_, notice) = settings_of(&args_with_replace(None), Gpu::Try);
        assert_eq!(notice, None);
    }

    #[test]
    fn the_state_of_the_gpu_reaches_the_settings() {
        let (settings, _) = settings_of(&args_with_replace(None), Gpu::Absent(Reason::NoDevice));
        assert_eq!(settings.gpu, Gpu::Absent(Reason::NoDevice));
    }

    #[test]
    fn a_taken_port_moves_the_server_and_adds_a_notice() -> Result<(), Box<dyn std::error::Error>> {
        let taken = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
        let port = taken.local_addr()?.port();
        let mut notices = Vec::new();
        start_server(&Hub::new(), port, &mut notices).map_err(|_| "the server did not start")?;
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].event, "server.fallback.port");
        assert_eq!(notices[0].reason, "in_use");
        assert!(notices[0].url.is_some());
        Ok(())
    }

    #[test]
    fn a_free_port_adds_no_notice() -> Result<(), Box<dyn std::error::Error>> {
        let mut notices = Vec::new();
        start_server(&Hub::new(), 0, &mut notices).map_err(|_| "the server did not start")?;
        assert!(notices.is_empty());
        Ok(())
    }
}

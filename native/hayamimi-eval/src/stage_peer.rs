//! A second audio source decoding while a run measures.
//!
//! Two sources are two `hayamimi` processes on one machine, each with its own
//! recognizer. The peer is such a process: it decodes the recording's
//! utterances over and over (the worst case, a source that never pauses)
//! until its standard input closes, so the sessions of the two never share an
//! address space and the measured run meets the same contention for the CPU
//! cores and for the device that a second source brings.
//!
//! The protocol is two lines on the peer's standard output: `ready` once its
//! recognizer decoded one utterance, and `decodes <n>` when it stops.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::EvalError;
use crate::stage_models::{self as models, Precision};
use crate::stage_runs::{Clip, Provider, assert_provider_loaded, load_clips};

const READY: &str = "ready";
const DECODES: &str = "decodes ";
const SAMPLE_RATE: i32 = models::SAMPLE_RATE;

fn invalid(message: impl Into<String>) -> EvalError {
    EvalError::Invalid(message.into())
}

fn io_error(what: &str, error: std::io::Error) -> EvalError {
    invalid(format!("{what}: {error}"))
}

/// The precision a source runs on: what the recognizer of the live pipeline uses on `provider`.
pub fn source_precision(provider: Provider) -> Precision {
    match provider {
        Provider::Cuda => Precision::Fp32,
        Provider::Cpu => Precision::Int8,
    }
}

/// What the peer process is told.
pub struct PeerArgs {
    pub provider: Provider,
    pub wav: PathBuf,
    pub manifest: PathBuf,
    pub models: PathBuf,
    pub precision: Precision,
}

/// The decodes of the whole recording, in order, again and again until `stop` is set.
/// Returns how many decodes finished. One decode happens before `ready` is announced.
fn decode_until(
    args: &PeerArgs,
    clips: &[Clip],
    stop: &AtomicBool,
    announce: impl FnOnce() -> Result<(), EvalError>,
) -> Result<u64, EvalError> {
    let recognizer = models::recognizer(&args.models, args.provider.name(), args.precision)?;
    assert_provider_loaded(args.provider)?;
    let decode = |clip: &Clip| {
        let stream = recognizer.create_stream();
        stream.accept_waveform(SAMPLE_RATE, &clip.samples);
        recognizer.decode(&stream);
        stream.get_result().is_some()
    };
    let Some(first) = clips.first() else {
        return Err(invalid("the recording holds no utterance"));
    };
    if !decode(first) {
        return Err(invalid("the recognizer gave no result"));
    }
    announce()?;
    let mut decodes = 0_u64;
    while !stop.load(Ordering::Relaxed) {
        for clip in clips {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            if !decode(clip) {
                return Err(invalid("the recognizer gave no result"));
            }
            decodes += 1;
        }
    }
    Ok(decodes)
}

/// Runs as the peer: decodes until standard input closes, then reports the count.
pub fn run_peer(args: &PeerArgs) -> Result<(), EvalError> {
    let clips = load_clips(&args.wav, &args.manifest)?;
    let stop = Arc::new(AtomicBool::new(false));
    let watcher = {
        let stop = Arc::clone(&stop);
        std::thread::spawn(move || {
            let mut sink = Vec::new();
            // The parent closing the pipe is the signal; what it may send before is not read.
            let _ = std::io::stdin().lock().read_to_end(&mut sink);
            stop.store(true, Ordering::Relaxed);
        })
    };
    let decodes = decode_until(args, &clips, &stop, || announce(READY))?;
    announce(&format!("{DECODES}{decodes}"))?;
    watcher
        .join()
        .map_err(|_| invalid("the stdin watcher panicked"))
}

fn announce(line: &str) -> Result<(), EvalError> {
    let mut out = std::io::stdout().lock();
    writeln!(out, "{line}").map_err(|e| io_error("stdout", e))?;
    out.flush().map_err(|e| io_error("stdout", e))
}

struct Running {
    child: Child,
    out: BufReader<ChildStdout>,
}

/// The peer processes of a run. Dropping them kills and waits for any still running.
pub struct Peers {
    running: Vec<Running>,
}

/// The command line that starts one peer, for `exe`.
pub fn peer_command(exe: &Path, args: &PeerArgs) -> Command {
    let mut command = Command::new(exe);
    command
        .arg("stages-peer")
        .args(["--provider", args.provider.name()])
        .arg("--wav")
        .arg(&args.wav)
        .arg("--manifest")
        .arg(&args.manifest)
        .arg("--models")
        .arg(&args.models)
        .args(["--precision", args.precision.name()]);
    command
}

/// The count on the peer's `decodes <n>` line.
fn parse_decodes(output: &str) -> Result<u64, EvalError> {
    let Some(line) = output.lines().find_map(|line| line.strip_prefix(DECODES)) else {
        return Err(invalid("the peer did not report its decodes"));
    };
    line.trim()
        .parse()
        .map_err(|_| invalid(format!("{line:?} is not a number of decodes")))
}

impl Peers {
    pub fn none() -> Self {
        Self {
            running: Vec::new(),
        }
    }

    /// Starts one peer per command and waits until each is decoding.
    pub fn start(commands: Vec<Command>) -> Result<Self, EvalError> {
        let mut peers = Self::none();
        for mut command in commands {
            command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit());
            let mut child = command
                .spawn()
                .map_err(|e| io_error("cannot start a peer", e))?;
            let Some(stdout) = child.stdout.take() else {
                // A child that is not recorded in `running` is not waited for by `Drop`.
                let _ = child.kill();
                let _ = child.wait();
                return Err(invalid("the peer has no standard output"));
            };
            peers.running.push(Running {
                child,
                out: BufReader::new(stdout),
            });
            peers.wait_ready()?;
        }
        Ok(peers)
    }

    fn wait_ready(&mut self) -> Result<(), EvalError> {
        let Some(last) = self.running.last_mut() else {
            return Ok(());
        };
        let mut line = String::new();
        loop {
            line.clear();
            let read = last
                .out
                .read_line(&mut line)
                .map_err(|e| io_error("reading a peer", e))?;
            if read == 0 {
                let status = last
                    .child
                    .wait()
                    .map_err(|e| io_error("waiting for a peer", e))?;
                return Err(invalid(format!(
                    "a peer ended before it was ready: {status}"
                )));
            }
            if line.trim() == READY {
                return Ok(());
            }
        }
    }

    /// Stops every peer, waits for each, and returns the decodes they finished.
    pub fn stop(mut self) -> Result<u64, EvalError> {
        let mut total = 0_u64;
        for mut peer in self.running.drain(..) {
            drop(peer.child.stdin.take());
            let mut rest = String::new();
            peer.out
                .read_to_string(&mut rest)
                .map_err(|e| io_error("reading a peer", e))?;
            let status = peer
                .child
                .wait()
                .map_err(|e| io_error("waiting for a peer", e))?;
            if !status.success() {
                return Err(invalid(format!("a peer ended with {status}")));
            }
            total += parse_decodes(&rest)?;
        }
        Ok(total)
    }
}

impl Drop for Peers {
    fn drop(&mut self) {
        for peer in &mut self.running {
            // Whatever is left was never stopped in order; the child is killed and collected.
            let _ = peer.child.kill();
            let _ = peer.child.wait();
        }
    }
}

/// The program that runs `stages-peer`: this executable.
pub fn current_exe() -> Result<PathBuf, EvalError> {
    std::env::current_exe().map_err(|e| io_error("current_exe", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_count_is_read_from_the_last_line() -> Result<(), EvalError> {
        assert_eq!(parse_decodes("ready\ndecodes 42\n")?, 42);
        Ok(())
    }

    #[test]
    fn a_peer_that_reports_nothing_is_an_error() {
        assert!(parse_decodes("").is_err());
        assert!(parse_decodes("ready\n").is_err());
        assert!(parse_decodes("decodes many\n").is_err());
    }

    #[test]
    fn a_source_runs_the_precision_of_its_provider() {
        assert_eq!(source_precision(Provider::Cuda), Precision::Fp32);
        assert_eq!(source_precision(Provider::Cpu), Precision::Int8);
    }

    #[test]
    fn no_peers_report_no_decodes() -> Result<(), EvalError> {
        assert_eq!(Peers::none().stop()?, 0);
        Ok(())
    }

    #[test]
    fn the_command_names_every_input_of_the_peer() {
        let args = PeerArgs {
            provider: Provider::Cpu,
            wav: PathBuf::from("a.wav"),
            manifest: PathBuf::from("m.json"),
            models: PathBuf::from("models"),
            precision: Precision::Int8,
        };
        let command = peer_command(Path::new("exe"), &args);
        let words: Vec<String> = command
            .get_args()
            .map(|word| word.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            words,
            [
                "stages-peer",
                "--provider",
                "cpu",
                "--wav",
                "a.wav",
                "--manifest",
                "m.json",
                "--models",
                "models",
                "--precision",
                "int8"
            ]
        );
    }

    /// A peer that follows the protocol with a shell: ready, wait for the pipe to close, report.
    #[cfg(unix)]
    fn shell_peer(script: &str) -> Command {
        let mut command = Command::new("sh");
        command.args(["-c", script]);
        command
    }

    #[cfg(unix)]
    #[test]
    fn peers_are_started_ready_stopped_and_counted() -> Result<(), EvalError> {
        let script = "echo ready; cat > /dev/null; echo 'decodes 7'";
        let peers = Peers::start(vec![shell_peer(script), shell_peer(script)])?;
        assert_eq!(peers.stop()?, 14);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn a_peer_that_ends_before_it_is_ready_is_an_error() {
        let started = Peers::start(vec![shell_peer("exit 3")]);
        assert!(
            matches!(&started, Err(EvalError::Invalid(m)) if m.contains("before it was ready")),
            "{:?}",
            started.err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_peer_that_fails_when_stopped_is_an_error() -> Result<(), EvalError> {
        let peers = Peers::start(vec![shell_peer("echo ready; cat > /dev/null; exit 2")])?;
        assert!(peers.stop().is_err());
        Ok(())
    }
}

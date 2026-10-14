//! The ways of starting the server, one after another: start it, wait until it
//! answers, and when it does not, start it the next way. The ways come from
//! [`super::plan::attempts`]; here is only what an attempt does and how the
//! sequence ends.

use std::io;
use std::path::PathBuf;
use std::process::Child;
use std::time::{Duration, Instant};

use hayamimi_relay::llm::HttpLlm;

use super::layout::Layout;
use super::plan::{HOST, Plan, server_args};
use super::spawn::spawn_detached;

/// How often a started server is asked whether it answers.
const POLL: Duration = Duration::from_millis(500);

/// Why one way of starting did not leave a server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttemptError {
    /// The build's executable is not installed.
    MissingBuild,
    /// The operating system would not start it.
    SpawnFailed(io::ErrorKind),
    /// It ended before it answered; the code is `None` when a signal ended it.
    Exited(Option<i32>),
    /// It was still running and did not answer in time.
    NotHealthy,
}

impl AttemptError {
    /// The reason name used in events.
    pub fn reason(self) -> &'static str {
        match self {
            AttemptError::MissingBuild => "missing_build",
            AttemptError::SpawnFailed(_) => "spawn_failed",
            AttemptError::Exited(_) => "exited",
            AttemptError::NotHealthy => "not_healthy",
        }
    }
}

/// A server that answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Running {
    pub pid: u32,
}

/// Starts the server one way and waits until it answers.
pub trait Launcher {
    fn launch(&mut self, plan: Plan) -> Result<Running, AttemptError>;
}

/// The way that worked, and the ways before it that did not.
#[derive(Debug, PartialEq, Eq)]
pub struct Started {
    pub plan: Plan,
    pub running: Running,
    pub failed: Vec<(Plan, AttemptError)>,
}

/// Tries `plans` in order and stops at the first that leaves a server; the
/// error is every way that failed, in order.
pub fn first_that_runs(
    launcher: &mut dyn Launcher,
    plans: &[Plan],
) -> Result<Started, Vec<(Plan, AttemptError)>> {
    let mut failed = Vec::new();
    for plan in plans {
        match launcher.launch(*plan) {
            Ok(running) => {
                return Ok(Started {
                    plan: *plan,
                    running,
                    failed,
                });
            }
            Err(error) => failed.push((*plan, error)),
        }
    }
    Err(failed)
}

/// The base URL of the server on `port`.
pub fn base_url(port: u16) -> String {
    format!("http://{HOST}:{port}")
}

/// Whether a server already answers on `port`.
pub fn answers(port: u16) -> bool {
    HttpLlm::new(&base_url(port)).health().is_ok()
}

/// Waits until the server answers, or its process ends, or `wait` is over.
fn wait_until_it_answers(child: &mut Child, port: u16, wait: Duration) -> Result<(), AttemptError> {
    let deadline = Instant::now() + wait;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Err(AttemptError::Exited(status.code())),
            Ok(None) => {}
            Err(error) => return Err(AttemptError::SpawnFailed(error.kind())),
        }
        if answers(port) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(AttemptError::NotHealthy);
        }
        std::thread::sleep(POLL);
    }
}

/// Ends a server that did not come up, and collects it so no process is left.
fn stop(child: &mut Child) {
    // The process may have ended already; either way it is collected next.
    let _ = child.kill();
    let _ = child.wait();
}

/// The real launcher: the installed executables, detached from this process.
pub struct Installed {
    pub layout: Layout,
    pub model: PathBuf,
    pub port: u16,
    pub slots: u32,
    pub wait: Duration,
}

impl Launcher for Installed {
    fn launch(&mut self, plan: Plan) -> Result<Running, AttemptError> {
        let exe = self.layout.server(plan.build);
        if !exe.is_file() {
            return Err(AttemptError::MissingBuild);
        }
        let args = server_args(plan, &self.model, self.port, self.slots);
        let mut child = spawn_detached(&exe, &args, &self.layout.log())
            .map_err(|error| AttemptError::SpawnFailed(error.kind()))?;
        match wait_until_it_answers(&mut child, self.port, self.wait) {
            // The server is left to run on purpose: the handle is dropped
            // without a wait, and the server is the user's to stop.
            Ok(()) => Ok(Running { pid: child.id() }),
            Err(error) => {
                stop(&mut child);
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llama::plan::Build;

    const CUDA: Plan = Plan {
        build: Build::Cuda,
        gpu_layers: 99,
    };
    const CPU: Plan = Plan {
        build: Build::Cpu,
        gpu_layers: 0,
    };

    /// Answers each plan from a script and remembers which were tried.
    struct Scripted {
        answers: Vec<Result<Running, AttemptError>>,
        tried: Vec<Plan>,
    }

    impl Launcher for Scripted {
        fn launch(&mut self, plan: Plan) -> Result<Running, AttemptError> {
            self.tried.push(plan);
            self.answers.remove(0)
        }
    }

    fn scripted(answers: Vec<Result<Running, AttemptError>>) -> Scripted {
        Scripted {
            answers,
            tried: Vec::new(),
        }
    }

    #[test]
    fn the_first_way_that_works_ends_the_sequence() {
        let mut launcher = scripted(vec![Ok(Running { pid: 7 })]);
        let started = first_that_runs(&mut launcher, &[CUDA, CPU]);
        assert_eq!(
            started,
            Ok(Started {
                plan: CUDA,
                running: Running { pid: 7 },
                failed: Vec::new(),
            })
        );
        assert_eq!(launcher.tried, [CUDA]);
    }

    #[test]
    fn a_way_that_fails_is_followed_by_the_next_and_remembered() {
        let mut launcher = scripted(vec![Err(AttemptError::NotHealthy), Ok(Running { pid: 9 })]);
        let started = first_that_runs(&mut launcher, &[CUDA, CPU]);
        assert_eq!(
            started,
            Ok(Started {
                plan: CPU,
                running: Running { pid: 9 },
                failed: vec![(CUDA, AttemptError::NotHealthy)],
            })
        );
        assert_eq!(launcher.tried, [CUDA, CPU]);
    }

    #[test]
    fn when_every_way_fails_the_error_is_every_way_in_order() {
        let mut launcher = scripted(vec![
            Err(AttemptError::Exited(Some(1))),
            Err(AttemptError::MissingBuild),
        ]);
        assert_eq!(
            first_that_runs(&mut launcher, &[CUDA, CPU]),
            Err(vec![
                (CUDA, AttemptError::Exited(Some(1))),
                (CPU, AttemptError::MissingBuild),
            ])
        );
    }

    #[test]
    fn no_way_to_try_is_an_empty_failure() {
        let mut launcher = scripted(Vec::new());
        assert_eq!(first_that_runs(&mut launcher, &[]), Err(Vec::new()));
    }

    #[test]
    fn each_failure_has_a_reason_name() {
        assert_eq!(AttemptError::MissingBuild.reason(), "missing_build");
        assert_eq!(
            AttemptError::SpawnFailed(io::ErrorKind::NotFound).reason(),
            "spawn_failed"
        );
        assert_eq!(AttemptError::Exited(None).reason(), "exited");
        assert_eq!(AttemptError::NotHealthy.reason(), "not_healthy");
    }

    #[test]
    fn the_url_is_the_loopback_address_and_the_port() {
        assert_eq!(base_url(8099), "http://127.0.0.1:8099");
    }

    #[test]
    fn nothing_answers_on_a_port_nobody_listens_on() {
        assert!(!answers(1));
    }

    fn installed(root: &str, wait: Duration) -> Installed {
        Installed {
            layout: Layout::new(PathBuf::from(root)),
            model: PathBuf::from("m.gguf"),
            port: 1,
            slots: 1,
            wait,
        }
    }

    #[test]
    fn a_build_that_is_not_installed_is_reported_without_starting_anything() {
        let mut launcher = installed("no-such-root-for-hayamimi", Duration::from_secs(1));
        assert_eq!(launcher.launch(CPU), Err(AttemptError::MissingBuild));
    }
}

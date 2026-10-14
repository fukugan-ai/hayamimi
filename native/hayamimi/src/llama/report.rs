//! What `hayamimi llama start` says: log events on stdout, and for the user a
//! few sentences from the locale table on stderr. A path or a model name is
//! never in an event.

use hayamimi_relay::logging::Level;
use serde_json::Value;

use super::keys;
use super::plan::{Plan, SLOTS};
use super::start::{AttemptError, Started};
use crate::log::event_line;
use crate::notice::Notice;
use crate::strings::Strings;

/// The component of every event of the command.
const COMPONENT: &str = "translate";
/// The error code of a way of starting that did not come up.
const FAILED_CODE: &str = "HAYAMIMI_TRANSLATE_SERVER_FAILED";
/// The error code of a start that left no server.
const UNAVAILABLE_CODE: &str = "HAYAMIMI_TRANSLATE_SERVER_UNAVAILABLE";

fn plan_fields(plan: Plan) -> [(&'static str, Value); 2] {
    [
        ("build", Value::from(plan.build.name())),
        ("gpu_layers", Value::from(plan.gpu_layers)),
    ]
}

/// `translate.server.started`: a server answers, and where it runs.
pub fn started_line(strings: &Strings, started: &Started, port: u16, tag: Option<&str>) -> String {
    let [build, layers] = plan_fields(started.plan);
    event_line(
        strings,
        Level::Info,
        keys::EVENT_STARTED,
        COMPONENT,
        None,
        &[
            build,
            layers,
            ("slots", Value::from(SLOTS)),
            ("port", Value::from(port)),
            ("pid", Value::from(started.running.pid)),
            ("tag", Value::from(tag)),
        ],
    )
}

/// `translate.server.running`: a server already answered, so none was started.
pub fn running_line(strings: &Strings, port: u16) -> String {
    event_line(
        strings,
        Level::Info,
        keys::EVENT_RUNNING,
        COMPONENT,
        None,
        &[("port", Value::from(port))],
    )
}

/// `translate.server.failed`: one way of starting that did not come up.
pub fn failed_line(strings: &Strings, plan: Plan, error: AttemptError) -> String {
    let [build, layers] = plan_fields(plan);
    event_line(
        strings,
        Level::Warn,
        keys::EVENT_FAILED,
        COMPONENT,
        Some(FAILED_CODE),
        &[build, layers, ("reason", Value::from(error.reason()))],
    )
}

/// `translate.server.unavailable`: the start left no server. `reason` is
/// `model_missing` or `attempts_failed`.
pub fn unavailable_line(strings: &Strings, reason: &str) -> String {
    event_line(
        strings,
        Level::Error,
        keys::EVENT_UNAVAILABLE,
        COMPONENT,
        Some(UNAVAILABLE_CODE),
        &[("reason", Value::from(reason))],
    )
}

/// The sentences for a stage that moved to the CPU: the stage, the reason and
/// where it went, then how to check the GPU. The sentences are table keys.
pub fn fallback_keys(notice: &Notice) -> Vec<String> {
    let mut keys = notice.stderr_keys().to_vec();
    keys.push(String::from(crate::strings::keys::NO_GPU_CHECK));
    keys
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llama::plan::Build;
    use crate::llama::start::Running;
    use hayamimi_asr::Reason;
    use hayamimi_relay::lang::Lang;

    fn strings() -> Result<Strings, String> {
        Strings::embedded(Lang::En)
    }

    fn parsed(line: &str) -> Result<Value, serde_json::Error> {
        serde_json::from_str(line)
    }

    const CPU: Plan = Plan {
        build: Build::Cpu,
        gpu_layers: 0,
    };

    fn started_on_cpu() -> Started {
        Started {
            plan: CPU,
            running: Running { pid: 42 },
            failed: Vec::new(),
        }
    }

    #[test]
    fn the_started_event_says_where_the_server_runs_and_which_release() -> Result<(), String> {
        let line = started_line(&strings()?, &started_on_cpu(), 8099, Some("b1"));
        let event = parsed(&line).map_err(|error| error.to_string())?;
        assert_eq!(event["event"], keys::EVENT_STARTED);
        assert_eq!(event["level"], "info");
        assert_eq!(event["build"], "cpu");
        assert_eq!(event["gpu_layers"], 0);
        assert_eq!(event["slots"], 4);
        assert_eq!(event["port"], 8099);
        assert_eq!(event["pid"], 42);
        assert_eq!(event["tag"], "b1");
        Ok(())
    }

    #[test]
    fn an_unknown_release_is_null_and_not_a_made_up_value() -> Result<(), String> {
        let line = started_line(&strings()?, &started_on_cpu(), 8099, None);
        let event = parsed(&line).map_err(|error| error.to_string())?;
        assert!(event["tag"].is_null());
        Ok(())
    }

    #[test]
    fn a_failed_way_is_a_warning_with_its_reason_and_error_code() -> Result<(), String> {
        let line = failed_line(&strings()?, CPU, AttemptError::NotHealthy);
        let event = parsed(&line).map_err(|error| error.to_string())?;
        assert_eq!(event["event"], keys::EVENT_FAILED);
        assert_eq!(event["level"], "warn");
        assert_eq!(event["error_code"], FAILED_CODE);
        assert_eq!(event["reason"], "not_healthy");
        assert_eq!(event["build"], "cpu");
        Ok(())
    }

    #[test]
    fn a_start_that_left_no_server_is_an_error() -> Result<(), String> {
        let line = unavailable_line(&strings()?, "attempts_failed");
        let event = parsed(&line).map_err(|error| error.to_string())?;
        assert_eq!(event["level"], "error");
        assert_eq!(event["error_code"], UNAVAILABLE_CODE);
        assert_eq!(event["reason"], "attempts_failed");
        Ok(())
    }

    #[test]
    fn a_running_server_is_an_info_with_its_port() -> Result<(), String> {
        let line = running_line(&strings()?, 8099);
        let event = parsed(&line).map_err(|error| error.to_string())?;
        assert_eq!(event["event"], keys::EVENT_RUNNING);
        assert_eq!(event["port"], 8099);
        Ok(())
    }

    #[test]
    fn no_event_carries_a_path_or_a_model_name() -> Result<(), String> {
        let lines = [
            started_line(&strings()?, &started_on_cpu(), 8099, Some("b1")),
            failed_line(&strings()?, CPU, AttemptError::MissingBuild),
            unavailable_line(&strings()?, "model_missing"),
        ];
        for line in lines {
            assert!(!line.contains(".gguf") && !line.contains('\\') && !line.contains(".exe"));
        }
        Ok(())
    }

    #[test]
    fn the_fallback_sentences_name_the_stage_the_reason_the_destination_and_the_check() {
        let notice = Notice::translate_on_cpu(Reason::NoDevice);
        assert_eq!(
            fallback_keys(&notice),
            [
                "stage_translate",
                "reason_no_device",
                "to_cpu",
                "no_gpu_check"
            ]
        );
    }
}

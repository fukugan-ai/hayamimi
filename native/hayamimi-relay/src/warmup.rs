//! The first request of a run: one short request in the form of T, sent to the
//! translator before the relay says that it serves, so that the first line of
//! the run does not pay for the translator's first request. The answer is
//! dropped. A request that fails is said in the log and the run goes on.

use std::time::{Duration, Instant};

use crate::lang::Lang;
use crate::llm::{Completion, Llm};
use crate::logging::{Level, LogEvent, LogSink};
use crate::pipeline::translate_prompt;

/// The log event of a first request that was answered.
pub const SENT_EVENT: &str = "translate.warmup.sent";
/// The log event of a first request that was not answered.
pub const FAILED_EVENT: &str = "translate.warmup.failed";

/// The sentence the first request translates.
const TEXT: &str = "こんにちは。";

/// How many tokens the answer may take; the answer is not used.
pub const TOKENS: u32 = 8;

/// How long the first request is waited for. A translator that has just
/// started answers its first request later than a live subtitle may wait.
pub const TIMEOUT: Duration = Duration::from_secs(10);

/// The user message of the first request: T's prompt for a short sentence.
pub fn prompt() -> String {
    translate_prompt(Lang::Ja, TEXT)
}

/// Sends the first request to `llm` and logs how it went.
pub fn warm_up<L: Llm>(llm: &L, sink: &mut dyn LogSink) {
    let prompt = prompt();
    let started = Instant::now();
    let answered = llm.complete(&Completion::new(&prompt, TOKENS));
    let duration = u64::try_from(started.elapsed().as_millis()).ok();
    match answered {
        Ok(_) => sink.emit(
            LogEvent::new(Level::Info, SENT_EVENT, "service")
                .with("dependency", "llama-server")
                .with("duration_ms", duration),
        ),
        Err(error) => sink.emit(
            LogEvent::new(Level::Warn, FAILED_EVENT, "service")
                .with("dependency", "llama-server")
                .with("duration_ms", duration)
                .with(
                    "detail",
                    error.to_string().split(':').next().map(str::to_owned),
                )
                .with_error("LLM_UNAVAILABLE"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;
    use crate::llm::LlmError;
    use crate::logging::MemorySink;

    /// What the translator was asked, and what it answers.
    struct Scripted {
        asked: RefCell<Vec<(String, Option<String>, u32)>>,
        answer: fn() -> Result<String, LlmError>,
    }

    impl Scripted {
        fn answering(answer: fn() -> Result<String, LlmError>) -> Scripted {
            Scripted {
                asked: RefCell::new(Vec::new()),
                answer,
            }
        }
    }

    impl Llm for Scripted {
        fn complete(&self, request: &Completion<'_>) -> Result<String, LlmError> {
            self.asked.borrow_mut().push((
                request.prompt.to_owned(),
                request.system.map(str::to_owned),
                request.n_predict,
            ));
            (self.answer)()
        }
    }

    #[test]
    fn the_first_request_is_one_t_prompt_in_one_message() {
        let llm = Scripted::answering(|| Ok(String::from("Hello.")));
        warm_up(&llm, &mut MemorySink::default());
        let asked = llm.asked.borrow();
        assert_eq!(
            *asked,
            [(
                String::from(
                    "Translate the following Japanese text into English.\n\n こんにちは。"
                ),
                None,
                TOKENS
            )]
        );
    }

    #[test]
    fn an_answered_request_logs_one_info_event_with_its_time() {
        let llm = Scripted::answering(|| Ok(String::from("Hello.")));
        let mut sink = MemorySink::default();
        warm_up(&llm, &mut sink);
        let [event] = sink.events.as_slice() else {
            panic!("{} events, not 1", sink.events.len());
        };
        assert_eq!(
            (event.event.as_str(), event.level),
            (SENT_EVENT, Level::Info)
        );
        assert!(event.fields.contains_key("duration_ms"));
        assert_eq!(event.error_code, None);
    }

    #[test]
    fn a_failed_request_logs_one_warn_event_and_returns() {
        let llm = Scripted::answering(|| Err(LlmError::Status(503)));
        let mut sink = MemorySink::default();
        warm_up(&llm, &mut sink);
        let [event] = sink.events.as_slice() else {
            panic!("{} events, not 1", sink.events.len());
        };
        assert_eq!(
            (event.event.as_str(), event.level),
            (FAILED_EVENT, Level::Warn)
        );
        assert_eq!(
            event.fields.get("detail"),
            Some(&serde_json::json!("status 503"))
        );
        assert!(event.error_code.is_some());
        assert_eq!(llm.asked.borrow().len(), 1);
    }

    #[test]
    fn a_transport_failure_keeps_the_cause_out_of_the_event() {
        let llm = Scripted::answering(|| {
            Err(LlmError::Transport(String::from(
                "connect to 127.0.0.1:1 refused",
            )))
        });
        let mut sink = MemorySink::default();
        warm_up(&llm, &mut sink);
        let Some(event) = sink.events.first() else {
            panic!("no event");
        };
        assert_eq!(
            event.fields.get("detail"),
            Some(&serde_json::json!("transport"))
        );
        assert!(!event.to_json_line().contains("127.0.0.1"));
    }
}

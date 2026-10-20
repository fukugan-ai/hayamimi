//! The first requests of a run: one short request in the form of T, then one
//! in the form of E (a system message and T's user message), sent one after
//! the other to the translator before the relay says that it serves, so that
//! the first line of the run pays for neither form. The answers are dropped.
//! A request that fails is said in the log and the run goes on.

use std::time::{Duration, Instant};

use crate::lang::Lang;
use crate::llm::{Completion, Llm, LlmError};
use crate::logging::{Level, LogEvent, LogSink};
use crate::pipeline::{edit_system, translate_prompt};
use crate::style::{Calques, JaStyle};

/// The log event of a first request that was answered.
pub const SENT_EVENT: &str = "translate.warmup.sent";
/// The log event of a first request that was not answered.
pub const FAILED_EVENT: &str = "translate.warmup.failed";

/// The sentence both first requests translate.
const TEXT: &str = "こんにちは。";

/// The draft the system message of the request in the form of E carries.
const DRAFT: &str = "Hello.";

/// How many tokens an answer may take; the answers are not used.
pub const TOKENS: u32 = 8;

/// How long the two first requests are waited for together. A translator that
/// has just started answers its first request later than a live subtitle may
/// wait.
pub const TIMEOUT: Duration = Duration::from_secs(10);

/// Which form a first request has; it is the `form` field of its log event.
#[derive(Clone, Copy)]
enum Form {
    T,
    E,
}

impl Form {
    fn name(self) -> &'static str {
        match self {
            Form::T => "t",
            Form::E => "e",
        }
    }
}

/// The user message of both first requests: T's prompt for a short sentence.
pub fn prompt() -> String {
    translate_prompt(Lang::Ja, TEXT)
}

/// The system message of the request in the form of E: the note E reads for a
/// Japanese line, with [`DRAFT`] as T's translation.
pub fn system() -> Result<String, String> {
    let (calques, ja_style) = (Calques::embedded()?, JaStyle::embedded()?);
    Ok(edit_system(Lang::Ja, DRAFT, &calques, &ja_style))
}

/// The kind of a failure, without the cause the error text carries after a colon.
fn kind(error: &LlmError) -> String {
    let text = error.to_string();
    match text.split_once(':') {
        Some((kind, _)) => kind.to_owned(),
        None => text,
    }
}

/// Logs how one first request went: answered when `failure` is `None`.
fn emit(form: Form, ms: Option<u64>, failure: Option<&str>, sink: &mut dyn LogSink) {
    let event = match failure {
        None => LogEvent::new(Level::Info, SENT_EVENT, "service"),
        Some(detail) => LogEvent::new(Level::Warn, FAILED_EVENT, "service")
            .with("detail", detail)
            .with_error("LLM_UNAVAILABLE"),
    };
    sink.emit(
        event
            .with("form", form.name())
            .with("dependency", "llama-server")
            .with("duration_ms", ms),
    );
}

/// Sends one first request and logs how it went.
fn send<L: Llm>(form: Form, llm: &L, request: &Completion<'_>, sink: &mut dyn LogSink) {
    let started = Instant::now();
    let answered = llm.complete(request);
    let ms = u64::try_from(started.elapsed().as_millis()).ok();
    match answered {
        Ok(_) => emit(form, ms, None, sink),
        Err(error) => emit(form, ms, Some(&kind(&error)), sink),
    }
}

/// Sends the request in the form of E when `left` time is left for it.
fn send_edit<L: Llm>(
    connect: &impl Fn(Duration) -> L,
    left: Duration,
    user: &str,
    sink: &mut dyn LogSink,
) {
    let Ok(system) = system() else {
        emit(Form::E, None, Some("table"), sink);
        return;
    };
    if left.is_zero() {
        emit(Form::E, None, Some("budget"), sink);
        return;
    }
    let request = Completion::new(user, TOKENS).with_system(&system);
    send(Form::E, &connect(left), &request, sink);
}

/// Sends the two first requests, the form of T first, and logs how each went.
/// The two together wait at most `budget`: `connect` makes the translator for
/// one request that may take `limit` at most, and the second request is given
/// what the first left.
fn warm_up_within<L: Llm>(
    budget: Duration,
    connect: impl Fn(Duration) -> L,
    sink: &mut dyn LogSink,
) {
    let started = Instant::now();
    let user = prompt();
    send(
        Form::T,
        &connect(budget),
        &Completion::new(&user, TOKENS),
        sink,
    );
    let left = budget.saturating_sub(started.elapsed());
    send_edit(&connect, left, &user, sink);
}

/// Sends the two first requests within [`TIMEOUT`] and logs how each went.
pub fn warm_up<L: Llm>(connect: impl Fn(Duration) -> L, sink: &mut dyn LogSink) {
    warm_up_within(TIMEOUT, connect, sink);
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;
    use crate::logging::MemorySink;

    /// What one request asked: the user message, the system message and the
    /// number of tokens.
    type Asked = (String, Option<String>, u32);

    /// What the translator was asked, and what it answers to the n-th request.
    struct Scripted {
        asked: RefCell<Vec<Asked>>,
        answer: fn(usize) -> Result<String, LlmError>,
    }

    impl Scripted {
        fn answering(answer: fn(usize) -> Result<String, LlmError>) -> Scripted {
            Scripted {
                asked: RefCell::new(Vec::new()),
                answer,
            }
        }
    }

    impl Llm for &Scripted {
        fn complete(&self, request: &Completion<'_>) -> Result<String, LlmError> {
            let nth = self.asked.borrow().len();
            self.asked.borrow_mut().push((
                request.prompt.to_owned(),
                request.system.map(str::to_owned),
                request.n_predict,
            ));
            (self.answer)(nth)
        }
    }

    /// Runs the first requests of `budget` against `llm`; returns the limit
    /// each request was made with.
    fn run(llm: &Scripted, budget: Duration, sink: &mut MemorySink) -> Vec<Duration> {
        let limits = RefCell::new(Vec::new());
        warm_up_within(
            budget,
            |limit| {
                limits.borrow_mut().push(limit);
                llm
            },
            sink,
        );
        limits.into_inner()
    }

    fn hello(_: usize) -> Result<String, LlmError> {
        Ok(String::from("Hello."))
    }

    fn forms_and_events(sink: &MemorySink) -> Vec<(&str, Option<&str>)> {
        sink.events
            .iter()
            .map(|event| {
                (
                    event.event.as_str(),
                    event.fields.get("form").and_then(serde_json::Value::as_str),
                )
            })
            .collect()
    }

    #[test]
    fn the_first_requests_are_t_in_one_message_then_e_in_two() -> Result<(), String> {
        let llm = Scripted::answering(hello);
        run(&llm, TIMEOUT, &mut MemorySink::default());
        let user =
            String::from("Translate the following Japanese text into English.\n\n こんにちは。");
        let system = edit_system(
            Lang::Ja,
            "Hello.",
            &Calques::embedded()?,
            &JaStyle::embedded()?,
        );
        assert!(
            system
                .starts_with("A draft English translation of the user's Japanese text is: Hello.")
        );
        assert_eq!(
            *llm.asked.borrow(),
            [(user.clone(), None, TOKENS), (user, Some(system), TOKENS),]
        );
        Ok(())
    }

    #[test]
    fn answered_requests_log_one_info_event_each_with_their_form_and_time() {
        let llm = Scripted::answering(hello);
        let mut sink = MemorySink::default();
        run(&llm, TIMEOUT, &mut sink);
        assert_eq!(
            forms_and_events(&sink),
            [(SENT_EVENT, Some("t")), (SENT_EVENT, Some("e"))]
        );
        for event in &sink.events {
            assert_eq!(event.level, Level::Info);
            assert_eq!(event.error_code, None);
            assert!(
                event
                    .fields
                    .get("duration_ms")
                    .is_some_and(|ms| ms.is_u64())
            );
        }
    }

    #[test]
    fn a_failed_t_does_not_keep_e_from_being_sent() {
        let llm = Scripted::answering(|nth| match nth {
            0 => Err(LlmError::Status(503)),
            _ => hello(nth),
        });
        let mut sink = MemorySink::default();
        run(&llm, TIMEOUT, &mut sink);
        assert_eq!(
            forms_and_events(&sink),
            [(FAILED_EVENT, Some("t")), (SENT_EVENT, Some("e"))]
        );
        assert_eq!(llm.asked.borrow().len(), 2);
    }

    #[test]
    fn a_failed_e_logs_one_warn_event_and_returns() {
        let llm = Scripted::answering(|nth| match nth {
            0 => hello(nth),
            _ => Err(LlmError::Status(503)),
        });
        let mut sink = MemorySink::default();
        run(&llm, TIMEOUT, &mut sink);
        assert_eq!(
            forms_and_events(&sink),
            [(SENT_EVENT, Some("t")), (FAILED_EVENT, Some("e"))]
        );
        let failed = sink.events.last();
        assert!(failed.is_some_and(|event| {
            event.level == Level::Warn
                && event.error_code.is_some()
                && event.fields.get("detail") == Some(&serde_json::json!("status 503"))
        }));
        assert_eq!(llm.asked.borrow().len(), 2);
    }

    #[test]
    fn a_transport_failure_keeps_the_cause_out_of_both_events() {
        let llm = Scripted::answering(|_| {
            Err(LlmError::Transport(String::from(
                "connect to 127.0.0.1:1 refused",
            )))
        });
        let mut sink = MemorySink::default();
        run(&llm, TIMEOUT, &mut sink);
        assert_eq!(sink.events.len(), 2);
        for event in &sink.events {
            assert_eq!(
                event.fields.get("detail"),
                Some(&serde_json::json!("transport"))
            );
            assert!(!event.to_json_line().contains("127.0.0.1"));
        }
    }

    #[test]
    fn the_first_request_may_take_the_whole_budget_and_the_second_what_is_left() {
        const BUDGET: Duration = Duration::from_secs(5);
        const SLOW: Duration = Duration::from_millis(50);
        let llm = Scripted::answering(|nth| {
            if nth == 0 {
                std::thread::sleep(SLOW);
            }
            hello(nth)
        });
        let limits = run(&llm, BUDGET, &mut MemorySink::default());
        let [first, second] = limits.as_slice() else {
            panic!("{} limits, not 2", limits.len());
        };
        assert_eq!(*first, BUDGET);
        assert!(!second.is_zero() && *second <= BUDGET - SLOW, "{second:?}");
    }

    #[test]
    fn warm_up_gives_the_first_request_the_timeout_of_the_first_requests() {
        let llm = Scripted::answering(hello);
        let limits = RefCell::new(Vec::new());
        warm_up(
            |limit| {
                limits.borrow_mut().push(limit);
                &llm
            },
            &mut MemorySink::default(),
        );
        assert_eq!(limits.borrow().first(), Some(&TIMEOUT));
    }

    #[test]
    fn no_time_left_for_e_is_said_and_e_is_not_sent() {
        const BUDGET: Duration = Duration::from_millis(20);
        let llm = Scripted::answering(|nth| {
            std::thread::sleep(Duration::from_millis(40));
            hello(nth)
        });
        let mut sink = MemorySink::default();
        let limits = run(&llm, BUDGET, &mut sink);
        assert_eq!(limits, [BUDGET]);
        assert_eq!(
            forms_and_events(&sink),
            [(SENT_EVENT, Some("t")), (FAILED_EVENT, Some("e"))]
        );
        let skipped = sink.events.last();
        assert!(skipped.is_some_and(|event| {
            event.fields.get("detail") == Some(&serde_json::json!("budget"))
                && event.fields.get("duration_ms") == Some(&serde_json::Value::Null)
        }));
        assert_eq!(llm.asked.borrow().len(), 1);
    }
}

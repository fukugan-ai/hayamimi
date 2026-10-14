//! Whether the translator can be reached, noticed once per outage.
//!
//! A line that cannot be translated is held and its original text goes on
//! (`pipeline`). That is said once when the outage begins and once when it
//! ends, not once per line. [`Reach`] wraps a translator and sees every
//! answer; [`Watch`] is the handle the caller keeps to read the changes and
//! to report a health check of its own.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};

use serde_json::{Value, json};

use crate::llm::{Completion, Llm, LlmError};
use crate::logging::{Level, LogEvent};

/// The stage name in events.
pub const STAGE: &str = "translate";
/// The log event of the translator becoming unreachable.
pub const HELD_EVENT: &str = "translate.fallback.held";
/// The log event of the translator becoming reachable again.
pub const CLEARED_EVENT: &str = "translate.fallback.cleared";
/// The error code of the held event.
pub const ERROR_CODE: &str = "HAYAMIMI_TRANSLATE_FALLBACK";
/// The reason of the held event.
pub const REASON: &str = "unreachable";

/// A change in whether the translator can be reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// The translator stopped answering; lines are held from now on.
    Held,
    /// The translator answers again.
    Cleared,
}

impl Change {
    /// The log event of the change.
    pub fn event(self) -> LogEvent {
        match self {
            Change::Held => {
                let mut held = LogEvent::new(Level::Warn, HELD_EVENT, "service")
                    .with("stage", STAGE)
                    .with("reason", REASON);
                held.error_code = Some(ERROR_CODE.to_owned());
                held
            }
            Change::Cleared => {
                LogEvent::new(Level::Info, CLEARED_EVENT, "service").with("stage", STAGE)
            }
        }
    }

    /// The `fallback` message for the screens; `to` is `null` when the
    /// translator is back.
    pub fn wire(self) -> Value {
        match self {
            Change::Held => json!({
                "type": "fallback", "stage": STAGE, "reason": REASON, "to": "held",
            }),
            Change::Cleared => json!({"type": "fallback", "stage": STAGE, "to": null}),
        }
    }
}

/// Whether an error means the server cannot take requests now: the
/// connection failed or timed out, or the server answered 5xx (a server that
/// is still loading its model answers 503). A 4xx or an answer that is not
/// the expected JSON came from a server that is there.
pub fn is_unreachable(error: &LlmError) -> bool {
    match error {
        LlmError::Transport(_) => true,
        LlmError::Status(code) => *code >= 500,
        LlmError::Body(_) | LlmError::Aborted => false,
    }
}

struct Shared {
    held: AtomicBool,
    changes: Sender<Change>,
}

impl Shared {
    fn note(&self, reachable: bool) {
        let was_held = self.held.swap(!reachable, Ordering::SeqCst);
        if was_held == reachable {
            let change = if reachable {
                Change::Cleared
            } else {
                Change::Held
            };
            // A caller that dropped its [`Watch`] wants no changes.
            let _ = self.changes.send(change);
        }
    }
}

/// A translator that records whether it can be reached.
pub struct Reach<L: Llm> {
    inner: L,
    shared: Arc<Shared>,
}

/// The caller's handle on a [`Reach`].
pub struct Watch {
    shared: Arc<Shared>,
    changes: Receiver<Change>,
}

impl<L: Llm> Reach<L> {
    /// Wraps `inner`, which is taken to be reachable until it fails.
    pub fn new(inner: L) -> (Reach<L>, Watch) {
        let (sender, receiver) = channel();
        let shared = Arc::new(Shared {
            held: AtomicBool::new(false),
            changes: sender,
        });
        let watch = Watch {
            shared: Arc::clone(&shared),
            changes: receiver,
        };
        (Reach { inner, shared }, watch)
    }
}

impl Watch {
    /// Reports the result of a health check made outside the requests.
    pub fn note(&self, reachable: bool) {
        self.shared.note(reachable);
    }

    /// The changes since the last call, in order.
    pub fn drain(&self) -> Vec<Change> {
        self.changes.try_iter().collect()
    }
}

impl<L: Llm> Llm for Reach<L> {
    fn complete(&self, request: &Completion<'_>) -> Result<String, LlmError> {
        let answer = self.inner.complete(request);
        let unreachable = answer.as_ref().err().is_some_and(is_unreachable);
        self.shared.note(!unreachable);
        answer
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Answers from a script, then fails as unreachable.
    struct Scripted(RefCell<Vec<Result<String, LlmError>>>);

    impl Llm for Scripted {
        fn complete(&self, _request: &Completion<'_>) -> Result<String, LlmError> {
            let mut answers = self.0.borrow_mut();
            if answers.is_empty() {
                return Err(LlmError::Transport(String::from("script ended")));
            }
            answers.remove(0)
        }
    }

    fn down() -> Result<String, LlmError> {
        Err(LlmError::Transport(String::from("refused")))
    }

    fn up() -> Result<String, LlmError> {
        Ok(String::from("text"))
    }

    fn reach_of(answers: Vec<Result<String, LlmError>>) -> (Reach<Scripted>, Watch) {
        Reach::new(Scripted(RefCell::new(answers)))
    }

    fn ask(reach: &Reach<Scripted>, times: usize) {
        for _ in 0..times {
            // Only the changes are looked at; the answers are the script's.
            let _ = reach.complete(&Completion::new("x", 1));
        }
    }

    #[test]
    fn a_reachable_translator_changes_nothing() {
        let (reach, watch) = reach_of(vec![up(), up()]);
        ask(&reach, 2);
        assert!(watch.drain().is_empty());
    }

    #[test]
    fn the_first_failure_is_held_once_however_many_lines_fail() {
        let (reach, watch) = reach_of(vec![up(), down(), down(), down()]);
        ask(&reach, 4);
        assert_eq!(watch.drain(), vec![Change::Held]);
    }

    #[test]
    fn an_answer_after_an_outage_clears_it_once() {
        let (reach, watch) = reach_of(vec![down(), down(), up(), up()]);
        ask(&reach, 4);
        assert_eq!(watch.drain(), vec![Change::Held, Change::Cleared]);
    }

    #[test]
    fn a_second_outage_is_held_again() {
        let (reach, watch) = reach_of(vec![down(), up(), down()]);
        ask(&reach, 3);
        assert_eq!(
            watch.drain(),
            vec![Change::Held, Change::Cleared, Change::Held]
        );
    }

    #[test]
    fn changes_are_read_once() {
        let (reach, watch) = reach_of(vec![down()]);
        ask(&reach, 1);
        assert_eq!(watch.drain(), vec![Change::Held]);
        assert!(watch.drain().is_empty());
    }

    #[test]
    fn a_server_error_is_an_outage_and_a_client_error_is_not() {
        assert!(is_unreachable(&LlmError::Status(503)));
        assert!(is_unreachable(&LlmError::Status(500)));
        assert!(!is_unreachable(&LlmError::Status(499)));
        assert!(!is_unreachable(&LlmError::Status(400)));
        assert!(is_unreachable(&LlmError::Transport(String::new())));
        assert!(!is_unreachable(&LlmError::Body(String::new())));
    }

    #[test]
    fn an_answer_that_is_not_the_expected_json_is_not_an_outage() {
        let (reach, watch) = reach_of(vec![Err(LlmError::Body(String::from("not json")))]);
        ask(&reach, 1);
        assert!(watch.drain().is_empty());
    }

    #[test]
    fn a_health_check_reports_through_the_same_state() {
        let (_reach, watch) = reach_of(Vec::new());
        watch.note(false);
        watch.note(false);
        watch.note(true);
        assert_eq!(watch.drain(), vec![Change::Held, Change::Cleared]);
    }

    #[test]
    fn the_events_name_the_stage_and_only_the_held_one_is_a_warning() {
        let held = Change::Held.event();
        assert_eq!(held.event, HELD_EVENT);
        assert_eq!(held.level, Level::Warn);
        assert_eq!(held.error_code.as_deref(), Some(ERROR_CODE));
        assert_eq!(held.fields.get("stage"), Some(&json!("translate")));
        assert_eq!(held.fields.get("reason"), Some(&json!("unreachable")));
        let cleared = Change::Cleared.event();
        assert_eq!(cleared.event, CLEARED_EVENT);
        assert_eq!(cleared.level, Level::Info);
        assert_eq!(cleared.error_code, None);
    }

    #[test]
    fn the_wire_forms_have_the_keys_of_the_design() {
        let held = Change::Held.wire();
        assert_eq!(held["type"], "fallback");
        assert_eq!(held["stage"], "translate");
        assert_eq!(held["reason"], "unreachable");
        assert_eq!(held["to"], "held");
        let cleared = Change::Cleared.wire();
        assert_eq!(cleared["type"], "fallback");
        assert_eq!(cleared["stage"], "translate");
        assert!(cleared["to"].is_null());
    }

    #[test]
    fn both_events_have_a_message_in_both_tables() -> Result<(), String> {
        use crate::strings::{DEFAULT_EN, DEFAULT_JA, Strings};
        for table in [DEFAULT_EN, DEFAULT_JA] {
            let strings = Strings::parse(table.as_bytes())?;
            for event in [HELD_EVENT, CLEARED_EVENT] {
                assert!(
                    strings.messages.contains_key(event),
                    "{event} has no message"
                );
            }
        }
        Ok(())
    }
}

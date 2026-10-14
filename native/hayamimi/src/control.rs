//! What the control routes share with the recognition loop.
//!
//! The loop owns the engine. A route never touches it: it checks the body,
//! updates [`Live`], the view the `GET` routes read, and sends a
//! [`Command`] that the loop applies between two windows. The detector
//! settings in the view are the ones in effect, not the ones asked for, as a
//! new detector can only replace the old one when it holds no speech.

use std::sync::mpsc::Sender;
use std::sync::{Mutex, MutexGuard};

use hayamimi_asr::itn::Overrides;
use hayamimi_asr::replace::Replacements;
use hayamimi_asr::vad::Sensitivity;
use tokio::sync::oneshot;

use crate::api::{ConfigChange, ItnBody, VadChange};

/// What a route asks the loop to do.
#[derive(Debug)]
pub enum Command {
    Replacements(Replacements),
    Itn(Overrides),
    Punctuate(bool),
    Vad(VadChange),
    /// Ends the session in progress and starts a new one; the sender is told when that is done.
    Reset(oneshot::Sender<()>),
}

#[derive(Debug)]
struct LiveData {
    replacements: Vec<(String, String)>,
    exclude: Vec<String>,
    force: Vec<(String, String)>,
    punctuate: bool,
    can_punctuate: bool,
    vad: Sensitivity,
}

/// The settings in effect, for the `GET` routes.
#[derive(Debug)]
pub struct Live(Mutex<LiveData>);

/// The configuration the loop starts with.
pub struct Initial {
    pub replacements: Replacements,
    pub overrides: Overrides,
    pub punctuate: bool,
    /// Whether the punctuation model is loaded, so that turning it on can take effect.
    pub can_punctuate: bool,
    pub vad: Sensitivity,
}

impl Live {
    pub fn new(initial: Initial) -> Live {
        Live(Mutex::new(LiveData {
            replacements: initial.replacements.pairs().to_vec(),
            exclude: initial.overrides.exclude,
            force: initial.overrides.force,
            punctuate: initial.punctuate && initial.can_punctuate,
            can_punctuate: initial.can_punctuate,
            vad: initial.vad,
        }))
    }

    /// The data holds whole values, so a thread that panicked while holding the lock left it valid.
    fn data(&self) -> MutexGuard<'_, LiveData> {
        match self.0.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    pub fn replacements(&self) -> Vec<(String, String)> {
        self.data().replacements.clone()
    }

    /// The excluded words in sorted order, and the forced pairs in the order given.
    pub fn itn(&self) -> (Vec<String>, Vec<(String, String)>) {
        let data = self.data();
        let mut exclude = data.exclude.clone();
        exclude.sort();
        (exclude, data.force.clone())
    }

    /// Whether punctuation is on, and the detector settings in effect.
    pub fn config(&self) -> (bool, Sensitivity) {
        let data = self.data();
        (data.punctuate, data.vad)
    }

    fn set_replacements(&self, replacements: &Replacements) {
        self.data().replacements = replacements.pairs().to_vec();
    }

    fn set_itn(&self, overrides: &Overrides) {
        let mut data = self.data();
        data.exclude = overrides.exclude.clone();
        data.force = overrides.force.clone();
    }

    fn set_punctuate(&self, on: bool) {
        let mut data = self.data();
        data.punctuate = on && data.can_punctuate;
    }

    /// The loop calls this when a detector with the new settings is in use.
    pub fn set_vad(&self, vad: Sensitivity) {
        self.data().vad = vad;
    }

    /// The engine is open: punctuation is on only if its model loaded.
    pub fn engine_opened(&self, can_punctuate: bool) {
        let mut data = self.data();
        data.can_punctuate = can_punctuate;
        data.punctuate &= can_punctuate;
    }
}

/// The control surface the routes hold.
pub struct Controls {
    live: Live,
    commands: Sender<Command>,
}

impl Controls {
    pub fn new(live: Live, commands: Sender<Command>) -> Controls {
        Controls { live, commands }
    }

    pub fn live(&self) -> &Live {
        &self.live
    }

    /// Sends a command. `false` means the loop has ended.
    fn send(&self, command: Command) -> bool {
        self.commands.send(command).is_ok()
    }

    /// Replaces the dictionary. `false` means the loop has ended.
    pub fn set_replacements(&self, pairs: Vec<(String, String)>) -> bool {
        let replacements = Replacements::from_pairs(pairs);
        self.live.set_replacements(&replacements);
        self.send(Command::Replacements(replacements))
    }

    /// Replaces the ITN overrides. `false` means the loop has ended.
    pub fn set_itn(&self, body: ItnBody) -> bool {
        let overrides = Overrides {
            exclude: body.exclude,
            force: body.force,
        };
        self.live.set_itn(&overrides);
        self.send(Command::Itn(overrides))
    }

    /// Applies a checked configuration change. `false` means the loop has ended.
    pub fn apply_config(&self, change: ConfigChange) -> bool {
        let mut sent = true;
        if let Some(on) = change.punctuate {
            self.live.set_punctuate(on);
            sent &= self.send(Command::Punctuate(on));
        }
        if let Some(vad) = change.vad {
            sent &= self.send(Command::Vad(vad));
        }
        sent
    }

    /// Asks the loop for a session reset; the receiver completes when it is done.
    /// `None` means the loop has ended.
    pub fn reset(&self) -> Option<oneshot::Receiver<()>> {
        let (done, wait) = oneshot::channel();
        self.send(Command::Reset(done)).then_some(wait)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn sensitivity() -> Sensitivity {
        Sensitivity {
            threshold: 0.5,
            min_silence: 0.3,
            max_speech: 12.0,
        }
    }

    fn live(can_punctuate: bool) -> Live {
        Live::new(Initial {
            replacements: Replacements::from_pairs(vec![(String::from("a"), String::from("b"))]),
            overrides: Overrides {
                exclude: vec![String::from("二"), String::from("一")],
                force: vec![(String::from("x"), String::from("y"))],
            },
            punctuate: true,
            can_punctuate,
            vad: sensitivity(),
        })
    }

    #[test]
    fn the_view_starts_with_the_initial_configuration() {
        let view = live(true);
        assert_eq!(view.replacements().len(), 1);
        assert_eq!(view.config(), (true, sensitivity()));
    }

    #[test]
    fn excluded_words_are_listed_sorted_and_forced_pairs_in_order() {
        let (exclude, force) = live(true).itn();
        assert_eq!(exclude, vec![String::from("一"), String::from("二")]);
        assert_eq!(force, vec![(String::from("x"), String::from("y"))]);
    }

    #[test]
    fn punctuation_cannot_be_on_without_its_model() {
        let view = live(false);
        assert!(!view.config().0);
        view.set_punctuate(true);
        assert!(!view.config().0);
    }

    #[test]
    fn an_engine_without_the_punctuation_model_turns_the_view_off() {
        let view = live(true);
        assert!(view.config().0);
        view.engine_opened(false);
        assert!(!view.config().0);
        view.set_punctuate(true);
        assert!(!view.config().0);
    }

    #[test]
    fn a_dictionary_update_changes_the_view_and_sends_the_command() -> Result<(), String> {
        let (tx, rx) = mpsc::channel();
        let controls = Controls::new(live(true), tx);
        assert!(controls.set_replacements(vec![
            (String::from("p"), String::from("q")),
            (String::new(), String::from("dropped")),
        ]));
        assert_eq!(
            controls.live().replacements(),
            vec![(String::from("p"), String::from("q"))]
        );
        match rx.try_recv() {
            Ok(Command::Replacements(sent)) => assert_eq!(sent.len(), 1),
            other => return Err(format!("unexpected {other:?}")),
        }
        Ok(())
    }

    #[test]
    fn a_config_change_sends_one_command_per_member() {
        let (tx, rx) = mpsc::channel();
        let controls = Controls::new(live(true), tx);
        let sent = controls.apply_config(ConfigChange {
            punctuate: Some(false),
            vad: Some(VadChange::default()),
        });
        assert!(sent);
        assert!(!controls.live().config().0);
        assert!(matches!(rx.try_recv(), Ok(Command::Punctuate(false))));
        assert!(matches!(rx.try_recv(), Ok(Command::Vad(_))));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn an_empty_change_sends_nothing() {
        let (tx, rx) = mpsc::channel();
        let controls = Controls::new(live(true), tx);
        assert!(controls.apply_config(ConfigChange::default()));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn commands_to_an_ended_loop_are_reported() {
        let (tx, rx) = mpsc::channel();
        let controls = Controls::new(live(true), tx);
        drop(rx);
        assert!(!controls.set_itn(ItnBody::default()));
        assert!(controls.reset().is_none());
    }

    #[test]
    fn a_reset_hands_over_a_receiver_that_completes_when_the_loop_answers()
    -> Result<(), Box<dyn std::error::Error>> {
        let (tx, rx) = mpsc::channel();
        let controls = Controls::new(live(true), tx);
        let mut wait = controls.reset().ok_or("no receiver")?;
        assert!(wait.try_recv().is_err());
        let Command::Reset(done) = rx.try_recv()? else {
            return Err("not a reset".into());
        };
        assert!(done.send(()).is_ok());
        assert!(wait.try_recv().is_ok());
        Ok(())
    }
}

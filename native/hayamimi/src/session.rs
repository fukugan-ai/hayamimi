//! The recognition loop: reads windows of audio from the queue, feeds the
//! engine, publishes what it makes, and applies the commands of the control
//! routes between two windows. It is the only place that touches the engine.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

use hayamimi_asr::audio::{SAMPLE_RATE, WINDOW};
use hayamimi_asr::vad::Sensitivity;
use hayamimi_asr::{AsrError, Engine, Final};
use serde_json::Value;

use crate::control::{Command, Controls};
use crate::feed::{AudioItem, AudioRx};
use crate::log;
use crate::server::Hub;
use crate::strings::Strings;
use crate::wire;

/// How long the loop waits for audio before it looks at the commands and the stop flag again.
const TICK: Duration = Duration::from_millis(20);
/// A live input is behind real time by more than this many windows (about 128 ms):
/// no drafts are made until it has caught up, so that finals are not delayed by them.
const BEHIND_WINDOWS: usize = 4;

/// What a session has made so far, for `session_summary`.
#[derive(Debug, Default)]
pub struct Stats {
    samples: u64,
    segments: u64,
    latency_sum_ms: f64,
    latency_max_ms: Option<f64>,
}

impl Stats {
    fn add_window(&mut self, samples: usize) {
        self.samples += samples as u64;
    }

    fn add_final(&mut self, done: &Final) {
        let latency = done.timings.l_dec_ms;
        self.segments += 1;
        self.latency_sum_ms += latency;
        self.latency_max_ms = Some(
            self.latency_max_ms
                .map_or(latency, |most| most.max(latency)),
        );
    }

    /// The `session_summary` event of what is counted.
    pub fn summary_event(&self) -> Value {
        let mean = (self.segments > 0).then(|| self.latency_sum_ms / self.segments as f64);
        wire::session_summary(
            self.samples as f64 / f64::from(SAMPLE_RATE),
            self.segments,
            mean,
            self.latency_max_ms,
        )
    }
}

/// What the loop is run with.
pub struct Session<'a> {
    pub engine: Option<Engine>,
    pub hub: &'a Hub,
    pub strings: &'a Strings,
    pub controls: &'a Controls,
    pub audio: AudioRx,
    pub commands: Receiver<Command>,
    /// Set by a signal handler to end the run.
    pub stop: &'a AtomicBool,
    /// Whether drafts of the utterance in progress are made.
    pub partial: bool,
    /// Whether the audio arrives at the speed of real time (a device or a
    /// client), so that a queue that has grown means the loop is behind.
    pub live: bool,
    /// The `source` of the utterances: `file`, `mic` or `loopback`.
    pub source: &'static str,
}

/// The state of the loop that is not a setting of the run.
struct State {
    stats: Stats,
    pending_vad: Option<Sensitivity>,
}

impl Session<'_> {
    /// Runs until the input ends, the stop flag is set or the engine fails.
    /// What the detector holds is finished and a summary is published.
    pub fn run(mut self) -> Result<(), AsrError> {
        let mut state = State {
            stats: Stats::default(),
            pending_vad: None,
        };
        while !self.stop.load(Ordering::Relaxed) {
            self.apply_commands(&mut state)?;
            self.retune(&mut state)?;
            match self.audio.recv_timeout(TICK) {
                Ok(AudioItem::Window(window)) => self.on_window(&mut state, &window)?,
                Ok(AudioItem::Flush) => self.flush(&mut state)?,
                Ok(AudioItem::End) | Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
        self.flush(&mut state)?;
        self.hub.publish(&state.stats.summary_event());
        Ok(())
    }

    fn on_window(&mut self, state: &mut State, window: &[f32]) -> Result<(), AsrError> {
        state.stats.add_window(WINDOW);
        // Read before the feed: the wait is the queue's, not the feed's own time.
        let (waited, backlog) = (self.audio.last_wait(), self.audio.backlog());
        let finals = match self.engine.as_mut() {
            Some(engine) => engine.feed(window)?,
            None => return Ok(()),
        };
        self.announce(state, &finals);
        self.say_feed_waits(&finals, waited, backlog);
        self.draft();
        Ok(())
    }

    /// Says how long the window that made each of `finals` waited on the queue
    /// before its feed started. After the finals are published, so a log line
    /// never delays a subtitle.
    fn say_feed_waits(&self, finals: &[Final], waited: Duration, backlog: usize) {
        for done in finals {
            log::print(&log::feed_waited_line(
                self.strings,
                done.id,
                waited,
                backlog,
            ));
        }
    }

    /// Logs and publishes what the utterances made. Every `final` goes out
    /// before the first speaker mark is computed, so a mark never delays a subtitle.
    fn announce(&mut self, state: &mut State, finals: &[Final]) {
        for done in finals {
            state.stats.add_final(done);
            log::print(&log::utterance_timed_line(self.strings, done));
            if done.punct_error.is_some() {
                log::print(&log::punct_failed_line(self.strings, done));
            }
            self.hub.publish(&wire::final_event(done, self.source));
            log::print(&log::final_published_line(
                self.strings,
                done.id,
                log::mono_us(),
            ));
        }
        for done in finals {
            self.announce_speaker(done);
        }
    }

    /// Sends the speaker of one utterance as its own event, once; says so in
    /// the log when the mark failed.
    fn announce_speaker(&mut self, done: &Final) {
        let Some(engine) = self
            .engine
            .as_mut()
            .filter(|engine| engine.has_speaker_stage())
        else {
            return;
        };
        log::print(&log::embedding_started_line(
            self.strings,
            done.id,
            log::mono_us(),
        ));
        let Some(mark) = engine.mark_speaker(done) else {
            return;
        };
        if mark.error.is_some() {
            log::print(&log::speaker_failed_line(self.strings, done));
            return;
        }
        self.hub
            .publish(&wire::speaker_event(done, self.source, &mark.label));
    }

    /// Ends the speech in progress and publishes what it made.
    fn flush(&mut self, state: &mut State) -> Result<(), AsrError> {
        let finals = match self.engine.as_mut() {
            Some(engine) => engine.finish()?,
            None => return Ok(()),
        };
        self.announce(state, &finals);
        Ok(())
    }

    /// Whether a live input has fallen behind real time.
    fn behind(&self) -> bool {
        self.live && self.audio.backlog() > BEHIND_WINDOWS
    }

    /// Publishes a draft of the utterance in progress when one is due. A
    /// draft that fails is skipped and said; the utterance itself is not affected.
    fn draft(&mut self) {
        if !self.partial || self.behind() {
            return;
        }
        let Some(engine) = self.engine.as_mut() else {
            return;
        };
        match engine.partial() {
            Ok(Some(text)) => self.hub.publish(&wire::partial_event(&text)),
            Ok(None) => {}
            Err(_) => log::print(&log::partial_failed_line(self.strings)),
        }
    }

    fn apply_commands(&mut self, state: &mut State) -> Result<(), AsrError> {
        while let Ok(command) = self.commands.try_recv() {
            self.apply(state, command)?;
        }
        Ok(())
    }

    fn apply(&mut self, state: &mut State, command: Command) -> Result<(), AsrError> {
        match command {
            Command::Reset(done) => {
                self.reset(state)?;
                // The route stopped waiting when this fails, and answered 202 already.
                let _ = done.send(());
            }
            Command::Vad(change) => {
                let base = match state.pending_vad {
                    Some(pending) => Some(pending),
                    None => self.engine.as_ref().and_then(Engine::sensitivity),
                };
                state.pending_vad = base.map(|current| change.apply(current));
            }
            other => self.apply_to_engine(other),
        }
        Ok(())
    }

    /// The commands that change a setting of the engine.
    fn apply_to_engine(&mut self, command: Command) {
        let Some(engine) = self.engine.as_mut() else {
            return;
        };
        match command {
            Command::Replacements(replacements) => engine.set_replacements(replacements),
            Command::Itn(overrides) => engine.set_itn_overrides(overrides),
            Command::Punctuate(on) => engine.set_punctuate(on),
            Command::Vad(_) | Command::Reset(_) => {}
        }
    }

    /// Ends the session in progress, says what it made, and starts a new one.
    fn reset(&mut self, state: &mut State) -> Result<(), AsrError> {
        self.flush(state)?;
        self.hub.publish(&state.stats.summary_event());
        state.stats = Stats::default();
        self.hub.publish(&wire::session_reset());
        Ok(())
    }

    /// Moves the detector to the settings asked for, once it holds no speech.
    fn retune(&mut self, state: &mut State) -> Result<(), AsrError> {
        let (Some(wanted), Some(engine)) = (state.pending_vad, self.engine.as_mut()) else {
            return Ok(());
        };
        if engine.retune_vad(wanted)? {
            self.controls.live().set_vad(wanted);
            state.pending_vad = None;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hayamimi_asr::Timings;

    fn done(latency: f64) -> Final {
        Final {
            id: 1,
            text: String::from("テスト"),
            audio_seconds: 1.0,
            t0_s: 0.0,
            t1_s: 1.0,
            end_wall_ms: 0,
            timings: Timings {
                vad_ms: 0.0,
                lid_ms: 0.0,
                decode_ms: 0.0,
                punct_ms: 0.0,
                itn_ms: 0.0,
                l_dec_ms: latency,
            },
            punct_error: None,
            speaker_audio: Vec::new(),
        }
    }

    #[test]
    fn an_empty_session_reports_no_latency_and_no_audio() {
        let event = Stats::default().summary_event();
        assert_eq!(event["stats"]["segments"], 0);
        assert_eq!(event["stats"]["total_audio_s"], 0.0);
        assert!(event["stats"]["mean_latency_ms"].is_null());
        assert!(event["stats"]["max_latency_ms"].is_null());
    }

    #[test]
    fn the_summary_counts_audio_segments_and_the_mean_and_max_latency() {
        let mut stats = Stats::default();
        for _ in 0..(SAMPLE_RATE as usize / WINDOW) {
            stats.add_window(WINDOW);
        }
        stats.add_final(&done(100.0));
        stats.add_final(&done(300.0));
        let event = stats.summary_event();
        assert_eq!(event["stats"]["segments"], 2);
        assert_eq!(event["stats"]["mean_latency_ms"], 200.0);
        assert_eq!(event["stats"]["max_latency_ms"], 300.0);
        // 31 whole windows of a second's worth: 15872 samples.
        let seconds = event["stats"]["total_audio_s"].as_f64();
        assert!(
            seconds.is_some_and(|s| (s - 0.992).abs() < 1e-9),
            "{seconds:?}"
        );
    }

    #[test]
    fn the_max_latency_does_not_fall_when_a_faster_final_follows() {
        let mut stats = Stats::default();
        stats.add_final(&done(300.0));
        stats.add_final(&done(100.0));
        assert_eq!(stats.summary_event()["stats"]["max_latency_ms"], 300.0);
    }
}

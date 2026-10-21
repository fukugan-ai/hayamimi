//! The recognition loop: reads windows of audio from the queue, feeds the
//! engine, publishes what it makes, and applies the commands of the control
//! routes between two windows. It is the only place that touches the engine.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use hayamimi_asr::audio::{SAMPLE_RATE, WINDOW};
use hayamimi_asr::vad::Sensitivity;
use hayamimi_asr::{AsrError, Engine, Final};
use hayamimi_gpu::memory::MemoryMeter;
use serde_json::Value;

use crate::control::{Command, Controls};
use crate::feed::{AudioItem, AudioRx};
use crate::log;
use crate::memory_guard::{Look, MemoryGuard};
use crate::notice::Notice;
use crate::report::{self, log_session, say_notice};
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
    /// The language of the last `final` of the session; none before the first.
    last_final_lang: Option<String>,
}

impl Stats {
    /// Whether `lang` differs from the language of the previous `final` of the
    /// session, and remembers it. The first `final` of a session never switches.
    fn switched_to(&mut self, lang: &str) -> bool {
        let same = self.last_final_lang.as_deref() == Some(lang);
        let switched = self.last_final_lang.is_some() && !same;
        if !same {
            self.last_final_lang = Some(lang.to_owned());
        }
        switched
    }

    /// The `final` event of an utterance, with `switched` read from the
    /// language of the one before it.
    fn final_event(&mut self, done: &Final, source: &str) -> Value {
        let switched = self.switched_to(&done.lang);
        wire::final_event(done, source, switched)
    }

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
    /// Reads the GPU memory; none when there is no GPU or it cannot be read.
    pub guard: Option<MemoryGuard<MemoryMeter>>,
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
        self.tell_resident();
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
        self.keep_within_budget()?;
        // Read before the feed: the wait is the queue's, not the feed's own time.
        let (waited, backlog) = (self.audio.last_wait(), self.audio.backlog());
        let finals = match self.engine.as_mut() {
            Some(engine) => engine.feed(window)?,
            None => return Ok(()),
        };
        self.announce(state, &finals);
        self.say_feed_waits(&finals, waited, backlog);
        self.tell_resident();
        self.draft();
        Ok(())
    }

    /// Reads the GPU memory when it is time to, and moves the next GPU stage
    /// to the CPU if the budget is reached. Reaching the budget never stops the run.
    fn keep_within_budget(&mut self) -> Result<(), AsrError> {
        let (Some(watch), Some(engine)) = (self.guard.as_mut(), self.engine.as_mut()) else {
            return Ok(());
        };
        match watch.look(Instant::now()) {
            Look::Quiet => {}
            Look::Blind => log::print(&log::memory_unreadable_line(self.strings)),
            Look::Reached { .. } => {
                if let Some(demotion) = engine.demote_for_memory()? {
                    let notice = Notice::from_fallback(&demotion.fallback);
                    say_notice(self.strings, self.hub, &notice);
                    if let Some(report) = &demotion.report {
                        log_session(self.strings, report);
                    }
                }
            }
        }
        Ok(())
    }

    /// Says what the recognizers and the route did since the last call.
    fn tell_resident(&mut self) {
        if let Some(engine) = self.engine.as_mut() {
            report::tell_resident(self.strings, self.hub, engine);
        }
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
            log::print(&log::endpoint_decided_line(self.strings, done));
            if done.punct_error.is_some() {
                log::print(&log::punct_failed_line(self.strings, done));
            }
            self.hub
                .publish(&state.stats.final_event(done, self.source));
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
        self.tell_resident();
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
    /// The engine forgets the audio, the language and the speakers of the old
    /// one after the summary, so that nothing above still needs them.
    fn reset(&mut self, state: &mut State) -> Result<(), AsrError> {
        self.flush(state)?;
        self.hub.publish(&state.stats.summary_event());
        state.stats = Stats::default();
        if let Some(engine) = self.engine.as_mut() {
            engine.reset_session();
        }
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
    use std::sync::{Arc, mpsc};

    use hayamimi_asr::Timings;
    use hayamimi_asr::itn::Overrides;
    use hayamimi_asr::replace::Replacements;
    use hayamimi_asr::vad::VadSettings;
    use hayamimi_relay::lang::Lang;
    use tokio::sync::{broadcast, oneshot};

    use crate::control::{Initial, Live};
    use crate::feed;

    fn done(latency: f64) -> Final {
        Final {
            id: 1,
            text: String::from("テスト"),
            lang: String::from("ja"),
            tier: "rz",
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
            endpoint: hayamimi_asr::endpoint::Decision {
                rule: hayamimi_asr::endpoint::Rule::Silence,
                silence_ms: 300,
                plan: None,
                held_gap_ms: None,
            },
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

    /// The `final` event of an utterance read in `lang`, made as the loop makes it.
    fn final_event_for(lang: &str, stats: &mut Stats) -> Value {
        let mut utterance = done(100.0);
        utterance.lang = String::from(lang);
        utterance.text = format!("hello in {lang}");
        stats.final_event(&utterance, "file")
    }

    #[test]
    fn final_event_switched_is_false_for_the_first_final() {
        let mut stats = Stats::default();
        assert_eq!(final_event_for("ja", &mut stats)["switched"], false);
    }

    #[test]
    fn final_event_switched_true_when_language_changes() {
        let mut stats = Stats::default();
        final_event_for("ja", &mut stats);
        assert_eq!(final_event_for("en", &mut stats)["switched"], true);
    }

    #[test]
    fn final_event_switched_false_when_language_repeats() {
        let mut stats = Stats::default();
        final_event_for("ja", &mut stats);
        final_event_for("en", &mut stats);
        assert_eq!(final_event_for("en", &mut stats)["switched"], false);
    }

    #[test]
    fn session_stats_reset_clears_last_final_lang() {
        let mut stats = Stats::default();
        final_event_for("en", &mut stats);
        assert_eq!(stats.last_final_lang.as_deref(), Some("en"));
        stats = Stats::default();
        assert_eq!(stats.last_final_lang, None);
        // The first `final` of a reset session is not a switch from the old session's language.
        assert_eq!(final_event_for("ja", &mut stats)["switched"], false);
    }

    // The loop with no engine: it reads the queue, counts audio and applies
    // the commands, which is all the stop flag, the end of the input and a
    // session reset have to be judged on.

    /// What a loop is run on: the hub, the texts, and the controls the routes hold.
    struct Rig {
        hub: Hub,
        strings: Strings,
        controls: Controls,
    }

    impl Rig {
        /// A loop with no engine and no memory guard, reading `audio`.
        fn session<'a>(
            &'a self,
            audio: AudioRx,
            commands: Receiver<Command>,
            stop: &'a AtomicBool,
        ) -> Session<'a> {
            Session {
                engine: None,
                guard: None,
                hub: &self.hub,
                strings: &self.strings,
                controls: &self.controls,
                audio,
                commands,
                stop,
                partial: false,
                live: false,
                source: "file",
            }
        }
    }

    /// A rig and the receiving end of its controls.
    fn rig() -> Result<(Rig, Receiver<Command>), String> {
        let (commands, received) = mpsc::channel();
        let live = Live::new(Initial {
            replacements: Replacements::default(),
            overrides: Overrides::default(),
            punctuate: false,
            can_punctuate: false,
            vad: Sensitivity::starting(VadSettings::default()),
        });
        let rig = Rig {
            hub: Hub::new(),
            strings: Strings::embedded(Lang::En)?,
            controls: Controls::new(live, commands),
        };
        Ok((rig, received))
    }

    /// The events published since the last call, parsed.
    fn drain(events: &mut broadcast::Receiver<String>) -> Result<Vec<Value>, String> {
        let mut parsed = Vec::new();
        while let Ok(text) = events.try_recv() {
            parsed.push(serde_json::from_str(&text).map_err(|error| error.to_string())?);
        }
        Ok(parsed)
    }

    fn types_of(events: &[Value]) -> Vec<&str> {
        events
            .iter()
            .filter_map(|event| event["type"].as_str())
            .collect()
    }

    fn new_state() -> State {
        State {
            stats: Stats::default(),
            pending_vad: None,
        }
    }

    #[test]
    fn reset_live_session_emits_summary_then_reset_in_order() -> Result<(), String> {
        let (rig, commands) = rig()?;
        let mut events = rig.hub.subscribe_live();
        let (_tx, audio) = feed::channel();
        let stop = AtomicBool::new(false);
        let mut session = rig.session(audio, commands, &stop);
        let mut state = new_state();
        for _ in 0..5 {
            state.stats.add_final(&done(100.0));
        }
        state.stats.add_window(12 * SAMPLE_RATE as usize);
        state.stats.switched_to("en");

        session
            .reset(&mut state)
            .map_err(|error| error.to_string())?;

        let published = drain(&mut events)?;
        assert_eq!(types_of(&published), ["session_summary", "session_reset"]);
        assert_eq!(published[0]["stats"]["segments"], 5);
        assert_eq!(published[0]["stats"]["total_audio_s"], 12.0);
        // The stats are zeroed after they were summarised, not before.
        assert_eq!(state.stats.segments, 0);
        assert_eq!(state.stats.last_final_lang, None);
        Ok(())
    }

    #[test]
    fn reset_live_session_tolerates_every_optional_piece_being_none() -> Result<(), String> {
        let (rig, commands) = rig()?;
        let mut events = rig.hub.subscribe_live();
        let (_tx, audio) = feed::channel();
        let stop = AtomicBool::new(false);
        // No engine and no memory guard.
        let mut session = rig.session(audio, commands, &stop);
        session
            .reset(&mut new_state())
            .map_err(|error| error.to_string())?;
        let published = drain(&mut events)?;
        assert_eq!(types_of(&published), ["session_summary", "session_reset"]);
        Ok(())
    }

    #[test]
    fn run_stream_applies_a_submitted_task_on_the_decode_thread_not_the_submitter()
    -> Result<(), String> {
        let (rig, commands) = rig()?;
        let mut events = rig.hub.subscribe_live();
        let (tx, audio) = feed::channel();
        let stop = AtomicBool::new(false);

        // Submitted from this thread while no loop is running.
        let mut wait = rig.controls.reset().ok_or("the loop has ended")?;
        // The submit itself applies nothing.
        assert!(drain(&mut events)?.is_empty());
        assert!(matches!(
            wait.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));

        let (rig_ref, stop_ref) = (&rig, &stop);
        let ran = std::thread::scope(|scope| {
            let looped = scope.spawn(move || rig_ref.session(audio, commands, stop_ref).run());
            tx.send(AudioItem::End);
            looped.join()
        });
        ran.map_err(|_| String::from("the loop panicked"))?
            .map_err(|error| error.to_string())?;

        // The loop applied it before it ended: the reset, then the closing summary.
        let published = drain(&mut events)?;
        assert_eq!(
            types_of(&published),
            ["session_summary", "session_reset", "session_summary"]
        );
        assert!(matches!(wait.try_recv(), Ok(())));
        Ok(())
    }

    /// Runs the loop on `audio` until it ends and returns the audio seconds of its summary.
    fn seconds_of_a_run(audio: AudioRx, stop: &AtomicBool) -> Result<f64, String> {
        let (rig, commands) = rig()?;
        let mut events = rig.hub.subscribe_live();
        rig.session(audio, commands, stop)
            .run()
            .map_err(|error| error.to_string())?;
        for event in drain(&mut events)? {
            if event["type"] == "session_summary" {
                return event["stats"]["total_audio_s"]
                    .as_f64()
                    .ok_or_else(|| String::from("the summary has no total_audio_s"));
            }
        }
        Err(String::from("the run published no session_summary"))
    }

    #[test]
    fn run_stream_stops_mid_wav_when_stop_event_is_set() -> Result<(), String> {
        // Five seconds of paced audio; the stop flag is set 150 ms in from another thread.
        let (tx, rx) = feed::channel();
        let player = feed::play_wav(vec![0.0; 5 * SAMPLE_RATE as usize], true, tx);
        let stop = Arc::new(AtomicBool::new(false));
        let stopper = {
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(150));
                stop.store(true, Ordering::Relaxed);
            })
        };
        let started = Instant::now();
        let seconds = seconds_of_a_run(rx, &stop)?;
        let took = started.elapsed();
        stopper
            .join()
            .map_err(|_| String::from("the stopper panicked"))?;
        // The player notices that the loop is gone on its next send and ends.
        player
            .join()
            .map_err(|_| String::from("the player panicked"))?;
        assert!(took < Duration::from_secs(2), "the run took {took:?}");
        assert!(seconds > 0.0 && seconds < 5.0, "{seconds}");
        Ok(())
    }

    #[test]
    fn run_stream_without_stop_event_runs_to_completion() -> Result<(), String> {
        let (tx, rx) = feed::channel();
        let player = feed::play_wav(vec![0.0; SAMPLE_RATE as usize / 5], false, tx);
        let stop = AtomicBool::new(false);
        let seconds = seconds_of_a_run(rx, &stop)?;
        player
            .join()
            .map_err(|_| String::from("the player panicked"))?;
        // The last window is padded to a whole one, so the total may pass the
        // raw length by less than one window.
        let window_seconds = WINDOW as f64 / f64::from(SAMPLE_RATE);
        assert!(seconds >= 0.2, "{seconds}");
        assert!(seconds < 0.2 + window_seconds, "{seconds}");
        Ok(())
    }
}

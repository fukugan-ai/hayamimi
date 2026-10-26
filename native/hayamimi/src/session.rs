//! The recognition loop: reads windows of audio from the queue, feeds the
//! engine, publishes what it makes, and applies the commands of the control
//! routes between two windows. It is the only place that touches the engine.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use hayamimi_asr::audio::{SAMPLE_RATE, WINDOW};
use hayamimi_asr::itn::Overrides;
use hayamimi_asr::partial::MidwayAsked;
use hayamimi_asr::pipeline::{ClosedSegment, Demotion};
use hayamimi_asr::replace::Replacements;
use hayamimi_asr::speaker_stage::Mark;
use hayamimi_asr::vad::Sensitivity;
use hayamimi_asr::{AsrError, Engine, Final};
use hayamimi_gpu::memory::MemoryMeter;
use serde_json::Value;

use crate::control::{Command, Controls};
use crate::feed::{AudioItem, AudioRx};
use crate::log;
use crate::memory_guard::{Look, MemoryGuard};
use crate::notice::Notice;
use crate::refine::{Closed, RefineCounts, Refiner, Spoken};
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
    /// What the refine pass counted for the session.
    refine: RefineCounts,
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

    /// Adds what the refine pass counted since it was last asked.
    fn add_refine(&mut self, counts: RefineCounts) {
        self.refine.groups_closed += counts.groups_closed;
        self.refine.lang_corrections += counts.lang_corrections;
        self.refine.lang_boundary_flushes += counts.lang_boundary_flushes;
        self.refine.groups_skipped += counts.groups_skipped;
    }

    /// The `session_summary` event of what is counted.
    pub fn summary_event(&self) -> Value {
        let mean = (self.segments > 0).then(|| self.latency_sum_ms / self.segments as f64);
        wire::session_summary_with(
            self.samples as f64 / f64::from(SAMPLE_RATE),
            self.segments,
            mean,
            self.latency_max_ms,
            &self.refine,
        )
    }
}

/// What the loop needs of the recognition side. [`Engine`] is the one of a
/// run; a test hands in a script, so that the order of what a reset or a
/// retune does is judged without a model.
pub trait Hearing {
    fn feed(&mut self, window: &[f32]) -> Result<Vec<Final>, AsrError>;
    fn finish(&mut self) -> Result<Vec<Final>, AsrError>;
    fn partial(&mut self) -> Result<Option<String>, AsrError>;
    /// Whether the detector is inside speech that has not been cut yet.
    fn speech_in_progress(&self) -> bool;
    fn sensitivity(&self) -> Option<Sensitivity>;
    /// Moves the detector to `wanted` when that loses no audio; whether it moved.
    fn retune_vad(&mut self, wanted: Sensitivity) -> Result<bool, AsrError>;
    fn set_replacements(&mut self, replacements: Replacements);
    fn set_itn_overrides(&mut self, overrides: Overrides);
    fn set_punctuate(&mut self, on: bool);
    fn has_speaker_stage(&self) -> bool;
    fn mark_speaker(&mut self, done: &Final) -> Option<Mark>;
    fn demote_for_memory(&mut self) -> Result<Option<Demotion>, AsrError>;
    /// Forgets the audio, the language and the speakers of the session.
    fn reset_session(&mut self);
    /// Says what the recognizers and the route did since the last call.
    fn tell_resident(&mut self, strings: &Strings, hub: &Hub);
    /// The stretches of voice the detector closed since the last call, each
    /// with the rule that ended it. A script that closes none says none.
    fn take_closed(&mut self) -> Vec<ClosedSegment> {
        Vec::new()
    }
    /// The questions put to the language identifier in the middle of an
    /// utterance since the last call. A script that asks none says none.
    fn take_midway(&mut self) -> Vec<MidwayAsked> {
        Vec::new()
    }
}

impl Hearing for Engine {
    fn feed(&mut self, window: &[f32]) -> Result<Vec<Final>, AsrError> {
        Engine::feed(self, window)
    }

    fn finish(&mut self) -> Result<Vec<Final>, AsrError> {
        Engine::finish(self)
    }

    fn partial(&mut self) -> Result<Option<String>, AsrError> {
        Engine::partial(self)
    }

    fn speech_in_progress(&self) -> bool {
        Engine::speech_in_progress(self)
    }

    fn sensitivity(&self) -> Option<Sensitivity> {
        Engine::sensitivity(self)
    }

    fn retune_vad(&mut self, wanted: Sensitivity) -> Result<bool, AsrError> {
        Engine::retune_vad(self, wanted)
    }

    fn set_replacements(&mut self, replacements: Replacements) {
        Engine::set_replacements(self, replacements);
    }

    fn set_itn_overrides(&mut self, overrides: Overrides) {
        Engine::set_itn_overrides(self, overrides);
    }

    fn set_punctuate(&mut self, on: bool) {
        Engine::set_punctuate(self, on);
    }

    fn has_speaker_stage(&self) -> bool {
        Engine::has_speaker_stage(self)
    }

    fn mark_speaker(&mut self, done: &Final) -> Option<Mark> {
        Engine::mark_speaker(self, done)
    }

    fn demote_for_memory(&mut self) -> Result<Option<Demotion>, AsrError> {
        Engine::demote_for_memory(self)
    }

    fn reset_session(&mut self) {
        Engine::reset_session(self);
    }

    fn tell_resident(&mut self, strings: &Strings, hub: &Hub) {
        report::tell_resident(strings, hub, self);
    }

    fn take_closed(&mut self) -> Vec<ClosedSegment> {
        Engine::take_closed(self)
    }

    fn take_midway(&mut self) -> Vec<MidwayAsked> {
        Engine::take_midway(self)
    }
}

/// What the loop is run with.
pub struct Session<'a, E = Engine> {
    pub engine: Option<E>,
    /// Reads the closed groups of utterances again; none when the refine pass is off.
    pub refiner: Option<Refiner>,
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

impl<E: Hearing> Session<'_, E> {
    /// Runs until the input ends, the stop flag is set or the engine fails.
    /// What the detector holds is finished, what the refine pass holds is
    /// read, and a summary is published.
    pub fn run(mut self) -> Result<(), AsrError> {
        let mut state = State {
            stats: Stats::default(),
            pending_vad: None,
        };
        self.tell_resident();
        while !self.stop.load(Ordering::Relaxed) {
            self.apply_commands(&mut state)?;
            self.retune(&mut state)?;
            self.tell_refine_if_behind();
            match self.audio.recv_timeout(TICK) {
                Ok(AudioItem::Window(window)) => self.on_window(&mut state, &window)?,
                Ok(AudioItem::Flush) => self.flush(&mut state)?,
                Ok(AudioItem::End) | Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
        self.flush(&mut state)?;
        self.settle_refine(&mut state);
        self.hub.publish(&state.stats.summary_event());
        if let Some(mut refiner) = self.refiner.take() {
            refiner.close();
        }
        Ok(())
    }

    fn on_window(&mut self, state: &mut State, window: &[f32]) -> Result<(), AsrError> {
        state.stats.add_window(WINDOW);
        self.keep_within_budget()?;
        // Read before the feed: the wait is the queue's, not the feed's own time.
        let (waited, backlog) = (self.audio.last_wait(), self.audio.backlog());
        if let Some(refiner) = self.refiner.as_mut() {
            refiner.hear(window);
        }
        let finals = match self.engine.as_mut() {
            Some(engine) => engine.feed(window)?,
            None => return Ok(()),
        };
        self.announce(state, &finals);
        self.say_feed_waits(&finals, waited, backlog);
        self.say_vad_closed(state.stats.samples, backlog);
        self.say_midway();
        self.tell_resident();
        self.close_due_group();
        self.draft();
        Ok(())
    }

    /// Runs `step` on the refine pass. One that has closed is dropped, and
    /// the run goes on without it.
    fn with_refiner(&mut self, step: impl FnOnce(&mut Refiner) -> Result<(), Closed>) {
        if let Some(refiner) = self.refiner.as_mut()
            && step(refiner).is_err()
        {
            self.refiner = None;
        }
    }

    /// Closes the open group when it is due, but not while speech is in
    /// progress: the silence since the last utterance is then not the group's.
    fn close_due_group(&mut self) {
        if !self.engine.as_ref().is_some_and(E::speech_in_progress) {
            self.with_refiner(Refiner::tick);
        }
    }

    /// Tells the refine pass whether the loop is behind the audio, so that it
    /// starts no new group until the loop has caught up.
    fn tell_refine_if_behind(&self) {
        if let Some(refiner) = self.refiner.as_ref() {
            refiner.set_behind(self.behind());
        }
    }

    /// Reads every group that is waiting, whatever the loop is doing, and
    /// adds what the refine pass counted to the session's.
    fn settle_refine(&mut self, state: &mut State) {
        if let Some(refiner) = self.refiner.as_mut() {
            refiner.drain();
            state.stats.add_refine(refiner.take_counts());
        }
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
            engine.tell_resident(self.strings, self.hub);
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

    /// Says which stretches of voice the detector closed in the feed that just
    /// ran, and how far behind the audio it was: the `fed` samples are what it
    /// has been given, and `queued` windows had come in behind them. After the
    /// finals are published, so a log line never delays a subtitle.
    fn say_vad_closed(&mut self, fed: u64, queued: usize) {
        let Some(engine) = self.engine.as_mut() else {
            return;
        };
        let closed = engine.take_closed();
        let received = fed + (queued * WINDOW) as u64;
        for line in log::vad_closed_lines(self.strings, self.source, &closed, (received, fed)) {
            log::print(&line);
        }
    }

    /// Says what the identifier answered when the feed that just ran asked it
    /// the language of the utterance in progress. After the finals are
    /// published, so a log line never delays a subtitle.
    fn say_midway(&mut self) {
        let Some(engine) = self.engine.as_mut() else {
            return;
        };
        for asked in engine.take_midway() {
            log::print(&log::midway_asked_line(self.strings, &asked));
        }
    }

    /// Logs and publishes what the utterances made. Every `final` goes out
    /// before the first speaker mark is computed, so a mark never delays a subtitle.
    fn announce(&mut self, state: &mut State, finals: &[Final]) {
        for done in finals {
            state.stats.add_final(done);
            log::print(&log::utterance_timed_line(self.strings, done));
            log::print(&log::endpoint_decided_line(self.strings, done));
            if let Some(line) = log::lang_fallback_line(self.strings, done) {
                log::print(&line);
            }
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
            let speaker = self.announce_speaker(done);
            // DEFAULT: an utterance with no speaker mark has the empty speaker, as its `final` event states it.
            let label = speaker.as_deref().unwrap_or_default();
            self.with_refiner(|refiner| refiner.add_span(Spoken::of(done, label)));
        }
    }

    /// Sends the speaker of one utterance as its own event, once; says so in
    /// the log when the mark failed. Returns the mark, or `None` when there is none.
    fn announce_speaker(&mut self, done: &Final) -> Option<String> {
        let engine = self
            .engine
            .as_mut()
            .filter(|engine| engine.has_speaker_stage())?;
        log::print(&log::embedding_started_line(
            self.strings,
            done.id,
            log::mono_us(),
        ));
        let mark = engine.mark_speaker(done)?;
        if mark.error.is_some() {
            log::print(&log::speaker_failed_line(self.strings, done));
            return None;
        }
        self.hub
            .publish(&wire::speaker_event(done, self.source, &mark.label));
        Some(mark.label)
    }

    /// Ends the speech in progress and publishes what it made. The open
    /// group of the refine pass is closed too, whatever its age.
    fn flush(&mut self, state: &mut State) -> Result<(), AsrError> {
        if let Some(engine) = self.engine.as_mut() {
            let finals = engine.finish()?;
            self.announce(state, &finals);
            // Speech cut short by a flush has no audio waiting behind it.
            self.say_vad_closed(state.stats.samples, 0);
            self.tell_resident();
        }
        self.with_refiner(Refiner::flush);
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
                    None => self.engine.as_ref().and_then(E::sensitivity),
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
    /// What the refine pass holds is read before the summary, so that the
    /// outgoing session's tail is not lost; the refine pass and the engine
    /// forget the audio, the language and the speakers of the old session
    /// after it, so that nothing above still needs them.
    fn reset(&mut self, state: &mut State) -> Result<(), AsrError> {
        self.flush(state)?;
        self.settle_refine(state);
        self.hub.publish(&state.stats.summary_event());
        state.stats = Stats::default();
        self.with_refiner(Refiner::reset);
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
            fallback: None,
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
                refiner: None,
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

        /// A loop over `engine` and `refiner`, with no memory guard.
        fn session_over<'a, E: Hearing>(
            &'a self,
            engine: E,
            refiner: Option<Refiner>,
            audio: AudioRx,
            commands: Receiver<Command>,
            stop: &'a AtomicBool,
        ) -> Session<'a, E> {
            Session {
                engine: Some(engine),
                refiner,
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

        /// A refine pass over `reader` that publishes to this rig's hub.
        fn refiner<R, M>(&self, make: M) -> Result<Refiner, String>
        where
            R: Reread,
            M: FnOnce() -> R + Send + 'static,
        {
            Refiner::start(make, self.hub.clone(), self.strings.clone(), "file")
                .map_err(|error| error.to_string())
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

    // The loop over a script and the refine pass: the order of what a reset
    // and a retune do, and what the refine pass is told, judged with no model.

    use std::collections::VecDeque;
    use std::io::Write;
    use std::sync::Mutex;

    use hayamimi_asr::route::{Refined, RouteError};

    use crate::refine::{Reread, locked};

    type Check = Result<(), Box<dyn std::error::Error>>;

    /// How the script answers a request to move the detector.
    #[derive(Clone, Copy)]
    enum Retune {
        Moves,
        Waits,
        Fails,
    }

    type Calls = Arc<Mutex<Vec<&'static str>>>;

    /// A recognition side that answers from a script and writes down the calls that matter.
    struct Script {
        calls: Calls,
        /// What each `feed` returns, one entry per call.
        feeds: VecDeque<Vec<Final>>,
        retune: Retune,
        in_speech: bool,
    }

    impl Script {
        fn new(retune: Retune) -> (Script, Calls) {
            let calls = Calls::default();
            let script = Script {
                calls: Arc::clone(&calls),
                feeds: VecDeque::new(),
                retune,
                in_speech: false,
            };
            (script, calls)
        }

        fn note(&self, call: &'static str) {
            locked(&self.calls).push(call);
        }
    }

    impl Hearing for Script {
        fn feed(&mut self, _window: &[f32]) -> Result<Vec<Final>, AsrError> {
            Ok(self.feeds.pop_front().into_iter().flatten().collect())
        }

        fn finish(&mut self) -> Result<Vec<Final>, AsrError> {
            self.note("finish");
            Ok(Vec::new())
        }

        fn partial(&mut self) -> Result<Option<String>, AsrError> {
            Ok(None)
        }

        fn speech_in_progress(&self) -> bool {
            self.in_speech
        }

        fn sensitivity(&self) -> Option<Sensitivity> {
            Some(Sensitivity::starting(VadSettings::default()))
        }

        fn retune_vad(&mut self, _wanted: Sensitivity) -> Result<bool, AsrError> {
            self.note("retune");
            match self.retune {
                Retune::Moves => Ok(true),
                Retune::Waits => Ok(false),
                Retune::Fails => Err(AsrError::Create("vad")),
            }
        }

        fn set_replacements(&mut self, _replacements: Replacements) {}

        fn set_itn_overrides(&mut self, _overrides: Overrides) {}

        fn set_punctuate(&mut self, _on: bool) {}

        fn has_speaker_stage(&self) -> bool {
            false
        }

        fn mark_speaker(&mut self, _done: &Final) -> Option<Mark> {
            None
        }

        fn demote_for_memory(&mut self) -> Result<Option<Demotion>, AsrError> {
            Ok(None)
        }

        fn reset_session(&mut self) {
            self.note("reset_session");
        }

        fn tell_resident(&mut self, _strings: &Strings, _hub: &Hub) {}
    }

    /// A reader that answers every group with the same text after `delay`,
    /// writes down how many samples each group had, and holds a token while it lives.
    struct Echo {
        heard: Arc<Mutex<Vec<usize>>>,
        delay: Duration,
        token: Arc<()>,
    }

    impl Echo {
        fn after(delay_ms: u64) -> (Echo, Arc<Mutex<Vec<usize>>>) {
            let heard = Arc::new(Mutex::new(Vec::new()));
            let echo = Echo {
                heard: Arc::clone(&heard),
                delay: Duration::from_millis(delay_ms),
                token: Arc::new(()),
            };
            (echo, heard)
        }
    }

    impl Reread for Echo {
        fn reread(&mut self, audio: &[f32], lang: &str) -> Result<Refined, RouteError> {
            let _held = &self.token;
            std::thread::sleep(self.delay);
            locked(&self.heard).push(audio.len());
            Ok(Refined {
                text: String::from("テスト"),
                lang: lang.to_owned(),
            })
        }
    }

    /// A transcript that keeps what it is given.
    struct Kept(Arc<Mutex<Vec<u8>>>);

    impl Write for Kept {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            locked(&self.0).extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    const SECOND: usize = SAMPLE_RATE as usize;

    fn one_second_of(lang: &str, text: &str) -> Spoken {
        Spoken {
            start: 0,
            end: SECOND,
            lang: lang.to_owned(),
            text: text.to_owned(),
            speaker: String::new(),
            end_wall_ms: 0,
        }
    }

    /// Gives the refine pass `seconds` of silence with one utterance in it.
    fn give_an_utterance(
        refiner: &mut Refiner,
        seconds: usize,
        spoken: Spoken,
    ) -> Result<(), Closed> {
        refiner.hear(&vec![0.0; seconds * SECOND]);
        refiner.add_span(spoken)
    }

    fn wanted_sensitivity() -> Sensitivity {
        Sensitivity {
            threshold: 0.9,
            ..Sensitivity::starting(VadSettings::default())
        }
    }

    #[test]
    fn the_summary_adds_what_the_refine_pass_counted_each_time_it_was_asked() {
        let mut stats = Stats::default();
        stats.add_refine(RefineCounts {
            groups_closed: 2,
            lang_corrections: 1,
            lang_boundary_flushes: 1,
            groups_skipped: 0,
        });
        stats.add_refine(RefineCounts {
            groups_closed: 1,
            lang_corrections: 0,
            lang_boundary_flushes: 0,
            groups_skipped: 3,
        });
        let counted = &stats.summary_event()["stats"];
        assert_eq!(counted["refine_groups_closed"], 3);
        assert_eq!(counted["refine_lang_corrections"], 1);
        assert_eq!(counted["refine_lang_boundary_flushes"], 1);
        assert_eq!(counted["refine_groups_skipped"], 3);
    }

    #[test]
    fn reset_live_session_flushes_pending_refine_before_summarizing() -> Check {
        let (rig, commands) = rig()?;
        let mut events = rig.hub.subscribe_live();
        let (_tx, audio) = feed::channel();
        let stop = AtomicBool::new(false);
        let (reader, _heard) = Echo::after(30);
        let mut refiner = rig.refiner(move || reader)?;
        give_an_utterance(&mut refiner, 3, one_second_of("ja", "テスト"))?;
        let (script, _calls) = Script::new(Retune::Moves);
        let mut session = rig.session_over(script, Some(refiner), audio, commands, &stop);

        session.reset(&mut new_state())?;

        // The group still open is read, and sent, before the summary that counts it.
        let published = drain(&mut events)?;
        assert_eq!(
            types_of(&published),
            ["refine", "session_summary", "session_reset"]
        );
        assert_eq!(published[0]["text"], "テスト");
        assert_eq!(published[1]["stats"]["refine_groups_closed"], 1);
        Ok(())
    }

    #[test]
    fn a_reset_makes_the_engine_and_the_refine_pass_forget_after_the_summary() -> Check {
        let (rig, commands) = rig()?;
        let mut events = rig.hub.subscribe_live();
        let (_tx, audio) = feed::channel();
        let stop = AtomicBool::new(false);
        let (reader, _heard) = Echo::after(0);
        let refiner = rig.refiner(move || reader)?;
        let written = Arc::new(Mutex::new(Vec::new()));
        refiner.set_transcript(Box::new(Kept(Arc::clone(&written))));
        let (script, calls) = Script::new(Retune::Moves);
        let mut session = rig.session_over(script, Some(refiner), audio, commands, &stop);

        session.reset(&mut new_state())?;

        assert_eq!(*locked(&calls), ["finish", "reset_session"]);
        assert_eq!(
            String::from_utf8_lossy(&locked(&written)),
            "\n----- session reset -----\n\n"
        );
        let published = drain(&mut events)?;
        assert_eq!(types_of(&published), ["session_summary", "session_reset"]);
        Ok(())
    }

    #[test]
    fn a_retune_that_moves_the_detector_clears_the_request_and_tells_the_controls() -> Check {
        let (rig, commands) = rig()?;
        let (_tx, audio) = feed::channel();
        let stop = AtomicBool::new(false);
        let (script, calls) = Script::new(Retune::Moves);
        let mut session = rig.session_over(script, None, audio, commands, &stop);
        let mut state = new_state();
        state.pending_vad = Some(wanted_sensitivity());

        session.retune(&mut state)?;

        assert_eq!(state.pending_vad, None);
        assert_eq!(rig.controls.live().config().1, wanted_sensitivity());
        assert_eq!(*locked(&calls), ["retune"]);
        Ok(())
    }

    #[test]
    fn a_retune_that_waits_keeps_the_request_for_the_next_window() -> Check {
        let (rig, commands) = rig()?;
        let (_tx, audio) = feed::channel();
        let stop = AtomicBool::new(false);
        let (script, _calls) = Script::new(Retune::Waits);
        let mut session = rig.session_over(script, None, audio, commands, &stop);
        let mut state = new_state();
        state.pending_vad = Some(wanted_sensitivity());

        session.retune(&mut state)?;

        assert_eq!(state.pending_vad, Some(wanted_sensitivity()));
        assert_ne!(rig.controls.live().config().1, wanted_sensitivity());
        Ok(())
    }

    #[test]
    fn a_retune_that_cannot_build_the_detector_returns_the_error_and_keeps_the_request() -> Check {
        let (rig, commands) = rig()?;
        let (_tx, audio) = feed::channel();
        let stop = AtomicBool::new(false);
        let (script, _calls) = Script::new(Retune::Fails);
        let mut session = rig.session_over(script, None, audio, commands, &stop);
        let mut state = new_state();
        state.pending_vad = Some(wanted_sensitivity());

        let result = session.retune(&mut state);

        assert!(matches!(result, Err(AsrError::Create("vad"))), "{result:?}");
        assert_eq!(state.pending_vad, Some(wanted_sensitivity()));
        assert_ne!(rig.controls.live().config().1, wanted_sensitivity());
        Ok(())
    }

    #[test]
    fn live_vad_rebuild_keeps_stream_preroll_and_refiner_coordinates() -> Check {
        let (rig, commands) = rig()?;
        let (_tx, audio) = feed::channel();
        let stop = AtomicBool::new(false);
        let (reader, heard) = Echo::after(0);
        let refiner = rig.refiner(move || reader)?;
        let (mut script, calls) = Script::new(Retune::Moves);
        // The utterance of seconds 1.5 to 2.0 is heard in the 66th window, after the rebuild.
        let mut utterance = done(100.0);
        (utterance.t0_s, utterance.t1_s) = (1.5, 2.0);
        script.feeds = (0..65)
            .map(|_| Vec::new())
            .chain(std::iter::once(vec![utterance]))
            .collect();
        let mut session = rig.session_over(script, Some(refiner), audio, commands, &stop);
        let mut state = new_state();
        let window = vec![0.0; WINDOW];

        for _ in 0..40 {
            session.on_window(&mut state, &window)?;
        }
        state.pending_vad = Some(wanted_sensitivity());
        session.retune(&mut state)?;
        for _ in 40..70 {
            session.on_window(&mut state, &window)?;
        }
        session.flush(&mut state)?;
        session.settle_refine(&mut state);

        assert_eq!(*locked(&calls), ["retune", "finish"]);
        // From a second before the utterance (sample 8000) to its end (sample 32000), across the
        // rebuild at sample 20480: the audio and the coordinates of the refine pass did not move.
        assert_eq!(*locked(&heard), [24_000]);
        assert_eq!(state.stats.refine.groups_closed, 1);
        Ok(())
    }

    #[test]
    fn the_refine_pass_starts_no_group_while_the_input_is_behind_and_starts_it_when_caught_up()
    -> Check {
        let (rig, commands) = rig()?;
        let (tx, audio) = feed::channel();
        let stop = AtomicBool::new(false);
        let (reader, heard) = Echo::after(0);
        let refiner = rig.refiner(move || reader)?;
        let (script, _calls) = Script::new(Retune::Moves);
        let mut session = rig.session_over(script, Some(refiner), audio, commands, &stop);
        session.live = true;
        for _ in 0..BEHIND_WINDOWS + 2 {
            assert!(tx.send(AudioItem::Window(vec![0.0; WINDOW])));
        }
        assert!(session.behind());
        session.tell_refine_if_behind();

        session.with_refiner(|refiner| {
            give_an_utterance(refiner, 3, one_second_of("ja", "テスト"))?;
            refiner.flush()
        });
        std::thread::sleep(Duration::from_millis(100));
        assert!(
            locked(&heard).is_empty(),
            "a group was started while the loop was behind"
        );

        // The loop takes windows until it has caught up.
        while session.behind() {
            session.audio.recv_timeout(TICK)?;
        }
        session.tell_refine_if_behind();
        let started = Instant::now();
        while locked(&heard).is_empty() && started.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(*locked(&heard), [SECOND]);
        Ok(())
    }

    #[test]
    fn an_open_group_is_not_closed_by_silence_while_speech_is_in_progress() -> Check {
        let (rig, commands) = rig()?;
        let (_tx, audio) = feed::channel();
        let stop = AtomicBool::new(false);
        let (reader, _heard) = Echo::after(0);
        let mut refiner = rig.refiner(move || reader)?;
        // Two seconds of silence follow the utterance: the group is due.
        give_an_utterance(&mut refiner, 3, one_second_of("ja", "テスト"))?;
        let (mut script, _calls) = Script::new(Retune::Moves);
        script.in_speech = true;
        let mut session = rig.session_over(script, Some(refiner), audio, commands, &stop);
        let mut state = new_state();

        session.close_due_group();
        session.settle_refine(&mut state);
        assert_eq!(state.stats.refine.groups_closed, 0);

        if let Some(engine) = session.engine.as_mut() {
            engine.in_speech = false;
        }
        session.close_due_group();
        session.settle_refine(&mut state);
        assert_eq!(state.stats.refine.groups_closed, 1);
        Ok(())
    }

    #[test]
    fn the_end_of_the_run_reads_what_waits_counts_it_and_closes_the_refine_pass() -> Check {
        let (rig, commands) = rig()?;
        let mut events = rig.hub.subscribe_live();
        let (tx, audio) = feed::channel();
        let stop = AtomicBool::new(false);
        let (reader, _heard) = Echo::after(0);
        let watch = Arc::downgrade(&reader.token);
        let mut refiner = rig.refiner(move || reader)?;
        give_an_utterance(&mut refiner, 3, one_second_of("ja", "テスト"))?;
        let (script, _calls) = Script::new(Retune::Moves);
        let session = rig.session_over(script, Some(refiner), audio, commands, &stop);
        assert!(tx.send(AudioItem::End));

        session.run()?;

        let published = drain(&mut events)?;
        assert_eq!(types_of(&published), ["refine", "session_summary"]);
        assert_eq!(published[1]["stats"]["refine_groups_closed"], 1);
        assert!(watch.upgrade().is_none(), "the reader was not released");
        Ok(())
    }
}

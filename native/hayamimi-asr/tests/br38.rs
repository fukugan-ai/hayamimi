//! BR-38: the drafts of the utterance in progress are read by a worker thread
//! with a reader of its own, and the loop takes the text on a later call.
//!
//! The reader is scripted: a test says what it answers to each job, in the
//! order the jobs come, and reads which thread called it and with what audio.
//! Nothing here opens a model. Every wait in this file has a bound, so a test
//! that fails ends instead of hanging, and the gates a test shuts are opened
//! before the drafter it feeds is dropped (a drop joins the worker).

use std::collections::VecDeque;
use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, ThreadId};
use std::time::{Duration, Instant};

use hayamimi_asr::audio::History;
use hayamimi_asr::draft::{DraftNote, DraftReader, Drafter};
use hayamimi_asr::partial::{DraftClock, MAX_SAMPLES};
use hayamimi_asr::{AsrError, Tier};

type TestResult = Result<(), Box<dyn Error>>;

/// What one call to `step` answered.
type Outcome = Result<Option<String>, AsrError>;

/// The longest any wait in this file takes before the test fails.
const WAIT: Duration = Duration::from_secs(10);

/// A flag a scripted reader waits at until the test opens it.
struct Gate {
    open: AtomicBool,
}

impl Gate {
    fn shut() -> Arc<Gate> {
        Arc::new(Gate {
            open: AtomicBool::new(false),
        })
    }

    fn open(&self) {
        self.open.store(true, Ordering::SeqCst);
    }

    /// Waits for the gate to open, for [`WAIT`] at most.
    fn pass(&self) {
        let until = Instant::now() + WAIT;
        while !self.open.load(Ordering::SeqCst) && Instant::now() < until {
            thread::sleep(Duration::from_millis(1));
        }
    }
}

/// What a scripted read does.
enum Answer {
    Text(&'static str),
    Fail,
    Panic,
}

/// One job as the reader saw it.
struct Seen {
    thread: ThreadId,
    tier: Tier,
    samples: Vec<f32>,
}

/// A reader that answers each job from a script, in the order the jobs come.
/// A job the script does not cover fails.
struct Script {
    answers: VecDeque<Answer>,
    prepare_gate: Option<Arc<Gate>>,
    read_gate: Option<Arc<Gate>>,
    nap: Duration,
    prepare_panics: bool,
    slow_release: bool,
    reads: Sender<Seen>,
    releases: Sender<ThreadId>,
    released: Arc<AtomicBool>,
}

/// The test's side of a script.
struct Probe {
    reads: Receiver<Seen>,
    releases: Receiver<ThreadId>,
    released: Arc<AtomicBool>,
}

impl Probe {
    /// The next job a read began on, for [`WAIT`] at most.
    fn next_read(&self) -> Result<Seen, RecvTimeoutError> {
        self.reads.recv_timeout(WAIT)
    }

    /// How many jobs began that the test has not taken yet.
    fn count_reads(&self) -> usize {
        self.reads.try_iter().count()
    }

    /// The audio lengths of the jobs that began, in order.
    fn lengths(&self) -> Vec<usize> {
        self.reads
            .try_iter()
            .map(|seen| seen.samples.len())
            .collect()
    }
}

impl Script {
    fn saying(answers: Vec<Answer>) -> (Script, Probe) {
        let (reads, began) = mpsc::channel();
        let (releases, freed) = mpsc::channel();
        let released = Arc::new(AtomicBool::new(false));
        let script = Script {
            answers: answers.into(),
            prepare_gate: None,
            read_gate: None,
            nap: Duration::ZERO,
            prepare_panics: false,
            slow_release: false,
            reads,
            releases,
            released: Arc::clone(&released),
        };
        let probe = Probe {
            reads: began,
            releases: freed,
            released,
        };
        (script, probe)
    }

    /// Prepares only once the gate is open.
    fn preparing_after(mut self, gate: &Arc<Gate>) -> Script {
        self.prepare_gate = Some(Arc::clone(gate));
        self
    }

    /// Answers each job only once the gate is open.
    fn reading_after(mut self, gate: &Arc<Gate>) -> Script {
        self.read_gate = Some(Arc::clone(gate));
        self
    }

    /// Sleeps this long in `prepare` and in every `read`.
    fn napping(mut self, nap: Duration) -> Script {
        self.nap = nap;
        self
    }

    /// Panics in `prepare`, which ends the worker's thread.
    fn failing_to_prepare(mut self) -> Script {
        self.prepare_panics = true;
        self
    }

    /// Takes 50 ms in `release` before it marks that it is done.
    fn slow_to_release(mut self) -> Script {
        self.slow_release = true;
        self
    }
}

impl DraftReader for Script {
    fn read(&mut self, tier: Tier, samples: &[f32]) -> Result<String, AsrError> {
        let seen = Seen {
            thread: thread::current().id(),
            tier,
            samples: samples.to_vec(),
        };
        // The test may have stopped listening; the read goes on.
        let _ = self.reads.send(seen);
        if let Some(gate) = &self.read_gate {
            gate.pass();
        }
        thread::sleep(self.nap);
        match self.answers.pop_front() {
            Some(Answer::Text(text)) => Ok(String::from(text)),
            Some(Answer::Panic) => panic!("the scripted reader panics"),
            Some(Answer::Fail) | None => Err(AsrError::NoResult),
        }
    }

    fn prepare(&mut self) {
        if let Some(gate) = &self.prepare_gate {
            gate.pass();
        }
        thread::sleep(self.nap);
        if self.prepare_panics {
            panic!("the scripted reader cannot prepare");
        }
    }

    fn release(&mut self) {
        // The test may have stopped listening.
        let _ = self.releases.send(thread::current().id());
        if self.slow_release {
            thread::sleep(Duration::from_millis(50));
        }
        self.released.store(true, Ordering::SeqCst);
    }
}

/// A clock inside speech that began at sample 0.
fn speaking() -> DraftClock {
    let mut clock = DraftClock::new();
    clock.observe(true, 0);
    clock
}

/// `len` samples from position `from`, each told apart from its neighbours.
fn pattern(from: usize, len: usize) -> Vec<f32> {
    (from..from + len)
        .map(|at| (at % 997) as f32 / 997.0)
        .collect()
}

/// Which of the reads in `notes` were used, in order.
fn used_of(notes: &[DraftNote]) -> Vec<bool> {
    notes
        .iter()
        .filter_map(|note| match note {
            DraftNote::Read(read) => Some(read.used),
            DraftNote::Ready { .. } | DraftNote::Lost => None,
        })
        .collect()
}

/// A drafter with the clock and the history it reads, and the reader that
/// takes the draft when there is no worker.
struct Rig {
    drafter: Drafter,
    clock: DraftClock,
    history: History,
    inline: Script,
    inline_probe: Probe,
}

impl Rig {
    fn new(drafter: Drafter, inline: Script, inline_probe: Probe) -> Rig {
        Rig {
            drafter,
            clock: speaking(),
            history: History::new(),
            inline,
            inline_probe,
        }
    }

    /// A worker that reads `worker`, and an inline reader that must stay unused.
    fn with_worker(worker: Script) -> Result<Rig, std::io::Error> {
        let (inline, inline_probe) = Script::saying(Vec::new());
        let drafter = Drafter::start(move || worker)?;
        Ok(Rig::new(drafter, inline, inline_probe))
    }

    /// No worker: `inline` reads on the calling thread.
    fn without_worker(inline: Script, inline_probe: Probe) -> Rig {
        Rig::new(Drafter::inline(), inline, inline_probe)
    }

    fn wait_ready(&mut self) -> TestResult {
        if self.drafter.wait_ready(WAIT) {
            Ok(())
        } else {
            Err(Box::from("the worker was not ready in time"))
        }
    }

    /// Adds `samples` more samples to the history.
    fn hear(&mut self, samples: usize) {
        let from = self.history.end();
        self.history.push(&pattern(from, samples));
    }

    /// Adds samples until the history ends at `end`.
    fn hear_until(&mut self, end: usize) {
        self.hear(end.saturating_sub(self.history.end()));
    }

    fn step(&mut self, tier: Tier) -> Outcome {
        self.drafter
            .step(&mut self.clock, &self.history, tier, &mut self.inline)
    }

    /// Calls `step` until the call that takes a finished read, and returns
    /// that call's answer with what the drafter said up to then. The calls
    /// before it find the worker busy and hand nothing over.
    fn step_until_read(&mut self, tier: Tier) -> Result<(Outcome, Vec<DraftNote>), Box<dyn Error>> {
        let until = Instant::now() + WAIT;
        let mut said = Vec::new();
        while Instant::now() < until {
            let answer = self.step(tier);
            let notes = self.drafter.take_notes();
            let taken = notes.iter().any(|note| matches!(note, DraftNote::Read(_)));
            said.extend(notes);
            if taken {
                return Ok((answer, said));
            }
            thread::sleep(Duration::from_millis(1));
        }
        Err(Box::from("no read came back in time"))
    }
}

/// T-BR-38-001
#[test]
fn br38_a_draft_is_read_by_a_worker_of_its_own_and_published_by_a_later_call() -> TestResult {
    let (script, probe) = Script::saying(vec![Answer::Text("one")]);
    let mut rig = Rig::with_worker(script)?;
    rig.wait_ready()?;
    rig.hear(8_000);
    assert!(
        rig.step(Tier::Rz)?.is_none(),
        "the call that hands a draft over has no text yet"
    );
    let seen = probe.next_read()?;
    assert_ne!(seen.thread, thread::current().id());
    rig.hear(160);
    let (outcome, _) = rig.step_until_read(Tier::Rz)?;
    assert_eq!(outcome?, Some(String::from("one")));
    assert_eq!(rig.inline_probe.count_reads(), 0);
    Ok(())
}

/// T-BR-38-002
#[test]
fn a_worker_takes_no_job_before_it_is_ready_and_the_due_tick_is_kept() -> TestResult {
    let gate = Gate::shut();
    let (script, probe) = Script::saying(vec![Answer::Text("one")]);
    let mut rig = Rig::with_worker(script.preparing_after(&gate))?;
    rig.hear(8_000);
    // (a) The worker is still preparing: nothing is handed over, and the tick is not used.
    assert!(rig.step(Tier::Rz)?.is_none());
    assert_eq!(probe.count_reads(), 0);
    // (b) Ready: the same position hands the draft over.
    gate.open();
    rig.wait_ready()?;
    assert!(rig.step(Tier::Rz)?.is_none());
    assert_eq!(probe.next_read()?.samples.len(), 8_000);
    Ok(())
}

/// T-BR-38-003
#[test]
fn a_busy_worker_gets_no_second_job_and_the_next_call_hands_the_latest_audio() -> TestResult {
    let gate = Gate::shut();
    let (script, probe) = Script::saying(vec![Answer::Text("a"), Answer::Text("b")]);
    let mut rig = Rig::with_worker(script.reading_after(&gate))?;
    rig.wait_ready()?;
    rig.hear(8_000);
    assert!(rig.step(Tier::Rz)?.is_none());
    // The first job begins and waits at the gate.
    probe.next_read()?;
    rig.hear(8_000);
    assert!(
        rig.step(Tier::Rz)?.is_none(),
        "a busy worker is handed nothing"
    );
    gate.open();
    let (outcome, _) = rig.step_until_read(Tier::Rz)?;
    assert_eq!(outcome?, Some(String::from("a")));
    // The call that took the text handed the latest audio over.
    let second = probe.next_read()?;
    assert_eq!(second.samples.len(), 16_000);
    assert_eq!(second.samples, rig.history.draft(0, MAX_SAMPLES));
    rig.drafter.close();
    assert_eq!(probe.count_reads(), 0, "no third job was handed over");
    Ok(())
}

/// T-BR-38-004
#[test]
fn a_result_whose_speech_start_moved_is_dropped() -> TestResult {
    let gate = Gate::shut();
    let (script, probe) = Script::saying(vec![Answer::Text("one"), Answer::Text("two")]);
    let mut rig = Rig::with_worker(script.reading_after(&gate))?;
    rig.wait_ready()?;
    rig.hear(8_000);
    assert!(rig.step(Tier::Rz)?.is_none());
    probe.next_read()?;
    // The speech start moves from 0 to 4 000 while the first draft is read.
    rig.hear(8_000);
    rig.clock.restart_at(4_000);
    gate.open();
    let (outcome, notes) = rig.step_until_read(Tier::Rz)?;
    assert!(
        outcome?.is_none(),
        "the text read from the earlier start is not used"
    );
    assert_eq!(used_of(&notes), [false]);
    // The same call handed the draft of the new start over.
    let second = probe.next_read()?;
    assert_eq!(second.samples, rig.history.draft(4_000, MAX_SAMPLES));
    let (outcome, notes) = rig.step_until_read(Tier::Rz)?;
    assert_eq!(outcome?, Some(String::from("two")));
    assert_eq!(used_of(&notes), [true]);
    Ok(())
}

/// T-BR-38-005
#[test]
fn a_result_after_the_speech_ended_is_dropped() -> TestResult {
    let gate = Gate::shut();
    let (script, probe) = Script::saying(vec![Answer::Text("one")]);
    let mut rig = Rig::with_worker(script.reading_after(&gate))?;
    rig.wait_ready()?;
    rig.hear(8_000);
    assert!(rig.step(Tier::Rz)?.is_none());
    probe.next_read()?;
    // Another half second is in, and the speech ends: a draft would be due if it went on.
    rig.hear(8_000);
    rig.clock.observe(false, 12_000);
    gate.open();
    let (outcome, notes) = rig.step_until_read(Tier::Rz)?;
    assert!(outcome?.is_none());
    assert_eq!(used_of(&notes), [false]);
    rig.drafter.close();
    assert_eq!(
        probe.count_reads(),
        0,
        "no draft is handed over outside speech"
    );
    Ok(())
}

/// T-BR-38-006
#[test]
fn a_result_of_an_earlier_session_is_dropped_and_the_worker_stays_busy_until_it_ends() -> TestResult
{
    let gate = Gate::shut();
    let (script, probe) = Script::saying(vec![Answer::Text("one"), Answer::Text("two")]);
    let mut rig = Rig::with_worker(script.reading_after(&gate))?;
    rig.wait_ready()?;
    rig.hear(8_000);
    assert!(rig.step(Tier::Rz)?.is_none());
    probe.next_read()?;
    rig.hear(8_000);
    rig.drafter.reset();
    // (a) The worker is still reading the earlier session's job, so nothing is handed over.
    assert!(rig.step(Tier::Rz)?.is_none());
    // (b) The job ends: its text is dropped, and the same call hands a new job over.
    gate.open();
    let (outcome, notes) = rig.step_until_read(Tier::Rz)?;
    assert!(outcome?.is_none());
    assert_eq!(used_of(&notes), [false]);
    assert_eq!(probe.next_read()?.samples.len(), 16_000);
    let (outcome, notes) = rig.step_until_read(Tier::Rz)?;
    assert_eq!(outcome?, Some(String::from("two")));
    assert_eq!(used_of(&notes), [true]);
    rig.drafter.close();
    assert_eq!(probe.count_reads(), 0, "(a) handed nothing over");
    Ok(())
}

/// T-BR-38-007
#[test]
fn a_failed_read_is_returned_by_the_later_call_and_the_next_job_is_read() -> TestResult {
    let (script, probe) = Script::saying(vec![Answer::Fail, Answer::Text("two")]);
    let mut rig = Rig::with_worker(script)?;
    rig.wait_ready()?;
    rig.hear(8_000);
    assert!(rig.step(Tier::Rz)?.is_none());
    probe.next_read()?;
    rig.hear(8_000);
    let (outcome, mut said) = rig.step_until_read(Tier::Rz)?;
    assert!(matches!(outcome, Err(AsrError::NoResult)), "{outcome:?}");
    // The same call handed the next job over.
    probe.next_read()?;
    let (outcome, more) = rig.step_until_read(Tier::Rz)?;
    assert_eq!(outcome?, Some(String::from("two")));
    said.extend(more);
    assert_eq!(
        used_of(&said),
        [false, true],
        "a failed read is not a used one"
    );
    assert!(
        !said.contains(&DraftNote::Lost),
        "a failed read does not lose the worker"
    );
    Ok(())
}

/// T-BR-38-008
#[test]
fn a_reader_that_panics_fails_its_job_and_not_the_worker() -> TestResult {
    let (script, probe) = Script::saying(vec![Answer::Panic, Answer::Text("two")]);
    let mut rig = Rig::with_worker(script)?;
    rig.wait_ready()?;
    rig.hear(8_000);
    assert!(rig.step(Tier::Rz)?.is_none());
    probe.next_read()?;
    rig.hear(8_000);
    let (outcome, mut said) = rig.step_until_read(Tier::Rz)?;
    assert!(matches!(outcome, Err(AsrError::Route(_))), "{outcome:?}");
    probe.next_read()?;
    let (outcome, more) = rig.step_until_read(Tier::Rz)?;
    assert_eq!(outcome?, Some(String::from("two")));
    said.extend(more);
    assert_eq!(used_of(&said), [false, true], "a panic is not a used read");
    assert!(
        !said.contains(&DraftNote::Lost),
        "a panic in a read does not lose the worker"
    );
    Ok(())
}

/// T-BR-38-009
#[test]
fn a_job_carries_the_audio_the_inline_read_would_take_and_the_tier_given_when_it_was_handed()
-> TestResult {
    let (script, probe) = Script::saying(vec![Answer::Text("one")]);
    let mut rig = Rig::with_worker(script)?;
    rig.wait_ready()?;
    rig.hear(320_000);
    assert!(rig.step(Tier::V3)?.is_none());
    let seen = probe.next_read()?;
    assert_eq!(seen.tier, Tier::V3);
    assert_eq!(seen.samples.len(), MAX_SAMPLES);
    assert_eq!(seen.samples, rig.history.draft(0, MAX_SAMPLES));
    // The tier given after the hand-over changes nothing about the job.
    let (outcome, _) = rig.step_until_read(Tier::Rz)?;
    assert_eq!(outcome?, Some(String::from("one")));
    Ok(())
}

/// T-BR-38-010
#[test]
fn without_a_worker_a_due_draft_is_read_in_the_same_call_on_the_callers_thread() -> TestResult {
    let (script, probe) = Script::saying(vec![Answer::Text("one")]);
    let mut rig = Rig::without_worker(script, probe);
    rig.hear(8_000);
    assert_eq!(rig.step(Tier::Rz)?, Some(String::from("one")));
    let seen = rig.inline_probe.next_read()?;
    assert_eq!(seen.thread, thread::current().id());
    assert_eq!(seen.samples, rig.history.draft(0, MAX_SAMPLES));
    let notes = rig.drafter.take_notes();
    let [DraftNote::Read(read)] = notes.as_slice() else {
        return Err(Box::from(format!("{notes:?}")));
    };
    assert!(!read.worker && read.used);
    assert_eq!(read.wait_ms, read.decode_ms);
    assert!(!rig.drafter.has_worker());
    Ok(())
}

/// T-BR-38-011
#[test]
fn a_worker_that_ended_is_replaced_by_the_inline_read_and_said_once() -> TestResult {
    let (worker, _) = Script::saying(Vec::new());
    let (inline, inline_probe) =
        Script::saying(vec![Answer::Text("inline"), Answer::Text("inline too")]);
    let drafter = Drafter::start(move || worker.failing_to_prepare())?;
    let mut rig = Rig::new(drafter, inline, inline_probe);
    assert!(
        !rig.drafter.wait_ready(WAIT),
        "a worker that ended is never ready"
    );
    rig.hear(8_000);
    assert_eq!(rig.step(Tier::Rz)?, Some(String::from("inline")));
    let said = rig.drafter.take_notes();
    assert_eq!(
        said.iter().filter(|note| **note == DraftNote::Lost).count(),
        1
    );
    rig.hear(8_000);
    assert_eq!(rig.step(Tier::Rz)?, Some(String::from("inline too")));
    assert!(!rig.drafter.take_notes().contains(&DraftNote::Lost));
    assert!(!rig.drafter.has_worker());
    Ok(())
}

/// T-BR-38-012
#[test]
fn closing_joins_the_worker_releases_the_reader_once_and_leaves_the_inline_read() -> TestResult {
    let (worker, probe) = Script::saying(vec![Answer::Text("one")]);
    let (inline, inline_probe) = Script::saying(vec![Answer::Text("inline")]);
    let drafter = Drafter::start(move || worker.slow_to_release())?;
    let mut rig = Rig::new(drafter, inline, inline_probe);
    rig.wait_ready()?;
    rig.hear(8_000);
    assert!(rig.step(Tier::Rz)?.is_none());
    let read_on = probe.next_read()?.thread;
    rig.drafter.close();
    assert!(
        probe.released.load(Ordering::SeqCst),
        "close returned before the reader was released"
    );
    rig.drafter.close();
    let released: Vec<ThreadId> = probe.releases.try_iter().collect();
    assert_eq!(released, [read_on]);
    assert_ne!(read_on, thread::current().id());
    // The drafts are read on this thread from now on, and nothing is said about it.
    rig.hear(8_000);
    assert_eq!(rig.step(Tier::Rz)?, Some(String::from("inline")));
    assert!(!rig.drafter.take_notes().contains(&DraftNote::Lost));
    Ok(())
}

/// T-BR-38-013
#[test]
fn the_notes_name_the_ready_time_and_each_read_with_its_times() -> TestResult {
    let (script, _) = Script::saying(vec![Answer::Text("one")]);
    let mut rig = Rig::with_worker(script.napping(Duration::from_millis(20)))?;
    rig.wait_ready()?;
    rig.hear(16_000);
    assert!(rig.step(Tier::Rz)?.is_none());
    let (outcome, notes) = rig.step_until_read(Tier::Rz)?;
    assert_eq!(outcome?, Some(String::from("one")));
    let [DraftNote::Ready { ready_ms }, DraftNote::Read(read)] = notes.as_slice() else {
        return Err(Box::from(format!("{notes:?}")));
    };
    assert!(*ready_ms >= 20.0, "{ready_ms}");
    assert!(read.worker && read.used);
    assert_eq!(read.audio_ms, 1_000.0);
    assert!(read.decode_ms >= 20.0, "{}", read.decode_ms);
    assert!(read.wait_ms >= read.decode_ms, "{read:?}");
    assert!(rig.drafter.take_notes().is_empty());
    Ok(())
}

/// T-BR-38-014
#[test]
fn waiting_for_the_worker_is_bounded_and_ends_when_it_is_ready() -> TestResult {
    // (a) A reader that prepares at once.
    let (script, _) = Script::saying(Vec::new());
    let mut quick = Drafter::start(move || script)?;
    assert!(quick.wait_ready(WAIT));
    // (b) A reader that waits at a gate: the wait ends at its bound, and again when the gate opens.
    let gate = Gate::shut();
    let (script, _) = Script::saying(Vec::new());
    let script = script.preparing_after(&gate);
    let mut slow = Drafter::start(move || script)?;
    let began = Instant::now();
    assert!(!slow.wait_ready(Duration::from_millis(50)));
    let took = began.elapsed();
    assert!(
        took >= Duration::from_millis(50) && took < Duration::from_secs(2),
        "{took:?}"
    );
    gate.open();
    assert!(slow.wait_ready(WAIT));
    Ok(())
}

/// T-BR-38-015
#[test]
fn the_due_rule_of_the_clock_is_the_same_with_and_without_a_worker() -> TestResult {
    // (a) Without a worker.
    let (script, probe) = Script::saying(vec![Answer::Text("a"), Answer::Text("b")]);
    let mut rig = Rig::without_worker(script, probe);
    let mut texts = Vec::new();
    for end in [4_000, 8_000, 12_000, 16_000] {
        rig.hear_until(end);
        texts.push(rig.step(Tier::Rz)?);
    }
    let (a, b) = (Some(String::from("a")), Some(String::from("b")));
    assert_eq!(texts, [None, a.clone(), None, b.clone()]);
    assert_eq!(rig.inline_probe.lengths(), [8_000, 16_000]);
    // (b) With a worker: the same two jobs, and each text comes on the call after its job.
    let (script, probe) = Script::saying(vec![Answer::Text("a"), Answer::Text("b")]);
    let mut rig = Rig::with_worker(script)?;
    rig.wait_ready()?;
    rig.hear_until(4_000);
    assert!(rig.step(Tier::Rz)?.is_none());
    rig.hear_until(8_000);
    assert!(rig.step(Tier::Rz)?.is_none());
    rig.hear_until(12_000);
    let (outcome, _) = rig.step_until_read(Tier::Rz)?;
    assert_eq!(outcome?, a);
    rig.hear_until(16_000);
    assert!(rig.step(Tier::Rz)?.is_none());
    let (outcome, _) = rig.step_until_read(Tier::Rz)?;
    assert_eq!(outcome?, b);
    rig.drafter.close();
    assert_eq!(probe.lengths(), [8_000, 16_000]);
    assert_eq!(rig.inline_probe.count_reads(), 0);
    Ok(())
}

/// T-BR-38-022
#[test]
fn a_worker_found_gone_by_the_step_itself_is_replaced_by_the_inline_read_in_that_call() -> TestResult
{
    let (worker, _) = Script::saying(Vec::new());
    let (inline, inline_probe) = Script::saying(vec![Answer::Text("inline")]);
    let drafter = Drafter::start(move || worker.failing_to_prepare())?;
    let mut rig = Rig::new(drafter, inline, inline_probe);
    rig.hear(8_000);
    // Nothing waits for the worker, so only a call of `step` can find it gone.
    // The calls before that one find it not ready: they hand nothing over and read nothing.
    let until = Instant::now() + WAIT;
    let mut found = None;
    while found.is_none() && Instant::now() < until {
        let outcome = rig.step(Tier::Rz)?;
        let said = rig.drafter.take_notes();
        if said.contains(&DraftNote::Lost) {
            found = Some((outcome, said));
        } else {
            assert!(
                outcome.is_none(),
                "a text came before the worker was found gone"
            );
            thread::sleep(Duration::from_millis(1));
        }
    }
    let Some((outcome, said)) = found else {
        return Err(Box::from("no call of step found the worker gone in time"));
    };
    assert_eq!(outcome, Some(String::from("inline")));
    assert_eq!(
        said.iter().filter(|note| **note == DraftNote::Lost).count(),
        1
    );
    assert!(!rig.drafter.has_worker());
    let seen = rig.inline_probe.next_read()?;
    assert_eq!(seen.thread, thread::current().id());
    assert_eq!(seen.samples.len(), 8_000);
    assert_eq!(rig.inline_probe.count_reads(), 0);
    Ok(())
}

//! The drafts of the utterance in progress, read off the loop.
//!
//! A draft is the audio of the utterance so far, decoded again about every half
//! second of input (see [`crate::partial`]). Read on the thread that feeds the
//! detector, a draft that falls at the end of an utterance holds the feed back
//! for as long as the decode takes. A [`Drafter`] with a worker hands the audio
//! to a thread of its own, with recognizers of its own, and takes the text on a
//! later call. A [`Drafter`] without one reads on the calling thread.
//!
//! The worker reads one draft at a time. While it reads, the loop hands it
//! nothing and does not use the tick of the draft clock, so the call after the
//! read ends hands the latest audio. A text is used only when the speech it was
//! read from is still the speech in progress and the session is the same;
//! otherwise it is dropped, so a draft never covers the text of a final.

use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::audio::{History, SAMPLE_RATE};
use crate::decode::Recognizers;
use crate::error::AsrError;
use crate::partial::{DraftClock, MAX_SAMPLES};
use crate::recognizer::warmup_silence;
use crate::route::Tier;

/// The longest the start waits for the worker to load and warm its recognizers.
pub const READY_LIMIT: Duration = Duration::from_secs(30);

/// What reads a draft: the recognizers of a run, or a script in a test.
pub trait DraftReader {
    /// The text `samples` read as with `tier`, as the recognizer made it.
    fn read(&mut self, tier: Tier, samples: &[f32]) -> Result<String, AsrError>;

    /// Loads and warms what reading needs. Called once, on the worker's
    /// thread, before the first read.
    fn prepare(&mut self) {}

    /// Gives back what the reader holds. Called once, on the worker's thread,
    /// after the last read.
    fn release(&mut self) {}
}

impl DraftReader for Recognizers {
    fn read(&mut self, tier: Tier, samples: &[f32]) -> Result<String, AsrError> {
        self.decode_whole(tier, samples).map(|decoded| decoded.text)
    }
}

/// The recognizers of the worker: recognizers of its own, with the tiers a
/// draft is likely to need loaded and read once before the first draft.
pub struct WorkerReader {
    recognizers: Recognizers,
    preload: Vec<Tier>,
}

impl WorkerReader {
    pub fn new(recognizers: Recognizers, preload: Vec<Tier>) -> Self {
        WorkerReader {
            recognizers,
            preload,
        }
    }
}

impl DraftReader for WorkerReader {
    fn read(&mut self, tier: Tier, samples: &[f32]) -> Result<String, AsrError> {
        self.recognizers.read(tier, samples)
    }

    fn prepare(&mut self) {
        let silence = warmup_silence();
        for tier in &self.preload {
            // A tier that cannot be loaded or read is left to the first draft, which is read by the tier that answers for it.
            let _ = self.recognizers.decode_exact(*tier, &silence);
        }
    }

    fn release(&mut self) {
        self.recognizers.release_all();
    }
}

/// One draft read: how it went, for the log.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DraftRead {
    /// Whether the worker read it.
    pub worker: bool,
    /// Whether its text was used. A text read from speech that has moved on is
    /// not, and neither is the outcome of a read that failed.
    pub used: bool,
    /// The audio the recognizer was given, in milliseconds.
    pub audio_ms: f64,
    /// The recognizer's time, in milliseconds.
    pub decode_ms: f64,
    /// From handing the audio over to using or dropping the text, in milliseconds.
    pub wait_ms: f64,
}

/// What the drafter has to say, until it is taken.
#[derive(Debug, Clone, PartialEq)]
pub enum DraftNote {
    /// The worker loaded and warmed its recognizers; the time that took.
    Ready { ready_ms: f64 },
    /// A draft was read.
    Read(DraftRead),
    /// The worker is not running: it could not be started or its thread ended.
    /// The drafts are read on the calling thread from then on.
    Lost,
}

/// The audio of one draft, for the worker.
struct Job {
    epoch: u64,
    start: usize,
    tier: Tier,
    samples: Vec<f32>,
    handed: Instant,
}

/// What the worker tells the loop.
enum Report {
    Ready {
        ms: f64,
    },
    Done {
        epoch: u64,
        start: usize,
        samples: usize,
        handed: Instant,
        read: Result<String, AsrError>,
        decode_ms: f64,
    },
}

fn millis(since: Instant) -> f64 {
    since.elapsed().as_secs_f64() * 1000.0
}

fn audio_ms(samples: usize) -> f64 {
    samples as f64 * 1000.0 / f64::from(SAMPLE_RATE)
}

/// The worker's thread: builds its reader, prepares it, then reads the jobs
/// one at a time until the loop closes the line. A reader that panics fails its
/// job and not the worker.
fn work<R: DraftReader>(make: impl FnOnce() -> R, jobs: &Receiver<Job>, reports: &Sender<Report>) {
    let started = Instant::now();
    let mut reader = make();
    reader.prepare();
    let ready = Report::Ready {
        ms: millis(started),
    };
    if reports.send(ready).is_ok() {
        serve(&mut reader, jobs, reports);
    }
    reader.release();
}

fn serve<R: DraftReader>(reader: &mut R, jobs: &Receiver<Job>, reports: &Sender<Report>) {
    while let Ok(job) = jobs.recv() {
        let began = Instant::now();
        let read = match catch_unwind(AssertUnwindSafe(|| reader.read(job.tier, &job.samples))) {
            Ok(read) => read,
            Err(_) => Err(AsrError::Route(String::from("the draft reader panicked"))),
        };
        let done = Report::Done {
            epoch: job.epoch,
            start: job.start,
            samples: job.samples.len(),
            handed: job.handed,
            read,
            decode_ms: millis(began),
        };
        if reports.send(done).is_err() {
            return;
        }
    }
}

/// The worker as the loop holds it.
struct Worker {
    jobs: Sender<Job>,
    reports: Receiver<Report>,
    handle: JoinHandle<()>,
    /// Its reader is prepared.
    ready: bool,
    /// A job is out and its report has not come back.
    busy: bool,
}

/// Reads the drafts of an utterance in progress, on a worker or on the calling thread.
pub struct Drafter {
    worker: Option<Worker>,
    /// Moves on with every new session; a job of another one is not used.
    epoch: u64,
    notes: Vec<DraftNote>,
}

impl Drafter {
    /// A drafter that reads on the calling thread.
    pub fn inline() -> Drafter {
        Drafter {
            worker: None,
            epoch: 0,
            notes: Vec::new(),
        }
    }

    /// A drafter whose drafts are read on a thread of its own. `make` builds
    /// the reader on that thread, so the reader need not move between threads.
    pub fn start<R, M>(make: M) -> io::Result<Drafter>
    where
        R: DraftReader,
        M: FnOnce() -> R + Send + 'static,
    {
        let (jobs, taken) = mpsc::channel();
        let (sent, reports) = mpsc::channel();
        let handle = std::thread::Builder::new()
            .name(String::from("draft"))
            .spawn(move || work(make, &taken, &sent))?;
        Ok(Drafter {
            worker: Some(Worker {
                jobs,
                reports,
                handle,
                ready: false,
                busy: false,
            }),
            epoch: 0,
            notes: Vec::new(),
        })
    }

    /// [`Drafter::start`], and a drafter that reads on the calling thread
    /// (and says [`DraftNote::Lost`]) when the thread cannot be started.
    pub fn start_or_inline<R, M>(make: M) -> Drafter
    where
        R: DraftReader,
        M: FnOnce() -> R + Send + 'static,
    {
        match Drafter::start(make) {
            Ok(drafter) => drafter,
            Err(_) => {
                let mut drafter = Drafter::inline();
                drafter.notes.push(DraftNote::Lost);
                drafter
            }
        }
    }

    /// Whether the drafts are read by a worker now.
    pub fn has_worker(&self) -> bool {
        self.worker.is_some()
    }

    /// Starts a new session: a job still out is read to its end and dropped.
    pub fn reset(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
    }

    /// What was said since the last call.
    pub fn take_notes(&mut self) -> Vec<DraftNote> {
        std::mem::take(&mut self.notes)
    }

    /// Waits up to `limit` for the worker to be ready. `true` when it is;
    /// `false` at the limit, when the worker is found to have ended, and when
    /// there is none.
    pub fn wait_ready(&mut self, limit: Duration) -> bool {
        let until = Instant::now() + limit;
        while let Some(worker) = self.worker.as_mut() {
            if worker.ready {
                break;
            }
            let left = until.saturating_duration_since(Instant::now());
            match worker.reports.recv_timeout(left) {
                Ok(Report::Ready { ms }) => {
                    worker.ready = true;
                    self.notes.push(DraftNote::Ready { ready_ms: ms });
                }
                // The worker hands out no job before it is ready, so no read is out yet.
                Ok(Report::Done { .. }) => {}
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => self.lose(),
            }
        }
        self.worker.as_ref().is_some_and(|worker| worker.ready)
    }

    /// One call at the rate of the loop. Takes the text the worker finished,
    /// hands it the next draft when one is due and it is free, and returns the
    /// text to publish: the finished one, if it is still the speech in
    /// progress. Without a worker it reads a due draft with `inline` and
    /// returns it in the same call.
    pub fn step<R: DraftReader>(
        &mut self,
        clock: &mut DraftClock,
        history: &History,
        tier: Tier,
        inline: &mut R,
    ) -> Result<Option<String>, AsrError> {
        if self.worker.is_none() {
            return self.step_inline(clock, history, tier, inline);
        }
        let finished = self.collect(clock.speech_start());
        if self.worker.is_none() {
            // The worker was found gone; the draft that is due is read here.
            return match finished {
                Ok(None) => self.step_inline(clock, history, tier, inline),
                other => other,
            };
        }
        self.hand(clock, history, tier);
        finished
    }

    /// Stops the worker after the read in progress and waits for its thread.
    /// The drafts are read on the calling thread afterwards.
    pub fn close(&mut self) {
        let Some(worker) = self.worker.take() else {
            return;
        };
        let Worker { jobs, handle, .. } = worker;
        drop(jobs);
        // A worker that panicked outside a read has nothing left to give back.
        let _ = handle.join();
    }

    /// Takes what the worker reported. A text read from the speech in
    /// progress, in this session, is the result; any other is dropped.
    fn collect(&mut self, speech_start: Option<usize>) -> Result<Option<String>, AsrError> {
        let mut result = Ok(None);
        while let Some(worker) = self.worker.as_mut() {
            match worker.reports.try_recv() {
                Ok(Report::Ready { ms }) => {
                    worker.ready = true;
                    self.notes.push(DraftNote::Ready { ready_ms: ms });
                }
                Ok(Report::Done {
                    epoch,
                    start,
                    samples,
                    handed,
                    read,
                    decode_ms,
                }) => {
                    worker.busy = false;
                    let current = epoch == self.epoch && speech_start == Some(start);
                    self.notes.push(DraftNote::Read(DraftRead {
                        worker: true,
                        used: current && read.is_ok(),
                        audio_ms: audio_ms(samples),
                        decode_ms,
                        wait_ms: millis(handed),
                    }));
                    if current {
                        result = read.map(Some);
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => self.lose(),
            }
        }
        result
    }

    /// Hands the worker the audio of the draft that is due, when it is ready
    /// and free. The tick of the clock is used only when the draft is handed.
    fn hand(&mut self, clock: &mut DraftClock, history: &History, tier: Tier) {
        let epoch = self.epoch;
        let Some(worker) = self.worker.as_mut() else {
            return;
        };
        if !worker.ready || worker.busy {
            return;
        }
        let Some(start) = clock.due(history.end()) else {
            return;
        };
        let job = Job {
            epoch,
            start,
            tier,
            samples: history.draft(start, MAX_SAMPLES),
            handed: Instant::now(),
        };
        if worker.jobs.send(job).is_ok() {
            worker.busy = true;
        } else {
            self.lose();
        }
    }

    /// Reads the draft that is due on the calling thread.
    fn step_inline<R: DraftReader>(
        &mut self,
        clock: &mut DraftClock,
        history: &History,
        tier: Tier,
        inline: &mut R,
    ) -> Result<Option<String>, AsrError> {
        let Some(start) = clock.due(history.end()) else {
            return Ok(None);
        };
        let samples = history.draft(start, MAX_SAMPLES);
        let began = Instant::now();
        let text = inline.read(tier, &samples)?;
        let decode_ms = millis(began);
        self.notes.push(DraftNote::Read(DraftRead {
            worker: false,
            used: true,
            audio_ms: audio_ms(samples.len()),
            decode_ms,
            wait_ms: decode_ms,
        }));
        Ok(Some(text))
    }

    /// The worker's thread is gone: it is joined, and the drafts are read on
    /// the calling thread from now on. Said once.
    fn lose(&mut self) {
        let Some(worker) = self.worker.take() else {
            return;
        };
        let Worker { jobs, handle, .. } = worker;
        drop(jobs);
        // A thread that panicked has nothing left to give back.
        let _ = handle.join();
        self.notes.push(DraftNote::Lost);
    }
}

impl Drop for Drafter {
    fn drop(&mut self) {
        self.close();
    }
}

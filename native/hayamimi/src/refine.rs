//! The refine pass of a live run: the finished utterances are gathered into
//! groups, and each group is read again, in order, once the speaker pauses.
//!
//! [`Collector`] decides what a group is. An utterance joins the open group;
//! the group is closed when the language changes, when two seconds of silence
//! have passed, when it has run twenty-five seconds, and when the session
//! resets or ends. A closed group carries the audio that is still held, so
//! the second reading has what the first one heard.
//!
//! [`Refiner`] reads the closed groups on one thread of its own, one at a
//! time, in the order they closed. The waiting line holds four groups: a
//! fifth drops the oldest, which keeps its fast text. While the live loop is
//! behind the audio, no new group is started (one that was started goes on);
//! a reset, the end of the run and [`Refiner::close`] read what is waiting
//! whatever the loop is doing. The recognizers are the refine pass's own
//! ([`Rereader`]), so a long reading never makes a live utterance wait for them.

use std::collections::VecDeque;
use std::fmt;
use std::fs::OpenOptions;
use std::io::Write;
use std::num::NonZeroUsize;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Instant;

use hayamimi_asr::audio::{History, PREROLL_SECONDS, SAMPLE_RATE};
use hayamimi_asr::lid::{Identify, script_corrected_lang};
use hayamimi_asr::refine::{GROUP_GAP_SECONDS, GROUP_MAX_SECONDS, Span, majority_label};
use hayamimi_asr::route::{Decode, Policy, Refined, RouteError, Router};
use hayamimi_asr::{AsrError, Final, Recognizers};
use hayamimi_relay::logging::Level;
use serde_json::Value;

use crate::log;
use crate::server::Hub;
use crate::strings::Strings;
use crate::wire::{self, Refinement};

/// Groups the waiting line holds; one more drops the oldest.
pub const QUEUE_GROUPS: usize = 4;
/// Threads of the refine pass's recognizers. The live ones keep theirs.
pub const RECOGNIZER_THREADS: i32 = 2;
/// A reading shorter than this share of the fast text lost content: the fast text stands.
const KEEP_RATIO: f64 = 0.7;

/// `refine.group.skipped`: a group was dropped from a full waiting line and keeps its fast text.
pub const GROUP_SKIPPED: &str = "refine.group.skipped";
/// `refine.group.failed`: a group could not be read again and keeps its fast text.
pub const GROUP_FAILED: &str = "refine.group.failed";
/// `refine.group.refined`: a group was read again and published.
pub const GROUP_REFINED: &str = "refine.group.refined";
/// `refine.transcript.failed`: a line could not be written to the transcript.
pub const TRANSCRIPT_FAILED: &str = "refine.transcript.failed";
const TRANSCRIPT_CODE: &str = "HAYAMIMI_ASR_REFINE_TRANSCRIPT_FAILED";
/// What the transcript gets when a new session begins.
const RESET_MARK: &str = "\n----- session reset -----\n\n";
const SKIPPED_CODE: &str = "HAYAMIMI_ASR_REFINE_QUEUE_FULL";
const FAILED_CODE: &str = "HAYAMIMI_ASR_REFINE_FAILED";

/// A group of fewer samples than this (half a second) is not read again.
const MIN_AUDIO_SAMPLES: usize = SAMPLE_RATE as usize / 2;
/// The silence that closes a group, in samples.
const GAP_SAMPLES: usize = (GROUP_GAP_SECONDS * SAMPLE_RATE as f64) as usize;
/// The length that closes a group, in samples.
const LONGEST_SAMPLES: usize = (GROUP_MAX_SECONDS * SAMPLE_RATE as f64) as usize;
/// The audio taken in front of a group, in samples.
const PREROLL_SAMPLES: usize = (PREROLL_SECONDS * SAMPLE_RATE as f64) as usize;

/// What the refine pass counted, for `session_summary`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RefineCounts {
    /// Groups closed that were long enough to be read again.
    pub groups_closed: u64,
    /// Groups whose second reading came back in another language.
    pub lang_corrections: u64,
    /// Groups closed because the next utterance was in another language.
    pub lang_boundary_flushes: u64,
    /// Groups dropped from a full waiting line.
    pub groups_skipped: u64,
}

/// One finished utterance as the refine pass keeps it.
#[derive(Debug, Clone, PartialEq)]
pub struct Spoken {
    /// Where it runs, in samples from the start of the audio fed to the engine.
    pub start: usize,
    pub end: usize,
    pub lang: String,
    pub text: String,
    /// The speaker mark the fast path gave it; empty when there is none.
    pub speaker: String,
    /// When the detector decided it had ended, in milliseconds since the Unix epoch.
    pub end_wall_ms: u64,
}

impl Spoken {
    /// The utterance `done`, with the speaker mark it was given.
    pub fn of(done: &Final, speaker: &str) -> Spoken {
        Spoken {
            start: sample_at(done.t0_s),
            end: sample_at(done.t1_s),
            lang: done.lang.clone(),
            text: done.text.clone(),
            speaker: speaker.to_owned(),
            end_wall_ms: done.end_wall_ms,
        }
    }

    /// The language the script of the text says it is in.
    fn script_lang(&self) -> &str {
        script_corrected_lang(&self.lang, &self.text)
    }
}

fn sample_at(seconds: f64) -> usize {
    (seconds * f64::from(SAMPLE_RATE)).round() as usize
}

fn seconds_at(sample: usize) -> f64 {
    sample as f64 / f64::from(SAMPLE_RATE)
}

/// A closed group, ready to be read again.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    /// The audio from a second before the first utterance to the end of the
    /// last, as far as it is still held.
    pub audio: Vec<f32>,
    /// The language every utterance of the group is in.
    pub lang: String,
    /// The texts of the fast path, joined: what stands when the second reading loses content.
    pub fast_text: String,
    /// The speaker most of the utterances were given; empty when none had a mark.
    pub speaker: String,
    pub t0_s: f64,
    pub t1_s: f64,
    pub end_wall_ms: u64,
}

/// The audio of the session and the utterances that have not been closed into a group.
pub struct Collector {
    history: History,
    open: Vec<Spoken>,
    counts: RefineCounts,
}

impl Collector {
    pub fn new() -> Collector {
        Collector {
            history: History::new(),
            open: Vec::new(),
            counts: RefineCounts::default(),
        }
    }

    /// Keeps a window of the input, so that a group can be cut from it later.
    pub fn hear(&mut self, window: &[f32]) {
        self.history.push(window);
    }

    /// Adds a finished utterance to the open group. An utterance in another
    /// language closes the group before it, which is returned, and starts a new one.
    pub fn add_span(&mut self, spoken: Spoken) -> Option<Group> {
        let closed = match self.open.last() {
            Some(last) if last.script_lang() != spoken.script_lang() => {
                self.counts.lang_boundary_flushes += 1;
                self.close()
            }
            _ => None,
        };
        self.open.push(spoken);
        closed
    }

    /// Closes the open group when it is due: two seconds have passed since its
    /// last utterance, or it has run twenty-five seconds.
    pub fn tick(&mut self) -> Option<Group> {
        if self.is_due() { self.close() } else { None }
    }

    /// Closes the open group whatever its age, for a reset and for the end.
    pub fn flush(&mut self) -> Option<Group> {
        self.close()
    }

    /// Forgets the open group and the audio, for a new session. The counts are kept.
    pub fn reset(&mut self) {
        self.open.clear();
        self.history.clear();
    }

    /// The utterances waiting in the open group.
    pub fn pending(&self) -> usize {
        self.open.len()
    }

    /// What was counted since the last call.
    pub fn take_counts(&mut self) -> RefineCounts {
        std::mem::take(&mut self.counts)
    }

    fn is_due(&self) -> bool {
        let (Some(first), Some(last)) = (self.open.first(), self.open.last()) else {
            return false;
        };
        let silent = self.history.end().saturating_sub(last.end);
        silent >= GAP_SAMPLES || last.end.saturating_sub(first.start) >= LONGEST_SAMPLES
    }

    /// Takes the open group out. A group with less than half a second of
    /// audio left is dropped and not counted.
    fn close(&mut self) -> Option<Group> {
        let open = std::mem::take(&mut self.open);
        let (first, last) = (open.first()?, open.last()?);
        let audio = self
            .history
            .slice(first.start.saturating_sub(PREROLL_SAMPLES), last.end);
        if audio.len() < MIN_AUDIO_SAMPLES {
            return None;
        }
        self.counts.groups_closed += 1;
        Some(Group {
            audio,
            lang: first.script_lang().to_owned(),
            fast_text: joined_text(&open),
            speaker: majority_speaker(&open),
            t0_s: seconds_at(first.start),
            t1_s: seconds_at(last.end),
            end_wall_ms: last.end_wall_ms,
        })
    }
}

impl Default for Collector {
    fn default() -> Self {
        Collector::new()
    }
}

/// The fast path's texts, joined with a space; blank ones drop out.
fn joined_text(open: &[Spoken]) -> String {
    let texts: Vec<&str> = open
        .iter()
        .map(|spoken| spoken.text.as_str())
        .filter(|text| !text.trim().is_empty())
        .collect();
    texts.join(" ")
}

/// The speaker given to most of the utterances that have one.
fn majority_speaker(open: &[Spoken]) -> String {
    let marked: Vec<Span> = open
        .iter()
        .filter(|spoken| !spoken.speaker.is_empty())
        .map(|spoken| Span {
            start: spoken.start,
            end: spoken.end,
            label: spoken.speaker.clone(),
        })
        .collect();
    match majority_label(&marked) {
        Some(label) => label.to_owned(),
        // DEFAULT: a group none of whose utterances has a speaker mark has no speaker, and the wire states that as an empty string.
        None => String::new(),
    }
}

/// What reads a closed group again. The recognizers of the run are
/// [`Rereader`]; a test reads with a script.
pub trait Reread {
    /// Reads `audio`, a group in `lang`, again.
    fn reread(&mut self, audio: &[f32], lang: &str) -> Result<Refined, RouteError>;

    /// Gives back what the reader holds: its models. Called once, on the
    /// worker's thread, after the last group.
    fn release(&mut self) {}
}

impl<D: Decode, L: Identify> Reread for Router<D, L> {
    fn reread(&mut self, audio: &[f32], lang: &str) -> Result<Refined, RouteError> {
        self.refine(audio, lang)
    }

    fn release(&mut self) {
        self.close();
    }
}

/// The identifier of the refine pass: it does not listen, it names the
/// language the group was given. The re-judgment of the language, with the
/// second SenseVoice reading it costs, is left out; the group's language is
/// the fast path's.
struct Given(Arc<Mutex<String>>);

impl Identify for Given {
    fn identify(&self, _samples: &[f32]) -> Result<String, AsrError> {
        Ok(locked(&self.0).clone())
    }
}

/// The refine pass's own recognizers: a router of its own over recognizers of
/// its own, so that a live utterance never waits for a long reading. Nothing
/// is loaded until the first group; only the tier of the group's language
/// stays in memory, besides the Japanese one.
pub struct Rereader {
    router: Router<Recognizers, Given>,
    given: Arc<Mutex<String>>,
}

impl Rereader {
    /// A reader over the models in `models`. `policy` is the live one; its
    /// forced language is dropped, since each group names its own.
    pub fn new(models: &Path, policy: Policy) -> Rereader {
        let given = Arc::new(Mutex::new(String::new()));
        let recognizers = Recognizers::new(models, RECOGNIZER_THREADS, NonZeroUsize::new(1));
        let policy = Policy {
            forced_lang: None,
            ..policy
        };
        let router = Router::new(recognizers, Some(Given(Arc::clone(&given))), policy);
        Rereader { router, given }
    }
}

impl Reread for Rereader {
    fn reread(&mut self, audio: &[f32], lang: &str) -> Result<Refined, RouteError> {
        lang.clone_into(&mut locked(&self.given));
        self.router.refine(audio, lang)
    }

    fn release(&mut self) {
        self.router.close();
    }
}

/// The lock of `mutex`. A thread that panicked while holding it left the
/// value whole, so the poison is passed over.
pub(crate) fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// The refiner was closed and takes no more work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Closed;

impl fmt::Display for Closed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the refiner is closed; build a new one for a new session"
        )
    }
}

impl std::error::Error for Closed {}

/// What the worker may do next.
#[derive(Debug, PartialEq)]
enum Next {
    Group(Group),
    /// Nothing may start now.
    Wait,
    /// Closed and empty: the worker ends.
    Done,
}

/// The state of the waiting line. A plain value, so that its rules are
/// judged without a thread.
#[derive(Debug, Default)]
struct Line {
    groups: VecDeque<Group>,
    /// The live loop is behind the audio: no new group is started.
    behind: bool,
    /// A caller waits for the line to empty: groups start whatever `behind` says.
    draining: bool,
    /// The worker is reading a group.
    busy: bool,
    closing: bool,
}

impl Line {
    /// Adds a group. A full line drops its oldest group, which is returned.
    fn push(&mut self, group: Group) -> Option<Group> {
        let dropped = if self.groups.len() >= QUEUE_GROUPS {
            self.groups.pop_front()
        } else {
            None
        };
        self.groups.push_back(group);
        dropped
    }

    /// The oldest group, unless the loop is behind. A line that is draining or closing starts them all.
    fn next(&mut self) -> Next {
        let may_start = !self.behind || self.draining || self.closing;
        if may_start && let Some(group) = self.groups.pop_front() {
            self.busy = true;
            return Next::Group(group);
        }
        if self.closing && self.groups.is_empty() {
            Next::Done
        } else {
            Next::Wait
        }
    }

    fn is_idle(&self) -> bool {
        self.groups.is_empty() && !self.busy
    }
}

/// The waiting line between the loop and the worker.
#[derive(Default)]
struct Queue {
    line: Mutex<Line>,
    changed: Condvar,
}

impl Queue {
    fn wait<'a>(&self, line: MutexGuard<'a, Line>) -> MutexGuard<'a, Line> {
        match self.changed.wait(line) {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    /// Adds a group; the oldest one it pushes out is returned.
    fn push(&self, group: Group) -> Result<Option<Group>, Closed> {
        let mut line = locked(&self.line);
        if line.closing {
            return Err(Closed);
        }
        let dropped = line.push(group);
        self.changed.notify_all();
        Ok(dropped)
    }

    fn set_behind(&self, behind: bool) {
        let mut line = locked(&self.line);
        if line.behind != behind {
            line.behind = behind;
            self.changed.notify_all();
        }
    }

    /// The next group to read, waiting for one; `None` once the line is closed and empty.
    fn take(&self) -> Option<Group> {
        let mut line = locked(&self.line);
        loop {
            match line.next() {
                Next::Group(group) => return Some(group),
                Next::Done => return None,
                Next::Wait => line = self.wait(line),
            }
        }
    }

    /// The group taken last has been dealt with.
    fn done(&self) {
        locked(&self.line).busy = false;
        self.changed.notify_all();
    }

    /// Waits until every group has been read, whatever the loop is doing.
    fn drain(&self) {
        let mut line = locked(&self.line);
        line.draining = true;
        self.changed.notify_all();
        while !line.is_idle() {
            line = self.wait(line);
        }
        line.draining = false;
    }

    fn close(&self) {
        locked(&self.line).closing = true;
        self.changed.notify_all();
    }
}

/// What a group came to once it was read again.
#[derive(Debug, PartialEq)]
struct Standing {
    text: String,
    lang: String,
    /// The reading lost content, so the fast text stands.
    kept_fast: bool,
}

/// The text and language that stand for `group` after the reading `refined`.
/// A reading much shorter than the fast texts together lost content, and the
/// fast text stands instead.
fn standing(group: &Group, refined: Refined) -> Standing {
    let read = refined.text.trim().chars().count() as f64;
    let fast = group.fast_text.chars().count() as f64;
    if read < KEEP_RATIO * fast {
        Standing {
            text: group.fast_text.clone(),
            lang: group.lang.clone(),
            kept_fast: true,
        }
    } else {
        Standing {
            text: refined.text,
            lang: refined.lang,
            kept_fast: false,
        }
    }
}

/// A time in milliseconds to three decimals.
fn millis_value(ms: f64) -> Value {
    Value::from((ms * 1000.0).round() / 1000.0)
}

fn audio_ms(group: &Group) -> u64 {
    group.audio.len() as u64 * 1000 / u64::from(SAMPLE_RATE)
}

/// `refine.group.skipped`: the group dropped from a full line.
fn skipped_line(strings: &Strings, dropped: &Group) -> String {
    log::event_line(
        strings,
        Level::Warn,
        GROUP_SKIPPED,
        "asr",
        Some(SKIPPED_CODE),
        &[
            ("lang", Value::from(dropped.lang.as_str())),
            ("audio_ms", Value::from(audio_ms(dropped))),
            ("queue_groups", Value::from(QUEUE_GROUPS)),
        ],
    )
}

/// `refine.group.failed`: the group could not be read again.
fn failed_line(strings: &Strings, group: &Group) -> String {
    log::event_line(
        strings,
        Level::Warn,
        GROUP_FAILED,
        "asr",
        Some(FAILED_CODE),
        &[
            ("lang", Value::from(group.lang.as_str())),
            ("audio_ms", Value::from(audio_ms(group))),
        ],
    )
}

/// `refine.group.refined`: the group was read again and published.
fn refined_line(strings: &Strings, group: &Group, stood: &Standing, refine_ms: f64) -> String {
    log::event_line(
        strings,
        Level::Info,
        GROUP_REFINED,
        "asr",
        None,
        &[
            ("lang", Value::from(stood.lang.as_str())),
            ("audio_ms", Value::from(audio_ms(group))),
            ("refine_ms", millis_value(refine_ms)),
            ("lang_corrected", Value::from(stood.lang != group.lang)),
            ("kept_fast", Value::from(stood.kept_fast)),
        ],
    )
}

/// `refine.transcript.failed`: a line could not be written to the transcript.
fn transcript_failed_line(strings: &Strings) -> String {
    log::event_line(
        strings,
        Level::Warn,
        TRANSCRIPT_FAILED,
        "asr",
        Some(TRANSCRIPT_CODE),
        &[],
    )
}

/// Where the refined lines are also written: a file of the run, or whatever a
/// test hands in. Shared by the worker, which writes the lines, and the
/// caller, which writes the reset mark and closes it.
type Transcript = Arc<Mutex<Option<Box<dyn Write + Send>>>>;

/// The line a group adds to the transcript: the speaker, when there is one, then the text.
fn transcript_line(speaker: &str, text: &str) -> String {
    if speaker.is_empty() {
        format!("{text}\n")
    } else {
        format!("{speaker}: {text}\n")
    }
}

/// Writes `text` to the transcript, if one is open, and flushes it, so that a
/// reader of the file sees every line as soon as it is read again. Says so in
/// the log when it fails.
fn write_transcript(transcript: &Transcript, strings: &Strings, text: &str) {
    let mut sink = locked(transcript);
    let Some(file) = sink.as_mut() else {
        return;
    };
    if file
        .write_all(text.as_bytes())
        .and_then(|()| file.flush())
        .is_err()
    {
        log::print(&transcript_failed_line(strings));
    }
}

/// Where the worker puts what it read: the event stream, the transcript and the log.
struct Output {
    hub: Hub,
    strings: Strings,
    source: &'static str,
    /// The `refine` ids handed out so far; they start at 1 and run on across sessions.
    issued: u64,
    corrections: Arc<AtomicU64>,
    transcript: Transcript,
}

impl Output {
    /// Publishes the reading of `group`, or says it failed. `reading` is
    /// `None` when the recognizers failed. The `refine` event goes out before
    /// the log line, so that a line never delays a subtitle.
    fn deliver(&mut self, group: &Group, reading: Option<Refined>, refine_ms: f64) {
        let Some(refined) = reading else {
            log::print(&failed_line(&self.strings, group));
            return;
        };
        let stood = standing(group, refined);
        if stood.text.trim().is_empty() {
            return;
        }
        if stood.lang != group.lang {
            self.corrections.fetch_add(1, Ordering::Relaxed);
        }
        self.issued += 1;
        self.hub.publish(&wire::refine_event(&Refinement {
            id: self.issued,
            source: self.source,
            t0_s: group.t0_s,
            t1_s: group.t1_s,
            end_wall_ms: group.end_wall_ms,
            text: &stood.text,
            lang: &stood.lang,
            speaker: &group.speaker,
            audio_s: group.audio.len() as f64 / f64::from(SAMPLE_RATE),
        }));
        let line = transcript_line(&group.speaker, &stood.text);
        write_transcript(&self.transcript, &self.strings, &line);
        log::print(&refined_line(&self.strings, group, &stood, refine_ms));
    }
}

/// The worker: reads the groups one at a time, in the order they closed,
/// until the line is closed and empty. A reader that panics fails its group
/// and not the worker, so a caller waiting on the line is never left waiting.
fn work<R: Reread>(queue: &Queue, mut reader: R, mut output: Output) {
    while let Some(group) = queue.take() {
        let started = Instant::now();
        let read = catch_unwind(AssertUnwindSafe(|| {
            reader.reread(&group.audio, &group.lang)
        }));
        let refine_ms = started.elapsed().as_secs_f64() * 1000.0;
        output.deliver(&group, read.ok().and_then(Result::ok), refine_ms);
        queue.done();
    }
    reader.release();
}

/// The refine pass of a live run: collects the finished utterances into
/// groups and reads each group again on a thread of its own.
pub struct Refiner {
    collector: Collector,
    queue: Arc<Queue>,
    worker: Option<JoinHandle<()>>,
    corrections: Arc<AtomicU64>,
    transcript: Transcript,
    /// Groups dropped from a full line since the counts were last taken.
    skipped: u64,
    strings: Strings,
    closed: bool,
}

impl Refiner {
    /// Starts the worker. `make` builds the reader on the worker's thread, so
    /// that the reader need not move between threads. `source` is the
    /// `source` of the run; the events go to `hub`.
    pub fn start<R, M>(
        make: M,
        hub: Hub,
        strings: Strings,
        source: &'static str,
    ) -> std::io::Result<Refiner>
    where
        R: Reread,
        M: FnOnce() -> R + Send + 'static,
    {
        let queue = Arc::new(Queue::default());
        let corrections = Arc::new(AtomicU64::new(0));
        let transcript = Transcript::default();
        let output = Output {
            hub,
            strings: strings.clone(),
            source,
            issued: 0,
            corrections: Arc::clone(&corrections),
            transcript: Arc::clone(&transcript),
        };
        let line = Arc::clone(&queue);
        let worker = std::thread::Builder::new()
            .name(String::from("refine"))
            .spawn(move || work(&line, make(), output))?;
        Ok(Refiner {
            collector: Collector::new(),
            queue,
            worker: Some(worker),
            corrections,
            transcript,
            skipped: 0,
            strings,
            closed: false,
        })
    }

    /// Appends every refined line to the file at `path` (created if it is
    /// not there), until the refiner is closed.
    pub fn open_transcript(&self, path: &Path) -> std::io::Result<()> {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        self.set_transcript(Box::new(file));
        Ok(())
    }

    /// Writes every refined line to `sink` instead of a file. It is dropped,
    /// and so closed, when the refiner is closed.
    pub fn set_transcript(&self, sink: Box<dyn Write + Send>) {
        *locked(&self.transcript) = Some(sink);
    }

    /// Starts a new session: reads what is waiting first, then forgets the
    /// open group and the audio held, and marks the transcript so that its
    /// readers can tell where the new session begins. An open group is not
    /// read here: [`Refiner::flush`] first if it should be.
    pub fn reset(&mut self) -> Result<(), Closed> {
        self.check_open()?;
        self.queue.drain();
        self.collector.reset();
        write_transcript(&self.transcript, &self.strings, RESET_MARK);
        Ok(())
    }

    fn check_open(&self) -> Result<(), Closed> {
        if self.closed { Err(Closed) } else { Ok(()) }
    }

    /// Puts a closed group on the waiting line. A full line drops its oldest
    /// group, which is counted and logged.
    fn enqueue(&mut self, group: Option<Group>) -> Result<(), Closed> {
        let Some(group) = group else {
            return Ok(());
        };
        if let Some(dropped) = self.queue.push(group)? {
            self.skipped += 1;
            log::print(&skipped_line(&self.strings, &dropped));
        }
        Ok(())
    }

    /// Keeps a window of the input, so that a group can be cut from it later.
    pub fn hear(&mut self, window: &[f32]) {
        self.collector.hear(window);
    }

    /// Adds a finished utterance to the open group; a change of language
    /// closes the group before it.
    pub fn add_span(&mut self, spoken: Spoken) -> Result<(), Closed> {
        self.check_open()?;
        let closed = self.collector.add_span(spoken);
        self.enqueue(closed)
    }

    /// Closes the open group when it is due.
    pub fn tick(&mut self) -> Result<(), Closed> {
        self.check_open()?;
        let closed = self.collector.tick();
        self.enqueue(closed)
    }

    /// Closes the open group whatever its age.
    pub fn flush(&mut self) -> Result<(), Closed> {
        self.check_open()?;
        let closed = self.collector.flush();
        self.enqueue(closed)
    }

    /// Tells the worker whether the live loop is behind the audio. While it
    /// is, no new group is started.
    pub fn set_behind(&self, behind: bool) {
        self.queue.set_behind(behind);
    }

    /// Waits until every closed group has been read, whatever the loop is doing.
    pub fn drain(&self) {
        self.queue.drain();
    }

    /// What was counted since the last call.
    pub fn take_counts(&mut self) -> RefineCounts {
        let mut counts = self.collector.take_counts();
        counts.groups_skipped = std::mem::take(&mut self.skipped);
        counts.lang_corrections = self.corrections.swap(0, Ordering::Relaxed);
        counts
    }

    /// Whether the worker's thread is running.
    pub fn worker_running(&self) -> bool {
        self.worker
            .as_ref()
            .is_some_and(|worker| !worker.is_finished())
    }

    /// Reads every group that is waiting, then stops the worker, gives the
    /// reader back and closes the transcript. The open group is not read:
    /// [`Refiner::flush`] first if it should be. Calling it again does nothing.
    pub fn close(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        self.queue.close();
        if let Some(worker) = self.worker.take() {
            // A worker that panicked outside a reading has nothing left to give back.
            let _ = worker.join();
        }
        // After the join: the worker writes the last lines.
        *locked(&self.transcript) = None;
    }
}

impl Drop for Refiner {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hayamimi_asr::refine::group_ranges;
    use std::sync::atomic::AtomicBool;

    const SECOND: usize = SAMPLE_RATE as usize;

    fn spoken(start: usize, end: usize, lang: &str, text: &str) -> Spoken {
        Spoken {
            start,
            end,
            lang: lang.to_owned(),
            text: text.to_owned(),
            speaker: String::new(),
            end_wall_ms: 0,
        }
    }

    /// A collector that has heard `seconds` of silence.
    fn heard(seconds: usize) -> Collector {
        let mut collector = Collector::new();
        collector.hear(&vec![0.0; seconds * SECOND]);
        collector
    }

    #[test]
    fn refiner_add_span_keeps_same_language_in_one_group() {
        let mut collector = heard(3);
        assert!(
            collector
                .add_span(spoken(0, SECOND, "ja", "こんにちは"))
                .is_none()
        );
        assert!(
            collector
                .add_span(spoken(SECOND, 2 * SECOND, "ja", "元気ですか"))
                .is_none()
        );
        assert_eq!(collector.pending(), 2);
        assert_eq!(collector.take_counts().lang_boundary_flushes, 0);
    }

    #[test]
    fn refiner_add_span_splits_group_at_language_boundary() {
        let mut collector = heard(5);
        collector.add_span(spoken(0, SECOND, "ja", "そこの上流かな"));
        let closed = collector.add_span(spoken(
            3 * SECOND,
            4 * SECOND,
            "en",
            "He was in a fevered state of mind",
        ));
        assert!(closed.as_ref().is_some_and(|group| group.lang == "ja"));
        assert_eq!(collector.pending(), 1);
        assert_eq!(collector.take_counts().lang_boundary_flushes, 1);
        let next = collector.flush();
        assert!(next.is_some_and(|group| group.lang == "en"));
    }

    #[test]
    fn refiner_add_span_splits_again_on_return_to_original_language() {
        let mut collector = heard(8);
        collector.add_span(spoken(0, SECOND, "ja", "そこの上流かな"));
        let first = collector.add_span(spoken(
            3 * SECOND,
            4 * SECOND,
            "en",
            "He was in a fevered state of mind",
        ));
        let second = collector.add_span(spoken(
            6 * SECOND,
            7 * SECOND,
            "ja",
            "得意のドルフィンキック",
        ));
        assert_eq!(
            first.map(|group| group.lang),
            Some(String::from("ja")),
            "the ja group closes at the en span"
        );
        assert_eq!(
            second.map(|group| group.lang),
            Some(String::from("en")),
            "the en group closes at the ja span"
        );
        assert_eq!(collector.pending(), 1);
        assert_eq!(collector.take_counts().lang_boundary_flushes, 2);
        assert!(collector.flush().is_some_and(|group| group.lang == "ja"));
    }

    /// The fast path's tag is not trusted over the script of its text: an
    /// `en` span full of kanji is Japanese and stays in the Japanese group.
    #[test]
    fn a_span_whose_script_says_japanese_does_not_split_a_japanese_group() {
        let mut collector = heard(4);
        collector.add_span(spoken(0, SECOND, "ja", "こんにちは"));
        let closed = collector.add_span(spoken(SECOND, 2 * SECOND, "en", "今日はいい天気ですね"));
        assert!(closed.is_none());
        assert_eq!(collector.pending(), 2);
    }

    /// The groups of the live pass against the replica the evaluation tool
    /// uses. The live pass is given the real clock, so a stretch of noise the
    /// recognizer declined to read still advances it: where the replica sees
    /// one group across three segments, the live pass closes two.
    #[test]
    fn production_over_splits_relative_to_eval_replica_on_a_skipped_vad_segment() {
        let first = (0, 16_000);
        let noise = (17_000, 48_000);
        let last = (49_000, 65_000);
        let replica: Vec<Span> = [first, noise, last]
            .iter()
            .map(|&(start, end)| Span {
                start,
                end,
                label: String::from("x"),
            })
            .collect();
        assert_eq!(group_ranges(&replica).len(), 1);

        let mut collector = Collector::new();
        collector.hear(&vec![0.0; 17_000]);
        collector.add_span(spoken(first.0, first.1, "en", "hello there"));
        assert!(collector.tick().is_none());
        // The noise span is never added: the recognizer returned no text for it.
        collector.hear(&vec![0.0; 49_000 - 17_000]);
        assert!(
            collector.tick().is_some(),
            "33000 samples after the first span is due"
        );
        collector.add_span(spoken(last.0, last.1, "en", "second utterance"));
        collector.hear(&vec![0.0; 66_000 - 49_000]);
        assert!(collector.flush().is_some());
        assert_eq!(collector.take_counts().groups_closed, 2);
    }

    #[test]
    fn a_silence_of_two_seconds_closes_the_group_and_one_just_under_does_not() {
        let mut collector = heard(1);
        collector.add_span(spoken(0, SECOND, "ja", "あ"));
        collector.hear(&vec![0.0; GAP_SAMPLES - 1]);
        assert!(collector.tick().is_none());
        collector.hear(&[0.0]);
        assert!(collector.tick().is_some());
        assert_eq!(collector.pending(), 0);
    }

    #[test]
    fn a_group_that_has_run_twenty_five_seconds_is_due_without_a_silence() {
        let mut collector = heard(26);
        collector.add_span(spoken(0, 10 * SECOND, "ja", "あ"));
        collector.add_span(spoken(10 * SECOND, 25 * SECOND - 1, "ja", "い"));
        collector.hear(&[0.0]);
        collector.add_span(spoken(25 * SECOND - 1, 25 * SECOND, "ja", "う"));
        assert!(collector.tick().is_some_and(|group| group.t1_s == 25.0));
    }

    #[test]
    fn nothing_open_is_never_due() {
        let mut collector = heard(40);
        assert!(collector.tick().is_none());
        assert!(collector.flush().is_none());
        assert_eq!(collector.take_counts(), RefineCounts::default());
    }

    #[test]
    fn a_group_with_under_half_a_second_of_audio_is_dropped_and_not_counted() {
        let mut collector = Collector::new();
        collector.hear(&vec![0.0; MIN_AUDIO_SAMPLES - 1]);
        collector.add_span(spoken(0, MIN_AUDIO_SAMPLES - 1, "ja", "あ"));
        assert!(collector.flush().is_none());
        assert_eq!(collector.pending(), 0);
        assert_eq!(collector.take_counts().groups_closed, 0);
    }

    #[test]
    fn a_closed_group_carries_a_second_of_audio_before_it_the_joined_texts_and_the_majority_speaker()
     {
        let mut collector = heard(6);
        let mut one = spoken(3 * SECOND, 4 * SECOND, "ja", "一つ目");
        one.speaker = String::from("S2");
        let mut two = spoken(4 * SECOND, 5 * SECOND, "ja", "二つ目");
        two.speaker = String::from("S2");
        two.end_wall_ms = 7;
        let mut three = spoken(5 * SECOND, 6 * SECOND, "ja", "   ");
        three.speaker = String::from("S1");
        for utterance in [one, two, three] {
            collector.add_span(utterance);
        }
        let Some(group) = collector.flush() else {
            panic!("the group was not closed");
        };
        assert_eq!(group.audio.len(), 4 * SECOND);
        assert_eq!(group.fast_text, "一つ目 二つ目");
        assert_eq!(group.speaker, "S2");
        assert_eq!((group.t0_s, group.t1_s, group.end_wall_ms), (3.0, 6.0, 0));
    }

    #[test]
    fn a_group_with_no_speaker_mark_has_an_empty_speaker() {
        let mut collector = heard(2);
        collector.add_span(spoken(0, SECOND, "ja", "あ"));
        assert!(
            collector
                .flush()
                .is_some_and(|group| group.speaker.is_empty())
        );
    }

    #[test]
    fn the_audio_in_front_of_a_group_is_cut_at_what_the_history_still_holds() {
        let mut collector = Collector::new();
        collector.hear(&vec![0.0; 40 * SECOND]);
        collector.add_span(spoken(35 * SECOND, 36 * SECOND, "ja", "あ"));
        let Some(group) = collector.flush() else {
            panic!("the group was not closed");
        };
        assert_eq!(group.audio.len(), 2 * SECOND);
    }

    #[test]
    fn a_reset_forgets_the_open_group_and_the_audio_but_keeps_the_counts() {
        let mut collector = heard(3);
        collector.add_span(spoken(0, SECOND, "ja", "あ"));
        assert!(collector.flush().is_some());
        collector.add_span(spoken(SECOND, 2 * SECOND, "ja", "い"));
        collector.reset();
        assert_eq!(collector.pending(), 0);
        collector.add_span(spoken(2 * SECOND, 3 * SECOND, "ja", "う"));
        assert!(
            collector.flush().is_none(),
            "the audio before the reset is gone"
        );
        assert_eq!(collector.take_counts().groups_closed, 1);
    }

    #[test]
    fn an_utterance_is_placed_by_the_seconds_the_engine_gave_it() {
        let seconds = 2.5;
        assert_eq!(sample_at(seconds), 40_000);
        assert_eq!(seconds_at(40_000), seconds);
        assert_eq!(sample_at(0.0), 0);
    }

    // The waiting line and the worker. Nothing here reads a model or the clock:
    // a group is held in the line by telling it the loop is behind, and a
    // reading is held by a channel the test lets go.

    use std::sync::mpsc;
    use std::time::Duration;

    use hayamimi_relay::lang::Lang;
    use tokio::sync::broadcast;

    type Check = Result<(), Box<dyn std::error::Error>>;

    /// A closed group in `tag`, which the scripted readers take for its language.
    fn group(tag: &str) -> Group {
        Group {
            audio: vec![0.0; SECOND],
            lang: tag.to_owned(),
            fast_text: String::from("x"),
            speaker: String::new(),
            t0_s: 0.0,
            t1_s: 1.0,
            end_wall_ms: 0,
        }
    }

    /// Reads every group as `text-<lang>` in the language it was given.
    fn echo(lang: &str) -> Result<Refined, RouteError> {
        Ok(Refined {
            text: format!("text-{lang}"),
            lang: lang.to_owned(),
        })
    }

    fn echo_read(_audio: &[f32], lang: &str) -> Result<Refined, RouteError> {
        echo(lang)
    }

    /// A reader that answers with a closure and holds a token for as long as it lives.
    struct Scripted<F> {
        answer: F,
        token: Arc<()>,
    }

    fn scripted<F>(answer: F) -> Scripted<F>
    where
        F: FnMut(&[f32], &str) -> Result<Refined, RouteError>,
    {
        Scripted {
            answer,
            token: Arc::new(()),
        }
    }

    impl<F> Reread for Scripted<F>
    where
        F: FnMut(&[f32], &str) -> Result<Refined, RouteError>,
    {
        fn reread(&mut self, audio: &[f32], lang: &str) -> Result<Refined, RouteError> {
            let _held = &self.token;
            (self.answer)(audio, lang)
        }
    }

    /// A refiner over `make`, and the receiving end of the events it publishes.
    fn started<R, M>(make: M) -> Result<(Refiner, broadcast::Receiver<String>), String>
    where
        R: Reread,
        M: FnOnce() -> R + Send + 'static,
    {
        let hub = Hub::new();
        let events = hub.subscribe_live();
        let strings = Strings::embedded(Lang::En)?;
        let refiner =
            Refiner::start(make, hub, strings, "file").map_err(|error| error.to_string())?;
        Ok((refiner, events))
    }

    /// The `refine` events published since the last call.
    fn refine_events(events: &mut broadcast::Receiver<String>) -> Result<Vec<Value>, String> {
        let mut found = Vec::new();
        while let Ok(line) = events.try_recv() {
            let event: Value = serde_json::from_str(&line).map_err(|error| error.to_string())?;
            if event["type"] == "refine" {
                found.push(event);
            }
        }
        Ok(found)
    }

    fn refined_texts(events: &mut broadcast::Receiver<String>) -> Result<Vec<String>, String> {
        Ok(refine_events(events)?
            .iter()
            .filter_map(|event| event["text"].as_str().map(str::to_owned))
            .collect())
    }

    #[test]
    fn the_line_holds_a_group_while_the_loop_is_behind_and_gives_it_when_it_is_not() {
        let mut line = Line::default();
        line.push(group("l1"));
        line.behind = true;
        assert_eq!(line.next(), Next::Wait);
        line.behind = false;
        assert!(matches!(line.next(), Next::Group(read) if read.lang == "l1"));
        assert!(line.busy);
    }

    #[test]
    fn a_line_that_is_draining_or_closing_starts_groups_although_the_loop_is_behind() {
        for (draining, closing) in [(true, false), (false, true)] {
            let mut line = Line::default();
            line.push(group("l1"));
            line.behind = true;
            line.draining = draining;
            line.closing = closing;
            assert!(
                matches!(line.next(), Next::Group(_)),
                "{draining} {closing}"
            );
        }
    }

    #[test]
    fn a_line_waits_when_it_is_empty_and_ends_only_once_closed_and_empty() {
        let mut line = Line::default();
        assert_eq!(line.next(), Next::Wait);
        line.push(group("l1"));
        line.closing = true;
        assert!(matches!(line.next(), Next::Group(_)));
        assert_eq!(line.next(), Next::Done);
    }

    #[test]
    fn groups_come_out_of_the_line_in_the_order_they_went_in() {
        let mut line = Line::default();
        for tag in ["l1", "l2", "l3"] {
            line.push(group(tag));
        }
        let order: Vec<String> = std::iter::from_fn(|| match line.next() {
            Next::Group(read) => Some(read.lang),
            Next::Wait | Next::Done => None,
        })
        .collect();
        assert_eq!(order, ["l1", "l2", "l3"]);
    }

    #[test]
    fn a_fifth_group_pushes_the_oldest_out_of_the_line() {
        let mut line = Line::default();
        let pushed: Vec<Option<String>> = ["l1", "l2", "l3", "l4", "l5", "l6"]
            .iter()
            .map(|tag| line.push(group(tag)).map(|dropped| dropped.lang))
            .collect();
        assert_eq!(pushed[..4], [None, None, None, None]);
        assert_eq!(pushed[4].as_deref(), Some("l1"));
        assert_eq!(pushed[5].as_deref(), Some("l2"));
        assert_eq!(line.groups.len(), QUEUE_GROUPS);
    }

    #[test]
    fn a_line_is_idle_only_when_it_is_empty_and_nothing_is_being_read() {
        let mut line = Line::default();
        assert!(line.is_idle());
        line.push(group("l1"));
        assert!(!line.is_idle());
        assert!(matches!(line.next(), Next::Group(_)));
        assert!(!line.is_idle(), "the group taken is still being read");
        line.busy = false;
        assert!(line.is_idle());
    }

    #[test]
    fn refiner_close_ends_the_worker_thread() -> Check {
        let (mut refiner, _events) = started(|| scripted(echo_read))?;
        assert!(refiner.worker_running());
        refiner.close();
        assert!(!refiner.worker_running());
        Ok(())
    }

    #[test]
    fn refiner_close_is_idempotent() -> Check {
        let (mut refiner, _events) = started(|| scripted(echo_read))?;
        refiner.close();
        refiner.close();
        assert!(!refiner.worker_running());
        Ok(())
    }

    #[test]
    fn refiner_close_runs_the_queued_backlog_first() -> Check {
        let (mut refiner, mut events) = started(|| scripted(echo_read))?;
        // The loop is behind, so nothing starts until the close lets the line empty.
        refiner.set_behind(true);
        refiner.enqueue(Some(group("l1")))?;
        refiner.enqueue(Some(group("l2")))?;
        assert!(refined_texts(&mut events)?.is_empty());
        refiner.close();
        assert_eq!(refined_texts(&mut events)?, ["text-l1", "text-l2"]);
        Ok(())
    }

    #[test]
    fn refiner_refuses_work_after_close() -> Check {
        let (mut refiner, _events) = started(|| scripted(echo_read))?;
        refiner.close();
        assert_eq!(
            refiner.add_span(spoken(0, SECOND, "ja", "text")),
            Err(Closed)
        );
        assert_eq!(refiner.tick(), Err(Closed));
        assert_eq!(refiner.flush(), Err(Closed));
        assert_eq!(refiner.enqueue(Some(group("l1"))), Err(Closed));
        Ok(())
    }

    #[test]
    fn closing_the_refiner_releases_the_engine() -> Check {
        let engine = Arc::new(());
        let watch = Arc::downgrade(&engine);
        let (mut refiner, _events) = started(move || Scripted {
            answer: echo_read,
            token: engine,
        })?;
        assert!(
            watch.upgrade().is_some(),
            "the live refiner holds its reader"
        );
        refiner.close();
        assert!(
            watch.upgrade().is_none(),
            "the reader outlived its closed refiner"
        );
        Ok(())
    }

    /// The reader is released by the worker, once, after the last group.
    #[test]
    fn the_reader_is_released_after_the_last_group_and_not_before() -> Check {
        struct Counting(Arc<Mutex<Vec<&'static str>>>);
        impl Reread for Counting {
            fn reread(&mut self, _audio: &[f32], lang: &str) -> Result<Refined, RouteError> {
                locked(&self.0).push("read");
                echo(lang)
            }
            fn release(&mut self) {
                locked(&self.0).push("release");
            }
        }
        let log = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&log);
        let (mut refiner, _events) = started(move || Counting(seen))?;
        refiner.set_behind(true);
        refiner.enqueue(Some(group("l1")))?;
        refiner.enqueue(Some(group("l2")))?;
        refiner.close();
        assert_eq!(*locked(&log), ["read", "read", "release"]);
        Ok(())
    }

    #[test]
    fn refiner_worker_prints_groups_in_enqueue_order_despite_slower_first_decode() -> Check {
        let (release, gate) = mpsc::channel::<()>();
        let (mut refiner, mut events) = started(move || {
            scripted(move |_audio, lang| {
                if lang == "l1" {
                    // The first group is slow: it waits here until the test lets it go.
                    let _ = gate.recv();
                }
                echo(lang)
            })
        })?;
        refiner.set_behind(true);
        refiner.enqueue(Some(group("l1")))?;
        refiner.enqueue(Some(group("l2")))?;
        refiner.set_behind(false);
        // A worker that read the second group while the first was held would
        // finish it in this time. The correct one cannot, and does not need it.
        std::thread::sleep(Duration::from_millis(100));
        release.send(())?;
        refiner.close();
        assert_eq!(refined_texts(&mut events)?, ["text-l1", "text-l2"]);
        Ok(())
    }

    #[test]
    fn refiner_shutdown_must_drain_full_backlog_not_just_last_task() -> Check {
        let (mut refiner, mut events) = started(|| scripted(echo_read))?;
        // A group closed earlier is still waiting when the end of the run asks
        // for the open group, and finds none.
        refiner.set_behind(true);
        refiner.enqueue(Some(group("late")))?;
        refiner.flush()?;
        assert!(refined_texts(&mut events)?.is_empty());
        refiner.close();
        assert_eq!(refined_texts(&mut events)?, ["text-late"]);
        Ok(())
    }

    #[test]
    fn a_full_line_drops_the_oldest_group_and_counts_it() -> Check {
        let (mut refiner, mut events) = started(|| scripted(echo_read))?;
        refiner.set_behind(true);
        for tag in ["l1", "l2", "l3", "l4", "l5"] {
            refiner.enqueue(Some(group(tag)))?;
        }
        assert_eq!(refiner.take_counts().groups_skipped, 1);
        assert_eq!(refiner.take_counts().groups_skipped, 0, "taken once");
        refiner.close();
        assert_eq!(
            refined_texts(&mut events)?,
            ["text-l2", "text-l3", "text-l4", "text-l5"]
        );
        Ok(())
    }

    #[test]
    fn the_skipped_line_is_a_warning_that_names_the_group_and_not_its_text() -> Check {
        let strings = Strings::embedded(Lang::En)?;
        let event: Value = serde_json::from_str(&skipped_line(&strings, &group("ja")))?;
        assert_eq!(event["event"], GROUP_SKIPPED);
        assert_eq!(event["level"], "warn");
        assert_eq!(event["error_code"], SKIPPED_CODE);
        assert_eq!(event["lang"], "ja");
        assert_eq!(event["audio_ms"], 1000);
        assert_eq!(event["queue_groups"], QUEUE_GROUPS);
        assert_ne!(event["message"], GROUP_SKIPPED, "the table has a message");
        Ok(())
    }

    #[test]
    fn the_failed_and_refined_lines_carry_their_level_code_and_no_text() -> Check {
        let strings = Strings::embedded(Lang::En)?;
        let failed: Value = serde_json::from_str(&failed_line(&strings, &group("ja")))?;
        assert_eq!(failed["event"], GROUP_FAILED);
        assert_eq!(failed["level"], "warn");
        assert_eq!(failed["error_code"], FAILED_CODE);
        let stood = Standing {
            text: String::from("secret words"),
            lang: String::from("en"),
            kept_fast: false,
        };
        let line = refined_line(&strings, &group("ja"), &stood, 12.3456);
        let refined: Value = serde_json::from_str(&line)?;
        assert_eq!(refined["event"], GROUP_REFINED);
        assert_eq!(refined["level"], "info");
        assert_eq!(refined["lang_corrected"], true);
        assert_eq!(refined["refine_ms"], 12.346);
        assert!(!line.contains("secret words"));
        Ok(())
    }

    #[test]
    fn the_refine_events_are_numbered_from_one_in_the_order_they_were_read() -> Check {
        let (mut refiner, mut events) = started(|| scripted(echo_read))?;
        refiner.enqueue(Some(group("l1")))?;
        refiner.enqueue(Some(group("l2")))?;
        refiner.close();
        let ids: Vec<String> = refine_events(&mut events)?
            .iter()
            .filter_map(|event| event["id"].as_str().map(str::to_owned))
            .collect();
        assert_eq!(ids, ["file-1", "file-2"]);
        Ok(())
    }

    #[test]
    fn a_group_that_cannot_be_read_keeps_its_fast_text_and_the_worker_goes_on() -> Check {
        let (mut refiner, mut events) = started(|| {
            scripted(|_audio, lang| match lang {
                "bad" => Err(RouteError::NoModels),
                "boom" => panic!("the recognizer panicked"),
                other => echo(other),
            })
        })?;
        for tag in ["bad", "boom", "ok"] {
            refiner.enqueue(Some(group(tag)))?;
        }
        refiner.drain();
        assert_eq!(refined_texts(&mut events)?, ["text-ok"]);
        refiner.close();
        Ok(())
    }

    #[test]
    fn drain_reads_what_is_waiting_although_the_loop_is_behind() -> Check {
        let (mut refiner, mut events) = started(|| scripted(echo_read))?;
        refiner.set_behind(true);
        refiner.enqueue(Some(group("l1")))?;
        refiner.drain();
        assert_eq!(refined_texts(&mut events)?, ["text-l1"]);
        // The worker holds again afterwards.
        refiner.enqueue(Some(group("l2")))?;
        assert!(refined_texts(&mut events)?.is_empty());
        refiner.close();
        assert_eq!(refined_texts(&mut events)?, ["text-l2"]);
        Ok(())
    }

    #[test]
    fn a_group_read_in_another_language_is_published_in_it_and_counted() -> Check {
        let (mut refiner, mut events) = started(|| {
            scripted(|_audio, _lang| {
                Ok(Refined {
                    text: String::from("hello there"),
                    lang: String::from("en"),
                })
            })
        })?;
        refiner.enqueue(Some(group("ja")))?;
        refiner.close();
        let published = refine_events(&mut events)?;
        assert_eq!(published.len(), 1);
        assert_eq!(published[0]["lang"], "en");
        assert_eq!(published[0]["text"], "hello there");
        assert_eq!(refiner.take_counts().lang_corrections, 1);
        Ok(())
    }

    #[test]
    fn a_reading_that_lost_content_gives_way_to_the_fast_text_in_the_fast_language() {
        let mut long = group("ja");
        long.fast_text = String::from("これは速報の長い文ですこれは速報の長い文です");
        let short = Refined {
            text: String::from("これは"),
            lang: String::from("en"),
        };
        let stood = standing(&long, short);
        assert_eq!(
            stood,
            Standing {
                text: long.fast_text.clone(),
                lang: String::from("ja"),
                kept_fast: true
            }
        );
        let whole = Refined {
            text: String::from("これは速報の長い文ですこれは速報の長い文です。"),
            lang: String::from("ja"),
        };
        assert!(!standing(&long, whole).kept_fast);
    }

    #[test]
    fn a_blank_reading_with_a_blank_fast_text_publishes_nothing() -> Check {
        let (mut refiner, mut events) = started(|| {
            scripted(|_audio, lang| {
                Ok(Refined {
                    text: String::from("  "),
                    lang: lang.to_owned(),
                })
            })
        })?;
        let mut blank = group("ja");
        blank.fast_text = String::new();
        refiner.enqueue(Some(blank))?;
        refiner.close();
        assert!(refine_events(&mut events)?.is_empty());
        Ok(())
    }

    #[test]
    fn the_refiner_closes_a_group_into_the_line_and_the_worker_reads_it() -> Check {
        let (mut refiner, mut events) = started(|| scripted(echo_read))?;
        refiner.hear(&vec![0.0; 3 * SECOND]);
        refiner.add_span(spoken(0, SECOND, "ja", "こんにちは"))?;
        refiner.add_span(spoken(2 * SECOND, 3 * SECOND, "en", "hello"))?;
        refiner.flush()?;
        refiner.close();
        assert_eq!(refined_texts(&mut events)?, ["text-ja", "text-en"]);
        let counts = refiner.take_counts();
        assert_eq!((counts.groups_closed, counts.lang_boundary_flushes), (2, 1));
        Ok(())
    }

    #[test]
    fn dropping_a_refiner_that_was_not_closed_joins_its_worker() -> Check {
        let engine = Arc::new(());
        let watch = Arc::downgrade(&engine);
        let (refiner, _events) = started(move || Scripted {
            answer: echo_read,
            token: engine,
        })?;
        drop(refiner);
        assert!(watch.upgrade().is_none());
        Ok(())
    }

    /// A transcript that keeps what it is given, and says when it is dropped.
    struct Sink {
        written: Arc<Mutex<Vec<u8>>>,
        dropped: Arc<AtomicBool>,
        broken: bool,
    }

    impl Write for Sink {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self.broken {
                return Err(std::io::Error::other("the disk is full"));
            }
            locked(&self.written).extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Drop for Sink {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::SeqCst);
        }
    }

    /// What a [`Sink`] was given, and whether it was dropped.
    struct Watch {
        written: Arc<Mutex<Vec<u8>>>,
        dropped: Arc<AtomicBool>,
    }

    impl Watch {
        fn text(&self) -> String {
            String::from_utf8_lossy(&locked(&self.written)).into_owned()
        }

        fn is_dropped(&self) -> bool {
            self.dropped.load(Ordering::SeqCst)
        }
    }

    fn sink(broken: bool) -> (Sink, Watch) {
        let written = Arc::new(Mutex::new(Vec::new()));
        let dropped = Arc::new(AtomicBool::new(false));
        let sink = Sink {
            written: Arc::clone(&written),
            dropped: Arc::clone(&dropped),
            broken,
        };
        (sink, Watch { written, dropped })
    }

    /// An utterance of the first second, marked as spoken by `speaker`.
    fn spoken_by(speaker: &str) -> Spoken {
        Spoken {
            speaker: speaker.to_owned(),
            ..spoken(0, SECOND, "ja", "こんにちは")
        }
    }

    #[test]
    fn a_line_has_the_speaker_in_front_when_there_is_one() {
        assert_eq!(transcript_line("S1", "こんにちは"), "S1: こんにちは\n");
        assert_eq!(transcript_line("", "こんにちは"), "こんにちは\n");
    }

    #[test]
    fn refiner_close_closes_the_transcript_file() -> Check {
        let (mut refiner, _events) = started(|| scripted(echo_read))?;
        let (sink, watch) = sink(false);
        refiner.set_transcript(Box::new(sink));
        refiner.hear(&vec![0.0; 3 * SECOND]);
        refiner.add_span(spoken_by("S1"))?;
        refiner.flush()?;
        refiner.drain();
        assert!(
            !watch.is_dropped(),
            "the transcript stays open while the refiner runs"
        );
        assert_eq!(watch.text(), "S1: text-ja\n");
        refiner.close();
        assert!(watch.is_dropped());
        Ok(())
    }

    #[test]
    fn a_group_read_while_closing_is_written_before_the_transcript_closes() -> Check {
        let (mut refiner, _events) = started(|| scripted(echo_read))?;
        let (sink, watch) = sink(false);
        refiner.set_transcript(Box::new(sink));
        refiner.hear(&vec![0.0; 3 * SECOND]);
        refiner.add_span(spoken_by(""))?;
        refiner.flush()?;
        refiner.close();
        assert_eq!(watch.text(), "text-ja\n");
        assert!(watch.is_dropped());
        Ok(())
    }

    #[test]
    fn a_transcript_that_cannot_be_written_does_not_stop_the_events_or_the_worker() -> Check {
        let (mut refiner, mut events) = started(|| scripted(echo_read))?;
        let (sink, watch) = sink(true);
        refiner.set_transcript(Box::new(sink));
        refiner.hear(&vec![0.0; 3 * SECOND]);
        refiner.add_span(spoken_by(""))?;
        refiner.flush()?;
        refiner.add_span(Spoken {
            lang: String::from("en"),
            ..spoken(2 * SECOND, 3 * SECOND, "en", "hello")
        })?;
        refiner.flush()?;
        refiner.close();
        assert_eq!(refined_texts(&mut events)?, ["text-ja", "text-en"]);
        assert_eq!(watch.text(), "");
        Ok(())
    }

    #[test]
    fn the_transcript_failed_line_is_a_warning_with_a_code_and_no_text() -> Check {
        let strings = Strings::embedded(Lang::En)?;
        let event: Value = serde_json::from_str(&transcript_failed_line(&strings))?;
        assert_eq!(event["event"], TRANSCRIPT_FAILED);
        assert_eq!(event["level"], "warn");
        assert_eq!(event["error_code"], TRANSCRIPT_CODE);
        assert_ne!(
            event["message"], TRANSCRIPT_FAILED,
            "the table has a message"
        );
        Ok(())
    }

    #[test]
    fn open_transcript_appends_to_the_file_and_creates_it() -> Check {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/unit-tmp");
        std::fs::create_dir_all(&directory)?;
        let path = directory.join(format!("refine-transcript-{}.txt", std::process::id()));
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        for _ in 0..2 {
            let (mut refiner, _events) = started(|| scripted(echo_read))?;
            refiner.open_transcript(&path)?;
            refiner.hear(&vec![0.0; 3 * SECOND]);
            refiner.add_span(spoken_by("S2"))?;
            refiner.flush()?;
            refiner.close();
        }
        let content = std::fs::read_to_string(&path)?;
        std::fs::remove_file(&path)?;
        assert_eq!(content, "S2: text-ja\nS2: text-ja\n");
        Ok(())
    }

    #[test]
    fn refiner_reset_drops_spans_and_clears_history() -> Check {
        let (mut refiner, mut events) = started(|| scripted(echo_read))?;
        refiner.hear(&vec![0.0; 3 * SECOND]);
        refiner.add_span(spoken(0, SECOND, "ja", "あ"))?;
        assert_eq!(refiner.collector.pending(), 1);
        refiner.reset()?;
        assert_eq!(refiner.collector.pending(), 0, "the open group is dropped");
        // The audio is gone too: an utterance heard after the reset has nothing to be cut from.
        refiner.add_span(spoken(2 * SECOND, 3 * SECOND, "ja", "い"))?;
        refiner.flush()?;
        refiner.close();
        assert!(refined_texts(&mut events)?.is_empty());
        assert_eq!(refiner.take_counts().groups_closed, 0);
        Ok(())
    }

    #[test]
    fn a_reset_reads_the_groups_that_are_waiting_before_it_forgets() -> Check {
        let (mut refiner, mut events) = started(|| scripted(echo_read))?;
        refiner.hear(&vec![0.0; 3 * SECOND]);
        refiner.add_span(spoken(0, SECOND, "ja", "あ"))?;
        refiner.flush()?;
        refiner.reset()?;
        assert_eq!(refined_texts(&mut events)?, ["text-ja"]);
        refiner.close();
        Ok(())
    }

    #[test]
    fn refiner_reset_writes_a_separator_into_an_open_transcript() -> Check {
        let (mut refiner, _events) = started(|| scripted(echo_read))?;
        let (sink, watch) = sink(false);
        refiner.set_transcript(Box::new(sink));
        refiner.reset()?;
        assert_eq!(watch.text(), "\n----- session reset -----\n\n");
        assert!(
            !watch.is_dropped(),
            "the transcript stays open across a reset"
        );
        // The new session goes on in the same file, after the mark.
        refiner.hear(&vec![0.0; 2 * SECOND]);
        refiner.add_span(spoken_by(""))?;
        refiner.flush()?;
        refiner.drain();
        assert_eq!(watch.text(), "\n----- session reset -----\n\ntext-ja\n");
        Ok(())
    }

    #[test]
    fn a_closed_refiner_cannot_be_reset() -> Check {
        let (mut refiner, _events) = started(|| scripted(echo_read))?;
        refiner.close();
        assert_eq!(refiner.reset(), Err(Closed));
        Ok(())
    }

    /// Resident set size in KiB, from `/proc/self/status`.
    fn rss_kib() -> Result<u64, Box<dyn std::error::Error>> {
        let status = std::fs::read_to_string("/proc/self/status")?;
        let line = status
            .lines()
            .find_map(|line| line.strip_prefix("VmRSS:"))
            .ok_or("no VmRSS line")?;
        Ok(line.trim().trim_end_matches("kB").trim().parse()?)
    }

    /// The first `models/` at or above this crate that holds the VAD model;
    /// model files are not in git, so a worktree reads the main checkout's.
    fn models_dir() -> Result<std::path::PathBuf, String> {
        let start = env!("CARGO_MANIFEST_DIR");
        Path::new(start)
            .ancestors()
            .map(|directory| directory.join("models"))
            .find(|models| models.join("silero_vad.onnx").is_file())
            .ok_or_else(|| format!("no models/silero_vad.onnx at or above {start}"))
    }

    /// Run with `--ignored --nocapture`: RSS before and after the refine
    /// router is made (no model is loaded until the first group).
    #[test]
    #[ignore = "measures; needs the models directory"]
    fn measure_rss_before_and_after_the_refine_router_is_made() -> Check {
        let before = rss_kib()?;
        let reader = Rereader::new(&models_dir()?, Policy::balanced());
        let after = rss_kib()?;
        eprintln!("VmRSS before = {before} KiB, after Rereader::new = {after} KiB");
        drop(reader);
        Ok(())
    }

    /// Run with `--ignored --nocapture` and `CUDA_VISIBLE_DEVICES=-1`: the
    /// time of `Router::refine` on a 25 s group, five runs after a first one
    /// that loads the models.
    #[test]
    #[ignore = "measures; needs the models directory and the golden clips"]
    fn measure_refine_of_a_25_second_group_on_the_cpu() -> Check {
        let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden/ja");
        let mut audio = Vec::new();
        for clip in ["ja_014.wav", "ja_024.wav", "ja_033.wav"] {
            audio.extend(hayamimi_asr::audio::read_wav(&golden.join(clip))?);
        }
        audio.truncate(25 * SECOND);
        let mut reader = Rereader::new(&models_dir()?, Policy::balanced());
        let first = Instant::now();
        reader.reread(&audio, "ja")?;
        eprintln!(
            "first (loads the models) = {:.3} s",
            first.elapsed().as_secs_f64()
        );
        let rss_loaded = rss_kib()?;
        let mut seconds = Vec::new();
        for _ in 0..5 {
            let start = Instant::now();
            reader.reread(&audio, "ja")?;
            seconds.push(start.elapsed().as_secs_f64());
        }
        seconds.sort_by(f64::total_cmp);
        eprintln!(
            "runs sorted = {seconds:.3?} s, p50 = {:.3} s, VmRSS after load = {rss_loaded} KiB",
            seconds[2]
        );
        reader.release();
        Ok(())
    }
}

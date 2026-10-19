//! The text of an utterance in progress, translated again each time it grows.
//!
//! A recognizer sends the growing text as `partial` events. They carry neither
//! the source nor the `id` the line will take, so the relay reads both off the
//! stream: the source is the source of the last `final` on it, and the `id` is
//! the one that follows that `final`'s (`mic-4`, then `mic-5`). Until a `final`
//! has come nothing is known, and nothing is translated.
//!
//! One translation runs at a time. A newer text of a source makes the older
//! one useless, so a [`Mailbox`] keeps the newest text of each source and the
//! request for the older one is dropped. The `final` of a source does the same
//! for the text of the utterance it closed, with one exception: when the
//! `final` only closes the last text (it is the same text, or the same with a
//! `。` added), the translation of that text is the translation T would give
//! the line, so the request is kept, and the lane of the line takes its answer
//! with [`Mailbox::claim`] instead of asking T again.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::lang::Lang;
use crate::line::{Line, SOURCES, Source};
use crate::llm::Wanted;
use crate::logging::{Level, LogEvent, LogSink};
use crate::pipeline::{LineKind, LineRequest, Reuse, ReuseMatch, ReuseState};

/// How long a line waits for the request of its text in progress before it
/// asks T itself. A request ends within the translator's own time limit, so
/// this only guards against a worker that is gone.
const CLAIM_WAIT_MAX: Duration = Duration::from_secs(5);

/// How many claimed translations are kept for lanes that have not collected
/// them yet; the oldest goes first. A line that is never translated (its
/// language, or only another route) leaves its claim here.
const TAKEN_MAX: usize = 8;

/// Whether the relay translates the text of an utterance in progress.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartialTranslation {
    Off,
    On,
}

/// A text in progress to translate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    pub source: Source,
    /// The `id` the line will take when it ends.
    pub id: String,
    pub src_lang: Lang,
    pub text: String,
}

impl Job {
    pub fn request(&self, now_epoch: i64) -> LineRequest<'_> {
        LineRequest {
            id: &self.id,
            of: LineKind::Partial,
            source: self.source.as_str(),
            src_lang: self.src_lang,
            text: &self.text,
            now_epoch,
        }
    }
}

struct Pending {
    job: Job,
    /// Which text was left first, among those waiting.
    order: u64,
    /// What the source's counter read when the text was left.
    generation: u64,
    /// Set when a line takes the answer over; see [`Tracked::pin`].
    pin: Arc<AtomicBool>,
}

/// How far the translation of a text in progress got.
enum Phase {
    /// Left in the mailbox; no worker has taken it.
    Waiting,
    /// The worker is asking T.
    Running,
    /// The request ended: T's answer as the translator gave it, or `None`
    /// when it gave none or the request was dropped.
    Done(Option<String>),
}

/// The last text left for a source, and how its translation stands.
struct Tracked {
    id: String,
    text: String,
    generation: u64,
    phase: Phase,
    /// Set when a line takes the answer over: the request then goes on though
    /// a newer text or the `final` moved the counter.
    pin: Arc<AtomicBool>,
}

/// A translation a `final` took over, until the lane of its line collects it.
struct Taken {
    source: Source,
    matched: ReuseMatch,
    tracked: Tracked,
}

/// How the text of a `final` relates to the text in progress it closes: the
/// same text, or the same with one `。` added. Any other text, or a text of
/// another `id`, is not that utterance's translation.
fn closes(line: &Line, tracked: &Tracked) -> Option<ReuseMatch> {
    if line.id != tracked.id || matches!(tracked.phase, Phase::Waiting | Phase::Done(None)) {
        return None;
    }
    if line.text == tracked.text {
        return Some(ReuseMatch::Same);
    }
    (line.text.strip_suffix('。') == Some(tracked.text.as_str())).then_some(ReuseMatch::ClosingMark)
}

/// What the screen has drawn of one source's translated text in progress. The
/// screen draws the longest common prefix of the last two texts in the settled
/// form, so a text that does not begin with that prefix would make drawn
/// characters vanish.
#[derive(Default)]
struct Drawn {
    id: String,
    text: String,
    /// The longest common prefix of the last two texts drawn for `id`.
    committed: String,
}

impl Drawn {
    /// Records `text` of `id` as drawn, and tells whether it may be: a text
    /// that keeps the committed prefix of the same `id`, or the first of an `id`.
    fn admit(&mut self, id: &str, text: &str) -> bool {
        if self.id != id {
            self.id = id.to_owned();
            self.committed.clear();
        } else if !text.starts_with(&self.committed) {
            return false;
        } else {
            self.committed = common_prefix(&self.text, text).to_owned();
        }
        self.text = text.to_owned();
        true
    }
}

/// The longest common prefix of two texts, cut at a character boundary.
fn common_prefix<'a>(left: &'a str, right: &str) -> &'a str {
    let mut end = 0;
    for ((at, a), b) in left.char_indices().zip(right.chars()) {
        if a != b {
            break;
        }
        end = at + a.len_utf8();
    }
    &left[..end]
}

#[derive(Default)]
struct State {
    drawn: [Drawn; SOURCES],
    pending: [Option<Pending>; SOURCES],
    /// The last text left for each source, so that the same text twice asks
    /// nothing new, and how its translation stands.
    last: [Option<Tracked>; SOURCES],
    /// Translations a `final` took over, the oldest first.
    taken: Vec<Taken>,
    left: u64,
}

impl State {
    /// The translation of `generation` of `source`, wherever it stands now.
    fn tracked_mut(&mut self, source: Source, generation: u64) -> Option<&mut Tracked> {
        let slot = source.index();
        if self.last[slot]
            .as_ref()
            .is_some_and(|tracked| tracked.generation == generation)
        {
            return self.last[slot].as_mut();
        }
        self.taken
            .iter_mut()
            .filter(|taken| taken.source == source)
            .map(|taken| &mut taken.tracked)
            .find(|tracked| tracked.generation == generation)
    }

    /// Keeps a translation for the lane of its line; the oldest one still
    /// kept is let go when too many wait.
    fn hand_over(&mut self, taken: Taken) {
        if self.taken.len() >= TAKEN_MAX {
            self.taken.remove(0);
        }
        self.taken.push(taken);
    }

    fn position(&self, source: Source, id: &str) -> Option<usize> {
        self.taken
            .iter()
            .position(|taken| taken.source == source && taken.tracked.id == id)
    }
}

fn locked(state: &Mutex<State>) -> MutexGuard<'_, State> {
    match state.lock() {
        Ok(guard) => guard,
        // A thread that panicked while holding the lock left whole texts.
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Waits on `signal` for at most `limit`, and gives the lock back.
fn waited_on<'a>(
    signal: &Condvar,
    state: MutexGuard<'a, State>,
    limit: Duration,
) -> MutexGuard<'a, State> {
    match signal.wait_timeout(state, limit) {
        Ok((guard, _)) => guard,
        Err(poisoned) => poisoned.into_inner().0,
    }
}

/// The newest text of every source, waiting for the one worker that translates.
pub struct Mailbox {
    state: Mutex<State>,
    wake: Condvar,
    /// Signalled when a request ends, for the lanes waiting for its answer.
    finished: Condvar,
    /// One counter per source. A request is wanted while its source's counter
    /// reads what it read when the request was made.
    generations: [Arc<AtomicU64>; SOURCES],
}

impl Mailbox {
    pub fn new() -> Mailbox {
        Mailbox {
            state: Mutex::new(State::default()),
            wake: Condvar::new(),
            finished: Condvar::new(),
            generations: std::array::from_fn(|_| Arc::new(AtomicU64::new(0))),
        }
    }

    /// Leaves `job` in the place of the older text of its source, and makes
    /// the request for that text stop. The text the source had last changes
    /// nothing.
    pub fn post(&self, job: Job) {
        let slot = job.source.index();
        let mut state = locked(&self.state);
        if state.last[slot]
            .as_ref()
            .is_some_and(|tracked| tracked.text == job.text)
        {
            return;
        }
        state.left += 1;
        let order = state.left;
        let generation = self.generations[slot].fetch_add(1, Ordering::AcqRel) + 1;
        let pin = Arc::new(AtomicBool::new(false));
        state.last[slot] = Some(Tracked {
            id: job.id.clone(),
            text: job.text.clone(),
            generation,
            phase: Phase::Waiting,
            pin: Arc::clone(&pin),
        });
        state.pending[slot] = Some(Pending {
            job,
            order,
            generation,
            pin,
        });
        drop(state);
        self.wake.notify_one();
    }

    /// The utterance of `source` ended: the text in progress is of no use any
    /// more, waiting or running.
    pub fn retire(&self, source: Source) {
        self.end(source, None);
    }

    /// `line`, a `final`, ended the utterance of its source. The text in
    /// progress is of no use any more, unless the line only closes it (see
    /// [`closes`]) and a worker has taken it: then its request goes on and its
    /// answer is the line's, collected with [`Mailbox::claim`]. A text no
    /// worker has taken is dropped, and the lane of the line asks T itself.
    pub fn close(&self, line: &Line) {
        self.end(line.source, Some(line));
    }

    fn end(&self, source: Source, line: Option<&Line>) {
        let slot = source.index();
        let mut state = locked(&self.state);
        self.generations[slot].fetch_add(1, Ordering::AcqRel);
        state.pending[slot] = None;
        state.drawn[slot] = Drawn::default();
        let Some(tracked) = state.last[slot].take() else {
            return;
        };
        let Some(matched) = line.and_then(|line| closes(line, &tracked)) else {
            return;
        };
        tracked.pin.store(true, Ordering::Release);
        state.hand_over(Taken {
            source,
            matched,
            tracked,
        });
    }

    /// The worker holds a server slot and is about to send the request of
    /// `generation`: from now on a `final` that only closes its text may take
    /// the request over. Before this the request is dropped unsent.
    pub fn begin(&self, source: Source, generation: u64) {
        if let Some(tracked) = locked(&self.state).tracked_mut(source, generation) {
            tracked.phase = Phase::Running;
        }
    }

    /// The request of `generation` for `source` ended with `answer` (`None`
    /// when the translator gave none or the request was dropped). A line that
    /// waits for it is woken.
    pub fn finish(&self, source: Source, generation: u64, answer: Option<String>) {
        let mut state = locked(&self.state);
        if let Some(tracked) = state.tracked_mut(source, generation) {
            tracked.phase = Phase::Done(answer);
        }
        drop(state);
        self.finished.notify_all();
    }

    /// The answer of the text in progress that the `final` `id` of `source`
    /// closed ([`Mailbox::close`]), waiting for it if the request still runs.
    /// `None` when there is none to take: the line asks T itself.
    pub fn claim(&self, source: Source, id: &str) -> Option<Reuse> {
        let started = Instant::now();
        let mut state = locked(&self.state);
        let at = state.position(source, id)?;
        let asked = match state.taken[at].tracked.phase {
            Phase::Done(_) => ReuseState::Done,
            Phase::Waiting | Phase::Running => ReuseState::Running,
        };
        loop {
            let at = state.position(source, id)?;
            if matches!(state.taken[at].tracked.phase, Phase::Done(_)) {
                break;
            }
            let left = CLAIM_WAIT_MAX.saturating_sub(started.elapsed());
            if left.is_zero() {
                state.taken.remove(at);
                return None;
            }
            state = waited_on(&self.finished, state, left);
        }
        let at = state.position(source, id)?;
        let Taken {
            matched, tracked, ..
        } = state.taken.remove(at);
        let Phase::Done(Some(answer)) = tracked.phase else {
            return None;
        };
        Some(Reuse {
            answer,
            matched,
            state: asked,
            waited: started.elapsed(),
        })
    }

    /// Whether the translation `text` of `id` may be shown for `source`
    /// without taking away characters already drawn in the settled form.
    /// A text that may be shown counts as drawn.
    pub fn admit(&self, source: Source, id: &str, text: &str) -> bool {
        locked(&self.state).drawn[source.index()].admit(id, text)
    }

    /// Waits for a text and takes the one that was left first, with the means
    /// to tell whether it is still wanted.
    pub fn take(&self) -> (Job, Wanted) {
        let mut state = locked(&self.state);
        loop {
            if let Some(slot) = oldest(&state.pending)
                && let Some(pending) = state.pending[slot].take()
            {
                let counter = Arc::clone(&self.generations[slot]);
                let wanted = Wanted::pinned(counter, pending.generation, pending.pin);
                return (pending.job, wanted);
            }
            state = match self.wake.wait(state) {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
        }
    }
}

impl Default for Mailbox {
    fn default() -> Self {
        Mailbox::new()
    }
}

/// The slot whose waiting text was left first.
fn oldest(pending: &[Option<Pending>; SOURCES]) -> Option<usize> {
    pending
        .iter()
        .enumerate()
        .filter_map(|(slot, waiting)| waiting.as_ref().map(|found| (found.order, slot)))
        .min()
        .map(|(_, slot)| slot)
}

/// Whether the text has a Japanese character: kana or an ideograph.
fn has_japanese(text: &str) -> bool {
    text.chars()
        .any(|c| matches!(c, '\u{3040}'..='\u{30ff}' | '\u{4e00}'..='\u{9fff}'))
}

/// The language of a text in progress, read off its characters, when the
/// source translates that language.
pub fn partial_language(source: Source, text: &str) -> Option<Lang> {
    let spoken = if has_japanese(text) {
        Lang::Ja
    } else if text.chars().any(|c| c.is_ascii_alphabetic()) {
        Lang::En
    } else {
        return None;
    };
    source.translates(spoken).then_some(spoken)
}

/// The line a stream will send next, as far as the last `final` tells.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Next {
    source: Source,
    number: u64,
}

impl Next {
    /// The line after `line`, when its `id` is `<source>-<number>`.
    fn after(line: &Line) -> Option<Next> {
        let number = line
            .id
            .strip_prefix(line.source.as_str())?
            .strip_prefix('-')?
            .parse::<u64>()
            .ok()?;
        Some(Next {
            source: line.source,
            number: number.checked_add(1)?,
        })
    }

    fn id(&self) -> String {
        format!("{}-{}", self.source.as_str(), self.number)
    }
}

/// Why a text in progress is not translated. The name goes into the log
/// event as `reason`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Unusable {
    /// No `final` has come on the stream, so the source and the `id` are unknown.
    NoSource,
    /// The source does not translate this language.
    Language,
}

impl Unusable {
    fn as_str(self) -> &'static str {
        match self {
            Unusable::NoSource => "no_source",
            Unusable::Language => "language",
        }
    }
}

/// What one stream tells about the text in progress, and where it goes.
pub struct Route {
    mailbox: Option<Arc<Mailbox>>,
    next: Option<Next>,
}

impl Route {
    /// A route into `mailbox`; with none, every text is left alone.
    pub fn new(mailbox: Option<Arc<Mailbox>>) -> Route {
        Route {
            mailbox,
            next: None,
        }
    }

    /// A line came. A `final` ends the utterance in progress and says what
    /// the next line will be called.
    pub fn note_line(&mut self, line: &Line) {
        let Some(mailbox) = &self.mailbox else { return };
        if line.kind != LineKind::Final {
            return;
        }
        mailbox.close(line);
        self.next = Next::after(line);
    }

    /// A new session began: the lines may be numbered from the start again.
    pub fn forget(&mut self) {
        self.next = None;
    }

    /// The text of the utterance in progress came.
    pub fn partial(&mut self, text: &str, sink: &mut dyn LogSink) {
        let Some(mailbox) = &self.mailbox else { return };
        match self.job_for(text) {
            Ok(job) => mailbox.post(job),
            Err(reason) => sink.emit(
                LogEvent::new(Level::Debug, "translate.partial.skipped", "service")
                    .with("reason", reason.as_str()),
            ),
        }
    }

    fn job_for(&self, text: &str) -> Result<Job, Unusable> {
        let next = self.next.as_ref().ok_or(Unusable::NoSource)?;
        let src_lang = partial_language(next.source, text).ok_or(Unusable::Language)?;
        Ok(Job {
            source: next.source,
            id: next.id(),
            src_lang,
            text: text.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::MemorySink;
    use std::time::{Duration, Instant};

    fn job(source: Source, id: &str, text: &str) -> Job {
        Job {
            source,
            id: id.to_owned(),
            src_lang: Lang::Ja,
            text: text.to_owned(),
        }
    }

    #[test]
    fn the_common_prefix_is_cut_at_a_character() {
        assert_eq!(
            common_prefix("予算は五百万", "予算は五百万円"),
            "予算は五百万"
        );
        assert_eq!(common_prefix("予算は五", "予算が五"), "予算");
        assert_eq!(common_prefix("abc", "xyz"), "");
        assert_eq!(common_prefix("", "abc"), "");
        assert_eq!(common_prefix("ab", "a"), "a");
    }

    #[test]
    fn a_text_that_keeps_the_committed_prefix_is_admitted() {
        let mut drawn = Drawn::default();
        assert!(drawn.admit("mic-2", "The budget is"));
        assert!(drawn.admit("mic-2", "The budget is five"));
        assert_eq!(drawn.committed, "The budget is");
        assert!(drawn.admit("mic-2", "The budget is five million"));
    }

    #[test]
    fn a_text_that_drops_the_committed_prefix_is_refused_and_changes_nothing() {
        let mut drawn = Drawn::default();
        assert!(drawn.admit("mic-2", "The budget is"));
        assert!(drawn.admit("mic-2", "The budget is five"));
        assert!(!drawn.admit("mic-2", "A budget of five"));
        assert_eq!(drawn.text, "The budget is five");
        assert!(drawn.admit("mic-2", "The budget is five yen"));
    }

    #[test]
    fn before_a_second_text_nothing_is_committed_so_any_text_is_admitted() {
        let mut drawn = Drawn::default();
        assert!(drawn.admit("mic-2", "The budget"));
        assert!(drawn.admit("mic-2", "A plan"));
        assert_eq!(drawn.committed, "");
    }

    #[test]
    fn a_new_id_starts_over() {
        let mut drawn = Drawn::default();
        assert!(drawn.admit("mic-2", "The budget"));
        assert!(drawn.admit("mic-2", "The budget is"));
        assert!(drawn.admit("mic-3", "Next"));
        assert_eq!(drawn.committed, "");
    }

    #[test]
    fn retiring_a_source_forgets_what_was_drawn() {
        let mailbox = Mailbox::new();
        assert!(mailbox.admit(Source::Mic, "mic-2", "The budget"));
        assert!(mailbox.admit(Source::Mic, "mic-2", "The budget is"));
        assert!(!mailbox.admit(Source::Mic, "mic-2", "A plan"));
        mailbox.retire(Source::Mic);
        assert!(mailbox.admit(Source::Mic, "mic-2", "A plan"));
    }

    #[test]
    fn sources_do_not_share_what_was_drawn() {
        let mailbox = Mailbox::new();
        assert!(mailbox.admit(Source::Mic, "mic-2", "The budget"));
        assert!(mailbox.admit(Source::Mic, "mic-2", "The budget is"));
        assert!(mailbox.admit(Source::Loopback, "loopback-2", "A plan"));
    }

    fn line(kind: LineKind, id: &str, source: Source) -> Line {
        Line {
            kind,
            id: id.to_owned(),
            source,
            lang: Some(Lang::Ja),
            text: String::from("あ"),
            span: None,
        }
    }

    #[test]
    fn a_text_is_read_as_the_language_its_characters_are() {
        assert_eq!(partial_language(Source::File, "こんにち"), Some(Lang::Ja));
        assert_eq!(partial_language(Source::File, "東京"), Some(Lang::Ja));
        assert_eq!(partial_language(Source::File, "Hello wor"), Some(Lang::En));
        assert_eq!(partial_language(Source::File, "ーー"), Some(Lang::Ja));
    }

    #[test]
    fn a_text_with_no_letters_is_no_language() {
        assert_eq!(partial_language(Source::File, "123 ..."), None);
        assert_eq!(partial_language(Source::File, ""), None);
    }

    #[test]
    fn a_source_translates_only_its_own_language() {
        assert_eq!(partial_language(Source::Mic, "Hello"), None);
        assert_eq!(partial_language(Source::Mic, "こん"), Some(Lang::Ja));
        assert_eq!(partial_language(Source::Loopback, "こん"), None);
        assert_eq!(partial_language(Source::Loopback, "Hello"), Some(Lang::En));
    }

    #[test]
    fn the_next_id_follows_the_one_of_the_final() {
        let next = Next::after(&line(LineKind::Final, "mic-4", Source::Mic));
        assert_eq!(next.map(|found| found.id()), Some(String::from("mic-5")));
    }

    #[test]
    fn an_id_that_is_not_source_and_number_names_no_next_line() {
        for id in ["mic4", "loopback-4", "mic-x", "mic-", "-4", ""] {
            assert_eq!(Next::after(&line(LineKind::Final, id, Source::Mic)), None);
        }
        let last = format!("mic-{}", u64::MAX);
        assert_eq!(
            Next::after(&line(LineKind::Final, &last, Source::Mic)),
            None
        );
    }

    #[test]
    fn a_newer_text_takes_the_place_of_the_older_one_of_its_source() {
        let mailbox = Mailbox::new();
        mailbox.post(job(Source::Mic, "mic-1", "あ"));
        mailbox.post(job(Source::Mic, "mic-1", "あい"));
        let (taken, wanted) = mailbox.take();
        assert_eq!(taken.text, "あい");
        assert!(wanted.still());
    }

    #[test]
    fn a_newer_text_makes_the_request_for_the_older_one_stop() {
        let mailbox = Mailbox::new();
        mailbox.post(job(Source::Mic, "mic-1", "あ"));
        let (_, wanted) = mailbox.take();
        assert!(wanted.still());
        mailbox.post(job(Source::Mic, "mic-1", "あい"));
        assert!(!wanted.still());
    }

    #[test]
    fn a_text_of_another_source_does_not_stop_the_request() {
        let mailbox = Mailbox::new();
        mailbox.post(job(Source::Mic, "mic-1", "あ"));
        let (_, wanted) = mailbox.take();
        mailbox.post(job(Source::Loopback, "loopback-1", "Hi"));
        assert!(wanted.still());
    }

    #[test]
    fn the_same_text_again_asks_nothing_new() {
        let mailbox = Mailbox::new();
        mailbox.post(job(Source::Mic, "mic-1", "あ"));
        let (_, wanted) = mailbox.take();
        mailbox.post(job(Source::Mic, "mic-1", "あ"));
        assert!(wanted.still());
    }

    #[test]
    fn the_end_of_the_utterance_stops_the_request_and_drops_the_waiting_text() {
        let mailbox = Mailbox::new();
        mailbox.post(job(Source::Mic, "mic-1", "あ"));
        let (_, running) = mailbox.take();
        mailbox.post(job(Source::Mic, "mic-1", "あい"));
        mailbox.retire(Source::Mic);
        assert!(!running.still());
        mailbox.post(job(Source::Loopback, "loopback-1", "Hi"));
        let (taken, _) = mailbox.take();
        assert_eq!(taken.source, Source::Loopback);
    }

    #[test]
    fn after_the_end_of_an_utterance_its_last_text_is_new_again() {
        let mailbox = Mailbox::new();
        mailbox.post(job(Source::Mic, "mic-1", "あ"));
        let _ = mailbox.take();
        mailbox.retire(Source::Mic);
        mailbox.post(job(Source::Mic, "mic-2", "あ"));
        let (taken, wanted) = mailbox.take();
        assert_eq!((taken.id.as_str(), wanted.still()), ("mic-2", true));
    }

    #[test]
    fn the_text_left_first_is_taken_first() {
        let mailbox = Mailbox::new();
        mailbox.post(job(Source::Loopback, "loopback-1", "Hi"));
        mailbox.post(job(Source::Mic, "mic-1", "あ"));
        let sources = [mailbox.take().0.source, mailbox.take().0.source];
        assert_eq!(sources, [Source::Loopback, Source::Mic]);
    }

    #[test]
    fn a_worker_waiting_for_a_text_wakes_when_one_is_left() -> Result<(), String> {
        let mailbox = Arc::new(Mailbox::new());
        let waiting = Arc::clone(&mailbox);
        let worker = std::thread::spawn(move || waiting.take().0.text);
        std::thread::sleep(Duration::from_millis(50));
        mailbox.post(job(Source::Mic, "mic-1", "あ"));
        let started = Instant::now();
        let text = worker.join().map_err(|_| String::from("worker panicked"))?;
        assert_eq!(text, "あ");
        assert!(started.elapsed() < Duration::from_secs(2));
        Ok(())
    }

    fn route() -> (Route, Arc<Mailbox>) {
        let mailbox = Arc::new(Mailbox::new());
        (Route::new(Some(Arc::clone(&mailbox))), mailbox)
    }

    #[test]
    fn nothing_is_translated_before_a_final_has_come() {
        let (mut route, mailbox) = route();
        let mut sink = MemorySink::default();
        route.partial("こんにち", &mut sink);
        assert_eq!(sink.events.len(), 1);
        assert_eq!(sink.events[0].event, "translate.partial.skipped");
        assert_eq!(sink.events[0].fields["reason"], "no_source");
        mailbox.post(job(Source::Mic, "probe", "x"));
        assert_eq!(mailbox.take().0.id, "probe");
    }

    #[test]
    fn a_text_after_a_final_is_left_under_the_next_id() {
        let (mut route, mailbox) = route();
        route.note_line(&line(LineKind::Final, "mic-7", Source::Mic));
        route.partial("こんにち", &mut MemorySink::default());
        let (taken, _) = mailbox.take();
        assert_eq!(
            (taken.id.as_str(), taken.source, taken.src_lang),
            ("mic-8", Source::Mic, Lang::Ja)
        );
    }

    #[test]
    fn a_final_stops_the_text_of_its_own_utterance() {
        let (mut route, mailbox) = route();
        route.note_line(&line(LineKind::Final, "mic-1", Source::Mic));
        route.partial("こんにち", &mut MemorySink::default());
        let (_, wanted) = mailbox.take();
        route.note_line(&line(LineKind::Final, "mic-2", Source::Mic));
        assert!(!wanted.still());
    }

    #[test]
    fn a_refine_neither_stops_a_text_nor_moves_the_numbering() {
        let (mut route, mailbox) = route();
        route.note_line(&line(LineKind::Final, "mic-1", Source::Mic));
        route.partial("こんにち", &mut MemorySink::default());
        let (_, wanted) = mailbox.take();
        route.note_line(&line(LineKind::Refine, "mic-9", Source::Mic));
        assert!(wanted.still());
        route.partial("こんにちは", &mut MemorySink::default());
        assert_eq!(mailbox.take().0.id, "mic-2");
    }

    #[test]
    fn a_new_session_forgets_the_numbering() {
        let (mut route, _mailbox) = route();
        route.note_line(&line(LineKind::Final, "mic-1", Source::Mic));
        route.forget();
        let mut sink = MemorySink::default();
        route.partial("こんにち", &mut sink);
        assert_eq!(sink.events[0].fields["reason"], "no_source");
    }

    #[test]
    fn a_language_the_source_does_not_translate_is_skipped_with_its_reason() {
        let (mut route, _mailbox) = route();
        route.note_line(&line(LineKind::Final, "mic-1", Source::Mic));
        let mut sink = MemorySink::default();
        route.partial("Hello", &mut sink);
        assert_eq!(sink.events[0].fields["reason"], "language");
    }

    #[test]
    fn a_route_into_no_mailbox_leaves_every_text_alone() {
        let mut route = Route::new(None);
        let mut sink = MemorySink::default();
        route.note_line(&line(LineKind::Final, "mic-1", Source::Mic));
        route.partial("こんにち", &mut sink);
        assert!(sink.events.is_empty());
    }

    fn final_line(id: &str, text: &str) -> Line {
        Line {
            text: text.to_owned(),
            ..line(LineKind::Final, id, Source::Mic)
        }
    }

    /// A mailbox whose worker has taken the text `text` of `mic-2` and sends it.
    fn running(text: &str) -> (Mailbox, Wanted) {
        let (mailbox, wanted) = taken(text);
        mailbox.begin(Source::Mic, wanted.generation());
        (mailbox, wanted)
    }

    /// A mailbox whose worker has taken the text but holds no slot yet.
    fn taken(text: &str) -> (Mailbox, Wanted) {
        let mailbox = Mailbox::new();
        mailbox.post(job(Source::Mic, "mic-2", text));
        let (_, wanted) = mailbox.take();
        (mailbox, wanted)
    }

    #[test]
    fn a_final_before_the_worker_holds_a_slot_stops_the_request() {
        let (mailbox, wanted) = taken("あい");
        mailbox.close(&final_line("mic-2", "あい。"));
        assert!(!wanted.still());
        mailbox.begin(Source::Mic, wanted.generation());
        assert!(mailbox.claim(Source::Mic, "mic-2").is_none());
    }

    #[test]
    fn a_final_that_only_closes_the_text_keeps_its_request_and_takes_its_answer() {
        let (mailbox, wanted) = running("あい");
        mailbox.close(&final_line("mic-2", "あい。"));
        assert!(wanted.still() && !wanted.newest());
        mailbox.finish(Source::Mic, wanted.generation(), Some(String::from("ai")));
        let reuse = mailbox.claim(Source::Mic, "mic-2");
        assert!(reuse.as_ref().is_some_and(|reuse| {
            reuse.answer == "ai"
                && reuse.matched == ReuseMatch::ClosingMark
                && reuse.state == ReuseState::Done
        }));
    }

    #[test]
    fn the_same_text_is_matched_as_the_same() {
        let (mailbox, wanted) = running("あい");
        mailbox.close(&final_line("mic-2", "あい"));
        mailbox.finish(Source::Mic, wanted.generation(), Some(String::from("ai")));
        let reuse = mailbox.claim(Source::Mic, "mic-2");
        assert!(reuse.is_some_and(|reuse| reuse.matched == ReuseMatch::Same));
    }

    #[test]
    fn a_text_a_worker_has_not_taken_is_dropped_by_its_final() {
        let mailbox = Mailbox::new();
        mailbox.post(job(Source::Mic, "mic-2", "あい"));
        mailbox.close(&final_line("mic-2", "あい。"));
        assert!(mailbox.claim(Source::Mic, "mic-2").is_none());
        mailbox.post(job(Source::Loopback, "loopback-1", "Hi"));
        assert_eq!(mailbox.take().0.source, Source::Loopback);
    }

    #[test]
    fn a_final_of_other_text_or_another_id_stops_the_request() {
        for closing in [
            final_line("mic-2", "あいう。"),
            final_line("mic-2", "あい。。"),
            final_line("mic-2", "あい？"),
            final_line("mic-2", "あい、"),
            final_line("mic-3", "あい。"),
        ] {
            let (mailbox, wanted) = running("あい");
            mailbox.close(&closing);
            assert!(!wanted.still(), "{} {}", closing.id, closing.text);
            mailbox.finish(Source::Mic, wanted.generation(), Some(String::from("ai")));
            assert!(mailbox.claim(Source::Mic, &closing.id).is_none());
        }
    }

    #[test]
    fn a_request_that_ended_without_an_answer_claims_nothing() {
        let (mailbox, wanted) = running("あい");
        mailbox.close(&final_line("mic-2", "あい。"));
        mailbox.finish(Source::Mic, wanted.generation(), None);
        assert!(mailbox.claim(Source::Mic, "mic-2").is_none());
    }

    #[test]
    fn a_claim_is_given_once() {
        let (mailbox, wanted) = running("あい");
        mailbox.close(&final_line("mic-2", "あい。"));
        mailbox.finish(Source::Mic, wanted.generation(), Some(String::from("ai")));
        assert!(mailbox.claim(Source::Mic, "mic-2").is_some());
        assert!(mailbox.claim(Source::Mic, "mic-2").is_none());
    }

    #[test]
    fn a_claim_waits_for_the_request_that_still_runs() -> Result<(), String> {
        let (mailbox, wanted) = running("あい");
        let mailbox = Arc::new(mailbox);
        mailbox.close(&final_line("mic-2", "あい。"));
        let waiting = Arc::clone(&mailbox);
        let claimer = std::thread::spawn(move || waiting.claim(Source::Mic, "mic-2"));
        std::thread::sleep(Duration::from_millis(100));
        mailbox.finish(Source::Mic, wanted.generation(), Some(String::from("ai")));
        let reuse = claimer
            .join()
            .map_err(|_| String::from("claimer panicked"))?;
        let Some(reuse) = reuse else {
            return Err(String::from("nothing was claimed"));
        };
        assert_eq!(
            (reuse.answer.as_str(), reuse.state),
            ("ai", ReuseState::Running)
        );
        assert!(
            reuse.waited >= Duration::from_millis(80),
            "{:?}",
            reuse.waited
        );
        Ok(())
    }

    #[test]
    fn the_next_text_does_not_stop_a_claimed_request() {
        let (mailbox, wanted) = running("あい");
        mailbox.close(&final_line("mic-2", "あい。"));
        mailbox.post(job(Source::Mic, "mic-3", "う"));
        assert!(wanted.still());
        mailbox.finish(Source::Mic, wanted.generation(), Some(String::from("ai")));
        assert!(mailbox.claim(Source::Mic, "mic-2").is_some());
        assert_eq!(mailbox.take().0.text, "う");
    }

    #[test]
    fn a_final_of_another_source_leaves_the_request_of_this_one() {
        let (mailbox, wanted) = running("あい");
        let loopback = Line {
            source: Source::Loopback,
            ..final_line("mic-2", "あい。")
        };
        mailbox.close(&loopback);
        assert!(wanted.still());
        assert!(mailbox.claim(Source::Loopback, "mic-2").is_none());
    }

    #[test]
    fn the_claims_kept_for_lanes_that_never_come_are_bounded() {
        let mailbox = Mailbox::new();
        for number in 0..TAKEN_MAX + 3 {
            let id = format!("mic-{number}");
            mailbox.post(job(Source::Mic, &id, &format!("あ{number}")));
            let (_, wanted) = mailbox.take();
            mailbox.begin(Source::Mic, wanted.generation());
            mailbox.close(&final_line(&id, &format!("あ{number}。")));
            mailbox.finish(Source::Mic, wanted.generation(), Some(String::from("a")));
        }
        assert!(mailbox.claim(Source::Mic, "mic-0").is_none());
        assert!(
            mailbox
                .claim(Source::Mic, &format!("mic-{}", TAKEN_MAX + 2))
                .is_some()
        );
    }

    #[test]
    fn a_job_makes_a_partial_request_under_its_own_id() {
        let made = job(Source::Loopback, "loopback-3", "Hi");
        let request = made.request(5);
        assert_eq!(
            (request.id, request.of, request.source, request.now_epoch),
            ("loopback-3", LineKind::Partial, "loopback", 5)
        );
    }
}

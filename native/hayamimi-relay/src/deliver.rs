//! What leaves the relay besides its event stream: the settled translation of
//! each `final` line goes to a subtitle file (SRT) and to the outside
//! destinations (Zoom, Teams), once per line.
//!
//! Only a settled translation is delivered. A caption that has been sent
//! cannot be replaced on the other side, so a draft is never sent. The
//! outside destinations show one caption line to everyone, so they get the
//! translations into English only, and a held line gets nothing there.
//!
//! The courier works in a thread of its own, so a destination that is slow or
//! down never delays a translation. The SRT file and the screens always get
//! the line first. A destination that fails therefore costs nothing that is
//! already kept: the failure is said once on every channel (`sink.fallback.local`
//! in the log, a `fallback` event on the stream, a line for the user), the
//! next line tries again, and a send that goes through says it is over.

use std::io::Write;
use std::sync::mpsc::{self, Sender};

use crate::annotate::Annotation;
use crate::hub::Hub;
use crate::lang::Lang;
use crate::line::Line;
use crate::logging::{Level, LogEvent, LogSink, SharedSink};
use crate::pipeline::{LineKind, Status, Translation, sha256_hex};
use crate::srt::{Cue, SrtWriter};
use crate::strings::Strings;
use crate::verify::HoldReason;

/// The stage the screens are told about.
const STAGE: &str = "sink";
const FALLBACK_LOCAL: &str =
    r#"{"type":"fallback","stage":"sink","reason":"unreachable","to":"local"}"#;
const FALLBACK_OVER: &str = r#"{"type":"fallback","stage":"sink","to":null}"#;

/// The caption an outside destination is asked to show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Caption {
    pub id: String,
    /// The translation with its converted-value notes written in.
    pub text: String,
}

/// Why a destination did not take a caption. It names no text, address or
/// token, so it is safe to log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendError {
    /// The destination could not be reached, or did not answer in time.
    Unreachable,
    /// The destination answered with this status and did not take the caption.
    Refused(u16),
}

impl SendError {
    pub fn reason(self) -> &'static str {
        match self {
            SendError::Unreachable => "unreachable",
            SendError::Refused(_) => "refused",
        }
    }
}

/// An outside destination of captions.
pub trait Destination: Send {
    /// A stable name for the log, such as `zoom`.
    fn name(&self) -> &'static str;

    /// Whether the destination is set up to take captions right now. One that
    /// is not (a meeting has not given its token yet) is passed over without a
    /// word, as if it were not there. The courier asks just before each
    /// [`Destination::send`], so a destination whose setup changes while the
    /// relay runs is asked again for every caption.
    fn ready(&mut self) -> bool {
        true
    }

    /// Sends one caption and returns when it is taken or has failed. It gives
    /// up after a time limit of its own, because the courier waits for it.
    fn send(&mut self, caption: &Caption) -> Result<(), SendError>;
}

/// The words the outputs write or say, taken from the locale's table.
#[derive(Clone, Debug)]
pub struct Words {
    /// Heads the source text of a line whose translation was withheld.
    pub withheld_mark: String,
    /// Tells the user that a destination is down and what is kept.
    pub sink_local: String,
    /// Tells the user that the destinations are reachable again.
    pub sink_cleared: String,
}

impl Words {
    pub fn from_strings(strings: &Strings) -> Words {
        Words {
            withheld_mark: strings.withheld_mark.clone(),
            sink_local: strings.cli_text("sink_local").to_owned(),
            sink_cleared: strings.cli_text("sink_cleared").to_owned(),
        }
    }
}

/// Where the settled translations go.
pub struct Outputs {
    pub destinations: Vec<Box<dyn Destination>>,
    /// The subtitle file, when there is one.
    pub srt: Option<Box<dyn Write + Send>>,
    pub words: Words,
}

/// Writes each note at the character index it names: a note with `end` 3
/// goes after the third character. A note past the end goes last. Notes with
/// the same index keep their order.
pub fn insert_notes(text: &str, notes: &[(usize, &str)]) -> String {
    let mut ordered: Vec<(usize, &str)> = notes.to_vec();
    ordered.sort_by_key(|(end, _)| *end);
    let mut pending = ordered.into_iter().peekable();
    let mut out = String::with_capacity(text.len());
    for (index, character) in text.chars().enumerate() {
        while let Some((_, note)) = pending.next_if(|(end, _)| *end <= index) {
            out.push_str(note);
        }
        out.push(character);
    }
    for (_, note) in pending {
        out.push_str(note);
    }
    out
}

/// The text of a translation with its annotations written in.
pub fn with_annotations(text: &str, annotations: &[Annotation]) -> String {
    let notes: Vec<(usize, &str)> = annotations
        .iter()
        .map(|note| (note.end, note.text.as_str()))
        .collect();
    insert_notes(text, &notes)
}

/// Why a line has no cue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoCue {
    /// The line carries no span of speech, or one that runs backwards.
    NoTime,
    /// No translation was made, and there is nothing to write in its place.
    Unavailable,
}

impl NoCue {
    fn as_str(self) -> &'static str {
        match self {
            NoCue::NoTime => "no_time",
            NoCue::Unavailable => "unavailable",
        }
    }
}

/// The settled translation of one `final` line, with what the outputs need of
/// the line itself.
pub struct Parcel {
    source_text: String,
    span: Option<(f64, f64)>,
    translation: Translation,
}

impl Parcel {
    /// `None` unless `translation` is the settled translation of a `final`
    /// line. A `refine` line goes to the screens and the minutes, not here.
    pub fn new(line: &Line, translation: &Translation) -> Option<Parcel> {
        let delivered = translation.settled && translation.of == LineKind::Final;
        delivered.then(|| Parcel {
            source_text: line.text.clone(),
            span: line.span,
            translation: translation.clone(),
        })
    }

    pub fn id(&self) -> &str {
        &self.translation.id
    }

    /// The caption for the outside destinations: a translation into English
    /// that was not withheld.
    pub fn caption(&self) -> Option<Caption> {
        let translation = &self.translation;
        let wanted = translation.status == Status::Ok && translation.lang == Lang::En;
        wanted.then(|| Caption {
            id: translation.id.clone(),
            text: with_annotations(&translation.text, &translation.annotations),
        })
    }

    /// The cue for the subtitle file: the translation, or for a withheld line
    /// its source text under the mark.
    pub fn cue(&self, words: &Words) -> Result<Cue, NoCue> {
        let translation = &self.translation;
        let text = match (translation.status, translation.hold_reason) {
            (Status::Ok, _) => with_annotations(&translation.text, &translation.annotations),
            (Status::Held, Some(HoldReason::Unavailable)) => return Err(NoCue::Unavailable),
            (Status::Held, _) => format!("{} {}", words.withheld_mark, self.source_text),
        };
        let (start, end) = self.span.ok_or(NoCue::NoTime)?;
        Cue::new(start, end, &text).ok_or(NoCue::NoTime)
    }
}

struct Slot {
    destination: Box<dyn Destination>,
    down: bool,
}

/// The state of the courier thread.
struct Courier {
    slots: Vec<Slot>,
    srt: Option<SrtWriter>,
    words: Words,
    hub: Hub,
    sink: SharedSink,
}

impl Courier {
    fn new(outputs: Outputs, hub: Hub, sink: SharedSink) -> Courier {
        Courier {
            slots: outputs
                .destinations
                .into_iter()
                .map(|destination| Slot {
                    destination,
                    down: false,
                })
                .collect(),
            srt: outputs.srt.map(SrtWriter::new),
            words: outputs.words,
            hub,
            sink,
        }
    }

    fn deliver(&mut self, parcel: &Parcel) {
        self.write_cue(parcel);
        if let Some(caption) = parcel.caption() {
            for index in 0..self.slots.len() {
                self.send_to(index, &caption);
            }
        }
    }

    fn write_cue(&mut self, parcel: &Parcel) {
        if self.srt.is_none() {
            return;
        }
        let cue = match parcel.cue(&self.words) {
            Ok(cue) => cue,
            Err(skipped) => {
                self.sink.emit(
                    LogEvent::new(Level::Info, "srt.cue.skipped", "storage")
                        .with("id", parcel.id())
                        .with("reason", skipped.as_str()),
                );
                return;
            }
        };
        let written = self.srt.as_mut().map(|srt| srt.push(&cue));
        if matches!(written, Some(Err(_))) {
            // A file that cannot be written is left; the screens go on.
            self.srt = None;
            self.sink.emit(
                LogEvent::new(Level::Error, "srt.output.failed", "storage")
                    .with("id", parcel.id())
                    .with_error("SRT_WRITE"),
            );
        }
    }

    fn send_to(&mut self, index: usize, caption: &Caption) {
        if !self.slots[index].destination.ready() {
            // A destination that is not set up is not failing.
            self.mark_up(index);
            return;
        }
        let result = self.slots[index].destination.send(caption);
        let name = self.slots[index].destination.name();
        match result {
            Ok(()) => {
                self.sink.emit(
                    LogEvent::new(Level::Info, "translate.send.ok", "transport")
                        .with("id", caption.id.as_str())
                        .with("destination", name)
                        .with("text_sha256", sha256_hex(&caption.text)),
                );
                self.mark_up(index);
            }
            Err(error) => {
                self.sink.emit(failed_event(caption, name, error));
                self.mark_down(index, name);
            }
        }
    }

    fn any_down(&self) -> bool {
        self.slots.iter().any(|slot| slot.down)
    }

    fn mark_down(&mut self, index: usize, name: &str) {
        let first = !self.any_down();
        self.slots[index].down = true;
        if first {
            self.say_fallback(name);
        }
    }

    fn mark_up(&mut self, index: usize) {
        let was_down = self.slots[index].down;
        self.slots[index].down = false;
        if was_down && !self.any_down() {
            self.say_over();
        }
    }

    fn say_fallback(&mut self, name: &str) {
        let mut event = LogEvent::new(Level::Warn, "sink.fallback.local", "transport")
            .with("stage", STAGE)
            .with("reason", "unreachable")
            .with("destination", name);
        event.error_code = Some(String::from("HAYAMIMI_SINK_FALLBACK"));
        self.sink.emit(event);
        self.hub.publish_fallback(STAGE, true, FALLBACK_LOCAL);
        eprintln!("{}", self.words.sink_local);
    }

    fn say_over(&mut self) {
        self.sink.emit(
            LogEvent::new(Level::Info, "sink.fallback.cleared", "transport").with("stage", STAGE),
        );
        self.hub.publish_fallback(STAGE, false, FALLBACK_OVER);
        eprintln!("{}", self.words.sink_cleared);
    }
}

fn failed_event(caption: &Caption, name: &str, error: SendError) -> LogEvent {
    let event = LogEvent::new(Level::Warn, "translate.send.failed", "transport")
        .with("id", caption.id.as_str())
        .with("destination", name)
        .with("reason", error.reason());
    match error {
        SendError::Refused(status) => event.with("status", status),
        SendError::Unreachable => event,
    }
}

/// The handle the lanes hold: parcels go to the courier thread.
#[derive(Clone)]
pub struct Delivery {
    parcels: Sender<Parcel>,
    sink: SharedSink,
}

impl Delivery {
    /// Starts the courier thread. It ends with the process.
    pub fn start(outputs: Outputs, hub: Hub, sink: SharedSink) -> std::io::Result<Delivery> {
        let (parcels, incoming) = mpsc::channel();
        let mut courier = Courier::new(outputs, hub, sink.clone());
        std::thread::Builder::new()
            .name(String::from("relay-deliver"))
            .spawn(move || {
                for parcel in incoming {
                    courier.deliver(&parcel);
                }
            })?;
        Ok(Delivery { parcels, sink })
    }

    /// Hands over the settled translation of `line`. A translation that is
    /// not the settled one of a `final` line is not delivered.
    pub fn submit(&self, line: &Line, translation: &Translation) {
        let Some(parcel) = Parcel::new(line, translation) else {
            return;
        };
        if self.parcels.send(parcel).is_err() {
            // The courier thread ended; the line is on the screens all the same.
            self.sink.clone().emit(
                LogEvent::new(Level::Error, "translate.deliver.failed", "worker")
                    .with("id", line.id.as_str())
                    .with_error("DELIVER"),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotate::Annotation;
    use crate::line::Source;
    use crate::logging::Level as LogLevel;
    use crate::pipeline::Pass;
    use std::sync::{Arc, Mutex};

    fn note(end: usize, text: &str) -> Annotation {
        Annotation {
            start: 0,
            end,
            text: text.to_owned(),
        }
    }

    #[test]
    fn a_note_goes_after_the_character_its_end_names() {
        assert_eq!(insert_notes("abcdef", &[(3, "[x]")]), "abc[x]def");
        assert_eq!(insert_notes("abc", &[(0, "[x]")]), "[x]abc");
        assert_eq!(insert_notes("abc", &[(3, "[x]")]), "abc[x]");
        assert_eq!(insert_notes("abc", &[(9, "[x]")]), "abc[x]");
        assert_eq!(insert_notes("abc", &[]), "abc");
    }

    #[test]
    fn notes_are_written_in_whatever_order_they_come() {
        let notes = [(5, "[b]"), (2, "[a]")];
        assert_eq!(insert_notes("abcdefg", &notes), "ab[a]cde[b]fg");
        assert_eq!(insert_notes("ab", &[(1, "[a]"), (1, "[b]")]), "a[a][b]b");
    }

    #[test]
    fn the_index_counts_characters_and_not_bytes() {
        assert_eq!(
            insert_notes("私は100ドル", &[(5, "（約）")]),
            "私は100（約）ドル"
        );
        assert_eq!(
            with_annotations("私は100ドル払った。", &[note(7, "（約 15,900 円）")]),
            "私は100ドル（約 15,900 円）払った。"
        );
    }

    fn translation(of: LineKind, status: Status, lang: Lang, settled: bool) -> Translation {
        Translation {
            event_type: "translation",
            id: String::from("mic-1"),
            of,
            source: String::from("mic"),
            src_lang: lang.other(),
            lang,
            src_sha256: String::new(),
            status,
            hold_reason: (status == Status::Held).then_some(HoldReason::Number),
            stage: Pass::E,
            settled,
            text: if status == Status::Ok {
                String::from("Hello.")
            } else {
                String::new()
            },
            annotations: Vec::new(),
            skipped: Vec::new(),
        }
    }

    fn line(span: Option<(f64, f64)>) -> Line {
        Line {
            kind: LineKind::Final,
            id: String::from("mic-1"),
            source: Source::Mic,
            lang: Some(Lang::Ja),
            text: String::from("こんにちは。"),
            span,
        }
    }

    fn words() -> Words {
        Words {
            withheld_mark: String::from("〔訳を控えました〕"),
            sink_local: String::from("local"),
            sink_cleared: String::from("cleared"),
        }
    }

    #[test]
    fn only_the_settled_translation_of_a_final_line_is_a_parcel() {
        let made = |of, settled| {
            let found = translation(of, Status::Ok, Lang::En, settled);
            Parcel::new(&line(None), &found).is_some()
        };
        assert!(made(LineKind::Final, true));
        assert!(!made(LineKind::Final, false));
        assert!(!made(LineKind::Refine, true));
    }

    fn parcel_of(status: Status, lang: Lang) -> Option<Parcel> {
        let found = translation(LineKind::Final, status, lang, true);
        Parcel::new(&line(Some((0.5, 1.5))), &found)
    }

    #[test]
    fn the_outside_gets_translations_into_english_that_were_not_withheld() {
        let caption = parcel_of(Status::Ok, Lang::En).and_then(|parcel| parcel.caption());
        assert_eq!(
            caption,
            Some(Caption {
                id: String::from("mic-1"),
                text: String::from("Hello.")
            })
        );
        let into_japanese = parcel_of(Status::Ok, Lang::Ja).and_then(|parcel| parcel.caption());
        assert_eq!(into_japanese, None);
        let withheld = parcel_of(Status::Held, Lang::En).and_then(|parcel| parcel.caption());
        assert_eq!(withheld, None);
    }

    #[test]
    fn a_cue_holds_the_translation_or_the_marked_source() {
        let cue = |status, lang| parcel_of(status, lang).map(|parcel| parcel.cue(&words()));
        let text =
            |found: Option<Result<Cue, NoCue>>| found.and_then(Result::ok).map(|cue| cue.text);
        assert_eq!(
            text(cue(Status::Ok, Lang::En)),
            Some(String::from("Hello."))
        );
        assert_eq!(
            text(cue(Status::Held, Lang::En)),
            Some(String::from("〔訳を控えました〕 こんにちは。"))
        );
    }

    #[test]
    fn a_line_without_a_span_or_without_a_translation_has_no_cue() {
        let found = translation(LineKind::Final, Status::Ok, Lang::En, true);
        let no_time = Parcel::new(&line(None), &found).map(|parcel| parcel.cue(&words()));
        assert_eq!(
            no_time.map(|result| result.err()),
            Some(Some(NoCue::NoTime))
        );
        let mut unavailable = translation(LineKind::Final, Status::Held, Lang::En, true);
        unavailable.hold_reason = Some(HoldReason::Unavailable);
        let none =
            Parcel::new(&line(Some((0.0, 1.0))), &unavailable).map(|parcel| parcel.cue(&words()));
        assert_eq!(
            none.map(|result| result.err()),
            Some(Some(NoCue::Unavailable))
        );
    }

    // ---- the courier ----------------------------------------------------------

    /// Log events kept for the test to read.
    #[derive(Clone, Default)]
    struct Kept(Arc<Mutex<Vec<LogEvent>>>);

    impl LogSink for Kept {
        fn emit(&mut self, event: LogEvent) {
            match self.0.lock() {
                Ok(mut events) => events.push(event),
                Err(poisoned) => poisoned.into_inner().push(event),
            }
        }
    }

    impl Kept {
        fn named(&self, name: &str) -> usize {
            let events = match self.0.lock() {
                Ok(events) => events.clone(),
                Err(poisoned) => poisoned.into_inner().clone(),
            };
            events.iter().filter(|event| event.event == name).count()
        }
    }

    /// A destination whose reachability the test flips.
    struct Flaky {
        reachable: Arc<Mutex<bool>>,
        sent: Arc<Mutex<Vec<String>>>,
    }

    impl Destination for Flaky {
        fn name(&self) -> &'static str {
            "test"
        }

        fn send(&mut self, caption: &Caption) -> Result<(), SendError> {
            let up = match self.reachable.lock() {
                Ok(up) => *up,
                Err(poisoned) => *poisoned.into_inner(),
            };
            if !up {
                return Err(SendError::Unreachable);
            }
            match self.sent.lock() {
                Ok(mut sent) => sent.push(caption.text.clone()),
                Err(poisoned) => poisoned.into_inner().push(caption.text.clone()),
            }
            Ok(())
        }
    }

    struct Rig {
        courier: Courier,
        kept: Kept,
        reachable: Arc<Mutex<bool>>,
        sent: Arc<Mutex<Vec<String>>>,
        hub: Hub,
    }

    fn rig(up: bool) -> Rig {
        let (reachable, sent) = (Arc::new(Mutex::new(up)), Arc::new(Mutex::new(Vec::new())));
        let flaky = Flaky {
            reachable: Arc::clone(&reachable),
            sent: Arc::clone(&sent),
        };
        let (kept, hub) = (Kept::default(), Hub::new());
        let outputs = Outputs {
            destinations: vec![Box::new(flaky)],
            srt: None,
            words: words(),
        };
        let courier = Courier::new(outputs, hub.clone(), SharedSink::new(kept.clone()));
        Rig {
            courier,
            kept,
            reachable,
            sent,
            hub,
        }
    }

    impl Rig {
        fn deliver_a_line(&mut self) {
            if let Some(parcel) = parcel_of(Status::Ok, Lang::En) {
                self.courier.deliver(&parcel);
            }
        }

        fn set_reachable(&self, up: bool) {
            match self.reachable.lock() {
                Ok(mut now) => *now = up,
                Err(poisoned) => *poisoned.into_inner() = up,
            }
        }

        fn sent_count(&self) -> usize {
            match self.sent.lock() {
                Ok(sent) => sent.len(),
                Err(poisoned) => poisoned.into_inner().len(),
            }
        }

        /// The `fallback` events a client that connects now is told of.
        fn fallbacks_in_effect(&self) -> usize {
            self.hub.subscribe().1.len()
        }
    }

    #[test]
    fn a_reachable_destination_gets_each_line_and_says_nothing_of_a_fall() {
        let mut rig = rig(true);
        rig.deliver_a_line();
        rig.deliver_a_line();
        assert_eq!(rig.sent_count(), 2);
        assert_eq!(rig.kept.named("translate.send.ok"), 2);
        assert_eq!(rig.kept.named("sink.fallback.local"), 0);
        assert_eq!(rig.fallbacks_in_effect(), 0);
    }

    #[test]
    fn an_unreachable_destination_is_said_once_however_many_lines_fail() {
        let mut rig = rig(false);
        for _ in 0..3 {
            rig.deliver_a_line();
        }
        assert_eq!(rig.kept.named("translate.send.failed"), 3);
        assert_eq!(rig.kept.named("sink.fallback.local"), 1);
        assert_eq!(rig.fallbacks_in_effect(), 1);
        assert_eq!(rig.sent_count(), 0);
    }

    #[test]
    fn a_destination_that_comes_back_is_said_over_once_and_gets_the_next_line() {
        let mut rig = rig(false);
        rig.deliver_a_line();
        rig.set_reachable(true);
        rig.deliver_a_line();
        rig.deliver_a_line();
        assert_eq!(rig.sent_count(), 2);
        assert_eq!(rig.kept.named("sink.fallback.cleared"), 1);
        assert_eq!(rig.fallbacks_in_effect(), 0);
    }

    #[test]
    fn a_destination_that_falls_again_is_said_again() {
        let mut rig = rig(false);
        rig.deliver_a_line();
        rig.set_reachable(true);
        rig.deliver_a_line();
        rig.set_reachable(false);
        rig.deliver_a_line();
        assert_eq!(rig.kept.named("sink.fallback.local"), 2);
        assert_eq!(rig.kept.named("sink.fallback.cleared"), 1);
        assert_eq!(rig.fallbacks_in_effect(), 1);
    }

    #[test]
    fn the_log_of_a_send_names_the_line_and_never_its_text() {
        let mut rig = rig(true);
        rig.deliver_a_line();
        let events = match rig.kept.0.lock() {
            Ok(events) => events.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        let sent = events
            .iter()
            .find(|event| event.event == "translate.send.ok");
        assert_eq!(sent.map(|event| event.level), Some(LogLevel::Info));
        assert_eq!(
            sent.and_then(|event| event.fields.get("text_sha256"))
                .cloned(),
            Some(serde_json::Value::from(sha256_hex("Hello.")))
        );
        let lines: Vec<String> = events.iter().map(LogEvent::to_json_line).collect();
        assert!(
            lines.iter().all(|line| !line.contains("Hello.")),
            "{lines:?}"
        );
    }

    #[test]
    fn a_refused_send_names_the_status() {
        let caption = Caption {
            id: String::from("mic-1"),
            text: String::from("Hello."),
        };
        let event = failed_event(&caption, "zoom", SendError::Refused(401));
        assert_eq!(event.fields.get("status").cloned(), Some(401.into()));
        assert_eq!(event.fields.get("reason").cloned(), Some("refused".into()));
    }
}

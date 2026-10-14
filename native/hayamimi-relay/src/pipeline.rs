//! The three stages for one line: translate (T), edit (E), verify (V), then
//! the unit annotation. T and E each run once, in this order. V checks each of
//! the two translations once, so it runs at most twice per line, and a
//! translation that fails V is never shown. The annotation is added after V
//! and never shown to it.
//!
//! A `final` line is shown as early as it can be: when T's translation passes
//! V, it goes out at once (`stage: "t"`, `settled: false`) while E is still
//! being asked. The line is then settled exactly once: E's translation when it
//! passes V (`stage: "e"`), otherwise T's translation when that passed
//! (`stage: "t"`), otherwise a held line. Only the settled translation is
//! sent outside (`deliver`). A `refine` line has no screen that shows a draft,
//! so it is published once, settled.
//!
//! The text of an utterance still in progress (`partial`) goes through T and
//! V only ([`Pipeline::translate_partial`]). It is never settled, and it is
//! dropped when a newer one makes it useless.

use std::fmt::Write as _;
use std::ops::{ControlFlow, Deref};
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::annotate::{
    AnnotateRequest, Annotated, Annotation, Annotator, AnnotatorConfig, Skipped,
};
use crate::edit_answer::translation_of;
use crate::lang::Lang;
use crate::llm::{Completion, Llm, LlmError, Wanted};
use crate::logging::{Level, LogEvent, LogSink};
use crate::style::{Calques, JaStyle};
use crate::target::Target;
use crate::verify::{HoldReason, Verdict, Verifier, contains_eos};
use crate::watched::Watched;

/// Tokens a translation or an edit may use at most.
pub const MAX_TOKENS: u32 = 256;

/// Which event of the recognizer a line came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LineKind {
    Final,
    Refine,
    /// The text of an utterance that has not ended.
    Partial,
}

/// Whether the line may be shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Ok,
    Held,
}

/// The pass that made the text of a translation event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Pass {
    /// The translation of T, before E.
    T,
    /// The translation of E.
    E,
}

impl Pass {
    /// The wire name of the pass.
    pub fn as_str(self) -> &'static str {
        match self {
            Pass::T => "t",
            Pass::E => "e",
        }
    }
}

/// The `translation` event the relay sends to the screens.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Translation {
    #[serde(rename = "type")]
    pub event_type: &'static str,
    pub id: String,
    pub of: LineKind,
    pub source: String,
    pub src_lang: Lang,
    /// The ISO 639-1 code of the language the text is in: the other side of
    /// the live pair, or a language of `--translate`.
    pub lang: &'static str,
    /// The SHA-256 of the source text, as lowercase hex. A reader that holds
    /// the text of a `final` finds the translation of that exact text by it.
    pub src_sha256: String,
    pub status: Status,
    pub hold_reason: Option<HoldReason>,
    /// The pass the text comes from. A held line has no text; its pass is the
    /// one that decided to withhold it.
    pub stage: Pass,
    /// True on the one event that decides the translation of the line, and
    /// false on the draft that may be replaced before it.
    pub settled: bool,
    /// The translation, as the model wrote it; empty when held.
    pub text: String,
    pub annotations: Vec<Annotation>,
    pub skipped: Vec<Skipped>,
}

/// One recognized line.
pub struct LineRequest<'a> {
    pub id: &'a str,
    pub of: LineKind,
    /// `mic`, `loopback` or `file`.
    pub source: &'a str,
    pub src_lang: Lang,
    pub text: &'a str,
    pub now_epoch: i64,
}

/// The language a line is translated into.
#[derive(Clone, Copy)]
enum Dest<'a> {
    /// The other side of the live pair, with unit notes.
    Pair,
    /// A language of `--translate` outside the live pair, without notes.
    Other(&'a Target),
}

/// A line and where it is translated to. It reads as the line itself.
struct Ctx<'a> {
    request: &'a LineRequest<'a>,
    dest: Dest<'a>,
}

impl<'a> Deref for Ctx<'a> {
    type Target = LineRequest<'a>;

    fn deref(&self) -> &LineRequest<'a> {
        self.request
    }
}

impl Ctx<'_> {
    fn lang_code(&self) -> &'static str {
        match self.dest {
            Dest::Pair => self.request.src_lang.other().code(),
            Dest::Other(target) => target.code,
        }
    }
}

/// A stage of the pipeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Translate,
    /// V on the translation of T, which decides whether it is shown early.
    VerifyDraft,
    Edit,
    /// V on the translation of E.
    Verify,
    Annotate,
}

/// Watches the pipeline from the outside (tests, tracing). Every method is a
/// no-op unless overridden.
pub trait Observer {
    /// A stage is about to run.
    fn stage(&mut self, _stage: Stage) {}
    /// The exact translation text of E handed to V.
    fn verify_input(&mut self, _translation: &str) {}
    /// The exact translation text of T handed to V.
    fn draft_verify_input(&mut self, _translation: &str) {}
}

/// What came of translating the text of an utterance in progress.
#[derive(Debug, PartialEq)]
pub enum PartialOutcome {
    /// T's translation passed V: the event to show.
    Shown(Translation),
    /// The answer was not wanted any more; nothing is shown.
    Aborted,
    /// V withheld the translation; nothing is shown.
    Held(HoldReason),
    /// The translator did not answer; nothing is shown.
    Failed,
}

/// An observer that sees nothing.
pub struct NoObserver;

impl Observer for NoObserver {}

/// The SHA-256 of `text`'s UTF-8 bytes, as lowercase hex.
pub fn sha256_hex(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        // Writing to a String cannot fail.
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

pub(crate) fn language_name(lang: Lang) -> &'static str {
    match lang {
        Lang::Ja => "Japanese",
        Lang::En => "English",
    }
}

/// The prompt for T, in the form the model card recommends.
pub fn translate_prompt(src_lang: Lang, text: &str) -> String {
    translate_prompt_between(
        language_name(src_lang),
        language_name(src_lang.other()),
        text,
    )
}

/// The prompt for T into `target`, which may be any language of the table.
pub fn translate_prompt_to(src_lang: Lang, target: &Target, text: &str) -> String {
    translate_prompt_between(language_name(src_lang), target.name, text)
}

fn translate_prompt_between(from: &str, to: &str, text: &str) -> String {
    format!("Translate the following {from} text into {to}.\n\n{text}")
}

/// The first part of the prompt for E: what to do, in which languages.
fn edit_instruction(from: &str, to: &str) -> String {
    format!(
        "Below is a {from} sentence from a live conversation and a draft {to} translation.\n\
         Rewrite the draft so that it reads as natural spoken {to}: natural word order, \
         connectives and tone. Do not change facts, numbers, proper names, negation or \
         uncertainty. Output only the rewritten translation.\n"
    )
}

/// The last part of the prompt for E: the source, the draft, and where the
/// answer goes.
fn edit_material(from: &str, to: &str, text: &str, draft: &str) -> String {
    format!("\n{from}:\n{text}\n\nDraft {to}:\n{draft}\n\nRewritten {to}:\n")
}

/// The prompt for E into `target`. The style notes exist for Japanese and
/// English output only; into another language the instruction stands alone.
pub fn edit_prompt_to(
    src_lang: Lang,
    target: &Target,
    text: &str,
    draft: &str,
    calques: &Calques,
    ja_style: &JaStyle,
) -> String {
    if target.lang() == Some(src_lang.other()) {
        return edit_prompt(src_lang, text, draft, calques, ja_style);
    }
    let from = language_name(src_lang);
    let mut prompt = edit_instruction(from, target.name);
    prompt.push_str(&edit_material(from, target.name, text, draft));
    prompt
}

/// The prompt for E. It carries the source and the draft, and for Japanese
/// output the style note from `ja_style.toml`.
pub fn edit_prompt(
    src_lang: Lang,
    text: &str,
    draft: &str,
    calques: &Calques,
    ja_style: &JaStyle,
) -> String {
    let (from, to) = (language_name(src_lang), language_name(src_lang.other()));
    let mut prompt = edit_instruction(from, to);
    match src_lang.other() {
        Lang::Ja => {
            prompt.push_str(&format!("Style: {}\n", ja_style.note));
            for pattern in &ja_style.patterns {
                prompt.push_str(&format!(
                    "Avoid \"{}\"; write \"{}\".\n",
                    pattern.literal, pattern.natural
                ));
            }
        }
        Lang::En => {
            for pattern in &calques.patterns {
                prompt.push_str(&format!(
                    "Avoid \"{}\"; write \"{}\".\n",
                    pattern.literal, pattern.natural
                ));
            }
        }
    }
    prompt.push_str(&edit_material(from, to, text, draft));
    prompt
}

fn embedded_calques() -> Result<Calques, String> {
    Calques::embedded()
}

fn embedded_ja_style() -> Result<JaStyle, String> {
    JaStyle::embedded()
}

/// Where the pipeline reads its files from.
#[derive(Default)]
pub struct PipelineConfig {
    pub rates_path: Option<PathBuf>,
    pub tables_dir: Option<PathBuf>,
}

/// The pipeline: a translator and the tables behind V and the annotation.
pub struct Pipeline<L: Llm> {
    llm: L,
    verifier: Verifier,
    annotator: Annotator,
    calques: Watched<Calques>,
    ja_style: Watched<JaStyle>,
}

/// T's answer, and whether V let it through.
struct Draft {
    text: String,
    verdict: Verdict,
    /// Milliseconds spent so far in T, E and V, in that order.
    ms: [u128; 3],
}

impl Draft {
    fn passed(&self) -> bool {
        self.verdict == Verdict::Ok
    }
}

impl<L: Llm> Pipeline<L> {
    pub fn open(
        llm: L,
        config: &PipelineConfig,
        sink: &mut dyn LogSink,
    ) -> Result<Pipeline<L>, String> {
        let table_file = |name: &str| -> Option<PathBuf> {
            config
                .tables_dir
                .as_deref()
                .map(|dir: &Path| dir.join(name))
        };
        Ok(Pipeline {
            llm,
            verifier: Verifier::open(config.tables_dir.as_deref(), sink)?,
            annotator: Annotator::open(
                &AnnotatorConfig {
                    rates_path: config.rates_path.clone(),
                    tables_dir: config.tables_dir.clone(),
                },
                sink,
            )?,
            calques: Watched::open(
                "calques",
                table_file("calques.toml"),
                embedded_calques,
                Calques::parse,
                sink,
            )?,
            ja_style: Watched::open(
                "ja_style",
                table_file("ja_style.toml"),
                embedded_ja_style,
                JaStyle::parse,
                sink,
            )?,
        })
    }

    /// Runs one line through T, E, V and the annotation and returns the
    /// settled translation. No draft is handed out.
    pub fn run(
        &mut self,
        request: &LineRequest<'_>,
        observer: &mut dyn Observer,
        sink: &mut dyn LogSink,
    ) -> Translation {
        self.run_staged(request, observer, &mut |_| {}, sink)
    }

    /// Runs one line like [`Pipeline::run`] and hands `early` the draft of a
    /// `final` line as soon as T's translation has passed V, before E is asked.
    pub fn run_staged(
        &mut self,
        request: &LineRequest<'_>,
        observer: &mut dyn Observer,
        early: &mut dyn FnMut(Translation),
        sink: &mut dyn LogSink,
    ) -> Translation {
        let ctx = Ctx {
            request,
            dest: Dest::Pair,
        };
        self.run_to(&ctx, observer, early, sink)
    }

    /// Runs one line into `target`, a language of `--translate` outside the
    /// live pair, like [`Pipeline::run_staged`]. The translation carries the
    /// language's code in `lang` and no unit notes.
    pub fn run_into(
        &mut self,
        request: &LineRequest<'_>,
        target: &Target,
        early: &mut dyn FnMut(Translation),
        sink: &mut dyn LogSink,
    ) -> Translation {
        let ctx = Ctx {
            request,
            dest: Dest::Other(target),
        };
        self.run_to(&ctx, &mut NoObserver, early, sink)
    }

    fn run_to(
        &mut self,
        request: &Ctx<'_>,
        observer: &mut dyn Observer,
        early: &mut dyn FnMut(Translation),
        sink: &mut dyn LogSink,
    ) -> Translation {
        self.calques.refresh(sink);
        self.ja_style.refresh(sink);
        let started = Instant::now();
        let draft = match self.draft(request, observer, started, sink) {
            ControlFlow::Continue(draft) => draft,
            ControlFlow::Break(done) => return done,
        };
        if matches!(request.dest, Dest::Other(_)) {
            return self.settle_draft(request, draft, observer, sink);
        }
        if draft.passed() && request.of == LineKind::Final {
            early(self.draft_event(request, &draft.text));
        }
        self.edit(request, draft, observer, started, sink)
    }

    /// A language outside the live pair has no E: the model that writes it
    /// takes its language codes as a structure and cannot be handed an edit
    /// instruction. T's translation is the settled one when it passed V, and
    /// the line is withheld when it did not.
    fn settle_draft(
        &mut self,
        request: &Ctx<'_>,
        draft: Draft,
        observer: &mut dyn Observer,
        sink: &mut dyn LogSink,
    ) -> Translation {
        match draft.verdict {
            Verdict::Ok => self.settle(request, draft.text, Pass::T, draft.ms, observer, sink),
            Verdict::Held(reason) => self.held(request, reason, draft.ms, Pass::T, sink),
        }
    }

    /// Translates the text of an utterance in progress with T and checks it
    /// with V. The request is dropped when `wanted` says a newer text has
    /// taken this one's place; nothing is shown then, whatever it answered.
    pub fn translate_partial(
        &mut self,
        request: &LineRequest<'_>,
        wanted: &Wanted,
        sink: &mut dyn LogSink,
    ) -> PartialOutcome {
        let started = Instant::now();
        if !wanted.still() {
            return partial_aborted(request, started, sink);
        }
        let prompt = translate_prompt(request.src_lang, request.text);
        let answer = self
            .llm
            .complete(&Completion::new(&prompt, MAX_TOKENS).wanted(wanted));
        match answer {
            Ok(text) if wanted.still() => self.judge_partial(request, &text, started, sink),
            Ok(_) | Err(LlmError::Aborted) => partial_aborted(request, started, sink),
            Err(error) => partial_failed(request, &error, started, sink),
        }
    }

    /// V on T's answer for a text in progress.
    fn judge_partial(
        &mut self,
        request: &LineRequest<'_>,
        answer: &str,
        started: Instant,
        sink: &mut dyn LogSink,
    ) -> PartialOutcome {
        let text = answer.trim();
        let verdict = if contains_eos(answer) {
            Verdict::Held(HoldReason::Form)
        } else {
            self.verifier
                .verify(request.text, text, request.src_lang, sink)
        };
        match verdict {
            Verdict::Ok => {
                sink.emit(partial_log("translate.partial.ok", request, started));
                let ctx = Ctx {
                    request,
                    dest: Dest::Pair,
                };
                PartialOutcome::Shown(self.draft_event(&ctx, text))
            }
            Verdict::Held(reason) => {
                sink.emit(
                    partial_log("translate.partial.held", request, started)
                        .with("hold_reason", reason.as_str()),
                );
                PartialOutcome::Held(reason)
            }
        }
    }

    /// T and the first V. `Break` carries a line that is settled without E.
    fn draft(
        &mut self,
        request: &Ctx<'_>,
        observer: &mut dyn Observer,
        started: Instant,
        sink: &mut dyn LogSink,
    ) -> ControlFlow<Translation, Draft> {
        observer.stage(Stage::Translate);
        let prompt = translate_prompt(request.src_lang, request.text);
        let completion = match request.dest {
            Dest::Pair => Completion::new(&prompt, MAX_TOKENS),
            Dest::Other(target) => Completion::coded(
                request.text,
                request.src_lang.code(),
                target.code,
                MAX_TOKENS,
            ),
        };
        let answer = match self.llm.complete(&completion) {
            Ok(text) => text,
            Err(error) => {
                let line = self.unavailable(request, &error.to_string(), started, Pass::T, sink);
                return ControlFlow::Break(line);
            }
        };
        let t_ms = started.elapsed().as_millis();
        if contains_eos(&answer) {
            // `stop` did not take effect; the marker would reach E's prompt.
            let line = self.held(request, HoldReason::Form, [t_ms, 0, 0], Pass::T, sink);
            return ControlFlow::Break(line);
        }
        let text = answer.trim().to_owned();
        let verify_started = Instant::now();
        observer.stage(Stage::VerifyDraft);
        observer.draft_verify_input(&text);
        let verdict = self.verify(request, &text, sink);
        ControlFlow::Continue(Draft {
            text,
            verdict,
            ms: [t_ms, 0, verify_started.elapsed().as_millis()],
        })
    }

    /// V on `text`, the translation of `request` into its destination.
    fn verify(&mut self, request: &Ctx<'_>, text: &str, sink: &mut dyn LogSink) -> Verdict {
        match request.dest {
            Dest::Pair => self
                .verifier
                .verify(request.text, text, request.src_lang, sink),
            Dest::Other(target) => self.verifier.verify_into(
                request.id,
                request.text,
                text,
                request.src_lang,
                target,
                sink,
            ),
        }
    }

    /// The prompt for E with T's translation `draft`. Only a line of the
    /// live pair reaches E.
    fn edit_prompt_for(&self, request: &Ctx<'_>, draft: &str) -> String {
        let (calques, ja_style) = (self.calques.current(), self.ja_style.current());
        edit_prompt(request.src_lang, request.text, draft, calques, ja_style)
    }

    /// E, the second V, and the settled translation.
    fn edit(
        &mut self,
        request: &Ctx<'_>,
        draft: Draft,
        observer: &mut dyn Observer,
        started: Instant,
        sink: &mut dyn LogSink,
    ) -> Translation {
        let edit_started = Instant::now();
        observer.stage(Stage::Edit);
        let prompt = self.edit_prompt_for(request, &draft.text);
        let edited = match self.llm.complete(&Completion::new(&prompt, MAX_TOKENS)) {
            Ok(text) => text,
            Err(error) => {
                return self.edit_failed(
                    request,
                    draft,
                    &error.to_string(),
                    observer,
                    started,
                    sink,
                );
            }
        };
        let e_ms = edit_started.elapsed().as_millis();
        let text = translation_of(&edited, language_name(request.src_lang)).to_owned();
        let verify_started = Instant::now();
        observer.stage(Stage::Verify);
        observer.verify_input(&text);
        let verdict = self.verify(request, &text, sink);
        let v_ms = draft.ms[2] + verify_started.elapsed().as_millis();
        let ms = [draft.ms[0], e_ms, v_ms];
        match verdict {
            Verdict::Ok => self.settle(request, text, Pass::E, ms, observer, sink),
            // An answer that still carries the end marker means `stop` did not
            // take effect; the line is withheld, not rescued by the draft.
            Verdict::Held(reason) if draft.passed() && !contains_eos(&text) => {
                self.keep_draft(request, draft.text, reason, ms, observer, sink)
            }
            Verdict::Held(reason) => self.held(request, reason, ms, Pass::E, sink),
        }
    }

    /// E could not be asked. A draft that passed V stands; otherwise the line
    /// is unavailable.
    fn edit_failed(
        &mut self,
        request: &Ctx<'_>,
        draft: Draft,
        detail: &str,
        observer: &mut dyn Observer,
        started: Instant,
        sink: &mut dyn LogSink,
    ) -> Translation {
        if !draft.passed() {
            return self.unavailable(request, detail, started, Pass::E, sink);
        }
        self.log_request_failed(request, detail, started, sink);
        let ms = [draft.ms[0], 0, draft.ms[2]];
        self.settle(request, draft.text, Pass::T, ms, observer, sink)
    }

    /// E's translation failed V, so T's translation, which passed, is the one
    /// that stays.
    fn keep_draft(
        &mut self,
        request: &Ctx<'_>,
        text: String,
        reason: HoldReason,
        ms: [u128; 3],
        observer: &mut dyn Observer,
        sink: &mut dyn LogSink,
    ) -> Translation {
        sink.emit(
            LogEvent::new(Level::Info, "translate.edit.held", "service")
                .with("id", request.id)
                .with("source", request.source)
                .with("hold_reason", reason.as_str()),
        );
        self.settle(request, text, Pass::T, ms, observer, sink)
    }

    /// Annotates a translation that passed V and settles it.
    fn settle(
        &mut self,
        request: &Ctx<'_>,
        text: String,
        pass: Pass,
        ms: [u128; 3],
        observer: &mut dyn Observer,
        sink: &mut dyn LogSink,
    ) -> Translation {
        observer.stage(Stage::Annotate);
        let annotated = match request.dest {
            Dest::Pair => self.annotator.annotate(
                &AnnotateRequest {
                    id: request.id,
                    src: request.text,
                    translation: &text,
                    src_lang: request.src_lang,
                    now_epoch: request.now_epoch,
                },
                sink,
            ),
            // The unit notes are written for Japanese and English readers.
            Dest::Other(_) => Annotated::default(),
        };
        self.finish_ok(request, text, annotated, ms, pass, sink)
    }

    fn base(
        &self,
        request: &Ctx<'_>,
        status: Status,
        reason: Option<HoldReason>,
        pass: Pass,
        settled: bool,
    ) -> Translation {
        Translation {
            event_type: "translation",
            id: request.id.to_owned(),
            of: request.of,
            source: request.source.to_owned(),
            src_lang: request.src_lang,
            lang: request.lang_code(),
            src_sha256: sha256_hex(request.text),
            status,
            hold_reason: reason,
            stage: pass,
            settled,
            text: String::new(),
            annotations: Vec::new(),
            skipped: Vec::new(),
        }
    }

    /// The draft event: T's translation, before E, without a note.
    fn draft_event(&self, request: &Ctx<'_>, text: &str) -> Translation {
        let mut line = self.base(request, Status::Ok, None, Pass::T, false);
        line.text = text.to_owned();
        line
    }

    fn finish_ok(
        &self,
        request: &Ctx<'_>,
        text: String,
        annotated: Annotated,
        ms: [u128; 3],
        pass: Pass,
        sink: &mut dyn LogSink,
    ) -> Translation {
        sink.emit(
            timing(
                LogEvent::new(Level::Info, "translate.line.ok", "service"),
                request,
                ms,
            )
            .with("stage", pass.as_str()),
        );
        let mut line = self.base(request, Status::Ok, None, pass, true);
        line.text = text;
        line.annotations = annotated.annotations;
        line.skipped = annotated.skipped;
        line
    }

    fn held(
        &self,
        request: &Ctx<'_>,
        reason: HoldReason,
        ms: [u128; 3],
        pass: Pass,
        sink: &mut dyn LogSink,
    ) -> Translation {
        sink.emit(timing(
            LogEvent::new(Level::Info, "translate.line.held", "service")
                .with("hold_reason", reason.as_str()),
            request,
            ms,
        ));
        self.base(request, Status::Held, Some(reason), pass, true)
    }

    fn log_request_failed(
        &self,
        request: &Ctx<'_>,
        detail: &str,
        started: Instant,
        sink: &mut dyn LogSink,
    ) {
        let duration = u64::try_from(started.elapsed().as_millis()).ok();
        sink.emit(
            LogEvent::new(Level::Error, "translate.request.failed", "service")
                .with("id", request.id)
                .with("dependency", "llama-server")
                .with("duration_ms", duration)
                .with("detail", detail.split(':').next().map(str::to_owned))
                .with_error("LLM_UNAVAILABLE"),
        );
    }

    fn unavailable(
        &self,
        request: &Ctx<'_>,
        detail: &str,
        started: Instant,
        pass: Pass,
        sink: &mut dyn LogSink,
    ) -> Translation {
        self.log_request_failed(request, detail, started, sink);
        self.base(
            request,
            Status::Held,
            Some(HoldReason::Unavailable),
            pass,
            true,
        )
    }
}

fn timing(event: LogEvent, request: &Ctx<'_>, ms: [u128; 3]) -> LogEvent {
    let clip = |value: u128| u64::try_from(value).ok();
    let event = match request.dest {
        Dest::Pair => event,
        Dest::Other(target) => event.with("target", target.code),
    };
    event
        .with("id", request.id)
        .with("source", request.source)
        .with("t_ms", clip(ms[0]))
        .with("e_ms", clip(ms[1]))
        .with("v_ms", clip(ms[2]))
}

/// A log event of the text in progress. These come about twice a second, so
/// they are `debug`.
fn partial_log(name: &str, request: &LineRequest<'_>, started: Instant) -> LogEvent {
    LogEvent::new(Level::Debug, name, "service")
        .with("id", request.id)
        .with("source", request.source)
        .with(
            "duration_ms",
            u64::try_from(started.elapsed().as_millis()).ok(),
        )
}

fn partial_aborted(
    request: &LineRequest<'_>,
    started: Instant,
    sink: &mut dyn LogSink,
) -> PartialOutcome {
    sink.emit(partial_log("translate.partial.aborted", request, started));
    PartialOutcome::Aborted
}

fn partial_failed(
    request: &LineRequest<'_>,
    error: &LlmError,
    started: Instant,
    sink: &mut dyn LogSink,
) -> PartialOutcome {
    sink.emit(
        partial_log("translate.partial.failed", request, started)
            .with("dependency", "llama-server")
            .with(
                "detail",
                error.to_string().split(':').next().map(str::to_owned),
            )
            .with_error("LLM_UNAVAILABLE"),
    );
    PartialOutcome::Failed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::MemorySink;
    use std::cell::RefCell;

    struct Scripted {
        answers: RefCell<Vec<Result<String, LlmError>>>,
        prompts: RefCell<Vec<String>>,
    }

    impl Scripted {
        fn new(answers: Vec<Result<String, LlmError>>) -> Scripted {
            Scripted {
                answers: RefCell::new(answers),
                prompts: RefCell::new(Vec::new()),
            }
        }

        /// A translator that answers T and then E with the given texts.
        fn saying(t: &str, e: &str) -> Scripted {
            Scripted::new(vec![Ok(t.to_owned()), Ok(e.to_owned())])
        }
    }

    impl Llm for &Scripted {
        fn complete(&self, request: &Completion<'_>) -> Result<String, LlmError> {
            self.prompts.borrow_mut().push(request.prompt.to_owned());
            let mut answers = self.answers.borrow_mut();
            if answers.is_empty() {
                return Err(LlmError::Transport(String::from("no scripted answer")));
            }
            answers.remove(0)
        }
    }

    fn request(text: &str) -> LineRequest<'_> {
        LineRequest {
            id: "mic-1",
            of: LineKind::Final,
            source: "mic",
            src_lang: Lang::Ja,
            text,
            now_epoch: 1_790_000_000,
        }
    }

    /// What one run showed early, what it settled, and what it asked V.
    struct Trace {
        early: Vec<Translation>,
        settled: Translation,
        stages: Vec<Stage>,
    }

    #[derive(Default)]
    struct Stages(Vec<Stage>);

    impl Observer for Stages {
        fn stage(&mut self, stage: Stage) {
            self.0.push(stage);
        }
    }

    fn run_traced(
        script: &Scripted,
        request: &LineRequest<'_>,
    ) -> Result<Trace, Box<dyn std::error::Error>> {
        let mut sink = MemorySink::default();
        let mut pipeline = Pipeline::open(script, &PipelineConfig::default(), &mut sink)?;
        let mut early = Vec::new();
        let mut stages = Stages::default();
        let settled = pipeline.run_staged(
            request,
            &mut stages,
            &mut |draft| early.push(draft),
            &mut sink,
        );
        Ok(Trace {
            early,
            settled,
            stages: stages.0,
        })
    }

    #[test]
    fn the_translate_prompt_follows_the_model_card() {
        assert_eq!(
            translate_prompt(Lang::Ja, "こんにちは。"),
            "Translate the following Japanese text into English.\n\nこんにちは。"
        );
    }

    #[test]
    fn the_edit_prompt_carries_source_draft_and_the_japanese_style() -> Result<(), String> {
        let prompt = edit_prompt(
            Lang::En,
            "Hi.",
            "やあ。",
            &Calques::embedded()?,
            &JaStyle::embedded()?,
        );
        assert!(prompt.contains("Hi.") && prompt.contains("やあ。") && prompt.contains("Style:"));
        Ok(())
    }

    #[test]
    fn a_clean_line_is_ok() -> Result<(), Box<dyn std::error::Error>> {
        let script = Scripted::new(vec![
            Ok(String::from("Hello.")),
            Ok(String::from(" Hello. \n")),
        ]);
        let mut sink = MemorySink::default();
        let mut pipeline = Pipeline::open(&script, &PipelineConfig::default(), &mut sink)?;
        let line = pipeline.run(&request("こんにちは。"), &mut NoObserver, &mut sink);
        assert_eq!(line.status, Status::Ok);
        assert_eq!(line.text, "Hello.");
        assert_eq!(script.prompts.borrow().len(), 2);
        Ok(())
    }

    #[test]
    fn an_unreachable_translator_holds_the_line_as_unavailable()
    -> Result<(), Box<dyn std::error::Error>> {
        let script = Scripted::new(vec![]);
        let mut sink = MemorySink::default();
        let mut pipeline = Pipeline::open(&script, &PipelineConfig::default(), &mut sink)?;
        let line = pipeline.run(&request("こんにちは。"), &mut NoObserver, &mut sink);
        assert_eq!(line.hold_reason, Some(HoldReason::Unavailable));
        assert!(
            sink.events
                .iter()
                .any(|e| e.event == "translate.request.failed")
        );
        Ok(())
    }

    #[test]
    fn the_wire_form_names_the_event_type() -> Result<(), Box<dyn std::error::Error>> {
        let script = Scripted::new(vec![]);
        let mut sink = MemorySink::default();
        let mut pipeline = Pipeline::open(&script, &PipelineConfig::default(), &mut sink)?;
        let line = pipeline.run(&request("こんにちは。"), &mut NoObserver, &mut sink);
        let json = serde_json::to_value(&line)?;
        assert_eq!(json["type"], "translation");
        assert_eq!(json["status"], "held");
        assert_eq!(json["hold_reason"], "unavailable");
        assert_eq!(json["lang"], "en");
        assert_eq!(json["src_sha256"], sha256_hex("こんにちは。"));
        assert_eq!(json["stage"], "t");
        assert_eq!(json["settled"], true);
        Ok(())
    }

    #[test]
    fn the_digest_is_the_lowercase_hex_of_the_utf8_bytes() {
        assert_eq!(
            sha256_hex("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn every_outcome_names_the_text_it_translated() -> Result<(), Box<dyn std::error::Error>> {
        let script = Scripted::new(vec![Ok(String::from("Hello.")), Ok(String::from("Hello."))]);
        let mut sink = MemorySink::default();
        let mut pipeline = Pipeline::open(&script, &PipelineConfig::default(), &mut sink)?;
        let said = pipeline.run(&request("こんにちは。"), &mut NoObserver, &mut sink);
        let held = pipeline.run(&request("さようなら。"), &mut NoObserver, &mut sink);
        assert_eq!(said.status, Status::Ok);
        assert_eq!(held.hold_reason, Some(HoldReason::Unavailable));
        assert_eq!(said.src_sha256, sha256_hex("こんにちは。"));
        assert_eq!(held.src_sha256, sha256_hex("さようなら。"));
        Ok(())
    }

    #[test]
    fn a_draft_that_passes_is_shown_before_e_is_asked() -> Result<(), Box<dyn std::error::Error>> {
        let script = Scripted::saying("Hello.", "Hi there.");
        let trace = run_traced(&script, &request("こんにちは。"))?;
        assert_eq!(trace.early.len(), 1);
        let draft = &trace.early[0];
        assert_eq!((draft.stage, draft.settled), (Pass::T, false));
        assert_eq!((draft.status, draft.text.as_str()), (Status::Ok, "Hello."));
        assert!(draft.annotations.is_empty());
        assert_eq!(
            (trace.settled.stage, trace.settled.settled),
            (Pass::E, true)
        );
        assert_eq!(trace.settled.text, "Hi there.");
        // The draft left before the edit stage began.
        let order = [
            Stage::Translate,
            Stage::VerifyDraft,
            Stage::Edit,
            Stage::Verify,
            Stage::Annotate,
        ];
        assert_eq!(trace.stages, order);
        Ok(())
    }

    #[test]
    fn the_draft_is_handed_out_before_the_edit_request_is_made()
    -> Result<(), Box<dyn std::error::Error>> {
        let script = Scripted::saying("Hello.", "Hello.");
        let mut sink = MemorySink::default();
        let mut pipeline = Pipeline::open(&script, &PipelineConfig::default(), &mut sink)?;
        let mut asked_when_drafted = Vec::new();
        let line = request("こんにちは。");
        pipeline.run_staged(
            &line,
            &mut NoObserver,
            &mut |_| asked_when_drafted.push(script.prompts.borrow().len()),
            &mut sink,
        );
        assert_eq!(asked_when_drafted, [1]);
        assert_eq!(script.prompts.borrow().len(), 2);
        Ok(())
    }

    #[test]
    fn an_edit_that_fails_v_leaves_the_draft_as_the_settled_translation()
    -> Result<(), Box<dyn std::error::Error>> {
        let script = Scripted::saying("The budget is 500 yen.", "The budget is 900 yen.");
        let trace = run_traced(&script, &request("予算は500円です。"))?;
        assert_eq!(trace.early.len(), 1);
        let settled = &trace.settled;
        assert_eq!(
            (settled.status, settled.stage, settled.settled),
            (Status::Ok, Pass::T, true)
        );
        assert_eq!(settled.text, trace.early[0].text);
        Ok(())
    }

    #[test]
    fn a_draft_that_fails_v_is_not_shown_and_a_good_edit_settles_the_line()
    -> Result<(), Box<dyn std::error::Error>> {
        let script = Scripted::saying("The budget is 900 yen.", "The budget is 500 yen.");
        let trace = run_traced(&script, &request("予算は500円です。"))?;
        assert!(trace.early.is_empty());
        assert_eq!(
            (trace.settled.status, trace.settled.stage),
            (Status::Ok, Pass::E)
        );
        assert_eq!(trace.settled.text, "The budget is 500 yen.");
        Ok(())
    }

    #[test]
    fn a_line_whose_two_translations_both_fail_v_is_held_once()
    -> Result<(), Box<dyn std::error::Error>> {
        let script = Scripted::saying("The budget is 900 yen.", "The budget is 800 yen.");
        let trace = run_traced(&script, &request("予算は500円です。"))?;
        assert!(trace.early.is_empty());
        let settled = &trace.settled;
        assert_eq!(settled.hold_reason, Some(HoldReason::Number));
        assert_eq!((settled.status, settled.settled), (Status::Held, true));
        assert!(settled.text.is_empty());
        Ok(())
    }

    #[test]
    fn a_failed_edit_request_leaves_a_draft_that_passed() -> Result<(), Box<dyn std::error::Error>>
    {
        let script = Scripted::new(vec![
            Ok(String::from("Hello.")),
            Err(LlmError::Transport(String::from("down"))),
        ]);
        let trace = run_traced(&script, &request("こんにちは。"))?;
        assert_eq!(trace.early.len(), 1);
        let settled = &trace.settled;
        assert_eq!(
            (settled.status, settled.stage, settled.settled),
            (Status::Ok, Pass::T, true)
        );
        assert_eq!(settled.text, "Hello.");
        Ok(())
    }

    #[test]
    fn a_failed_edit_request_holds_a_line_whose_draft_failed()
    -> Result<(), Box<dyn std::error::Error>> {
        let script = Scripted::new(vec![
            Ok(String::from("The budget is 900 yen.")),
            Err(LlmError::Transport(String::from("down"))),
        ]);
        let trace = run_traced(&script, &request("予算は500円です。"))?;
        assert!(trace.early.is_empty());
        assert_eq!(trace.settled.hold_reason, Some(HoldReason::Unavailable));
        Ok(())
    }

    #[test]
    fn a_refine_line_is_published_once_and_settled() -> Result<(), Box<dyn std::error::Error>> {
        let script = Scripted::saying("Hello.", "Hello.");
        let mut line = request("こんにちは。");
        line.of = LineKind::Refine;
        let trace = run_traced(&script, &line)?;
        assert!(trace.early.is_empty());
        assert_eq!(
            (trace.settled.of, trace.settled.settled),
            (LineKind::Refine, true)
        );
        Ok(())
    }

    #[test]
    fn a_draft_with_the_end_marker_is_held_without_asking_e()
    -> Result<(), Box<dyn std::error::Error>> {
        let script = Scripted::saying("Hello.</s>", "Hello.");
        let trace = run_traced(&script, &request("こんにちは。"))?;
        assert!(trace.early.is_empty());
        assert_eq!(trace.settled.hold_reason, Some(HoldReason::Form));
        assert_eq!(script.prompts.borrow().len(), 1);
        Ok(())
    }

    #[test]
    fn the_lines_that_edit_failed_to_improve_say_so_in_the_log()
    -> Result<(), Box<dyn std::error::Error>> {
        let script = Scripted::saying("The budget is 500 yen.", "The budget is 900 yen.");
        let mut sink = MemorySink::default();
        let mut pipeline = Pipeline::open(&script, &PipelineConfig::default(), &mut sink)?;
        pipeline.run(&request("予算は500円です。"), &mut NoObserver, &mut sink);
        let held: Vec<_> = sink
            .events
            .iter()
            .filter(|event| event.event == "translate.edit.held")
            .collect();
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].fields["hold_reason"], "number");
        Ok(())
    }

    // ---- the text of an utterance in progress ---------------------------------

    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn partial_request(text: &str) -> LineRequest<'_> {
        LineRequest {
            id: "mic-2",
            of: LineKind::Partial,
            source: "mic",
            src_lang: Lang::Ja,
            text,
            now_epoch: 1_790_000_000,
        }
    }

    fn still_wanted() -> Wanted {
        Wanted::new(Arc::new(AtomicU64::new(1)), 1)
    }

    fn translated_in_progress(
        script: &Scripted,
        text: &str,
    ) -> Result<(PartialOutcome, MemorySink), Box<dyn std::error::Error>> {
        let mut sink = MemorySink::default();
        let mut pipeline = Pipeline::open(script, &PipelineConfig::default(), &mut sink)?;
        let outcome =
            pipeline.translate_partial(&partial_request(text), &still_wanted(), &mut sink);
        Ok((outcome, sink))
    }

    #[test]
    fn a_text_in_progress_is_translated_by_t_alone() -> Result<(), Box<dyn std::error::Error>> {
        let script = Scripted::new(vec![Ok(String::from(" Hello. \n"))]);
        let (outcome, _) = translated_in_progress(&script, "こんにちは。")?;
        let PartialOutcome::Shown(shown) = outcome else {
            return Err(format!("not shown: {outcome:?}").into());
        };
        assert_eq!(script.prompts.borrow().len(), 1);
        assert_eq!(
            script.prompts.borrow()[0],
            translate_prompt(Lang::Ja, "こんにちは。")
        );
        assert_eq!(
            (shown.of, shown.stage, shown.settled, shown.status),
            (LineKind::Partial, Pass::T, false, Status::Ok)
        );
        assert_eq!(
            (shown.id.as_str(), shown.text.as_str()),
            ("mic-2", "Hello.")
        );
        assert!(shown.annotations.is_empty() && shown.skipped.is_empty());
        assert_eq!(shown.src_sha256, sha256_hex("こんにちは。"));
        Ok(())
    }

    #[test]
    fn the_wire_form_of_a_text_in_progress_names_it_partial()
    -> Result<(), Box<dyn std::error::Error>> {
        let script = Scripted::new(vec![Ok(String::from("Hello."))]);
        let (outcome, _) = translated_in_progress(&script, "こんにちは。")?;
        let PartialOutcome::Shown(shown) = outcome else {
            return Err(format!("not shown: {outcome:?}").into());
        };
        let json = serde_json::to_value(&shown)?;
        assert_eq!(json["type"], "translation");
        assert_eq!(json["of"], "partial");
        assert_eq!(json["settled"], false);
        assert_eq!(json["stage"], "t");
        Ok(())
    }

    #[test]
    fn a_translation_that_fails_v_is_not_shown() -> Result<(), Box<dyn std::error::Error>> {
        let script = Scripted::new(vec![Ok(String::from("The budget is 900 yen."))]);
        let (outcome, sink) = translated_in_progress(&script, "予算は500円です。")?;
        assert_eq!(outcome, PartialOutcome::Held(HoldReason::Number));
        let held: Vec<_> = sink
            .events
            .iter()
            .filter(|event| event.event == "translate.partial.held")
            .collect();
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].fields["hold_reason"], "number");
        Ok(())
    }

    #[test]
    fn an_answer_with_the_end_marker_is_held_as_form() -> Result<(), Box<dyn std::error::Error>> {
        let script = Scripted::new(vec![Ok(String::from("Hello.</s>"))]);
        let (outcome, _) = translated_in_progress(&script, "こんにちは。")?;
        assert_eq!(outcome, PartialOutcome::Held(HoldReason::Form));
        Ok(())
    }

    #[test]
    fn a_text_not_wanted_when_it_is_asked_is_not_sent() -> Result<(), Box<dyn std::error::Error>> {
        let script = Scripted::new(vec![Ok(String::from("Hello."))]);
        let mut sink = MemorySink::default();
        let mut pipeline = Pipeline::open(&script, &PipelineConfig::default(), &mut sink)?;
        let latest = Arc::new(AtomicU64::new(2));
        let wanted = Wanted::new(latest, 1);
        let outcome =
            pipeline.translate_partial(&partial_request("こんにちは。"), &wanted, &mut sink);
        assert_eq!(outcome, PartialOutcome::Aborted);
        assert!(script.prompts.borrow().is_empty());
        Ok(())
    }

    /// A translator that moves the counter on while it answers, as a newer
    /// text does.
    struct Superseded(Arc<AtomicU64>);

    impl Llm for Superseded {
        fn complete(&self, _request: &Completion<'_>) -> Result<String, LlmError> {
            self.0.fetch_add(1, Ordering::AcqRel);
            Ok(String::from("Hello."))
        }
    }

    #[test]
    fn an_answer_that_comes_when_it_is_not_wanted_is_dropped()
    -> Result<(), Box<dyn std::error::Error>> {
        let latest = Arc::new(AtomicU64::new(1));
        let wanted = Wanted::new(Arc::clone(&latest), 1);
        let mut sink = MemorySink::default();
        let mut pipeline = Pipeline::open(
            Superseded(Arc::clone(&latest)),
            &PipelineConfig::default(),
            &mut sink,
        )?;
        let outcome =
            pipeline.translate_partial(&partial_request("こんにちは。"), &wanted, &mut sink);
        assert_eq!(outcome, PartialOutcome::Aborted);
        assert!(
            sink.events
                .iter()
                .any(|event| event.event == "translate.partial.aborted")
        );
        Ok(())
    }

    #[test]
    fn a_request_the_translator_dropped_is_aborted_and_said()
    -> Result<(), Box<dyn std::error::Error>> {
        let script = Scripted::new(vec![Err(LlmError::Aborted)]);
        let (outcome, sink) = translated_in_progress(&script, "こんにちは。")?;
        assert_eq!(outcome, PartialOutcome::Aborted);
        assert_eq!(sink.events.len(), 1);
        assert_eq!(sink.events[0].event, "translate.partial.aborted");
        assert_eq!(sink.events[0].error_code, None);
        Ok(())
    }

    #[test]
    fn a_translator_that_does_not_answer_fails_the_text() -> Result<(), Box<dyn std::error::Error>>
    {
        let script = Scripted::new(vec![Err(LlmError::Transport(String::from("down")))]);
        let (outcome, sink) = translated_in_progress(&script, "こんにちは。")?;
        assert_eq!(outcome, PartialOutcome::Failed);
        let failed: Vec<_> = sink
            .events
            .iter()
            .filter(|event| event.event == "translate.partial.failed")
            .collect();
        assert_eq!(failed.len(), 1);
        assert!(failed[0].error_code.is_some());
        Ok(())
    }

    #[test]
    fn the_log_of_a_text_in_progress_carries_no_subtitle_text()
    -> Result<(), Box<dyn std::error::Error>> {
        let script = Scripted::new(vec![Ok(String::from("Hello."))]);
        let (_, sink) = translated_in_progress(&script, "こんにちは。")?;
        let line = sink
            .events
            .iter()
            .find(|event| event.event == "translate.partial.ok")
            .map(LogEvent::to_json_line)
            .ok_or("no ok event")?;
        assert!(
            !line.contains("こんにちは") && !line.contains("Hello"),
            "{line}"
        );
        assert!(line.contains("\"id\":\"mic-2\"") && line.contains("\"source\":\"mic\""));
        Ok(())
    }
}

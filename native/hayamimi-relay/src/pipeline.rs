//! The three stages for one line: translate (T), edit (E), verify (V), then
//! the unit annotation. Each stage runs once, in this order. V sees the text
//! of E exactly; the annotation is added after V and never shown to it.

use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::Serialize;

use crate::annotate::{
    AnnotateRequest, Annotated, Annotation, Annotator, AnnotatorConfig, Skipped,
};
use crate::lang::Lang;
use crate::llm::{Completion, Llm};
use crate::logging::{Level, LogEvent, LogSink};
use crate::style::{Calques, JaStyle};
use crate::verify::{HoldReason, Verdict, Verifier, contains_eos};
use crate::watched::Watched;

/// Tokens a translation or an edit may use at most.
const MAX_TOKENS: u32 = 256;

/// Which event of the recognizer a line came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LineKind {
    Final,
    Refine,
}

/// Whether the line may be shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Ok,
    Held,
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
    pub lang: Lang,
    pub status: Status,
    pub hold_reason: Option<HoldReason>,
    /// The edited translation, as the model wrote it; empty when held.
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

/// A stage of the pipeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Translate,
    Edit,
    Verify,
    Annotate,
}

/// Watches the pipeline from the outside (tests, tracing). Every method is a
/// no-op unless overridden.
pub trait Observer {
    /// A stage is about to run.
    fn stage(&mut self, _stage: Stage) {}
    /// The exact translation text handed to V.
    fn verify_input(&mut self, _translation: &str) {}
}

/// An observer that sees nothing.
pub struct NoObserver;

impl Observer for NoObserver {}

fn language_name(lang: Lang) -> &'static str {
    match lang {
        Lang::Ja => "Japanese",
        Lang::En => "English",
    }
}

/// The prompt for T, in the form the model card recommends.
pub fn translate_prompt(src_lang: Lang, text: &str) -> String {
    format!(
        "Translate the following {} text into {}.\n\n{text}",
        language_name(src_lang),
        language_name(src_lang.other())
    )
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
    let mut prompt = format!(
        "Below is a {from} sentence from a live conversation and a draft {to} translation.\n\
         Rewrite the draft so that it reads as natural spoken {to}: natural word order, \
         connectives and tone. Do not change facts, numbers, proper names, negation or \
         uncertainty. Output only the rewritten translation.\n"
    );
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
    prompt.push_str(&format!(
        "\n{from}:\n{text}\n\nDraft {to}:\n{draft}\n\nRewritten {to}:\n"
    ));
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

    /// Runs one line through T, E, V and the annotation.
    pub fn run(
        &mut self,
        request: &LineRequest<'_>,
        observer: &mut dyn Observer,
        sink: &mut dyn LogSink,
    ) -> Translation {
        self.calques.refresh(sink);
        self.ja_style.refresh(sink);
        let started = Instant::now();
        observer.stage(Stage::Translate);
        let prompt = translate_prompt(request.src_lang, request.text);
        let draft = match self.llm.complete(&Completion::new(&prompt, MAX_TOKENS)) {
            Ok(text) => text,
            Err(error) => return self.unavailable(request, &error.to_string(), started, sink),
        };
        let t_ms = started.elapsed().as_millis();
        if contains_eos(&draft) {
            // `stop` did not take effect; the marker would reach E's prompt.
            return self.held(request, HoldReason::Form, [t_ms, 0, 0], sink);
        }
        let edit_started = Instant::now();
        observer.stage(Stage::Edit);
        let prompt = edit_prompt(
            request.src_lang,
            request.text,
            draft.trim(),
            self.calques.current(),
            self.ja_style.current(),
        );
        let edited = match self.llm.complete(&Completion::new(&prompt, MAX_TOKENS)) {
            Ok(text) => text,
            Err(error) => return self.unavailable(request, &error.to_string(), started, sink),
        };
        let e_ms = edit_started.elapsed().as_millis();
        let text = edited.trim().to_owned();
        let verify_started = Instant::now();
        observer.stage(Stage::Verify);
        observer.verify_input(&text);
        let verdict = self
            .verifier
            .verify(request.text, &text, request.src_lang, sink);
        let v_ms = verify_started.elapsed().as_millis();
        match verdict {
            Verdict::Held(reason) => self.held(request, reason, [t_ms, e_ms, v_ms], sink),
            Verdict::Ok => {
                observer.stage(Stage::Annotate);
                let annotated = self.annotator.annotate(
                    &AnnotateRequest {
                        id: request.id,
                        src: request.text,
                        translation: &text,
                        src_lang: request.src_lang,
                        now_epoch: request.now_epoch,
                    },
                    sink,
                );
                self.finish_ok(request, text, annotated, [t_ms, e_ms, v_ms], sink)
            }
        }
    }

    fn base(
        &self,
        request: &LineRequest<'_>,
        status: Status,
        reason: Option<HoldReason>,
    ) -> Translation {
        Translation {
            event_type: "translation",
            id: request.id.to_owned(),
            of: request.of,
            source: request.source.to_owned(),
            src_lang: request.src_lang,
            lang: request.src_lang.other(),
            status,
            hold_reason: reason,
            text: String::new(),
            annotations: Vec::new(),
            skipped: Vec::new(),
        }
    }

    fn finish_ok(
        &self,
        request: &LineRequest<'_>,
        text: String,
        annotated: Annotated,
        ms: [u128; 3],
        sink: &mut dyn LogSink,
    ) -> Translation {
        sink.emit(timing(
            LogEvent::new(Level::Info, "translate.line.ok", "service"),
            request,
            ms,
        ));
        let mut line = self.base(request, Status::Ok, None);
        line.text = text;
        line.annotations = annotated.annotations;
        line.skipped = annotated.skipped;
        line
    }

    fn held(
        &self,
        request: &LineRequest<'_>,
        reason: HoldReason,
        ms: [u128; 3],
        sink: &mut dyn LogSink,
    ) -> Translation {
        sink.emit(timing(
            LogEvent::new(Level::Info, "translate.line.held", "service")
                .with("hold_reason", reason.as_str()),
            request,
            ms,
        ));
        self.base(request, Status::Held, Some(reason))
    }

    fn unavailable(
        &self,
        request: &LineRequest<'_>,
        detail: &str,
        started: Instant,
        sink: &mut dyn LogSink,
    ) -> Translation {
        let duration = u64::try_from(started.elapsed().as_millis()).ok();
        sink.emit(
            LogEvent::new(Level::Error, "translate.request.failed", "service")
                .with("id", request.id)
                .with("dependency", "llama-server")
                .with("duration_ms", duration)
                .with("detail", detail.split(':').next().map(str::to_owned))
                .with_error("LLM_UNAVAILABLE"),
        );
        self.base(request, Status::Held, Some(HoldReason::Unavailable))
    }
}

fn timing(event: LogEvent, request: &LineRequest<'_>, ms: [u128; 3]) -> LogEvent {
    let clip = |value: u128| u64::try_from(value).ok();
    event
        .with("id", request.id)
        .with("source", request.source)
        .with("t_ms", clip(ms[0]))
        .with("e_ms", clip(ms[1]))
        .with("v_ms", clip(ms[2]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::LlmError;
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
        Ok(())
    }
}

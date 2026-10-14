//! A recognized line as it arrives on a recognizer's `/events` stream, and the
//! rule for which lines are translated.
//!
//! Only `final` and `refine` events are lines. Every other event type (a
//! partial, a fallback, a session start) is ignored. A line must carry the
//! items the relay joins a translation to: `id`, `source`, `lang` and `text`.

use serde_json::Value;

use crate::lang::Lang;
use crate::pipeline::{LineKind, LineRequest};

/// How many distinct sources there are, and so how many lanes at most.
pub const SOURCES: usize = 3;

/// Where the audio of a line came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Mic,
    Loopback,
    File,
}

impl Source {
    /// The wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Mic => "mic",
            Source::Loopback => "loopback",
            Source::File => "file",
        }
    }

    /// The source a wire name names; `None` for any other word.
    pub fn from_wire(name: &str) -> Option<Source> {
        match name {
            "mic" => Some(Source::Mic),
            "loopback" => Some(Source::Loopback),
            "file" => Some(Source::File),
            _ => None,
        }
    }

    /// A number below [`SOURCES`] that is different for each source.
    pub fn index(self) -> usize {
        match self {
            Source::Mic => 0,
            Source::Loopback => 1,
            Source::File => 2,
        }
    }
}

/// Why an event that claims to be a line was not used. The name goes into the
/// log event as `reason`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Skip {
    NotJson,
    NoId,
    NoSource,
    NoLang,
    NoText,
}

impl Skip {
    pub fn as_str(self) -> &'static str {
        match self {
            Skip::NotJson => "not_json",
            Skip::NoId => "no_id",
            Skip::NoSource => "no_source",
            Skip::NoLang => "no_lang",
            Skip::NoText => "no_text",
        }
    }
}

/// One line to translate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub kind: LineKind,
    pub id: String,
    pub source: Source,
    /// `None` for a language the relay does not translate.
    pub lang: Option<Lang>,
    pub text: String,
}

fn string_of<'a>(event: &'a Value, key: &str) -> Option<&'a str> {
    event.get(key).and_then(Value::as_str)
}

fn kind_of(event: &Value) -> Option<LineKind> {
    match string_of(event, "type")? {
        "final" => Some(LineKind::Final),
        "refine" => Some(LineKind::Refine),
        _ => None,
    }
}

/// Reads one event. `Ok(None)` is an event that is not a line; `Err` is a line
/// that lacks what the relay needs, or a text that is not JSON at all.
pub fn parse_line(json: &str) -> Result<Option<Line>, Skip> {
    let event: Value = serde_json::from_str(json).map_err(|_| Skip::NotJson)?;
    let Some(kind) = kind_of(&event) else {
        return Ok(None);
    };
    let id = string_of(&event, "id")
        .filter(|id| !id.is_empty())
        .ok_or(Skip::NoId)?;
    let source = string_of(&event, "source")
        .and_then(Source::from_wire)
        .ok_or(Skip::NoSource)?;
    let lang = string_of(&event, "lang").ok_or(Skip::NoLang)?;
    let text = string_of(&event, "text")
        .filter(|text| !text.trim().is_empty())
        .ok_or(Skip::NoText)?;
    Ok(Some(Line {
        kind,
        id: id.to_owned(),
        source,
        lang: Lang::from_code(lang),
        text: text.to_owned(),
    }))
}

impl Line {
    /// The language to translate from, or `None` when the line is not
    /// translated: the microphone is Japanese and is translated to English,
    /// the loopback is English and is translated to Japanese, and a file is
    /// translated whichever of the two it is. Any other language is shown as
    /// it is.
    pub fn source_language(&self) -> Option<Lang> {
        let lang = self.lang?;
        let wanted = match self.source {
            Source::Mic => lang == Lang::Ja,
            Source::Loopback => lang == Lang::En,
            Source::File => true,
        };
        wanted.then_some(lang)
    }

    /// The request that runs this line through the pipeline.
    pub fn request(&self, src_lang: Lang, now_epoch: i64) -> LineRequest<'_> {
        LineRequest {
            id: &self.id,
            of: self.kind,
            source: self.source.as_str(),
            src_lang,
            text: &self.text,
            now_epoch,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(lang: &str, source: &str) -> String {
        format!(
            r#"{{"type":"final","id":"{source}-1","source":"{source}","lang":"{lang}","text":"こんにちは。","t0_s":0.5,"t1_s":1.5,"end_wall_ms":1790000000000}}"#
        )
    }

    #[test]
    fn a_final_with_the_items_is_a_line() -> Result<(), Skip> {
        let parsed = parse_line(&line("ja", "mic"))?;
        assert_eq!(
            parsed,
            Some(Line {
                kind: LineKind::Final,
                id: String::from("mic-1"),
                source: Source::Mic,
                lang: Some(Lang::Ja),
                text: String::from("こんにちは。"),
            })
        );
        Ok(())
    }

    #[test]
    fn a_refine_is_a_line_of_the_refine_kind() -> Result<(), Skip> {
        let json = line("en", "loopback").replace("final", "refine");
        let parsed = parse_line(&json)?;
        assert!(parsed.is_some_and(|found| found.kind == LineKind::Refine));
        Ok(())
    }

    #[test]
    fn other_event_types_are_not_lines() -> Result<(), Skip> {
        for json in [
            r#"{"type":"partial","text":"こん"}"#,
            r#"{"type":"fallback","stage":"punct","to":"cpu"}"#,
            r#"{"type":"session_start"}"#,
            r#"{"text":"no type"}"#,
            "{}",
        ] {
            assert_eq!(parse_line(json)?, None, "{json}");
        }
        Ok(())
    }

    #[test]
    fn a_line_without_what_the_relay_joins_on_is_refused_with_a_reason() {
        let cases = [
            ("not json", Skip::NotJson),
            ("", Skip::NotJson),
            (
                r#"{"type":"final","source":"mic","lang":"ja","text":"あ"}"#,
                Skip::NoId,
            ),
            (
                r#"{"type":"final","id":"","source":"mic","lang":"ja","text":"あ"}"#,
                Skip::NoId,
            ),
            (
                r#"{"type":"final","id":"x-1","lang":"ja","text":"あ"}"#,
                Skip::NoSource,
            ),
            (
                r#"{"type":"final","id":"x-1","source":"radio","lang":"ja","text":"あ"}"#,
                Skip::NoSource,
            ),
            (
                r#"{"type":"final","id":"x-1","source":"mic","text":"あ"}"#,
                Skip::NoLang,
            ),
            (
                r#"{"type":"final","id":"x-1","source":"mic","lang":"ja"}"#,
                Skip::NoText,
            ),
            (
                r#"{"type":"final","id":"x-1","source":"mic","lang":"ja","text":" "}"#,
                Skip::NoText,
            ),
            (
                r#"{"type":"final","id":7,"source":"mic","lang":"ja","text":"あ"}"#,
                Skip::NoId,
            ),
        ];
        for (json, reason) in cases {
            assert_eq!(parse_line(json), Err(reason), "{json}");
        }
    }

    #[test]
    fn a_language_other_than_ja_and_en_is_a_line_that_is_not_translated() -> Result<(), Skip> {
        let parsed = parse_line(&line("zh", "loopback"))?;
        assert!(parsed.as_ref().is_some_and(|found| found.lang.is_none()));
        assert!(parsed.is_some_and(|found| found.source_language().is_none()));
        Ok(())
    }

    fn translated(lang: &str, source: &str) -> Result<Option<Lang>, Skip> {
        Ok(parse_line(&line(lang, source))?.and_then(|found| found.source_language()))
    }

    #[test]
    fn the_microphone_is_translated_from_japanese_only() -> Result<(), Skip> {
        assert_eq!(translated("ja", "mic")?, Some(Lang::Ja));
        assert_eq!(translated("en", "mic")?, None);
        Ok(())
    }

    #[test]
    fn the_loopback_is_translated_from_english_only() -> Result<(), Skip> {
        assert_eq!(translated("en", "loopback")?, Some(Lang::En));
        assert_eq!(translated("ja", "loopback")?, None);
        Ok(())
    }

    #[test]
    fn a_file_is_translated_from_either_language() -> Result<(), Skip> {
        assert_eq!(translated("ja", "file")?, Some(Lang::Ja));
        assert_eq!(translated("en", "file")?, Some(Lang::En));
        assert_eq!(translated("ko", "file")?, None);
        Ok(())
    }

    #[test]
    fn every_source_has_its_own_index_below_the_count() {
        let mut seen = [false; SOURCES];
        for source in [Source::Mic, Source::Loopback, Source::File] {
            assert!(!seen[source.index()], "{source:?}");
            seen[source.index()] = true;
            assert_eq!(Source::from_wire(source.as_str()), Some(source));
        }
        assert!(seen.iter().all(|taken| *taken));
    }

    #[test]
    fn the_request_carries_the_line_as_it_is() -> Result<(), Skip> {
        let found = parse_line(&line("ja", "mic"))?;
        let Some(found) = found else {
            return Err(Skip::NoId);
        };
        let request = found.request(Lang::Ja, 7);
        assert_eq!(
            (request.id, request.source, request.text, request.now_epoch),
            ("mic-1", "mic", "こんにちは。", 7)
        );
        assert_eq!(request.of, LineKind::Final);
        Ok(())
    }
}

//! The one place that knows the key names of the `/events` stream.
//!
//! A message of the stream is one JSON object in a `data:` line. `parse` turns
//! it into a [`UiEvent`], the only shape the rest of the crate reads. The three
//! points where the wire is not settled are all here, so that a change of the
//! wire is a change of this file:
//!
//! - a translation of a partial is a `translation` whose `of` is `"partial"`;
//! - the translation that decides a line is the one whose `settled` is true;
//! - a `fallback` carries `stage`, an optional `reason` and `to`, and a `to` of
//!   `null` ends that stage's fallback.
//!
//! Keys the pages do not read (`tier`, `audio_s`, `t0_s`, `src_sha256`, ...) are
//! left in the message and ignored, so a key added to the stream breaks nothing.

use serde::Deserialize;

/// One message of the stream, as the pages read it.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiEvent {
    /// The text heard so far of the utterance being spoken.
    Partial { text: String },
    /// A finished utterance.
    Final(FinalEvent),
    /// The speaker of an earlier `final`, found after it was sent.
    Speaker { id: String, speaker: String },
    /// A translation of a partial, of a final or of a refined paragraph.
    Translation(TranslationEvent),
    /// A paragraph rewritten from several finals.
    Refine(RefineEvent),
    /// A stage that runs on another road than the usual one, or stops doing so.
    Fallback(FallbackEvent),
    /// Any other message (`session_start`, `ready`, `warning`, ...): the pages
    /// show nothing of it, and its arrival alone keeps the overlay's timer.
    #[serde(other)]
    Other,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct FinalEvent {
    /// `<source>-<number>`; absent on a stream that does not number its lines.
    pub id: Option<String>,
    pub text: String,
    #[serde(default)]
    pub lang: String,
    #[serde(default)]
    pub speaker: String,
    pub latency_ms: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct RefineEvent {
    pub id: Option<String>,
    pub text: String,
    #[serde(default)]
    pub lang: String,
    #[serde(default)]
    pub speaker: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct FallbackEvent {
    pub stage: String,
    pub reason: Option<String>,
    /// Where the stage runs now; `None` when the fallback has ended.
    pub to: Option<String>,
}

/// What a translation is a translation of.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Of {
    Partial,
    /// A translation without `of` is the older form, which was always of a final.
    #[default]
    Final,
    Refine,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    #[default]
    Ok,
    Held,
    Unavailable,
}

/// A converted value written after the character at `end` of the translation.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Annotation {
    /// A position in Unicode scalar values from the start of `text`.
    pub end: usize,
    pub text: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct TranslationEvent {
    #[serde(default)]
    pub of: Of,
    /// The `id` of the line translated; absent on a translation of a partial.
    pub id: Option<String>,
    pub lang: String,
    /// Empty when the translation was withheld.
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub status: Status,
    pub hold_reason: Option<String>,
    /// True on the one event that decides the translation of the line.
    #[serde(default)]
    pub settled: bool,
    #[serde(default)]
    pub annotations: Vec<Annotation>,
}

impl TranslationEvent {
    /// Whether the translator could not be reached. The relay sends this as a
    /// withheld line whose reason is `unavailable`.
    pub fn is_unavailable(&self) -> bool {
        self.status == Status::Unavailable
            || (self.status == Status::Held && self.hold_reason.as_deref() == Some("unavailable"))
    }
}

/// Reads the `data` of one message.
pub fn parse(data: &str) -> Result<UiEvent, serde_json::Error> {
    serde_json::from_str(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_final_keeps_what_the_pages_read_and_ignores_the_rest() -> Result<(), serde_json::Error> {
        let data = r#"{"type":"final","id":"mic-3","text":"こんにちは","lang":"ja","speaker":"S1","latency_ms":412.5,"tier":"rz","t0_s":1.0}"#;
        let UiEvent::Final(found) = parse(data)? else {
            return Err(serde::de::Error::custom("not a final"));
        };
        assert_eq!(found.id.as_deref(), Some("mic-3"));
        assert_eq!(found.speaker, "S1");
        assert_eq!(found.latency_ms, Some(412.5));
        Ok(())
    }

    #[test]
    fn a_final_without_an_id_or_a_latency_is_still_a_final() -> Result<(), serde_json::Error> {
        let data = r#"{"type":"final","text":"a","lang":"en","latency_ms":null}"#;
        let UiEvent::Final(found) = parse(data)? else {
            return Err(serde::de::Error::custom("not a final"));
        };
        assert_eq!(found.id, None);
        assert_eq!(found.latency_ms, None);
        assert_eq!(found.speaker, "");
        Ok(())
    }

    #[test]
    fn an_unknown_type_is_other() -> Result<(), serde_json::Error> {
        assert_eq!(parse(r#"{"type":"session_start"}"#)?, UiEvent::Other);
        assert_eq!(
            parse(r#"{"type":"never_heard_of_it","x":1}"#)?,
            UiEvent::Other
        );
        Ok(())
    }

    #[test]
    fn a_final_without_text_is_refused() {
        assert!(parse(r#"{"type":"final","lang":"ja"}"#).is_err());
    }

    #[test]
    fn a_latency_that_is_not_a_number_is_refused() {
        assert!(parse(r#"{"type":"final","text":"a","latency_ms":"fast"}"#).is_err());
    }

    #[test]
    fn a_message_that_is_not_json_or_has_no_type_is_refused() {
        assert!(parse("not json").is_err());
        assert!(parse(r#"{"text":"a"}"#).is_err());
        assert!(parse("[]").is_err());
    }

    #[test]
    fn a_translation_without_of_is_the_older_form_of_a_final() -> Result<(), serde_json::Error> {
        let UiEvent::Translation(found) =
            parse(r#"{"type":"translation","lang":"en","text":"hi"}"#)?
        else {
            return Err(serde::de::Error::custom("not a translation"));
        };
        assert_eq!(found.of, Of::Final);
        assert_eq!(found.id, None);
        assert!(!found.settled);
        Ok(())
    }

    #[test]
    fn a_translation_of_an_unknown_kind_is_refused() {
        assert!(parse(r#"{"type":"translation","of":"x","lang":"en"}"#).is_err());
    }

    #[test]
    fn a_withheld_line_for_an_unreachable_translator_is_unavailable()
    -> Result<(), serde_json::Error> {
        let held = r#"{"type":"translation","of":"final","id":"a-1","lang":"en","status":"held","hold_reason":"unavailable","text":"","settled":true}"#;
        let UiEvent::Translation(found) = parse(held)? else {
            return Err(serde::de::Error::custom("not a translation"));
        };
        assert!(found.is_unavailable());
        let number = held.replace("unavailable\",\"text", "number\",\"text");
        let UiEvent::Translation(other) = parse(&number)? else {
            return Err(serde::de::Error::custom("not a translation"));
        };
        assert!(!other.is_unavailable());
        Ok(())
    }

    #[test]
    fn a_fallback_with_a_null_to_has_ended() -> Result<(), serde_json::Error> {
        let UiEvent::Fallback(found) = parse(r#"{"type":"fallback","stage":"sink","to":null}"#)?
        else {
            return Err(serde::de::Error::custom("not a fallback"));
        };
        assert_eq!(found.to, None);
        assert_eq!(found.reason, None);
        Ok(())
    }
}

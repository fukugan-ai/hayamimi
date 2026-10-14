//! RP-25: how much a translation moves in front of a reader.
//!
//! A run is a list of [`Event`]s in time order: what the screen showed for an
//! utterance, and what was sent to a destination outside. Per utterance the
//! screen may change its text once, from the draft to the settled translation;
//! after that it stays. A prefix drawn in the settled form never disappears.
//! Each destination gets one caption per `id`, and it is the settled text.

use serde_json::Value;

use hayamimi_relay::deliver::insert_notes;

/// What the screen showed for one utterance at one moment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shown {
    pub id: String,
    /// The translation with its converted-value notes written in.
    pub text: String,
    /// True on the translation that decides the line.
    pub settled: bool,
    /// How many leading characters were drawn in the settled form.
    pub committed: usize,
}

/// One caption handed to a destination outside the machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sent {
    pub id: String,
    pub to: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Shown(Shown),
    Sent(Sent),
}

/// What a check refuses. Every variant but the last names the utterance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fault {
    /// The text changed more than once before the settled one.
    TooManyReplacements(String),
    /// The text changed after the settled one.
    ChangedAfterSettled(String),
    /// A prefix drawn in the settled form is gone or different.
    VanishedPrefix(String),
    /// A destination got more than one caption for the id.
    SentTwice(String),
    /// A destination got a text other than the settled one, or the line was
    /// not settled yet.
    SentNotSettled(String),
    /// A `translation` event lacks an item the check reads.
    Malformed,
}

/// The screen events of one utterance, in order.
fn shown_of<'a>(events: &'a [Event], id: &str) -> Vec<&'a Shown> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Shown(shown) if shown.id == id => Some(shown),
            _ => None,
        })
        .collect()
}

/// The ids in the order they first appear.
fn ids_of(events: &[Event]) -> Vec<&str> {
    let mut ids: Vec<&str> = Vec::new();
    for event in events {
        let id = match event {
            Event::Shown(shown) => shown.id.as_str(),
            Event::Sent(sent) => sent.id.as_str(),
        };
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}

fn first_chars(text: &str, count: usize) -> &str {
    match text.char_indices().nth(count) {
        Some((end, _)) => &text[..end],
        None => text,
    }
}

/// How many times the shown text differs from the one before it.
fn count_changed(shown: &[&Shown]) -> usize {
    shown
        .windows(2)
        .filter(|pair| pair[0].text != pair[1].text)
        .count()
}

fn check_screen(id: &str, shown: &[&Shown]) -> Result<(), Fault> {
    for pair in shown.windows(2) {
        let kept = first_chars(&pair[0].text, pair[0].committed);
        if !pair[1].text.starts_with(kept) {
            return Err(Fault::VanishedPrefix(id.to_owned()));
        }
        if pair[0].settled && pair[0].text != pair[1].text {
            return Err(Fault::ChangedAfterSettled(id.to_owned()));
        }
    }
    if count_changed(shown) > 1 {
        return Err(Fault::TooManyReplacements(id.to_owned()));
    }
    Ok(())
}

/// The text of the settled translation of an utterance, when there is one.
fn settled_text<'a>(shown: &[&'a Shown]) -> Option<&'a str> {
    shown
        .iter()
        .rev()
        .find(|item| item.settled)
        .map(|item| item.text.as_str())
}

fn check_sends(id: &str, events: &[Event], shown: &[&Shown]) -> Result<(), Fault> {
    let settled = settled_text(shown);
    let mut seen: Vec<&str> = Vec::new();
    for event in events {
        let Event::Sent(sent) = event else { continue };
        if sent.id != id {
            continue;
        }
        if seen.contains(&sent.to.as_str()) {
            return Err(Fault::SentTwice(id.to_owned()));
        }
        seen.push(&sent.to);
        if settled != Some(sent.text.as_str()) {
            return Err(Fault::SentNotSettled(id.to_owned()));
        }
    }
    Ok(())
}

/// Checks every utterance of a run.
pub fn check_flicker(events: &[Event]) -> Result<(), Fault> {
    for id in ids_of(events) {
        let shown = shown_of(events, id);
        check_screen(id, &shown)?;
        check_sends(id, events, &shown)?;
    }
    Ok(())
}

/// How many utterances were first shown with a text other than the settled
/// one: the number of replacements a reader saw.
pub fn count_replaced(events: &[Event]) -> usize {
    ids_of(events)
        .into_iter()
        .filter(|id| {
            let shown = shown_of(events, id);
            let first = shown.first().map(|item| item.text.as_str());
            settled_text(&shown).is_some_and(|last| first != Some(last))
        })
        .count()
}

/// The text of a `translation` event with its notes written in, or `None`
/// when the event lacks `text` or a well-formed annotation.
fn rendered(event: &Value) -> Option<String> {
    let text = event.get("text")?.as_str()?;
    let list = event.get("annotations")?.as_array()?;
    let mut notes: Vec<(usize, &str)> = Vec::with_capacity(list.len());
    for note in list {
        let end = usize::try_from(note.get("end")?.as_u64()?).ok()?;
        notes.push((end, note.get("text")?.as_str()?));
    }
    Some(insert_notes(text, &notes))
}

/// Reads one event of the relay's stream. `Ok(None)` is an event that is not a
/// translation.
pub fn shown_from_wire(json: &str) -> Result<Option<Shown>, Fault> {
    let event: Value = serde_json::from_str(json).map_err(|_| Fault::Malformed)?;
    if event.get("type").and_then(Value::as_str) != Some("translation") {
        return Ok(None);
    }
    // The translation of the text in progress shares the `id` the line will
    // take, but it is a line of its own on the screen.
    if event.get("of").and_then(Value::as_str) == Some("partial") {
        return Ok(None);
    }
    let id = event.get("id").and_then(Value::as_str);
    let settled = event.get("settled").and_then(Value::as_bool);
    let (Some(id), Some(settled), Some(text)) = (id, settled, rendered(&event)) else {
        return Err(Fault::Malformed);
    };
    Ok(Some(Shown {
        id: id.to_owned(),
        text,
        settled,
        committed: 0,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_run_passes() {
        assert_eq!(check_flicker(&[]), Ok(()));
        assert_eq!(count_replaced(&[]), 0);
    }

    #[test]
    fn a_wire_event_is_read_with_its_notes() -> Result<(), Fault> {
        let json = r#"{"type":"translation","id":"a","settled":true,"text":"100ドル","annotations":[{"start":0,"end":5,"text":"(x)"}]}"#;
        let found = shown_from_wire(json)?;
        assert_eq!(
            found.map(|item| (item.text, item.settled)),
            Some((String::from("100ドル(x)"), true))
        );
        assert_eq!(shown_from_wire(r#"{"type":"final"}"#), Ok(None));
        assert_eq!(
            shown_from_wire(r#"{"type":"translation","id":"a"}"#),
            Err(Fault::Malformed)
        );
        assert_eq!(shown_from_wire("nope"), Err(Fault::Malformed));
        Ok(())
    }

    #[test]
    fn the_translation_of_a_text_in_progress_is_not_a_line_of_the_screen() {
        let json = r#"{"type":"translation","id":"a","of":"partial","settled":false,"text":"Hi","annotations":[]}"#;
        assert_eq!(shown_from_wire(json), Ok(None));
    }

    #[test]
    fn a_prefix_is_cut_by_characters() {
        assert_eq!(first_chars("あいう", 2), "あい");
        assert_eq!(first_chars("あ", 5), "あ");
        assert_eq!(first_chars("あ", 0), "");
    }
}

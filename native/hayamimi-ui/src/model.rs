//! What the three pages show, and how each event of the stream changes it.
//!
//! `Model` holds every page's state in one place. The pages only read it, and
//! the only writers are `apply` (an event arrived) and `expire` (a deadline from
//! `next_deadline` came). Neither reads a clock: the caller passes the time in
//! milliseconds, so the timing rules are the same on a page and in a test.

use std::collections::VecDeque;

use crate::tr::{TrRow, partial_row, settled_row};
use crate::wire::{FallbackEvent, FinalEvent, Of, RefineEvent, TranslationEvent, UiEvent};

/// The newest rows each column keeps. The time to draw a line grows with the
/// rows on the page, and a line must reach the screen within one frame.
pub const ROW_LIMIT: usize = 500;

/// The overlay clears its line this long after the last message of any kind.
pub const OVERLAY_HOLD_MS: u64 = 6000;
/// The dashboard's strip fades this long after the last partial.
pub const STRIP_QUIET_MS: u64 = 4000;
/// A translation whose line has not arrived is kept this long.
pub const ORPHAN_HOLD_MS: u64 = 5000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Conn {
    #[default]
    Connecting,
    Connected,
    Reconnecting,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FinalRow {
    pub id: Option<String>,
    pub text: String,
    pub lang: String,
    pub speaker: String,
    pub latency_ms: Option<f64>,
    /// One row per language translated into.
    pub trs: Vec<TrRow>,
    /// When the row last changed, in the milliseconds the caller passed to
    /// `apply`: the line arrived, or a translation joined it or replaced one.
    /// A translation that leaves `trs` as it was does not move it.
    pub updated_ms: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RefineRow {
    pub id: Option<String>,
    pub text: String,
    pub lang: String,
    pub speaker: String,
    pub trs: Vec<TrRow>,
}

/// A stage that runs on another road than the usual one.
#[derive(Clone, Debug, PartialEq)]
pub struct Fallback {
    pub stage: String,
    pub reason: Option<String>,
    pub to: String,
}

/// The overlay's two lines and the translation under the confirmed one: what
/// the page holds between one message and the next.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Overlay {
    /// The `id` of the line shown, to join its translation.
    pub final_id: Option<String>,
    pub final_text: String,
    pub partial_text: String,
    pub final_tr: Option<TrRow>,
}

impl Overlay {
    fn has_final(&self) -> bool {
        self.final_id.is_some() || !self.final_text.is_empty()
    }

    fn clear_final(&mut self) {
        self.final_id = None;
        self.final_text.clear();
        self.final_tr = None;
    }
}

/// The numbers of the dashboard's header.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    pub finals: usize,
    latency_sum: f64,
    latency_count: usize,
    /// Languages in the order they were first seen.
    pub langs: Vec<String>,
}

impl Stats {
    fn add(&mut self, event: &FinalEvent) {
        self.finals += 1;
        if let Some(latency) = event.latency_ms {
            self.latency_sum += latency;
            self.latency_count += 1;
        }
        if !event.lang.is_empty() && !self.langs.contains(&event.lang) {
            self.langs.push(event.lang.clone());
        }
    }

    /// The mean latency of the finals that carry one, rounded; `None` before the first.
    pub fn mean_latency_ms(&self) -> Option<u64> {
        if self.latency_count == 0 {
            return None;
        }
        Some((self.latency_sum / self.latency_count as f64).round() as u64)
    }
}

/// A translation that arrived before its line.
#[derive(Clone, Debug, PartialEq)]
struct Orphan {
    at_ms: u64,
    event: TranslationEvent,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Model {
    pub conn: Conn,
    pub overlay: Overlay,
    /// The translation of the partial, shared by the overlay and the strip.
    pub partial_tr: Option<TrRow>,
    /// Whether a translation of a partial or of a final has ever arrived; the
    /// overlay shows its translation lines from then on.
    pub tr_seen: bool,
    /// The dashboard's strip; unlike the overlay's line a final does not clear it.
    pub strip_text: String,
    pub strip_quiet: bool,
    /// The newest `ROW_LIMIT` confirmed lines, oldest first.
    pub finals: VecDeque<FinalRow>,
    /// How many older confirmed lines were dropped from `finals`.
    pub dropped_finals: usize,
    /// The newest `ROW_LIMIT` refined paragraphs, oldest first.
    pub refines: VecDeque<RefineRow>,
    /// How many older paragraphs were dropped from `refines`.
    pub dropped_refines: usize,
    pub fallbacks: Vec<Fallback>,
    pub stats: Stats,
    orphans: Vec<Orphan>,
    last_event_ms: Option<u64>,
    last_partial_ms: Option<u64>,
}

impl Model {
    pub fn set_conn(&mut self, conn: Conn) {
        self.conn = conn;
    }

    /// The confirmed lines received so far, those dropped from the page included.
    pub fn final_total(&self) -> usize {
        self.dropped_finals + self.finals.len()
    }

    /// The refined paragraphs received so far, those dropped from the page included.
    pub fn refine_total(&self) -> usize {
        self.dropped_refines + self.refines.len()
    }

    /// Takes one event of the stream, which arrived at `now_ms`.
    pub fn apply(&mut self, event: UiEvent, now_ms: u64) {
        self.last_event_ms = Some(now_ms);
        match event {
            UiEvent::Partial { text } => self.on_partial(text, now_ms),
            UiEvent::Final(found) => self.on_final(found, now_ms),
            UiEvent::Speaker { id, speaker } => self.on_speaker(&id, speaker),
            UiEvent::Translation(found) => self.on_translation(found, now_ms),
            UiEvent::Refine(found) => self.on_refine(found, now_ms),
            UiEvent::Fallback(found) => self.on_fallback(found),
            UiEvent::Other => {}
        }
    }

    /// The time of the next change `expire` makes, if any is waiting.
    pub fn next_deadline(&self) -> Option<u64> {
        [
            self.overlay_deadline(),
            self.quiet_deadline(),
            self.orphan_deadline(),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    /// Makes every change whose time is `now_ms` or earlier.
    pub fn expire(&mut self, now_ms: u64) {
        if self.overlay_deadline().is_some_and(|due| due <= now_ms) {
            self.overlay.clear_final();
        }
        if self.quiet_deadline().is_some_and(|due| due <= now_ms) {
            self.strip_quiet = true;
        }
        self.orphans
            .retain(|held| held.at_ms + ORPHAN_HOLD_MS > now_ms);
    }

    fn overlay_deadline(&self) -> Option<u64> {
        self.last_event_ms
            .filter(|_| self.overlay.has_final())
            .map(|at| at + OVERLAY_HOLD_MS)
    }

    fn quiet_deadline(&self) -> Option<u64> {
        self.last_partial_ms
            .filter(|_| !self.strip_quiet && !self.strip_text.is_empty())
            .map(|at| at + STRIP_QUIET_MS)
    }

    fn orphan_deadline(&self) -> Option<u64> {
        self.orphans
            .iter()
            .map(|held| held.at_ms + ORPHAN_HOLD_MS)
            .min()
    }

    fn on_partial(&mut self, text: String, now_ms: u64) {
        self.overlay.partial_text.clone_from(&text);
        self.strip_text = text;
        self.strip_quiet = false;
        self.last_partial_ms = Some(now_ms);
    }

    fn on_final(&mut self, found: FinalEvent, now_ms: u64) {
        self.overlay = Overlay {
            final_id: found.id.clone(),
            final_text: found.text.clone(),
            partial_text: String::new(),
            final_tr: None,
        };
        self.partial_tr = None;
        self.stats.add(&found);
        push_capped(
            &mut self.finals,
            &mut self.dropped_finals,
            FinalRow {
                id: found.id.clone(),
                text: found.text,
                lang: found.lang,
                speaker: found.speaker,
                latency_ms: found.latency_ms,
                trs: Vec::new(),
                updated_ms: now_ms,
            },
        );
        self.adopt_orphans(Of::Final, found.id.as_deref(), now_ms);
    }

    fn on_refine(&mut self, found: RefineEvent, now_ms: u64) {
        push_capped(
            &mut self.refines,
            &mut self.dropped_refines,
            RefineRow {
                id: found.id.clone(),
                text: found.text,
                lang: found.lang,
                speaker: found.speaker,
                trs: Vec::new(),
            },
        );
        self.adopt_orphans(Of::Refine, found.id.as_deref(), now_ms);
    }

    /// The speaker of a line is found after the line was sent.
    fn on_speaker(&mut self, id: &str, speaker: String) {
        if let Some(row) = self
            .finals
            .iter_mut()
            .rev()
            .find(|row| row.id.as_deref() == Some(id))
        {
            row.speaker = speaker;
        }
    }

    fn on_translation(&mut self, found: TranslationEvent, now_ms: u64) {
        match found.of {
            Of::Partial => {
                self.tr_seen = true;
                self.partial_tr = partial_row(&found, self.partial_tr.as_ref());
            }
            Of::Final => {
                self.tr_seen = true;
                if !self.attach_to_final(&found, now_ms) {
                    self.hold(found, now_ms);
                }
            }
            Of::Refine => {
                if !self.attach_to_refine(&found) {
                    self.hold(found, now_ms);
                }
            }
        }
    }

    /// Joins a translation to its final by `id`. False when the line has not
    /// arrived; true otherwise, including a translation with no `id` to join by.
    /// The row's `updated_ms` becomes `now_ms` when the translation changes it.
    fn attach_to_final(&mut self, found: &TranslationEvent, now_ms: u64) -> bool {
        let Some(id) = found.id.as_deref() else {
            return true;
        };
        let Some(row) = self
            .finals
            .iter_mut()
            .rev()
            .find(|row| row.id.as_deref() == Some(id))
        else {
            return false;
        };
        let (shown, changed) = upsert(&mut row.trs, found);
        if changed {
            row.updated_ms = now_ms;
        }
        if self.overlay.final_id.as_deref() == Some(id) {
            self.overlay.final_tr = Some(shown);
        }
        true
    }

    fn attach_to_refine(&mut self, found: &TranslationEvent) -> bool {
        let Some(id) = found.id.as_deref() else {
            return true;
        };
        let Some(row) = self
            .refines
            .iter_mut()
            .rev()
            .find(|row| row.id.as_deref() == Some(id))
        else {
            return false;
        };
        upsert(&mut row.trs, found);
        true
    }

    fn hold(&mut self, event: TranslationEvent, now_ms: u64) {
        self.orphans.push(Orphan {
            at_ms: now_ms,
            event,
        });
    }

    /// Joins the translations that waited for the line `id` of kind `of`.
    fn adopt_orphans(&mut self, of: Of, id: Option<&str>, now_ms: u64) {
        let Some(id) = id else {
            return;
        };
        let (mine, rest): (Vec<Orphan>, Vec<Orphan>) = std::mem::take(&mut self.orphans)
            .into_iter()
            .partition(|held| held.event.of == of && held.event.id.as_deref() == Some(id));
        self.orphans = rest;
        for held in mine {
            self.on_translation(held.event, now_ms);
        }
    }

    fn on_fallback(&mut self, found: FallbackEvent) {
        let Some(to) = found.to else {
            self.fallbacks.retain(|held| held.stage != found.stage);
            return;
        };
        let entry = Fallback {
            stage: found.stage,
            reason: found.reason,
            to,
        };
        match self
            .fallbacks
            .iter_mut()
            .find(|held| held.stage == entry.stage)
        {
            Some(held) => *held = entry,
            None => self.fallbacks.push(entry),
        }
    }
}

/// Adds `row` as the newest and, past `ROW_LIMIT`, drops the oldest.
fn push_capped<T>(rows: &mut VecDeque<T>, dropped: &mut usize, row: T) {
    rows.push_back(row);
    if rows.len() > ROW_LIMIT {
        rows.pop_front();
        *dropped += 1;
    }
}

/// Sets the row of the event's language in `trs` and returns it, with whether
/// that changed `trs`.
fn upsert(trs: &mut Vec<TrRow>, found: &TranslationEvent) -> (TrRow, bool) {
    match trs.iter().position(|row| row.lang == found.lang) {
        Some(at) => {
            let row = settled_row(found, trs.get(at));
            let changed = trs[at] != row;
            trs[at] = row.clone();
            (row, changed)
        }
        None => {
            let row = settled_row(found, None);
            trs.push(row.clone());
            (row, true)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tr::TrState;
    use crate::wire::{Annotation, parse};

    fn event(data: &str) -> UiEvent {
        match parse(data) {
            Ok(found) => found,
            Err(error) => panic!("test event does not parse: {error}: {data}"),
        }
    }

    fn final_line(id: &str, text: &str) -> UiEvent {
        event(&format!(
            r#"{{"type":"final","id":"{id}","text":"{text}","lang":"ja","speaker":"","latency_ms":300.0}}"#
        ))
    }

    fn translation(of: &str, id: &str, text: &str, settled: bool) -> UiEvent {
        event(&format!(
            r#"{{"type":"translation","of":"{of}","id":"{id}","lang":"en","text":"{text}","status":"ok","settled":{settled}}}"#
        ))
    }

    #[test]
    fn the_overlay_keeps_a_final_for_six_seconds_after_the_last_message_of_any_kind() {
        let mut model = Model::default();
        model.apply(final_line("a-1", "こんにちは"), 1000);
        assert_eq!(model.overlay_deadline(), Some(7000));
        model.apply(event(r#"{"type":"partial","text":"次の"}"#), 5000);
        assert_eq!(model.overlay_deadline(), Some(11000));
        model.apply(event(r#"{"type":"session_start"}"#), 9000);
        assert_eq!(model.overlay_deadline(), Some(15000));
        model.expire(14999);
        assert_eq!(model.overlay.final_text, "こんにちは");
        model.expire(15000);
        assert_eq!(model.overlay.final_text, "");
        // The partial line is not on a timer.
        assert_eq!(model.overlay.partial_text, "次の");
        assert_eq!(model.overlay_deadline(), None);
    }

    #[test]
    fn a_final_empties_the_partial_line_and_the_translation_of_the_partial() {
        let mut model = Model::default();
        model.apply(event(r#"{"type":"partial","text":"こん"}"#), 0);
        model.apply(translation("partial", "", "hel", false), 10);
        assert!(model.partial_tr.is_some());
        model.apply(final_line("a-1", "こんにちは"), 20);
        assert_eq!(model.overlay.partial_text, "");
        assert_eq!(model.partial_tr, None);
        // The dashboard's strip keeps the last partial.
        assert_eq!(model.strip_text, "こん");
    }

    #[test]
    fn a_translation_joins_the_overlay_only_when_its_id_is_the_line_shown() {
        let mut model = Model::default();
        model.apply(final_line("a-1", "一"), 0);
        model.apply(final_line("a-2", "二"), 10);
        model.apply(translation("final", "a-1", "one", true), 20);
        assert_eq!(model.overlay.final_tr, None);
        model.apply(translation("final", "a-2", "two", true), 30);
        assert_eq!(
            model.overlay.final_tr.as_ref().map(|row| row.text.as_str()),
            Some("two")
        );
        assert_eq!(model.finals[0].trs[0].text, "one");
        assert!(model.tr_seen);
    }

    #[test]
    fn a_draft_is_replaced_once_by_the_deciding_translation_in_the_same_row() {
        let mut model = Model::default();
        model.apply(final_line("a-1", "一"), 0);
        model.apply(translation("final", "a-1", "early", false), 10);
        assert_eq!(model.finals[0].trs[0].state, TrState::First);
        model.apply(translation("final", "a-1", "better", true), 20);
        assert_eq!(model.finals[0].trs.len(), 1);
        assert_eq!(model.finals[0].trs[0].state, TrState::Confirmed);
        assert_eq!(model.finals[0].trs[0].text, "better");
        assert_eq!(
            model.overlay.final_tr.as_ref().map(|row| row.text.as_str()),
            Some("better")
        );
    }

    #[test]
    fn a_translation_that_comes_before_its_line_waits_five_seconds() {
        let mut model = Model::default();
        model.apply(translation("final", "a-1", "one", true), 1000);
        assert_eq!(model.next_deadline(), Some(6000));
        model.apply(final_line("a-1", "一"), 2000);
        assert_eq!(model.finals[0].trs[0].text, "one");
        assert_eq!(model.next_deadline(), Some(8000));

        let mut late = Model::default();
        late.apply(translation("final", "a-9", "lost", true), 1000);
        late.expire(6000);
        late.apply(final_line("a-9", "九"), 6001);
        assert!(late.finals[0].trs.is_empty());
    }

    #[test]
    fn a_translation_with_no_id_is_not_stuck_to_the_line_before() {
        let mut model = Model::default();
        model.apply(final_line("a-1", "一"), 0);
        model.apply(
            event(r#"{"type":"translation","lang":"en","text":"hi"}"#),
            10,
        );
        assert!(model.finals[0].trs.is_empty());
        // Nothing waits for a line: only the overlay's six seconds are running.
        assert_eq!(model.orphan_deadline(), None);
    }

    #[test]
    fn a_translation_of_a_refined_paragraph_joins_the_paragraph() {
        let mut model = Model::default();
        model.apply(
            event(r#"{"type":"refine","id":"a-1","text":"段落","lang":"ja","speaker":"S1"}"#),
            0,
        );
        model.apply(translation("refine", "a-1", "paragraph", true), 10);
        assert_eq!(model.refines[0].trs[0].text, "paragraph");
        assert!(!model.tr_seen);
    }

    #[test]
    fn a_speaker_found_later_is_written_into_the_line() {
        let mut model = Model::default();
        model.apply(final_line("a-1", "一"), 0);
        model.apply(event(r#"{"type":"speaker","id":"a-1","speaker":"S2"}"#), 5);
        assert_eq!(model.finals[0].speaker, "S2");
        model.apply(event(r#"{"type":"speaker","id":"zz","speaker":"S9"}"#), 6);
        assert_eq!(model.finals.len(), 1);
    }

    #[test]
    fn a_fallback_is_replaced_by_stage_and_ended_by_a_null_to() {
        let mut model = Model::default();
        model.apply(
            event(r#"{"type":"fallback","stage":"punct","reason":"no_device","to":"cpu"}"#),
            0,
        );
        model.apply(
            event(r#"{"type":"fallback","stage":"sink","to":"local"}"#),
            1,
        );
        model.apply(
            event(r#"{"type":"fallback","stage":"punct","reason":"oom","to":"cpu"}"#),
            2,
        );
        assert_eq!(model.fallbacks.len(), 2);
        assert_eq!(model.fallbacks[0].reason.as_deref(), Some("oom"));
        model.apply(event(r#"{"type":"fallback","stage":"punct","to":null}"#), 3);
        assert_eq!(model.fallbacks.len(), 1);
        assert_eq!(model.fallbacks[0].stage, "sink");
        // Ending a fallback that is not in effect changes nothing.
        model.apply(
            event(r#"{"type":"fallback","stage":"nothing","to":null}"#),
            4,
        );
        assert_eq!(model.fallbacks.len(), 1);
    }

    #[test]
    fn the_strip_fades_four_seconds_after_the_last_partial_and_wakes_on_the_next() {
        let mut model = Model::default();
        assert_eq!(model.next_deadline(), None);
        model.apply(event(r#"{"type":"partial","text":"a"}"#), 100);
        assert_eq!(model.next_deadline(), Some(4100));
        model.expire(4100);
        assert!(model.strip_quiet);
        assert_eq!(model.quiet_deadline(), None);
        model.apply(event(r#"{"type":"partial","text":"ab"}"#), 5000);
        assert!(!model.strip_quiet);
    }

    #[test]
    fn the_header_numbers_follow_the_finals() {
        let mut model = Model::default();
        assert_eq!(model.stats.mean_latency_ms(), None);
        model.apply(final_line("a-1", "一"), 0);
        model.apply(
            event(r#"{"type":"final","id":"a-2","text":"x","lang":"en","latency_ms":null}"#),
            1,
        );
        model.apply(
            event(r#"{"type":"final","id":"a-3","text":"y","lang":"en","latency_ms":401.0}"#),
            2,
        );
        assert_eq!(model.stats.finals, 3);
        // The mean is of the finals that carry a latency: (300 + 401) / 2.
        assert_eq!(model.stats.mean_latency_ms(), Some(351));
        assert_eq!(model.stats.langs, ["ja", "en"]);
    }

    #[test]
    fn a_translation_of_a_partial_settles_the_prefix_shared_with_the_one_before() {
        let mut model = Model::default();
        model.apply(translation("partial", "", "予算は五百万", false), 0);
        assert_eq!(
            model.partial_tr.as_ref().map(|row| row.state.clone()),
            Some(TrState::Partial { confirmed: 0 })
        );
        model.apply(translation("partial", "", "予算は五百万円を想定", false), 1);
        assert_eq!(
            model.partial_tr.as_ref().map(|row| row.state.clone()),
            Some(TrState::Partial { confirmed: 6 })
        );
    }

    fn refine_line(id: &str) -> UiEvent {
        event(&format!(
            r#"{{"type":"refine","id":"{id}","text":"段落 {id}","lang":"ja","speaker":"S1"}}"#
        ))
    }

    /// W7: the 501st line leaves 500 on the page, drops the oldest, and counts it.
    #[test]
    fn the_501st_confirmed_line_drops_the_oldest_and_keeps_the_header_count() {
        let mut model = Model::default();
        for number in 0..=ROW_LIMIT {
            model.apply(final_line(&format!("a-{number}"), "行"), number as u64);
        }
        assert_eq!(model.finals.len(), ROW_LIMIT);
        assert_eq!(model.dropped_finals, 1);
        assert_eq!(model.finals[0].id.as_deref(), Some("a-1"));
        assert_eq!(
            model.finals.back().and_then(|row| row.id.as_deref()),
            Some("a-500")
        );
        // The header counts every line, the dropped one included.
        assert_eq!(model.stats.finals, ROW_LIMIT + 1);
        assert_eq!(model.final_total(), ROW_LIMIT + 1);
    }

    #[test]
    fn the_500th_confirmed_line_drops_nothing() {
        let mut model = Model::default();
        for number in 0..ROW_LIMIT {
            model.apply(final_line(&format!("a-{number}"), "行"), number as u64);
        }
        assert_eq!(model.finals.len(), ROW_LIMIT);
        assert_eq!(model.dropped_finals, 0);
    }

    #[test]
    fn the_501st_refined_paragraph_drops_the_oldest_and_the_finals_are_untouched() {
        let mut model = Model::default();
        model.apply(final_line("keep", "残る"), 0);
        for number in 0..=ROW_LIMIT {
            model.apply(refine_line(&format!("r-{number}")), 1 + number as u64);
        }
        assert_eq!(model.refines.len(), ROW_LIMIT);
        assert_eq!(model.dropped_refines, 1);
        assert_eq!(model.refines[0].id.as_deref(), Some("r-1"));
        assert_eq!(model.refine_total(), ROW_LIMIT + 1);
        // A dropped line is not moved to the other column.
        assert_eq!(model.finals.len(), 1);
        assert_eq!(model.dropped_finals, 0);
    }

    #[test]
    fn a_translation_of_a_dropped_line_waits_as_an_orphan_and_changes_no_row() {
        let mut model = Model::default();
        for number in 0..=ROW_LIMIT {
            model.apply(final_line(&format!("a-{number}"), "行"), number as u64);
        }
        model.apply(translation("final", "a-0", "gone", true), 600);
        assert!(model.finals.iter().all(|row| row.trs.is_empty()));
        assert_eq!(model.orphan_deadline(), Some(600 + ORPHAN_HOLD_MS));
    }

    #[test]
    fn a_row_is_stamped_when_its_line_arrives_and_when_a_translation_changes_it() {
        let mut model = Model::default();
        model.apply(final_line("a-1", "一"), 1000);
        assert_eq!(model.finals[0].updated_ms, 1000);
        model.apply(translation("final", "a-1", "early", false), 1500);
        assert_eq!(model.finals[0].updated_ms, 1500);
        model.apply(translation("final", "a-1", "better", true), 2100);
        assert_eq!(model.finals[0].updated_ms, 2100);
    }

    #[test]
    fn a_translation_that_leaves_the_row_as_it_was_does_not_move_the_stamp() {
        let mut model = Model::default();
        model.apply(final_line("a-1", "一"), 1000);
        model.apply(translation("final", "a-1", "better", true), 2000);
        model.apply(translation("final", "a-1", "better", true), 5000);
        assert_eq!(model.finals[0].updated_ms, 2000);
        // A draft after the deciding translation changes nothing either.
        model.apply(translation("final", "a-1", "early", false), 6000);
        assert_eq!(model.finals[0].updated_ms, 2000);
    }

    #[test]
    fn a_translation_that_waited_for_its_line_is_stamped_when_it_joined() {
        let mut model = Model::default();
        model.apply(translation("final", "a-1", "one", true), 1000);
        model.apply(final_line("a-1", "一"), 2000);
        assert_eq!(model.finals[0].trs[0].text, "one");
        assert_eq!(model.finals[0].updated_ms, 2000);
    }

    #[test]
    fn only_the_row_a_message_changes_is_stamped() {
        let mut model = Model::default();
        model.apply(final_line("a-1", "一"), 1000);
        model.apply(final_line("a-2", "二"), 1100);
        model.apply(
            event(r#"{"type":"speaker","id":"a-1","speaker":"S2"}"#),
            1200,
        );
        model.apply(translation("final", "a-2", "two", true), 1300);
        assert_eq!(model.finals[0].updated_ms, 1000);
        assert_eq!(model.finals[1].updated_ms, 1300);
    }

    #[test]
    fn an_annotation_arrives_with_the_row() {
        let mut model = Model::default();
        model.apply(final_line("a-1", "一"), 0);
        model.apply(
            event(r#"{"type":"translation","of":"final","id":"a-1","lang":"en","text":"$100","settled":true,"annotations":[{"start":0,"end":4,"text":" (15000 yen)"}]}"#),
            1,
        );
        assert_eq!(
            model.finals[0].trs[0].annotations,
            [Annotation {
                end: 4,
                text: String::from(" (15000 yen)")
            }]
        );
    }
}

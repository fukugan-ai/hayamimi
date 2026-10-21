//! The refine pass of a live run: the finished utterances are gathered into
//! groups, and each group is read again, in order, once the speaker pauses.
//!
//! This part decides what a group is. An utterance joins the open group; the
//! group is closed when the language changes, when two seconds of silence
//! have passed, when it has run twenty-five seconds, and when the session
//! resets or ends. A closed group carries the audio that is still held, so
//! the second reading has what the first one heard.

use hayamimi_asr::Final;
use hayamimi_asr::audio::{History, PREROLL_SECONDS, SAMPLE_RATE};
use hayamimi_asr::lid::script_corrected_lang;
use hayamimi_asr::refine::{GROUP_GAP_SECONDS, GROUP_MAX_SECONDS, Span, majority_label};

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

#[cfg(test)]
mod tests {
    use super::*;
    use hayamimi_asr::refine::group_ranges;

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
}

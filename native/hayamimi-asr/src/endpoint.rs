//! The end of an utterance: which rule ended it, and whether a sentence read
//! at the early detector's notice is complete enough to be finalised then.
//!
//! The tables are constants. A Japanese sentence is complete when it ends in
//! 。 ? ! (or their full-width forms), the form before the mark is a sentence
//! ending and not a connective. An English sentence is complete when it ends
//! in . ? ! and its last word does not go on. Other languages are never
//! judged, so an early candidate in them is not used. A complete sentence is
//! finalised at an early notice only when the speech behind it is long enough.
//!
//! The form a Japanese sentence ends in is classified as well ([`Tail`]). A
//! shadow rule that counts a noun or an honorific ending as a sentence end is
//! evaluated beside the live rule ([`shadow_early_hold`]). The shadow rule is
//! only logged; it never decides anything.

use crate::audio::{SAMPLE_RATE, WINDOW};
use crate::vad::EARLY_MIN_SEGMENT_MS;

/// Sentence endings that can close a Japanese sentence.
const JA_FINAL: [&str; 17] = [
    "です",
    "ます",
    "ました",
    "でした",
    "ません",
    "ない",
    "なかった",
    "ている",
    "ていた",
    "た",
    "だ",
    "ね",
    "よ",
    "か",
    "う",
    "る",
    "い",
];

/// Connective forms: a Japanese sentence that ends in one of these goes on.
const JA_CONNECTIVE: [&str; 14] = [
    "が",
    "けど",
    "けれど",
    "けれども",
    "て",
    "で",
    "し",
    "から",
    "ので",
    "のに",
    "ば",
    "たら",
    "と",
    "ながら",
];

/// Honorifics a Japanese name ends in.
const JA_HONORIFIC: [&str; 3] = ["さん", "様", "氏"];

/// Fillers and interjections. They are read before the particles: `あの` ends
/// in the particle `の`.
const JA_INTERJECTION: [&str; 14] = [
    "さあ",
    "ああ",
    "ええ",
    "まあ",
    "うん",
    "はい",
    "いいえ",
    "えっと",
    "えーと",
    "あのう",
    "あの",
    "うーん",
    "おお",
    "へえ",
];

/// Particles a Japanese sentence can stop on, besides [`JA_CONNECTIVE`].
const JA_PARTICLE: [&str; 10] = [
    "を", "も", "は", "に", "の", "へ", "や", "など", "まで", "より",
];

/// Words an English sentence does not end on.
const EN_CONTINUATION: [&str; 22] = [
    "and", "but", "so", "because", "the", "a", "an", "to", "of", "or", "that", "with", "for", "in",
    "on", "at", "by", "if", "when", "as", "than", "then",
];

/// How the end of an utterance is decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The early detector may finalise a complete sentence before the silence rule.
    Early,
    /// Only the silence rule of the main detector ends an utterance.
    Silence,
}

/// The rule that ended one utterance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// A complete sentence at a notice of the early detector.
    Early,
    /// The main detector's silence.
    Silence,
    /// The main detector's limit on continuous speech.
    MaxSpeech,
    /// The speech in progress was cut by a flush.
    Flush,
}

impl Rule {
    /// The name the log gives the rule.
    pub fn name(self) -> &'static str {
        match self {
            Rule::Early => "early",
            Rule::Silence => "silence",
            Rule::MaxSpeech => "max_speech",
            Rule::Flush => "flush",
        }
    }
}

/// What ended an utterance, for the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decision {
    pub rule: Rule,
    /// The silence the rule waited for, in milliseconds; 0 for a limit or a flush.
    pub silence_ms: u32,
    /// How the main detector's segment was turned into text; `None` for a
    /// sentence the early detector finalised itself.
    pub plan: Option<PlanKind>,
    /// When the segment was read again although a sentence had been kept:
    /// how far the end of that sentence's range was from the segment's end,
    /// in milliseconds, positive when the range ended after the segment did.
    /// `None` when nothing was kept, or the kept one was used.
    pub held_gap_ms: Option<i32>,
}

/// How a segment of the main detector was turned into text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanKind {
    /// The sentence the early detector kept was used as it was.
    Held,
    /// The whole segment was read.
    Decode,
    /// Only what follows the line the early detector finalised was read.
    Rest,
}

impl PlanKind {
    /// The name the log gives the kind.
    pub fn name(self) -> &'static str {
        match self {
            PlanKind::Held => "held",
            PlanKind::Decode => "decode",
            PlanKind::Rest => "rest",
        }
    }
}

/// How far the end of a kept sentence's range was from the end of the segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gap {
    /// The range ended this many samples before the segment did.
    Short(usize),
    /// The range ended this many samples after the segment did (0: with it).
    Over(usize),
}

impl Gap {
    fn between(range_end: usize, seg_end: usize) -> Gap {
        if range_end < seg_end {
            Gap::Short(seg_end - range_end)
        } else {
            Gap::Over(range_end - seg_end)
        }
    }

    /// The gap in milliseconds, rounded to the nearest and negative when the
    /// range ended before the segment did. `None` when it does not fit.
    pub fn signed_ms(self) -> Option<i32> {
        let rate = SAMPLE_RATE as usize;
        let millis = |samples: usize| i32::try_from((samples * 1000 + rate / 2) / rate).ok();
        match self {
            Gap::Over(samples) => millis(samples),
            Gap::Short(samples) => millis(samples).map(|ms| -ms),
        }
    }
}

/// How far the end of a kept sentence's range may be from the end of the
/// segment, in samples, for the sentence to be used as the segment's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reach {
    /// The most the range may end before the segment does.
    pub behind: usize,
    /// The most the range may end after the segment does.
    pub ahead: usize,
}

impl Reach {
    /// One window either way: the range ends where the segment does.
    pub const ONE_WINDOW: Reach = Reach {
        behind: WINDOW,
        ahead: WINDOW,
    };

    fn holds(self, gap: Gap) -> bool {
        match gap {
            Gap::Short(samples) => samples <= self.behind,
            Gap::Over(samples) => samples <= self.ahead,
        }
    }
}

/// How far the end of a kept sentence's range may be from the end of the
/// segment, either way.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HeldReuse {
    /// One window.
    Strict,
    /// Also as far as the silence the closing rule waited for: the detector
    /// counts that silence as no voice, so the part of the segment the
    /// sentence did not read holds no speech, and the part of the sentence
    /// past the segment holds none either.
    #[default]
    Covering,
}

/// The reach of a kept sentence for a segment that `decision` closed.
pub fn reach_for(reuse: HeldReuse, decision: &Decision) -> Reach {
    match reuse {
        HeldReuse::Strict => Reach::ONE_WINDOW,
        HeldReuse::Covering => {
            let silence = silence_samples(decision);
            Reach {
                behind: Reach::ONE_WINDOW.behind.max(silence),
                ahead: Reach::ONE_WINDOW.ahead.max(silence),
            }
        }
    }
}

/// The samples of the silence the rule that closed the segment waited for:
/// none for a limit or a flush.
fn silence_samples(decision: &Decision) -> usize {
    if decision.rule == Rule::Silence {
        decision.silence_ms as usize * SAMPLE_RATE as usize / 1000
    } else {
        0
    }
}

/// The text a kept sentence is the final with, for a segment that `decision`
/// closed. The silence rule ended the sentence, and the reading of the whole
/// segment puts the terminal mark there; the kept sentence was read before that
/// silence was over, so a pause mark at its end becomes the terminal mark of
/// its language. Only `covering` does this, and only for the silence rule:
/// `strict` and the closes by a limit or a flush keep the text as it was read.
pub fn closing_text(reuse: HeldReuse, decision: &Decision, lang: &str, text: &str) -> String {
    if reuse != HeldReuse::Covering || decision.rule != Rule::Silence {
        return text.to_string();
    }
    let (pauses, terminal): (&[char], char) = match lang {
        "en" => (&[',', ';', ':'], '.'),
        "ja" => (&['、', '，'], '。'),
        _ => return text.to_string(),
    };
    let body = text.trim_end();
    if !body.ends_with(pauses) {
        return text.to_string();
    }
    format!("{}{terminal}", body.trim_end_matches(pauses))
}

/// `decision` with the way the segment was read: the kind of `plan`, and, when
/// the segment was read although a sentence had been kept (`gap` between the
/// ends), that gap. A plan that makes no final names no kind.
pub fn note_plan<T>(decision: Decision, plan: &Plan<T>, gap: Option<Gap>) -> Decision {
    let plan_kind = match plan {
        Plan::Whole => Some(PlanKind::Decode),
        Plan::Rest(_) => Some(PlanKind::Rest),
        Plan::Held(..) => Some(PlanKind::Held),
        Plan::Drop => None,
    };
    let reread = matches!(plan_kind, Some(PlanKind::Decode | PlanKind::Rest));
    let held_gap_ms = gap.filter(|_| reread).and_then(Gap::signed_ms);
    Decision {
        plan: plan_kind,
        held_gap_ms,
        ..decision
    }
}

/// A confirmed sentence inside the voice the main detector is still in: the
/// sample the text was finalised at, and where the early detector's stretch of
/// voice ended (any later stretch of voice starts after it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Line {
    pub at: usize,
    pub floor: usize,
}

/// The sentence read at a candidate that was not complete, kept so that it is
/// not read again when the main detector ends the utterance where it stands.
#[derive(Debug, Clone, PartialEq)]
pub struct Held<T> {
    pub solved: T,
    /// The sample the audio was read up to.
    pub range_end: usize,
    /// Where the early detector's stretch of voice ended.
    pub floor: usize,
    /// Whether the early detector read another stretch of voice since.
    pub voice_after: bool,
}

/// What to do with a segment the main detector hands over.
#[derive(Debug, Clone, PartialEq)]
pub enum Plan<T> {
    /// Read it whole, as without the early detector.
    Whole,
    /// Read only what follows the line, as one segment starting there.
    Rest(usize),
    /// Everything in it is finalised already, or is silence: make no final.
    Drop,
    /// Use the sentence that was kept; the segment starts at the sample given.
    Held(Held<T>, usize),
}

/// The early path's memory between two windows.
#[derive(Debug, Clone, PartialEq)]
pub struct State<T> {
    line: Option<Line>,
    line_voice_after: bool,
    held: Option<Held<T>>,
}

impl<T> Default for State<T> {
    fn default() -> Self {
        State {
            line: None,
            line_voice_after: false,
            held: None,
        }
    }
}

impl<T> State<T> {
    /// The line the voice in progress has been finalised up to.
    pub fn line(&self) -> Option<Line> {
        self.line
    }

    /// The early detector finished a stretch of voice that starts at `start`.
    pub fn note_voice(&mut self, start: usize) {
        if self.line.is_some_and(|line| start >= line.floor) {
            self.line_voice_after = true;
        }
        if let Some(held) = self.held.as_mut().filter(|held| start >= held.floor) {
            held.voice_after = true;
        }
    }

    /// A complete sentence was finalised at `at`; nothing kept before it is of use.
    pub fn confirm(&mut self, at: usize, floor: usize) {
        self.line = Some(Line { at, floor });
        self.line_voice_after = false;
        self.held = None;
    }

    /// A sentence that is not complete was read up to `range_end`.
    pub fn hold(&mut self, solved: T, range_end: usize, floor: usize) {
        self.held = Some(Held {
            solved,
            range_end,
            floor,
            voice_after: false,
        });
    }

    /// Forgets the line and the kept sentence.
    pub fn clear(&mut self) {
        *self = State::default();
    }

    /// What to do with the segment `seg_start..seg_end` of the main detector.
    /// The state is cleared: the next utterance starts afresh. `reach` is how
    /// far the end of the kept sentence's range may be from the segment's end.
    pub fn plan(&mut self, seg_start: usize, seg_end: usize, reach: Reach) -> Plan<T> {
        self.plan_with_gap(seg_start, seg_end, reach).0
    }

    /// [`State::plan`], and how far the end of the kept sentence's range was
    /// from `seg_end` (`None` when nothing was kept), whether or not the
    /// sentence was used.
    pub fn plan_with_gap(
        &mut self,
        seg_start: usize,
        seg_end: usize,
        reach: Reach,
    ) -> (Plan<T>, Option<Gap>) {
        let line = self.line.take();
        let voice_after = std::mem::take(&mut self.line_voice_after);
        let kept = self.held.take();
        let gap = kept
            .as_ref()
            .map(|held| Gap::between(held.range_end, seg_end));
        let held = kept.filter(|held| !held.voice_after && gap.is_some_and(|gap| reach.holds(gap)));
        let plan = match line {
            Some(line) if line.at > seg_start => {
                if seg_end <= line.at || !voice_after {
                    Plan::Drop
                } else if let Some(held) = held {
                    Plan::Held(held, line.at)
                } else {
                    Plan::Rest(line.at)
                }
            }
            _ => match held {
                Some(held) => Plan::Held(held, seg_start),
                None => Plan::Whole,
            },
        };
        (plan, gap)
    }
}

fn is_terminal(ch: char) -> bool {
    matches!(ch, '。' | '？' | '！' | '.' | '?' | '!')
}

fn ja_complete(text: &str) -> bool {
    let trimmed = text.trim_end();
    if !trimmed.chars().last().is_some_and(is_terminal) {
        return false;
    }
    let body = trimmed.trim_end_matches(is_terminal);
    !JA_CONNECTIVE.iter().any(|tail| body.ends_with(tail))
        && JA_FINAL.iter().any(|tail| body.ends_with(tail))
}

fn en_complete(text: &str) -> bool {
    let trimmed = text.trim_end();
    if !trimmed.chars().last().is_some_and(is_terminal) {
        return false;
    }
    trimmed
        .trim_end_matches(is_terminal)
        .split_whitespace()
        .next_back()
        .map(str::to_lowercase)
        .is_some_and(|word| !EN_CONTINUATION.contains(&word.as_str()))
}

/// Whether `text`, read in `lang`, is a complete sentence. Only ja and en are judged.
pub fn is_complete(lang: &str, text: &str) -> bool {
    match lang {
        "ja" => ja_complete(text),
        "en" => en_complete(text),
        _ => false,
    }
}

/// The shortest speech an early notice may finalise, in samples.
const EARLY_MIN_SAMPLES: usize = EARLY_MIN_SEGMENT_MS as usize * SAMPLE_RATE as usize / 1000;

/// Why an early notice kept the sentence it read instead of finalising it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hold {
    /// The sentence is not complete.
    Incomplete,
    /// The sentence is complete, but the speech behind it is shorter than
    /// [`EARLY_MIN_SEGMENT_MS`]: a short pause inside a word or a phrase ends
    /// a stretch of voice that is not yet a sentence.
    TooShort,
}

impl Hold {
    /// The name the log gives the reason.
    pub fn name(self) -> &'static str {
        match self {
            Hold::Incomplete => "incomplete",
            Hold::TooShort => "too_short",
        }
    }
}

/// Whether a sentence read in `lang` from `samples` samples of speech is kept
/// at an early notice. `None` means it is finalised there. The silence rule of
/// the main detector does not ask.
pub fn early_hold(lang: &str, text: &str, samples: usize) -> Option<Hold> {
    hold_for(is_complete(lang, text), samples)
}

/// The hold of a sentence judged `complete`, read from `samples` samples of speech.
fn hold_for(complete: bool, samples: usize) -> Option<Hold> {
    if !complete {
        return Some(Hold::Incomplete);
    }
    (!long_enough_to_close(samples)).then_some(Hold::TooShort)
}

/// Whether `samples` samples of speech are enough for an early notice to
/// finalise a sentence read from them.
pub fn long_enough_to_close(samples: usize) -> bool {
    samples >= EARLY_MIN_SAMPLES
}

/// The form a sentence ends in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tail {
    /// A kanji, a katakana or a letter or digit of the ASCII or full-width forms.
    Noun,
    /// `さん`, `様` or `氏`.
    Honorific,
    /// A particle or a connective form.
    Particle,
    /// A filler or an interjection.
    Interjection,
    /// Any other form, and every language but Japanese.
    Other,
}

impl Tail {
    /// The name the log gives the form.
    pub fn name(self) -> &'static str {
        match self {
            Tail::Noun => "noun",
            Tail::Honorific => "honorific",
            Tail::Particle => "particle",
            Tail::Interjection => "interjection",
            Tail::Other => "other",
        }
    }

    /// Whether the shadow rule counts a Japanese sentence that ends so as complete.
    pub fn ends_a_sentence(self) -> bool {
        matches!(self, Tail::Noun | Tail::Honorific)
    }
}

/// Whether a character ends a noun the recognizer wrote: a kanji, a katakana
/// (the long-vowel mark included), or a letter or digit.
fn is_noun_char(ch: char) -> bool {
    matches!(ch,
        '\u{3400}'..='\u{4DBF}'
        | '\u{4E00}'..='\u{9FFF}'
        | '々'
        | '\u{30A1}'..='\u{30FA}'
        | '\u{30FC}'..='\u{30FE}'
        | '0'..='9'
        | 'a'..='z'
        | 'A'..='Z'
        | '０'..='９'
        | 'ａ'..='ｚ'
        | 'Ａ'..='Ｚ')
}

fn ja_tail(body: &str) -> Tail {
    let ends = |tails: &[&str]| tails.iter().any(|tail| body.ends_with(tail));
    if ends(&JA_HONORIFIC) {
        Tail::Honorific
    } else if ends(&JA_INTERJECTION) {
        Tail::Interjection
    } else if ends(&JA_PARTICLE) || ends(&JA_CONNECTIVE) {
        Tail::Particle
    } else if body.chars().next_back().is_some_and(is_noun_char) {
        Tail::Noun
    } else {
        Tail::Other
    }
}

/// The form `text`, read in `lang`, ends in, judged on what stands before the
/// closing mark. Only Japanese is classified.
pub fn tail_of(lang: &str, text: &str) -> Tail {
    if lang != "ja" {
        return Tail::Other;
    }
    ja_tail(text.trim_end().trim_end_matches(is_terminal).trim_end())
}

/// Whether the shadow rule counts `text` as complete: the live rule, or a
/// Japanese sentence under a closing mark that ends in a noun or an honorific.
fn shadow_complete(lang: &str, text: &str) -> bool {
    is_complete(lang, text)
        || (lang == "ja"
            && text.trim_end().chars().next_back().is_some_and(is_terminal)
            && tail_of(lang, text).ends_a_sentence())
}

/// What [`early_hold`] would answer if a noun or an honorific ending closed a
/// Japanese sentence. The live rule does not ask; the log records the answer.
pub fn shadow_early_hold(lang: &str, text: &str, samples: usize) -> Option<Hold> {
    hold_for(shadow_complete(lang, text), samples)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_japanese_sentence_is_complete_with_a_sentence_ending_and_no_connective() {
        assert!(is_complete("ja", "今日は雨です。"));
        assert!(is_complete("ja", "本当ですか？"));
        assert!(!is_complete("ja", "雨が降っていますが。"));
        assert!(!is_complete("ja", "今日は雨です"));
        assert!(!is_complete("ja", "そして、"));
        assert!(!is_complete("ja", ""));
    }

    #[test]
    fn an_english_sentence_is_complete_when_its_last_word_does_not_go_on() {
        assert!(is_complete("en", "It is raining."));
        assert!(is_complete("en", "Is it raining?"));
        assert!(!is_complete("en", "It is raining and."));
        assert!(!is_complete("en", "It is raining"));
        assert!(!is_complete("en", "."));
    }

    #[test]
    fn a_language_outside_ja_and_en_is_never_complete() {
        assert!(!is_complete("zh", "今天下雨了。"));
        assert!(!is_complete("", "It is raining."));
    }

    const TOLERANCE: usize = WINDOW;
    const REACH: Reach = Reach::ONE_WINDOW;

    #[test]
    fn a_segment_with_nothing_kept_is_read_whole() {
        let mut state: State<&str> = State::default();
        assert_eq!(state.plan(0, 1000, REACH), Plan::Whole);
    }

    #[test]
    fn a_segment_after_a_line_with_no_voice_after_it_makes_no_final() {
        let mut state: State<&str> = State::default();
        state.confirm(800, 700);
        assert_eq!(state.plan(0, 1200, REACH), Plan::Drop);
        assert_eq!(state.line(), None);
    }

    #[test]
    fn voice_after_the_line_makes_the_rest_one_segment() {
        let mut state: State<&str> = State::default();
        state.confirm(800, 700);
        state.note_voice(900);
        assert_eq!(state.plan(0, 2000, REACH), Plan::Rest(800));
    }

    #[test]
    fn voice_that_starts_before_the_floor_is_not_voice_after() {
        let mut state: State<&str> = State::default();
        state.confirm(800, 700);
        state.note_voice(699);
        assert_eq!(state.plan(0, 2000, REACH), Plan::Drop);
    }

    #[test]
    fn a_segment_that_starts_after_the_line_is_the_next_utterance() {
        let mut state: State<&str> = State::default();
        state.confirm(800, 700);
        assert_eq!(state.plan(900, 2000, REACH), Plan::Whole);
        assert_eq!(state.line(), None);
    }

    #[test]
    fn a_kept_sentence_is_used_when_the_segment_ends_within_a_window_of_its_range() {
        let mut state: State<&str> = State::default();
        state.hold("kept", 1000, 900);
        assert_eq!(
            state.plan(100, 1000 + TOLERANCE, REACH),
            Plan::Held(
                Held {
                    solved: "kept",
                    range_end: 1000,
                    floor: 900,
                    voice_after: false
                },
                100
            )
        );
    }

    #[test]
    fn a_kept_sentence_is_not_used_when_the_range_differs_or_voice_came_after() {
        let mut far: State<&str> = State::default();
        far.hold("kept", 1000, 900);
        assert_eq!(far.plan(100, 1000 + TOLERANCE + 1, REACH), Plan::Whole);
        let mut later: State<&str> = State::default();
        later.hold("kept", 1000, 900);
        later.note_voice(900);
        assert_eq!(later.plan(100, 1000, REACH), Plan::Whole);
    }

    #[test]
    fn a_kept_sentence_after_a_line_is_the_rest() {
        let mut state: State<&str> = State::default();
        state.confirm(800, 700);
        state.note_voice(900);
        state.hold("rest", 1500, 1400);
        assert!(matches!(state.plan(0, 1500, REACH), Plan::Held(_, 800)));
    }

    #[test]
    fn clearing_forgets_the_line_and_the_kept_sentence() {
        let mut state: State<&str> = State::default();
        state.confirm(800, 700);
        state.hold("kept", 1000, 900);
        state.clear();
        assert_eq!(state, State::default());
    }

    // Speech lengths at 16 kHz: 999 ms, 1000 ms and 1001 ms.
    const MS_999: usize = 15_984;
    const MS_1000: usize = 16_000;
    const MS_1001: usize = 16_016;

    #[test]
    fn a_complete_sentence_is_kept_below_one_second_and_finalised_from_one_second() {
        for (lang, text) in [("ja", "言ったろう。"), ("en", "It is raining.")] {
            assert_eq!(
                early_hold(lang, text, MS_999),
                Some(Hold::TooShort),
                "{lang}"
            );
            assert_eq!(early_hold(lang, text, MS_1000), None, "{lang}");
            assert_eq!(early_hold(lang, text, MS_1001), None, "{lang}");
        }
    }

    #[test]
    fn a_sentence_that_is_not_complete_is_kept_whatever_its_length() {
        for samples in [0, MS_999, MS_1000, MS_1001, 10 * MS_1000] {
            assert_eq!(
                early_hold("ja", "雨が降っていますが。", samples),
                Some(Hold::Incomplete)
            );
            assert_eq!(
                early_hold("zh", "今天下雨了。", samples),
                Some(Hold::Incomplete)
            );
        }
    }

    #[test]
    fn a_complete_sentence_with_no_speech_is_too_short() {
        assert_eq!(early_hold("ja", "はい。", 0), Some(Hold::TooShort));
    }

    #[test]
    fn holds_are_named_for_the_log() {
        assert_eq!(Hold::Incomplete.name(), "incomplete");
        assert_eq!(Hold::TooShort.name(), "too_short");
    }

    fn silence() -> Decision {
        Decision {
            rule: Rule::Silence,
            silence_ms: 300,
            plan: None,
            held_gap_ms: None,
        }
    }

    fn kept(range_end: usize) -> Held<&'static str> {
        Held {
            solved: "kept",
            range_end,
            floor: 0,
            voice_after: false,
        }
    }

    #[test]
    fn the_held_path_and_the_decode_path_are_named_apart() {
        let held = note_plan(silence(), &Plan::Held(kept(1000), 0), Some(Gap::Over(0)));
        let decode = note_plan(silence(), &Plan::<&str>::Whole, Some(Gap::Short(4000)));
        assert_eq!(held.plan, Some(PlanKind::Held));
        assert_eq!(decode.plan, Some(PlanKind::Decode));
        assert_ne!(
            held.plan.map(PlanKind::name),
            decode.plan.map(PlanKind::name)
        );
        let rest = note_plan(silence(), &Plan::<&str>::Rest(800), None);
        assert_eq!(rest.plan.map(PlanKind::name), Some("rest"));
        assert_eq!(held.plan.map(PlanKind::name), Some("held"));
        assert_eq!(decode.plan.map(PlanKind::name), Some("decode"));
    }

    #[test]
    fn the_gap_is_noted_only_when_a_kept_sentence_was_read_again() {
        let again = note_plan(silence(), &Plan::<&str>::Whole, Some(Gap::Over(2243)));
        assert_eq!(again.held_gap_ms, Some(140));
        let used = note_plan(silence(), &Plan::Held(kept(1000), 0), Some(Gap::Over(8)));
        assert_eq!(used.held_gap_ms, None);
        let nothing_kept = note_plan(silence(), &Plan::<&str>::Whole, None);
        assert_eq!(nothing_kept.held_gap_ms, None);
        let rest = note_plan(silence(), &Plan::<&str>::Rest(800), Some(Gap::Short(16)));
        assert_eq!(rest.held_gap_ms, Some(-1));
        assert_eq!(again.rule, Rule::Silence);
        assert_eq!(again.silence_ms, 300);
    }

    #[test]
    fn a_plan_that_makes_no_final_names_no_kind() {
        let dropped = note_plan(silence(), &Plan::<&str>::Drop, Some(Gap::Over(100)));
        assert_eq!(dropped.plan, None);
        assert_eq!(dropped.held_gap_ms, None);
    }

    #[test]
    fn the_gap_to_a_kept_sentence_is_reported_whether_or_not_it_was_used() {
        let mut used: State<&str> = State::default();
        used.hold("kept", 1000, 900);
        let (plan, gap) = used.plan_with_gap(100, 1000 + TOLERANCE, REACH);
        assert!(matches!(plan, Plan::Held(..)));
        assert_eq!(gap, Some(Gap::Short(TOLERANCE)));
        let mut far: State<&str> = State::default();
        far.hold("kept", 1000, 900);
        let (plan, gap) = far.plan_with_gap(100, 1000 + TOLERANCE + 1, REACH);
        assert_eq!(plan, Plan::Whole);
        assert_eq!(gap, Some(Gap::Short(TOLERANCE + 1)));
        let mut over: State<&str> = State::default();
        over.hold("kept", 1000 + TOLERANCE + 1, 900);
        let (plan, gap) = over.plan_with_gap(100, 1000, REACH);
        assert_eq!(plan, Plan::Whole);
        assert_eq!(gap, Some(Gap::Over(TOLERANCE + 1)));
        let mut none: State<&str> = State::default();
        assert_eq!(none.plan_with_gap(0, 1000, REACH), (Plan::Whole, None));
    }

    #[test]
    fn a_japanese_tail_is_told_by_the_form_before_the_closing_mark() {
        for (text, tail) in [
            ("近藤さん。", Tail::Honorific),
            ("田中様。", Tail::Honorific),
            ("山田氏。", Tail::Honorific),
            ("独占配信。", Tail::Noun),
            ("コーヒー。", Tail::Noun),
            ("二〇二六年。", Tail::Noun),
            ("2026年。", Tail::Noun),
            ("GPU。", Tail::Noun),
            ("ＧＰＵ。", Tail::Noun),
            ("本音とは。", Tail::Particle),
            ("これを。", Tail::Particle),
            ("ものの。", Tail::Particle),
            ("それも。", Tail::Particle),
            ("雨が降っていますが。", Tail::Particle),
            ("さあ。", Tail::Interjection),
            ("それはさあ。", Tail::Interjection),
            ("あの。", Tail::Interjection),
            ("今日は雨です。", Tail::Other),
            ("これ。", Tail::Other),
        ] {
            assert_eq!(tail_of("ja", text), tail, "{text}");
        }
    }

    #[test]
    fn a_tail_does_not_need_the_closing_mark_or_the_space_around_it() {
        assert_eq!(tail_of("ja", "独占配信"), Tail::Noun);
        assert_eq!(tail_of("ja", "独占配信。 "), Tail::Noun);
        assert_eq!(tail_of("ja", "独占配信 。"), Tail::Noun);
        assert_eq!(tail_of("ja", "近藤さん？！"), Tail::Honorific);
    }

    #[test]
    fn an_empty_body_and_a_language_but_ja_have_the_other_tail() {
        assert_eq!(tail_of("ja", ""), Tail::Other);
        assert_eq!(tail_of("ja", "。"), Tail::Other);
        assert_eq!(tail_of("en", "Exclusive release."), Tail::Other);
        assert_eq!(tail_of("zh", "独占配信。"), Tail::Other);
        assert_eq!(tail_of("", "独占配信。"), Tail::Other);
    }

    #[test]
    fn only_a_noun_and_an_honorific_end_a_sentence_in_the_shadow_rule() {
        let ends: Vec<Tail> = [
            Tail::Noun,
            Tail::Honorific,
            Tail::Particle,
            Tail::Interjection,
            Tail::Other,
        ]
        .into_iter()
        .filter(|tail| tail.ends_a_sentence())
        .collect();
        assert_eq!(ends, [Tail::Noun, Tail::Honorific]);
    }

    #[test]
    fn tails_are_named_for_the_log() {
        let names: Vec<&str> = [
            Tail::Noun,
            Tail::Honorific,
            Tail::Particle,
            Tail::Interjection,
            Tail::Other,
        ]
        .iter()
        .map(|tail| tail.name())
        .collect();
        assert_eq!(
            names,
            ["noun", "honorific", "particle", "interjection", "other"]
        );
    }

    /// The live rule keeps a noun or an honorific ending; the shadow rule
    /// would have finalised it from one second of speech.
    #[test]
    fn the_shadow_rule_finalises_a_noun_or_an_honorific_ending_that_the_live_rule_keeps() {
        for text in ["独占配信。", "近藤さん。"] {
            assert_eq!(early_hold("ja", text, MS_1000), Some(Hold::Incomplete));
            assert_eq!(shadow_early_hold("ja", text, MS_1000), None, "{text}");
            assert_eq!(
                shadow_early_hold("ja", text, MS_999),
                Some(Hold::TooShort),
                "{text}"
            );
        }
    }

    #[test]
    fn the_shadow_rule_keeps_every_other_ending_the_live_rule_keeps() {
        for text in [
            "本音とは。",
            "これを。",
            "さあ。",
            "これ。",
            "雨が降っていますが。",
            "独占配信",
            "近藤さん",
            "",
        ] {
            assert_eq!(
                shadow_early_hold("ja", text, 10 * MS_1000),
                Some(Hold::Incomplete),
                "{text}"
            );
        }
    }

    #[test]
    fn the_shadow_rule_answers_as_the_live_rule_where_the_live_rule_finalises_or_does_not_judge() {
        for (lang, text) in [
            ("ja", "言ったろう。"),
            ("ja", "はい。"),
            ("en", "It is raining."),
            ("en", "It is raining and."),
            ("zh", "今天下雨了。"),
            ("en", "Exclusive release."),
        ] {
            for samples in [0, MS_999, MS_1000, 10 * MS_1000] {
                assert_eq!(
                    shadow_early_hold(lang, text, samples),
                    early_hold(lang, text, samples),
                    "{lang} {text} {samples}"
                );
            }
        }
    }

    #[test]
    fn rules_are_named_for_the_log() {
        let names: Vec<&str> = [Rule::Early, Rule::Silence, Rule::MaxSpeech, Rule::Flush]
            .iter()
            .map(|rule| rule.name())
            .collect();
        assert_eq!(names, ["early", "silence", "max_speech", "flush"]);
    }
}

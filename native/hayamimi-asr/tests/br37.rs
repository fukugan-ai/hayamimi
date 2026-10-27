//! BR-37: in a pair run the language of a final whose voice reached four
//! seconds is the one the identifier named at that point, and the final does
//! not ask again.
//!
//! The identifier and the recognizers are scripted, so a test says what the
//! identifier answers to each question, in the order the questions come, and
//! then reads which questions were put and to which audio. Nothing here opens a
//! model. The windows go to the call the engine makes once a window is in
//! (`observe`), and the answer goes to the call the engine makes when a segment
//! closes (`closing_for`, then `transcribe`).

use std::cell::RefCell;
use std::rc::Rc;

use hayamimi_asr::AsrError;
use hayamimi_asr::audio::{History, WINDOW};
use hayamimi_asr::lid::{Identify, lid_clip};
use hayamimi_asr::partial::{
    CLOSING_ALIGN_SAMPLES, MIDWAY_CLOSING_SAMPLES, Midway, MidwayAsked, MidwayLid,
};
use hayamimi_asr::route::{
    Decode, DecodeError, Decoded, Policy, Request, Router, Tier, Transcript,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// The audio of every question put to the identifier, in order.
type Questions = Rc<RefCell<Vec<Vec<f32>>>>;

/// The window that takes the voice to two seconds (32256 samples).
const TWO_SECONDS_AT: usize = 63;
/// The window that takes the voice to four seconds (64000 samples).
const FOUR_SECONDS_AT: usize = 125;

/// What the identifier says to one question.
#[derive(Clone, Copy)]
enum Say {
    Lang(&'static str),
    Fail,
}

/// An identifier that answers each question from a script, in the order the
/// questions come, and keeps the audio of every question. A question the script
/// does not cover fails.
struct Listener {
    script: Vec<Say>,
    questions: Questions,
}

impl Listener {
    fn saying(script: &[Say]) -> (Listener, Questions) {
        let questions = Questions::default();
        let listener = Listener {
            script: script.to_vec(),
            questions: Rc::clone(&questions),
        };
        (listener, questions)
    }

    fn always(code: &'static str) -> (Listener, Questions) {
        Listener::saying(&[Say::Lang(code); 8])
    }
}

impl Identify for Listener {
    fn identify(&self, samples: &[f32]) -> Result<String, AsrError> {
        let mut asked = self.questions.borrow_mut();
        let at = asked.len();
        asked.push(samples.to_vec());
        match self.script.get(at) {
            Some(Say::Lang(code)) => Ok((*code).to_owned()),
            Some(Say::Fail) | None => Err(AsrError::NoResult),
        }
    }
}

/// Recognizers that read a plain sentence of the tier's language.
struct Models;

impl Decode for Models {
    fn decode(&mut self, tier: Tier, _samples: &[f32]) -> Result<Decoded, DecodeError> {
        let text = match tier {
            Tier::Rz | Tier::Pja => "はい",
            Tier::V3 | Tier::V2 => "yes",
            Tier::Pz => "你好",
            Tier::Omni => "ok",
            Tier::Sv => return Err(DecodeError::Unavailable),
        };
        Ok(Decoded {
            text: text.to_owned(),
            tag: String::new(),
        })
    }

    fn split_retry(
        &mut self,
        text: &str,
        _samples: &[f32],
        _lang: &str,
        _tier: Tier,
    ) -> Result<String, DecodeError> {
        Ok(text.to_owned())
    }

    fn prefetch(&mut self, _tier: Tier) {}

    fn close(&mut self) {}
}

type Fake = Router<Models, Listener>;

fn router(listener: Listener, policy: Policy) -> Fake {
    Router::new(Models, Some(listener), policy)
}

/// A voice of `windows` windows that began at `start`. After each window the
/// questions are put as the engine puts them. Gives each question with the
/// number (from 1) of the window it was put after.
fn speak(
    router: &mut Fake,
    midway: &mut MidwayLid,
    history: &mut History,
    start: usize,
    windows: usize,
) -> Vec<(usize, MidwayAsked)> {
    let window = [0.5_f32; WINDOW];
    let mut asked = Vec::new();
    for number in 1..=windows {
        history.push(&window);
        if let Some(note) = midway.observe(router, history, Some(start)) {
            asked.push((number, note));
        }
    }
    asked
}

/// The numbers of the windows that put a question.
fn window_numbers(asked: &[(usize, MidwayAsked)]) -> Vec<usize> {
    asked.iter().map(|(number, _)| *number).collect()
}

/// T-BR-37-001
#[test]
fn br37_a_midway_answer_at_four_seconds_names_the_final_language_without_a_second_question()
-> TestResult {
    let (listener, questions) = Listener::always("ja");
    let mut routed = router(listener, Policy::pair());
    let mut midway = MidwayLid::new();
    let mut history = History::new();
    let asked = speak(&mut routed, &mut midway, &mut history, 0, FOUR_SECONDS_AT);
    assert_eq!(window_numbers(&asked), [TWO_SECONDS_AT, FOUR_SECONDS_AT]);
    let Some((_, closing)) = asked.last() else {
        return Err("the four-second question was not put".into());
    };
    assert_eq!(closing.voice_samples, MIDWAY_CLOSING_SAMPLES);
    assert_eq!(closing.answer, Midway::Heard("ja"));
    let named = midway.closing_for(0);
    assert_eq!(named, Some("ja"));
    assert_eq!(questions.borrow().len(), 2);
    let voice = vec![0.5_f32; MIDWAY_CLOSING_SAMPLES];
    let transcript = routed.transcribe(&Request {
        midway_lang: named,
        speech_s: Some(4.0),
        ..Request::live(&voice)
    })?;
    assert_eq!(questions.borrow().len(), 2, "the final asked again");
    assert_eq!(transcript.lang, "ja");
    assert_eq!(transcript.lid_ms, 0.0);
    assert_eq!(transcript.fallback, None);
    Ok(())
}

/// T-BR-37-002
#[test]
fn the_four_second_question_is_asked_once_when_the_voice_reaches_four_seconds() -> TestResult {
    let (listener, questions) = Listener::always("ja");
    let mut routed = router(listener, Policy::pair());
    let mut midway = MidwayLid::new();
    let mut history = History::new();
    let asked = speak(&mut routed, &mut midway, &mut history, 0, 130);
    assert_eq!(window_numbers(&asked), [TWO_SECONDS_AT, FOUR_SECONDS_AT]);
    assert_eq!(questions.borrow().len(), 2);
    Ok(())
}

/// T-BR-37-003
#[test]
fn the_four_second_question_carries_up_to_one_second_before_the_voice() -> TestResult {
    // The silence before the voice, and the audio the four-second question is given.
    for (lead, given) in [(20_000, 80_000), (5_000, 69_000)] {
        let (listener, questions) = Listener::always("ja");
        let mut routed = router(listener, Policy::pair());
        let mut midway = MidwayLid::new();
        let mut history = History::new();
        history.push(&vec![0.0_f32; lead]);
        let asked = speak(
            &mut routed,
            &mut midway,
            &mut history,
            lead,
            FOUR_SECONDS_AT,
        );
        let Some((_, closing)) = asked.last() else {
            return Err("the four-second question was not put".into());
        };
        assert_eq!(closing.voice_samples, given, "lead {lead}");
        let heard = questions.borrow();
        let Some(last) = heard.last() else {
            return Err("the identifier was not asked".into());
        };
        assert_eq!(last.len(), given, "lead {lead}");
    }
    Ok(())
}

/// T-BR-37-004
#[test]
fn the_four_second_question_is_asked_only_by_a_pair_run_that_asks_for_it() -> TestResult {
    // A policy, the questions put, what the final may take, what the drafts heard.
    let cases = [
        (
            "pair, final_midway off",
            Policy {
                final_midway: false,
                ..Policy::pair()
            },
            1,
            None,
            Some("ja"),
        ),
        (
            "pair, partial_lid off",
            Policy {
                partial_lid: false,
                ..Policy::pair()
            },
            1,
            Some("ja"),
            None,
        ),
        ("fast", Policy::fast(), 0, None, None),
        ("single", Policy::single("ja"), 0, None, None),
    ];
    for (name, policy, put, closing, heard) in cases {
        let (listener, questions) = Listener::always("ja");
        let mut routed = router(listener, policy);
        let mut midway = MidwayLid::new();
        let mut history = History::new();
        speak(&mut routed, &mut midway, &mut history, 0, FOUR_SECONDS_AT);
        assert_eq!(questions.borrow().len(), put, "{name}");
        assert_eq!(midway.closing_for(0), closing, "{name}");
        assert_eq!(midway.heard(), heard, "{name}");
    }
    Ok(())
}

/// T-BR-37-005
#[test]
fn the_two_questions_due_in_one_call_cost_one_question() -> TestResult {
    let (listener, questions) = Listener::always("ja");
    let mut routed = router(listener, Policy::pair());
    let mut midway = MidwayLid::new();
    let mut history = History::new();
    history.push(&vec![0.5_f32; 130 * WINDOW]);
    let Some(note) = midway.observe(&mut routed, &history, Some(0)) else {
        return Err("no question was put".into());
    };
    assert_eq!(note.voice_samples, 130 * WINDOW);
    assert_eq!(questions.borrow().len(), 1);
    assert_eq!(midway.closing_for(0), Some("ja"));
    assert_eq!(midway.heard(), Some("ja"));
    Ok(())
}

/// T-BR-37-006
#[test]
fn a_midway_answer_outside_the_pair_or_a_failure_is_not_used_for_the_final() -> TestResult {
    // The answers to the two-second and to the four-second question, what the
    // final may take, what the drafts heard.
    let cases = [
        (
            "en, en",
            [Say::Lang("en"), Say::Lang("en")],
            Some("en"),
            Some("en"),
        ),
        (
            "ja, zh",
            [Say::Lang("ja"), Say::Lang("zh")],
            None,
            Some("ja"),
        ),
        (
            "ja, failure",
            [Say::Lang("ja"), Say::Fail],
            None,
            Some("ja"),
        ),
        (
            "zh, ja",
            [Say::Lang("zh"), Say::Lang("ja")],
            Some("ja"),
            None,
        ),
    ];
    for (name, script, closing, heard) in cases {
        let (listener, _questions) = Listener::saying(&script);
        let mut routed = router(listener, Policy::pair());
        let mut midway = MidwayLid::new();
        let mut history = History::new();
        speak(&mut routed, &mut midway, &mut history, 0, FOUR_SECONDS_AT);
        assert_eq!(midway.closing_for(0), closing, "{name}");
        assert_eq!(midway.heard(), heard, "{name}");
    }
    Ok(())
}

/// T-BR-37-007
#[test]
fn the_answer_is_used_only_for_a_segment_that_starts_just_before_the_voice() -> TestResult {
    let lead = 20_000;
    let (listener, _questions) = Listener::always("ja");
    let mut routed = router(listener, Policy::pair());
    let mut midway = MidwayLid::new();
    let mut history = History::new();
    history.push(&vec![0.0_f32; lead]);
    speak(
        &mut routed,
        &mut midway,
        &mut history,
        lead,
        FOUR_SECONDS_AT,
    );
    // The voice began `lead - from` samples after the segment did.
    assert_eq!(CLOSING_ALIGN_SAMPLES, 8_000);
    assert_eq!(midway.closing_for(lead), Some("ja"));
    assert_eq!(midway.closing_for(lead - CLOSING_ALIGN_SAMPLES), Some("ja"));
    assert_eq!(midway.closing_for(lead - CLOSING_ALIGN_SAMPLES - 1), None);
    assert_eq!(midway.closing_for(lead + 1), None);
    Ok(())
}

/// T-BR-37-008
#[test]
fn a_new_voice_start_forgets_the_answer_and_asks_again_at_four_seconds() -> TestResult {
    let (listener, questions) = Listener::always("ja");
    let mut routed = router(listener, Policy::pair());
    let mut midway = MidwayLid::new();
    let mut history = History::new();
    speak(&mut routed, &mut midway, &mut history, 0, FOUR_SECONDS_AT);
    assert_eq!(midway.closing_for(0), Some("ja"));
    // The speech ends: the next call has no start.
    assert_eq!(midway.observe(&mut routed, &history, None), None);
    assert_eq!(midway.closing_for(0), None);
    // Another utterance begins where the history ends.
    let next = history.end();
    assert_eq!(midway.observe(&mut routed, &history, Some(next)), None);
    assert_eq!(midway.closing_for(next), None);
    let asked = speak(
        &mut routed,
        &mut midway,
        &mut history,
        next,
        FOUR_SECONDS_AT,
    );
    assert_eq!(window_numbers(&asked), [TWO_SECONDS_AT, FOUR_SECONDS_AT]);
    assert_eq!(midway.closing_for(next), Some("ja"));
    assert_eq!(questions.borrow().len(), 4);
    Ok(())
}

/// T-BR-37-009
#[test]
fn the_final_asks_the_identifier_unless_a_midway_answer_names_a_language_of_the_pair() -> TestResult
{
    // The answer carried by the request, the questions the final put, the language it was read in.
    let cases = [
        (Some("ja"), 0, "ja"),
        (Some("en"), 0, "en"),
        (Some("zh"), 1, "en"),
        (None, 1, "en"),
    ];
    for (midway_lang, put, lang) in cases {
        let (listener, questions) = Listener::always("en");
        let mut routed = router(listener, Policy::pair());
        let voice = vec![0.5_f32; MIDWAY_CLOSING_SAMPLES];
        let transcript: Transcript = routed.transcribe(&Request {
            midway_lang,
            speech_s: Some(6.0),
            ..Request::live(&voice)
        })?;
        assert_eq!(questions.borrow().len(), put, "{midway_lang:?}");
        assert_eq!(transcript.lang, lang, "{midway_lang:?}");
        if put == 0 {
            assert_eq!(transcript.lid_ms, 0.0, "{midway_lang:?}");
        }
    }
    Ok(())
}

/// T-BR-37-010
#[test]
fn the_four_second_question_and_the_final_read_the_same_clip() -> TestResult {
    // The silence before the voice (32 windows), and the voice's start that the
    // detector places 9 windows (about 0.3 s) late.
    let lead = 32 * WINDOW;
    let detected = lead + 9 * WINDOW;
    let policy = Policy {
        partial_lid: false,
        ..Policy::pair()
    };
    let (listener, questions) = Listener::always("ja");
    let mut routed = router(listener, policy);
    let mut midway = MidwayLid::new();
    let mut history = History::new();
    history.push(&vec![0.0_f32; lead]);
    let asked = speak(&mut routed, &mut midway, &mut history, detected, 134);
    assert_eq!(asked.len(), 1);
    // The final reads the segment from the voice's real start, with its pre-roll.
    let end = history.end();
    let segment = history.slice(lead, end);
    let read = history.with_preroll(lead, &segment);
    let transcript = routed.transcribe(&Request::live(&read))?;
    assert_eq!(transcript.lang, "ja");
    let heard = questions.borrow();
    let [midway_clip, final_clip] = heard.as_slice() else {
        return Err(format!("{} questions were put, not 2", heard.len()).into());
    };
    let (midway_clip, final_clip) = (lid_clip(midway_clip), lid_clip(final_clip));
    assert_eq!(midway_clip.len(), MIDWAY_CLOSING_SAMPLES);
    assert_eq!(final_clip.len(), MIDWAY_CLOSING_SAMPLES);
    assert!(midway_clip == final_clip, "the two clips differ");
    Ok(())
}

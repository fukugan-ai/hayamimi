//! BR-4: in a pair run the drafts of an utterance are read in the language the
//! identifier names once the voice reaches two seconds, and the final's way of
//! choosing its language is not touched.
//!
//! The identifier and the recognizers are scripted, so a test says what each
//! answers and then reads which tier each draft was read in, which models were
//! asked to load, and how much voice the identifier was asked about. The
//! identifier tells its questions apart by their size: a read shorter than two
//! seconds makes the language of the last final, a question of two seconds or
//! more and shorter than the whole segment is the midway one, and a question
//! the size of the whole segment is the final's. Nothing here opens a model.
//! The windows go to the same two calls the engine makes: `observe` once a
//! window is in, and `tier` when a draft is read.
//!
//! The language a draft carries to the relay is read off the draft's characters
//! there (`partial_language`), so a draft read by the English tier is an English
//! draft; the tier and the text are what this crate decides.

use std::cell::RefCell;
use std::rc::Rc;

use hayamimi_asr::AsrError;
use hayamimi_asr::audio::{History, WINDOW};
use hayamimi_asr::lid::Identify;
use hayamimi_asr::partial::{MIDWAY_SAMPLES, Midway, MidwayAsked, MidwayLid};
use hayamimi_asr::route::{
    Decode, DecodeError, Decoded, Policy, Request, RouteError, Router, Tier, Transcript,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// The size, in samples, of every question put to the identifier, in order.
type Questions = Rc<RefCell<Vec<usize>>>;

/// Windows of voice one short of two seconds (31744 samples).
const BELOW: usize = 62;
/// Windows of voice that first reach two seconds (32256 samples).
const AT: usize = 63;
/// A segment of four seconds (64000 samples).
const LONG: usize = 125;
/// The segments that run on past [`AT`], each longer than the one before.
const LONGER: [usize; 8] = [64, 72, 80, 90, 100, 110, 120, 125];
/// The next utterance after a [`LONG`] one: 1.25 seconds.
const NEXT: usize = 40;
/// The short read that makes the language of the last final: 0.3 seconds.
const PRIMER: usize = 4_800;
/// What the Japanese tiers read.
const JAPANESE: &str = "はい";
/// What the English tiers read.
const ENGLISH: &str = "yes";

/// What the identifier names, or that it fails.
#[derive(Clone, Copy)]
enum Say {
    Lang(&'static str),
    Fail,
}

/// What the identifier says, by the kind of question.
#[derive(Clone, Copy)]
struct Hears {
    /// The short read that makes the language of the last final.
    first: &'static str,
    /// A question in the middle of an utterance: two seconds of voice or more,
    /// and shorter than the whole segment.
    midway: Say,
    /// The question of the whole segment, when the detector closes it.
    whole: Say,
}

/// An identifier that answers by the size of what it is asked, and writes every
/// size into the log the test holds.
struct Heard {
    hears: Hears,
    /// The size of the whole segment, in samples.
    whole_len: usize,
    log: Questions,
}

impl Identify for Heard {
    fn identify(&self, samples: &[f32]) -> Result<String, AsrError> {
        let asked = samples.len();
        self.log.borrow_mut().push(asked);
        let say = if asked == self.whole_len {
            self.hears.whole
        } else if asked >= MIDWAY_SAMPLES {
            self.hears.midway
        } else {
            Say::Lang(self.hears.first)
        };
        match say {
            Say::Lang(code) => Ok(code.to_owned()),
            Say::Fail => Err(AsrError::NoResult),
        }
    }
}

/// Recognizers that read a plain sentence of the tier's language, and remember
/// the tiers they were asked to read in and to load.
struct Models {
    calls: Vec<Tier>,
    prefetched: Vec<Tier>,
}

impl Decode for Models {
    fn decode(&mut self, tier: Tier, _samples: &[f32]) -> Result<Decoded, DecodeError> {
        self.calls.push(tier);
        let text = match tier {
            Tier::Rz | Tier::Pja => JAPANESE,
            Tier::V3 | Tier::V2 => ENGLISH,
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

    fn prefetch(&mut self, tier: Tier) {
        self.prefetched.push(tier);
    }

    fn close(&mut self) {}
}

type Fake = Router<Models, Heard>;

/// One draft: the tier it was read in and the text that tier read.
#[derive(Debug)]
struct Draft {
    tier: Tier,
    text: String,
}

/// A router for a segment of `windows` windows, with an identifier that answers
/// as `hears` says, and the log of its questions.
fn router(hears: Hears, windows: usize, policy: Policy) -> (Fake, Questions) {
    // The drafts are this test's subject; the question a final puts at four
    // seconds of voice is BR-37's, and would change the questions counted here.
    let policy = Policy {
        final_midway: false,
        ..policy
    };
    let log = Questions::default();
    let heard = Heard {
        hears,
        whole_len: windows * WINDOW,
        log: Rc::clone(&log),
    };
    let models = Models {
        calls: Vec::new(),
        prefetched: Vec::new(),
    };
    (Router::new(models, Some(heard), policy), log)
}

/// The pair policy with the midway question switched off.
fn pair_without_the_midway_question() -> Policy {
    Policy {
        partial_lid: false,
        ..Policy::pair()
    }
}

/// Reads `samples` as a whole segment, as the engine does when the detector closes it.
fn read_whole(router: &mut Fake, samples: &[f32]) -> Result<Transcript, RouteError> {
    router.transcribe(&Request::live(samples))
}

/// A router whose last final was read in the language `hears.first` names: one
/// short segment has been read. The log keeps that first question.
fn after_a_final(
    hears: Hears,
    windows: usize,
    policy: Policy,
) -> Result<(Fake, Questions), RouteError> {
    let (mut routed, log) = router(hears, windows, policy);
    let primed = read_whole(&mut routed, &[0.5; PRIMER])?;
    assert_eq!(primed.lang, hears.first);
    assert_eq!(routed.last_lang(), Some(hears.first));
    Ok((routed, log))
}

/// The route error a draft that could not be read stands for.
fn unread(error: DecodeError) -> RouteError {
    match error {
        DecodeError::Unavailable => RouteError::NoModels,
        DecodeError::Failed(inner) => RouteError::Decode(inner),
    }
}

/// What an utterance made: a draft for each window, and the questions the
/// identifier was asked in the middle of it.
struct Spoken {
    drafts: Vec<Draft>,
    asked: Vec<MidwayAsked>,
}

/// One utterance of `windows` windows of voice that began at `start`. After
/// each window the midway question is asked as the engine asks it, and a draft
/// is read in the tier it names.
fn speak_noted(
    router: &mut Fake,
    midway: &mut MidwayLid,
    history: &mut History,
    start: usize,
    windows: usize,
) -> Result<Spoken, RouteError> {
    let window = [0.5_f32; WINDOW];
    let mut spoken = Spoken {
        drafts: Vec::new(),
        asked: Vec::new(),
    };
    for _ in 0..windows {
        history.push(&window);
        if let Some(note) = midway.observe(router, history, Some(start)) {
            spoken.asked.push(note);
        }
        let tier = midway.tier(router);
        let read = router.decoder_mut().decode(tier, &window).map_err(unread)?;
        spoken.drafts.push(Draft {
            tier,
            text: read.text,
        });
    }
    Ok(spoken)
}

/// The drafts of [`speak_noted`], one per window.
fn speak(
    router: &mut Fake,
    midway: &mut MidwayLid,
    history: &mut History,
    start: usize,
    windows: usize,
) -> Result<Vec<Draft>, RouteError> {
    Ok(speak_noted(router, midway, history, start, windows)?.drafts)
}

/// Whether every draft was read in `tier` and came out as `text`.
fn read_all_in(drafts: &[Draft], tier: Tier, text: &str) -> bool {
    drafts
        .iter()
        .all(|draft| draft.tier == tier && draft.text == text)
}

/// The sizes of the questions asked since the short read that made the last final.
fn after_priming(log: &Questions) -> Vec<usize> {
    log.borrow().iter().skip(1).copied().collect()
}

fn whole_utterance(windows: usize) -> Vec<f32> {
    vec![0.5; windows * WINDOW]
}

/// T-BR-04-001
#[test]
fn br04_a_partial_after_a_switch_is_read_in_the_language_heard_so_far() -> TestResult {
    let hears = Hears {
        first: "ja",
        midway: Say::Lang("en"),
        whole: Say::Lang("en"),
    };
    let (mut routed, log) = after_a_final(hears, LONG, Policy::pair())?;
    let loaded = routed.decoder().prefetched.len();
    let mut midway = MidwayLid::new();
    let mut history = History::new();
    let drafts = speak(&mut routed, &mut midway, &mut history, 0, LONG)?;
    // The 63rd window takes the voice to 32256 samples and the question is
    // asked right after it: the 62 drafts before are read in Japanese, the
    // drafts from the 63rd on in English.
    let (before, after) = drafts.split_at(BELOW);
    assert!(read_all_in(before, Tier::Rz, JAPANESE), "{before:?}");
    assert!(!after.is_empty());
    assert!(read_all_in(after, Tier::V3, ENGLISH), "{after:?}");
    assert_eq!(midway.answer(), Midway::Heard("en"));
    // The English model starts loading when it is named, so it is resident at the final.
    assert_eq!(routed.decoder().prefetched[loaded..], [Tier::V3]);
    let got = read_whole(&mut routed, &whole_utterance(LONG))?;
    assert_eq!(got.lang, "en");
    // The midway question and the question of the whole segment.
    assert_eq!(after_priming(&log), [AT * WINDOW, LONG * WINDOW]);
    Ok(())
}

/// T-BR-04-002
#[test]
fn with_partial_lid_off_the_partial_stays_in_the_language_of_the_last_final() -> TestResult {
    let hears = Hears {
        first: "ja",
        midway: Say::Lang("en"),
        whole: Say::Lang("en"),
    };
    let (mut routed, log) = after_a_final(hears, LONG, pair_without_the_midway_question())?;
    let loaded = routed.decoder().prefetched.len();
    let mut midway = MidwayLid::new();
    let mut history = History::new();
    let drafts = speak(&mut routed, &mut midway, &mut history, 0, LONG)?;
    assert!(read_all_in(&drafts, Tier::Rz, JAPANESE), "{drafts:?}");
    assert_eq!(midway.answer(), Midway::Waiting);
    assert!(after_priming(&log).is_empty());
    assert_eq!(routed.decoder().prefetched.len(), loaded);
    let got = read_whole(&mut routed, &whole_utterance(LONG))?;
    assert_eq!(got.lang, "en");
    assert_eq!(after_priming(&log), [LONG * WINDOW]);
    Ok(())
}

/// T-BR-04-003
#[test]
fn a_segment_of_62_windows_gets_no_midway_identification() -> TestResult {
    const { assert!(BELOW * WINDOW < MIDWAY_SAMPLES) };
    let hears = Hears {
        first: "ja",
        midway: Say::Lang("en"),
        whole: Say::Lang("en"),
    };
    let (mut routed, log) = after_a_final(hears, BELOW, Policy::pair())?;
    let mut midway = MidwayLid::new();
    let mut history = History::new();
    let drafts = speak(&mut routed, &mut midway, &mut history, 0, BELOW)?;
    assert!(read_all_in(&drafts, Tier::Rz, JAPANESE), "{drafts:?}");
    assert_eq!(midway.answer(), Midway::Waiting);
    assert!(after_priming(&log).is_empty());
    read_whole(&mut routed, &whole_utterance(BELOW))?;
    assert_eq!(after_priming(&log), [BELOW * WINDOW]);
    Ok(())
}

/// T-BR-04-004
#[test]
fn a_segment_of_63_windows_gets_one_midway_identification() -> TestResult {
    const { assert!(AT * WINDOW >= MIDWAY_SAMPLES) };
    let hears = Hears {
        first: "ja",
        midway: Say::Lang("en"),
        whole: Say::Lang("en"),
    };
    for windows in std::iter::once(AT).chain(LONGER) {
        let (mut routed, log) = after_a_final(hears, windows, Policy::pair())?;
        let mut midway = MidwayLid::new();
        let mut history = History::new();
        speak(&mut routed, &mut midway, &mut history, 0, windows)?;
        read_whole(&mut routed, &whole_utterance(windows))?;
        // The first question carries the voice up to the window that took it
        // past two seconds, not 32000 samples exactly, and there is one.
        assert_eq!(
            after_priming(&log),
            [AT * WINDOW, windows * WINDOW],
            "{windows} windows"
        );
    }
    Ok(())
}

/// T-BR-04-005
#[test]
fn a_midway_answer_outside_the_pair_leaves_the_partial_where_it_was() -> TestResult {
    let hears = Hears {
        first: "en",
        midway: Say::Lang("zh"),
        whole: Say::Lang("en"),
    };
    let (mut routed, log) = after_a_final(hears, LONG, Policy::pair())?;
    let loaded = routed.decoder().prefetched.len();
    let mut midway = MidwayLid::new();
    let mut history = History::new();
    let drafts = speak(&mut routed, &mut midway, &mut history, 0, LONG)?;
    assert!(read_all_in(&drafts, Tier::V3, ENGLISH), "{drafts:?}");
    assert_eq!(midway.answer(), Midway::Outside);
    assert_eq!(after_priming(&log), [AT * WINDOW]);
    // A language the pair never reads has no tier asked to read or to load.
    assert!(!routed.decoder().calls.contains(&Tier::Pz));
    assert_eq!(routed.decoder().prefetched.len(), loaded);
    Ok(())
}

/// T-BR-04-006
#[test]
fn the_midway_answer_does_not_become_the_language_of_the_next_segment() -> TestResult {
    let hears = Hears {
        first: "ja",
        midway: Say::Lang("en"),
        whole: Say::Lang("ja"),
    };
    let (mut routed, _log) = after_a_final(hears, LONG, Policy::pair())?;
    let mut midway = MidwayLid::new();
    let mut history = History::new();
    speak(&mut routed, &mut midway, &mut history, 0, LONG)?;
    assert_eq!(midway.answer(), Midway::Heard("en"));
    let first = read_whole(&mut routed, &whole_utterance(LONG))?;
    assert_eq!(first.lang, "ja");
    // The speech ends: the next window sees no speech in progress.
    midway.observe(&mut routed, &history, None);
    assert_eq!(midway.answer(), Midway::Waiting);
    let read_so_far = routed.decoder().calls.len();
    let second = speak(&mut routed, &mut midway, &mut history, LONG * WINDOW, NEXT)?;
    assert!(!second.is_empty());
    assert!(read_all_in(&second, Tier::Rz, JAPANESE), "{second:?}");
    assert!(
        !routed.decoder().calls[read_so_far..].contains(&Tier::V3),
        "the English tier was called for the next segment"
    );
    Ok(())
}

/// T-BR-04-007
#[test]
fn the_final_identification_is_called_and_answers_the_same_with_the_midway_one_on_or_off()
-> TestResult {
    let hears = Hears {
        first: "ja",
        midway: Say::Lang("en"),
        whole: Say::Lang("ja"),
    };
    let mut seen = Vec::new();
    for policy in [Policy::pair(), pair_without_the_midway_question()] {
        let on = policy.partial_lid;
        let (mut routed, log) = after_a_final(hears, LONG, policy)?;
        let mut midway = MidwayLid::new();
        let mut history = History::new();
        speak(&mut routed, &mut midway, &mut history, 0, LONG)?;
        let got = read_whole(&mut routed, &whole_utterance(LONG))?;
        let asked = after_priming(&log);
        let whole_questions = asked.iter().filter(|size| **size == LONG * WINDOW).count();
        let midway_questions = asked.iter().filter(|size| **size == AT * WINDOW).count();
        assert_eq!(midway_questions, usize::from(on), "midway on={on}");
        seen.push((whole_questions, got.lang, got.fallback));
    }
    assert_eq!(seen[0], seen[1]);
    assert_eq!(seen[0].0, 1);
    assert_eq!(seen[0].1, "ja");
    assert_eq!(seen[0].2, None);
    Ok(())
}

/// T-BR-04-010
#[test]
fn a_failed_midway_question_is_kept_and_not_asked_again() -> TestResult {
    let hears = Hears {
        first: "ja",
        midway: Say::Fail,
        whole: Say::Lang("en"),
    };
    let (mut routed, log) = after_a_final(hears, LONG, Policy::pair())?;
    let loaded = routed.decoder().prefetched.len();
    let mut midway = MidwayLid::new();
    let mut history = History::new();
    let drafts = speak(&mut routed, &mut midway, &mut history, 0, LONG)?;
    assert_eq!(midway.answer(), Midway::Failed);
    assert!(read_all_in(&drafts, Tier::Rz, JAPANESE), "{drafts:?}");
    assert_eq!(after_priming(&log), [AT * WINDOW]);
    assert_eq!(routed.decoder().prefetched.len(), loaded);
    // The failure does not reach the final, which asks and answers on its own.
    let got = read_whole(&mut routed, &whole_utterance(LONG))?;
    assert_eq!(got.lang, "en");
    assert_eq!(after_priming(&log), [AT * WINDOW, LONG * WINDOW]);
    Ok(())
}

/// T-BR-04-012
#[test]
fn a_question_asked_in_the_middle_is_reported_once_with_what_it_came_to() -> TestResult {
    let cases = [
        (Say::Lang("en"), Midway::Heard("en")),
        (Say::Lang("zh"), Midway::Outside),
        (Say::Fail, Midway::Failed),
    ];
    for (said, came_to) in cases {
        let hears = Hears {
            first: "ja",
            midway: said,
            whole: Say::Lang("en"),
        };
        let (mut routed, _log) = after_a_final(hears, LONG, Policy::pair())?;
        let mut midway = MidwayLid::new();
        let mut history = History::new();
        let spoken = speak_noted(&mut routed, &mut midway, &mut history, 0, LONG)?;
        assert_eq!(spoken.asked.len(), 1, "{came_to:?}");
        let asked = spoken.asked[0];
        assert_eq!(asked.answer, came_to);
        assert_eq!(asked.voice_samples, AT * WINDOW);
        assert!(asked.lid_ms.is_finite() && asked.lid_ms >= 0.0, "{asked:?}");
    }
    // A segment too short for the question reports none.
    let hears = Hears {
        first: "ja",
        midway: Say::Lang("en"),
        whole: Say::Lang("en"),
    };
    let (mut routed, _log) = after_a_final(hears, BELOW, Policy::pair())?;
    let mut midway = MidwayLid::new();
    let mut history = History::new();
    let spoken = speak_noted(&mut routed, &mut midway, &mut history, 0, BELOW)?;
    assert!(spoken.asked.is_empty());
    Ok(())
}

/// T-BR-04-011
#[test]
fn a_run_that_is_not_a_pair_run_never_asks_the_midway_question() -> TestResult {
    let hears = Hears {
        first: "ja",
        midway: Say::Lang("en"),
        whole: Say::Lang("en"),
    };
    let others = [
        Policy::balanced(),
        Policy::fast(),
        Policy::single("ja"),
        Policy {
            pair: false,
            ..Policy::pair()
        },
    ];
    for policy in others {
        let (mut routed, log) = router(hears, LONG, policy);
        let mut midway = MidwayLid::new();
        let mut history = History::new();
        let drafts = speak(&mut routed, &mut midway, &mut history, 0, LONG)?;
        assert!(read_all_in(&drafts, Tier::Rz, JAPANESE), "{drafts:?}");
        assert_eq!(midway.answer(), Midway::Waiting);
        assert!(log.borrow().is_empty());
    }
    Ok(())
}

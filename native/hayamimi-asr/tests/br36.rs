//! BR-36: a sentence the early detector kept is the final of a segment when it
//! was read to a point inside the silence that closed the segment, before or
//! after the end of the segment, and is read again when it was read to a point
//! further from the end than that silence, or with voice after it.
//!
//! The state is driven through the same calls the engine makes: `hold` when the
//! detector keeps a sentence, `note_voice` when it hears more voice, and
//! `plan_with_gap` with the reach of the flag when the main detector hands the
//! segment over. Nothing here opens a model. Samples are 16 kHz: one window is
//! 512 samples (32 ms), and the silence a `silence` close waits for is 300 ms.

use hayamimi_asr::audio::{SAMPLE_RATE, WINDOW};
use hayamimi_asr::endpoint::{
    Decision, Gap, Held, HeldReuse, Plan, PlanKind, Reach, Rule, State, closing_text, note_plan,
    reach_for,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// The segment the main detector hands over.
const SEG_START: usize = 1000;
const SEG_END: usize = 20_000;
/// Where the early detector's stretch of voice ended.
const FLOOR: usize = 19_000;
/// The silence a `silence` close waits for, in milliseconds and in samples.
const SILENCE_MS: u32 = 300;
const SILENCE: usize = SILENCE_MS as usize * SAMPLE_RATE as usize / 1000;
/// The silence an early notice reads past the end of the voice: 120 ms.
const NOTICE_SILENCE: usize = 1920;
const BOTH: [HeldReuse; 2] = [HeldReuse::Covering, HeldReuse::Strict];

/// What closed the segment: a `silence` close waited for 300 ms, a limit or a flush for none.
fn closed_by(rule: Rule) -> Decision {
    Decision {
        rule,
        silence_ms: if rule == Rule::Silence { SILENCE_MS } else { 0 },
        plan: None,
        held_gap_ms: None,
    }
}

/// What `reuse` makes of a sentence kept at `range_end` when `rule` closes the
/// segment, with the gap the plan reports.
fn planned(
    reuse: HeldReuse,
    rule: Rule,
    range_end: usize,
    voice_after: bool,
) -> (Plan<&'static str>, Option<Gap>) {
    let mut state: State<&str> = State::default();
    state.hold("held", range_end, FLOOR);
    if voice_after {
        state.note_voice(FLOOR);
    }
    state.plan_with_gap(SEG_START, SEG_END, reach_for(reuse, &closed_by(rule)))
}

fn is_held(plan: &Plan<&str>, start: usize) -> bool {
    matches!(plan, Plan::Held(held, from) if held.solved == "held" && *from == start)
}

/// The `held_gap_ms` the log would carry for a `silence` close.
fn logged_gap(plan: &Plan<&str>, gap: Option<Gap>) -> Option<i32> {
    note_plan(closed_by(Rule::Silence), plan, gap).held_gap_ms
}

/// T-BR-36-001
#[test]
fn br36_a_held_sentence_that_covers_the_closing_silence_is_used_as_the_final() -> TestResult {
    let range_end = SEG_END + NOTICE_SILENCE;
    let (covering, covering_gap) = planned(HeldReuse::Covering, Rule::Silence, range_end, false);
    assert!(is_held(&covering, SEG_START), "{covering:?}");
    let used = note_plan(closed_by(Rule::Silence), &covering, covering_gap);
    assert_eq!(used.plan, Some(PlanKind::Held));
    assert_eq!(used.held_gap_ms, None);
    let (strict, strict_gap) = planned(HeldReuse::Strict, Rule::Silence, range_end, false);
    assert_eq!(strict, Plan::Whole);
    let again = note_plan(closed_by(Rule::Silence), &strict, strict_gap);
    assert_eq!(again.plan, Some(PlanKind::Decode));
    assert_eq!(again.held_gap_ms, Some(120));
    Ok(())
}

/// T-BR-36-002
#[test]
fn a_held_sentence_read_one_window_short_is_used_by_both_flags() -> TestResult {
    for reuse in BOTH {
        let (plan, _) = planned(reuse, Rule::Silence, SEG_END - WINDOW, false);
        assert!(is_held(&plan, SEG_START), "{reuse:?} {plan:?}");
    }
    Ok(())
}

/// T-BR-36-003
#[test]
fn a_held_sentence_read_more_than_one_window_short_is_used_by_covering() -> TestResult {
    let range_end = SEG_END - WINDOW - 1;
    let (covering, _) = planned(HeldReuse::Covering, Rule::Silence, range_end, false);
    assert!(is_held(&covering, SEG_START), "{covering:?}");
    let (strict, gap) = planned(HeldReuse::Strict, Rule::Silence, range_end, false);
    assert_eq!(strict, Plan::Whole);
    assert_eq!(gap, Some(Gap::Short(WINDOW + 1)));
    assert_eq!(logged_gap(&strict, gap), Some(-32));
    Ok(())
}

/// T-BR-36-004
#[test]
fn a_held_sentence_read_to_the_end_of_the_closing_silence_is_used_by_covering() -> TestResult {
    let range_end = SEG_END + SILENCE;
    let (covering, _) = planned(HeldReuse::Covering, Rule::Silence, range_end, false);
    assert!(is_held(&covering, SEG_START), "{covering:?}");
    let (strict, gap) = planned(HeldReuse::Strict, Rule::Silence, range_end, false);
    assert_eq!(strict, Plan::Whole);
    assert_eq!(logged_gap(&strict, gap), Some(300));
    Ok(())
}

/// T-BR-36-005
#[test]
fn a_held_sentence_read_past_the_closing_silence_is_read_again_by_both_flags() -> TestResult {
    for reuse in BOTH {
        let (plan, gap) = planned(reuse, Rule::Silence, SEG_END + SILENCE + 1, false);
        assert_eq!(plan, Plan::Whole, "{reuse:?}");
        assert_eq!(logged_gap(&plan, gap), Some(300), "{reuse:?}");
    }
    Ok(())
}

/// T-BR-36-016
#[test]
fn a_held_sentence_read_short_inside_the_closing_silence_is_used_by_covering() -> TestResult {
    let typical = SEG_END - NOTICE_SILENCE - WINDOW;
    for range_end in [typical, SEG_END - SILENCE] {
        let (covering, _) = planned(HeldReuse::Covering, Rule::Silence, range_end, false);
        assert!(is_held(&covering, SEG_START), "{range_end} {covering:?}");
        let (strict, gap) = planned(HeldReuse::Strict, Rule::Silence, range_end, false);
        assert_eq!(strict, Plan::Whole, "{range_end}");
        assert_eq!(gap, Some(Gap::Short(SEG_END - range_end)), "{range_end}");
    }
    Ok(())
}

/// T-BR-36-017
#[test]
fn a_held_sentence_read_short_past_the_closing_silence_is_read_again_by_both_flags() -> TestResult {
    for reuse in BOTH {
        let (plan, gap) = planned(reuse, Rule::Silence, SEG_END - SILENCE - 1, false);
        assert_eq!(plan, Plan::Whole, "{reuse:?}");
        assert_eq!(logged_gap(&plan, gap), Some(-300), "{reuse:?}");
    }
    Ok(())
}

/// T-BR-36-018
#[test]
fn a_kept_sentence_closed_by_silence_ends_with_the_terminal_mark_of_its_language() -> TestResult {
    let silence = closed_by(Rule::Silence);
    let rows = [
        (
            "en",
            "He would have to pay her the money,",
            "He would have to pay her the money.",
        ),
        ("en", "Done, then; ", "Done, then."),
        ("en", "It is raining.", "It is raining."),
        ("en", "A, B", "A, B"),
        ("en", "", ""),
        ("ja", "雨が降っていますが、", "雨が降っていますが。"),
        ("ja", "雨が降っています。", "雨が降っています。"),
        ("zh", "你好，", "你好，"),
    ];
    for (lang, text, expected) in rows {
        let closed = closing_text(HeldReuse::Covering, &silence, lang, text);
        assert_eq!(closed, expected, "{lang} {text:?}");
    }
    assert_eq!(
        closing_text(HeldReuse::Strict, &silence, "en", "Done,"),
        "Done,"
    );
    for rule in [Rule::MaxSpeech, Rule::Flush] {
        let close = closed_by(rule);
        assert_eq!(
            closing_text(HeldReuse::Covering, &close, "en", "Done,"),
            "Done,",
            "{rule:?}"
        );
    }
    Ok(())
}

/// T-BR-36-006
#[test]
fn a_held_sentence_with_voice_after_it_is_never_used() -> TestResult {
    for reuse in BOTH {
        let (plan, gap) = planned(reuse, Rule::Silence, SEG_END + NOTICE_SILENCE, true);
        assert_eq!(plan, Plan::Whole, "{reuse:?}");
        assert_eq!(logged_gap(&plan, gap), Some(120), "{reuse:?}");
    }
    Ok(())
}

/// T-BR-36-007
#[test]
fn a_close_without_silence_gets_no_wider_reach() -> TestResult {
    for rule in [Rule::MaxSpeech, Rule::Flush] {
        let (plan, gap) = planned(HeldReuse::Covering, rule, SEG_END + NOTICE_SILENCE, false);
        assert_eq!(plan, Plan::Whole, "{rule:?}");
        assert_eq!(logged_gap(&plan, gap), Some(120), "{rule:?}");
        let (short, short_gap) = planned(HeldReuse::Covering, rule, SEG_END - WINDOW - 1, false);
        assert_eq!(short, Plan::Whole, "{rule:?}");
        assert_eq!(logged_gap(&short, short_gap), Some(-32), "{rule:?}");
    }
    Ok(())
}

/// T-BR-36-008
#[test]
fn a_held_sentence_after_a_line_is_used_from_the_line_by_covering() -> TestResult {
    let line = 5000;
    for (reuse, covered) in [(HeldReuse::Covering, true), (HeldReuse::Strict, false)] {
        let mut state: State<&str> = State::default();
        state.confirm(line, line - 100);
        state.note_voice(line + 200);
        state.hold("held", SEG_END + NOTICE_SILENCE, FLOOR);
        let reach = reach_for(reuse, &closed_by(Rule::Silence));
        let (plan, _) = state.plan_with_gap(SEG_START, SEG_END, reach);
        if covered {
            assert!(is_held(&plan, line), "{reuse:?} {plan:?}");
        } else {
            assert_eq!(plan, Plan::Rest(line), "{reuse:?}");
        }
    }
    Ok(())
}

/// T-BR-36-009
#[test]
fn the_reach_of_each_flag_and_close_is_the_one_in_the_design() -> TestResult {
    let silence = closed_by(Rule::Silence);
    assert_eq!(
        reach_for(HeldReuse::Covering, &silence),
        Reach {
            behind: SILENCE,
            ahead: SILENCE
        }
    );
    for rule in [Rule::MaxSpeech, Rule::Flush] {
        let close = closed_by(rule);
        assert_eq!(
            reach_for(HeldReuse::Covering, &close),
            Reach::ONE_WINDOW,
            "{rule:?}"
        );
    }
    for rule in [Rule::MaxSpeech, Rule::Flush, Rule::Early] {
        let carrying = Decision {
            silence_ms: SILENCE_MS,
            ..closed_by(rule)
        };
        assert_eq!(
            reach_for(HeldReuse::Covering, &carrying),
            Reach::ONE_WINDOW,
            "{rule:?} that carries a silence"
        );
    }
    assert_eq!(reach_for(HeldReuse::Strict, &silence), Reach::ONE_WINDOW);
    assert_eq!(
        Reach::ONE_WINDOW,
        Reach {
            behind: WINDOW,
            ahead: WINDOW
        }
    );
    Ok(())
}

/// T-BR-36-010
#[test]
fn the_held_gap_is_a_signed_number_of_milliseconds() -> TestResult {
    let kept = Held {
        solved: "held",
        range_end: SEG_END + NOTICE_SILENCE,
        floor: FLOOR,
        voice_after: false,
    };
    let rows = [
        (Plan::Whole, Some(Gap::Over(NOTICE_SILENCE)), Some(120)),
        (Plan::Whole, Some(Gap::Short(WINDOW + 1)), Some(-32)),
        (Plan::Whole, None, None),
        (
            Plan::Held(kept, SEG_START),
            Some(Gap::Over(NOTICE_SILENCE)),
            None,
        ),
        (Plan::Rest(5000), Some(Gap::Over(NOTICE_SILENCE)), Some(120)),
    ];
    for (plan, gap, expected) in rows {
        assert_eq!(logged_gap(&plan, gap), expected, "{plan:?} {gap:?}");
    }
    Ok(())
}

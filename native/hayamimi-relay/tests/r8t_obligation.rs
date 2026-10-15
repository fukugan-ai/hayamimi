//! R8t Q2 (names provisional): the Japanese obligation `なければならなかった`
//! and its kin carry the `なかっ` of a past tense; they are not a negation.
//! A real negation still holds, and so does a source that is negated while the
//! translation is not.

use hayamimi_relay::lang::Lang;
use hayamimi_relay::logging::MemorySink;
use hayamimi_relay::verify::{HoldReason, Verdict, Verifier};

fn verdict(src: &str, translation: &str) -> Result<Verdict, String> {
    let mut sink = MemorySink::default();
    let mut verifier = Verifier::open(None, &mut sink)?;
    Ok(verifier.verify(src, translation, Lang::En, &mut sink))
}

/// The positive control: today the first is held for `negation`.
#[test]
fn r8t_a_past_obligation_is_not_a_negation() -> Result<(), String> {
    let pay = verdict("He had to pay her.", "彼は彼女に払わなければならなかった。")?;
    assert_eq!(pay, Verdict::Ok);
    let long = verdict(
        "he would have to pay her the money which she would now regularly demand",
        "彼は彼女が今定期的に要求するであろう金額を支払わなければならなかった",
    )?;
    assert_eq!(long, Verdict::Ok);
    for translation in [
        "彼は支払わなければならなかった。",
        "彼は支払わないといけなかった。",
        "彼は支払わなくてはならない。",
        "彼は支払わなくてはなりません。",
        "彼は支払わなくてはならなかった。",
        "彼は支払わなくてはいけない。",
        "彼は支払わなくてはいけません。",
        "彼は支払わなくてはいけなかった。",
    ] {
        assert_eq!(
            verdict("He had to pay.", translation)?,
            Verdict::Ok,
            "{translation}"
        );
    }
    Ok(())
}

/// The controls: a real negation holds, a short `ならなかった` is not hidden,
/// and a negated source whose translation is an obligation holds.
#[test]
fn r8t_a_real_negation_is_still_held() -> Result<(), String> {
    let held = Verdict::Held(HoldReason::Negation);
    assert_eq!(
        verdict("It bothered him.", "それは気にならなかった。")?,
        held
    );
    assert_eq!(verdict("He paid.", "彼は支払わなかった。")?, held);
    let negated = verdict("You did not have to pay.", "払わなければならなかった。")?;
    assert_eq!(negated, held);
    Ok(())
}

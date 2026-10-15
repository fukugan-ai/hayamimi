//! RP-49: an English clock time ("at one thirty") and the Japanese `1時半` or
//! `1時30分` name the same time, so V lets the pair through; a wrong hour or
//! minute still holds for `number`.
//!
//! The cause was that `one` is below the 2 that V compares while the Japanese
//! `半` is not a number, so the 30 stood on one side only. English number words
//! are not merged: `thirty-first` stays 31 and `5 million` stays 5,000,000.
//!
//! V is called directly with the built-in tables.

use hayamimi_relay::lang::Lang;
use hayamimi_relay::logging::MemorySink;
use hayamimi_relay::numbers::{MICRO, Tongue, compared_values};
use hayamimi_relay::target::Target;
use hayamimi_relay::verify::{HoldReason, Verdict, Verifier};

const NUMBER: Verdict = Verdict::Held(HoldReason::Number);

const AT_ONE_THIRTY: &str = "At one thirty he went to Rectors for lunch, and when he returned a messenger was waiting for him.";
const JA_HALF_PAST_ONE: &str =
    "1時半になると、彼はレクターズで昼食を取りに行き、戻ると使者が待っていた。";

fn verdict(src_lang: Lang, src: &str, translation: &str) -> Result<Verdict, String> {
    let mut sink = MemorySink::default();
    let mut verifier = Verifier::open(None, &mut sink)?;
    Ok(verifier.verify(src, translation, src_lang, &mut sink))
}

/// Japanese `src` against English `translation`.
fn ja_to_en(src: &str, translation: &str) -> Result<Verdict, String> {
    verdict(Lang::Ja, src, translation)
}

/// T-RP-49-001: the reproduction. Before the change `1時半` against
/// `one thirty` was held for `number`.
#[test]
fn rp49_half_past_one_matches_at_one_thirty() -> Result<(), String> {
    let src = "1時半に会いましょう。";
    assert_eq!(ja_to_en(src, "Let's meet at one thirty.")?, Verdict::Ok);
    assert_eq!(ja_to_en(src, "At one thirty, let's meet.")?, Verdict::Ok);
    Ok(())
}

/// The line of the E2E run: an English source and its Japanese translation.
#[test]
fn rp49_the_e2e_line_goes_out_in_both_directions() -> Result<(), String> {
    assert_eq!(
        verdict(Lang::En, AT_ONE_THIRTY, JA_HALF_PAST_ONE)?,
        Verdict::Ok
    );
    let wrong = "2時半になると、彼はレクターズで昼食を取りに行き、戻ると使者が待っていた。";
    assert_eq!(verdict(Lang::En, AT_ONE_THIRTY, wrong)?, NUMBER);
    Ok(())
}

/// T-RP-49-002: `1時30分` against `one thirty`.
#[test]
fn rp49_one_thirty_in_digits_matches_at_one_thirty() -> Result<(), String> {
    let said = ja_to_en("1時30分に会いましょう。", "Let's meet at one thirty.")?;
    assert_eq!(said, Verdict::Ok);
    Ok(())
}

/// T-RP-49-003: a wrong hour is held.
#[test]
fn rp49_two_thirty_for_half_past_one_is_held() -> Result<(), String> {
    let said = ja_to_en("1時半に会いましょう。", "Let's meet at two thirty.")?;
    assert_eq!(said, NUMBER);
    Ok(())
}

/// T-RP-49-004: a minute word of two words.
#[test]
fn rp49_one_thirty_five_matches_thirty_five_past_one() -> Result<(), String> {
    let said = ja_to_en("1時35分に着きます。", "I arrive at one thirty-five.")?;
    assert_eq!(said, Verdict::Ok);
    Ok(())
}

/// T-RP-49-005: a wrong minute is held.
#[test]
fn rp49_a_different_minute_is_held() -> Result<(), String> {
    let src = "1時35分に着きます。";
    assert_eq!(ja_to_en(src, "I arrive at one thirty-six.")?, NUMBER);
    assert_eq!(ja_to_en(src, "I arrive at one thirty.")?, NUMBER);
    Ok(())
}

/// T-RP-49-006: the ends of the hour and minute ranges.
#[test]
fn rp49_the_ends_of_the_hour_and_minute_ranges_are_read_as_a_clock_time() -> Result<(), String> {
    assert_eq!(
        ja_to_en("12時10分に会います。", "I meet at twelve ten.")?,
        Verdict::Ok
    );
    assert_eq!(
        ja_to_en("12時59分に会います。", "I meet at twelve fifty-nine.")?,
        Verdict::Ok
    );
    Ok(())
}

/// T-RP-49-007: readings outside a clock time did not move.
#[test]
fn rp49_readings_outside_a_clock_time_stay_as_they_were() -> Result<(), String> {
    assert_eq!(
        ja_to_en("30人が来ました。", "Thirty people came.")?,
        Verdict::Ok
    );
    assert_eq!(
        ja_to_en("31日です。", "It is the thirty-first.")?,
        Verdict::Ok
    );
    assert_eq!(
        ja_to_en("500万円です。", "It is 5 million yen.")?,
        Verdict::Ok
    );
    assert_eq!(
        compared_values("the thirty-first", Tongue::English),
        [31 * MICRO]
    );
    assert_eq!(
        compared_values("5 million yen", Tongue::English),
        [5_000_000 * MICRO]
    );
    Ok(())
}

/// T-RP-49-008: an hour word with no minute word is read as before.
#[test]
fn rp49_an_hour_word_without_a_minute_word_is_read_as_before() -> Result<(), String> {
    let src = "1時に会いましょう。";
    assert_eq!(ja_to_en(src, "Let's meet at one.")?, Verdict::Ok);
    assert_eq!(ja_to_en(src, "Let's meet at two.")?, NUMBER);
    Ok(())
}

/// `half past one` and `1時半` agreed before and still do, in both directions.
#[test]
fn rp49_half_past_one_still_matches_half_past_one() -> Result<(), String> {
    let en = "At half past one he went out.";
    let ja = "1時半に彼は出かけた。";
    assert_eq!(verdict(Lang::En, en, ja)?, Verdict::Ok);
    assert_eq!(ja_to_en(ja, en)?, Verdict::Ok);
    Ok(())
}

/// The cause, as values: the English side keeps the 30 alone and the Japanese
/// `半` is not a number in the plain reading.
#[test]
fn rp49_the_plain_reading_has_the_thirty_on_one_side_only() {
    assert_eq!(
        compared_values("At one thirty", Tongue::English),
        [30 * MICRO]
    );
    let japanese = compared_values("1時半に行った。", Tongue::Other);
    assert!(japanese.is_empty(), "{japanese:?}");
}

/// A translation outside the live pair is not asked for the thirty: `一点半`
/// for `1時半` goes out as before.
#[test]
fn rp49_a_translation_into_zh_is_not_asked_for_the_thirty() -> Result<(), String> {
    let Some(target) = Target::from_code("zh") else {
        return Err(String::from("no target zh"));
    };
    let mut sink = MemorySink::default();
    let mut verifier = Verifier::open(None, &mut sink)?;
    let said = verifier.verify_into(
        "rp49-1",
        "1時半に昼食を取りに行った。",
        "一点半去吃午饭了。",
        Lang::Ja,
        &target,
        &mut sink,
    );
    assert_eq!(said, Verdict::Ok);
    Ok(())
}

/// The acceptance test of the row.
#[test]
fn rp49_spelled_clock_time_is_read_as_hour_and_minute() -> Result<(), String> {
    let src = "1時半に会いましょう。";
    assert_eq!(ja_to_en(src, "Let's meet at one thirty.")?, Verdict::Ok);
    assert_eq!(ja_to_en(src, "Let's meet at two thirty.")?, NUMBER);
    assert_eq!(
        ja_to_en("1時30分に会いましょう。", "Let's meet at one thirty.")?,
        Verdict::Ok
    );
    Ok(())
}

//! RP-43: V reads "なさそう", "なさげ" and "なさすぎ" in a Japanese source as
//! negation, so an English translation that keeps the negation goes out and one
//! that drops it is held for `negation`. "なさい", "なさる" and "ごめんなさい"
//! are not negation, and the "ない" row reads as it did.
//!
//! V is called directly with the built-in tables.

use hayamimi_relay::lang::Lang;
use hayamimi_relay::logging::MemorySink;
use hayamimi_relay::verify::{HoldReason, Verdict, Verifier};

/// A Japanese source with "なさそう", "なさげ" or "なさすぎ", a translation that
/// keeps the negation and one that drops it.
const NEGATED: [(&str, &str, &str); 3] = [
    (
        "あまり美味しくなさそうです。",
        "It doesn't seem very tasty.",
        "It seems very tasty.",
    ),
    (
        "彼は興味がなさげでした。",
        "He was not interested.",
        "He was interested.",
    ),
    (
        "やる気がなさすぎます。",
        "He has no motivation at all.",
        "He has too much motivation.",
    ),
];

/// A source with a word that holds "なさ" and is not negation, with a
/// translation that has no negation either.
const NOT_NEGATED: [(&str, &str); 4] = [
    ("早く食べなさい。", "Eat quickly."),
    ("ごめんなさい。", "I'm sorry."),
    ("先生がお話しなさる。", "The teacher speaks."),
    ("皆さん、静かにしなさい。", "Everyone, be quiet."),
];

/// The "ない" row, as before: a negation kept and a negation dropped.
const PLAIN_NAI: (&str, &str, &str) = ("それは美味しくない。", "It is not tasty.", "It is tasty.");

/// Positive adjectives that end in "な" and take "さそう" or "さげ".
const POSITIVE: [(&str, &str); 3] = [
    ("つまらなさそうな顔をしている。", "He seems bored."),
    ("少なさげな数字です。", "The number looks small."),
    ("危なさそうな道です。", "The road seems dangerous."),
];

fn verdict(src: &str, translation: &str) -> Result<Verdict, String> {
    let mut sink = MemorySink::default();
    let mut verifier = Verifier::open(None, &mut sink)?;
    Ok(verifier.verify(src, translation, Lang::Ja, &mut sink))
}

/// The acceptance test of the row.
#[test]
fn rp43_nasasou_is_read_as_negation() -> Result<(), String> {
    for (src, kept, dropped) in NEGATED {
        assert_eq!(verdict(src, kept)?, Verdict::Ok, "{src}: kept");
        assert_eq!(
            verdict(src, dropped)?,
            Verdict::Held(HoldReason::Negation),
            "{src}: dropped"
        );
    }
    for (src, translation) in NOT_NEGATED {
        assert_eq!(verdict(src, translation)?, Verdict::Ok, "{src}");
    }
    Ok(())
}

/// The negative control: the "ない" row reads as it did.
#[test]
fn rp43_the_nai_row_reads_as_before() -> Result<(), String> {
    let (src, kept, dropped) = PLAIN_NAI;
    assert_eq!(verdict(src, kept)?, Verdict::Ok);
    assert_eq!(verdict(src, dropped)?, Verdict::Held(HoldReason::Negation));
    Ok(())
}

/// A positive adjective with the stem "な" is not negation, so
/// a translation without a negation goes out.
#[test]
fn rp43_a_positive_adjective_with_the_stem_na_is_not_negation() -> Result<(), String> {
    for (src, translation) in POSITIVE {
        assert_eq!(verdict(src, translation)?, Verdict::Ok, "{src}");
    }
    Ok(())
}

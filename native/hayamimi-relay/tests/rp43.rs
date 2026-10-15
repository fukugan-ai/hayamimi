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

/// Positive adjectives that end in "な" and take "さそう" or "さげ", and the
/// set words "申し訳ない", "仕方ない", "情けない" and "頼りない" in both forms.
const POSITIVE: [(&str, &str); 11] = [
    ("つまらなさそうな顔をしている。", "He seems bored."),
    ("少なさげな数字です。", "The number looks small."),
    ("危なさそうな道です。", "The road seems dangerous."),
    ("申し訳なさそうな顔をしている。", "He seems apologetic."),
    ("仕方なさそうに笑った。", "He seems resigned and smiled."),
    ("情けなさそうな声だった。", "His voice seemed pitiful."),
    ("頼りなさそうな人です。", "He seems unreliable."),
    ("申し訳ない気持ちです。", "I feel sorry."),
    ("仕方ない話です。", "It is unavoidable."),
    ("情けない結果です。", "The result is pitiful."),
    ("頼りない人です。", "He is unreliable."),
];

/// The three words followed by more text: a translation that keeps the
/// negation and one that drops it.
const CONJUGATED: [(&str, &str, &str); 3] = [
    (
        "ご飯が美味しくなさそうに見える。",
        "It does not seem tasty.",
        "The food looks tasty.",
    ),
    (
        "興味がなさげな顔だった。",
        "He was not interested.",
        "He looked interested.",
    ),
    (
        "やる気がなさすぎて困る。",
        "He has no motivation, which is a problem.",
        "He has motivation, which is a problem.",
    ),
];

/// The same first two sources with a translation that keeps the negation in a
/// form the English tables do not read.
const NEGATION_THE_TABLES_DO_NOT_READ: [(&str, &str, HoldReason); 2] = [
    (
        "ご飯が美味しくなさそうに見える。",
        "The food does not look tasty.",
        HoldReason::Uncertainty,
    ),
    (
        "興味がなさげな顔だった。",
        "He looked uninterested.",
        HoldReason::Negation,
    ),
];

/// "なさそう" after "では": a translation that keeps the negation and one that
/// drops it.
const AFTER_DEWA: (&str, &str, &str) = (
    "それは本物ではなさそうです。",
    "It does not seem to be genuine.",
    "It seems to be genuine.",
);

/// A source without a negation word and a translation that adds one.
const NEGATION_ADDED: (&str, &str) = ("あまり美味しそうです。", "It does not seem tasty.");

/// "〜なさそう" followed by a negation, with a translation that carries a
/// hedge word of the table ("seem", "appear").
const NEGATED_WITH_HEDGE: [(&str, &str); 2] = [
    ("申し訳なさそうではない。", "He does not seem apologetic."),
    ("仕方なさそうにしない。", "He does not appear resigned."),
];

/// The same two sources with a translation that keeps the negation and has no
/// hedge word of the table ("look" alone is not one).
const NEGATED_WITHOUT_HEDGE: [(&str, &str); 2] = [
    ("申し訳なさそうではない。", "He is not apologetic."),
    ("仕方なさそうにしない。", "He does not look resigned."),
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

/// T-RP-43-006: the words are read inside a longer form too.
#[test]
fn rp43_the_conjugated_forms_are_read_as_negation_too() -> Result<(), String> {
    for (src, kept, dropped) in CONJUGATED {
        assert_eq!(verdict(src, kept)?, Verdict::Ok, "{src}: kept");
        assert_eq!(
            verdict(src, dropped)?,
            Verdict::Held(HoldReason::Negation),
            "{src}: dropped"
        );
    }
    Ok(())
}

/// T-RP-43-007: the character before "なさそう" does not matter.
#[test]
fn rp43_a_nasasou_after_dewa_is_read_as_negation() -> Result<(), String> {
    let (src, kept, dropped) = AFTER_DEWA;
    assert_eq!(verdict(src, kept)?, Verdict::Ok);
    assert_eq!(verdict(src, dropped)?, Verdict::Held(HoldReason::Negation));
    Ok(())
}

/// T-RP-43-008: the check compares presence on both sides, so a negation the
/// translation adds to a source without one is held too.
#[test]
fn rp43_a_negation_added_to_a_plain_source_is_held() -> Result<(), String> {
    let (src, translation) = NEGATION_ADDED;
    assert_eq!(
        verdict(src, translation)?,
        Verdict::Held(HoldReason::Negation)
    );
    Ok(())
}

/// T-RP-43-010: pins the behavior of H59. The "そう" of "〜なさそう" is a hedge
/// word of the Japanese table whatever follows it, so a translation that
/// keeps the negation and has a hedge word of the English table passes.
#[test]
fn rp43_a_negated_nasasou_with_a_hedge_word_in_the_translation_passes() -> Result<(), String> {
    for (src, translation) in NEGATED_WITH_HEDGE {
        assert_eq!(verdict(src, translation)?, Verdict::Ok, "{src}");
    }
    Ok(())
}

/// T-RP-43-011: pins the behavior of H59. The same sources with a correct
/// translation that has no hedge word of the table are held for
/// `uncertainty`. This row falls on the day H59 is fixed.
#[test]
fn rp43_a_negated_nasasou_without_a_hedge_word_in_the_translation_is_held_for_uncertainty()
-> Result<(), String> {
    for (src, translation) in NEGATED_WITHOUT_HEDGE {
        assert_eq!(
            verdict(src, translation)?,
            Verdict::Held(HoldReason::Uncertainty),
            "{src}"
        );
    }
    Ok(())
}

/// T-RP-43-012: pins the current tables. A correct translation that keeps the
/// negation in a form the tables do not read is held. This row falls on the
/// day the tables read those forms.
#[test]
fn rp43_a_conjugated_form_whose_translation_the_tables_do_not_read_is_held_for_now()
-> Result<(), String> {
    for (src, translation, reason) in NEGATION_THE_TABLES_DO_NOT_READ {
        assert_eq!(
            verdict(src, translation)?,
            Verdict::Held(reason),
            "{src}: {translation}"
        );
    }
    Ok(())
}

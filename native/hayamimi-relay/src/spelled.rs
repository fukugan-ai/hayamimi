//! Words that stand for a number in a translation: Spanish number words,
//! English ordinals, and the month names that a Japanese `N月` becomes.
//!
//! Verification compares the numbers of a source and a translation, so a
//! translation that writes `dos` or `December` must read as 2 and 12. These
//! tables are consulted only for the language they belong to (see
//! [`Tongue`]); in any other text the same letters are ordinary words (the
//! English `once` is not 11).

use crate::numbers::{Kind, MICRO, Tongue};

/// The Spanish number words, accents folded away, with their kind and value.
/// `un` and `una` are articles and are left out; 1 is never compared.
const SPANISH: [(&str, Kind, u128); 58] = [
    ("uno", Kind::Unit, 1),
    ("dos", Kind::Unit, 2),
    ("tres", Kind::Unit, 3),
    ("cuatro", Kind::Unit, 4),
    ("cinco", Kind::Unit, 5),
    ("seis", Kind::Unit, 6),
    ("siete", Kind::Unit, 7),
    ("ocho", Kind::Unit, 8),
    ("nueve", Kind::Unit, 9),
    ("diez", Kind::Teen, 10),
    ("once", Kind::Teen, 11),
    ("doce", Kind::Teen, 12),
    ("trece", Kind::Teen, 13),
    ("catorce", Kind::Teen, 14),
    ("quince", Kind::Teen, 15),
    ("dieciseis", Kind::Teen, 16),
    ("diecisiete", Kind::Teen, 17),
    ("dieciocho", Kind::Teen, 18),
    ("diecinueve", Kind::Teen, 19),
    ("veinte", Kind::Tens, 20),
    ("veintiuno", Kind::Teen, 21),
    ("veintiun", Kind::Teen, 21),
    ("veintidos", Kind::Teen, 22),
    ("veintitres", Kind::Teen, 23),
    ("veinticuatro", Kind::Teen, 24),
    ("veinticinco", Kind::Teen, 25),
    ("veintiseis", Kind::Teen, 26),
    ("veintisiete", Kind::Teen, 27),
    ("veintiocho", Kind::Teen, 28),
    ("veintinueve", Kind::Teen, 29),
    ("treinta", Kind::Tens, 30),
    ("cuarenta", Kind::Tens, 40),
    ("cincuenta", Kind::Tens, 50),
    ("sesenta", Kind::Tens, 60),
    ("setenta", Kind::Tens, 70),
    ("ochenta", Kind::Tens, 80),
    ("noventa", Kind::Tens, 90),
    ("cien", Kind::Hundreds, 100),
    ("ciento", Kind::Hundreds, 100),
    ("doscientos", Kind::Hundreds, 200),
    ("doscientas", Kind::Hundreds, 200),
    ("trescientos", Kind::Hundreds, 300),
    ("trescientas", Kind::Hundreds, 300),
    ("cuatrocientos", Kind::Hundreds, 400),
    ("cuatrocientas", Kind::Hundreds, 400),
    ("quinientos", Kind::Hundreds, 500),
    ("quinientas", Kind::Hundreds, 500),
    ("seiscientos", Kind::Hundreds, 600),
    ("seiscientas", Kind::Hundreds, 600),
    ("setecientos", Kind::Hundreds, 700),
    ("setecientas", Kind::Hundreds, 700),
    ("ochocientos", Kind::Hundreds, 800),
    ("ochocientas", Kind::Hundreds, 800),
    ("novecientos", Kind::Hundreds, 900),
    ("novecientas", Kind::Hundreds, 900),
    ("mil", Kind::Scale, 1_000),
    ("millon", Kind::Scale, 1_000_000),
    ("millones", Kind::Scale, 1_000_000),
];

/// English ordinals from the third on. `first` and `second` are left out
/// because they are also "primary" and a unit of time; they count only after
/// a tens word (`thirty-first`).
const ORDINALS: [(&str, Kind, u128); 25] = [
    ("third", Kind::Unit, 3),
    ("fourth", Kind::Unit, 4),
    ("fifth", Kind::Unit, 5),
    ("sixth", Kind::Unit, 6),
    ("seventh", Kind::Unit, 7),
    ("eighth", Kind::Unit, 8),
    ("ninth", Kind::Unit, 9),
    ("tenth", Kind::Teen, 10),
    ("eleventh", Kind::Teen, 11),
    ("twelfth", Kind::Teen, 12),
    ("thirteenth", Kind::Teen, 13),
    ("fourteenth", Kind::Teen, 14),
    ("fifteenth", Kind::Teen, 15),
    ("sixteenth", Kind::Teen, 16),
    ("seventeenth", Kind::Teen, 17),
    ("eighteenth", Kind::Teen, 18),
    ("nineteenth", Kind::Teen, 19),
    ("twentieth", Kind::Tens, 20),
    ("thirtieth", Kind::Tens, 30),
    ("fortieth", Kind::Tens, 40),
    ("fiftieth", Kind::Tens, 50),
    ("sixtieth", Kind::Tens, 60),
    ("seventieth", Kind::Tens, 70),
    ("eightieth", Kind::Tens, 80),
    ("ninetieth", Kind::Tens, 90),
];

const ENGLISH_MONTHS: [&str; 12] = [
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];

const SPANISH_MONTHS: [&str; 12] = [
    "enero",
    "febrero",
    "marzo",
    "abril",
    "mayo",
    "junio",
    "julio",
    "agosto",
    "septiembre",
    "octubre",
    "noviembre",
    "diciembre",
];

fn unaccent(c: char) -> char {
    match c {
        'á' => 'a',
        'é' => 'e',
        'í' => 'i',
        'ó' => 'o',
        'ú' | 'ü' => 'u',
        'ñ' => 'n',
        _ => c,
    }
}

/// A Spanish word with its accents folded away and its first letter in lower
/// case. The rest of the word must already be lower case to be found, so
/// `Dos` is a number and `DOS` is not.
pub(crate) fn fold(word: &str) -> String {
    let mut chars = word.chars();
    let mut folded = String::with_capacity(word.len());
    if let Some(first) = chars.next() {
        folded.extend(first.to_lowercase().map(unaccent));
    }
    folded.extend(chars.map(unaccent));
    folded
}

/// The kind and value of a folded Spanish number word.
pub(crate) fn spanish_kind(folded: &str) -> Option<(Kind, u128)> {
    SPANISH
        .iter()
        .find(|(word, _, _)| *word == folded)
        .map(|(_, kind, value)| (*kind, *value))
}

/// The kind and value of an English ordinal. `first` and `second` count only
/// after a tens word, as in `thirty-first`.
pub(crate) fn english_ordinal(word: &str, after_tens: bool) -> Option<(Kind, u128)> {
    if after_tens && word.eq_ignore_ascii_case("first") {
        return Some((Kind::Unit, 1));
    }
    if after_tens && word.eq_ignore_ascii_case("second") {
        return Some((Kind::Unit, 2));
    }
    ORDINALS
        .iter()
        .find(|(text, _, _)| word.eq_ignore_ascii_case(text))
        .map(|(_, kind, value)| (*kind, *value))
}

/// The number of the month that `tokens[at]` names, for the language of the
/// text. An English month name is capitalised, and `May` and `March` also
/// need a word before them or a number after them, since the lower-case
/// words are common and a sentence can open with either.
fn month_of(tokens: &[&str], at: usize, tongue: Tongue) -> Option<u128> {
    let token = tokens.get(at)?;
    let names = match tongue {
        Tongue::English => &ENGLISH_MONTHS,
        Tongue::Spanish => &SPANISH_MONTHS,
        Tongue::Other | Tongue::Chinese | Tongue::Korean => return None,
    };
    let number = names
        .iter()
        .position(|name| token.eq_ignore_ascii_case(name))?;
    if tongue == Tongue::English {
        let ambiguous = matches!(number, 2 | 4);
        let numeric_next = tokens
            .get(at + 1)
            .is_some_and(|next| next.bytes().all(|byte| byte.is_ascii_digit()));
        let capital = token.starts_with(char::is_uppercase);
        if !capital || (ambiguous && at == 0 && !numeric_next) {
            return None;
        }
    }
    u128::try_from(number + 1).ok()
}

/// Whether `c` ends a Japanese hour numeral: a digit or a kanji digit.
fn is_hour_numeral(c: char) -> bool {
    c.is_ascii_digit() || ('０'..='９').contains(&c) || "〇零一二三四五六七八九十".contains(c)
}

/// Whether `word` names an hour of the clock: digits or `one` to `twelve`.
fn is_english_hour(word: &str) -> bool {
    const HOURS: [&str; 12] = [
        "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven",
        "twelve",
    ];
    word.bytes().all(|b| b.is_ascii_digit()) || HOURS.iter().any(|h| word.eq_ignore_ascii_case(h))
}

/// How many times `text` says "half past <hour>" (`half past one`).
fn english_half_hours(text: &str) -> usize {
    let mut count = 0;
    let (mut after_half, mut after_past) = (false, false);
    for word in text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
    {
        if after_past && is_english_hour(word) {
            count += 1;
        }
        after_past = after_half && word.eq_ignore_ascii_case("past");
        after_half = word.eq_ignore_ascii_case("half");
    }
    count
}

/// How many times `text` says "<hour>時半" (`1時半`, `一時半`).
fn japanese_half_hours(text: &str) -> usize {
    text.match_indices("時半")
        .filter(|(at, _)| text[..*at].chars().next_back().is_some_and(is_hour_numeral))
        .count()
}

/// The 30, in millionths, that each half hour spelled in words stands for:
/// Japanese `N時半` and English `half past N`. Other languages have none.
pub(crate) fn half_hour_values(text: &str, tongue: Tongue) -> Vec<u128> {
    let count = match tongue {
        Tongue::English => english_half_hours(text),
        Tongue::Other => japanese_half_hours(text),
        Tongue::Spanish | Tongue::Chinese | Tongue::Korean => 0,
    };
    vec![30 * MICRO; count]
}

/// The month numbers, in millionths, that the month names of a text stand for.
pub(crate) fn month_values(text: &str, tongue: Tongue) -> Vec<u128> {
    let tokens: Vec<&str> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect();
    (0..tokens.len())
        .filter_map(|at| month_of(&tokens, at, tongue))
        .map(|month| month * MICRO)
        .collect()
}

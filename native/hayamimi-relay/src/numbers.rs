//! Reading numbers out of free text.
//!
//! One reader serves both the verification step and the unit annotation, so
//! "what number is this" has a single answer. It reads Arabic digits (ASCII
//! and full-width, with `,` digit grouping and a decimal point, optionally
//! followed by 万/億/兆 or an English scale word, and for Chinese, Korean and
//! Spanish by a run of their scale words: `5百万`, `5백만`, `3 mil millones`),
//! Japanese numerals
//! (五百万, 二〇二六, 三千), and English number words (fifty thousand,
//! a hundred, twenty-five).
//!
//! Values are kept as whole millionths so that equality is exact.

use crate::spelled;

/// Millionths per unit: the scale of [`Number::micro`].
pub const MICRO: u128 = 1_000_000;

/// A number found in a text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Number {
    /// Byte offset of the first character of the number.
    pub start: usize,
    /// Byte offset just past the last character of the number.
    pub end: usize,
    /// The magnitude in millionths.
    pub micro: u128,
    /// A minus sign stood directly before the number.
    pub negative: bool,
}

/// The language a text is written in, where that decides which words are
/// numbers: Spanish number words, English ordinals and month names are read
/// only in their own language.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tongue {
    Other,
    English,
    Spanish,
    /// Digits are followed by `十百千` and `万亿` (`5百万`).
    Chinese,
    /// Digits are followed by `십백천` and `만억조` (`5백만`).
    Korean,
}

impl Tongue {
    /// The tongue of an ISO 639-1 code; any language without word tables is `Other`.
    pub fn of_code(code: &str) -> Tongue {
        match code {
            "en" => Tongue::English,
            "es" => Tongue::Spanish,
            "zh" => Tongue::Chinese,
            "ko" => Tongue::Korean,
            _ => Tongue::Other,
        }
    }
}

/// Every number in `text`, in order.
pub fn extract(text: &str) -> Vec<Number> {
    extract_in(text, Tongue::Other)
}

/// Every number in `text`, in order, reading the word tables of `tongue`.
pub fn extract_in(text: &str, tongue: Tongue) -> Vec<Number> {
    let scan = Scan::new(text, tongue);
    let mut found = Vec::new();
    let mut i = 0;
    while i < scan.len() {
        match scan.number_at(i) {
            Some((micro, end)) => {
                found.push(Number {
                    start: scan.byte(i),
                    end: scan.byte(end),
                    micro,
                    negative: scan.negative_before(i),
                });
                i = end;
            }
            // A digit run too long to hold is skipped whole, never read from
            // its middle.
            None if scan.digit(i).is_some() => {
                while scan.digit(i).is_some() {
                    i += 1;
                }
            }
            None => i += 1,
        }
    }
    found
}

/// The values the verification step compares: every number of 2 or more,
/// as millionths, sorted. A value of 1 is left out because phrases such as
/// "もう一度" and "a week" carry a 1 on one side only. A month name counts as
/// its month, since a Japanese `7月` is written `July` elsewhere.
pub fn compared_values(text: &str, tongue: Tongue) -> Vec<u128> {
    let mut values: Vec<u128> = extract_in(text, tongue)
        .into_iter()
        .map(|number| number.micro)
        .collect();
    values.extend(spelled::month_values(text, tongue));
    values.retain(|micro| *micro >= 2 * MICRO);
    values.sort_unstable();
    values
}

/// Whether `c` is a letter of a script written with spaces between words,
/// as opposed to Han, kana and Hangul.
fn is_spaced_letter(c: char) -> bool {
    c.is_alphabetic() && u32::from(c) < 0x2E80
}

struct Scan<'a> {
    text: &'a str,
    chars: Vec<(usize, char)>,
    tongue: Tongue,
}

impl<'a> Scan<'a> {
    fn new(text: &'a str, tongue: Tongue) -> Scan<'a> {
        Scan {
            text,
            chars: text.char_indices().collect(),
            tongue,
        }
    }

    fn len(&self) -> usize {
        self.chars.len()
    }

    /// The byte offset of character `i`; the text length past the end.
    fn byte(&self, i: usize) -> usize {
        match self.chars.get(i) {
            Some((offset, _)) => *offset,
            None => self.text.len(),
        }
    }

    fn ch(&self, i: usize) -> Option<char> {
        self.chars.get(i).map(|(_, c)| *c)
    }

    fn digit(&self, i: usize) -> Option<u32> {
        match self.ch(i)? {
            c @ '0'..='9' => Some(u32::from(c) - u32::from('0')),
            c @ '０'..='９' => Some(u32::from(c) - u32::from('０')),
            _ => None,
        }
    }

    fn number_at(&self, i: usize) -> Option<(u128, usize)> {
        let c = self.ch(i)?;
        if self.digit(i).is_some() {
            return if self.is_unit_exponent(i) {
                None
            } else {
                self.arabic(i)
            };
        }
        if kanji_digit(c).is_some() || small_unit(c).is_some() {
            return if self.is_percent_prefix(i) {
                None
            } else {
                self.kanji(i)
            };
        }
        let word_start = c.is_ascii_alphabetic()
            && (i == 0
                || !self
                    .ch(i - 1)
                    .is_some_and(|prev| prev.is_ascii_alphanumeric() || is_spaced_letter(prev)));
        if !word_start {
            return None;
        }
        match self.tongue {
            Tongue::Spanish => self.spanish(i).or_else(|| self.english(i)),
            Tongue::English | Tongue::Other | Tongue::Chinese | Tongue::Korean => self.english(i),
        }
    }

    /// Whether the digit at `i` is the 2 or 3 of a length unit (`km2`, `m3`).
    /// It is written with a digit but is an exponent, not a quantity.
    fn is_unit_exponent(&self, i: usize) -> bool {
        if !matches!(self.digit(i), Some(2 | 3)) || self.digit(i + 1).is_some() {
            return false;
        }
        let start = (0..i)
            .rev()
            .take_while(|&at| self.ch(at).is_some_and(|c| c.is_ascii_lowercase()))
            .last();
        let Some(start) = start else {
            return false;
        };
        let glued_before = start > 0 && self.ch(start - 1).is_some_and(char::is_alphanumeric);
        !glued_before
            && matches!(
                &self.text[self.byte(start)..self.byte(i)],
                "m" | "cm" | "km" | "mm" | "dm"
            )
    }

    /// Whether the numeral at `i` is the 百 or 千 of 百分之 (percent), which
    /// names a denominator and not a quantity of its own.
    fn is_percent_prefix(&self, i: usize) -> bool {
        matches!(self.ch(i), Some('百' | '千'))
            && self.ch(i + 1) == Some('分')
            && self.ch(i + 2) == Some('之')
    }

    fn negative_before(&self, i: usize) -> bool {
        if i == 0 || !matches!(self.ch(i - 1), Some('-' | '−')) {
            return false;
        }
        i == 1 || !self.ch(i - 2).is_some_and(char::is_alphanumeric)
    }

    /// Whether character `j` is a separator that groups exactly three
    /// digits: a `,`, or a space between a group of at most three digits and
    /// the three that follow (`19 500`, written so in Spanish and French).
    fn is_group_comma(&self, j: usize) -> bool {
        let three_follow =
            (1..=3).all(|k| self.digit(j + k).is_some()) && self.digit(j + 4).is_none();
        match self.ch(j) {
            Some(',' | '，') => three_follow,
            Some(' ' | '\u{a0}' | '\u{202f}') => three_follow && self.head_is_short(j),
            _ => false,
        }
    }

    /// Whether the digit run that ends just before `j` has one to three digits
    /// and does not continue a longer number (a decimal or a `,` group).
    fn head_is_short(&self, j: usize) -> bool {
        let run = (0..j)
            .rev()
            .take_while(|&at| self.digit(at).is_some())
            .count();
        let before = j.checked_sub(run + 1).and_then(|at| self.ch(at));
        (1..=3).contains(&run) && !matches!(before, Some(',' | '，' | '.' | '．'))
    }

    fn arabic(&self, i: usize) -> Option<(u128, usize)> {
        let mut j = i;
        let mut int: u128 = 0;
        while let Some(d) = self.digit(j) {
            int = int.checked_mul(10)?.checked_add(u128::from(d))?;
            j += 1;
            if self.is_group_comma(j) {
                j += 1;
            }
        }
        let mut micro = int.checked_mul(MICRO)?;
        if matches!(self.ch(j), Some('.' | '．')) && self.digit(j + 1).is_some() {
            j += 1;
            let mut place = MICRO / 10;
            while let Some(d) = self.digit(j) {
                micro = micro.checked_add(place * u128::from(d))?;
                place /= 10;
                j += 1;
            }
        }
        if let Some((scale, end)) = self.digit_scales(j) {
            return Some((micro.checked_mul(scale)?, end));
        }
        if let Some(scale) = self.ch(j).and_then(big_unit) {
            return Some((micro.checked_mul(scale)?, j + 1));
        }
        if let Some((scale, end)) = self.english_scale(j) {
            return Some((micro.checked_mul(scale)?, end));
        }
        Some((micro, j))
    }

    /// The product of the run of scale words right after digits, in the
    /// tongue of the text (`5百万`, `5백만`, `3 mil millones`), and where the
    /// run ends. `None` when no scale word follows.
    fn digit_scales(&self, j: usize) -> Option<(u128, usize)> {
        match self.tongue {
            Tongue::Chinese => self.unit_run(j, |c| small_unit(c).or_else(|| big_unit(c))),
            Tongue::Korean => self.unit_run(j, korean_unit),
            Tongue::Spanish => self.spanish_scales(j),
            Tongue::English | Tongue::Other => None,
        }
    }

    /// A run of one-character scale units starting at `j`, multiplied.
    fn unit_run(&self, j: usize, unit: fn(char) -> Option<u128>) -> Option<(u128, usize)> {
        let mut scale: u128 = 1;
        let mut end = j;
        while let Some(value) = self.ch(end).and_then(unit)
            && !self.is_percent_prefix(end)
        {
            scale = scale.checked_mul(value)?;
            end += 1;
        }
        (end > j).then_some((scale, end))
    }

    /// Spanish scale words after digits, each optionally after one space.
    fn spanish_scales(&self, j: usize) -> Option<(u128, usize)> {
        let mut scale: u128 = 1;
        let mut end = j;
        loop {
            let start = if self.ch(end) == Some(' ') {
                end + 1
            } else {
                end
            };
            let Some((value, next)) = self.spanish_scale_word(start) else {
                break;
            };
            scale = scale.checked_mul(value)?;
            end = next;
        }
        (end > j).then_some((scale, end))
    }

    fn spanish_scale_word(&self, at: usize) -> Option<(u128, usize)> {
        SPANISH_SCALES.iter().find_map(|(word, value)| {
            self.word_is(at, word)
                .then(|| (*value, at + word.chars().count()))
        })
    }

    /// Whether `word` (lower case) stands at `at`, in any case, and is not
    /// the start of a longer word.
    fn word_is(&self, at: usize, word: &str) -> bool {
        let matches = word.chars().enumerate().all(|(k, expected)| {
            self.ch(at + k)
                .is_some_and(|c| c.to_lowercase().eq(std::iter::once(expected)))
        });
        matches
            && !self
                .ch(at + word.chars().count())
                .is_some_and(char::is_alphanumeric)
    }

    /// An English scale word (optionally after one space) right after digits.
    fn english_scale(&self, j: usize) -> Option<(u128, usize)> {
        let start = if self.ch(j) == Some(' ') { j + 1 } else { j };
        let (word, end) = self.word(start)?;
        let scale = match word.to_ascii_lowercase().as_str() {
            "hundred" => 100,
            "thousand" => 1_000,
            "million" => 1_000_000,
            "billion" => 1_000_000_000,
            "trillion" => 1_000_000_000_000,
            _ => return None,
        };
        Some((scale, end))
    }

    /// Japanese numerals: positional (二〇二六) or with 十百千 and 万億兆.
    fn kanji(&self, i: usize) -> Option<(u128, usize)> {
        let mut j = i;
        let mut total: u128 = 0;
        let mut group: u128 = 0;
        let mut current: u128 = 0;
        let mut has_current = false;
        while let Some(c) = self.ch(j) {
            if let Some(d) = kanji_digit(c) {
                current = current.checked_mul(10)?.checked_add(d)?;
                has_current = true;
            } else if let Some(unit) = small_unit(c) {
                let count = if has_current { current } else { 1 };
                group = group.checked_add(count.checked_mul(unit)?)?;
                current = 0;
                has_current = false;
            } else if let Some(unit) = big_unit(c) {
                // A lone 万 (as in 万が一) is not a number.
                if !has_current && group == 0 {
                    break;
                }
                group = group.checked_add(current)?;
                total = total.checked_add(group.checked_mul(unit)?)?;
                group = 0;
                current = 0;
                has_current = false;
            } else {
                break;
            }
            j += 1;
        }
        if j == i {
            return None;
        }
        total = total.checked_add(group)?.checked_add(current)?;
        Some((total.checked_mul(MICRO)?, j))
    }

    /// A maximal run of ASCII letters starting at `j`.
    fn word(&self, j: usize) -> Option<(&'a str, usize)> {
        if !self.ch(j)?.is_ascii_alphabetic() {
            return None;
        }
        let mut end = j;
        while self.ch(end).is_some_and(|c| c.is_ascii_alphabetic()) {
            end += 1;
        }
        // The run is a prefix of a longer word (`tenía`), not a word.
        if self.ch(end).is_some_and(is_spaced_letter) {
            return None;
        }
        Some((&self.text[self.byte(j)..self.byte(end)], end))
    }

    /// A maximal run of letters of a spaced script starting at `j`.
    fn letters(&self, j: usize) -> Option<(&'a str, usize)> {
        let mut end = j;
        while self.ch(end).is_some_and(is_spaced_letter) {
            end += 1;
        }
        (end > j).then(|| (&self.text[self.byte(j)..self.byte(end)], end))
    }

    /// The kind and value of a number word in the tongue of the text.
    fn kind_of(&self, word: &str, after_tens: bool) -> Option<(Kind, u128)> {
        let plain = word_kind(word);
        if plain.is_some() || self.tongue != Tongue::English {
            return plain;
        }
        spelled::english_ordinal(word, after_tens)
    }

    /// Spanish number words: "dos", "treinta y cinco", "dos mil quinientos".
    /// The `cien` of `por cien(to)` (percent) is not a quantity.
    fn spanish(&self, i: usize) -> Option<(u128, usize)> {
        let mut acc = Words::default();
        let mut j = i;
        let mut end = i;
        let mut read = 0;
        while let Some((word, word_end)) = self.letters(j) {
            let folded = spelled::fold(word);
            let Some((kind, value)) = spelled::spanish_kind(&folded) else {
                if folded == "y" && self.joins_unit(&acc, word_end) {
                    j = word_end + 1;
                    continue;
                }
                break;
            };
            if !acc.accepts(kind) || (read == 0 && self.is_percent_word(&folded, i)) {
                break;
            }
            acc.apply(kind, value);
            read += 1;
            end = word_end;
            if self.ch(word_end) != Some(' ') {
                break;
            }
            j = word_end + 1;
        }
        (read > 0).then(|| (acc.finish().saturating_mul(MICRO), end))
    }

    /// Whether a `y` after a tens word is followed by a unit: "treinta y cinco".
    fn joins_unit(&self, acc: &Words, y_end: usize) -> bool {
        acc.last == Some(Kind::Tens)
            && self.ch(y_end) == Some(' ')
            && self.letters(y_end + 1).is_some_and(|(next, _)| {
                matches!(
                    spelled::spanish_kind(&spelled::fold(next)),
                    Some((Kind::Unit, _))
                )
            })
    }

    /// Whether `folded`, the first word of a number at `i`, is the `cien(to)`
    /// of `por ciento`.
    fn is_percent_word(&self, folded: &str, i: usize) -> bool {
        matches!(folded, "cien" | "ciento")
            && self.text[..self.byte(i)]
                .trim_end()
                .to_lowercase()
                .ends_with(" por")
    }

    /// English number words: "fifty thousand", "a hundred", "twenty-five".
    fn english(&self, i: usize) -> Option<(u128, usize)> {
        let mut acc = Words::default();
        let mut j = i;
        let (first, first_end) = self.word(i)?;
        if first.eq_ignore_ascii_case("a") || first.eq_ignore_ascii_case("an") {
            let (next, _) = self.word(first_end + 1)?;
            if self.ch(first_end) != Some(' ')
                || !matches!(word_kind(next), Some((Kind::Hundred | Kind::Scale, _)))
            {
                return None;
            }
            acc.apply(Kind::Unit, 1);
            j = first_end + 1;
        }
        let mut end = i;
        let mut real_words = 0;
        while let Some((word, word_end)) = self.word(j) {
            let Some((kind, value)) = self.kind_of(word, acc.last == Some(Kind::Tens)) else {
                // "one hundred and five": "and" joins when a number follows.
                let joins = word.eq_ignore_ascii_case("and")
                    && acc.can_join_and()
                    && self.ch(word_end) == Some(' ')
                    && self
                        .word(word_end + 1)
                        .is_some_and(|(next, _)| word_kind(next).is_some());
                if joins {
                    j = word_end + 1;
                    continue;
                }
                break;
            };
            if !acc.accepts(kind) {
                break;
            }
            acc.apply(kind, value);
            real_words += 1;
            end = word_end;
            if matches!(self.ch(word_end), Some(' ' | '-')) {
                j = word_end + 1;
            } else {
                break;
            }
        }
        (real_words > 0).then(|| (acc.finish().saturating_mul(MICRO), end))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Unit,
    Teen,
    Tens,
    Hundred,
    /// A word that already means hundreds (Spanish `quinientos`, `ciento`).
    Hundreds,
    Scale,
}

fn word_kind(word: &str) -> Option<(Kind, u128)> {
    const WORDS: [(&str, Kind, u128); 33] = [
        ("one", Kind::Unit, 1),
        ("two", Kind::Unit, 2),
        ("three", Kind::Unit, 3),
        ("four", Kind::Unit, 4),
        ("five", Kind::Unit, 5),
        ("six", Kind::Unit, 6),
        ("seven", Kind::Unit, 7),
        ("eight", Kind::Unit, 8),
        ("nine", Kind::Unit, 9),
        ("ten", Kind::Teen, 10),
        ("eleven", Kind::Teen, 11),
        ("twelve", Kind::Teen, 12),
        ("thirteen", Kind::Teen, 13),
        ("fourteen", Kind::Teen, 14),
        ("fifteen", Kind::Teen, 15),
        ("sixteen", Kind::Teen, 16),
        ("seventeen", Kind::Teen, 17),
        ("eighteen", Kind::Teen, 18),
        ("nineteen", Kind::Teen, 19),
        ("twenty", Kind::Tens, 20),
        ("thirty", Kind::Tens, 30),
        ("forty", Kind::Tens, 40),
        ("fifty", Kind::Tens, 50),
        ("sixty", Kind::Tens, 60),
        ("seventy", Kind::Tens, 70),
        ("eighty", Kind::Tens, 80),
        ("ninety", Kind::Tens, 90),
        ("hundred", Kind::Hundred, 100),
        ("thousand", Kind::Scale, 1_000),
        ("million", Kind::Scale, 1_000_000),
        ("billion", Kind::Scale, 1_000_000_000),
        ("trillion", Kind::Scale, 1_000_000_000_000),
        ("dozen", Kind::Scale, 12),
    ];
    WORDS
        .iter()
        .find(|(text, _, _)| word.eq_ignore_ascii_case(text))
        .map(|(_, kind, value)| (*kind, *value))
}

#[derive(Default)]
struct Words {
    total: u128,
    current: u128,
    last: Option<Kind>,
}

impl Words {
    fn accepts(&self, next: Kind) -> bool {
        use Kind::{Hundred, Hundreds, Scale, Teen, Tens, Unit};
        matches!(
            (self.last, next),
            (None, Unit | Teen | Tens | Hundreds)
                | (Some(Unit | Teen), Hundred | Scale)
                | (Some(Tens), Unit | Scale)
                | (Some(Hundred), Unit | Teen | Tens | Scale)
                | (Some(Hundreds), Unit | Teen | Tens | Scale)
                | (Some(Scale), Unit | Teen | Tens | Hundreds)
        )
    }

    fn can_join_and(&self) -> bool {
        matches!(self.last, Some(Kind::Hundred | Kind::Scale))
    }

    fn apply(&mut self, kind: Kind, value: u128) {
        match kind {
            Kind::Unit | Kind::Teen | Kind::Tens | Kind::Hundreds => self.current += value,
            Kind::Hundred => self.current *= value,
            Kind::Scale => {
                let count = self.current.max(1);
                self.total += count * value;
                self.current = 0;
            }
        }
        self.last = Some(kind);
    }

    fn finish(&self) -> u128 {
        self.total + self.current
    }
}

fn kanji_digit(c: char) -> Option<u128> {
    Some(match c {
        '〇' | '零' => 0,
        '一' => 1,
        '二' => 2,
        '三' => 3,
        '四' => 4,
        '五' => 5,
        '六' => 6,
        '七' => 7,
        '八' => 8,
        '九' => 9,
        '两' | '兩' => 2,
        _ => return None,
    })
}

fn small_unit(c: char) -> Option<u128> {
    match c {
        '十' => Some(10),
        '百' => Some(100),
        '千' => Some(1_000),
        _ => None,
    }
}

/// Korean scale units: `십백천` and `만억조`.
fn korean_unit(c: char) -> Option<u128> {
    match c {
        '십' => Some(10),
        '백' => Some(100),
        '천' => Some(1_000),
        '만' => Some(10_000),
        '억' => Some(100_000_000),
        '조' => Some(1_000_000_000_000),
        _ => None,
    }
}

/// Spanish scale words after digits, longest first.
const SPANISH_SCALES: [(&str, u128); 4] = [
    ("millones", 1_000_000),
    ("millón", 1_000_000),
    ("millon", 1_000_000),
    ("mil", 1_000),
];

fn big_unit(c: char) -> Option<u128> {
    match c {
        '万' => Some(10_000),
        '億' | '亿' => Some(100_000_000),
        '兆' => Some(1_000_000_000_000),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(text: &str) -> Vec<u128> {
        extract(text).iter().map(|n| n.micro / MICRO).collect()
    }

    #[test]
    fn arabic_with_grouping_and_decimals() {
        assert_eq!(values("1,500,000 yen"), [1_500_000]);
        assert_eq!(extract("98.6")[0].micro, 98_600_000);
        assert_eq!(values("１２３"), [123]);
        assert_eq!(values("1, 2"), [1, 2]);
    }

    #[test]
    fn arabic_with_scale_suffixes() {
        assert_eq!(values("3万"), [30_000]);
        assert_eq!(values("5 million"), [5_000_000]);
        assert_eq!(extract("1.5億")[0].micro, 150_000_000 * MICRO);
    }

    #[test]
    fn japanese_numerals() {
        assert_eq!(values("五百万円"), [5_000_000]);
        assert_eq!(values("二〇二六年"), [2026]);
        assert_eq!(values("三千万"), [30_000_000]);
        assert_eq!(values("十"), [10]);
        assert_eq!(values("一億二千三百万"), [123_000_000]);
    }

    #[test]
    fn a_lone_man_is_not_a_number() {
        assert_eq!(values("万が一"), [1]);
        assert!(extract("万").is_empty());
    }

    #[test]
    fn english_number_words() {
        assert_eq!(values("fifty thousand dollars"), [50_000]);
        assert_eq!(values("a hundred dollars"), [100]);
        assert_eq!(values("twenty-five"), [25]);
        assert_eq!(values("five million"), [5_000_000]);
        assert_eq!(values("one hundred and five"), [105]);
        assert_eq!(values("five hundred thousand"), [500_000]);
    }

    #[test]
    fn two_separate_english_numbers_do_not_merge() {
        assert_eq!(values("two three"), [2, 3]);
    }

    #[test]
    fn number_words_inside_other_words_are_not_numbers() {
        assert!(extract("someone often mentioned stone").is_empty());
        assert!(extract("a bit").is_empty());
    }

    #[test]
    fn scale_words_after_digits_are_multiplied_in_zh_ko_es() {
        assert_eq!(counted("5百万", Tongue::Chinese), [5_000_000]);
        assert_eq!(counted("3千亿", Tongue::Chinese), [300_000_000_000]);
        assert_eq!(counted("1.5亿", Tongue::Chinese), [150_000_000]);
        assert_eq!(counted("5백만", Tongue::Korean), [5_000_000]);
        assert_eq!(counted("2천", Tongue::Korean), [2_000]);
        assert_eq!(counted("3 mil millones", Tongue::Spanish), [3_000_000_000]);
        assert_eq!(counted("5 Millones", Tongue::Spanish), [5_000_000]);
        assert_eq!(counted("1 millón", Tongue::Spanish), [1_000_000]);
    }

    #[test]
    fn a_word_that_only_begins_like_a_scale_word_is_not_one() {
        assert_eq!(counted("5 milímetros", Tongue::Spanish), [5]);
        assert_eq!(counted("5百分之", Tongue::Chinese), [5]);
    }

    #[test]
    fn offsets_point_into_the_text() {
        let text = "I paid $100.";
        let found = extract(text);
        assert_eq!(&text[found[0].start..found[0].end], "100");
    }

    #[test]
    fn a_minus_sign_is_remembered_but_a_range_hyphen_is_not() {
        assert!(extract("-40 degrees")[0].negative);
        assert!(!extract("3-5")[1].negative);
        assert!(!extract("10-day")[0].negative);
    }

    #[test]
    fn compared_values_drop_ones_and_sort() {
        assert_eq!(
            compared_values("もう一度 3 と 10 と二", Tongue::Other),
            [2 * MICRO, 3 * MICRO, 10 * MICRO]
        );
        assert!(compared_values("a week", Tongue::English).is_empty());
        assert!(compared_values("", Tongue::Other).is_empty());
    }

    #[test]
    fn overflowing_digit_runs_are_skipped_not_wrapped() {
        let long = "9".repeat(60);
        assert!(extract(&long).is_empty());
    }

    fn counted(text: &str, tongue: Tongue) -> Vec<u128> {
        compared_values(text, tongue)
            .iter()
            .map(|micro| micro / MICRO)
            .collect()
    }

    #[test]
    fn a_prefix_of_a_longer_word_is_not_a_number() {
        assert!(counted("no tenía sentido", Tongue::Spanish).is_empty());
        assert!(counted("no tenía sentido", Tongue::Other).is_empty());
        assert_eq!(counted("ten dogs", Tongue::Other), [10]);
    }

    #[test]
    fn spanish_number_words_are_read_in_spanish_only() {
        assert_eq!(counted("dos horas", Tongue::Spanish), [2]);
        assert_eq!(counted("treinta y cinco personas", Tongue::Spanish), [35]);
        assert_eq!(counted("dos mil quinientos", Tongue::Spanish), [2500]);
        assert_eq!(counted("veintitrés años", Tongue::Spanish), [23]);
        assert_eq!(counted("once", Tongue::Spanish), [11]);
        assert!(counted("once upon a time", Tongue::Other).is_empty());
        assert!(counted("DOS attack", Tongue::Spanish).is_empty());
    }

    #[test]
    fn por_ciento_is_not_a_hundred() {
        assert_eq!(counted("ochenta por ciento", Tongue::Spanish), [80]);
        assert_eq!(counted("ciento cinco", Tongue::Spanish), [105]);
    }

    #[test]
    fn english_ordinals_are_read_in_english_only() {
        assert_eq!(
            counted("the thirteenth and twenty-fourth", Tongue::English),
            [13, 24]
        );
        assert_eq!(counted("thirty-first", Tongue::English), [31]);
        assert!(counted("first place, second best", Tongue::English).is_empty());
        assert!(counted("the third", Tongue::Other).is_empty());
    }

    #[test]
    fn chinese_two_and_percent_are_read() {
        assert_eq!(counted("两小时", Tongue::Other), [2]);
        assert_eq!(counted("两千", Tongue::Other), [2000]);
        assert_eq!(counted("百分之八十", Tongue::Other), [80]);
        assert_eq!(counted("百分之百", Tongue::Other), [100]);
        assert_eq!(counted("3亿", Tongue::Other), [300_000_000]);
    }

    #[test]
    fn a_squared_unit_is_not_a_quantity() {
        assert_eq!(counted("19,500 km2", Tongue::Other), [19_500]);
        assert_eq!(counted("5 m3", Tongue::Other), [5]);
        assert_eq!(counted("H2O", Tongue::Other), [2]);
        assert_eq!(counted("km 2", Tongue::Other), [2]);
    }

    #[test]
    fn a_space_groups_three_digits_after_a_short_head() {
        assert_eq!(counted("19 500 km", Tongue::Spanish), [19_500]);
        assert_eq!(counted("in 2019 500 more", Tongue::Other), [500, 2019]);
    }

    #[test]
    fn a_month_name_counts_as_its_month_in_its_own_language() {
        assert_eq!(counted("7月", Tongue::Other), [7]);
        assert_eq!(counted("in July", Tongue::English), [7]);
        assert_eq!(counted("en julio", Tongue::Spanish), [7]);
        assert_eq!(counted("born in May", Tongue::English), [5]);
        assert!(counted("May I help", Tongue::English).is_empty());
        assert!(counted("July", Tongue::Other).is_empty());
        assert!(counted("may result", Tongue::English).is_empty());
    }
}

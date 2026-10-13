//! Reading numbers out of free text.
//!
//! One reader serves both the verification step and the unit annotation, so
//! "what number is this" has a single answer. It reads Arabic digits (ASCII
//! and full-width, with `,` digit grouping and a decimal point, optionally
//! followed by 万/億/兆 or an English scale word), Japanese numerals
//! (五百万, 二〇二六, 三千), and English number words (fifty thousand,
//! a hundred, twenty-five).
//!
//! Values are kept as whole millionths so that equality is exact.

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

/// Every number in `text`, in order.
pub fn extract(text: &str) -> Vec<Number> {
    let scan = Scan::new(text);
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
/// "もう一度" and "a week" carry a 1 on one side only.
pub fn compared_values(text: &str) -> Vec<u128> {
    let mut values: Vec<u128> = extract(text)
        .into_iter()
        .map(|number| number.micro)
        .filter(|micro| *micro >= 2 * MICRO)
        .collect();
    values.sort_unstable();
    values
}

struct Scan<'a> {
    text: &'a str,
    chars: Vec<(usize, char)>,
}

impl<'a> Scan<'a> {
    fn new(text: &'a str) -> Scan<'a> {
        Scan {
            text,
            chars: text.char_indices().collect(),
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
            return self.arabic(i);
        }
        if kanji_digit(c).is_some() || small_unit(c).is_some() {
            return self.kanji(i);
        }
        let word_start = c.is_ascii_alphabetic()
            && (i == 0
                || !self
                    .ch(i - 1)
                    .is_some_and(|prev| prev.is_ascii_alphanumeric()));
        if word_start { self.english(i) } else { None }
    }

    fn negative_before(&self, i: usize) -> bool {
        if i == 0 || !matches!(self.ch(i - 1), Some('-' | '−')) {
            return false;
        }
        i == 1 || !self.ch(i - 2).is_some_and(char::is_alphanumeric)
    }

    /// Whether character `j` is a `,` that groups exactly three digits.
    fn is_group_comma(&self, j: usize) -> bool {
        matches!(self.ch(j), Some(',' | '，'))
            && (1..=3).all(|k| self.digit(j + k).is_some())
            && self.digit(j + 4).is_none()
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
        if let Some(scale) = self.ch(j).and_then(big_unit) {
            return Some((micro.checked_mul(scale)?, j + 1));
        }
        if let Some((scale, end)) = self.english_scale(j) {
            return Some((micro.checked_mul(scale)?, end));
        }
        Some((micro, j))
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
        Some((&self.text[self.byte(j)..self.byte(end)], end))
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
            let Some((kind, value)) = word_kind(word) else {
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
enum Kind {
    Unit,
    Teen,
    Tens,
    Hundred,
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
        use Kind::{Hundred, Scale, Teen, Tens, Unit};
        matches!(
            (self.last, next),
            (None, Unit | Teen | Tens)
                | (Some(Unit | Teen), Hundred | Scale)
                | (Some(Tens), Unit | Scale)
                | (Some(Hundred), Unit | Teen | Tens | Scale)
                | (Some(Scale), Unit | Teen | Tens)
        )
    }

    fn can_join_and(&self) -> bool {
        matches!(self.last, Some(Kind::Hundred | Kind::Scale))
    }

    fn apply(&mut self, kind: Kind, value: u128) {
        match kind {
            Kind::Unit | Kind::Teen | Kind::Tens => self.current += value,
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

fn big_unit(c: char) -> Option<u128> {
    match c {
        '万' => Some(10_000),
        '億' => Some(100_000_000),
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
            compared_values("もう一度 3 と 10 と二"),
            [2 * MICRO, 3 * MICRO, 10 * MICRO]
        );
        assert!(compared_values("a week").is_empty());
        assert!(compared_values("").is_empty());
    }

    #[test]
    fn overflowing_digit_runs_are_skipped_not_wrapped() {
        let long = "9".repeat(60);
        assert!(extract(&long).is_empty());
    }
}

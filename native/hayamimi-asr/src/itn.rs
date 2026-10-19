//! Conservative inverse text normalisation of kanji numerals into digits for
//! Japanese, Chinese and Cantonese text.
//!
//! The postprocessing order is fixed: ITN first, then punctuation, then the
//! user's replacement dictionary. ITN runs first so that punctuation sees the
//! normalised digits; the dictionary runs last so that it can override what
//! the earlier stages produced.
//!
//! What is converted, and nothing else:
//!
//! 1. Magnitude-form numbers: digits combined with 十, 百 and 千 are computed
//!    to an integer (三千二百 becomes 3200). With 万, 萬 or 億 present those
//!    units stay literal and only the part before each is converted
//!    (四万人弱 becomes 4万人弱).
//! 2. Decimals with 点: both sides are read digit by digit unless the integer
//!    side holds a magnitude unit (二点四 becomes 2.4).
//! 3. Runs of two or more kanji digits without a unit are read digit by digit,
//!    the way years and codes are spoken (一九四五年 becomes 1945年).
//!
//! A single numeral character is never converted, which keeps idioms and
//! proper nouns such as 一番, 一緒, 四国 and 九州 intact; a short exclusion
//! list backs that up for phrases that could form a longer run.
//!
//! This module reads and writes strings only. It has no model and no device.

/// Languages the rules apply to; any other language passes through.
const APPLICABLE_LANGS: [&str; 3] = ["ja", "zh", "yue"];

/// Phrases whose numerals are never converted.
const DEFAULT_EXCLUSIONS: [&str; 16] = [
    "一番",
    "一緒",
    "一部",
    "四国",
    "九州",
    "十分",
    "一石二鳥",
    "二人三脚",
    "一体",
    "一旦",
    "一気",
    "七夕",
    "三味線",
    "二重",
    "十字架",
    "一杯",
];

/// The longest run of numeral characters one token takes.
const MAX_RUN: usize = 12;
/// The most digits after 点 one token takes.
const MAX_DECIMALS: usize = 10;

/// User-level overrides. `exclude` adds to the built-in exclusions; `force`
/// pairs are plain literal replacements applied before the rules, so their
/// output is immune to them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Overrides {
    pub exclude: Vec<String>,
    pub force: Vec<(String, String)>,
}

/// The value of a kanji digit. Simplified and traditional variants share a
/// slot, and 兩 and 两 (the "two" before a classifier) read as 二.
fn digit(c: char) -> Option<u64> {
    match c {
        '〇' | '零' => Some(0),
        '一' => Some(1),
        '二' | '兩' | '两' => Some(2),
        '三' => Some(3),
        '四' => Some(4),
        '五' => Some(5),
        '六' => Some(6),
        '七' => Some(7),
        '八' => Some(8),
        '九' => Some(9),
        _ => None,
    }
}

/// The multiplier of a unit usable inside one segment below 10000.
fn small_unit(c: char) -> Option<u64> {
    match c {
        '十' => Some(10),
        '百' => Some(100),
        '千' => Some(1000),
        _ => None,
    }
}

fn is_wan(c: char) -> bool {
    matches!(c, '万' | '萬')
}

fn is_numeral(c: char) -> bool {
    digit(c).is_some() || small_unit(c).is_some() || is_wan(c) || c == '億'
}

fn has_small_unit(segment: &[char]) -> bool {
    segment.iter().any(|c| small_unit(*c).is_some())
}

/// Digit-by-digit reading; `None` when a character is not a plain digit.
fn digit_string(segment: &[char]) -> Option<String> {
    let mut out = String::with_capacity(segment.len());
    for c in segment {
        out.push(char::from_digit(u32::try_from(digit(*c)?).ok()?, 10)?);
    }
    Some(out)
}

/// Arithmetic reading of a segment below 10000 (digits and 十, 百, 千 only).
/// A digit directly before a unit multiplies it, a unit without one counts as
/// one, and consecutive digits keep the last one.
fn parse_small(segment: &[char]) -> Option<u64> {
    let (mut total, mut number) = (0_u64, 0_u64);
    for c in segment {
        if let Some(value) = digit(*c) {
            number = value;
            continue;
        }
        let unit = small_unit(*c)?;
        let factor = if number == 0 { 1 } else { number };
        total += factor * unit;
        number = 0;
    }
    Some(total + number)
}

/// The value before a big unit: the unit alone reads as one.
fn before_unit(segment: &[char]) -> Option<u64> {
    if segment.is_empty() {
        Some(1)
    } else {
        parse_small(segment)
    }
}

/// A token with 億 or 万/萬: each big unit stays literal, only the segment in
/// front of it is converted.
fn convert_big(token: &[char]) -> Option<String> {
    let mut out = String::new();
    let mut rest = token;
    if let Some(at) = rest.iter().position(|c| *c == '億') {
        out.push_str(&before_unit(&rest[..at])?.to_string());
        out.push('億');
        rest = &rest[at + 1..];
    }
    if let Some(at) = rest.iter().position(|c| is_wan(*c)) {
        out.push_str(&before_unit(&rest[..at])?.to_string());
        out.push(rest[at]);
        rest = &rest[at + 1..];
    }
    if !rest.is_empty() {
        let value = if has_small_unit(rest) {
            parse_small(rest)?
        } else {
            digit_string(rest)?.parse::<u64>().ok()?
        };
        out.push_str(&value.to_string());
    }
    Some(out)
}

/// A decimal token: both sides digit by digit, or the integer side as a
/// magnitude number when it holds a unit.
fn convert_decimal(integer: &[char], decimals: &[char]) -> Option<String> {
    if integer.is_empty() || decimals.is_empty() {
        return None;
    }
    let whole = if has_small_unit(integer) {
        parse_small(integer)?.to_string()
    } else {
        digit_string(integer)?
    };
    Some(format!("{whole}.{}", digit_string(decimals)?))
}

/// The converted text of one token, or `None` when the token stays as it is.
fn convert_token(token: &[char]) -> Option<String> {
    if let Some(at) = token.iter().position(|c| *c == '点') {
        return convert_decimal(&token[..at], &token[at + 1..]);
    }
    if token.len() == 1 {
        return None;
    }
    if token.iter().any(|c| *c == '億' || is_wan(*c)) {
        return convert_big(token);
    }
    if has_small_unit(token) {
        return parse_small(token).map(|value| value.to_string());
    }
    digit_string(token)
}

/// The next token at or after `from`: up to 12 numeral characters, then an
/// optional 点 with up to 10 digits. Returns the half-open range.
fn next_token(chars: &[char], from: usize) -> Option<(usize, usize)> {
    let start = from + chars[from..].iter().position(|c| is_numeral(*c))?;
    let run = chars[start..]
        .iter()
        .take(MAX_RUN)
        .take_while(|c| is_numeral(**c))
        .count();
    let mut end = start + run;
    if chars.get(end) == Some(&'点') {
        let decimals = chars[end + 1..]
            .iter()
            .take(MAX_DECIMALS)
            .take_while(|c| digit(**c).is_some())
            .count();
        if decimals > 0 {
            end += 1 + decimals;
        }
    }
    Some((start, end))
}

/// Every occurrence (overlapping ones included) of every excluded phrase.
fn protected_spans(chars: &[char], extra: &[String]) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let words = DEFAULT_EXCLUSIONS
        .iter()
        .copied()
        .chain(extra.iter().map(String::as_str));
    for word in words {
        let word: Vec<char> = word.chars().collect();
        if word.is_empty() || word.len() > chars.len() {
            continue;
        }
        for (at, window) in chars.windows(word.len()).enumerate() {
            if window == word.as_slice() {
                spans.push((at, at + word.len()));
            }
        }
    }
    spans
}

fn applies_to(lang: &str) -> bool {
    APPLICABLE_LANGS.contains(&lang)
}

/// Applies the conservative kanji-numeral rules to `text` for `lang`; a
/// language the rules do not cover, and empty text, pass through unchanged.
///
/// `overrides.force` is applied first as plain literal replacements, in
/// order. A token that overlaps an excluded phrase is left untouched.
pub fn convert(text: &str, lang: &str, overrides: &Overrides) -> String {
    if !applies_to(lang) || text.is_empty() {
        return text.to_owned();
    }
    let mut forced = text.to_owned();
    for (wrong, right) in &overrides.force {
        if !wrong.is_empty() {
            forced = forced.replace(wrong.as_str(), right);
        }
    }
    let chars: Vec<char> = forced.chars().collect();
    let protected = protected_spans(&chars, &overrides.exclude);
    let mut out = String::with_capacity(forced.len());
    let mut copied = 0;
    while let Some((start, end)) = next_token(&chars, copied) {
        out.extend(&chars[copied..start]);
        let overlaps = protected
            .iter()
            .any(|(from, to)| start < *to && *from < end);
        match convert_token(&chars[start..end]) {
            Some(converted) if !overlaps => out.push_str(&converted),
            _ => out.extend(&chars[start..end]),
        }
        copied = end;
    }
    out.extend(&chars[copied..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ja(text: &str) -> String {
        convert(text, "ja", &Overrides::default())
    }

    #[test]
    fn a_date_with_a_magnitude_year_converts_and_a_lone_numeral_stays() {
        assert_eq!(ja("千九百四十年八月十五日"), "1940年八月15日");
    }

    #[test]
    fn a_count_converts() {
        assert_eq!(ja("三十四件"), "34件");
    }

    #[test]
    fn magnitude_forms_are_computed() {
        assert_eq!(ja("三千二百"), "3200");
        assert_eq!(ja("一百五十"), "150");
        assert_eq!(ja("十五"), "15");
    }

    #[test]
    fn big_units_stay_literal() {
        assert_eq!(ja("四万人弱"), "4万人弱");
        assert_eq!(ja("三億五千万"), "3億5000万");
        assert_eq!(ja("萬"), "萬");
        assert_eq!(ja("二萬三千"), "2萬3000");
    }

    #[test]
    fn digit_runs_are_read_one_by_one() {
        assert_eq!(ja("一九四五年"), "1945年");
        assert_eq!(ja("二〇二〇"), "2020");
        assert_eq!(ja("〇一"), "01");
        assert_eq!(ja("兩两"), "22");
    }

    #[test]
    fn decimals_read_both_sides() {
        assert_eq!(ja("二点四"), "2.4");
        assert_eq!(ja("八零二点一"), "802.1");
        assert_eq!(ja("十二点五"), "12.5");
    }

    #[test]
    fn a_decimal_point_without_digits_after_it_converts_only_the_run() {
        assert_eq!(ja("二点"), "二点");
        assert_eq!(ja("点五"), "点五");
    }

    #[test]
    fn a_single_numeral_is_never_converted() {
        assert_eq!(ja("一"), "一");
        assert_eq!(ja("十"), "十");
        assert_eq!(ja("八月"), "八月");
    }

    #[test]
    fn idioms_and_proper_nouns_stay() {
        for text in [
            "一番",
            "一緒に行く",
            "九州",
            "四国",
            "十分です",
            "二人三脚",
            "一石二鳥",
        ] {
            assert_eq!(ja(text), text);
        }
    }

    #[test]
    fn a_token_that_overlaps_an_excluded_phrase_is_left_alone() {
        // 一番 sits inside the run 二一番, so the whole run stays.
        assert_eq!(ja("二一番"), "二一番");
    }

    #[test]
    fn a_user_exclusion_adds_to_the_built_in_list() {
        let overrides = Overrides {
            exclude: vec![String::from("三十四")],
            force: Vec::new(),
        };
        assert_eq!(convert("三十四件", "ja", &overrides), "三十四件");
        assert_eq!(convert("一九四五年", "ja", &overrides), "1945年");
    }

    #[test]
    fn forced_replacements_win_over_the_rules() {
        let overrides = Overrides {
            exclude: Vec::new(),
            force: vec![(String::from("三十四"), String::from("三四"))],
        };
        assert_eq!(convert("三十四件", "ja", &overrides), "34件");
        let kept = Overrides {
            exclude: Vec::new(),
            force: vec![(String::from("三十四件"), String::from("34 pieces"))],
        };
        assert_eq!(convert("三十四件", "ja", &kept), "34 pieces");
    }

    #[test]
    fn an_empty_force_key_is_skipped() {
        let overrides = Overrides {
            exclude: Vec::new(),
            force: vec![(String::new(), String::from("x"))],
        };
        assert_eq!(convert("三十四件", "ja", &overrides), "34件");
    }

    #[test]
    fn other_languages_and_empty_text_pass_through() {
        assert_eq!(convert("三十四件", "en", &Overrides::default()), "三十四件");
        assert_eq!(convert("", "ja", &Overrides::default()), "");
    }

    #[test]
    fn chinese_and_cantonese_are_covered() {
        assert_eq!(convert("三十四", "zh", &Overrides::default()), "34");
        assert_eq!(convert("三十四", "yue", &Overrides::default()), "34");
    }

    #[test]
    fn a_run_longer_than_twelve_is_split_at_twelve() {
        let text = "一".repeat(13);
        let expected = format!("{}一", "1".repeat(12));
        assert_eq!(ja(&text), expected);
    }

    #[test]
    fn two_big_units_of_the_same_kind_leave_the_token_alone() {
        assert_eq!(ja("二万三万"), "二万三万");
    }

    #[test]
    fn text_without_numerals_is_unchanged() {
        assert_eq!(ja("こんにちは。"), "こんにちは。");
    }

    fn conv(text: &str, lang: &str) -> String {
        convert(text, lang, &Overrides::default())
    }

    #[test]
    fn only_applies_to_ja_zh_yue() {
        for lang in ["ja", "zh", "yue"] {
            assert_eq!(conv("三千二百", lang), "3200", "{lang}");
        }
        for lang in ["en", "ko", "de", ""] {
            assert_eq!(conv("三千二百", lang), "三千二百", "{lang:?}");
        }
    }

    #[test]
    fn empty_text_is_a_noop() {
        assert_eq!(conv("", "ja"), "");
    }

    #[test]
    fn magnitude_form_full_arithmetic_without_big_units() {
        assert_eq!(conv("三千二百", "ja"), "3200");
        assert_eq!(conv("一百五十", "zh"), "150");
        assert_eq!(conv("四十九", "ja"), "49");
    }

    #[test]
    fn magnitude_form_keeps_man_and_oku_as_literal_units() {
        assert_eq!(conv("四万人弱", "ja"), "4万人弱");
        assert_eq!(conv("一億円", "ja"), "1億円");
        assert_eq!(conv("三千万円", "ja"), "3000万円");
    }

    #[test]
    fn traditional_wan_and_liang_forms() {
        assert_eq!(conv("四萬人", "yue"), "4萬人");
        assert_eq!(conv("兩百", "yue"), "200");
        assert_eq!(conv("两千", "zh"), "2000");
    }

    #[test]
    fn decimal_digit_by_digit_reading() {
        assert_eq!(conv("二点四", "ja"), "2.4");
        assert_eq!(conv("八零二点一", "zh"), "802.1");
    }

    #[test]
    fn decimal_with_magnitude_integer_part() {
        assert_eq!(conv("三十点五", "ja"), "30.5");
    }

    #[test]
    fn bare_digit_run_reads_digit_by_digit() {
        assert_eq!(conv("一九四五年", "ja"), "1945年");
        assert_eq!(conv("二〇二〇", "zh"), "2020");
        assert_eq!(conv("零一二三", "ja"), "0123");
    }

    #[test]
    fn single_bare_numeral_untouched() {
        assert_eq!(conv("一つください", "ja"), "一つください");
        assert_eq!(conv("十本", "ja"), "十本");
        assert_eq!(conv("九月", "ja"), "九月");
    }

    #[test]
    fn idioms_and_proper_nouns_stay_untouched() {
        for sentence in [
            "一番好きです",
            "一緒に行きます",
            "一部だけです",
            "四国に住んでいます",
            "九州出身です",
            "十分に休みました",
            "一石二鳥の作戦です",
            "二人三脚で頑張ります",
        ] {
            assert_eq!(conv(sentence, "ja"), sentence);
        }
    }

    fn with_exclude(words: &[&str]) -> Overrides {
        Overrides {
            exclude: words.iter().map(|word| (*word).to_owned()).collect(),
            force: Vec::new(),
        }
    }

    fn with_force(wrong: &str, right: &str) -> Overrides {
        Overrides {
            exclude: Vec::new(),
            force: vec![(wrong.to_owned(), right.to_owned())],
        }
    }

    #[test]
    fn default_exclusion_list_blocks_a_longer_run_directly() {
        assert_eq!(conv("十分だと思う", "ja"), "十分だと思う");
        assert_eq!(conv("一石二鳥だ", "ja"), "一石二鳥だ");
        // Runs of two or more numerals that overlap a built-in phrase: only the
        // exclusion list keeps them, since the rules alone would read them.
        assert_eq!(conv("三一旦だ", "ja"), "三一旦だ");
        assert_eq!(conv("二九州", "ja"), "二九州");
        assert_eq!(conv("三十分", "ja"), "三十分");
    }

    #[test]
    fn user_exclude_adds_to_builtin_list() {
        assert_eq!(conv("二十歳", "ja"), "20歳");
        assert_eq!(
            convert("二十歳", "ja", &with_exclude(&["二十歳"])),
            "二十歳"
        );
        assert_eq!(
            convert("一番だ", "ja", &with_exclude(&["二十歳"])),
            "一番だ"
        );
        // The built-in list still applies to a run the user list does not name.
        assert_eq!(
            convert("三一旦", "ja", &with_exclude(&["二十歳"])),
            "三一旦"
        );
    }

    #[test]
    fn user_force_wins_over_rule_output() {
        let forced = with_force("三千二百", "3,200");
        assert_eq!(convert("三千二百人", "ja", &forced), "3,200人");
    }

    #[test]
    fn user_force_can_apply_to_text_the_rules_would_leave_alone() {
        let forced = with_force("一番", "No.1");
        assert_eq!(convert("一番だ", "ja", &forced), "No.1だ");
    }
}

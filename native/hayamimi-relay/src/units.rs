//! Units, their spellings and the factors between them (`local_units.toml`).
//!
//! Conversion is a table lookup and a multiplication. No language model is
//! involved, and a spelling that could mean two units is never guessed.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::lang::Lang;

pub const DEFAULT_TABLE: &str = include_str!("../data/local_units.toml");

/// What a unit measures.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Currency,
    Length,
    Mass,
    Temperature,
    Volume,
    Area,
}

#[derive(Clone, Debug, Deserialize)]
struct UnitRaw {
    id: String,
    kind: Kind,
    aliases: Vec<String>,
    #[serde(default)]
    prefix: Vec<String>,
    #[serde(default)]
    place: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct AmbiguousRaw {
    kind: Kind,
    aliases: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct ReaderRaw {
    currency: String,
}

#[derive(Clone, Debug, Deserialize)]
struct ConversionRaw {
    reader: Lang,
    from: String,
    to: String,
    #[serde(default)]
    pre: f64,
    #[serde(default = "one")]
    factor: f64,
    #[serde(default = "one")]
    divisor: f64,
    #[serde(default)]
    post: f64,
    via: Option<String>,
}

fn one() -> f64 {
    1.0
}

#[derive(Clone, Debug, Deserialize)]
struct TableRaw {
    reader: BTreeMap<String, ReaderRaw>,
    unit: Vec<UnitRaw>,
    #[serde(default)]
    ambiguous: Vec<AmbiguousRaw>,
    #[serde(default)]
    conversion: Vec<ConversionRaw>,
}

/// One row of the conversion table: `to = (from + pre) * factor / divisor + post`.
#[derive(Clone, Debug, PartialEq)]
pub struct Conversion {
    pub reader: Lang,
    pub from: String,
    pub to: String,
    pub pre: f64,
    pub factor: f64,
    pub divisor: f64,
    pub post: f64,
    /// Convert through this unit first, using the rows already in the table.
    pub via: Option<String>,
}

impl Conversion {
    /// Applies this row's arithmetic to a value in the `from` unit.
    pub fn apply(&self, value: f64) -> f64 {
        (value + self.pre) * self.factor / self.divisor + self.post
    }

    /// The multiplicative factor of a pure scaling row.
    pub fn ratio(&self) -> f64 {
        self.factor / self.divisor
    }
}

/// A unit and every way it is written.
#[derive(Clone, Debug)]
pub struct Unit {
    pub id: String,
    pub kind: Kind,
    /// Spellings after the number, longest first: `aliases` then `place`.
    suffixes: Vec<String>,
    /// Spellings before the number (`$`), longest first.
    prefixes: Vec<String>,
}

/// What a spelling resolves to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolved {
    /// The unit at this index of the table.
    Unit(usize),
    /// A spelling that names more than one unit; carries the likely kind.
    Ambiguous(Kind),
}

/// A spelling found in a text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub resolved: Resolved,
    /// Byte offset where the spelling starts.
    pub start: usize,
    /// Byte offset just past the spelling.
    pub end: usize,
}

#[derive(Clone, Debug)]
struct Alias {
    text: String,
    resolved: Resolved,
}

/// A converted value: the target unit and the number in it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Converted<'a> {
    pub unit: &'a str,
    pub value: f64,
}

/// `local_units.toml`, validated and indexed.
#[derive(Clone, Debug)]
pub struct LocalUnits {
    units: Vec<Unit>,
    conversions: Vec<Conversion>,
    currency: BTreeMap<Lang, String>,
    after: Vec<Alias>,
    before: Vec<Alias>,
}

impl LocalUnits {
    /// Parses and validates the table.
    pub fn parse(bytes: &[u8]) -> Result<LocalUnits, String> {
        let text = std::str::from_utf8(bytes).map_err(|error| error.to_string())?;
        let raw: TableRaw = toml::from_str(text).map_err(|error| error.to_string())?;
        let mut table = LocalUnits {
            units: Vec::with_capacity(raw.unit.len()),
            conversions: Vec::with_capacity(raw.conversion.len()),
            currency: BTreeMap::new(),
            after: Vec::new(),
            before: Vec::new(),
        };
        for unit in raw.unit {
            table.add_unit(unit);
        }
        for ambiguous in raw.ambiguous {
            for alias in ambiguous.aliases {
                table.after.push(Alias {
                    text: alias,
                    resolved: Resolved::Ambiguous(ambiguous.kind),
                });
            }
        }
        for (code, reader) in raw.reader {
            let lang = match code.as_str() {
                "ja" => Lang::Ja,
                "en" => Lang::En,
                other => return Err(format!("unknown reader language {other}")),
            };
            table.check_currency(&reader.currency)?;
            table.currency.insert(lang, reader.currency);
        }
        for row in raw.conversion {
            table.add_conversion(row)?;
        }
        for lang in [Lang::Ja, Lang::En] {
            if !table.currency.contains_key(&lang) {
                return Err(format!("reader.{} is missing", lang.code()));
            }
        }
        table
            .after
            .sort_by_key(|alias| std::cmp::Reverse(alias.text.chars().count()));
        table
            .before
            .sort_by_key(|alias| std::cmp::Reverse(alias.text.chars().count()));
        for unit in &mut table.units {
            unit.suffixes
                .sort_by_key(|text| std::cmp::Reverse(text.chars().count()));
            unit.prefixes
                .sort_by_key(|text| std::cmp::Reverse(text.chars().count()));
        }
        Ok(table)
    }

    /// The compiled-in table.
    pub fn embedded() -> Result<LocalUnits, String> {
        LocalUnits::parse(DEFAULT_TABLE.as_bytes())
    }

    fn add_unit(&mut self, raw: UnitRaw) {
        let index = self.units.len();
        let resolved = Resolved::Unit(index);
        for alias in &raw.aliases {
            self.after.push(Alias {
                text: alias.clone(),
                resolved,
            });
        }
        for alias in &raw.prefix {
            self.before.push(Alias {
                text: alias.clone(),
                resolved,
            });
        }
        let mut suffixes = raw.aliases;
        suffixes.extend(raw.place);
        self.units.push(Unit {
            id: raw.id,
            kind: raw.kind,
            suffixes,
            prefixes: raw.prefix,
        });
    }

    fn check_currency(&self, id: &str) -> Result<(), String> {
        match self.unit(id) {
            Some(unit) if unit.kind == Kind::Currency => Ok(()),
            _ => Err(format!("{id} is not a currency unit")),
        }
    }

    fn add_conversion(&mut self, raw: ConversionRaw) -> Result<(), String> {
        for id in [Some(&raw.from), Some(&raw.to), raw.via.as_ref()]
            .into_iter()
            .flatten()
        {
            if self.unit(id).is_none() {
                return Err(format!("conversion names unknown unit {id}"));
            }
        }
        if raw.divisor == 0.0 || !raw.factor.is_finite() || !raw.divisor.is_finite() {
            return Err(format!(
                "conversion {} to {} has a bad factor",
                raw.from, raw.to
            ));
        }
        self.conversions.push(Conversion {
            reader: raw.reader,
            from: raw.from,
            to: raw.to,
            pre: raw.pre,
            factor: raw.factor,
            divisor: raw.divisor,
            post: raw.post,
            via: raw.via,
        });
        Ok(())
    }

    /// The unit with this id.
    pub fn unit(&self, id: &str) -> Option<&Unit> {
        self.units.iter().find(|unit| unit.id == id)
    }

    /// The unit at a table index, as carried by [`Resolved::Unit`].
    pub fn unit_at(&self, index: usize) -> Option<&Unit> {
        self.units.get(index)
    }

    /// The currency a reader of this language has at home.
    pub fn local_currency(&self, reader: Lang) -> Option<&str> {
        self.currency.get(&reader).map(String::as_str)
    }

    /// The row converting `from` into `to` for this reader.
    pub fn row(&self, reader: Lang, from: &str, to: &str) -> Option<&Conversion> {
        self.conversions
            .iter()
            .find(|row| row.reader == reader && row.from == from && row.to == to)
    }

    /// Converts `value` from one unit to another with the table's row.
    pub fn convert(&self, reader: Lang, from: &str, to: &str, value: f64) -> Option<f64> {
        self.row(reader, from, to).map(|row| row.apply(value))
    }

    /// Every conversion a reader gets for a value in `from`, in table order.
    /// Empty when `from` is already at home for that reader.
    pub fn targets(&self, reader: Lang, from: &str, value: f64) -> Vec<Converted<'_>> {
        let mut out = Vec::new();
        for row in self
            .conversions
            .iter()
            .filter(|row| row.reader == reader && row.from == from)
        {
            let converted = match &row.via {
                None => Some(row.apply(value)),
                Some(mid) => self
                    .convert(reader, from, mid, value)
                    .and_then(|middle| self.convert(reader, mid, &row.to, middle)),
            };
            if let Some(value) = converted {
                out.push(Converted {
                    unit: row.to.as_str(),
                    value,
                });
            }
        }
        out
    }

    /// The spelling that follows a number at byte `pos`, after optional spaces.
    pub fn read_after(&self, text: &str, pos: usize) -> Option<Hit> {
        let start = skip_spaces(text, pos);
        self.after.iter().find_map(|alias| {
            let end = match_after(text, start, &alias.text)?;
            Some(Hit {
                resolved: alias.resolved,
                start,
                end,
            })
        })
    }

    /// The spelling written right before byte `pos` (`$` before `100`).
    pub fn read_before(&self, text: &str, pos: usize) -> Option<Hit> {
        self.before.iter().find_map(|alias| {
            let start = match_before(text, pos, &alias.text)?;
            Some(Hit {
                resolved: alias.resolved,
                start,
                end: pos,
            })
        })
    }

    /// Where a spelling of unit `index` ends, when it follows byte `pos`.
    pub fn place_after(&self, index: usize, text: &str, pos: usize) -> Option<usize> {
        let start = skip_spaces(text, pos);
        self.units
            .get(index)?
            .suffixes
            .iter()
            .find_map(|alias| match_after(text, start, alias))
    }

    /// Where a spelling of unit `index` starts, when it ends at byte `pos`.
    pub fn place_before(&self, index: usize, text: &str, pos: usize) -> Option<usize> {
        self.units
            .get(index)?
            .prefixes
            .iter()
            .find_map(|alias| match_before(text, pos, alias))
    }
}

/// `pos` moved past any spaces; `pos` itself when it is not inside the text.
fn skip_spaces(text: &str, pos: usize) -> usize {
    let Some(rest) = text.get(pos..) else {
        return pos;
    };
    pos + rest.len() - rest.trim_start_matches([' ', '\u{3000}', '\u{a0}']).len()
}

fn word_char(c: char) -> bool {
    c.is_ascii_alphanumeric()
}

/// The end offset if `alias` is at `pos` (ASCII case ignored) and, for an
/// alias ending in a letter or digit, is not glued to a following one.
fn match_after(text: &str, pos: usize, alias: &str) -> Option<usize> {
    let end = pos + alias.len();
    let found = text.as_bytes().get(pos..end)?;
    if !found.eq_ignore_ascii_case(alias.as_bytes()) {
        return None;
    }
    let ends_in_word = alias.chars().next_back().is_some_and(word_char);
    let glued = text.get(end..)?.chars().next().is_some_and(word_char);
    (!(ends_in_word && glued)).then_some(end)
}

/// The start offset if `alias` ends exactly at `pos` and does not begin in
/// the middle of a word.
fn match_before(text: &str, pos: usize, alias: &str) -> Option<usize> {
    let start = pos.checked_sub(alias.len())?;
    let found = text.as_bytes().get(start..pos)?;
    if !found.eq_ignore_ascii_case(alias.as_bytes()) {
        return None;
    }
    let starts_in_word = alias.chars().next().is_some_and(word_char);
    let glued = text
        .get(..start)?
        .chars()
        .next_back()
        .is_some_and(word_char);
    (!(starts_in_word && glued)).then_some(start)
}

/// A value to three significant digits, written without an exponent, with a
/// `,` every three digits once it reaches 1000 (`15,900`, `8.05`, `37.0`).
pub fn format_sig3(value: f64) -> String {
    if !value.is_finite() {
        return value.to_string();
    }
    let scientific = format!("{:.2e}", value.abs());
    let Some((mantissa, exponent)) = scientific.split_once('e') else {
        return value.to_string();
    };
    let Ok(exp) = exponent.parse::<i32>() else {
        return value.to_string();
    };
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let body = if exp >= 2 {
        let integer = format!("{digits}{}", "0".repeat(exp as usize - 2));
        group_thousands(&integer)
    } else if exp >= 0 {
        let cut = exp as usize + 1;
        format!("{}.{}", &digits[..cut], &digits[cut..])
    } else {
        format!("0.{}{digits}", "0".repeat((-exp) as usize - 1))
    };
    if value < 0.0 {
        format!("-{body}")
    } else {
        body
    }
}

fn group_thousands(integer: &str) -> String {
    let mut out = String::with_capacity(integer.len() + integer.len() / 3);
    for (index, digit) in integer.chars().enumerate() {
        if index > 0 && (integer.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> Result<LocalUnits, String> {
        LocalUnits::embedded()
    }

    #[test]
    fn format_rounds_to_three_significant_digits() {
        assert_eq!(format_sig3(8.04672), "8.05");
        assert_eq!(format_sig3(15859.94), "15,900");
        assert_eq!(format_sig3(0.999744), "1.00");
        assert_eq!(format_sig3(37.0), "37.0");
        assert_eq!(format_sig3(0.5283441), "0.528");
        assert_eq!(format_sig3(7_929_970.5), "7,930,000");
        assert_eq!(format_sig3(-40.0), "-40.0");
        assert_eq!(format_sig3(0.0), "0.00");
    }

    #[test]
    fn format_leaves_non_finite_values_alone() {
        assert_eq!(format_sig3(f64::NAN), "NaN");
    }

    #[test]
    fn grouping_starts_at_four_digits() {
        assert_eq!(group_thousands("999"), "999");
        assert_eq!(group_thousands("1000"), "1,000");
        assert_eq!(group_thousands("1234567"), "1,234,567");
    }

    #[test]
    fn the_embedded_table_parses_and_knows_both_readers() -> Result<(), String> {
        let units = table()?;
        assert_eq!(units.local_currency(Lang::Ja), Some("jpy"));
        assert_eq!(units.local_currency(Lang::En), Some("usd"));
        Ok(())
    }

    #[test]
    fn parse_rejects_a_conversion_to_an_unknown_unit() {
        let broken = DEFAULT_TABLE.replace("to = \"km\"", "to = \"nowhere\"");
        assert!(LocalUnits::parse(broken.as_bytes()).is_err());
        assert!(LocalUnits::parse(b"[[[").is_err());
    }

    #[test]
    fn a_row_applies_its_arithmetic() -> Result<(), String> {
        let units = table()?;
        let km = units.convert(Lang::Ja, "mile", "km", 5.0);
        assert!(km.is_some_and(|v| (v - 8.04672).abs() < 1e-9));
        let celsius = units.convert(Lang::Ja, "f", "c", -40.0);
        assert!(celsius.is_some_and(|v| (v + 40.0).abs() < 1e-9));
        assert_eq!(units.convert(Lang::Ja, "km", "mile", 1.0), None);
        Ok(())
    }

    #[test]
    fn a_via_row_goes_through_the_middle_unit() -> Result<(), String> {
        let units = table()?;
        let targets = units.targets(Lang::En, "tsubo", 3.0);
        let names: Vec<&str> = targets.iter().map(|t| t.unit).collect();
        assert_eq!(names, ["m2", "sqft"]);
        assert!((targets[1].value - 106.7495).abs() < 1e-3);
        Ok(())
    }

    #[test]
    fn a_home_unit_has_no_targets() -> Result<(), String> {
        assert!(table()?.targets(Lang::Ja, "km", 5.0).is_empty());
        Ok(())
    }

    #[test]
    fn spellings_are_read_longest_first_and_ignore_ascii_case() -> Result<(), String> {
        let units = table()?;
        let hit = units.read_after("2 US Gallons", 1);
        let named = hit.and_then(|h| match h.resolved {
            Resolved::Unit(index) => units.unit_at(index).map(|u| u.id.clone()),
            Resolved::Ambiguous(_) => None,
        });
        assert_eq!(named.as_deref(), Some("us_gallon"));
        let bare = units.read_after("5 gallons", 1);
        assert!(bare.is_some_and(|h| matches!(h.resolved, Resolved::Ambiguous(Kind::Volume))));
        Ok(())
    }

    #[test]
    fn a_spelling_glued_to_a_longer_word_is_not_read() -> Result<(), String> {
        let units = table()?;
        assert_eq!(units.read_after("5 more minutes", 1), None);
        assert!(units.read_after("5 in the morning", 1).is_some());
        Ok(())
    }

    #[test]
    fn a_currency_symbol_is_read_before_the_number() -> Result<(), String> {
        let units = table()?;
        let hit = units.read_before("I paid $100.", 8);
        assert_eq!(hit.map(|h| (h.start, h.end)), Some((7, 8)));
        assert_eq!(units.read_before("I paid 100.", 7), None);
        Ok(())
    }

    #[test]
    fn placing_finds_a_unit_spelling_for_one_unit_only() -> Result<(), String> {
        let units = table()?;
        let lb = units
            .units
            .iter()
            .position(|u| u.id == "lb")
            .ok_or("no lb")?;
        assert!(units.place_after(lb, "10 ポンド", 2).is_some());
        let usd = units
            .units
            .iter()
            .position(|u| u.id == "usd")
            .ok_or("no usd")?;
        assert_eq!(units.place_after(usd, "10 ポンド", 2), None);
        assert_eq!(units.place_before(usd, "$100", 1), Some(0));
        Ok(())
    }

    #[test]
    fn ratio_is_factor_over_divisor() {
        let row = Conversion {
            reader: Lang::En,
            from: String::from("a"),
            to: String::from("b"),
            pre: 0.0,
            factor: 400.0,
            divisor: 121.0,
            post: 0.0,
            via: None,
        };
        assert!((row.ratio() - 400.0 / 121.0).abs() < 1e-12);
    }
}

//! CX-9: unit factors and three-significant-digit rounding.
//!
//! Each row converts a value and a unit with the built-in table and compares
//! the result, written to three significant digits, with the expected text.

mod support;

use hayamimi_relay::lang::Lang;
use hayamimi_relay::units::{DEFAULT_TABLE, LocalUnits, format_sig3};
use support::TestResult;

/// What the row check refuses.
#[derive(Debug, PartialEq)]
enum RowFault {
    NoRow,
    Mismatch { got: String },
}

type Formatter = fn(f64) -> String;

/// Converts with the table's row and compares the written value.
fn check_row_equals(
    table: &LocalUnits,
    reader: Lang,
    from: &str,
    to: &str,
    value: f64,
    expected: &str,
    write: Formatter,
) -> Result<(), RowFault> {
    let converted = table
        .convert(reader, from, to, value)
        .ok_or(RowFault::NoRow)?;
    let got = write(converted);
    if got == expected {
        Ok(())
    } else {
        Err(RowFault::Mismatch { got })
    }
}

/// A broken rounding that keeps four digits.
fn four_significant_digits(value: f64) -> String {
    let magnitude = value.abs().log10().floor();
    let decimals = (3.0 - magnitude).max(0.0);
    format!("{:.*}", decimals as usize, value)
}

fn expect_row(
    reader: Lang,
    from: &str,
    to: &str,
    value: f64,
    expected: &str,
    id: &str,
) -> TestResult {
    let table = LocalUnits::embedded()?;
    let verdict = check_row_equals(&table, reader, from, to, value, expected, format_sig3);
    assert_eq!(verdict, Ok(()), "{id}: {value} {from} -> {to}");
    Ok(())
}

/// Reader rows: ja reads foreign units, en reads metric and Japanese units.
fn ja(from: &str, to: &str, value: f64, expected: &str, id: &str) -> TestResult {
    expect_row(Lang::Ja, from, to, value, expected, id)
}

fn en(from: &str, to: &str, value: f64, expected: &str, id: &str) -> TestResult {
    expect_row(Lang::En, from, to, value, expected, id)
}

/// T-CX-9-001
#[test]
fn cx09_unit_factors_and_three_digit_rounding_mile_to_km() -> TestResult {
    ja("mile", "km", 5.0, "8.05", "T-CX-9-001")
}

/// T-CX-9-002
#[test]
fn cx09_unit_factors_and_three_digit_rounding_ft_to_m() -> TestResult {
    ja("ft", "m", 3.0, "0.914", "T-CX-9-002")
}

/// T-CX-9-003
#[test]
fn cx09_unit_factors_and_three_digit_rounding_inch_to_cm() -> TestResult {
    ja("inch", "cm", 10.0, "25.4", "T-CX-9-003")
}

/// T-CX-9-004
#[test]
fn cx09_unit_factors_and_three_digit_rounding_yd_to_m() -> TestResult {
    ja("yd", "m", 10.0, "9.14", "T-CX-9-004")
}

/// T-CX-9-005
#[test]
fn cx09_unit_factors_and_three_digit_rounding_lb_to_kg() -> TestResult {
    ja("lb", "kg", 10.0, "4.54", "T-CX-9-005")
}

/// T-CX-9-006
#[test]
fn cx09_unit_factors_and_three_digit_rounding_oz_to_g() -> TestResult {
    ja("oz", "g", 10.0, "283", "T-CX-9-006")
}

/// T-CX-9-007
#[test]
fn cx09_unit_factors_and_three_digit_rounding_fahrenheit_to_celsius() -> TestResult {
    ja("f", "c", 98.6, "37.0", "T-CX-9-007")
}

/// T-CX-9-008
#[test]
fn cx09_unit_factors_and_three_digit_rounding_us_gallon_to_l() -> TestResult {
    ja("us_gallon", "l", 2.0, "7.57", "T-CX-9-008")
}

/// T-CX-9-009
#[test]
fn cx09_unit_factors_and_three_digit_rounding_us_fl_oz_to_ml() -> TestResult {
    ja("us_floz", "ml", 10.0, "296", "T-CX-9-009")
}

/// T-CX-9-010
#[test]
fn cx09_unit_factors_and_three_digit_rounding_sq_ft_to_m2() -> TestResult {
    ja("sqft", "m2", 100.0, "9.29", "T-CX-9-010")
}

/// T-CX-9-011
#[test]
fn cx09_unit_factors_and_three_digit_rounding_acre_to_m2() -> TestResult {
    ja("acre", "m2", 0.1, "405", "T-CX-9-011")
}

/// Prints what the English reader gets for a Japanese unit (recorded, not judged).
fn record_japanese_unit(unit: &str, value: f64, id: &str) -> TestResult {
    let table = LocalUnits::embedded()?;
    let targets = table.targets(Lang::En, unit, value);
    assert_eq!(targets.len(), 2, "{id}: the table has rows for {unit}");
    for target in &targets {
        eprintln!(
            "{id}: {value} {unit} = {} {}",
            format_sig3(target.value),
            target.unit
        );
    }
    Ok(())
}

/// T-CX-9-012
#[test]
fn cx09_unit_factors_and_three_digit_rounding_tsubo_to_m2_and_sq_ft_recorded() -> TestResult {
    record_japanese_unit("tsubo", 3.0, "T-CX-9-012")
}

/// T-CX-9-013
#[test]
fn cx09_unit_factors_and_three_digit_rounding_tatami_to_m2_and_sq_ft_recorded() -> TestResult {
    record_japanese_unit("tatami", 6.0, "T-CX-9-013")
}

/// T-CX-9-014
#[test]
fn cx09_unit_factors_and_three_digit_rounding_km_to_mile() -> TestResult {
    en("km", "mile", 10.0, "6.21", "T-CX-9-014")
}

/// T-CX-9-015
#[test]
fn cx09_unit_factors_and_three_digit_rounding_m_to_ft() -> TestResult {
    en("m", "ft", 100.0, "328", "T-CX-9-015")
}

/// T-CX-9-016
#[test]
fn cx09_unit_factors_and_three_digit_rounding_cm_to_inch() -> TestResult {
    en("cm", "inch", 10.0, "3.94", "T-CX-9-016")
}

/// T-CX-9-017
#[test]
fn cx09_unit_factors_and_three_digit_rounding_kg_to_lb() -> TestResult {
    en("kg", "lb", 70.0, "154", "T-CX-9-017")
}

/// T-CX-9-018
#[test]
fn cx09_unit_factors_and_three_digit_rounding_g_to_oz() -> TestResult {
    en("g", "oz", 500.0, "17.6", "T-CX-9-018")
}

/// T-CX-9-019
#[test]
fn cx09_unit_factors_and_three_digit_rounding_celsius_to_fahrenheit() -> TestResult {
    en("c", "f", 20.0, "68.0", "T-CX-9-019")
}

/// T-CX-9-020
#[test]
fn cx09_unit_factors_and_three_digit_rounding_l_to_us_gallon() -> TestResult {
    en("l", "us_gallon", 2.0, "0.528", "T-CX-9-020")
}

/// T-CX-9-021
#[test]
fn cx09_unit_factors_and_three_digit_rounding_ml_to_us_fl_oz() -> TestResult {
    en("ml", "us_floz", 250.0, "8.45", "T-CX-9-021")
}

/// T-CX-9-022
#[test]
fn cx09_unit_factors_and_three_digit_rounding_m2_to_sq_ft() -> TestResult {
    en("m2", "sqft", 10.0, "108", "T-CX-9-022")
}

/// T-CX-9-023
#[test]
fn cx09_unit_factors_and_three_digit_rounding_ft_to_m_carries_into_the_next_digit() -> TestResult {
    ja("ft", "m", 3.28, "1.00", "T-CX-9-023")
}

/// T-CX-9-024
#[test]
fn cx09_unit_factors_and_three_digit_rounding_temperature_negative_and_zero() -> TestResult {
    ja("f", "c", -40.0, "-40.0", "T-CX-9-024: -40 F")?;
    en("c", "f", 0.0, "32.0", "T-CX-9-024: 0 C")
}

/// T-CX-9-025
#[test]
fn cx09_unit_factors_and_three_digit_rounding_forward_and_reverse_factors_round_trip() -> TestResult
{
    let table = LocalUnits::embedded()?;
    let pairs = [
        ("mile", "km"),
        ("ft", "m"),
        ("inch", "cm"),
        ("lb", "kg"),
        ("oz", "g"),
        ("us_gallon", "l"),
        ("us_floz", "ml"),
        ("sqft", "m2"),
    ];
    for (foreign, metric) in pairs {
        let forward = table
            .row(Lang::Ja, foreign, metric)
            .ok_or("missing forward row")?;
        let reverse = table
            .row(Lang::En, metric, foreign)
            .ok_or("missing reverse row")?;
        let product = forward.ratio() * reverse.ratio();
        assert!(
            (product - 1.0).abs() <= 1e-12,
            "T-CX-9-025: {foreign}<->{metric}: {product}"
        );
    }
    let to_c = table.row(Lang::Ja, "f", "c").ok_or("missing F to C row")?;
    let to_f = table.row(Lang::En, "c", "f").ok_or("missing C to F row")?;
    for x in [-40.0_f64, 0.0, 20.0, 98.6] {
        let back = to_f.apply(to_c.apply(x));
        let error = (back - x).abs() / x.abs().max(1.0);
        assert!(error <= 1e-12, "T-CX-9-025: F(C({x})) = {back}");
    }
    Ok(())
}

/// T-CX-9-026
#[test]
fn cx09_unit_factors_and_three_digit_rounding_shifted_factor_table_is_rejected_by_the_check()
-> TestResult {
    assert_eq!(
        DEFAULT_TABLE.matches("factor = 1.609344").count(),
        1,
        "T-CX-9-026: one mile factor in the table"
    );
    let shifted = DEFAULT_TABLE.replacen("factor = 1.609344", "factor = 0.1609344", 1);
    let table = LocalUnits::parse(shifted.as_bytes())?;
    let verdict = check_row_equals(&table, Lang::Ja, "mile", "km", 5.0, "8.05", format_sig3);
    assert_eq!(
        verdict,
        Err(RowFault::Mismatch {
            got: String::from("0.805")
        }),
        "T-CX-9-026"
    );
    Ok(())
}

/// T-CX-9-027
#[test]
fn cx09_unit_factors_and_three_digit_rounding_four_digit_rounding_is_rejected_by_the_check()
-> TestResult {
    let table = LocalUnits::embedded()?;
    let verdict = check_row_equals(
        &table,
        Lang::Ja,
        "mile",
        "km",
        5.0,
        "8.05",
        four_significant_digits,
    );
    assert_eq!(
        verdict,
        Err(RowFault::Mismatch {
            got: String::from("8.047")
        }),
        "T-CX-9-027"
    );
    Ok(())
}

//! CX-11: a rate file or a unit table rewritten while the reader runs is
//! used from the next line.
//!
//! The reader is opened once per test. Nothing waits between a rewrite and
//! the next line, so "from the next line" is what is measured.

mod support;

use std::fs;
use std::path::{Path, PathBuf};

use hayamimi_relay::annotate::{AnnotateRequest, annotate_with};
use hayamimi_relay::fx::Rates;
use hayamimi_relay::lang::Lang;
use hayamimi_relay::logging::NullSink;
use hayamimi_relay::strings::Strings;
use hayamimi_relay::units::{DEFAULT_TABLE, LocalUnits};
use support::{Harness, TempDir, TestResult, epoch, rates_a, write_by_rename, write_in_place};

const RATES_B: &str = include_str!("fixtures/cx11_rates_b.json");
const RATES_C: &str = include_str!("fixtures/cx11_rates_c.json");
const EMPTY: &str = include_str!("fixtures/cx11_empty.json");
const BROKEN_TABLE: &str = include_str!("fixtures/cx11_broken.toml");
const TRUNCATED: &str = include_str!("fixtures/cx07_after_truncated.json");

const NOW: &str = "2026-10-15T09:00:00+09:00";
const DOLLAR_SRC: &str = "I paid $100.";
const DOLLAR_EDITED: &str = "私は100ドル払いました。";
const YEN_SRC: &str = "予算は五百万円です。";
const YEN_EDITED: &str = "The budget is five million yen.";
const MILE_SRC: &str = "I walked 5 miles.";
const MILE_EDITED: &str = "5マイル歩きました。";

const NOTE_A: &str = "（約 15,900 円、10/12 の値）";
const NOTE_B: &str = "（約 15,000 円、10/13 の値）";
const NOTE_C: &str = "（約 14,000 円、10/14 の値）";
const NOTE_MILE_DEFAULT: &str = "（約 8.05 km）";
const NOTE_MILE_REWRITTEN: &str = "（約 8.00 km）";

type Failure = Box<dyn std::error::Error>;

/// What the annotation check refuses.
#[derive(Debug)]
enum AnnotationFault {
    Differs { got: String },
}

fn check_annotation_equals(got: &str, expected: &str) -> Result<(), AnnotationFault> {
    if got == expected {
        Ok(())
    } else {
        Err(AnnotationFault::Differs {
            got: got.to_owned(),
        })
    }
}

/// The text of the only note in a list.
fn only_note(notes: &[String], id: &str) -> Result<String, Failure> {
    match notes {
        [one] => Ok(one.clone()),
        other => Err(format!("{id}: expected one note, got {other:?}").into()),
    }
}

/// A reader opened once on a rate file in its own directory.
struct RateReader {
    _dir: TempDir,
    path: PathBuf,
    harness: Harness,
}

fn open_on_rates(content: &str) -> Result<RateReader, Failure> {
    let dir = TempDir::new("cx11")?;
    let path = dir.join("fx_rates.json");
    fs::write(&path, content)?;
    let harness = Harness::open(Some(path.clone()), None)?;
    Ok(RateReader {
        _dir: dir,
        path,
        harness,
    })
}

impl RateReader {
    fn note(&mut self, src: &str, lang: Lang, edited: &str, id: &str) -> Result<String, Failure> {
        let line = self.harness.line(src, lang, edited, epoch(NOW)?);
        let notes: Vec<String> = line
            .annotations
            .iter()
            .map(|note| note.text.trim().to_owned())
            .collect();
        only_note(&notes, id)
    }

    fn dollar_note(&mut self, id: &str) -> Result<String, Failure> {
        self.note(DOLLAR_SRC, Lang::En, DOLLAR_EDITED, id)
    }
}

/// A reader opened once on a directory that will receive table files.
struct TableReader {
    dir: TempDir,
    harness: Harness,
}

fn open_on_tables() -> Result<TableReader, Failure> {
    let dir = TempDir::new("cx11-tables")?;
    let harness = Harness::open(None, Some(dir.path().to_path_buf()))?;
    Ok(TableReader { dir, harness })
}

impl TableReader {
    fn units_file(&self) -> PathBuf {
        self.dir.join("local_units.toml")
    }

    fn mile_note(&mut self, id: &str) -> Result<String, Failure> {
        let line = self
            .harness
            .line(MILE_SRC, Lang::En, MILE_EDITED, epoch(NOW)?);
        let notes: Vec<String> = line
            .annotations
            .iter()
            .map(|note| note.text.trim().to_owned())
            .collect();
        only_note(&notes, id)
    }
}

/// The repository's table with the mile factor changed to 1.6.
fn rewritten_units_table() -> Result<String, Failure> {
    if !DEFAULT_TABLE.contains("1.609344") {
        return Err("the default table no longer holds 1.609344; nothing was replaced".into());
    }
    Ok(DEFAULT_TABLE.replace("1.609344", "1.6"))
}

/// A reader that reads its files once, when it is opened.
struct StartupOnlyReader {
    rates: Option<Rates>,
    units: LocalUnits,
    strings_ja: Strings,
    strings_en: Strings,
}

impl StartupOnlyReader {
    fn open(
        rates_path: Option<&Path>,
        units_path: Option<&Path>,
    ) -> Result<StartupOnlyReader, Failure> {
        let rates = match rates_path {
            Some(path) => Rates::parse_present(&fs::read(path)?)?,
            None => None,
        };
        let units = match units_path {
            Some(path) => LocalUnits::parse(&fs::read(path)?)?,
            None => LocalUnits::embedded()?,
        };
        Ok(StartupOnlyReader {
            rates,
            units,
            strings_ja: Strings::embedded(Lang::Ja)?,
            strings_en: Strings::embedded(Lang::En)?,
        })
    }

    fn note(&self, src: &str, lang: Lang, translation: &str, id: &str) -> Result<String, Failure> {
        let strings = match lang.other() {
            Lang::Ja => &self.strings_ja,
            Lang::En => &self.strings_en,
        };
        let request = AnnotateRequest {
            id: "startup-only",
            src,
            translation,
            src_lang: lang,
            now_epoch: epoch(NOW)?,
        };
        let result = annotate_with(
            &self.units,
            strings,
            self.rates.as_ref(),
            &request,
            &mut NullSink,
        );
        let notes: Vec<String> = result
            .annotations
            .iter()
            .map(|note| note.text.trim().to_owned())
            .collect();
        only_note(&notes, id)
    }
}

/// T-CX-11-001
#[test]
fn cx11_rewritten_rate_file_is_used_from_the_next_line_before_rewrite_uses_the_old_rate()
-> TestResult {
    let mut reader = open_on_rates(&rates_a())?;
    assert_eq!(reader.dollar_note("T-CX-11-001")?, NOTE_A, "T-CX-11-001");
    Ok(())
}

/// T-CX-11-002
#[test]
fn cx11_rewritten_rate_file_is_used_from_the_next_line_rewritten_rate_file_is_used_from_the_next_line()
-> TestResult {
    let mut reader = open_on_rates(&rates_a())?;
    reader.dollar_note("T-CX-11-002: before")?;
    write_by_rename(&reader.path, RATES_B.as_bytes())?;
    assert_eq!(reader.dollar_note("T-CX-11-002")?, NOTE_B, "T-CX-11-002");
    Ok(())
}

/// T-CX-11-003
#[test]
fn cx11_rewritten_rate_file_is_used_from_the_next_line_rewritten_rate_file_is_used_by_the_english_reader()
-> TestResult {
    let mut reader = open_on_rates(&rates_a())?;
    reader.note(YEN_SRC, Lang::Ja, YEN_EDITED, "T-CX-11-003: before")?;
    write_by_rename(&reader.path, RATES_B.as_bytes())?;
    let note = reader.note(YEN_SRC, Lang::Ja, YEN_EDITED, "T-CX-11-003")?;
    assert_eq!(note, "(about $33,300, rate of Oct 13)", "T-CX-11-003");
    Ok(())
}

/// T-CX-11-004
#[test]
fn cx11_rewritten_rate_file_is_used_from_the_next_line_in_place_overwrite_is_used_from_the_next_line()
-> TestResult {
    let mut reader = open_on_rates(&rates_a())?;
    reader.dollar_note("T-CX-11-004: before")?;
    write_in_place(&reader.path, RATES_B.as_bytes())?;
    assert_eq!(reader.dollar_note("T-CX-11-004")?, NOTE_B, "T-CX-11-004");
    Ok(())
}

/// T-CX-11-005
#[test]
fn cx11_rewritten_rate_file_is_used_from_the_next_line_second_rewrite_replaces_the_first()
-> TestResult {
    let mut reader = open_on_rates(&rates_a())?;
    write_by_rename(&reader.path, RATES_B.as_bytes())?;
    assert_eq!(
        reader.dollar_note("T-CX-11-005: first")?,
        NOTE_B,
        "T-CX-11-005: first"
    );
    write_by_rename(&reader.path, RATES_C.as_bytes())?;
    assert_eq!(
        reader.dollar_note("T-CX-11-005: second")?,
        NOTE_C,
        "T-CX-11-005: second"
    );
    Ok(())
}

/// T-CX-11-006
#[test]
fn cx11_rewritten_rate_file_is_used_from_the_next_line_truncated_rate_file_keeps_the_previous_values_and_warns_once()
-> TestResult {
    let mut reader = open_on_rates(&rates_a())?;
    write_by_rename(&reader.path, TRUNCATED.as_bytes())?;
    assert_eq!(reader.dollar_note("T-CX-11-006")?, NOTE_A, "T-CX-11-006");
    assert_eq!(reader.harness.warn_count(), 1, "T-CX-11-006: warn lines");
    reader.dollar_note("T-CX-11-006: again")?;
    assert_eq!(
        reader.harness.warn_count(),
        1,
        "T-CX-11-006: the same bad content warns once"
    );
    Ok(())
}

/// T-CX-11-007
#[test]
fn cx11_rewritten_rate_file_is_used_from_the_next_line_empty_rate_file_keeps_the_previous_values_and_warns_once()
-> TestResult {
    let mut reader = open_on_rates(&rates_a())?;
    write_by_rename(&reader.path, EMPTY.as_bytes())?;
    assert_eq!(reader.dollar_note("T-CX-11-007")?, NOTE_A, "T-CX-11-007");
    assert_eq!(reader.harness.warn_count(), 1, "T-CX-11-007: warn lines");
    Ok(())
}

/// T-CX-11-008
#[test]
fn cx11_rewritten_rate_file_is_used_from_the_next_line_valid_rewrite_after_a_broken_one_is_used()
-> TestResult {
    let mut reader = open_on_rates(&rates_a())?;
    write_by_rename(&reader.path, TRUNCATED.as_bytes())?;
    assert_eq!(
        reader.dollar_note("T-CX-11-008: broken")?,
        NOTE_A,
        "T-CX-11-008: broken"
    );
    write_by_rename(&reader.path, RATES_B.as_bytes())?;
    assert_eq!(reader.dollar_note("T-CX-11-008")?, NOTE_B, "T-CX-11-008");
    Ok(())
}

/// T-CX-11-009
#[test]
fn cx11_rewritten_rate_file_is_used_from_the_next_line_startup_only_reader_misses_the_rate_rewrite()
-> TestResult {
    let mut real = open_on_rates(&rates_a())?;
    let fake = StartupOnlyReader::open(Some(&real.path), None)?;
    let real_before = real.dollar_note("T-CX-11-009: real before")?;
    assert!(
        check_annotation_equals(&real_before, NOTE_A).is_ok(),
        "T-CX-11-009: real reader before"
    );
    write_by_rename(&real.path, RATES_B.as_bytes())?;
    let real_after = real.dollar_note("T-CX-11-009: real after")?;
    assert!(
        check_annotation_equals(&real_after, NOTE_B).is_ok(),
        "T-CX-11-009: real reader after"
    );
    let fake_after = fake.note(DOLLAR_SRC, Lang::En, DOLLAR_EDITED, "T-CX-11-009: fake")?;
    let verdict = check_annotation_equals(&fake_after, NOTE_B);
    assert!(
        matches!(&verdict, Err(AnnotationFault::Differs { got }) if got == NOTE_A),
        "T-CX-11-009: {verdict:?}"
    );
    Ok(())
}

/// T-CX-11-010
#[test]
fn cx11_rewritten_rate_file_is_used_from_the_next_line_before_rewrite_uses_the_default_table()
-> TestResult {
    let mut reader = open_on_tables()?;
    assert_eq!(
        reader.mile_note("T-CX-11-010")?,
        NOTE_MILE_DEFAULT,
        "T-CX-11-010"
    );
    Ok(())
}

/// T-CX-11-011
#[test]
fn cx11_rewritten_rate_file_is_used_from_the_next_line_rewritten_unit_table_is_used_from_the_next_line()
-> TestResult {
    let mut reader = open_on_tables()?;
    reader.mile_note("T-CX-11-011: before")?;
    write_by_rename(&reader.units_file(), rewritten_units_table()?.as_bytes())?;
    assert_eq!(
        reader.mile_note("T-CX-11-011")?,
        NOTE_MILE_REWRITTEN,
        "T-CX-11-011"
    );
    Ok(())
}

/// T-CX-11-012
#[test]
fn cx11_rewritten_rate_file_is_used_from_the_next_line_broken_unit_table_keeps_the_previous_values_and_warns_once()
-> TestResult {
    let mut reader = open_on_tables()?;
    reader.mile_note("T-CX-11-012: before")?;
    write_by_rename(&reader.units_file(), BROKEN_TABLE.as_bytes())?;
    assert_eq!(
        reader.mile_note("T-CX-11-012")?,
        NOTE_MILE_DEFAULT,
        "T-CX-11-012"
    );
    assert_eq!(reader.harness.warn_count(), 1, "T-CX-11-012: warn lines");
    Ok(())
}

/// T-CX-11-013
#[test]
fn cx11_rewritten_rate_file_is_used_from_the_next_line_startup_only_reader_misses_the_table_rewrite()
-> TestResult {
    let mut real = open_on_tables()?;
    let fake = StartupOnlyReader::open(None, None)?;
    real.mile_note("T-CX-11-013: real before")?;
    write_by_rename(&real.units_file(), rewritten_units_table()?.as_bytes())?;
    let real_after = real.mile_note("T-CX-11-013: real after")?;
    assert!(
        check_annotation_equals(&real_after, NOTE_MILE_REWRITTEN).is_ok(),
        "T-CX-11-013: real reader"
    );
    let fake_after = fake.note(MILE_SRC, Lang::En, MILE_EDITED, "T-CX-11-013: fake")?;
    let verdict = check_annotation_equals(&fake_after, NOTE_MILE_REWRITTEN);
    assert!(
        matches!(&verdict, Err(AnnotationFault::Differs { got }) if got == NOTE_MILE_DEFAULT),
        "T-CX-11-013: {verdict:?}"
    );
    Ok(())
}

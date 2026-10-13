//! Unit annotation: after a translation has passed verification, a converted
//! value in the reader's own units is added in brackets next to each value
//! and unit that the source and the translation share.
//!
//! The translation text itself is never edited. The note is returned with
//! its position, and the screen or the SRT writer places it. A figure the
//! relay cannot convert with certainty is reported in `skipped`, not guessed.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::calendar::{self, Date};
use crate::fx::{RATES_FILE_NAME, Rates};
use crate::lang::Lang;
use crate::logging::{Level, LogEvent, LogSink};
use crate::numbers::{self, MICRO, Number};
use crate::strings::Strings;
use crate::units::{self, Kind, LocalUnits, Resolved};
use crate::watched::Watched;

/// A rate older than this many calendar days is not used.
pub const MAX_RATE_AGE_DAYS: i64 = 7;

/// A converted-value note and where it goes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Annotation {
    /// Character (Unicode scalar value) index where the source figure starts.
    pub start: usize,
    /// Character index just past it; the note goes here.
    pub end: usize,
    pub text: String,
}

/// Why a figure got no note.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// The unit spelling could mean more than one unit.
    Ambiguous,
    /// No rate file, or the rate file lacks this currency.
    NoRate,
    /// The rate is more than [`MAX_RATE_AGE_DAYS`] days old.
    StaleRate,
    /// The figure was not found exactly once in the translation.
    Unplaced,
}

/// A figure that was read but not annotated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Skipped {
    pub kind: Kind,
    pub reason: SkipReason,
}

/// The result of annotating one line.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Annotated {
    pub annotations: Vec<Annotation>,
    pub skipped: Vec<Skipped>,
}

/// One line to annotate.
pub struct AnnotateRequest<'a> {
    /// The line's id, for the log only.
    pub id: &'a str,
    pub src: &'a str,
    /// The translation that passed verification.
    pub translation: &'a str,
    pub src_lang: Lang,
    /// The current instant (Unix seconds); decides whether a rate is stale.
    pub now_epoch: i64,
}

/// Where the annotator reads its files from.
#[derive(Default)]
pub struct AnnotatorConfig {
    /// The rate file; without it every currency is `no_rate`.
    pub rates_path: Option<PathBuf>,
    /// A directory whose `local_units.toml` and `strings.<locale>.toml`
    /// replace the built-in tables.
    pub tables_dir: Option<PathBuf>,
}

/// The annotation step with its files, re-read when they change.
pub struct Annotator {
    rates: Watched<Option<Rates>>,
    units: Watched<LocalUnits>,
    strings_ja: Watched<Strings>,
    strings_en: Watched<Strings>,
}

fn no_rates() -> Result<Option<Rates>, String> {
    Ok(None)
}

fn embedded_ja() -> Result<Strings, String> {
    Strings::embedded(Lang::Ja)
}

fn embedded_en() -> Result<Strings, String> {
    Strings::embedded(Lang::En)
}

impl Annotator {
    /// Opens the built-in tables, overlaid by files that exist.
    pub fn open(config: &AnnotatorConfig, sink: &mut dyn LogSink) -> Result<Annotator, String> {
        let table_file = |name: &str| -> Option<PathBuf> {
            config.tables_dir.as_deref().map(|dir| dir.join(name))
        };
        Ok(Annotator {
            rates: Watched::open(
                "fx_rates",
                config.rates_path.clone(),
                no_rates,
                Rates::parse_present,
                sink,
            )?,
            units: Watched::open(
                "local_units",
                table_file("local_units.toml"),
                LocalUnits::embedded,
                LocalUnits::parse,
                sink,
            )?,
            strings_ja: Watched::open(
                "strings_ja",
                table_file("strings.ja.toml"),
                embedded_ja,
                Strings::parse,
                sink,
            )?,
            strings_en: Watched::open(
                "strings_en",
                table_file("strings.en.toml"),
                embedded_en,
                Strings::parse,
                sink,
            )?,
        })
    }

    /// Annotates one line, first picking up any file that changed.
    pub fn annotate(&mut self, request: &AnnotateRequest<'_>, sink: &mut dyn LogSink) -> Annotated {
        self.rates.refresh(sink);
        self.units.refresh(sink);
        self.strings_ja.refresh(sink);
        self.strings_en.refresh(sink);
        let reader = request.src_lang.other();
        let strings = match reader {
            Lang::Ja => self.strings_ja.current(),
            Lang::En => self.strings_en.current(),
        };
        annotate_with(
            self.units.current(),
            strings,
            self.rates.current().as_ref(),
            request,
            sink,
        )
    }
}

/// The rate to use for a currency pair, or the reason there is none.
struct CurrencyRates {
    from: f64,
    to: f64,
    date: Date,
}

fn currency_rates(
    rates: Option<&Rates>,
    from: &str,
    to: &str,
    now_epoch: i64,
) -> Result<CurrencyRates, SkipReason> {
    let rates = rates.ok_or(SkipReason::NoRate)?;
    let date = rates.date().ok_or(SkipReason::NoRate)?;
    if calendar::days_between(date, calendar::jst_date(now_epoch)) > MAX_RATE_AGE_DAYS {
        return Err(SkipReason::StaleRate);
    }
    match (rates.rate(from), rates.rate(to)) {
        (Some(from), Some(to)) => Ok(CurrencyRates { from, to, date }),
        _ => Err(SkipReason::NoRate),
    }
}

/// Where the figure sits in the translation: byte offsets of the whole
/// expression (symbol, number, unit spelling), when there is exactly one.
fn place_in_translation(
    table: &LocalUnits,
    unit_index: usize,
    translation: &str,
    figure: &Number,
) -> Option<(usize, usize)> {
    let mut found = None;
    for candidate in numbers::extract(translation) {
        if candidate.micro != figure.micro || candidate.negative != figure.negative {
            continue;
        }
        let before = table.place_before(unit_index, translation, candidate.start);
        let after = table.place_after(unit_index, translation, candidate.end);
        if before.is_none() && after.is_none() {
            continue;
        }
        if found.is_some() {
            return None;
        }
        // A spelling on one side only: the number itself bounds the other.
        let from = match before {
            Some(start) => start,
            None => candidate.start,
        };
        let to = match after {
            Some(end) => end,
            None => candidate.end,
        };
        found = Some((from, to));
    }
    found
}

/// What a figure becomes: the converted values, and the rate date for money.
struct Conversion {
    values: Vec<(String, f64)>,
    date: Option<Date>,
}

fn convert_figure(
    table: &LocalUnits,
    rates: Option<&Rates>,
    unit_id: &str,
    kind: Kind,
    value: f64,
    request: &AnnotateRequest<'_>,
) -> Result<Option<Conversion>, SkipReason> {
    let reader = request.src_lang.other();
    if kind == Kind::Currency {
        let Some(home) = table.local_currency(reader) else {
            return Ok(None);
        };
        if home == unit_id {
            return Ok(None);
        }
        let pair = currency_rates(
            rates,
            &unit_id.to_ascii_uppercase(),
            &home.to_ascii_uppercase(),
            request.now_epoch,
        )?;
        let converted = value / pair.from * pair.to;
        return Ok(Some(Conversion {
            values: vec![(home.to_owned(), converted)],
            date: Some(pair.date),
        }));
    }
    let targets = table.targets(reader, unit_id, value);
    if targets.is_empty() {
        return Ok(None);
    }
    Ok(Some(Conversion {
        values: targets
            .iter()
            .map(|t| (t.unit.to_owned(), t.value))
            .collect(),
        date: None,
    }))
}

/// Writes the note: `（約 15,900 円、10/12 の値）` or `(about $31,500, rate of Oct 12)`.
fn render_note(strings: &Strings, conversion: &Conversion) -> Option<String> {
    let mut parts = Vec::with_capacity(conversion.values.len());
    for (unit, value) in &conversion.values {
        let label = strings.unit.get(unit)?;
        let number = units::format_sig3(*value);
        parts.push(if label.prefix {
            format!("{}{number}", label.label)
        } else if label.space {
            format!("{number} {}", label.label)
        } else {
            format!("{number}{}", label.label)
        });
    }
    let values = parts.join(&strings.annotation.join);
    Some(match conversion.date {
        Some(date) => strings
            .annotation
            .currency
            .replace("{values}", &values)
            .replace("{date}", &strings.format_date(date)),
        None => strings.annotation.plain.replace("{values}", &values),
    })
}

fn skip_event(id: &str, skipped: Skipped) -> LogEvent {
    let kind = format!("{:?}", skipped.kind).to_lowercase();
    let reason = match skipped.reason {
        SkipReason::Ambiguous => "ambiguous",
        SkipReason::NoRate => "no_rate",
        SkipReason::StaleRate => "stale_rate",
        SkipReason::Unplaced => "unplaced",
    };
    LogEvent::new(Level::Info, "annotate.line.skipped", "service")
        .with("id", id)
        .with("kind", kind)
        .with("reason", reason)
}

/// Annotates one line against already-loaded tables.
pub fn annotate_with(
    table: &LocalUnits,
    strings: &Strings,
    rates: Option<&Rates>,
    request: &AnnotateRequest<'_>,
    sink: &mut dyn LogSink,
) -> Annotated {
    let mut out = Annotated::default();
    for figure in numbers::extract(request.src) {
        let hit = table
            .read_before(request.src, figure.start)
            .or_else(|| table.read_after(request.src, figure.end));
        let Some(hit) = hit else {
            continue;
        };
        let (index, kind, unit_id) = match hit.resolved {
            Resolved::Ambiguous(kind) => {
                out.skipped.push(Skipped {
                    kind,
                    reason: SkipReason::Ambiguous,
                });
                continue;
            }
            Resolved::Unit(index) => match table.unit_at(index) {
                Some(unit) => (index, unit.kind, unit.id.as_str()),
                None => continue,
            },
        };
        let value = figure.micro as f64 / MICRO as f64;
        let value = if figure.negative { -value } else { value };
        let conversion = match convert_figure(table, rates, unit_id, kind, value, request) {
            Ok(Some(conversion)) => conversion,
            Ok(None) => continue,
            Err(reason) => {
                out.skipped.push(Skipped { kind, reason });
                continue;
            }
        };
        let Some((from, to)) = place_in_translation(table, index, request.translation, &figure)
        else {
            out.skipped.push(Skipped {
                kind,
                reason: SkipReason::Unplaced,
            });
            continue;
        };
        let Some(text) = render_note(strings, &conversion) else {
            sink.emit(
                LogEvent::new(Level::Warn, "annotate.label.missing", "service")
                    .with("id", request.id)
                    .with("unit", unit_id),
            );
            continue;
        };
        out.annotations.push(Annotation {
            start: request.translation[..from].chars().count(),
            end: request.translation[..to].chars().count(),
            text,
        });
    }
    out.annotations.sort_by_key(|a| (a.end, a.start));
    emit_events(request.id, &out, sink);
    out
}

fn emit_events(id: &str, out: &Annotated, sink: &mut dyn LogSink) {
    for _ in &out.annotations {
        sink.emit(LogEvent::new(Level::Info, "annotate.line.added", "service").with("id", id));
    }
    for skipped in &out.skipped {
        sink.emit(skip_event(id, *skipped));
    }
}

/// The rate file inside a data directory.
pub fn rates_file_in(dir: &Path) -> PathBuf {
    dir.join(RATES_FILE_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::MemorySink;

    const NOW: &str = "2026-10-13T09:00:00+09:00";

    fn rates_a() -> Rates {
        let mut rates = std::collections::BTreeMap::new();
        rates.insert(String::from("USD"), 1.1181);
        rates.insert(String::from("JPY"), 177.33);
        Rates {
            source_url: String::new(),
            rate_date: String::from("2026-10-12"),
            fetched_at: String::new(),
            base: String::from("EUR"),
            rates,
        }
    }

    fn run(
        src: &str,
        translation: &str,
        lang: Lang,
        rates: Option<&Rates>,
    ) -> Result<Annotated, String> {
        let table = LocalUnits::embedded()?;
        let strings = Strings::embedded(lang.other())?;
        let now = calendar::parse_rfc3339(NOW).ok_or("bad now")?;
        let request = AnnotateRequest {
            id: "t",
            src,
            translation,
            src_lang: lang,
            now_epoch: now,
        };
        Ok(annotate_with(
            &table,
            &strings,
            rates,
            &request,
            &mut MemorySink::default(),
        ))
    }

    #[test]
    fn dollars_become_yen_with_the_rate_date() -> Result<(), String> {
        let rates = rates_a();
        let said = run(
            "I paid $100.",
            "私は100ドル払いました。",
            Lang::En,
            Some(&rates),
        )?;
        assert_eq!(said.annotations.len(), 1);
        assert_eq!(said.annotations[0].text, "（約 15,900 円、10/12 の値）");
        assert_eq!((said.annotations[0].start, said.annotations[0].end), (2, 7));
        Ok(())
    }

    #[test]
    fn yen_become_dollars_for_an_english_reader() -> Result<(), String> {
        let rates = rates_a();
        let said = run(
            "予算は五百万円です。",
            "The budget is five million yen.",
            Lang::Ja,
            Some(&rates),
        )?;
        assert_eq!(said.annotations[0].text, "(about $31,500, rate of Oct 12)");
        assert_eq!(said.annotations[0].end, 30);
        Ok(())
    }

    #[test]
    fn a_missing_rate_file_skips_money_but_not_length() -> Result<(), String> {
        let said = run(
            "I paid $100 for 5 miles.",
            "5マイルで100ドル払いました。",
            Lang::En,
            None,
        )?;
        assert_eq!(said.annotations.len(), 1);
        assert_eq!(said.annotations[0].text, "（約 8.05 km）");
        assert_eq!(
            said.skipped,
            [Skipped {
                kind: Kind::Currency,
                reason: SkipReason::NoRate
            }]
        );
        Ok(())
    }

    #[test]
    fn a_stale_rate_is_skipped() -> Result<(), String> {
        let mut rates = rates_a();
        rates.rate_date = String::from("2026-10-05");
        let said = run(
            "I paid $100.",
            "私は100ドル払いました。",
            Lang::En,
            Some(&rates),
        )?;
        assert_eq!(
            said.skipped.first().map(|s| s.reason),
            Some(SkipReason::StaleRate)
        );
        rates.rate_date = String::from("2026-10-06");
        let fresh = run(
            "I paid $100.",
            "私は100ドル払いました。",
            Lang::En,
            Some(&rates),
        )?;
        assert_eq!(fresh.annotations.len(), 1);
        Ok(())
    }

    #[test]
    fn an_ambiguous_spelling_is_skipped_not_guessed() -> Result<(), String> {
        let said = run(
            "It weighs ten pounds.",
            "重さは10ポンドです。",
            Lang::En,
            None,
        )?;
        assert!(said.annotations.is_empty());
        assert_eq!(
            said.skipped,
            [Skipped {
                kind: Kind::Mass,
                reason: SkipReason::Ambiguous
            }]
        );
        Ok(())
    }

    #[test]
    fn a_figure_missing_from_the_translation_is_unplaced() -> Result<(), String> {
        let rates = rates_a();
        let said = run(
            "I paid $100.",
            "私は100払いました。",
            Lang::En,
            Some(&rates),
        )?;
        assert_eq!(
            said.skipped.first().map(|s| s.reason),
            Some(SkipReason::Unplaced)
        );
        Ok(())
    }

    #[test]
    fn a_dollar_sign_before_the_figure_is_part_of_the_span() -> Result<(), String> {
        let rates = rates_a();
        let said = run(
            "I paid $100.",
            "私は$100払いました。",
            Lang::En,
            Some(&rates),
        )?;
        assert_eq!((said.annotations[0].start, said.annotations[0].end), (2, 6));
        Ok(())
    }

    #[test]
    fn a_unit_already_at_home_gets_no_note() -> Result<(), String> {
        let said = run("It is 5 km away.", "5 kmの距離です。", Lang::En, None)?;
        assert!(said.annotations.is_empty() && said.skipped.is_empty());
        Ok(())
    }

    #[test]
    fn the_rates_file_path_joins_the_fixed_name() {
        assert!(rates_file_in(Path::new("d")).ends_with("fx_rates.json"));
    }

    #[test]
    fn the_annotator_opens_without_any_file() -> Result<(), Box<dyn std::error::Error>> {
        let mut sink = MemorySink::default();
        let mut annotator = Annotator::open(&AnnotatorConfig::default(), &mut sink)?;
        let now = calendar::parse_rfc3339(NOW).ok_or("bad now")?;
        let request = AnnotateRequest {
            id: "t",
            src: "5 miles",
            translation: "5マイル",
            src_lang: Lang::En,
            now_epoch: now,
        };
        let said = annotator.annotate(&request, &mut sink);
        assert_eq!(said.annotations.len(), 1);
        Ok(())
    }
}

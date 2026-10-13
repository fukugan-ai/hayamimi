//! Exchange rates: the rate file the annotation reads, and the fetch that
//! writes it.
//!
//! The fetch is one bare GET of the ECB daily reference file. It runs once a
//! morning from the task scheduler; nothing on the subtitle path touches the
//! network.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::calendar::{self, Date};
use crate::logging::{Level, LogEvent, LogSink};

/// The ECB euro foreign exchange reference rates, daily file.
pub const DEFAULT_SOURCE_URL: &str =
    "https://www.ecb.europa.eu/stats/eurofxref/eurofxref-daily.xml";
/// File name of the rate file inside the data directory.
pub const RATES_FILE_NAME: &str = "fx_rates.json";
/// The currency every ECB rate is quoted against.
pub const BASE_CURRENCY: &str = "EUR";
/// A fetch that has not finished in this long is abandoned.
const FETCH_TIMEOUT: Duration = Duration::from_secs(30);

/// The rate file: currencies per one unit of `base`, as of `rate_date`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rates {
    pub source_url: String,
    /// The date the ECB published the rates (`YYYY-MM-DD`), not the fetch date.
    pub rate_date: String,
    /// When this file was written (RFC 3339, +09:00).
    pub fetched_at: String,
    pub base: String,
    pub rates: BTreeMap<String, f64>,
}

impl Rates {
    /// Parses and validates a rate file.
    pub fn parse(bytes: &[u8]) -> Result<Rates, String> {
        let rates: Rates = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
        if rates.base != BASE_CURRENCY {
            return Err(String::from("base is not EUR"));
        }
        if rates.date().is_none() {
            return Err(String::from("rate_date is not a date"));
        }
        if rates.rates.is_empty()
            || rates
                .rates
                .values()
                .any(|rate| !rate.is_finite() || *rate <= 0.0)
        {
            return Err(String::from("rates are empty or not positive"));
        }
        Ok(rates)
    }

    /// The form [`crate::watched::Watched`] keeps: a file that was never
    /// read is `None`.
    pub fn parse_present(bytes: &[u8]) -> Result<Option<Rates>, String> {
        Rates::parse(bytes).map(Some)
    }

    /// `rate_date` as a calendar date.
    pub fn date(&self) -> Option<Date> {
        Date::parse_iso(&self.rate_date)
    }

    /// Units of `code` per one EUR; the base itself is 1.
    pub fn rate(&self, code: &str) -> Option<f64> {
        if code.eq_ignore_ascii_case(&self.base) {
            return Some(1.0);
        }
        self.rates.get(&code.to_ascii_uppercase()).copied()
    }
}

/// Why a fetch did not produce a new file.
#[derive(Debug)]
pub enum FetchError {
    /// The server could not be reached or the exchange failed.
    Connect(String),
    /// The server answered with a status other than 2xx.
    Status(u16),
    /// The body could not be read.
    Body(String),
    /// The body is not an XML document.
    Xml(String),
    /// The document has no dated `Cube`.
    MissingTime,
    /// The document lacks a rate the relay needs (`USD`, `JPY`) or one is malformed.
    MissingRate(&'static str),
    /// The new file could not be written.
    Write(io::Error),
}

impl FetchError {
    /// A short code for the log.
    pub fn reason(&self) -> &'static str {
        match self {
            FetchError::Connect(_) => "CONNECT",
            FetchError::Status(_) => "STATUS",
            FetchError::Body(_) => "BODY",
            FetchError::Xml(_) => "XML",
            FetchError::MissingTime => "NO_TIME",
            FetchError::MissingRate(_) => "NO_RATE",
            FetchError::Write(_) => "WRITE",
        }
    }
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::Connect(why) => write!(f, "connect: {why}"),
            FetchError::Status(code) => write!(f, "status {code}"),
            FetchError::Body(why) => write!(f, "body: {why}"),
            FetchError::Xml(why) => write!(f, "xml: {why}"),
            FetchError::MissingTime => write!(f, "no dated Cube"),
            FetchError::MissingRate(code) => write!(f, "no usable rate for {code}"),
            FetchError::Write(why) => write!(f, "write: {why}"),
        }
    }
}

impl std::error::Error for FetchError {}

/// The rates of one published day.
#[derive(Debug, PartialEq)]
pub struct Snapshot {
    pub rate_date: String,
    pub rates: BTreeMap<String, f64>,
}

/// Reads the ECB daily XML. A dated `Cube`, `USD` and `JPY` are required.
pub fn parse_ecb_xml(xml: &str) -> Result<Snapshot, FetchError> {
    let document =
        roxmltree::Document::parse(xml).map_err(|error| FetchError::Xml(error.to_string()))?;
    let dated = document
        .descendants()
        .find(|node| node.has_tag_name("Cube") && node.attribute("time").is_some())
        .ok_or(FetchError::MissingTime)?;
    let rate_date = dated.attribute("time").ok_or(FetchError::MissingTime)?;
    if Date::parse_iso(rate_date).is_none() {
        return Err(FetchError::MissingTime);
    }
    let mut rates = BTreeMap::new();
    for cube in dated.children().filter(|node| node.has_tag_name("Cube")) {
        let (Some(code), Some(text)) = (cube.attribute("currency"), cube.attribute("rate")) else {
            continue;
        };
        let Ok(rate) = text.parse::<f64>() else {
            return Err(FetchError::MissingRate("malformed"));
        };
        if !rate.is_finite() || rate <= 0.0 {
            return Err(FetchError::MissingRate("malformed"));
        }
        rates.insert(code.to_ascii_uppercase(), rate);
    }
    for needed in ["USD", "JPY"] {
        if !rates.contains_key(needed) {
            return Err(FetchError::MissingRate(needed));
        }
    }
    Ok(Snapshot {
        rate_date: rate_date.to_owned(),
        rates,
    })
}

/// Replaces the rate file in one step.
pub trait RateStore {
    fn replace(&self, path: &Path, bytes: &[u8]) -> io::Result<()>;
}

/// Writes a sibling file, flushes it, then renames it over the target, so a
/// reader sees the old file or the new one and never half of either.
pub struct AtomicFileStore;

impl RateStore for AtomicFileStore {
    fn replace(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        let Some(name) = path.file_name() else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "path has no file name",
            ));
        };
        let mut temp_name = name.to_os_string();
        temp_name.push(".tmp");
        let temp = path.with_file_name(temp_name);
        let written = (|| {
            let mut file = File::create(&temp)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            fs::rename(&temp, path)
        })();
        if written.is_err() {
            // The temp file is only a leftover of the failed write.
            let _ = fs::remove_file(&temp);
        }
        written
    }
}

/// Where to fetch from and where to write.
pub struct FetchOptions<'a> {
    pub source_url: &'a str,
    pub out: &'a Path,
}

/// One GET, nothing in the body or the query.
fn http_get(url: &str) -> Result<String, FetchError> {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(FETCH_TIMEOUT))
        .build();
    let agent = ureq::Agent::new_with_config(config);
    let mut response = agent.get(url).call().map_err(|error| match error {
        ureq::Error::StatusCode(code) => FetchError::Status(code),
        other => FetchError::Connect(other.to_string()),
    })?;
    response
        .body_mut()
        .read_to_string()
        .map_err(|error| FetchError::Body(error.to_string()))
}

fn fetch_inner(
    options: &FetchOptions<'_>,
    now_epoch: i64,
    store: &dyn RateStore,
) -> Result<Rates, FetchError> {
    let body = http_get(options.source_url)?;
    let snapshot = parse_ecb_xml(&body)?;
    let rates = Rates {
        source_url: options.source_url.to_owned(),
        rate_date: snapshot.rate_date,
        fetched_at: calendar::rfc3339_jst(now_epoch),
        base: BASE_CURRENCY.to_owned(),
        rates: snapshot.rates,
    };
    let mut bytes = serde_json::to_vec_pretty(&rates)
        .map_err(|error| FetchError::Write(io::Error::other(error)))?;
    bytes.push(b'\n');
    store
        .replace(options.out, &bytes)
        .map_err(FetchError::Write)?;
    Ok(rates)
}

/// Fetches the daily rates and writes the rate file. On any failure the
/// previous file is left as it was and one `error` event is logged.
pub fn fetch(
    options: &FetchOptions<'_>,
    now_epoch: i64,
    store: &dyn RateStore,
    sink: &mut dyn LogSink,
) -> Result<Rates, FetchError> {
    let result = fetch_inner(options, now_epoch, store);
    match &result {
        Ok(rates) => sink.emit(
            LogEvent::new(Level::Info, "fx.fetch.ok", "scheduler")
                .with("rate_date", rates.rate_date.as_str())
                .with("currencies", rates.rates.len()),
        ),
        Err(error) => {
            let mut event = LogEvent::new(Level::Error, "fx.fetch.failed", "scheduler")
                .with("reason", error.reason())
                .with_error(error.reason());
            if let FetchError::Status(code) = error {
                event = event.with("http_status", *code);
            }
            sink.emit(event);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "<?xml version='1.0' encoding='UTF-8'?>\n\
<gesmes:Envelope xmlns:gesmes='http://www.gesmes.org/xml/2002-08-01' xmlns='http://www.ecb.int/vocabulary/2002-08-01/eurofxref'>\n\
<gesmes:subject>Reference rates</gesmes:subject>\n\
<Cube><Cube time='2026-10-12'>\n\
<Cube currency='USD' rate='1.1181'/><Cube currency='JPY' rate='177.33'/>\n\
<Cube currency='GBP' rate='0.8700'/></Cube></Cube></gesmes:Envelope>";

    #[test]
    fn the_sample_document_is_read() {
        let snapshot = parse_ecb_xml(SAMPLE);
        assert_eq!(
            snapshot.as_ref().map(|s| s.rate_date.as_str()).ok(),
            Some("2026-10-12")
        );
        assert_eq!(
            snapshot.ok().and_then(|s| s.rates.get("JPY").copied()),
            Some(177.33)
        );
    }

    #[test]
    fn a_document_without_usd_is_refused() {
        let cut = SAMPLE.replace("<Cube currency='USD' rate='1.1181'/>", "");
        assert!(matches!(
            parse_ecb_xml(&cut),
            Err(FetchError::MissingRate("USD"))
        ));
    }

    #[test]
    fn a_document_without_a_time_is_refused() {
        let cut = SAMPLE.replace("<Cube time='2026-10-12'>", "<Cube>");
        assert!(matches!(parse_ecb_xml(&cut), Err(FetchError::MissingTime)));
    }

    #[test]
    fn text_that_is_not_xml_is_refused() {
        assert!(matches!(
            parse_ecb_xml("maintenance"),
            Err(FetchError::Xml(_))
        ));
    }

    #[test]
    fn a_non_positive_rate_is_refused() {
        let broken = SAMPLE.replace("1.1181", "0");
        assert!(matches!(
            parse_ecb_xml(&broken),
            Err(FetchError::MissingRate(_))
        ));
    }

    #[test]
    fn the_rate_file_round_trips_through_parse() -> Result<(), Box<dyn std::error::Error>> {
        let snapshot = parse_ecb_xml(SAMPLE)?;
        let rates = Rates {
            source_url: String::from(DEFAULT_SOURCE_URL),
            rate_date: snapshot.rate_date,
            fetched_at: String::from("2026-10-13T07:30:00+09:00"),
            base: String::from("EUR"),
            rates: snapshot.rates,
        };
        let bytes = serde_json::to_vec(&rates)?;
        assert_eq!(Rates::parse(&bytes)?, rates);
        Ok(())
    }

    #[test]
    fn parse_rejects_a_truncated_or_empty_file() {
        assert!(Rates::parse(b"{\"source_url\":\"https://www.ecb.eu").is_err());
        assert!(Rates::parse(b"").is_err());
        assert!(Rates::parse_present(b"{}").is_err());
    }

    #[test]
    fn parse_rejects_a_wrong_base_or_date() {
        let wrong_base = br#"{"source_url":"u","rate_date":"2026-10-12","fetched_at":"x","base":"USD","rates":{"JPY":1.0}}"#;
        let wrong_date = br#"{"source_url":"u","rate_date":"10/12","fetched_at":"x","base":"EUR","rates":{"JPY":1.0}}"#;
        assert!(Rates::parse(wrong_base).is_err());
        assert!(Rates::parse(wrong_date).is_err());
    }

    #[test]
    fn the_base_currency_has_rate_one() -> Result<(), Box<dyn std::error::Error>> {
        let snapshot = parse_ecb_xml(SAMPLE)?;
        let rates = Rates {
            source_url: String::new(),
            rate_date: snapshot.rate_date,
            fetched_at: String::new(),
            base: String::from("EUR"),
            rates: snapshot.rates,
        };
        assert_eq!(rates.rate("eur"), Some(1.0));
        assert_eq!(rates.rate("usd"), Some(1.1181));
        assert_eq!(rates.rate("CHF"), None);
        Ok(())
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/unit-tmp");
        let dir = base.join(format!("fx-{}-{name}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn the_atomic_store_replaces_without_leaving_a_temp_file()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = scratch("replace");
        let target = dir.join("fx_rates.json");
        fs::write(&target, b"old")?;
        AtomicFileStore.replace(&target, b"new")?;
        assert_eq!(fs::read(&target)?, b"new");
        assert!(!dir.join("fx_rates.json.tmp").exists());
        Ok(())
    }

    #[test]
    fn the_atomic_store_fails_cleanly_when_the_directory_is_missing() {
        let target = scratch("missing").join("no-such-dir").join("fx_rates.json");
        assert!(AtomicFileStore.replace(&target, b"new").is_err());
        assert!(!target.exists());
    }

    #[test]
    fn the_atomic_store_refuses_a_path_without_a_file_name() {
        assert!(AtomicFileStore.replace(Path::new("/"), b"x").is_err());
    }

    #[test]
    fn fetch_error_reasons_are_distinct_codes() {
        assert_eq!(FetchError::Status(500).reason(), "STATUS");
        assert_eq!(FetchError::MissingTime.reason(), "NO_TIME");
        assert_eq!(FetchError::Xml(String::new()).to_string(), "xml: ");
    }
}

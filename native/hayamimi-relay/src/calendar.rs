//! Calendar arithmetic for the fixed UTC+09:00 clock the relay reports in.
//!
//! The relay stamps files and logs in Japan Standard Time. Dates are counted
//! as calendar days (Gregorian, days-from-civil), never as elapsed hours.

use std::time::{SystemTime, UNIX_EPOCH};

/// Seconds east of UTC for the clock the relay reports in.
pub const JST_OFFSET_SECS: i64 = 9 * 3600;

/// A Gregorian calendar date.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub year: i32,
    pub month: u8,
    pub day: u8,
}

impl Date {
    /// Parses `YYYY-MM-DD`; `None` when the text is not a real date.
    pub fn parse_iso(text: &str) -> Option<Date> {
        let bytes = text.as_bytes();
        if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
            return None;
        }
        let year: i32 = text.get(0..4)?.parse().ok()?;
        let month: u8 = text.get(5..7)?.parse().ok()?;
        let day: u8 = text.get(8..10)?.parse().ok()?;
        let date = Date { year, month, day };
        (date.is_valid() && text.bytes().all(|b| b == b'-' || b.is_ascii_digit())).then_some(date)
    }

    fn is_valid(self) -> bool {
        (1..=12).contains(&self.month)
            && self.day >= 1
            && self.day <= days_in_month(self.year, self.month)
    }

    /// Days since 1970-01-01 (negative before it).
    pub fn days_since_epoch(self) -> i64 {
        let year = i64::from(self.year) - i64::from(self.month <= 2);
        let era = year.div_euclid(400);
        let year_of_era = year - era * 400;
        let month = i64::from(self.month);
        let shifted = if month > 2 { month - 3 } else { month + 9 };
        let day_of_year = (153 * shifted + 2) / 5 + i64::from(self.day) - 1;
        let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
        era * 146_097 + day_of_era - 719_468
    }

    /// The date `days` days after 1970-01-01.
    pub fn from_days_since_epoch(days: i64) -> Date {
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let day_of_era = z - era * 146_097;
        let year_of_era =
            (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let mp = (5 * day_of_year + 2) / 153;
        let day = day_of_year - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = year_of_era + era * 400 + i64::from(month <= 2);
        // The civil algorithm bounds month to 1..=12 and day to 1..=31.
        Date {
            year: year as i32,
            month: month as u8,
            day: day as u8,
        }
    }
}

fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// The calendar date in JST of an instant given as Unix seconds.
pub fn jst_date(epoch_secs: i64) -> Date {
    Date::from_days_since_epoch((epoch_secs + JST_OFFSET_SECS).div_euclid(86_400))
}

/// Calendar days from `earlier` to `later` (negative when `later` is first).
pub fn days_between(earlier: Date, later: Date) -> i64 {
    later.days_since_epoch() - earlier.days_since_epoch()
}

/// Formats Unix seconds as RFC 3339 in JST, e.g. `2026-10-13T07:30:00+09:00`.
pub fn rfc3339_jst(epoch_secs: i64) -> String {
    let local = epoch_secs + JST_OFFSET_SECS;
    let date = Date::from_days_since_epoch(local.div_euclid(86_400));
    let secs = local.rem_euclid(86_400);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}+09:00",
        date.year,
        date.month,
        date.day,
        secs / 3600,
        secs % 3600 / 60,
        secs % 60
    )
}

/// Parses an RFC 3339 timestamp with a `Z` or `+hh:mm` / `-hh:mm` offset
/// into Unix seconds. Fractional seconds are not accepted.
pub fn parse_rfc3339(text: &str) -> Option<i64> {
    let (date_text, rest) = text.split_once('T')?;
    let date = Date::parse_iso(date_text)?;
    let (clock, offset_secs) = split_offset(rest)?;
    let clock = clock.as_bytes();
    if clock.len() != 8 || clock[2] != b':' || clock[5] != b':' {
        return None;
    }
    let part = |range: std::ops::Range<usize>| -> Option<i64> {
        let slice = std::str::from_utf8(&clock[range]).ok()?;
        slice
            .bytes()
            .all(|b| b.is_ascii_digit())
            .then(|| slice.parse().ok())?
    };
    let (hour, minute, second) = (part(0..2)?, part(3..5)?, part(6..8)?);
    if hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    Some(date.days_since_epoch() * 86_400 + hour * 3600 + minute * 60 + second - offset_secs)
}

fn split_offset(rest: &str) -> Option<(&str, i64)> {
    if let Some(clock) = rest.strip_suffix('Z') {
        return Some((clock, 0));
    }
    let cut = rest.len().checked_sub(6)?;
    let (clock, offset) = (rest.get(..cut)?, rest.get(cut..)?);
    let sign = match offset.as_bytes()[0] {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    if offset.as_bytes()[3] != b':' {
        return None;
    }
    let hours: i64 = offset.get(1..3)?.parse().ok()?;
    let minutes: i64 = offset.get(4..6)?.parse().ok()?;
    Some((clock, sign * (hours * 3600 + minutes * 60)))
}

/// The current instant as Unix seconds; `None` when the system clock is
/// set before 1970.
pub fn now_epoch_secs() -> Option<i64> {
    let since = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(since.as_secs()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_day_zero_is_1970_01_01() {
        let date = Date::parse_iso("1970-01-01");
        assert_eq!(date.map(Date::days_since_epoch), Some(0));
    }

    #[test]
    fn days_round_trip_across_a_leap_day() {
        let leap = Date::parse_iso("2028-02-29");
        let days = leap.map(Date::days_since_epoch);
        assert_eq!(days.map(Date::from_days_since_epoch), leap);
    }

    #[test]
    fn parse_iso_rejects_impossible_dates() {
        assert_eq!(Date::parse_iso("2026-02-29"), None);
        assert_eq!(Date::parse_iso("2026-13-01"), None);
        assert_eq!(Date::parse_iso("2026-1-01"), None);
        assert_eq!(Date::parse_iso("2026-10-+1"), None);
        assert_eq!(Date::parse_iso(""), None);
    }

    #[test]
    fn days_between_counts_calendar_days() {
        let from = Date::parse_iso("2026-10-12");
        let to = Date::parse_iso("2026-10-20");
        assert_eq!(from.zip(to).map(|(a, b)| days_between(a, b)), Some(8));
    }

    #[test]
    fn rfc3339_round_trips_in_jst() {
        let text = "2026-10-13T09:00:00+09:00";
        let secs = parse_rfc3339(text);
        assert_eq!(secs.map(rfc3339_jst).as_deref(), Some(text));
    }

    #[test]
    fn rfc3339_accepts_utc_and_other_offsets() {
        assert_eq!(
            parse_rfc3339("2026-10-13T00:00:00Z"),
            parse_rfc3339("2026-10-13T09:00:00+09:00")
        );
        assert_eq!(
            parse_rfc3339("2026-10-12T19:00:00-05:00"),
            parse_rfc3339("2026-10-13T00:00:00Z")
        );
    }

    #[test]
    fn rfc3339_rejects_malformed_text() {
        assert_eq!(parse_rfc3339("2026-10-13 09:00:00+09:00"), None);
        assert_eq!(parse_rfc3339("2026-10-13T09:00:00"), None);
        assert_eq!(parse_rfc3339("2026-10-13T25:00:00+09:00"), None);
    }

    #[test]
    fn jst_date_rolls_over_at_local_midnight() {
        let before = parse_rfc3339("2026-10-13T23:59:59+09:00");
        let after = parse_rfc3339("2026-10-14T00:00:00+09:00");
        assert_eq!(before.map(jst_date).map(|d| d.day), Some(13));
        assert_eq!(after.map(jst_date).map(|d| d.day), Some(14));
    }

    #[test]
    fn the_system_clock_is_after_1970() {
        assert!(now_epoch_secs().is_some_and(|secs| secs > 0));
    }
}

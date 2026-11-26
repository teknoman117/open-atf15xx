//! C `ctime()` formatting (§6.3): `Www Mmm dd hh:mm:ss yyyy`, day space-padded to width 2.

use chrono::{DateTime, Local, TimeZone, Utc};

/// `ctime()` layout; `%e` is the space-padded day of month.
pub const CTIME_FORMAT: &str = "%a %b %e %H:%M:%S %Y";

pub fn format<Tz: TimeZone>(t: &DateTime<Tz>) -> String
where
    Tz::Offset: std::fmt::Display,
{
    t.format(CTIME_FORMAT).to_string()
}

/// Local time now, as ATMISP writes it.
pub fn now_local() -> String {
    format(&Local::now())
}

/// A Unix timestamp rendered in UTC (used for `SOURCE_DATE_EPOCH`).
pub fn from_epoch_utc(epoch: i64) -> Option<String> {
    Utc.timestamp_opt(epoch, 0).single().map(|t| format(&t))
}

#[cfg(test)]
mod tests {
    #[test]
    fn ctime_format() {
        // 2026-11-05 09:03:07 UTC was a Thursday
        assert_eq!(super::from_epoch_utc(1793869387).unwrap(), "Thu Nov  5 09:03:07 2026");
    }
}

//! Shared price-history windowing + downsampling, reused by the per-card price-history
//! endpoint ([`crate::handlers::catalog::prices`]), the sealed-product one
//! ([`crate::handlers::catalog::products`]), and the collection value-over-time endpoint
//! ([`crate::handlers::collection::value_history`]).
//!
//! Each per-entity series is one row per `(entity, day)` of decimal-string prices, and the
//! collection series aggregates many of those into one total-per-day line; only the row
//! shape differs. So the `?range` vocabulary, the window cutoff, and the "keep the last
//! real row per bucket" downsampling live here, generic over the row type via a date
//! accessor — the handlers just convert the retained rows into their own point DTO.

use chrono::{Datelike, Duration, NaiveDate};
use serde::Deserialize;

use crate::error::AppError;

/// Query params for a price-history / value-history endpoint.
#[derive(Debug, Deserialize)]
pub struct PriceParams {
    /// Window + resolution (`7d`/`30d`/`1y`/`2y`/`3y`/`all`). Absent/blank = the
    /// full daily series; an unknown value is a 422.
    #[serde(default)]
    pub range: Option<String>,
}

/// Time window + sampling resolution for a price history, selected by a detail-page or
/// collection chart via `?range`. Longer windows are **downsampled** to a coarser
/// resolution so the wire payload (and the plotted line) stays light however much history
/// accrues. When no `range` is given the endpoint returns the full, un-sampled daily series.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PriceRange {
    /// Last 7 days, daily.
    D7,
    /// Last 30 days, daily.
    D30,
    /// Last year, weekly.
    Y1,
    /// Last 2 years, fortnightly.
    Y2,
    /// Last 3 years, monthly.
    Y3,
    /// All of history, every ~2 months.
    All,
}

impl PriceRange {
    /// An unrecognised value is a 422 (consistent with a bad `sort`/`q`). Blank/absent
    /// is handled by the caller (it means "full series"), so this is only ever called
    /// with a non-empty value.
    pub(crate) fn parse(value: &str) -> Result<Self, AppError> {
        Ok(match value {
            "7d" => PriceRange::D7,
            "30d" => PriceRange::D30,
            "1y" => PriceRange::Y1,
            "2y" => PriceRange::Y2,
            "3y" => PriceRange::Y3,
            "all" => PriceRange::All,
            other => return Err(AppError::Validation(format!("unknown range '{other}'"))),
        })
    }

    /// How many days back the window reaches, or `None` for all of history.
    fn window_days(self) -> Option<i64> {
        match self {
            PriceRange::D7 => Some(7),
            PriceRange::D30 => Some(30),
            PriceRange::Y1 => Some(365),
            PriceRange::Y2 => Some(730),
            PriceRange::Y3 => Some(1095),
            PriceRange::All => None,
        }
    }

    /// Width of one downsample bucket in days; one representative day (the most recent
    /// in the bucket) is kept per bucket, so a larger value = coarser chart.
    pub(crate) fn bucket_days(self) -> i64 {
        match self {
            PriceRange::D7 | PriceRange::D30 => 1,
            PriceRange::Y1 => 7,
            PriceRange::Y2 => 14,
            PriceRange::Y3 => 30,
            PriceRange::All => 60,
        }
    }
}

/// The inclusive lower bound (`"YYYY-MM-DD"`) for a range's window relative to `today`,
/// or `None` for [`PriceRange::All`] (no lower bound). Pure so the date arithmetic stays
/// unit-testable; the handler passes `Utc::now().date_naive()`.
pub(crate) fn cutoff_date(today: NaiveDate, range: PriceRange) -> Option<String> {
    range
        .window_days()
        .map(|days| crate::scryfall::format_date(today - Duration::days(days)))
}

/// Downsample an **ascending** run of price-history rows to one representative row per
/// `bucket_days`-wide bucket, keeping the *last* (most recent) row in each bucket — so
/// the newest day is always retained. `bucket_days <= 1` is a passthrough (full
/// resolution). Rows are kept whole (never averaged), so every returned row stays a
/// real, internally consistent day. Generic over the row type via `date_of`, which
/// yields each row's `"YYYY-MM-DD"` string.
pub(crate) fn downsample_rows<T>(
    rows: Vec<T>,
    bucket_days: i64,
    date_of: impl Fn(&T) -> &str,
) -> Vec<T> {
    if bucket_days <= 1 {
        return rows;
    }
    let mut out: Vec<T> = Vec::new();
    let mut last_key: Option<i64> = None;
    for row in rows {
        // Bucket on (days-since-CE / width). For zero-padded `YYYY-MM-DD` rows the keys
        // are monotonic in an ascending series, so equal keys are contiguous. An
        // unparseable date (shouldn't happen) gets a sentinel key that never coalesces,
        // keeping the row rather than dropping it.
        let key = NaiveDate::parse_from_str(date_of(&row), "%Y-%m-%d")
            .map(|d| i64::from(d.num_days_from_ce()) / bucket_days)
            .unwrap_or(i64::MIN);
        if last_key == Some(key) && key != i64::MIN {
            *out.last_mut().expect("out is non-empty once last_key is set") = row;
        } else {
            out.push(row);
            last_key = Some(key);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rejects_unknown_range() {
        assert!(PriceRange::parse("7d").is_ok());
        assert!(PriceRange::parse("nope").is_err());
    }

    #[test]
    fn downsample_keeps_last_row_per_bucket() {
        // Weekly buckets (7 days): three rows in one week collapse to the newest, and
        // a row in the next week is its own bucket.
        let rows = vec![
            "2024-01-01".to_string(),
            "2024-01-03".to_string(),
            "2024-01-06".to_string(),
            "2024-01-10".to_string(),
        ];
        let kept = downsample_rows(rows, 7, |s| s.as_str());
        // Days-since-CE / 7 buckets 01-01..01-06 together (last = 01-06), 01-10 alone.
        assert_eq!(kept, vec!["2024-01-06".to_string(), "2024-01-10".to_string()]);
    }

    #[test]
    fn downsample_passthrough_at_daily_resolution() {
        let rows = vec!["2024-01-01".to_string(), "2024-01-02".to_string()];
        assert_eq!(downsample_rows(rows.clone(), 1, |s| s.as_str()), rows);
    }
}

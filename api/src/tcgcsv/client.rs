//! HTTP helpers for TCGCSV (https://tcgcsv.com), a free keyless daily mirror of
//! TCGplayer's catalog + prices.
//!
//! We fetch one daily *price archive* at a time: a solid-PPMd `7z` bundling every
//! category's `prices` JSON for that day. TCGCSV blocks generic/empty User-Agents,
//! so every request carries the configured descriptive `TCGCSV_USER_AGENT`.

use bytes::Bytes;
use chrono::NaiveDate;
use reqwest::{Client, StatusCode, header};

use super::BASE_URL;
use super::BackfillError;

/// The archive URL for a given day's prices, e.g.
/// `https://tcgcsv.com/archive/tcgplayer/prices-2024-02-08.ppmd.7z`.
pub fn archive_url(date: NaiveDate) -> String {
    format!(
        "{BASE_URL}/archive/tcgplayer/prices-{}.ppmd.7z",
        date.format("%Y-%m-%d")
    )
}

/// Download the price archive for `date`, sending the configured User-Agent.
///
/// Returns `Ok(None)` when TCGCSV has no archive for that day (`404`) — there is no
/// archive index, so the caller constructs candidate dates and tolerates the gaps.
/// Any other non-success status is an error.
pub async fn fetch_archive(
    client: &Client,
    user_agent: &str,
    date: NaiveDate,
) -> Result<Option<Bytes>, BackfillError> {
    let response = client
        .get(archive_url(date))
        // Overrides the client's default UA (TCGCSV rejects generic ones).
        .header(header::USER_AGENT, user_agent)
        .send()
        .await?;
    if response.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let response = response.error_for_status()?;
    Ok(Some(response.bytes().await?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_url_is_dated() {
        let d = NaiveDate::from_ymd_opt(2024, 2, 8).unwrap();
        assert_eq!(
            archive_url(d),
            "https://tcgcsv.com/archive/tcgplayer/prices-2024-02-08.ppmd.7z"
        );
    }
}

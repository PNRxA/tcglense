//! HTTP helpers for TCGCSV (https://tcgcsv.com), a free keyless daily mirror of
//! TCGplayer's catalog + prices.
//!
//! We fetch one daily *price archive* at a time: a solid-PPMd `7z` bundling every
//! category's `prices` JSON for that day. TCGCSV blocks generic/empty User-Agents,
//! so every request carries the configured descriptive `TCGCSV_USER_AGENT`.

use bytes::Bytes;
use chrono::NaiveDate;
use reqwest::{Client, StatusCode, header};

use super::BackfillError;
use super::model::{GroupsFile, PriceFile, ProductsFile};

/// Fetch TCGCSV's `last-updated.txt` — a plain-text timestamp bumped once a day when
/// the daily data refresh completes. Used to version-gate the whole products sweep so
/// an unchanged day is a single cheap request (their documented pattern). `base_url`
/// is the upstream host or its TCGLense mirror, per the dataset source.
pub async fn last_updated(
    client: &Client,
    base_url: &str,
    user_agent: &str,
) -> Result<String, BackfillError> {
    let text = client
        .get(format!("{base_url}/last-updated.txt"))
        .header(header::USER_AGENT, user_agent)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    Ok(text.trim().to_string())
}

/// Fetch all groups (roughly sets/expansions) for a category — one unpaginated call.
pub async fn fetch_groups(
    client: &Client,
    base_url: &str,
    user_agent: &str,
    category_id: u32,
) -> Result<GroupsFile, BackfillError> {
    Ok(client
        .get(format!("{base_url}/tcgplayer/{category_id}/groups"))
        .header(header::USER_AGENT, user_agent)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

/// Fetch every product in a group (cards + sealed; the caller filters to sealed).
pub async fn fetch_products(
    client: &Client,
    base_url: &str,
    user_agent: &str,
    category_id: u32,
    group_id: i64,
) -> Result<ProductsFile, BackfillError> {
    Ok(client
        .get(format!(
            "{base_url}/tcgplayer/{category_id}/{group_id}/products"
        ))
        .header(header::USER_AGENT, user_agent)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

/// Fetch a group's live prices (same shape as the archive `prices` files). Only
/// products with active listings appear, so absence is normal.
pub async fn fetch_prices(
    client: &Client,
    base_url: &str,
    user_agent: &str,
    category_id: u32,
    group_id: i64,
) -> Result<PriceFile, BackfillError> {
    Ok(client
        .get(format!(
            "{base_url}/tcgplayer/{category_id}/{group_id}/prices"
        ))
        .header(header::USER_AGENT, user_agent)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

/// The archive URL for a given day's prices under `base_url`, e.g.
/// `https://tcgcsv.com/archive/tcgplayer/prices-2024-02-08.ppmd.7z`.
pub fn archive_url(base_url: &str, date: NaiveDate) -> String {
    format!(
        "{base_url}/archive/tcgplayer/prices-{}.ppmd.7z",
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
    base_url: &str,
    user_agent: &str,
    date: NaiveDate,
) -> Result<Option<Bytes>, BackfillError> {
    let response = client
        .get(archive_url(base_url, date))
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
            archive_url(super::super::BASE_URL, d),
            "https://tcgcsv.com/archive/tcgplayer/prices-2024-02-08.ppmd.7z"
        );
        // The mirror base flows straight through the same join.
        assert_eq!(
            archive_url("https://tcglense.com/api/mirror/tcgcsv", d),
            "https://tcglense.com/api/mirror/tcgcsv/archive/tcgplayer/prices-2024-02-08.ppmd.7z"
        );
    }
}

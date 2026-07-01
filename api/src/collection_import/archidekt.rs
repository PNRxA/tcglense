//! Archidekt collection provider.
//!
//! Archidekt's public collection API is paginated JSON at
//! `https://archidekt.com/api/collection/{id}/?page={n}` — 25 rows per page, with no
//! page-size override. Each row's `card.uid` is the Scryfall id (our
//! `cards.external_id`) and the row's `foil` boolean is the finish (the sibling
//! `modifier` field is *not* a reliable finish signal). The same printing can appear
//! across several rows (differing condition / language / tags), so the caller
//! aggregates by `(uid, foil)`.
//!
//! We construct every request URL ourselves from the host constant plus a
//! digits-only collection id, so there is no SSRF surface (the id can carry neither a
//! host nor a path).

use reqwest::{StatusCode, header};
use serde::Deserialize;

use super::rate_limit::RateLimiter;
use super::{FetchedHolding, ImportError, MAX_IMPORT_ROWS};

const API_BASE: &str = "https://archidekt.com/api/collection";
const ACCEPT_JSON: &str = "application/json";
/// Archidekt's fixed collection page size.
const PAGE_SIZE: usize = 25;

#[derive(Debug, Deserialize)]
struct CollectionPage {
    #[serde(default)]
    count: usize,
    /// The next page's URL, or `null`/absent on the last page (DRF pagination). We
    /// drive pagination off this rather than `count` so a missing/renamed `count`
    /// can't silently truncate the import.
    #[serde(default)]
    next: Option<String>,
    #[serde(default)]
    results: Vec<CollectionRow>,
}

#[derive(Debug, Deserialize)]
struct CollectionRow {
    #[serde(default)]
    quantity: i32,
    #[serde(default)]
    foil: bool,
    card: RowCard,
}

#[derive(Debug, Deserialize)]
struct RowCard {
    /// Scryfall card id — equals our `cards.external_id`.
    uid: String,
}

/// Extract the collection id from a user-supplied source: a full Archidekt URL
/// (`https://archidekt.com/collection/v2/1042487`, `/collection/1042487`, with or
/// without a trailing slash, query, or fragment) or a bare numeric id. Returns the id,
/// or `None` if no plausible (all-digits) id is present.
pub fn parse_collection_id(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    // A bare numeric id.
    if trimmed.bytes().all(|b| b.is_ascii_digit()) {
        return Some(trimmed.to_string());
    }
    // Otherwise treat it as a URL/path: drop the scheme and any query/fragment, then
    // take the last all-digits path segment (skips `v2`, `collection`, the host, …).
    let without_scheme = trimmed
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(trimmed);
    let path = without_scheme
        .split(['?', '#'])
        .next()
        .unwrap_or(without_scheme);
    path.split('/')
        .filter(|seg| !seg.is_empty())
        .rev()
        .find(|seg| seg.bytes().all(|b| b.is_ascii_digit()))
        .map(|seg| seg.to_string())
}

/// Fetch every holding for an Archidekt collection id, following pagination. Enforces
/// [`MAX_IMPORT_ROWS`] up front (via the first page's `count`) so a huge public
/// collection can't make us fan out an unbounded number of upstream requests.
pub async fn fetch(
    http: &reqwest::Client,
    limiter: &RateLimiter,
    collection_id: &str,
) -> Result<Vec<FetchedHolding>, ImportError> {
    // A hard page ceiling bounds the worst case even if the provider keeps handing us a
    // `next` link (25 rows/page, so this many pages covers `MAX_IMPORT_ROWS`).
    let max_pages = MAX_IMPORT_ROWS / PAGE_SIZE + 1;

    let mut holdings: Vec<FetchedHolding> = Vec::new();
    let mut checked_size = false;

    for page in 1..=max_pages {
        // Respect the provider's request cap across all imports before every request.
        limiter.acquire().await;

        let url = format!("{API_BASE}/{collection_id}/?page={page}");
        let response = http
            .get(&url)
            .header(header::ACCEPT, ACCEPT_JSON)
            .send()
            .await
            .map_err(|e| ImportError::Upstream(format!("request to Archidekt failed: {e}")))?;

        // Archidekt answers a missing or private collection with `400 ["No public
        // collection found."]`; treat 400/404 alike as not-found. We follow the `next`
        // link and never request past the last page, so this only fires on page 1.
        let status = response.status();
        if status == StatusCode::BAD_REQUEST || status == StatusCode::NOT_FOUND {
            return Err(ImportError::CollectionNotFound(collection_id.to_string()));
        }
        let response = response
            .error_for_status()
            .map_err(|e| ImportError::Upstream(format!("Archidekt returned an error: {e}")))?;
        let body: CollectionPage = response.json().await.map_err(|e| {
            ImportError::Upstream(format!("couldn't parse the Archidekt response: {e}"))
        })?;

        // Reject an over-large collection up front (from the first page's `count`).
        if !checked_size {
            if body.count > MAX_IMPORT_ROWS {
                return Err(ImportError::TooLarge {
                    count: body.count,
                    max: MAX_IMPORT_ROWS,
                });
            }
            checked_size = true;
        }

        // An empty page means we're done (a truly empty collection on page 1, or a
        // provider quirk) — nothing more to collect.
        if body.results.is_empty() {
            break;
        }
        for row in body.results {
            holdings.push(FetchedHolding {
                external_card_id: row.card.uid,
                foil: row.foil,
                quantity: row.quantity,
            });
        }
        // Stop when the provider says there's no next page — the source of truth for
        // "last page", robust to a missing/zero `count`.
        if body.next.is_none() {
            break;
        }
        // Belt-and-braces: bound total rows even if `next` never clears.
        if holdings.len() >= MAX_IMPORT_ROWS {
            break;
        }
    }

    Ok(holdings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bare_numeric_id() {
        assert_eq!(parse_collection_id("1042487").as_deref(), Some("1042487"));
        assert_eq!(parse_collection_id("  1042487  ").as_deref(), Some("1042487"));
    }

    #[test]
    fn parses_url_forms() {
        assert_eq!(
            parse_collection_id("https://archidekt.com/collection/v2/1042487").as_deref(),
            Some("1042487")
        );
        assert_eq!(
            parse_collection_id("https://archidekt.com/collection/1042487/").as_deref(),
            Some("1042487")
        );
        assert_eq!(
            parse_collection_id("archidekt.com/collection/v2/1042487?foo=bar#frag").as_deref(),
            Some("1042487")
        );
        assert_eq!(
            parse_collection_id("http://archidekt.com/api/collection/1042487/?page=2").as_deref(),
            Some("1042487")
        );
    }

    #[test]
    fn rejects_sources_without_an_id() {
        assert_eq!(parse_collection_id(""), None);
        assert_eq!(parse_collection_id("   "), None);
        assert_eq!(parse_collection_id("https://archidekt.com/collection/"), None);
        assert_eq!(parse_collection_id("not-a-url"), None);
    }

    #[test]
    fn deserializes_a_collection_page() {
        // Trimmed to the fields we consume, in Archidekt's real response shape.
        let json = r#"{
            "count": 3,
            "next": "http://archidekt.com/api/collection/1/?page=2",
            "results": [
                { "quantity": 2, "foil": false, "card": { "uid": "aaa" } },
                { "quantity": 1, "foil": true,  "card": { "uid": "bbb" } }
            ]
        }"#;
        let page: CollectionPage = serde_json::from_str(json).expect("parse page");
        assert_eq!(page.count, 3);
        assert_eq!(page.next.as_deref(), Some("http://archidekt.com/api/collection/1/?page=2"));
        assert_eq!(page.results.len(), 2);
        assert_eq!(page.results[0].card.uid, "aaa");
        assert!(!page.results[0].foil);
        assert_eq!(page.results[0].quantity, 2);
        assert!(page.results[1].foil);
    }

    #[test]
    fn last_page_has_no_next() {
        // The final page carries `next: null`; that's what stops pagination.
        let json = r#"{ "count": 1, "next": null, "results": [
            { "quantity": 1, "foil": false, "card": { "uid": "aaa" } }
        ] }"#;
        let page: CollectionPage = serde_json::from_str(json).expect("parse page");
        assert!(page.next.is_none());
    }
}

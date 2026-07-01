//! Archidekt collection provider.
//!
//! Archidekt's public collection API is paginated JSON at
//! `https://archidekt.com/api/collection/{id}/?page={n}` — 25 rows per page, with no
//! page-size override. Each row's `card.uid` is the Scryfall id (our
//! `cards.external_id`). The finish comes from the row's `modifier` string
//! ("Normal" / "Foil" / "Etched") — the sibling `foil` boolean is unreliable (often
//! left `false` even for a foil), so `modifier` is the primary signal and the boolean
//! is only a fallback (see [`is_foil_finish`]). The same printing can appear across
//! several rows (differing condition / language / tags / finish), so the caller
//! aggregates by `(uid, foil)`.
//!
//! We construct every request URL ourselves from the host constant plus a
//! digits-only collection id, so there is no SSRF surface (the id can carry neither a
//! host nor a path).

use std::time::Duration;

use reqwest::{StatusCode, header};
use serde::Deserialize;

use super::rate_limit::RateLimiter;
use super::{FetchedHolding, ImportError, MAX_IMPORT_ROWS};

const API_BASE: &str = "https://archidekt.com/api/collection";
const ACCEPT_JSON: &str = "application/json";
/// Archidekt's fixed collection page size.
const PAGE_SIZE: usize = 25;
/// Minimum wait after a `429` before retrying — the provider's rate window (one minute).
const RATE_LIMIT_BACKOFF_SECS: u64 = 60;
/// Ceiling on a single backoff, so a huge/hostile `Retry-After` can't stall an import.
const MAX_BACKOFF_SECS: u64 = 300;
/// Give up (fail the import) after this many `429`s, bounding the total added wait.
const MAX_RATE_LIMIT_RETRIES: u32 = 5;

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
    /// Legacy finish flag — unreliable (often `false` even for a foil), so only used as
    /// a fallback when `modifier` is absent. See [`is_foil_finish`].
    #[serde(default)]
    foil: bool,
    /// The finish label ("Normal" / "Foil" / "Etched"). This, not `foil`, is the real
    /// finish signal in practice; any non-"Normal" value is a foil finish.
    #[serde(default)]
    modifier: Option<String>,
    card: RowCard,
}

#[derive(Debug, Deserialize)]
struct RowCard {
    /// Scryfall card id — equals our `cards.external_id`.
    uid: String,
}

/// Decide whether a collection row is a foil finish.
///
/// Archidekt records the finish in two places: the `modifier` string
/// ("Normal" / "Foil" / "Etched") and a legacy `foil` boolean. Real collections leave
/// the boolean `false` even for foils, so `modifier` is the primary signal — any
/// non-empty, non-"Normal" modifier (foil, etched, or any other special finish) counts
/// as a foil in our two-bucket regular/foil model. The boolean is honored too, so a
/// foil is caught whichever field the provider populates; a card is treated as regular
/// only when *both* say so (modifier absent or "Normal", and the boolean `false`).
///
/// `pub(super)` so the CSV importer ([`super::csv_import`]) can key a CSV row's `Finish`
/// column off the exact same rule (a CSV has no `foil` boolean, so it passes `false`),
/// keeping the two Archidekt ingestion formats consistent on what counts as a foil.
pub(super) fn is_foil_finish(foil: bool, modifier: Option<&str>) -> bool {
    if foil {
        return true;
    }
    match modifier {
        Some(m) => {
            let m = m.trim();
            !m.is_empty() && !m.eq_ignore_ascii_case("normal")
        }
        None => false,
    }
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
    let mut rate_limit_retries = 0u32;
    let mut page = 1usize;

    while page <= max_pages {
        // Respect the provider's request cap across all imports before every request.
        limiter.acquire().await;

        let url = format!("{API_BASE}/{collection_id}/?page={page}");
        let response = http
            .get(&url)
            .header(header::ACCEPT, ACCEPT_JSON)
            .send()
            .await
            .map_err(|e| ImportError::Upstream(format!("request to Archidekt failed: {e}")))?;

        let status = response.status();

        // Rate-limited: back off (globally, so every import waits) and retry the *same*
        // page. Wait at least the provider's window (honoring a larger `Retry-After`);
        // give up after a few tries so a persistent 429 doesn't hang the import forever.
        if status == StatusCode::TOO_MANY_REQUESTS {
            if rate_limit_retries >= MAX_RATE_LIMIT_RETRIES {
                return Err(ImportError::RateLimited);
            }
            rate_limit_retries += 1;
            let wait = backoff_after(response.headers());
            tracing::warn!(
                page,
                wait_secs = wait.as_secs(),
                "Archidekt rate-limited us (429); backing off before retrying"
            );
            limiter.back_off(wait).await;
            continue; // retry the same page; the next `acquire()` waits out the backoff
        }

        // Archidekt answers a missing or private collection with `400 ["No public
        // collection found."]`; treat 400/404 alike as not-found. We follow the `next`
        // link and never request past the last page, so this only fires on page 1.
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
            let foil = is_foil_finish(row.foil, row.modifier.as_deref());
            holdings.push(FetchedHolding {
                external_card_id: row.card.uid,
                foil,
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
        page += 1;
    }

    Ok(holdings)
}

/// How long to back off after a `429`: the `Retry-After` seconds if the provider sent a
/// (numeric) one, otherwise the default window — always at least the window (per our
/// "wait a minute" policy) and never longer than [`MAX_BACKOFF_SECS`].
fn backoff_after(headers: &header::HeaderMap) -> Duration {
    let requested = headers
        .get(header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0);
    Duration::from_secs(requested.clamp(RATE_LIMIT_BACKOFF_SECS, MAX_BACKOFF_SECS))
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
        // Trimmed to the fields we consume, in Archidekt's real response shape. The
        // `modifier` carries the finish; a real collection leaves `foil` false even for
        // a foil (the first row), so `modifier` is what we key off.
        let json = r#"{
            "count": 3,
            "next": "http://archidekt.com/api/collection/1/?page=2",
            "results": [
                { "quantity": 2, "foil": false, "modifier": "Foil",   "card": { "uid": "aaa" } },
                { "quantity": 1, "foil": false, "modifier": "Normal", "card": { "uid": "bbb" } }
            ]
        }"#;
        let page: CollectionPage = serde_json::from_str(json).expect("parse page");
        assert_eq!(page.count, 3);
        assert_eq!(page.next.as_deref(), Some("http://archidekt.com/api/collection/1/?page=2"));
        assert_eq!(page.results.len(), 2);
        assert_eq!(page.results[0].card.uid, "aaa");
        assert_eq!(page.results[0].modifier.as_deref(), Some("Foil"));
        assert!(!page.results[0].foil, "the boolean is left false even for the foil row");
        assert_eq!(page.results[0].quantity, 2);
        assert_eq!(page.results[1].modifier.as_deref(), Some("Normal"));
    }

    #[test]
    fn missing_modifier_defaults_to_none() {
        // An older/renamed payload without `modifier` still parses; the finish then
        // falls back to the `foil` boolean.
        let json =
            r#"{ "count": 1, "next": null, "results": [ { "quantity": 1, "foil": true, "card": { "uid": "aaa" } } ] }"#;
        let page: CollectionPage = serde_json::from_str(json).expect("parse page");
        assert!(page.results[0].modifier.is_none());
        assert!(is_foil_finish(page.results[0].foil, page.results[0].modifier.as_deref()));
    }

    #[test]
    fn modifier_is_the_primary_finish_signal() {
        // The real-world case (issue #98): a foil whose boolean is false but whose
        // modifier says "Foil" must import as a foil, not a base card.
        assert!(is_foil_finish(false, Some("Foil")));
        // Etched (and any other non-"Normal" special finish) is a foil in our model.
        assert!(is_foil_finish(false, Some("Etched")));
        assert!(is_foil_finish(false, Some("Gilded")));
        // Matching is case-insensitive and tolerant of surrounding whitespace.
        assert!(is_foil_finish(false, Some("  foil ")));
        assert!(!is_foil_finish(false, Some("normal")));
        // "Normal" (or a blank/absent modifier) with a false boolean is a regular card.
        assert!(!is_foil_finish(false, Some("Normal")));
        assert!(!is_foil_finish(false, Some("")));
        assert!(!is_foil_finish(false, Some("   ")));
        assert!(!is_foil_finish(false, None));
        // The legacy boolean is still honored as a fallback when it's the one set.
        assert!(is_foil_finish(true, None));
        assert!(is_foil_finish(true, Some("Normal")));
    }

    #[test]
    fn backoff_honors_retry_after_within_bounds() {
        use reqwest::header::{HeaderMap, HeaderValue, RETRY_AFTER};
        let with = |v: &'static str| {
            let mut h = HeaderMap::new();
            h.insert(RETRY_AFTER, HeaderValue::from_static(v));
            h
        };
        // No / unparseable header -> the one-minute floor.
        assert_eq!(backoff_after(&HeaderMap::new()), Duration::from_secs(60));
        assert_eq!(backoff_after(&with("soon")), Duration::from_secs(60));
        // Below the floor is raised to a minute; within range is honored; huge is capped.
        assert_eq!(backoff_after(&with("30")), Duration::from_secs(60));
        assert_eq!(backoff_after(&with("120")), Duration::from_secs(120));
        assert_eq!(backoff_after(&with("100000")), Duration::from_secs(300));
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

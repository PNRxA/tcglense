//! Moxfield collection provider.
//!
//! Moxfield's (unofficial) collection API is paginated JSON at
//! `https://api2.moxfield.com/v1/collections/search/{id}` — `pageNumber` is 1-based,
//! `pageSize` up to 100, and the envelope carries `totalResults` / `totalPages` to page
//! by. Each row's `card.scryfall_id` (snake_case, unlike the row's own camelCase
//! fields) is the Scryfall id — our `cards.external_id`. The finish is the row's
//! `finish` string (`"nonFoil"` / `"foil"` / `"etched"`) with a redundant `isFoil`
//! boolean we use only as a fallback; `isProxy` rows are skipped (a proxy isn't a real
//! card). The same printing can span several rows (condition / language / binder), so
//! the caller aggregates by `(scryfall_id, foil)`. For a smart sync,
//! `sortType=lastUpdated&sortDirection=descending` pages most-recently-edited first —
//! the equivalent of Archidekt's `orderBy=-updatedAt`.
//!
//! **Access:** since late 2024 Moxfield fronts the API with bot protection that rejects
//! unknown clients; they approve a specific `User-Agent` string on request (email
//! support@moxfield.com). When `MOXFIELD_USER_AGENT` is configured we send it (treat it
//! as a credential); without one — or with an unapproved one — Moxfield answers `403`,
//! which we surface as a clear "needs an approved User-Agent" error rather than a
//! generic upstream failure. The API is unofficial and can change without notice.
//!
//! We construct every request URL ourselves from the host constant plus a
//! charset-validated collection id, so there is no SSRF surface (the id can carry
//! neither a host nor a path).

use std::collections::HashMap;
use std::time::Duration;

use reqwest::{StatusCode, header};
use serde::Deserialize;

use super::archidekt::backoff_after;
use super::{FetchedHolding, ImportError, MAX_IMPORT_ROWS, ProviderContext};

const API_BASE: &str = "https://api2.moxfield.com/v1/collections/search";
const ACCEPT_JSON: &str = "application/json";
/// Page size we request. Moxfield's own UI pages 50; 100 is accepted and halves the
/// request count (the response echoes the effective `pageSize`, but we page on
/// `totalPages`, so a silently-clamped size still terminates correctly).
const PAGE_SIZE: usize = 100;
/// Sort that puts the most-recently-edited rows first, for a smart sync (rows carry a
/// `lastUpdatedAtUtc`; this sorts by it, so a card whose count changed bubbles up).
const SORT_RECENT: &str = "sortType=lastUpdated&sortDirection=descending";
/// Give up (fail the import) after this many `429`s, bounding the total added wait.
/// (Backoff durations are shared with Archidekt via [`backoff_after`].)
const MAX_RATE_LIMIT_RETRIES: u32 = 5;
/// Overall per-request deadline. Moxfield's bot mitigation **tarpits** unapproved
/// clients — it drips bytes slowly enough that the client's per-read timeout never
/// fires (observed live: ~7 minutes for one page), which would otherwise let a single
/// import monopolise the one import slot indefinitely. A collection page is ~400 KB,
/// so a healthy fetch finishes in seconds; anything past this is the tarpit (or an
/// outage) and should fail the job instead of hanging it.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// Shortest / longest collection id we'll accept. Moxfield's public collection ids are
/// 22-char base64url tokens; the bounds leave headroom without accepting garbage.
const MIN_ID_LEN: usize = 10;
const MAX_ID_LEN: usize = 64;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CollectionPage {
    /// Total rows (distinct entries) across the whole collection — the size guard.
    #[serde(default)]
    total_results: usize,
    /// Total pages at the effective page size — what pagination terminates on. A
    /// missing/renamed field defaults to 0, which stops after the first page rather
    /// than looping forever.
    #[serde(default)]
    total_pages: usize,
    #[serde(default)]
    data: Vec<CollectionRow>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CollectionRow {
    #[serde(default)]
    quantity: i32,
    /// The finish label (`"nonFoil"` / `"foil"` / `"etched"`) — the primary signal.
    #[serde(default)]
    finish: Option<String>,
    /// Redundant foil boolean; used only when `finish` is absent.
    #[serde(default)]
    is_foil: bool,
    /// Proxies aren't real cards; those rows are skipped.
    #[serde(default)]
    is_proxy: bool,
    card: RowCard,
}

/// The row's card. NOT `rename_all = "camelCase"`: `scryfall_id` really is snake_case
/// in Moxfield's payload (unlike the row fields around it).
#[derive(Debug, Deserialize)]
struct RowCard {
    /// Scryfall card id — equals our `cards.external_id`. Absent on the odd custom
    /// card, so the row is skipped rather than failing the page.
    #[serde(default)]
    scryfall_id: Option<String>,
}

/// Decide whether a Moxfield row is a foil finish: any finish other than `"nonFoil"`
/// (i.e. `foil`, `etched`, or a future special finish) counts as foil in our two-bucket
/// model, mirroring the Archidekt rule. The `isFoil` boolean is only consulted when the
/// `finish` string is absent (older payloads).
fn is_foil_finish(finish: Option<&str>, is_foil: bool) -> bool {
    match finish.map(str::trim) {
        Some(f) if !f.is_empty() => !f.eq_ignore_ascii_case("nonfoil"),
        _ => is_foil,
    }
}

/// Extract the collection id from a user-supplied source: a full Moxfield collection
/// URL (`https://moxfield.com/collection/4xUdq-66IEKK6X53bhUS8Q`, with or without a
/// trailing slash, query, or fragment) or a bare id. Returns the id, or `None` if no
/// plausible id is present. A binder URL (`/binders/...`) is deliberately rejected —
/// binders live on a different endpoint we don't import yet.
pub fn parse_collection_id(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    // A bare id (no slashes): validate charset + length directly.
    if !trimmed.contains('/') {
        return valid_id(trimmed).then(|| trimmed.to_string());
    }
    // Otherwise treat it as a URL/path: drop the scheme and any query/fragment, then
    // take the path segment right after `collection`.
    let without_scheme = trimmed
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(trimmed);
    let path = without_scheme
        .split(['?', '#'])
        .next()
        .unwrap_or(without_scheme);
    let mut segments = path.split('/').filter(|seg| !seg.is_empty());
    segments
        .by_ref()
        .find(|seg| seg.eq_ignore_ascii_case("collection"))?;
    let id = segments.next()?;
    valid_id(id).then(|| id.to_string())
}

/// Whether a candidate collection id is plausible: base64url charset (letters, digits,
/// `-`, `_`) within the length bounds. This is what makes the constructed URL safe.
fn valid_id(id: &str) -> bool {
    (MIN_ID_LEN..=MAX_ID_LEN).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Fetch every holding for a Moxfield collection id, following pagination. Enforces
/// [`MAX_IMPORT_ROWS`] up front (via the first page's `totalResults`) so a huge public
/// collection can't make us fan out an unbounded number of upstream requests.
pub async fn fetch(
    ctx: &ProviderContext<'_>,
    collection_id: &str,
) -> Result<Vec<FetchedHolding>, ImportError> {
    // A hard page ceiling bounds the worst case even if the provider keeps reporting
    // more pages.
    let max_pages = MAX_IMPORT_ROWS / PAGE_SIZE + 1;

    let mut holdings: Vec<FetchedHolding> = Vec::new();
    let mut checked_size = false;
    // Cumulative across the whole import (not per page), so a provider that keeps
    // rate-limiting us fails fast rather than backing off once per page.
    let mut rate_limit_retries = 0u32;
    let mut page = 1usize;

    while page <= max_pages {
        let body = get_page(ctx, collection_id, page, false, &mut rate_limit_retries).await?;
        check_size(&body, &mut checked_size)?;

        if body.data.is_empty() {
            break;
        }
        let last_page = page >= body.total_pages;
        for row in body.data {
            let Some(holding) = row_to_holding(row) else {
                continue;
            };
            holdings.push(holding);
        }
        // `totalPages` is the provider's own "last page" signal; the row cap is
        // belt-and-braces on top.
        if last_page || holdings.len() >= MAX_IMPORT_ROWS {
            break;
        }
        page += 1;
    }

    Ok(holdings)
}

/// Fetch the recently-updated prefix of a Moxfield collection for a smart sync: page
/// most-recently-edited first ([`SORT_RECENT`]) and stop once a whole page already
/// matches `local` (`external id -> (regular, foil)`). Returns the fetched holdings plus
/// whether we stopped early (reached the already-synced tail) rather than paging the
/// whole collection. Same page ceiling / size guard / rate-limit handling as [`fetch`].
pub async fn fetch_smart(
    ctx: &ProviderContext<'_>,
    collection_id: &str,
    local: &HashMap<String, (i32, i32)>,
) -> Result<(Vec<FetchedHolding>, bool), ImportError> {
    let max_pages = MAX_IMPORT_ROWS / PAGE_SIZE + 1;

    let mut holdings: Vec<FetchedHolding> = Vec::new();
    let mut running: HashMap<String, (i64, i64)> = HashMap::new();
    let mut checked_size = false;
    let mut stopped_early = false;
    let mut rate_limit_retries = 0u32;
    let mut page = 1usize;

    while page <= max_pages {
        let body = get_page(ctx, collection_id, page, true, &mut rate_limit_retries).await?;
        check_size(&body, &mut checked_size)?;

        if body.data.is_empty() {
            break;
        }
        let last_page = page >= body.total_pages;
        let rows = body.data.into_iter().filter_map(|row| {
            let h = row_to_holding(row)?;
            Some((h.external_card_id, h.foil, h.quantity))
        });
        // Fold the page into the running aggregate; `all_match` is the stop signal.
        let all_match = super::smart_absorb_page(&mut running, &mut holdings, local, rows);

        if last_page || holdings.len() >= MAX_IMPORT_ROWS {
            break;
        }
        // A whole page already in sync means the rest (edited even longer ago) is too.
        if all_match {
            stopped_early = true;
            break;
        }
        page += 1;
    }

    Ok((holdings, stopped_early))
}

/// Shape a network-level failure (timeout, dropped connection, stalled body). With no
/// approved User-Agent configured, the by-far most likely cause is Moxfield's bot
/// mitigation slow-walking us (observed live: a page dripped over ~7 minutes before
/// dying), so say that actionably; with one configured, it's a genuine upstream
/// failure and stays a generic gateway error.
fn fetch_failure(ctx: &ProviderContext<'_>, detail: String) -> ImportError {
    if ctx.settings.moxfield_user_agent.is_none() {
        tracing::warn!(error = %detail, "Moxfield fetch failed without an approved User-Agent");
        return ImportError::ProviderDenied(
            "Moxfield didn't answer in time — their API throttles clients it hasn't \
             approved. The server operator must request an approved User-Agent from \
             Moxfield (email support@moxfield.com) and set it as MOXFIELD_USER_AGENT — \
             or import your collection as a CSV export instead."
                .to_string(),
        );
    }
    ImportError::Upstream(detail)
}

/// Normalize one provider row, or `None` for a row we skip (a proxy, or a card without
/// a Scryfall id).
fn row_to_holding(row: CollectionRow) -> Option<FetchedHolding> {
    if row.is_proxy {
        return None;
    }
    let external_card_id = row.card.scryfall_id?;
    let foil = is_foil_finish(row.finish.as_deref(), row.is_foil);
    Some(FetchedHolding {
        external_card_id,
        foil,
        quantity: row.quantity,
    })
}

/// Reject an over-large collection up front, from the first page's `totalResults`.
/// `checked` tracks whether this guard has already run (only the first page carries it).
fn check_size(body: &CollectionPage, checked: &mut bool) -> Result<(), ImportError> {
    if !*checked {
        if body.total_results > MAX_IMPORT_ROWS {
            return Err(ImportError::TooLarge {
                count: body.total_results,
                max: MAX_IMPORT_ROWS,
            });
        }
        *checked = true;
    }
    Ok(())
}

/// Fetch and parse one collection page, throttled by the shared limiter and transparently
/// retrying past a `429` (backing off the whole limiter so every import waits).
/// `order_recent` adds the smart sync's most-recently-edited-first sort. Sends the
/// configured approved `User-Agent` when present. Maps a missing/private collection
/// (`400`/`404`) to `CollectionNotFound`, and a `403` (Moxfield's bot wall rejecting our
/// client) to a clear "needs an approved User-Agent" error.
async fn get_page(
    ctx: &ProviderContext<'_>,
    collection_id: &str,
    page: usize,
    order_recent: bool,
    rate_limit_retries: &mut u32,
) -> Result<CollectionPage, ImportError> {
    loop {
        // Respect the provider's request cap across all imports before every request.
        ctx.limiter.acquire().await;

        let mut url = format!("{API_BASE}/{collection_id}?pageNumber={page}&pageSize={PAGE_SIZE}");
        if order_recent {
            url.push('&');
            url.push_str(SORT_RECENT);
        }
        let mut request = ctx
            .http
            .get(&url)
            .header(header::ACCEPT, ACCEPT_JSON)
            // Whole-request deadline (connect + headers + body) — see REQUEST_TIMEOUT.
            .timeout(REQUEST_TIMEOUT);
        if let Some(ua) = ctx.settings.moxfield_user_agent.as_deref() {
            request = request.header(header::USER_AGENT, ua);
        }
        let response = match request.send().await {
            Ok(r) => r,
            Err(e) => return Err(fetch_failure(ctx, format!("request to Moxfield failed: {e}"))),
        };

        let status = response.status();

        // Rate-limited: back off (globally, so every import waits) and retry the *same*
        // page; give up after a few tries so a persistent 429 doesn't hang the import.
        if status == StatusCode::TOO_MANY_REQUESTS {
            if *rate_limit_retries >= MAX_RATE_LIMIT_RETRIES {
                return Err(ImportError::RateLimited);
            }
            *rate_limit_retries += 1;
            let wait = backoff_after(response.headers());
            tracing::warn!(
                page,
                wait_secs = wait.as_secs(),
                "Moxfield rate-limited us (429); backing off before retrying"
            );
            ctx.limiter.back_off(wait).await;
            continue; // retry the same page; the next `acquire()` waits out the backoff
        }

        // Moxfield's bot protection turns away clients whose User-Agent isn't on their
        // allow-list — a deployment configuration issue, not a transient failure, so
        // say so instead of a generic "provider unreachable".
        if status == StatusCode::FORBIDDEN {
            return Err(ImportError::ProviderDenied(
                "Moxfield declined the request: their API only serves approved clients. \
                 The server operator must request an approved User-Agent from Moxfield \
                 (email support@moxfield.com) and set it as MOXFIELD_USER_AGENT — or \
                 import your collection as a CSV export instead."
                    .to_string(),
            ));
        }

        // A missing or private collection.
        if status == StatusCode::BAD_REQUEST || status == StatusCode::NOT_FOUND {
            return Err(ImportError::CollectionNotFound(collection_id.to_string()));
        }
        let response = response
            .error_for_status()
            .map_err(|e| ImportError::Upstream(format!("Moxfield returned an error: {e}")))?;
        // Read then parse in two steps (rather than `.json()`) so a connection dropped
        // mid-body reads as a transfer failure, distinct from a shape mismatch — the
        // serde error then carries the offending field path for the log.
        let body = match response.text().await {
            Ok(b) => b,
            Err(e) => {
                return Err(fetch_failure(
                    ctx,
                    format!("couldn't read the Moxfield response: {e}"),
                ));
            }
        };
        return serde_json::from_str(&body).map_err(|e| {
            ImportError::Upstream(format!("couldn't parse the Moxfield response: {e}"))
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "4xUdq-66IEKK6X53bhUS8Q";

    #[test]
    fn parses_bare_id() {
        assert_eq!(parse_collection_id(ID).as_deref(), Some(ID));
        assert_eq!(parse_collection_id(&format!("  {ID}  ")).as_deref(), Some(ID));
        // Underscores are part of the base64url charset.
        assert_eq!(parse_collection_id("abc_DEF-01234").as_deref(), Some("abc_DEF-01234"));
    }

    #[test]
    fn parses_url_forms() {
        assert_eq!(
            parse_collection_id(&format!("https://moxfield.com/collection/{ID}")).as_deref(),
            Some(ID)
        );
        assert_eq!(
            parse_collection_id(&format!("https://www.moxfield.com/collection/{ID}/")).as_deref(),
            Some(ID)
        );
        assert_eq!(
            parse_collection_id(&format!("moxfield.com/collection/{ID}?foo=bar#frag")).as_deref(),
            Some(ID)
        );
    }

    #[test]
    fn rejects_sources_without_a_plausible_id() {
        assert_eq!(parse_collection_id(""), None);
        assert_eq!(parse_collection_id("   "), None);
        assert_eq!(parse_collection_id("https://moxfield.com/collection/"), None);
        // Too short / bad charset.
        assert_eq!(parse_collection_id("abc"), None);
        assert_eq!(parse_collection_id("https://moxfield.com/collection/a b c d e f"), None);
        // A deck or binder URL isn't a collection (binders are a different endpoint).
        assert_eq!(parse_collection_id("https://moxfield.com/decks/aBcDeFgHiJkLmNo"), None);
        assert_eq!(parse_collection_id("https://moxfield.com/binders/aBcDeFgHiJkLmNo"), None);
        // An id long enough but with an invalid character.
        assert_eq!(parse_collection_id("abcdefghij!lmnop"), None);
    }

    #[test]
    fn deserializes_a_collection_page() {
        // Trimmed to the fields we consume, in Moxfield's real response shape: camelCase
        // row fields but a snake_case `scryfall_id` on the nested card.
        let json = r#"{
            "totalResults": 3, "totalPages": 2, "pageNumber": 1, "pageSize": 2,
            "data": [
                { "quantity": 1, "finish": "foil", "isFoil": true, "isProxy": false,
                  "card": { "id": "LRdeq", "scryfall_id": "1e8a43c1-42d1-45ef-8a63-4b87775a6e88", "set": "rna", "cn": "151" } },
                { "quantity": 2, "finish": "nonFoil", "isFoil": false, "isProxy": false,
                  "card": { "id": "aaAaa", "scryfall_id": "f369827d-e4cd-4bc7-8c5e-72882eff0908" } }
            ]
        }"#;
        let page: CollectionPage = serde_json::from_str(json).expect("parse page");
        assert_eq!(page.total_results, 3);
        assert_eq!(page.total_pages, 2);
        assert_eq!(page.data.len(), 2);
        assert_eq!(
            page.data[0].card.scryfall_id.as_deref(),
            Some("1e8a43c1-42d1-45ef-8a63-4b87775a6e88")
        );
        assert_eq!(page.data[0].finish.as_deref(), Some("foil"));
        assert_eq!(page.data[1].quantity, 2);
    }

    #[test]
    fn rows_normalize_with_finish_as_the_primary_signal() {
        let row = |finish: Option<&str>, is_foil: bool, is_proxy: bool| CollectionRow {
            quantity: 1,
            finish: finish.map(str::to_string),
            is_foil,
            is_proxy,
            card: RowCard {
                scryfall_id: Some("uid".to_string()),
            },
        };
        // The finish string wins over the boolean.
        assert!(row_to_holding(row(Some("foil"), false, false)).expect("kept").foil);
        assert!(row_to_holding(row(Some("etched"), false, false)).expect("kept").foil);
        assert!(!row_to_holding(row(Some("nonFoil"), true, false)).expect("kept").foil);
        // Absent finish falls back to the boolean.
        assert!(row_to_holding(row(None, true, false)).expect("kept").foil);
        assert!(!row_to_holding(row(None, false, false)).expect("kept").foil);
        // Proxies are dropped entirely.
        assert!(row_to_holding(row(Some("nonFoil"), false, true)).is_none());
    }

    #[test]
    fn rows_without_a_scryfall_id_are_skipped() {
        let row = CollectionRow {
            quantity: 1,
            finish: None,
            is_foil: false,
            is_proxy: false,
            card: RowCard { scryfall_id: None },
        };
        assert!(row_to_holding(row).is_none());
    }

    #[test]
    fn over_large_collections_are_rejected_from_the_first_page() {
        let page = CollectionPage {
            total_results: MAX_IMPORT_ROWS + 1,
            total_pages: 9999,
            data: vec![],
        };
        let mut checked = false;
        let err = check_size(&page, &mut checked).expect_err("too large");
        assert!(matches!(err, ImportError::TooLarge { .. }));
    }
}

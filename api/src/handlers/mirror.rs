//! Dataset **mirror**: re-serve the raw provider datasets so other TCGLense instances
//! can pull them from here instead of hammering the upstream services.
//!
//! By default a self-host reads the big dataset files (Scryfall's `default_cards` bulk
//! file + set list, MTGJSON's `AllPrintings.json.gz`, TCGCSV's catalog / prices /
//! archives) from a mirror rather than from the upstreams (see [`crate::datasets`]). The
//! public site runs these endpoints so it is that mirror: each handler streams the
//! corresponding file straight from the upstream service on demand and re-serves it with
//! **CDN-cacheable** headers, so a fronting CDN absorbs the repeat downloads and the
//! upstream is hit at most about once per cache period. Nothing is persisted on disk —
//! the same fetch-and-serve posture as the image proxy's [`CDN_MODE`](crate::config::Config::cdn_mode).
//!
//! Serving is gated on [`Config::mirror_enabled`](crate::config::Config) (off by default,
//! so an ordinary self-host isn't an open proxy to the upstreams). The routes are wired
//! only when it is set; see [`crate::router`].
//!
//! **Consistency note.** The bulk *file* endpoints are given a strictly shorter shared
//! TTL than the *catalog* endpoint that advertises the version, so a consumer can never
//! read a fresh `updated_at` while the CDN still holds a stale file (which it would then
//! stamp as up-to-date and never re-fetch). See [`MIRROR_FILE_CACHE`].

use axum::{
    Json,
    body::Body,
    extract::State,
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{ACCEPT, CACHE_CONTROL, CONTENT_TYPE, ETAG, IF_NONE_MATCH, USER_AGENT},
    },
    response::{IntoResponse, Response},
};

use crate::{catalog::fingerprint_sync, error::AppError, extract::Path, state::AppState};

/// `Cache-Control` for the small mirror **metadata** (the Scryfall bulk-data catalog and
/// set list, and every TCGCSV JSON / `last-updated.txt`). Shared-cacheable for an hour —
/// these change at most daily — served stale-while-revalidate so a CDN miss never blocks.
const MIRROR_META_CACHE: &str = "public, max-age=300, s-maxage=3600, stale-while-revalidate=86400";

/// `Cache-Control` for the big dataset **file** (the streamed Scryfall bulk card file).
///
/// Deliberately a **shorter** shared TTL (`s-maxage`) than [`MIRROR_META_CACHE`]: the
/// catalog advertises an `updated_at` the consumer version-gates on, so the file must
/// never be *staler* than the catalog. With a shorter file TTL, whenever the catalog
/// re-fetches a new version the file cache has already expired and is re-fetched with it
/// — otherwise a consumer could pair a new `updated_at` with a stale file, import the old
/// data, stamp it as current, and never pull the real new file.
const MIRROR_FILE_CACHE: &str = "public, max-age=300, s-maxage=1800, stale-while-revalidate=86400";

/// `Cache-Control` for TCGCSV's dated price **archives** (`archive/tcgplayer/prices-{date}.ppmd.7z`).
///
/// Deliberately a **much longer** shared TTL than [`MIRROR_META_CACHE`], and the only
/// mirror route that is `immutable`: an archive is a *dated* daily snapshot, fixed once
/// published — unlike the JSON endpoints next to it, which re-describe a moving catalog
/// and must expire. (TCGCSV re-stamped its whole back-catalog once, in 2024-11, so
/// "immutable" is a property of the data, not a promise from upstream; a CDN purge is the
/// escape hatch if it ever happens again.)
///
/// The short meta TTL is actively wrong here. The one-time historic price backfill
/// ([`crate::tcgcsv::backfill`]) walks ~900 archive days, and each day is a **distinct URL
/// fetched exactly once** per consumer — so within one backfill there is no repeat request
/// for a shared cache to serve, and across consumers a one-hour TTL has long expired by
/// the time the next self-host backfills. The mirror therefore re-fetches every archive
/// from TCGCSV for every self-host that runs the walk, under the mirror's own User-Agent
/// and IP (see [`tcgcsv_proxy`]), spending TCGCSV's request budget on data that never
/// changed. A year of `immutable` lets the CDN absorb the repeats instead.
///
/// Safe against pinning a *missing* day: a day TCGCSV has not published is a `404`, which
/// [`crate::handlers::cache::public_cache_layer`] stamps `no-store`, so this header is only
/// ever attached to an archive that actually exists.
const MIRROR_ARCHIVE_CACHE: &str = "public, max-age=31536000, immutable";

/// `Cache-Control` for MTGJSON's `AllPrintings.json.gz`. That file is version-gated on
/// its HTTP `ETag`, so the win is the conditional `304` (the consumer only re-ingests a
/// changed file), not caching the ~160 MB body. Tell shared caches to revalidate every
/// time so the `If-None-Match` is forwarded and the upstream `304` is relayed through.
const MIRROR_REVALIDATE_CACHE: &str = "public, max-age=0, must-revalidate";

/// Whether `kind` is a safe bulk-dataset slug (defence in depth — it selects a catalog
/// entry, never touches the filesystem): non-empty, lowercase ASCII + underscores only.
fn is_safe_dataset_kind(kind: &str) -> bool {
    !kind.is_empty() && kind.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
}

/// Validate + normalise an arbitrary TCGCSV sub-path (from the `{*path}` capture) so it
/// can only ever address a resource *under* `https://tcgcsv.com/` — no host escape, no
/// traversal. Every `/`-separated segment must be non-empty and match `[A-Za-z0-9._-]+`
/// (which excludes `.`/`..` dot-segments, `:`, and empty segments that could form `//`
/// or a scheme). Returns the cleaned path (identical to the input when valid).
fn sanitize_tcgcsv_path(path: &str) -> Option<String> {
    let segments: Vec<&str> = path.split('/').collect();
    for seg in &segments {
        if seg.is_empty()
            || *seg == "."
            || *seg == ".."
            || !seg
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        {
            return None;
        }
    }
    Some(segments.join("/"))
}

/// Whether a (already-sanitised) TCGCSV sub-path addresses the dated price archives
/// rather than the live JSON catalog. Everything under `archive/` is a per-day snapshot
/// keyed by date in its filename, so it is immutable once published and earns
/// [`MIRROR_ARCHIVE_CACHE`]; every other path re-describes a moving catalog and keeps
/// [`MIRROR_META_CACHE`]. A bare `archive` (a directory listing) is not a snapshot, so
/// the trailing slash is required.
fn is_tcgcsv_archive_path(path: &str) -> bool {
    path.starts_with("archive/")
}

/// Pick the `Cache-Control` for a (sanitised) TCGCSV sub-path: a dated archive is an
/// immutable snapshot ([`MIRROR_ARCHIVE_CACHE`]); everything else re-describes a moving
/// catalog and keeps [`MIRROR_META_CACHE`].
///
/// Kept as a pure function — mirroring [`crate::handlers::cache::public_cache_value`] —
/// so the policy is unit-testable without an upstream to fetch from. Inverting it would
/// pin the live catalog for a year, which no route-level test would catch.
fn tcgcsv_cache_control(path: &str) -> &'static str {
    if is_tcgcsv_archive_path(path) {
        MIRROR_ARCHIVE_CACHE
    } else {
        MIRROR_META_CACHE
    }
}

/// Map a request-path upstream failure to a `502`, tagged with which mirror hop failed.
/// The raw upstream error (`reqwest`/`IngestError` `Display` — which can carry the
/// upstream URL, connection/TLS detail, or JSON-decode internals) is logged, never sent:
/// the client sees only the static `context` tag.
fn bad_gateway(context: &str, err: impl std::fmt::Display) -> AppError {
    tracing::warn!(context, error = %err, "mirror upstream request failed");
    AppError::BadGateway(format!("mirror: {context}: upstream request failed"))
}

/// Stream an upstream `GET` through as the response, forwarding the upstream
/// `Content-Type` (+ `ETag`) and stamping `cache_control`. The body streams, so memory
/// stays bounded regardless of file size. A `304` (MTGJSON conditional) is relayed
/// bodyless with its `ETag`; a `404` (a missing TCGCSV archive day) is relayed as `404`
/// so the consumer treats it as "no archive"; any other non-success is a `502`.
async fn proxy_stream(
    state: &AppState,
    context: &str,
    url: &str,
    user_agent: Option<&str>,
    if_none_match: Option<&str>,
    cache_control: &'static str,
) -> Result<Response, AppError> {
    let mut request = state.http.get(url);
    if let Some(ua) = user_agent {
        request = request.header(USER_AGENT, ua);
    }
    if let Some(inm) = if_none_match {
        request = request.header(IF_NONE_MATCH, inm);
    }
    let upstream = request
        .send()
        .await
        .map_err(|err| bad_gateway(context, err))?;
    let status = upstream.status();

    // Relay a conditional 304 bodyless, carrying the ETag + our cache policy.
    if status == StatusCode::NOT_MODIFIED {
        let mut response = Response::new(Body::empty());
        *response.status_mut() = StatusCode::NOT_MODIFIED;
        if let Some(etag) = upstream.headers().get(ETAG).cloned() {
            response.headers_mut().insert(ETAG, etag);
        }
        response
            .headers_mut()
            .insert(CACHE_CONTROL, HeaderValue::from_static(cache_control));
        return Ok(response);
    }
    // A missing upstream resource (e.g. a day with no TCGCSV archive) stays a 404 so the
    // consumer's own not-found handling kicks in, rather than a misleading 502.
    if status == StatusCode::NOT_FOUND {
        return Err(AppError::NotFound(format!(
            "mirror: {context}: upstream 404"
        )));
    }
    let upstream = upstream
        .error_for_status()
        .map_err(|err| bad_gateway(context, err))?;

    // Capture the headers worth forwarding before the response is consumed into a stream.
    let content_type = upstream.headers().get(CONTENT_TYPE).cloned();
    let etag = upstream.headers().get(ETAG).cloned();

    let mut response = Response::new(Body::from_stream(upstream.bytes_stream()));
    let headers = response.headers_mut();
    if let Some(ct) = content_type {
        headers.insert(CONTENT_TYPE, ct);
    }
    if let Some(tag) = etag {
        headers.insert(ETAG, tag);
    }
    headers.insert(CACHE_CONTROL, HeaderValue::from_static(cache_control));
    Ok(response)
}

/// `GET /api/mirror/scryfall/bulk-data` — the Scryfall bulk-data catalog (small JSON
/// describing each downloadable file). The consumer reads `updated_at`/`size` from it and
/// builds the file URL from the mirror, so the embedded upstream `download_uri` is
/// re-served verbatim but never followed by a mirror consumer.
pub async fn scryfall_bulk_data(State(state): State<AppState>) -> Result<Response, AppError> {
    proxy_stream(
        &state,
        "scryfall bulk-data",
        crate::scryfall::BULK_DATA_URL,
        None,
        None,
        MIRROR_META_CACHE,
    )
    .await
}

/// `GET /api/mirror/scryfall/sets` — the full Scryfall set list, every upstream page
/// folded into one `{ has_more: false, data: [...] }` so the consumer's pagination loop
/// terminates after a single request. The set objects are passed through untouched.
pub async fn scryfall_sets(State(state): State<AppState>) -> Result<Response, AppError> {
    let mut data: Vec<serde_json::Value> = Vec::new();
    let mut url = crate::scryfall::SETS_URL.to_string();
    loop {
        let page: serde_json::Value = state
            .http
            .get(&url)
            .header(ACCEPT, "application/json")
            .send()
            .await
            .map_err(|err| bad_gateway("scryfall sets", err))?
            .error_for_status()
            .map_err(|err| bad_gateway("scryfall sets", err))?
            .json()
            .await
            .map_err(|err| bad_gateway("scryfall sets", err))?;
        if let Some(arr) = page.get("data").and_then(|v| v.as_array()) {
            data.extend(arr.iter().cloned());
        }
        match (
            page.get("has_more")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            page.get("next_page").and_then(|v| v.as_str()),
        ) {
            (true, Some(next)) => url = next.to_string(),
            _ => break,
        }
    }
    let body = serde_json::json!({ "object": "list", "has_more": false, "data": data });
    Ok((
        [(CACHE_CONTROL, HeaderValue::from_static(MIRROR_META_CACHE))],
        Json(body),
    )
        .into_response())
}

/// `GET /api/mirror/scryfall/file/{kind}` — stream the current bulk file for `kind`
/// (e.g. `default_cards`). Resolves the live download URL from the catalog, then streams
/// its bytes through (bounded memory).
pub async fn scryfall_file(
    Path(kind): Path<String>,
    State(state): State<AppState>,
) -> Result<Response, AppError> {
    if !is_safe_dataset_kind(&kind) {
        return Err(AppError::NotFound(
            "mirror: unknown bulk dataset".to_string(),
        ));
    }
    let entry = crate::scryfall::client::bulk_data(&state.http, crate::scryfall::BULK_DATA_URL)
        .await
        .map_err(|err| bad_gateway("scryfall catalog", err))?
        .into_iter()
        .find(|b| b.kind == kind)
        .ok_or_else(|| AppError::NotFound(format!("mirror: bulk dataset '{kind}' not found")))?;
    proxy_stream(
        &state,
        "scryfall file",
        &entry.download_uri,
        None,
        None,
        MIRROR_FILE_CACHE,
    )
    .await
}

/// `GET /api/mirror/scryfall/sld-drops` — the current Secret Lair drop snapshot (curated titles +
/// collector numbers) as JSON. Served from this origin's in-memory drop store — the daily Scryfall
/// gallery scrape ([`crate::scryfall::sld_scrape`]), or the committed fallback before the first
/// scrape — so other TCGLense instances import it daily ([`crate::scryfall::sld_sync`]) instead of
/// each scraping Scryfall. Those titles aren't in the bulk card API, so this is the only
/// machine-readable source other instances have.
///
/// Unlike the dataset proxies above this touches **no upstream**: it re-serves this origin's own
/// snapshot. Version-gated by a strong content `ETag`, so a consumer whose snapshot is current gets
/// a bodyless `304`; the snapshot changes at most daily, so it's shared-cacheable like the other
/// mirror metadata.
pub async fn scryfall_sld_drops(headers: HeaderMap) -> Result<Response, AppError> {
    use crate::scryfall::drops;
    // Read the JSON body and its version from ONE store snapshot, so the ETag and the body always
    // describe the same snapshot even if a concurrent daily install swaps the store between reads.
    let (json, version) = drops::current_snapshot();
    let etag = format!("\"sld-{version}\"");
    // Provably ASCII (`"sld-<hex>"`); map the impossible failure to a 500 rather than panic.
    let etag_value = HeaderValue::from_str(&etag)
        .map_err(|_| AppError::Internal("sld-drops etag not header-safe".to_string()))?;

    // Conditional request: an unchanged snapshot is a cheap bodyless 304 carrying the ETag.
    if let Some(inm) = headers.get(IF_NONE_MATCH).and_then(|v| v.to_str().ok()) {
        if inm == etag {
            let mut response = Response::new(Body::empty());
            *response.status_mut() = StatusCode::NOT_MODIFIED;
            let out = response.headers_mut();
            out.insert(ETAG, etag_value);
            out.insert(CACHE_CONTROL, HeaderValue::from_static(MIRROR_META_CACHE));
            return Ok(response);
        }
    }

    let mut response = Response::new(Body::from(json));
    let out = response.headers_mut();
    out.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    out.insert(ETAG, etag_value);
    out.insert(CACHE_CONTROL, HeaderValue::from_static(MIRROR_META_CACHE));
    Ok(response)
}

/// `GET /api/mirror/mtgjson/AllPrintings.json.gz` — MTGJSON's sealed-contents dump,
/// forwarding `If-None-Match`/`ETag` so the consumer's ETag version-gate (its cheap
/// unchanged-file `304`) still works through the mirror.
pub async fn mtgjson_all_printings(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Result<Response, AppError> {
    let if_none_match = headers.get(IF_NONE_MATCH).and_then(|v| v.to_str().ok());
    let url = format!("{}/AllPrintings.json.gz", crate::mtgjson::BASE_URL);
    proxy_stream(
        &state,
        "mtgjson AllPrintings",
        &url,
        None,
        if_none_match,
        MIRROR_REVALIDATE_CACHE,
    )
    .await
}

/// `GET /api/mirror/tcgcsv/{*path}` — proxy an arbitrary TCGCSV path (`last-updated.txt`,
/// `tcgplayer/{cat}/groups`, `.../products`, `.../prices`, `archive/...`), host-locked
/// and path-sanitised. Carries the configured TCGCSV User-Agent, since TCGCSV blocks
/// generic ones.
///
/// The dated archives are cached `immutable` for a year ([`MIRROR_ARCHIVE_CACHE`]); the
/// live catalog paths keep the short [`MIRROR_META_CACHE`].
pub async fn tcgcsv_proxy(
    Path(path): Path<String>,
    State(state): State<AppState>,
) -> Result<Response, AppError> {
    let clean = sanitize_tcgcsv_path(&path)
        .ok_or_else(|| AppError::NotFound("mirror: invalid tcgcsv path".to_string()))?;
    let cache_control = tcgcsv_cache_control(&clean);
    let url = format!("{}/{clean}", crate::tcgcsv::BASE_URL);
    proxy_stream(
        &state,
        "tcgcsv",
        &url,
        Some(state.config.tcgcsv_user_agent.as_str()),
        None,
        cache_control,
    )
    .await
}

/// `GET /api/mirror/fingerprints/{game}` — the visual-scanner match index for `game`,
/// serialized as a compact binary payload (see [`crate::catalog::fingerprint_sync`]) so
/// other TCGLense instances import the finished index instead of fetching + hashing every
/// card image themselves (the whole point of the opt-in operator build).
///
/// Unlike the dataset proxies above this touches **no upstream**: it serializes this
/// origin's own in-memory index. It is version-gated by a strong content `ETag`, so a
/// consumer whose index is current gets a bodyless `304`. The payload changes only when
/// the origin rebuilds (about daily), so it is shared-cacheable like the other metadata.
pub async fn fingerprint_index(
    Path(game): Path<String>,
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Result<Response, AppError> {
    let algo_version = state.config.fingerprint_algo_version;
    let index = state.fingerprint_index();
    let entries = index.export_entries(&game);
    let body = fingerprint_sync::serialize(algo_version, &entries);
    let etag = fingerprint_sync::etag(&body);
    // Provably ASCII (`"fp-<hex>"`); map the impossible failure to a 500 rather than panic.
    let etag_value = HeaderValue::from_str(&etag)
        .map_err(|_| AppError::Internal("fingerprint etag not header-safe".to_string()))?;

    // Conditional request: an unchanged index is a cheap bodyless 304 carrying the ETag.
    if let Some(inm) = headers.get(IF_NONE_MATCH).and_then(|v| v.to_str().ok()) {
        if inm == etag {
            let mut response = Response::new(Body::empty());
            *response.status_mut() = StatusCode::NOT_MODIFIED;
            let out = response.headers_mut();
            out.insert(ETAG, etag_value);
            out.insert(CACHE_CONTROL, HeaderValue::from_static(MIRROR_META_CACHE));
            return Ok(response);
        }
    }

    let mut response = Response::new(Body::from(body));
    let out = response.headers_mut();
    out.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    out.insert(ETAG, etag_value);
    out.insert(CACHE_CONTROL, HeaderValue::from_static(MIRROR_META_CACHE));
    Ok(response)
}

/// `GET /api/mirror/currency` — the daily USD reference-rate (FX) feed, proxied from the
/// upstream provider ([`crate::currency::RATES_URL`], Frankfurter) and re-served with
/// CDN-cacheable headers, so mirror consumers pull exchange rates from this origin instead of
/// contacting the provider (their [`crate::currency::CurrencyRates`] points at this route — see
/// [`crate::currency::CurrencyRates::from_config`]).
///
/// A proxy passthrough like the TCGCSV / MTGJSON routes above — the body is the provider's JSON
/// **verbatim**, so the consumer's existing parser reads it unchanged. The feed needs no special
/// User-Agent, and it changes at most daily, so it shares the metadata TTL ([`MIRROR_META_CACHE`]).
pub async fn currency_proxy(State(state): State<AppState>) -> Result<Response, AppError> {
    proxy_stream(
        &state,
        "currency",
        crate::currency::RATES_URL,
        None,
        None,
        MIRROR_META_CACHE,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dataset_kind_slug_is_strict() {
        assert!(is_safe_dataset_kind("default_cards"));
        assert!(is_safe_dataset_kind("oracle_cards"));
        assert!(!is_safe_dataset_kind(""));
        assert!(!is_safe_dataset_kind("Default")); // uppercase
        assert!(!is_safe_dataset_kind("../etc")); // traversal chars
        assert!(!is_safe_dataset_kind("a b")); // space
        assert!(!is_safe_dataset_kind("all-cards")); // hyphen not allowed here
    }

    #[test]
    fn tcgcsv_path_accepts_the_real_endpoints() {
        for p in [
            "last-updated.txt",
            "tcgplayer/1/groups",
            "tcgplayer/1/2649/products",
            "tcgplayer/1/2649/prices",
            "archive/tcgplayer/prices-2024-02-08.ppmd.7z",
        ] {
            assert_eq!(
                sanitize_tcgcsv_path(p).as_deref(),
                Some(p),
                "should accept {p}"
            );
        }
    }

    #[test]
    fn only_dated_archives_are_immutable() {
        // The backfill's archive days: immutable, so the CDN can absorb the ~900-day walk.
        assert!(is_tcgcsv_archive_path(
            "archive/tcgplayer/prices-2024-02-08.ppmd.7z"
        ));
        // The live catalog paths re-describe a moving catalog: never immutable.
        for p in [
            "last-updated.txt",
            "tcgplayer/1/groups",
            "tcgplayer/1/2649/products",
            "tcgplayer/1/2649/prices",
        ] {
            assert!(!is_tcgcsv_archive_path(p), "{p} must not be immutable");
        }
        // A bare listing isn't a dated snapshot, and a look-alike prefix isn't the
        // archive tree.
        assert!(!is_tcgcsv_archive_path("archive"));
        assert!(!is_tcgcsv_archive_path("archived/prices.7z"));
    }

    #[test]
    fn tcgcsv_cache_control_maps_archives_to_immutable_and_catalog_to_meta() {
        // The mapping, not just the predicate: inverting the arms would pin the live,
        // moving catalog for a year, so assert the header each path actually earns.
        assert_eq!(
            tcgcsv_cache_control("archive/tcgplayer/prices-2024-02-08.ppmd.7z"),
            MIRROR_ARCHIVE_CACHE
        );
        for p in [
            "last-updated.txt",
            "tcgplayer/1/groups",
            "tcgplayer/1/2649/products",
            "tcgplayer/1/2649/prices",
            "archive",
            "archived/prices.7z",
        ] {
            assert_eq!(
                tcgcsv_cache_control(p),
                MIRROR_META_CACHE,
                "{p} must not be immutable"
            );
        }
    }

    #[test]
    fn archive_cache_outlives_the_meta_cache_and_is_immutable() {
        /// Pull `directive=<seconds>` out of a `Cache-Control` value.
        fn ttl(cache_control: &str, directive: &str) -> u64 {
            cache_control
                .split(',')
                .map(str::trim)
                .find_map(|d| d.strip_prefix(&format!("{directive}=")))
                .and_then(|v| v.parse().ok())
                .unwrap_or_else(|| panic!("{cache_control} has no {directive}"))
        }
        // The point of the split: an archive must never inherit the short meta TTL that
        // makes the mirror re-fetch every day of the ~900-day walk from TCGCSV.
        assert!(ttl(MIRROR_ARCHIVE_CACHE, "max-age") > ttl(MIRROR_META_CACHE, "s-maxage"));
        assert!(MIRROR_ARCHIVE_CACHE.contains("immutable"));
        assert!(!MIRROR_META_CACHE.contains("immutable"));
    }

    #[test]
    fn tcgcsv_path_rejects_traversal_and_host_escape() {
        // Dot-segments, empty segments (`//`), backslashes, and anything that could
        // change the host or scheme are refused.
        for p in [
            "..",
            "../secret",
            "tcgplayer/../../etc/passwd",
            "tcgplayer//groups", // empty segment
            "",                  // empty capture
            "a b/groups",        // space
            "http:/evil.com",    // colon
            "tcgplayer/@evil",   // stray symbol
        ] {
            assert_eq!(sanitize_tcgcsv_path(p), None, "should reject {p:?}");
        }
    }
}

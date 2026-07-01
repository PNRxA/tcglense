//! DB-backed XML sitemaps advertising the public card catalog to crawlers.
//!
//! A sitemap **index** (`GET /api/sitemap.xml`) points at child sitemaps for the
//! static/landing pages, every set, and the cards (chunked, since a single sitemap
//! is capped at 50 000 URLs / 50 MB). The `<loc>`s are the SPA's own routes
//! (e.g. `/cards/mtg/sets/blb`), built against the configured public site origin
//! ([`crate::config::Config::public_site_url`]) — not the API's `/api/...` URLs.
//! See issue #75.
//!
//! The sitemaps are served under `/api/` because that is the only path routed to
//! the backend in both the dev Vite proxy and the recommended same-origin
//! production deploy; `robots.txt` (emitted by the web build) points crawlers at
//! `/api/sitemap.xml`. A shared cache may store them (they change at most daily,
//! with the sync), so each carries a long [`SITEMAP_CACHE_CONTROL`].

use axum::{
    extract::{Path, State},
    http::header,
    response::{IntoResponse, Response},
};
use sea_orm::{EntityTrait, PaginatorTrait, QueryOrder, QuerySelect};

use crate::catalog;
use crate::entities::prelude::{Card, CardSet, IngestState};
use crate::entities::{card, card_set, ingest_state};
use crate::error::AppError;
use crate::state::AppState;

/// Protocol cap on URLs per sitemap (50 000). Cards are split into child sitemaps
/// of at most this many URLs each.
const MAX_URLS_PER_SITEMAP: u64 = 50_000;

/// `Content-Type` for a sitemap document.
const SITEMAP_CONTENT_TYPE: &str = "application/xml; charset=utf-8";

/// `Cache-Control` for sitemap responses. Longer-lived than the catalog default
/// because a sitemap is comparatively expensive to build (the card sitemaps scan
/// the whole `cards` table) and the underlying data turns over at most once a day
/// (the sync): a shared cache keeps it fresh for a day and may serve it stale for a
/// week while it refreshes, so crawlers hitting the sitemap almost never reach the
/// origin. Set by the handler so the public cache layer preserves it (it only fills
/// in a missing header), and errors still fall through to `no-store`.
pub const SITEMAP_CACHE_CONTROL: &str =
    "public, max-age=3600, s-maxage=86400, stale-while-revalidate=604800";

// ---------- Handlers ----------

/// `GET /api/sitemap.xml` -> the sitemap **index**: pointers to the `pages`, `sets`,
/// and per-chunk `cards` child sitemaps. Each entry carries a `<lastmod>` of the
/// latest card-data sync so crawlers can tell when the catalog last changed.
pub async fn sitemap_index(State(state): State<AppState>) -> Result<Response, AppError> {
    let base = &state.config.public_site_url;
    let lastmod = latest_ingest_lastmod(&state.db).await?;
    let chunks = card_chunk_count(&state.db).await?;

    let mut body = String::new();
    push_sitemap(&mut body, &format!("{base}/api/sitemaps/pages.xml"), lastmod.as_deref());
    push_sitemap(&mut body, &format!("{base}/api/sitemaps/sets.xml"), lastmod.as_deref());
    // Card chunks are 1-based; zero cards means no card sitemaps at all.
    for n in 1..=chunks {
        push_sitemap(&mut body, &format!("{base}/api/sitemaps/cards-{n}.xml"), lastmod.as_deref());
    }

    Ok(xml_response(sitemapindex(body)))
}

/// `GET /api/sitemaps/{name}` -> one child sitemap. Dispatches on the filename so a
/// single route covers `pages.xml`, `sets.xml`, and `cards-{n}.xml` (matchit-safe:
/// one whole-segment param). Any other name — including an out-of-range card chunk —
/// is a `404`.
pub async fn sitemap_child(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Response, AppError> {
    let base = &state.config.public_site_url;
    match name.as_str() {
        "pages.xml" => Ok(xml_response(urlset(pages_body(base)))),
        "sets.xml" => sets_sitemap(&state, base).await,
        other => match parse_card_chunk(other) {
            Some(n) => cards_sitemap(&state, base, n).await,
            None => Err(AppError::NotFound(format!("unknown sitemap '{name}'"))),
        },
    }
}

// ---------- Child sitemap bodies ----------

/// The evergreen landing pages plus each game's hub and all-cards browse view.
/// Games are a static registry, so this needs no database.
fn pages_body(base: &str) -> String {
    let mut body = String::new();
    push_url(&mut body, &format!("{base}/"), None);
    push_url(&mut body, &format!("{base}/cards"), None);
    for game in catalog::GAMES {
        push_url(&mut body, &format!("{base}/cards/{}", game.id), None);
        push_url(&mut body, &format!("{base}/cards/{}/cards", game.id), None);
    }
    body
}

/// Every set's detail page, across all games. Sets are bounded (hundreds to low
/// thousands per game), comfortably under [`MAX_URLS_PER_SITEMAP`], so they fit in
/// one file. `<lastmod>` is the set's release date. Only the three columns the URL
/// needs are selected so the whole `card_sets` table isn't materialised.
async fn sets_sitemap(state: &AppState, base: &str) -> Result<Response, AppError> {
    let rows: Vec<(String, String, Option<String>)> = CardSet::find()
        .select_only()
        .column(card_set::Column::Game)
        .column(card_set::Column::Code)
        .column(card_set::Column::ReleasedAt)
        .order_by_asc(card_set::Column::Id)
        .into_tuple()
        .all(&state.db)
        .await?;

    let mut body = String::new();
    for (game, code, released_at) in rows {
        push_url(&mut body, &format!("{base}/cards/{game}/sets/{code}"), released_at.as_deref());
    }
    Ok(xml_response(urlset(body)))
}

/// One chunk of card detail pages (`n` is 1-based). Cards are ordered by the stable
/// primary key and windowed with `OFFSET`/`LIMIT`, so chunk boundaries stay put
/// across requests. `<lastmod>` is the card's release date. A chunk past the end
/// (or any request when there are no cards) is a `404` rather than an empty
/// document, so a stale index entry reads as a clear miss. Only the three columns
/// the URL needs are selected.
async fn cards_sitemap(state: &AppState, base: &str, n: u64) -> Result<Response, AppError> {
    // saturating so an absurd chunk number can't overflow the offset (it just
    // windows past the end and 404s below).
    let offset = n.saturating_sub(1).saturating_mul(MAX_URLS_PER_SITEMAP);
    let rows: Vec<(String, String, Option<String>)> = Card::find()
        .select_only()
        .column(card::Column::Game)
        .column(card::Column::ExternalId)
        .column(card::Column::ReleasedAt)
        .order_by_asc(card::Column::Id)
        .offset(offset)
        .limit(MAX_URLS_PER_SITEMAP)
        .into_tuple()
        .all(&state.db)
        .await?;

    if rows.is_empty() {
        return Err(AppError::NotFound(format!("sitemap card chunk {n} is out of range")));
    }

    let mut body = String::new();
    for (game, external_id, released_at) in rows {
        push_url(
            &mut body,
            &format!("{base}/cards/{game}/cards/{external_id}"),
            released_at.as_deref(),
        );
    }
    Ok(xml_response(urlset(body)))
}

// ---------- Data helpers ----------

/// Number of card child sitemaps needed to cover every card, at
/// [`MAX_URLS_PER_SITEMAP`] URLs each. Zero cards -> zero chunks.
async fn card_chunk_count(db: &sea_orm::DatabaseConnection) -> Result<u64, AppError> {
    let total = Card::find().count(db).await?;
    Ok(chunk_count(total))
}

/// Pure chunk-count arithmetic, split out so it is unit-testable without a DB.
fn chunk_count(total: u64) -> u64 {
    total.div_ceil(MAX_URLS_PER_SITEMAP)
}

/// The most recent card-data sync completion across games, as an RFC 3339 string,
/// or `None` if nothing has finished importing yet. Used as the sitemaps'
/// `<lastmod>`. Ordered descending with NULLs last (SQLite parks NULL below any
/// value in `DESC`), so the first row is the greatest real `finished_at` if any.
async fn latest_ingest_lastmod(
    db: &sea_orm::DatabaseConnection,
) -> Result<Option<String>, AppError> {
    let row = IngestState::find()
        .order_by_desc(ingest_state::Column::FinishedAt)
        .one(db)
        .await?;
    Ok(row.and_then(|r| r.finished_at).map(|t| t.to_rfc3339()))
}

/// Parse a card child-sitemap filename (`cards-<n>.xml`) into its 1-based chunk
/// number, or `None` if it isn't that shape (so the caller 404s). Rejects a
/// non-positive or non-numeric index.
fn parse_card_chunk(name: &str) -> Option<u64> {
    name.strip_prefix("cards-")
        .and_then(|rest| rest.strip_suffix(".xml"))
        .and_then(|n| n.parse::<u64>().ok())
        .filter(|n| *n >= 1)
}

// ---------- XML building ----------

/// Wrap a sitemap document body with the XML prolog and a `<urlset>` element.
fn urlset(body: String) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n{body}</urlset>\n"
    )
}

/// Wrap a sitemap-index body with the XML prolog and a `<sitemapindex>` element.
fn sitemapindex(body: String) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <sitemapindex xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n{body}\
         </sitemapindex>\n"
    )
}

/// Append one `<url>` entry (optionally with `<lastmod>`) to a `<urlset>` body.
/// `loc` is a fully-formed absolute URL; `lastmod` a W3C-datetime / `YYYY-MM-DD`.
fn push_url(out: &mut String, loc: &str, lastmod: Option<&str>) {
    out.push_str("  <url><loc>");
    out.push_str(&xml_escape(loc));
    out.push_str("</loc>");
    if let Some(lastmod) = lastmod {
        out.push_str("<lastmod>");
        out.push_str(&xml_escape(lastmod));
        out.push_str("</lastmod>");
    }
    out.push_str("</url>\n");
}

/// Append one `<sitemap>` entry (optionally with `<lastmod>`) to a `<sitemapindex>`
/// body. `loc` is the absolute URL of a child sitemap file.
fn push_sitemap(out: &mut String, loc: &str, lastmod: Option<&str>) {
    out.push_str("  <sitemap><loc>");
    out.push_str(&xml_escape(loc));
    out.push_str("</loc>");
    if let Some(lastmod) = lastmod {
        out.push_str("<lastmod>");
        out.push_str(&xml_escape(lastmod));
        out.push_str("</lastmod>");
    }
    out.push_str("</sitemap>\n");
}

/// Escape the five XML predefined entities so a `<loc>`/`<lastmod>` value is always
/// well-formed. Game slugs, set codes and card ids (UUIDs) don't contain these
/// today, but escaping keeps the document valid regardless of what the data holds.
fn xml_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Build a sitemap HTTP response: the XML body with its content type and the long
/// [`SITEMAP_CACHE_CONTROL`] (preserved by the public cache layer).
fn xml_response(body: String) -> Response {
    (
        [
            (header::CONTENT_TYPE, SITEMAP_CONTENT_TYPE),
            (header::CACHE_CONTROL, SITEMAP_CACHE_CONTROL),
        ],
        body,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_count_rounds_up_and_handles_empty() {
        assert_eq!(chunk_count(0), 0);
        assert_eq!(chunk_count(1), 1);
        assert_eq!(chunk_count(MAX_URLS_PER_SITEMAP), 1);
        assert_eq!(chunk_count(MAX_URLS_PER_SITEMAP + 1), 2);
        assert_eq!(chunk_count(MAX_URLS_PER_SITEMAP * 3), 3);
    }

    #[test]
    fn parse_card_chunk_accepts_valid_and_rejects_the_rest() {
        assert_eq!(parse_card_chunk("cards-1.xml"), Some(1));
        assert_eq!(parse_card_chunk("cards-42.xml"), Some(42));
        // Wrong prefix/suffix, non-numeric, zero, or the sibling filenames.
        assert_eq!(parse_card_chunk("cards-0.xml"), None);
        assert_eq!(parse_card_chunk("cards-.xml"), None);
        assert_eq!(parse_card_chunk("cards-1"), None);
        assert_eq!(parse_card_chunk("cards-1.json"), None);
        assert_eq!(parse_card_chunk("cards-abc.xml"), None);
        assert_eq!(parse_card_chunk("pages.xml"), None);
        assert_eq!(parse_card_chunk("sets.xml"), None);
    }

    #[test]
    fn xml_escape_escapes_the_five_entities() {
        assert_eq!(xml_escape("a&b<c>d\"e'f"), "a&amp;b&lt;c&gt;d&quot;e&apos;f");
        assert_eq!(xml_escape("no-special-chars"), "no-special-chars");
    }

    #[test]
    fn push_url_emits_loc_with_optional_lastmod() {
        let mut out = String::new();
        push_url(&mut out, "https://x.test/cards/mtg", Some("2024-01-02"));
        assert_eq!(
            out,
            "  <url><loc>https://x.test/cards/mtg</loc><lastmod>2024-01-02</lastmod></url>\n"
        );

        let mut bare = String::new();
        push_url(&mut bare, "https://x.test/", None);
        assert_eq!(bare, "  <url><loc>https://x.test/</loc></url>\n");
    }

    #[test]
    fn pages_body_covers_static_and_every_game() {
        let body = pages_body("https://x.test");
        assert!(body.contains("<loc>https://x.test/</loc>"));
        assert!(body.contains("<loc>https://x.test/cards</loc>"));
        // Every registered game contributes a hub + browse URL.
        for game in catalog::GAMES {
            assert!(body.contains(&format!("<loc>https://x.test/cards/{}</loc>", game.id)));
            assert!(body.contains(&format!("<loc>https://x.test/cards/{}/cards</loc>", game.id)));
        }
    }
}

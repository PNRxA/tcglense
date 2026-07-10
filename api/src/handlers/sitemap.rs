//! DB-backed XML sitemaps advertising the public catalog to crawlers.
//!
//! A sitemap **index** (`GET /sitemap.xml`) points at child sitemaps for the
//! static/landing pages, every set, the cards, and the sealed products (both
//! chunked — see [`MAX_URLS_PER_SITEMAP`]). The `<loc>`s are the SPA's own routes
//! (e.g. `/cards/mtg/sets/blb`), built against the configured public site origin
//! ([`crate::config::Config::public_site_url`]) — not the API's `/api/...` URLs.
//! See issues #75 and #294.
//!
//! The sitemaps live at the site root (`/sitemap.xml`, `/sitemaps/...`) so the
//! strict sitemap-protocol scope rule ("a sitemap may only claim URLs under its own
//! directory") holds for every crawler; `robots.txt` (emitted by the web build)
//! points crawlers there. The same handlers also answer under `/api/` so the URLs
//! already submitted to search consoles keep working. A shared cache may store them
//! (they change at most daily, with the sync), so each carries a long
//! [`SITEMAP_CACHE_CONTROL`].

use axum::{
    extract::State,
    http::header,
    response::{IntoResponse, Response},
};
use sea_orm::{
    ColumnTrait, EntityTrait, Order, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect,
    sea_query::NullOrdering,
};

use crate::catalog;
use crate::entities::prelude::{Card, CardSet, IngestState, Product};
use crate::entities::{card, card_set, ingest_state, product};
use crate::error::AppError;
use crate::extract::Path;
use crate::state::AppState;

/// URLs per child sitemap. The protocol allows 50 000 / 50 MB, but Google timed out
/// fetching our 50 000-URL card chunks (issue #294) — each was a multi-megabyte
/// document built from a single large OFFSET window on a modest origin. We dropped to
/// 10 000, then to 5 000 (issue #318) for still-smaller documents that build and
/// transfer even faster; the index grows by a few dozen more entries, which is nothing
/// (an index may hold 50 000 sitemaps).
const MAX_URLS_PER_SITEMAP: u64 = 5_000;

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

/// `GET /sitemap.xml` (and `/api/sitemap.xml`) -> the sitemap **index**: pointers to
/// the `pages`, `sets`, per-chunk `cards`, and per-chunk `products` child sitemaps.
/// Each entry carries a `<lastmod>` of the latest card-data sync so crawlers can
/// tell when the catalog last changed. Children are always advertised at the root
/// `/sitemaps/...` path, whichever alias served the index.
pub async fn sitemap_index(State(state): State<AppState>) -> Result<Response, AppError> {
    let base = &state.config.public_site_url;
    let lastmod = latest_ingest_lastmod(&state.db).await?;
    let card_chunks = chunk_count(Card::find().count(&state.db).await?);
    let product_chunks = chunk_count(Product::find().count(&state.db).await?);

    let mut body = String::new();
    push_sitemap(
        &mut body,
        &format!("{base}/sitemaps/pages.xml"),
        lastmod.as_deref(),
    );
    push_sitemap(
        &mut body,
        &format!("{base}/sitemaps/sets.xml"),
        lastmod.as_deref(),
    );
    // Chunks are 1-based; zero rows means no child sitemaps of that kind at all.
    for n in 1..=card_chunks {
        push_sitemap(
            &mut body,
            &format!("{base}/sitemaps/cards-{n}.xml"),
            lastmod.as_deref(),
        );
    }
    for n in 1..=product_chunks {
        push_sitemap(
            &mut body,
            &format!("{base}/sitemaps/products-{n}.xml"),
            lastmod.as_deref(),
        );
    }

    Ok(xml_response(sitemapindex(body)))
}

/// `GET /sitemaps/{name}` (and `/api/sitemaps/{name}`) -> one child sitemap.
/// Dispatches on the filename so a single route covers `pages.xml`, `sets.xml`,
/// `cards-{n}.xml`, and `products-{n}.xml` (matchit-safe: one whole-segment param).
/// Any other name — including an out-of-range chunk — is a `404`.
pub async fn sitemap_child(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Response, AppError> {
    let base = &state.config.public_site_url;
    match name.as_str() {
        "pages.xml" => Ok(xml_response(urlset(pages_body(base)))),
        "sets.xml" => sets_sitemap(&state, base).await,
        other => {
            if let Some(n) = parse_chunk(other, "cards") {
                cards_sitemap(&state, base, n).await
            } else if let Some(n) = parse_chunk(other, "products") {
                products_sitemap(&state, base, n).await
            } else {
                Err(AppError::NotFound(format!("unknown sitemap '{name}'")))
            }
        }
    }
}

// ---------- Child sitemap bodies ----------

/// The evergreen landing pages (home, legal), the cards + sealed hubs, and each
/// game's card hub/browse and sealed browse views. Games are a static registry, so
/// this needs no database.
fn pages_body(base: &str) -> String {
    let mut body = String::new();
    push_url(&mut body, &format!("{base}/"), None);
    push_url(&mut body, &format!("{base}/cards"), None);
    push_url(&mut body, &format!("{base}/sealed"), None);
    for game in catalog::GAMES {
        push_url(&mut body, &format!("{base}/cards/{}", game.id), None);
        push_url(&mut body, &format!("{base}/cards/{}/cards", game.id), None);
        push_url(&mut body, &format!("{base}/sealed/{}", game.id), None);
    }
    push_url(&mut body, &format!("{base}/docs"), None);
    push_url(&mut body, &format!("{base}/terms"), None);
    push_url(&mut body, &format!("{base}/privacy"), None);
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
        push_url(
            &mut body,
            &format!("{base}/cards/{game}/sets/{code}"),
            released_at.as_deref(),
        );
    }
    Ok(xml_response(urlset(body)))
}

/// One chunk of card detail pages (`n` is 1-based). Cards are ordered by the stable
/// primary key so chunk boundaries stay put across requests. `<lastmod>` is the
/// card's release date. A chunk past the end (or any request when there are no
/// cards) is a `404` rather than an empty document, so a stale index entry reads as
/// a clear miss. Only the three columns the URL needs are selected.
///
/// The payload window is taken by **keyset** (seek), not `OFFSET`: a plain `OFFSET`
/// made the database scan and discard every row *before* the window while dragging the
/// wide, non-indexed columns through each one, so later chunks got slower and slower
/// (issue #334 — a card chunk taking >1.5 s in prod). Instead we resolve the chunk's
/// first primary key with a light id-only lookup ([`chunk_start_id`]) and range-scan
/// forward with `id >= start`, so the payload reads exactly its 5 000-row window with
/// no rows scanned and thrown away — the part that dominated the slow query.
async fn cards_sitemap(state: &AppState, base: &str, n: u64) -> Result<Response, AppError> {
    let Some(start_id) = chunk_start_id(&state.db, Card::find(), card::Column::Id, n).await? else {
        return Err(AppError::NotFound(format!(
            "sitemap card chunk {n} is out of range"
        )));
    };
    let rows: Vec<(String, String, Option<String>)> = Card::find()
        .select_only()
        .column(card::Column::Game)
        .column(card::Column::ExternalId)
        .column(card::Column::ReleasedAt)
        .filter(card::Column::Id.gte(start_id))
        .order_by_asc(card::Column::Id)
        .limit(MAX_URLS_PER_SITEMAP)
        .into_tuple()
        .all(&state.db)
        .await?;

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

/// One chunk of sealed-product detail pages (`n` is 1-based) — `cards_sitemap`'s
/// twin over the `products` table (issue #294). Same stable keyset window and
/// 404-past-the-end behavior; the SPA's product route is `/sealed/{game}/{external_id}`.
async fn products_sitemap(state: &AppState, base: &str, n: u64) -> Result<Response, AppError> {
    let Some(start_id) =
        chunk_start_id(&state.db, Product::find(), product::Column::Id, n).await?
    else {
        return Err(AppError::NotFound(format!(
            "sitemap product chunk {n} is out of range"
        )));
    };
    let rows: Vec<(String, String, Option<String>)> = Product::find()
        .select_only()
        .column(product::Column::Game)
        .column(product::Column::ExternalId)
        .column(product::Column::ReleasedAt)
        .filter(product::Column::Id.gte(start_id))
        .order_by_asc(product::Column::Id)
        .limit(MAX_URLS_PER_SITEMAP)
        .into_tuple()
        .all(&state.db)
        .await?;

    let mut body = String::new();
    for (game, external_id, released_at) in rows {
        push_url(
            &mut body,
            &format!("{base}/sealed/{game}/{external_id}"),
            released_at.as_deref(),
        );
    }
    Ok(xml_response(urlset(body)))
}

// ---------- Data helpers ----------

/// Pure chunk-count arithmetic, split out so it is unit-testable without a DB.
fn chunk_count(total: u64) -> u64 {
    total.div_ceil(MAX_URLS_PER_SITEMAP)
}

/// The primary key of the first row in chunk `n` (1-based), or `None` when the chunk
/// starts past the last row (an out-of-range chunk, which the caller turns into a
/// `404`). This is the seek anchor for the keyset windowing in `cards_sitemap` /
/// `products_sitemap`. It still uses `OFFSET`, so it is not free — but it selects only
/// the narrow `id` column (an index-only skip of PK tuples on Postgres; a rowid-only
/// walk on SQLite, where `id` is the rowid), far cheaper than the old payload `OFFSET`
/// that pulled the wide `game`/`external_id`/`released_at` row through every discarded
/// entry. The caller then range-scans forward from the returned id with no `OFFSET` at
/// all — that payload seek is the actual fix for issue #334.
async fn chunk_start_id<E>(
    db: &sea_orm::DatabaseConnection,
    query: sea_orm::Select<E>,
    id_col: <E as EntityTrait>::Column,
    n: u64,
) -> Result<Option<i32>, AppError>
where
    E: EntityTrait,
{
    // Saturating so an absurd chunk number can't overflow the offset; the lookup just
    // seeks past the end and returns None.
    let offset = n.saturating_sub(1).saturating_mul(MAX_URLS_PER_SITEMAP);
    let start_id: Option<i32> = query
        .select_only()
        .column(id_col)
        .order_by_asc(id_col)
        .offset(offset)
        .limit(1)
        .into_tuple()
        .one(db)
        .await?;
    Ok(start_id)
}

/// The most recent card-data sync completion across games, as an RFC 3339 string,
/// or `None` if nothing has finished importing yet. Used as the sitemaps'
/// `<lastmod>`. Ordered descending with an explicit `NULLS LAST` so the first row is
/// the greatest real `finished_at` if any — a no-op on SQLite (its DESC already parks
/// NULL last) and correct on Postgres (which otherwise puts NULLs first under DESC).
async fn latest_ingest_lastmod(
    db: &sea_orm::DatabaseConnection,
) -> Result<Option<String>, AppError> {
    let row = IngestState::find()
        .order_by_with_nulls(
            ingest_state::Column::FinishedAt,
            Order::Desc,
            NullOrdering::Last,
        )
        .one(db)
        .await?;
    Ok(row.and_then(|r| r.finished_at).map(|t| t.to_rfc3339()))
}

/// Parse a chunked child-sitemap filename (`<prefix>-<n>.xml`) into its 1-based
/// chunk number, or `None` if it isn't that shape (so the caller 404s). Rejects a
/// non-positive or non-numeric index.
fn parse_chunk(name: &str, prefix: &str) -> Option<u64> {
    name.strip_prefix(prefix)
        .and_then(|rest| rest.strip_prefix('-'))
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
    fn parse_chunk_accepts_valid_and_rejects_the_rest() {
        assert_eq!(parse_chunk("cards-1.xml", "cards"), Some(1));
        assert_eq!(parse_chunk("cards-42.xml", "cards"), Some(42));
        assert_eq!(parse_chunk("products-1.xml", "products"), Some(1));
        // Wrong prefix/suffix, non-numeric, zero, or the sibling filenames.
        assert_eq!(parse_chunk("cards-0.xml", "cards"), None);
        assert_eq!(parse_chunk("cards-.xml", "cards"), None);
        assert_eq!(parse_chunk("cards-1", "cards"), None);
        assert_eq!(parse_chunk("cards-1.json", "cards"), None);
        assert_eq!(parse_chunk("cards-abc.xml", "cards"), None);
        assert_eq!(parse_chunk("products-1.xml", "cards"), None);
        assert_eq!(parse_chunk("cards-1.xml", "products"), None);
        assert_eq!(parse_chunk("pages.xml", "cards"), None);
        assert_eq!(parse_chunk("sets.xml", "cards"), None);
    }

    #[test]
    fn xml_escape_escapes_the_five_entities() {
        assert_eq!(
            xml_escape("a&b<c>d\"e'f"),
            "a&amp;b&lt;c&gt;d&quot;e&apos;f"
        );
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
        assert!(body.contains("<loc>https://x.test/sealed</loc>"));
        assert!(body.contains("<loc>https://x.test/docs</loc>"));
        assert!(body.contains("<loc>https://x.test/terms</loc>"));
        assert!(body.contains("<loc>https://x.test/privacy</loc>"));
        // Every registered game contributes a card hub + browse URL and a sealed
        // browse URL.
        for game in catalog::GAMES {
            assert!(body.contains(&format!("<loc>https://x.test/cards/{}</loc>", game.id)));
            assert!(body.contains(&format!(
                "<loc>https://x.test/cards/{}/cards</loc>",
                game.id
            )));
            assert!(body.contains(&format!("<loc>https://x.test/sealed/{}</loc>", game.id)));
        }
    }
}

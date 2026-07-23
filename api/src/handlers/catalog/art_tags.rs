//! Art-tag lookup endpoint: powers the advanced-search panel's art-tag autocomplete
//! and its "browse all tags" dialog (issue #140). Tag data comes from the ingested
//! `art_tags` metadata table (`crate::scryfall::art_tags`); the search filter itself
//! (`art:`) is compiled by `crate::scryfall::search` against `card_art_tags`.

use axum::{Json, extract::State};
use sea_orm::{ColumnTrait, EntityTrait, Order, QueryFilter, QueryOrder, QuerySelect};
use serde::{Deserialize, Serialize};

use crate::entities::art_tag;
use crate::entities::prelude::ArtTag;
use crate::error::AppError;
use crate::extract::{Path, Query};
use crate::handlers::shared::{DataBody, require_game, trim_query};
use crate::scryfall::search::{cust_vals, escape_like};
use crate::state::AppState;

/// Default / max number of tag suggestions a `q` lookup returns. The max is higher
/// than the card-name autocomplete's: the tag browser's filter box also queries with
/// `q` and wants a fuller page of matches.
const DEFAULT_TAG_SUGGESTIONS: u64 = 10;
const MAX_TAG_SUGGESTIONS: u64 = 50;

/// One art tag: a community Tagger label describing what a card's artwork depicts.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct ArtTagEntry {
    /// Canonical slug — the value the `art:` search filter matches (e.g. `squirrel`).
    pub slug: String,
    /// Human-readable name (e.g. `Squirrel`).
    pub label: String,
    /// Distinct artworks in the catalog matching this tag (hierarchy-expanded, so a
    /// parent tag counts its descendants' artworks too).
    pub count: i32,
    /// Optional community description of what the tag represents.
    pub description: Option<String>,
}

/// Query params for the art-tag lookup endpoint.
#[derive(Debug, Deserialize)]
pub struct ArtTagParams {
    /// Substring to match tag slugs/labels against (case-insensitively). Absent/blank
    /// returns the game's **full** tag list — the tag-browser payload.
    #[serde(default)]
    pub q: Option<String>,
    /// How many suggestions a `q` lookup returns, clamped to `[1, MAX_TAG_SUGGESTIONS]`.
    /// Absent = `DEFAULT_TAG_SUGGESTIONS`. Ignored without `q` (the full list has a
    /// natural bound: the tag vocabulary itself).
    #[serde(default)]
    pub limit: Option<u64>,
}

/// List art tags
///
/// `GET /api/games/{game}/art-tags?q=&limit=` -> art tags usable with the `art:`
/// search filter. With `q`: up to `limit` tags whose slug or label contains `q`
/// (case-insensitively), starts-with matches first, then by how many artworks match,
/// for the advanced-search autocomplete. Without `q`: the game's full tag list
/// ordered by slug — the tag-browser payload (a few thousand entries, ETag/CDN
/// cacheable). Tags whose artworks we don't store are absent (their count would be 0).
#[utoipa::path(
    get,
    path = "/api/games/{game}/art-tags",
    tag = "Cards",
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("q" = Option<String>, Query, description = "Substring to match tag slugs/labels against (case-insensitive); blank/absent returns the full tag list"),
        ("limit" = Option<u64>, Query, description = "Max suggestions for a `q` lookup (clamped to [1, 50]); absent = 10; ignored without `q`"),
    ),
    responses(
        (status = 200, description = "Matching art tags (with `q`), or the game's full tag list (without).", body = DataBody<Vec<ArtTagEntry>>),
        (status = 404, description = "Unknown game."),
    ),
)]
pub async fn list_art_tags(
    State(state): State<AppState>,
    Path(game): Path<String>,
    Query(params): Query<ArtTagParams>,
) -> Result<Json<DataBody<Vec<ArtTagEntry>>>, AppError> {
    require_game(&game)?;
    let mut query = ArtTag::find().filter(art_tag::Column::Game.eq(game.as_str()));

    match trim_query(params.q.as_deref()) {
        // No search term: the whole vocabulary, slug-ordered, for the tag browser.
        None => query = query.order_by_asc(art_tag::Column::Slug),
        Some(term) => {
            let limit = params
                .limit
                .unwrap_or(DEFAULT_TAG_SUGGESTIONS)
                .clamp(1, MAX_TAG_SUGGESTIONS);
            // Case-insensitive literal substring over slug OR label, metacharacters
            // escaped (same posture as the card-name autocomplete). `to_ascii_lowercase`
            // matches SQLite's ASCII `LOWER()`; the table is ~10k narrow rows, so a scan
            // is fine on both backends.
            let escaped = escape_like(term).to_ascii_lowercase();
            let dialect = state.dialect();
            let matches = |pattern: &str| {
                cust_vals(
                    dialect,
                    "LOWER(slug) LIKE ? ESCAPE '\\'",
                    [pattern.to_string()],
                )
                .or(cust_vals(
                    dialect,
                    "LOWER(label) LIKE ? ESCAPE '\\'",
                    [pattern.to_string()],
                ))
            };
            // Boolean sort key: DESC puts matches first on both backends (Postgres
            // orders true > false; SQLite orders 1 > 0).
            let starts_with = matches(&format!("{escaped}%"));
            query = query
                .filter(matches(&format!("%{escaped}%")))
                .order_by(starts_with, Order::Desc)
                .order_by_desc(art_tag::Column::TaggingsCount)
                .order_by_asc(art_tag::Column::Slug)
                .limit(limit);
        }
    }

    let data = query
        .select_only()
        .column(art_tag::Column::Slug)
        .column(art_tag::Column::Label)
        .column(art_tag::Column::TaggingsCount)
        .column(art_tag::Column::Description)
        .into_tuple::<(String, String, i32, Option<String>)>()
        .all(&state.db)
        .await?
        .into_iter()
        .map(|(slug, label, count, description)| ArtTagEntry {
            slug,
            label,
            count,
            description,
        })
        .collect();

    Ok(Json(DataBody { data }))
}

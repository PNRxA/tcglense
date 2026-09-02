//! The universal search (`GET /api/games/{game}/search`): one query, answered across the
//! catalog at once — cards, sealed products, preconstructed decks and the rules-keyword
//! glossary — as the top few matches of each kind. The homepage search box's backend, and
//! the one call a CLI needs to answer "what does TCGLense know about *this*".
//!
//! **Composition, not a fifth search.** Each leg is the surface's own name rule, reached
//! through the seam that surface already exposes: cards through
//! [`crate::handlers::catalog::search_cards`] (the listing's base query, folded one row per
//! name), sealed products through [`crate::handlers::catalog::search_products`], precons
//! through [`crate::handlers::precons::search_precons`] (the browse's own filter builder),
//! keywords through [`crate::catalog::keywords::search`]. All four match the way the sealed
//! and precon listings always have — every whitespace-separated word as an order-independent,
//! case-insensitive name substring (`handlers::shared::every_word_matches`) — so "commander
//! tarkir" means the same thing in every group; and all four rank a name that *starts* with
//! the text above one that merely contains it (`starts_with_rank`), the autocomplete's rule.
//! A grammar the card listing understands (`t:goblin`) is deliberately **not** applied here:
//! a universal box is typed into by name, and a colon in a card name (`Elspeth, Sun's
//! Champion`, `Krark's Thumb`) must never turn into a 422 for every group at once. The full
//! Scryfall grammar is one click away on the card listing this box hands off to.
//!
//! **Per-user data stays out.** The response is the same for every visitor, which is what
//! lets it sit in the router's public, ETag + CDN-cached catalog group and be rate-limited
//! per IP like the autocomplete. The SPA adds the signed-in user's own decks client-side,
//! from the deck list it already holds — a public read must not vary by session.
//!
//! **Bounded on purpose.** Each group is cut at `limit` (default 5, at most 10) and answers
//! `has_more` from one row of over-fetch, never a `COUNT(*)` — counting every card whose
//! name contains two letters, on every keystroke, would cost more than the answer.

use axum::{Json, extract::State};
use serde::Serialize;

use crate::catalog::keywords::{self, KeywordEntry};
use crate::error::AppError;
use crate::extract::{Path, Query};
use crate::handlers::catalog::{NameSuggestParams, search_cards, search_products};
use crate::handlers::precons::{PreconDeckResponse, search_precons};
use crate::handlers::shared::{
    CardResponse, ProductResponse, SearchGroup, require_game, set_name_map, trim_query,
};
use crate::state::AppState;

/// Matches per group when the caller names no `limit`.
pub(crate) const DEFAULT_SEARCH_LIMIT: u64 = 5;
/// The most matches per group a caller may ask for.
pub(crate) const MAX_SEARCH_LIMIT: u64 = 10;

/// Everything the catalog knows that matches one query, grouped by kind. Every group
/// carries the same wire shape its own listing does (`Card`, `Product`, `PreconDeck`,
/// `KeywordEntry`), so a client renders a hit with the component it already has and can
/// open it with the link it already builds.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct SearchResults {
    /// Distinct card names, each as one representative printing's full card payload.
    pub cards: SearchGroup<CardResponse>,
    /// Sealed products (boxes, bundles, decks) by name.
    pub products: SearchGroup<ProductResponse>,
    /// Preconstructed decks by name.
    pub precons: SearchGroup<PreconDeckResponse>,
    /// Rules keywords by name — never by reminder text.
    pub keywords: SearchGroup<KeywordEntry>,
}

impl SearchResults {
    /// What a blank query answers: every group empty, nothing withheld.
    fn empty() -> Self {
        SearchResults {
            cards: SearchGroup::empty(),
            products: SearchGroup::empty(),
            precons: SearchGroup::empty(),
            keywords: SearchGroup::empty(),
        }
    }
}

/// Search the catalog
///
/// `GET /api/games/{game}/search?q=&limit=` -> the top `limit` cards (one per distinct
/// name), sealed products, preconstructed decks and rules keywords whose name contains
/// every word of `q`, each group flagging whether more matched. Names that start with `q`
/// lead each group. A blank `q` answers empty groups; an unknown game is a `404`; a `q` of
/// more than 32 words is a `422`.
#[utoipa::path(
    get,
    path = "/api/games/{game}/search",
    tag = "Search",
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("q" = Option<String>, Query, description = "Text to match names against: every whitespace-separated word must appear (case-insensitive, any order). Blank/absent yields empty groups"),
        ("limit" = Option<u64>, Query, description = "Max matches per group (clamped to [1, 10]); absent = 5"),
    ),
    responses(
        (status = 200, description = "The top matches of each kind, prefix matches first.", body = SearchResults),
        (status = 404, description = "Unknown game."),
        (status = 422, description = "The query has more than 32 words."),
    ),
)]
pub async fn universal_search(
    State(state): State<AppState>,
    Path(game): Path<String>,
    Query(params): Query<NameSuggestParams>,
) -> Result<Json<SearchResults>, AppError> {
    require_game(&game)?;
    let Some(term) = trim_query(params.q.as_deref()) else {
        return Ok(Json(SearchResults::empty()));
    };
    let limit = params
        .limit
        .unwrap_or(DEFAULT_SEARCH_LIMIT)
        .clamp(1, MAX_SEARCH_LIMIT) as usize;

    // The set-name map dresses both the product and the precon rows; load it once and lend
    // it to both legs rather than paying the same lookup twice per keystroke.
    let set_names = set_name_map(&state, &game).await?;
    let (cards, products, precons) = tokio::try_join!(
        search_cards(&state, &game, term, limit),
        search_products(&state, &game, term, limit, &set_names),
        search_precons(&state, &game, term, limit, &set_names),
    )?;
    let (keyword_hits, keywords_more) = keywords::search(&game, term, limit);

    Ok(Json(SearchResults {
        cards,
        products,
        precons,
        keywords: SearchGroup {
            data: keyword_hits,
            has_more: keywords_more,
        },
    }))
}

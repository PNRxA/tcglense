//! Catalog card-combos endpoint (issue #683): the Commander Spellbook combos a card is a
//! piece of — the card page's "Combos with" panel, keyed by the card's gameplay identity
//! (`oracle_id`) so every printing answers the same list.

use axum::{Json, extract::State};
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect};
use serde::Serialize;

use crate::entities::prelude::{Combo, ComboPiece};
use crate::entities::{combo, combo_piece};
use crate::error::AppError;
use crate::extract::Path;
use crate::handlers::shared::combos::{
    CardCombo, ComboPieceResponse, ComboSummary, load_combos, representative_printings,
};
use crate::handlers::shared::{load_card, require_game};
use crate::spellbook;
use crate::state::AppState;

/// How many combos a card page lists. Sol Ring is a piece of thousands; the page shows the
/// most-played and states the total, and the rest are one click away on the source.
const MAX_CARD_COMBOS: u64 = 50;

/// The combos a card is a piece of, most-played first.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct CardCombos {
    /// At most 50, by popularity; `total` is exact.
    pub combos: Vec<CardCombo>,
    pub total: i64,
    /// Where the data comes from, for the attribution the source asks for.
    pub source: String,
    pub source_url: String,
}

/// Get a card's combos
///
/// `GET /api/games/{game}/cards/{id}/combos` -> the Commander Spellbook combos the card is
/// a piece of, most-played first (at most 50; `total` is exact). Keyed by the card's
/// gameplay identity (`oracle_id`), so every printing returns the same list; every piece
/// links to a catalog printing where one exists. `404` if the game or card id is unknown;
/// an empty list when the card is in no combo — or when no combo data has been synced.
#[utoipa::path(
    get,
    path = "/api/games/{game}/cards/{id}/combos",
    tag = "Cards",
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("id" = String, Path, description = "External card id"),
    ),
    responses(
        (status = 200, description = "The card's combos, most-played first.", body = CardCombos),
        (status = 404, description = "Unknown game or card."),
    ),
)]
pub async fn card_combos(
    State(state): State<AppState>,
    Path((game, id)): Path<(String, String)>,
) -> Result<Json<CardCombos>, AppError> {
    require_game(&game)?;
    let card = load_card(&state, &game, &id).await?;

    let mut response = CardCombos {
        combos: Vec::new(),
        total: 0,
        source: spellbook::ATTRIBUTION.to_string(),
        source_url: spellbook::SITE_URL.to_string(),
    };
    // Combos key on the gameplay identity; a card without one (a token) is in none.
    let Some(oracle_id) = card.oracle_id.as_deref() else {
        return Ok(Json(response));
    };

    let matches = ComboPiece::find()
        .filter(combo_piece::Column::Game.eq(game.as_str()))
        .filter(combo_piece::Column::OracleId.eq(oracle_id));
    response.total = i64::try_from(matches.clone().count(&state.db).await?).unwrap_or(i64::MAX);
    if response.total == 0 {
        return Ok(Json(response));
    }
    // The most-played first: the piece rows joined to their parent for the popularity.
    let ids: Vec<i32> = matches
        .select_only()
        .column(combo_piece::Column::ComboId)
        .inner_join(Combo)
        .order_by_desc(combo::Column::Popularity)
        .order_by_asc(combo::Column::PieceCount)
        .order_by_asc(combo::Column::Id)
        .limit(MAX_CARD_COMBOS)
        .into_tuple()
        .all(&state.db)
        .await?;

    let rows = load_combos(&state.db, &game, &ids).await?;
    let oracle_ids: Vec<String> = rows
        .iter()
        .flat_map(|(_, pieces)| pieces.iter().map(|p| p.oracle_id.clone()))
        .collect();
    let printings = representative_printings(&state.db, &game, &oracle_ids).await?;

    response.combos = rows
        .iter()
        .map(|(model, pieces)| CardCombo {
            summary: ComboSummary::from_model(model),
            pieces: pieces
                .iter()
                .map(|p| ComboPieceResponse {
                    oracle_id: p.oracle_id.clone(),
                    name: p.name.clone(),
                    quantity: p.quantity,
                    must_be_commander: p.must_be_commander,
                    card_id: printings.get(&p.oracle_id).cloned(),
                })
                .collect(),
        })
        .collect();
    Ok(Json(response))
}

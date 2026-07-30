//! The per-deck record — what the optional per-seat deck link is *for*.
//!
//! A deck's record is derived, never stored: it's every finished session where a seat played
//! that deck, folded by outcome. That means it needs no bookkeeping on the deck row, it can't
//! drift out of step with the games it summarises, and deleting a game or a deck simply removes
//! its contribution.
//!
//! Two deliberate narrowings:
//!
//! - Only **finished** sessions count, and only seats carrying a result. A game abandoned
//!   mid-play has no outcome to attribute, so it must not dilute a win rate.
//! - The deck is **inner-joined and ownership-checked**. The seat's `deck_id` has no foreign
//!   key (deleting a played deck must not fail or take history with it), so a deck that is gone
//!   — or was never the caller's — contributes nothing rather than a dangling id.

use std::collections::HashMap;

use axum::{
    Json,
    extract::{Query, State},
};
use sea_orm::prelude::DateTimeUtc;
use sea_orm::sea_query::JoinType;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect, RelationTrait};

use crate::auth::extractor::AuthUser;
use crate::entities::prelude::LifeSessionPlayer;
use crate::entities::{deck, life_session, life_session_player};
use crate::error::AppError;
use crate::extract::Path;
use crate::handlers::shared::{DataBody, require_game};
use crate::state::AppState;

use super::{
    DeckRecordParams, LifeDeckRecord, RESULT_DRAW, RESULT_LOSS, RESULT_NONE, RESULT_WIN,
    STATUS_FINISHED,
};

/// Per-deck win/loss record
///
/// `GET /api/tools/{game}/life/decks` -> how each of the caller's decks has actually performed
/// across the games they've tracked: games played, wins, losses, draws, win rate and when it
/// was last played. Ordered by games played (most-played first), then by name.
///
/// `?deck_id=` narrows to one deck — what a deck's own page asks for, so showing one record
/// line doesn't pull every deck's.
///
/// Only finished games with a recorded result count, and `win_rate` is `null` (not `0`) for a
/// deck with no games, so an unplayed deck never reads as a losing one.
#[utoipa::path(
    get,
    path = "/api/tools/{game}/life/decks",
    tag = "Tools",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("deck_id" = Option<i32>, Query, description = "Narrow to a single deck."),
    ),
    responses(
        (status = 200, description = "The caller's decks that have been played, most-played first.", body = DataBody<Vec<LifeDeckRecord>>),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game."),
    ),
)]
pub async fn deck_records(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
    Query(params): Query<DeckRecordParams>,
) -> Result<Json<DataBody<Vec<LifeDeckRecord>>>, AppError> {
    require_game(&game)?;

    // Grouped by (deck, outcome) so the counts come back as plain `COUNT(*)`/`MAX(...)`
    // aggregates — no conditional sums, so the query stays inside the SeaORM API and reads the
    // same on SQLite and Postgres. At most four rows per deck, folded below.
    let mut query = LifeSessionPlayer::find()
        .select_only()
        .column(life_session_player::Column::DeckId)
        .column(deck::Column::Name)
        .column(life_session_player::Column::Result)
        .column_as(life_session_player::Column::Id.count(), "games")
        .column_as(life_session::Column::StartedAt.max(), "last_played_at")
        .join(
            JoinType::InnerJoin,
            life_session_player::Relation::Session.def(),
        )
        .join(
            JoinType::InnerJoin,
            life_session_player::Relation::Deck.def(),
        )
        .filter(life_session::Column::UserId.eq(user.id))
        .filter(life_session::Column::Game.eq(&game))
        .filter(life_session::Column::Status.eq(STATUS_FINISHED))
        .filter(life_session_player::Column::Result.ne(RESULT_NONE))
        // The join alone doesn't prove the deck is the caller's: `deck_id` is unconstrained, so
        // scope it to their decks for this game explicitly.
        .filter(deck::Column::UserId.eq(user.id))
        .filter(deck::Column::Game.eq(&game));
    if let Some(deck_id) = params.deck_id {
        query = query.filter(deck::Column::Id.eq(deck_id));
    }
    let rows: Vec<(Option<i32>, String, String, i64, Option<DateTimeUtc>)> = query
        .group_by(life_session_player::Column::DeckId)
        .group_by(deck::Column::Name)
        .group_by(life_session_player::Column::Result)
        .into_tuple()
        .all(&state.db)
        .await?;

    let mut by_deck: HashMap<i32, LifeDeckRecord> = HashMap::new();
    for (deck_id, deck_name, result, count, last_played_at) in rows {
        // Nulls are filtered out by the inner join; skip defensively rather than unwrap.
        let Some(deck_id) = deck_id else { continue };
        let record = by_deck.entry(deck_id).or_insert_with(|| LifeDeckRecord {
            deck_id,
            deck_name,
            games: 0,
            wins: 0,
            losses: 0,
            draws: 0,
            win_rate: None,
            last_played_at: None,
        });
        record.games += count;
        match result.as_str() {
            RESULT_WIN => record.wins += count,
            RESULT_LOSS => record.losses += count,
            RESULT_DRAW => record.draws += count,
            // An outcome vocabulary this build doesn't know still counts as a game played,
            // rather than being silently dropped from the total.
            _ => {}
        }
        record.last_played_at = record.last_played_at.max(last_played_at);
    }

    let mut data: Vec<LifeDeckRecord> = by_deck.into_values().collect();
    for record in &mut data {
        if record.games > 0 {
            record.win_rate = Some(record.wins as f64 / record.games as f64);
        }
    }
    // Most-played first — the decks whose record actually means something — then alphabetical
    // so the order is stable between reads.
    data.sort_by(|a, b| {
        b.games
            .cmp(&a.games)
            .then_with(|| a.deck_name.cmp(&b.deck_name))
            .then_with(|| a.deck_id.cmp(&b.deck_id))
    });

    Ok(Json(DataBody { data }))
}

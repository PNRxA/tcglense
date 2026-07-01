//! Shared card-search compilation: turns a `?q` string into a SeaORM `Condition`,
//! dispatching to the game's query syntax. Reused by the catalog card lists and the
//! authenticated collection list so both accept the same search grammar.

use sea_orm::{
    Condition,
    sea_query::{Expr, LikeExpr, SimpleExpr},
};

use crate::catalog::Game;
use crate::entities::card;
use crate::error::AppError;
use crate::scryfall::search::escape_like;

/// Build the `q` search filter, dispatching to the game's query syntax. MTG
/// (Scryfall) gets the full Scryfall-style grammar (see [`crate::scryfall::search`]);
/// any other game falls back to a plain card-name substring match. A malformed
/// Scryfall query becomes an `AppError::Validation` (HTTP 422).
pub(crate) fn search_condition(game: &Game, search: &str) -> Result<Condition, AppError> {
    match game.id {
        crate::scryfall::GAME => Ok(crate::scryfall::search::parse(search)?),
        _ => Ok(Condition::all().add(name_like(search))),
    }
}

/// A `name LIKE %term%` filter for the fallback (non-Scryfall) game search, with
/// LIKE metacharacters in `search` escaped so they match literally (paired with an
/// explicit `ESCAPE '\'`).
pub(crate) fn name_like(search: &str) -> SimpleExpr {
    let pattern = format!("%{}%", escape_like(search));
    Expr::col((card::Entity, card::Column::Name)).like(LikeExpr::new(pattern).escape('\\'))
}

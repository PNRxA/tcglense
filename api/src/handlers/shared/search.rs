//! Shared card-search compilation: turns a `?q` string into a SeaORM `Condition`,
//! dispatching to the game's query syntax. Reused by the catalog card lists and the
//! authenticated collection list so both accept the same search grammar.

use sea_orm::{
    Condition,
    sea_query::{Expr, Func, LikeExpr, SimpleExpr},
};

use crate::catalog::Game;
use crate::db::Dialect;
use crate::entities::card;
use crate::error::AppError;
use crate::scryfall::search::escape_like;

/// Build the `q` search filter, dispatching to the game's query syntax. MTG
/// (Scryfall) gets the full Scryfall-style grammar (see [`crate::scryfall::search`]);
/// any other game falls back to a plain card-name substring match. A malformed
/// Scryfall query becomes an `AppError::Validation` (HTTP 422). `dialect` selects
/// the backend SQL flavour for the compiled MTG fragments (the fallback arm is a
/// typed builder, so it needs no dialect).
pub(crate) fn search_condition(
    game: &Game,
    search: &str,
    dialect: Dialect,
) -> Result<Condition, AppError> {
    match game.id {
        crate::scryfall::GAME => Ok(crate::scryfall::search::parse(search, dialect)?),
        _ => Ok(Condition::all().add(name_like(search))),
    }
}

/// A `LOWER(name) LIKE %term%` filter for the fallback (non-Scryfall) game search,
/// with LIKE metacharacters in `search` escaped so they match literally (paired with
/// an explicit `ESCAPE '\'`). Folds both sides to lower-case so the match is
/// case-insensitive on Postgres too; `to_ascii_lowercase` matches SQLite's ASCII-only
/// `LOWER()`, so the SQLite result set is byte-identical. Typed, so sea-query emits
/// the correct placeholder for either backend without a dialect param.
pub(crate) fn name_like(search: &str) -> SimpleExpr {
    let pattern = format!("%{}%", escape_like(search).to_ascii_lowercase());
    Expr::expr(Func::lower(Expr::col((card::Entity, card::Column::Name))))
        .like(LikeExpr::new(pattern).escape('\\'))
}

//! Shared card-search compilation: turns a `?q` string into a SeaORM `Condition`,
//! dispatching to the game's query syntax. Reused by the catalog card lists and the
//! authenticated collection list so both accept the same search grammar.

use sea_orm::{
    Condition,
    sea_query::{Expr, Func, IntoColumnRef, LikeExpr, SimpleExpr},
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

/// Most whitespace-separated words a plain name search may carry.
///
/// Every word becomes its own `LIKE`, so this bounds both the SQL we build and the work the
/// database does. Real names are short — the longest sealed product in the catalog is under a
/// dozen words — so this is far above any genuine query and exists purely as a guard.
pub(crate) const MAX_NAME_SEARCH_WORDS: usize = 32;

/// The "every whitespace-separated word must appear in the name" filter (issue #273's rule),
/// shared by the sealed-product and preconstructed-deck listings so the two answer
/// "commander tarkir" identically.
///
/// **Returns one flat [`Condition`] — never a chain of `.filter()` calls — and that is
/// load-bearing, not style.** SeaORM folds each successive `.filter()` into a *nested* binary
/// AND, and sea-query's SQL builder walks that tree with mutual recursion
/// (`prepare_simple_expr` -> `binary_expr` -> `prepare_simple_expr_common`, ~3 stack frames per
/// level). Both listings previously looped `.filter()` once per word over a caller-supplied
/// `?q`, so ~1000 words nested ~1000 deep and overflowed the tokio worker's stack — which is a
/// **process abort**, not a 500: one anonymous GET killed the whole server and every request in
/// flight with it. A flat `Condition::all()` is a single level regardless of word count, and
/// the cap keeps the query itself bounded. Anything else that wants per-word matching must come
/// through here.
///
/// Over [`MAX_NAME_SEARCH_WORDS`] words is a `Validation` error (422) rather than a silent
/// truncation, which would quietly answer a different question than the one asked.
pub(crate) fn every_word_matches<C>(column: C, search: &str) -> Result<Condition, AppError>
where
    C: IntoColumnRef + Clone,
{
    every_word_matches_with(search, |pattern| {
        Expr::expr(Func::lower(Expr::col(column.clone()))).like(LikeExpr::new(pattern).escape('\\'))
    })
}

/// The per-word engine behind [`every_word_matches`], with the `LIKE` leaf pluggable.
///
/// The typed column form above is right for `products` and `precon_decks`, but the card
/// listing's name column is served on Postgres by an **expression** index
/// (`idx_cards_name_trgm`, `m..027`, built on `LOWER(COALESCE(name, ''))`), and the planner
/// only matches that index when the `LIKE`'s left side is spelled exactly the same way — so
/// the universal search's card leg hands in the indexed spelling instead
/// (`handlers::catalog::indexed_name_like`). Both callers get the one word split, the one
/// word cap, and the one flat `Condition`, which is the part that must never be forked.
///
/// `like` receives each word's ready-made pattern: `%word%`, `LIKE`-escaped and ASCII
/// lower-cased (see [`every_word_matches`] for why that folding is the portable one), and
/// must pair it with `ESCAPE '\'`.
pub(crate) fn every_word_matches_with(
    search: &str,
    mut like: impl FnMut(String) -> SimpleExpr,
) -> Result<Condition, AppError> {
    let words: Vec<&str> = search.split_whitespace().collect();
    if words.len() > MAX_NAME_SEARCH_WORDS {
        return Err(AppError::Validation(format!(
            "search accepts at most {MAX_NAME_SEARCH_WORDS} words"
        )));
    }
    let mut condition = Condition::all();
    for word in words {
        // LIKE metacharacters escaped so they match literally (paired with an explicit
        // ESCAPE '\'). Both sides folded to lower-case so the match is case-insensitive on
        // Postgres too; `to_ascii_lowercase` matches SQLite's ASCII-only `LOWER()`, so the
        // SQLite result set stays byte-identical.
        let pattern = format!("%{}%", escape_like(word).to_ascii_lowercase());
        condition = condition.add(like(pattern));
    }
    Ok(condition)
}

/// A sort key that surfaces the rows whose name **starts with** the whole search text
/// before the rows that merely contain it: `0` for a prefix match, `1` otherwise, so an
/// `ORDER BY … ASC` on it leads with "Sol Ring" for `sol r` and only then lists "Parasol
/// Ring". The card-name autocomplete has always ranked this way (`name_suggestions_query`);
/// the universal search applies the same rank to every leg so the groups read alike.
///
/// Case-insensitive through the same lower-both fold as [`every_word_matches`], and the
/// text is `LIKE`-escaped so a literal `%`/`_` can't widen the prefix. Pure ordering: it
/// never filters, so it composes with any `WHERE`.
pub(crate) fn starts_with_rank<C>(column: C, search: &str) -> SimpleExpr
where
    C: IntoColumnRef,
{
    let pattern = format!("{}%", escape_like(search.trim()).to_ascii_lowercase());
    let starts_with =
        Expr::expr(Func::lower(Expr::col(column))).like(LikeExpr::new(pattern).escape('\\'));
    Expr::case(starts_with, 0).finally(1).into()
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

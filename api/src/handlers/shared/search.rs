//! Shared card-search compilation: turns a `?q` string into a SeaORM `Condition`,
//! dispatching to the game's query syntax. Reused by the catalog card lists and the
//! authenticated collection list so both accept the same search grammar.

use std::collections::HashMap;

use sea_orm::{
    ColumnTrait, Condition,
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
    let mut condition = Condition::all();
    for word in search_words(search)? {
        condition = condition.add(like(contains_pattern(word)));
    }
    Ok(condition)
}

/// The one word split behind every per-word name rule: whitespace-separated, capped at
/// [`MAX_NAME_SEARCH_WORDS`] (a `Validation` error past it — see [`every_word_matches`] for
/// why the cap is a refusal rather than a truncation).
pub(crate) fn search_words(search: &str) -> Result<Vec<&str>, AppError> {
    let words: Vec<&str> = search.split_whitespace().collect();
    if words.len() > MAX_NAME_SEARCH_WORDS {
        return Err(AppError::Validation(format!(
            "search accepts at most {MAX_NAME_SEARCH_WORDS} words"
        )));
    }
    Ok(words)
}

/// One word's `%word%` `LIKE` pattern: metacharacters escaped so they match literally
/// (paired with an explicit `ESCAPE '\'`), and lower-cased so the match is case-insensitive
/// on Postgres too — `to_ascii_lowercase` matches SQLite's ASCII-only `LOWER()`, so the
/// SQLite result set stays byte-identical.
fn contains_pattern(word: &str) -> String {
    format!("%{}%", escape_like(word).to_ascii_lowercase())
}

/// The universal search's **card** rule (issue #709): every whitespace-separated word must
/// appear in the card's name, **or** name its set, **or** be its collector number within a
/// set another word names — and the words must identify a card: at least one in the name, or
/// a set word paired with a collector number.
///
/// "Sol Ring" is one name printed in a hundred sets, and a visitor who types `sol ring cmr`
/// or `lightning bolt alpha` is naming a printing — so a word the name doesn't carry may
/// instead be a set code (`cmr`, matched whole) or a piece of a set name (`legends`), and
/// the fold behind the card leg then represents the name with a printing from *that* set,
/// because the filter runs before the fold. A set and a collector number identify a
/// printing as surely as its name does (`cmr 129` is the Commander Legends Sol Ring; that is
/// how a checklist, a binder page and a deck export all spell it), so a word that is the
/// collector number of a printing in a set another word names counts too — but only paired
/// with a set word: `129` alone names nothing (every set has a #129), and an arm on a
/// number with no set to lead the `(game, set_code, collector_number)` index would be a
/// scan.
///
/// The set half is resolved in Rust from the set map the handler already holds
/// (`set_codes_for`: a word → the codes of every set it names), so what reaches SQL is
/// `name LIKE … OR set_code IN (…) OR (set_code IN (…) AND LOWER(collector_number) = …)` —
/// literal lists the `(game, set_code[, collector_number])` indexes answer — and never a
/// `LIKE` on `cards.set_name` (no index) or a correlated subquery, either of which turns the
/// whole per-keystroke read into a scan of the `cards` heap (see `AGENTS.md`'s note on
/// `m..068`).
///
/// **The words must identify a card.** Without that conjunct, a bare set name —
/// `bloomburrow`, `commander legends` — would match every card in the set and answer a
/// handful of arbitrary ones; that is the sets group's question, not the cards'. So at
/// least one word must be in the name, or one word must be a set another word's number
/// sits in. The conjunct is an `OR` of the name leaves (plus the set-and-number pair), which
/// the Postgres planner can drive from the trigram and set-code indexes as a `BitmapOr`, so
/// nothing un-indexed is ever the driving scan.
///
/// Same word split, cap and leaf-pluggable `LIKE` as [`every_word_matches_with`] — the card
/// leg hands in its trigram-indexed spelling — and one bounded tree: a flat `AND` of
/// per-word `OR`s plus the flat identity `OR`, never a `.filter()` chain per word.
///
/// The widened rule can only ever **append** to what the plain name rule answers, never
/// reorder it: a set word is any substring of a set name, so `sol ring` also matches
/// "Soldier of the Grey Host" through "The Lord of the **Ring**s", and a caller must sort
/// by [`NameOrSetMatch::by_name_alone`] first so every name match still leads. Hence the
/// pair: the filter, and the name-only half of it as a rank.
pub(crate) fn every_word_in_name_or_set_with<C>(
    search: &str,
    mut like: impl FnMut(String) -> SimpleExpr,
    set_column: C,
    number_column: C,
    set_codes_for: impl Fn(&str) -> Vec<String>,
) -> Result<NameOrSetMatch, AppError>
where
    C: ColumnTrait,
{
    let words = search_words(search)?;
    let codes_per_word: Vec<Vec<String>> = words.iter().map(|word| set_codes_for(word)).collect();
    // Every set any word names — the sets a collector-number word may sit in.
    let mut named_sets: Vec<String> = codes_per_word.iter().flatten().cloned().collect();
    named_sets.sort();
    named_sets.dedup();
    // Qualified (`"cards"."collector_number"`) like the `is_in` leaves beside it.
    let lower_number = || Expr::expr(Func::lower(Expr::col(number_column.as_column_ref())));

    let mut every_word = Condition::all();
    let mut every_word_in_name = Condition::all();
    let mut identifies = Condition::any();
    for (word, codes) in words.iter().zip(codes_per_word) {
        let in_name = like(contains_pattern(word));
        every_word_in_name = every_word_in_name.add(in_name.clone());
        identifies = identifies.add(in_name.clone());
        let mut arms = Condition::any().add(in_name);
        if !codes.is_empty() {
            arms = arms.add(set_column.is_in(codes));
        }
        if !named_sets.is_empty() {
            arms = arms.add(
                Condition::all()
                    .add(set_column.is_in(named_sets.iter().cloned()))
                    .add(lower_number().eq(word.to_ascii_lowercase())),
            );
        }
        every_word = every_word.add(arms);
    }
    if !named_sets.is_empty() {
        identifies = identifies.add(
            Condition::all()
                .add(set_column.is_in(named_sets.iter().cloned()))
                .add(lower_number().is_in(words.iter().map(|word| word.to_ascii_lowercase()))),
        );
    }
    Ok(NameOrSetMatch {
        filter: every_word.add(identifies),
        by_name_alone: every_word_in_name,
    })
}

/// What [`every_word_in_name_or_set_with`] compiles a term to.
pub(crate) struct NameOrSetMatch {
    /// The row filter: every word in the name, or naming the set, or a number in a named
    /// set — and the words identifying a card.
    pub(crate) filter: Condition,
    /// The plain name rule alone (every word in the name), for a caller to **rank** by:
    /// `ORDER BY CASE WHEN <this> THEN 0 ELSE 1 END` keeps every row the name rule answers
    /// ahead of the rows only a set word let in. Pure ordering; it never filters.
    pub(crate) by_name_alone: Condition,
}

impl NameOrSetMatch {
    /// `0` for a row every word matched by name, `1` for one that needed a set word — the
    /// leading sort key of the card leg, so the widened rule appends and never reorders.
    pub(crate) fn by_name_alone_rank(&self) -> SimpleExpr {
        Expr::case(self.by_name_alone.clone(), 0).finally(1).into()
    }
}

/// Fewest characters a word needs before it may name a set by **substring**: every set name
/// has an "a" and most have a "the", so a one- or two-letter word would name the whole
/// catalog — and bind a code list the size of it, per word. A whole set **code** still
/// matches at any length, since a code is an exact identifier.
pub(crate) const MIN_SET_NAME_WORD_CHARS: usize = 3;

/// The set codes one search word names, out of a game's `code → name` set map: a set whose
/// **code** is the word (`cmr`, case-insensitively, whole — a code is an identifier, and a
/// substring of one names nothing) or whose **name** contains it (`legends` → Commander
/// Legends, Legends, …; [`MIN_SET_NAME_WORD_CHARS`] or longer), under the same ASCII
/// lower-case fold as the SQL `LIKE`s beside it. Sorted, so the SQL a term compiles to is
/// deterministic. The word arrives raw — not `LIKE`-escaped — since this match is in Rust.
pub(crate) fn set_codes_matching(word: &str, sets: &HashMap<String, String>) -> Vec<String> {
    let needle = word.to_ascii_lowercase();
    let by_name = needle.chars().count() >= MIN_SET_NAME_WORD_CHARS;
    let mut codes: Vec<String> = sets
        .iter()
        .filter(|(code, name)| {
            code.eq_ignore_ascii_case(&needle)
                || (by_name && name.to_ascii_lowercase().contains(&needle))
        })
        .map(|(code, _)| code.clone())
        .collect();
    codes.sort();
    codes
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

/// [`starts_with_rank`] refined by **how many leading words** of the search the name starts
/// with: `0` for the whole text (exactly the tier `starts_with_rank` leads with), then one
/// tier per leading word dropped, and last a name that starts with none of them. For
/// `sol ring cmr` that is `CASE WHEN name LIKE 'sol ring cmr%' THEN 0 WHEN name LIKE
/// 'sol ring%' THEN 1 WHEN name LIKE 'sol%' THEN 2 ELSE 3 END` — so "Sol Ring" (tier 1)
/// still leads "Parasol Ring" (tier 3) once a set word is appended to the term, where the
/// whole-text rank would have tied them and let the alphabet put Parasol first.
///
/// A strict refinement of [`starts_with_rank`] — tier `0` is the same rows, and its tier
/// `1` is only ever split, never reordered against tier `0` — so every leg still reads
/// "prefix matches first". Words are re-joined with single spaces, so a doubled space in
/// the typed text can't cost a name its prefix tier. Bounded by the word cap the caller
/// already enforced (`search_words`); a term past it never reaches here.
pub(crate) fn leading_words_rank<C>(column: C, search: &str) -> SimpleExpr
where
    C: IntoColumnRef + Clone,
{
    let words: Vec<&str> = search.split_whitespace().collect();
    let tiers = words.len();
    let mut case: Option<sea_orm::sea_query::CaseStatement> = None;
    for (tier, keep) in (1..=tiers).rev().enumerate() {
        let prefix = words[..keep].join(" ");
        let pattern = format!("{}%", escape_like(&prefix).to_ascii_lowercase());
        let starts_with = Expr::expr(Func::lower(Expr::col(column.clone())))
            .like(LikeExpr::new(pattern).escape('\\'));
        case = Some(match case {
            None => Expr::case(starts_with, tier as i32),
            Some(case) => case.case(starts_with, tier as i32),
        });
    }
    match case {
        Some(case) => case.finally(tiers as i32).into(),
        // A blank search ranks everything alike.
        None => Expr::value(0),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sets() -> HashMap<String, String> {
        HashMap::from([
            ("cmr".to_string(), "Commander Legends".to_string()),
            ("leg".to_string(), "Legends".to_string()),
            (
                "ltr".to_string(),
                "The Lord of the Rings: Tales of Middle-earth".to_string(),
            ),
            ("m10".to_string(), "Magic 2010".to_string()),
        ])
    }

    #[test]
    fn set_codes_matching_is_a_whole_code_or_a_name_substring_sorted() {
        assert_eq!(set_codes_matching("legends", &sets()), vec!["cmr", "leg"]);
        assert_eq!(set_codes_matching("LEGENDS", &sets()), vec!["cmr", "leg"]);
        assert_eq!(set_codes_matching("CMR", &sets()), vec!["cmr"]);
        assert_eq!(set_codes_matching("ring", &sets()), vec!["ltr"]);
        // A code matches whole, never as a substring.
        assert!(set_codes_matching("cm", &sets()).is_empty());
        // A short word names no set by substring ("m1" is in "Magic 2010"? no — but "10"
        // is, and a two-character word must not bind a code list the size of the catalog).
        assert!(set_codes_matching("10", &sets()).is_empty());
        assert!(set_codes_matching("of", &sets()).is_empty());
        // …though a whole code still matches at any length.
        let short = HashMap::from([("5e".to_string(), "Fifth Edition".to_string())]);
        assert_eq!(set_codes_matching("5E", &short), vec!["5e"]);
        assert!(set_codes_matching("zzz", &sets()).is_empty());
    }

    #[test]
    fn name_or_set_match_needs_a_set_before_it_touches_the_number_column() {
        use crate::entities::card;
        let like = |pattern: String| {
            Expr::expr(Func::lower(Expr::col((card::Entity, card::Column::Name))))
                .like(LikeExpr::new(pattern).escape('\\'))
        };
        let no_sets = HashMap::new();
        let plain = every_word_in_name_or_set_with(
            "sol ring",
            like,
            card::Column::SetCode,
            card::Column::CollectorNumber,
            |word| set_codes_matching(word, &no_sets),
        )
        .expect("compiles");
        let sql = sea_orm::sea_query::Query::select()
            .expr(Expr::value(1))
            .from(card::Entity)
            .cond_where(plain.filter)
            .to_string(sea_orm::sea_query::PostgresQueryBuilder);
        assert!(!sql.contains("set_code"), "{sql}");
        assert!(!sql.contains("collector_number"), "{sql}");

        // Over the word cap it is the same refusal as the plain rule.
        let long = vec!["x"; MAX_NAME_SEARCH_WORDS + 1].join(" ");
        assert!(matches!(
            every_word_in_name_or_set_with(
                &long,
                like,
                card::Column::SetCode,
                card::Column::CollectorNumber,
                |_| Vec::new()
            ),
            Err(AppError::Validation(_))
        ));
    }
}

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
/// The typed column form above is right for `products`, `precon_decks` and `card_sets`,
/// but the card listing's name column is served on Postgres by an **expression** index
/// (`idx_cards_name_trgm`, `m..027`, built on `LOWER(COALESCE(name, ''))`), and the planner
/// only matches that index when the `LIKE`'s left side is spelled exactly the same way — so
/// a card-name caller hands in the indexed spelling instead
/// (`handlers::catalog::indexed_name_like`). The universal search's card leg builds on the
/// same word split, cap and pattern through [`every_word_in_name_or_set_with`], its
/// name-or-set widening; the split and the cap are the part that must never be forked.
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
fn search_words(search: &str) -> Result<Vec<&str>, AppError> {
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
/// appear in the card's name, **or** name its set, **or** — for a word carrying a digit — be
/// its collector number within a set another word names. Unless the whole term names a
/// set, in which case the card leg is the plain name rule.
///
/// "Sol Ring" is one name printed in a hundred sets, and a visitor who types `sol ring cmr`
/// or `lightning bolt alpha` is naming a printing — so a word the name doesn't carry may
/// instead be a set code (`cmr`, matched whole) or a piece of a set name (`legends`), and
/// the fold behind the card leg then represents the name with a printing from *that* set,
/// because the filter runs before the fold. A set and a collector number identify a
/// printing as surely as its name does (`cmr 129` is the Commander Legends Sol Ring; that is
/// how a checklist, a binder page and a deck export all spell it), so a digit-bearing word
/// that is the collector number of a printing in a set another word names counts too — but
/// only paired with a set word: `129` alone names nothing (every set has a #129).
///
/// **A term that names a set asks about the set, not its cards** (`term_names_a_set`, the
/// caller's answer from [`term_names_a_set`]). Without that gate `bloomburrow`, `commander
/// legends` or `oath of the gatewatch` would match every card in the set — through the set
/// arms, or through a short word like `of` that names the set but is too short to resolve
/// to codes and so falls back to a name leaf — and answer a handful of arbitrary ones; that
/// is the sets group's question. Gated, such a term compiles to the plain name rule, so
/// the card "Time Spiral" still answers `time spiral` and nothing answers `oath of the
/// gatewatch`. Past the gate no clause has to say "at least one word is in the name": a
/// row whose every word was explained by its set would be a row whose set name carries
/// every word, which is exactly the gated case, and the one other way to match without a
/// name word is the set-and-number pair, which is meant.
///
/// The set half is resolved in Rust from the set map the handler already holds
/// (`set_codes_for`: a word → the codes of every set it names), so what reaches SQL is
/// `name LIKE … OR set_code IN (…) OR (set_code IN (…) AND collector_number IN (…))` —
/// bound lists the `(game, set_code[, collector_number])` indexes answer — and never a
/// `LIKE` on `cards.set_name` (no index) or a correlated subquery, either of which turns the
/// whole per-keystroke read into a scan of the `cards` heap (see `AGENTS.md`'s note on
/// `m..068`). The number is compared **raw** (the word as typed, lower- and upper-cased,
/// never `LOWER(collector_number)`), because `m..024`'s composite index is on the raw text
/// column and only a raw comparison lets its third key seek.
///
/// **Bounded, because every list here is bound parameters and the caller's fold repeats
/// the filter** (Postgres allows 65,535 binds a statement, SQLite 32,766). Words are
/// deduplicated; a word naming more than [`MAX_SET_CODES_PER_WORD`] sets names none (no
/// three-letter word names that many today — the worst is ~200 — so only a term built to
/// hurt hits it); and at most [`MAX_NUMBER_WORDS`] digit words get the number arm, since
/// that arm binds the union of every named set and a printing has one number. Worst case
/// is then under 25k binds a copy, and the unit test below pins the doubled Postgres
/// shape under the limit.
///
/// Same word split, cap and leaf-pluggable `LIKE` as [`every_word_matches_with`] — the card
/// leg hands in its trigram-indexed spelling — and one bounded tree: a flat `AND` of
/// per-word `OR`s, never a `.filter()` chain per word.
///
/// The widened rule can only ever **append** to what the plain name rule answers, never
/// reorder it: a set word is any substring of a set name, so `sol ring` also matches
/// "Soldier of the Grey Host" through "The Lord of the **Ring**s", and a caller must sort
/// by [`NameOrSetMatch::by_name_alone_rank`] first so every name match still leads.
pub(crate) fn every_word_in_name_or_set_with<C>(
    search: &str,
    mut like: impl FnMut(String) -> SimpleExpr,
    set_column: C,
    number_column: C,
    set_codes_for: impl Fn(&str) -> Vec<String>,
    term_names_a_set: bool,
) -> Result<NameOrSetMatch, AppError>
where
    C: ColumnTrait,
{
    let words = dedup_words(search_words(search)?);
    let leaves: Vec<SimpleExpr> = words
        .iter()
        .map(|word| like(contains_pattern(word)))
        .collect();
    let by_name_alone = leaves
        .iter()
        .cloned()
        .fold(Condition::all(), |all, leaf| all.add(leaf));
    if term_names_a_set {
        return Ok(NameOrSetMatch {
            filter: by_name_alone,
            by_name_alone: None,
        });
    }

    let codes_per_word: Vec<Vec<String>> = words
        .iter()
        .map(|word| {
            let codes = set_codes_for(word);
            if codes.len() > MAX_SET_CODES_PER_WORD {
                Vec::new()
            } else {
                codes
            }
        })
        .collect();
    // Every set any word names — the sets a collector-number word may sit in.
    let mut named_sets: Vec<String> = codes_per_word.iter().flatten().cloned().collect();
    named_sets.sort();
    named_sets.dedup();
    let mut number_words = 0;
    let mut widened = false;

    let mut every_word = Condition::all();
    for ((word, leaf), codes) in words.iter().zip(leaves).zip(codes_per_word) {
        let mut arms = Condition::any().add(leaf);
        if !codes.is_empty() {
            widened = true;
            arms = arms.add(set_column.is_in(codes));
        }
        if !named_sets.is_empty()
            && number_words < MAX_NUMBER_WORDS
            && word.chars().any(|c| c.is_ascii_digit())
        {
            number_words += 1;
            widened = true;
            arms = arms.add(
                Condition::all()
                    .add(set_column.is_in(named_sets.iter().cloned()))
                    .add(number_column.is_in(number_spellings(word))),
            );
        }
        every_word = every_word.add(arms);
    }
    Ok(NameOrSetMatch {
        filter: every_word,
        by_name_alone: widened.then_some(by_name_alone),
    })
}

/// What [`every_word_in_name_or_set_with`] compiles a term to.
pub(crate) struct NameOrSetMatch {
    /// The row filter: every word in the name, or naming the set, or a number in a named
    /// set — or, for a term that names a set, the plain name rule.
    pub(crate) filter: Condition,
    /// The plain name rule alone (every word in the name), to **rank** by — `Some` only when
    /// the filter was actually widened by a set or number arm; a plain name filter has
    /// nothing to rank ahead of.
    by_name_alone: Option<Condition>,
}

impl NameOrSetMatch {
    /// `0` for a row every word matched by name, `1` for one that needed a set word — the
    /// leading sort key of the card leg, so the widened rule appends and never reorders.
    /// `None` when the filter is the plain name rule, where the key would be a constant.
    pub(crate) fn by_name_alone_rank(&self) -> Option<SimpleExpr> {
        self.by_name_alone
            .as_ref()
            .map(|by_name| Expr::case(by_name.clone(), 0).finally(1).into())
    }
}

/// Most sets one word may name before it is taken to name none: a word this vague
/// identifies no printing, and its code list would be that many bound parameters per copy
/// of the filter. Above the widest real word (~200 sets for a three-letter substring) so it
/// only ever bites a term built to grow the statement.
pub(crate) const MAX_SET_CODES_PER_WORD: usize = 256;

/// Most digit-bearing words that get the set-scoped collector-number arm — the arm binds
/// the union of every named set, so it is priced per word. A printing has one number; two
/// covers a typo being corrected.
pub(crate) const MAX_NUMBER_WORDS: usize = 2;

/// The spellings a typed collector number is compared against, **raw**: as typed, ASCII
/// lower-cased and upper-cased (`12a` finds a stored `12A`), deduplicated. A raw `IN` on the
/// column is what lets `m..024`'s `(game, set_code, collector_number)` index seek its third
/// key; `LOWER(collector_number) = …` would be a heap recheck over every card of every
/// named set.
fn number_spellings(word: &str) -> Vec<String> {
    let mut spellings = vec![
        word.to_string(),
        word.to_ascii_lowercase(),
        word.to_ascii_uppercase(),
    ];
    spellings.sort();
    spellings.dedup();
    spellings
}

/// Case-insensitive, order-preserving dedup: `sol sol ring` binds "sol" once.
fn dedup_words(words: Vec<&str>) -> Vec<&str> {
    let mut seen: Vec<String> = Vec::new();
    words
        .into_iter()
        .filter(|word| {
            let key = word.to_ascii_lowercase();
            if seen.contains(&key) {
                false
            } else {
                seen.push(key);
                true
            }
        })
        .collect()
}

/// Fewest characters a word needs before it may name a set by **substring**: every set name
/// has an "a" and most an "of" or "he", so a one- or two-letter word would name most of the
/// catalog — and bind a code list that size, per word. A whole set **code** still matches
/// at any length, since a code is an exact identifier. Three-letter words like "the" do
/// pass (they name ~130 sets); that is what [`MAX_SET_CODES_PER_WORD`] and the
/// [`term_names_a_set`] gate are for.
const MIN_SET_NAME_WORD_CHARS: usize = 3;

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

/// Whether the whole term names a set: some set's name contains **every** word (any
/// length — this is the one place a short word counts, since `of` and `the` are what a
/// set name is made of), or the term is a set's code. The card leg's gate: such a term is
/// the sets group's question, and the card leg answers it with the plain name rule alone
/// (see [`every_word_in_name_or_set_with`]). A blank term names nothing.
pub(crate) fn term_names_a_set(term: &str, sets: &HashMap<String, String>) -> bool {
    let words: Vec<String> = term
        .split_whitespace()
        .map(str::to_ascii_lowercase)
        .collect();
    if words.is_empty() {
        return false;
    }
    let whole = words.join(" ");
    sets.iter().any(|(code, name)| {
        code.eq_ignore_ascii_case(&whole) || {
            let name = name.to_ascii_lowercase();
            words.iter().all(|word| name.contains(word.as_str()))
        }
    })
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
    use crate::entities::card;
    use sea_orm::sea_query::{PostgresQueryBuilder, Query, SqliteQueryBuilder};

    fn sets() -> HashMap<String, String> {
        HashMap::from([
            ("cmr".to_string(), "Commander Legends".to_string()),
            ("leg".to_string(), "Legends".to_string()),
            (
                "ltr".to_string(),
                "The Lord of the Rings: Tales of Middle-earth".to_string(),
            ),
            ("m10".to_string(), "Magic 2010".to_string()),
            ("ogw".to_string(), "Oath of the Gatewatch".to_string()),
            ("tsp".to_string(), "Time Spiral".to_string()),
        ])
    }

    fn name_like(pattern: String) -> SimpleExpr {
        Expr::expr(Func::lower(Expr::col((card::Entity, card::Column::Name))))
            .like(LikeExpr::new(pattern).escape('\\'))
    }

    /// The filter's SQL and bound values, as one single-copy SELECT.
    fn compile(term: &str, sets: &HashMap<String, String>, pg: bool) -> (String, usize) {
        let m = every_word_in_name_or_set_with(
            term,
            name_like,
            card::Column::SetCode,
            card::Column::CollectorNumber,
            |word| set_codes_matching(word, sets),
            term_names_a_set(term, sets),
        )
        .expect("compiles");
        let mut q = Query::select();
        q.expr(Expr::value(1))
            .from(card::Entity)
            .cond_where(m.filter);
        let (sql, values) = if pg {
            q.build(PostgresQueryBuilder)
        } else {
            q.build(SqliteQueryBuilder)
        };
        (sql, values.0.len())
    }

    #[test]
    fn set_codes_matching_is_a_whole_code_or_a_name_substring_sorted() {
        assert_eq!(set_codes_matching("legends", &sets()), vec!["cmr", "leg"]);
        assert_eq!(set_codes_matching("LEGENDS", &sets()), vec!["cmr", "leg"]);
        assert_eq!(set_codes_matching("CMR", &sets()), vec!["cmr"]);
        assert_eq!(set_codes_matching("ring", &sets()), vec!["ltr"]);
        // "the" passes the floor and names what it names — the gate and the cap bound it.
        assert_eq!(set_codes_matching("the", &sets()), vec!["ltr", "ogw"]);
        // A code matches whole, never as a substring.
        assert!(set_codes_matching("cm", &sets()).is_empty());
        // A one- or two-character word names no set by substring ("of" is in two names).
        assert!(set_codes_matching("of", &sets()).is_empty());
        assert!(set_codes_matching("10", &sets()).is_empty());
        // …though a whole code still matches at any length.
        let short = HashMap::from([("5e".to_string(), "Fifth Edition".to_string())]);
        assert_eq!(set_codes_matching("5E", &short), vec!["5e"]);
        assert!(set_codes_matching("zzz", &sets()).is_empty());
    }

    #[test]
    fn term_names_a_set_takes_every_word_of_any_length_or_a_whole_code() {
        assert!(term_names_a_set("oath of the gatewatch", &sets()));
        assert!(term_names_a_set("OATH of", &sets()));
        assert!(term_names_a_set("time spiral", &sets()));
        assert!(term_names_a_set("cmr", &sets()));
        assert!(term_names_a_set("the", &sets()));
        assert!(!term_names_a_set("sol ring cmr", &sets()));
        assert!(!term_names_a_set("cmr 129", &sets()));
        assert!(!term_names_a_set("time spiral 12", &sets()));
        assert!(!term_names_a_set("", &sets()));
    }

    #[test]
    fn a_term_that_names_a_set_compiles_to_the_plain_name_rule() {
        let (sql, _) = compile("oath of the gatewatch", &sets(), true);
        assert!(!sql.contains("set_code"), "{sql}");
        assert!(!sql.contains("collector_number"), "{sql}");
        assert_eq!(sql.matches("LIKE").count(), 4, "{sql}");
        // And a pure-name term (naming no set) is the same plain rule, with no rank.
        let (sql, _) = compile("sol", &HashMap::new(), true);
        assert!(!sql.contains("set_code"), "{sql}");
        let m = every_word_in_name_or_set_with(
            "sol",
            name_like,
            card::Column::SetCode,
            card::Column::CollectorNumber,
            |_| Vec::new(),
            false,
        )
        .expect("compiles");
        assert!(m.by_name_alone_rank().is_none());
    }

    #[test]
    fn the_number_arm_is_raw_digit_gated_and_inside_the_named_set_list() {
        // A digit word gets `(set_code IN (…) AND collector_number IN (…))`, raw and
        // three-spelled; a word without a digit gets no number arm at all.
        let (sql, _) = compile("cmr 12a", &sets(), true);
        assert!(
            !sql.contains("LOWER(\"cards\".\"collector_number\")"),
            "{sql}"
        );
        assert_eq!(
            sql.matches("\"cards\".\"collector_number\" IN (").count(),
            1,
            "{sql}"
        );
        assert!(
            sql.contains(") AND \"cards\".\"collector_number\" IN ("),
            "the number arm sits inside the named-set AND: {sql}"
        );
        assert!(!sql.contains("OR \"cards\".\"collector_number\""), "{sql}");
        let (sql, _) = compile("sol ring legends", &sets(), true);
        assert!(!sql.contains("collector_number"), "{sql}");
        // The cap: a third digit word matches by name only.
        let (sql, _) = compile("cmr 1 2 3", &sets(), true);
        assert_eq!(
            sql.matches("\"cards\".\"collector_number\" IN (").count(),
            MAX_NUMBER_WORDS,
            "{sql}"
        );
    }

    #[test]
    fn words_are_deduplicated_and_a_vague_word_names_no_set() {
        let (sql, binds) = compile("sol SOL sol", &sets(), true);
        assert_eq!(sql.matches("LIKE").count(), 1, "{sql}");
        // One bind for the leaf's pattern (the other is the `SELECT 1`).
        assert_eq!(binds, 2);
        // A word naming more than the cap names none: only its name leaf remains.
        let many: HashMap<String, String> = (0..MAX_SET_CODES_PER_WORD + 1)
            .map(|i| (format!("s{i:04}"), format!("Vague Set {i}")))
            .collect();
        let (sql, _) = compile("vague", &many, false);
        assert!(!sql.contains("set_code"), "{sql}");
        assert_eq!(number_spellings("12a"), vec!["12A", "12a"]);
        assert_eq!(number_spellings("129"), vec!["129"]);
    }

    /// The worst term a caller can build — the word cap's worth of distinct three-letter
    /// words, each naming the most sets a word may, two of them digit-bearing — stays under
    /// both backends' bind limits even with the Postgres fold repeating the filter.
    #[test]
    fn the_worst_case_term_stays_under_the_bind_limits() {
        let words: Vec<String> = (0..MAX_NAME_SEARCH_WORDS - MAX_NUMBER_WORDS)
            .map(|i| format!("w{i:02}"))
            .chain((0..MAX_NUMBER_WORDS).map(|i| format!("{i}00")))
            .collect();
        let all_words = words.join(" ");
        // Every set's name carries every word, so each word names every set.
        let many: HashMap<String, String> = (0..MAX_SET_CODES_PER_WORD)
            .map(|i| (format!("s{i:04}"), format!("Set {i} {all_words}")))
            .collect();
        // Not the gated case: no set name carries the digit words as typed…
        assert!(!term_names_a_set(&format!("{all_words} zz9"), &many));
        let term = format!("{all_words}");
        let (_, sqlite_binds) = compile(&term, &many, false);
        let (_, pg_binds) = compile(&term, &many, true);
        assert_eq!(sqlite_binds, pg_binds);
        assert!(sqlite_binds < 32_766, "{sqlite_binds}");
        assert!(pg_binds * 2 < 65_535, "{pg_binds}");
    }

    #[test]
    fn over_the_word_cap_is_the_plain_rules_refusal() {
        let long = vec!["x"; MAX_NAME_SEARCH_WORDS + 1].join(" ");
        assert!(matches!(
            every_word_in_name_or_set_with(
                &long,
                name_like,
                card::Column::SetCode,
                card::Column::CollectorNumber,
                |_| Vec::new(),
                false,
            ),
            Err(AppError::Validation(_))
        ));
    }
}

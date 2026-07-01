//! A Scryfall-style search-query compiler.
//!
//! Turns a query string like `c:rg t:creature mv>=3 r:rare -o:flying` into a
//! [`sea_orm::Condition`] over the `cards` table, supporting the subset of
//! [Scryfall syntax](https://scryfall.com/docs/syntax) our columns can back:
//! name / type / oracle text, colours & colour identity, mana cost, mana value,
//! power / toughness / loyalty, prices, rarity, set, set type, collector number,
//! language, layout, release date, plus boolean `and`/`or`, `-` negation,
//! parentheses, and quoted phrases. Filters we cannot back (oracle keywords,
//! format legality,
//! artist, …) return a [`SearchError`] that the handler maps to HTTP 422.
//!
//! Pipeline: [`lex`] → [`Parser::parse_query`] (recursive descent → `Node`) →
//! [`compile`] (→ `Condition`). Three rules keep it safe and predictable:
//!
//! 1. **No SQL injection.** Every user value is bound as a parameter (via
//!    `Expr::cust_with_values` `?` placeholders or the typed `Expr` builders);
//!    the only interpolated identifiers are trusted, fixed column-name constants.
//! 2. **Total leaves.** Every leaf predicate evaluates to 0/1 for every row
//!    (nullable columns are wrapped with `IFNULL` / explicit `IS [NOT] NULL`), so
//!    negation is a single uniform, NULL-safe `Condition::not()`.
//! 3. **Bounded work.** Token count and parenthesis depth are capped so the
//!    public, unauthenticated search route can't be driven into pathological work.
//!
//! The stages live in their own submodules ([`error`], [`lexer`], [`parser`],
//! [`compile`]); this module owns the limit/vocabulary constants and the public
//! [`parse`] entry point, and re-exports the crate-visible symbols.

mod compile;
mod error;
mod lexer;
mod parser;

#[cfg(test)]
mod tests;

use sea_orm::Condition;

use compile::compile;
use lexer::lex;
use parser::Parser;

pub use error::SearchError;
pub(crate) use compile::escape_like;

/// Max input length (bytes) — bounds lexer allocation and LIKE-pattern length on
/// this public route, independent of the token count.
const MAX_QUERY_BYTES: usize = 4096;
/// Max tokens in one query — guards the public route against pathological input.
const MAX_TOKENS: usize = 256;
/// Max parenthesis nesting depth.
const MAX_DEPTH: usize = 64;
/// Max distinct/total mana symbols in one `m:` value (bounds the dedup scan).
const MAX_MANA_SYMBOLS: usize = 64;

/// The five MTG colours, in canonical WUBRG order.
const WUBRG: [char; 5] = ['W', 'U', 'B', 'R', 'G'];
/// Rarities low→high; index is the ordinal used by `r>=`/`r<` comparisons and by
/// the catalog's rarity sort ([`crate::handlers::catalog`]), so both rank rarity
/// identically.
pub(crate) const RARITIES: [&str; 6] = ["common", "uncommon", "rare", "special", "mythic", "bonus"];

/// Parse a Scryfall-style query into a `Condition`. An empty / whitespace-only
/// query yields `Condition::all()` (no filter).
pub fn parse(input: &str) -> Result<Condition, SearchError> {
    let tokens = lex(input)?;
    if tokens.is_empty() {
        return Ok(Condition::all());
    }
    let node = Parser::new(tokens).parse_query()?;
    compile(&node)
}

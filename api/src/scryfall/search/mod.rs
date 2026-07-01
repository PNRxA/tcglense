//! A Scryfall-style search-query compiler.
//!
//! Turns a query string like `c:rg t:creature mv>=3 r:rare -o:flying` into a
//! [`sea_orm::Condition`] over the `cards` table (plus optional result-shaping
//! directives), supporting most of [Scryfall syntax](https://scryfall.com/docs/syntax):
//! name / type / oracle (regex `/…/` too), colours & colour identity (incl. colour
//! names and guild/shard nicknames), mana cost + relational operators, mana value,
//! power / toughness / loyalty / pt / defense, prices, rarity, set, set type,
//! collector number, language, layout, release date, legality (`f:`/`banned:`/…),
//! keywords, artist, flavour text, watermark, border/frame/stamp, finishes and the
//! print-flag `is:` subjects, printing counts (`prints`/`sets`), plus boolean
//! `and`/`or`, `-` negation, parentheses, quoted phrases, and the `order:` /
//! `direction:` / `unique:` result-shaping directives. Filters backed by datasets we
//! don't ingest — Tagger tags (`otag:`/`atag:`/`function:`) and `cube:` — return a
//! [`SearchError`] the handler maps to HTTP 422.
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
//! [`parse`] / [`parse_query`] entry points (the latter also returns the `order:` /
//! `direction:` / `unique:` directives), and re-exports the crate-visible symbols.

mod compile;
mod error;
mod lexer;
mod parser;

#[cfg(test)]
mod tests;

use sea_orm::Condition;

use compile::compile;
use lexer::{Token, lex};
use parser::Parser;

pub use error::SearchError;
pub(crate) use compile::escape_like;

/// A sort field a query can request via `order:`. Mapped to the catalog's own
/// `SortField` in the handler layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Name,
    Set,
    Released,
    Rarity,
    Color,
    Cmc,
    Power,
    Toughness,
    Usd,
    Eur,
    Tix,
    Edhrec,
    Artist,
    Number,
}

/// Sort direction requested via `direction:`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Asc,
    Desc,
}

/// Result de-duplication mode requested via `unique:`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UniqueMode {
    /// One row per printing (the default — no de-duplication).
    Prints,
    /// One row per card (grouped by `oracle_id`).
    Cards,
    /// One row per distinct artwork (grouped by `illustration_id`).
    Art,
}

/// A parsed query: the row filter plus any global result-shaping directives
/// (`order:` / `direction:` / `unique:`), which are not row predicates.
pub struct ParsedQuery {
    pub condition: Condition,
    pub order: Option<SortKey>,
    pub direction: Option<Direction>,
    pub unique: Option<UniqueMode>,
}

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
/// the shared card-list rarity sort ([`crate::handlers::shared::sort`]), so both
/// rank rarity identically.
pub(crate) const RARITIES: [&str; 6] = ["common", "uncommon", "rare", "special", "mythic", "bonus"];

/// Parse a Scryfall-style query into just its row `Condition`, discarding any
/// result-shaping directives. An empty / whitespace-only query yields
/// `Condition::all()` (no filter).
pub fn parse(input: &str) -> Result<Condition, SearchError> {
    parse_query(input).map(|q| q.condition)
}

/// Parse a Scryfall-style query into its row filter plus any `order:` /
/// `direction:` / `unique:` directives. The directives are global (not boolean
/// operands), so they are pulled out of the token stream before the boolean
/// grammar runs — keeping every compiled leaf total/NULL-safe.
pub fn parse_query(input: &str) -> Result<ParsedQuery, SearchError> {
    let tokens = lex(input)?;
    let (tokens, directives) = extract_directives(tokens)?;
    let condition = if tokens.is_empty() {
        Condition::all()
    } else {
        compile(&Parser::new(tokens).parse_query()?)?
    };
    Ok(ParsedQuery {
        condition,
        order: directives.order,
        direction: directives.direction,
        unique: directives.unique,
    })
}

/// The result-shaping directives extracted from a query.
#[derive(Default)]
struct Directives {
    order: Option<SortKey>,
    direction: Option<Direction>,
    unique: Option<UniqueMode>,
}

/// Pull the global `order:` / `direction:` / `unique:` directive tokens out of the
/// stream (last-one-wins), returning the remaining tokens for the boolean parser.
/// A directive may not be negated (`-order:…`).
fn extract_directives(tokens: Vec<Token>) -> Result<(Vec<Token>, Directives), SearchError> {
    let mut d = Directives::default();
    let mut kept = Vec::with_capacity(tokens.len());
    let mut prev_was_not = false;
    for tok in tokens {
        if let Token::Filter { key, value, .. } = &tok {
            match key.as_str() {
                "order" | "direction" | "dir" | "unique" => {
                    if prev_was_not {
                        return Err(error::invalid(key, value, "cannot be negated"));
                    }
                    match key.as_str() {
                        "order" => d.order = Some(parse_sort_key(value)?),
                        "direction" | "dir" => d.direction = Some(parse_direction(value)?),
                        _ => d.unique = Some(parse_unique(value)?),
                    }
                    continue;
                }
                _ => {}
            }
        }
        prev_was_not = matches!(tok, Token::Not);
        kept.push(tok);
    }
    Ok((kept, d))
}

fn parse_sort_key(value: &str) -> Result<SortKey, SearchError> {
    Ok(match value.to_lowercase().as_str() {
        "name" => SortKey::Name,
        "set" => SortKey::Set,
        "released" | "release" | "date" => SortKey::Released,
        "rarity" => SortKey::Rarity,
        "color" | "colors" => SortKey::Color,
        "cmc" | "mv" | "manavalue" => SortKey::Cmc,
        "power" | "pow" => SortKey::Power,
        "toughness" | "tou" => SortKey::Toughness,
        "usd" | "price" => SortKey::Usd,
        "eur" => SortKey::Eur,
        "tix" => SortKey::Tix,
        "edhrec" => SortKey::Edhrec,
        "artist" => SortKey::Artist,
        "number" | "cn" | "collector" => SortKey::Number,
        other => return Err(error::invalid("order", other, "unknown sort field")),
    })
}

fn parse_direction(value: &str) -> Result<Direction, SearchError> {
    Ok(match value.to_lowercase().as_str() {
        "asc" | "ascending" | "up" => Direction::Asc,
        "desc" | "descending" | "down" => Direction::Desc,
        other => return Err(error::invalid("direction", other, "expected asc or desc")),
    })
}

fn parse_unique(value: &str) -> Result<UniqueMode, SearchError> {
    Ok(match value.to_lowercase().as_str() {
        "cards" | "card" => UniqueMode::Cards,
        "art" | "arts" => UniqueMode::Art,
        "prints" | "printings" => UniqueMode::Prints,
        other => return Err(error::invalid("unique", other, "expected cards, art or prints")),
    })
}

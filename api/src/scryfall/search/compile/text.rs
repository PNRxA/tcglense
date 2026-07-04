//! Free-text filters: name / type / oracle substring and exact matches.

use sea_orm::Condition;
use sea_orm::sea_query::SimpleExpr;

use crate::db::Dialect;
use super::common::{cond_one, cust_vals, escape_like};
use super::super::error::{SearchError, invalid, unsupported_op};
use super::super::lexer::Op;

/// Case-insensitive substring match; total (NULL → no match). Folds both sides to
/// lower-case (`LOWER(col) LIKE lowered-pattern`) so the match is case-insensitive
/// on Postgres too; `to_ascii_lowercase` matches SQLite's ASCII-only `LOWER()`/LIKE
/// fold exactly, so the SQLite result set is byte-identical to the pre-Postgres form.
pub(super) fn contains(dialect: Dialect, col: &str, value: &str) -> SimpleExpr {
    let pattern = format!("%{}%", escape_like(value).to_ascii_lowercase());
    cust_vals(
        dialect,
        format!("LOWER(COALESCE({col}, '')) LIKE ? ESCAPE '\\'"),
        [pattern],
    )
}

/// Case-insensitive exact match (wildcard-free LIKE), LOWER-both like [`contains`].
pub(super) fn exact(dialect: Dialect, col: &str, value: &str) -> SimpleExpr {
    let pattern = escape_like(value).to_ascii_lowercase();
    cust_vals(
        dialect,
        format!("LOWER(COALESCE({col}, '')) LIKE ? ESCAPE '\\'"),
        [pattern],
    )
}

pub(super) fn text_field(
    dialect: Dialect,
    col: &str,
    key: &str,
    op: Op,
    value: &str,
) -> Result<Condition, SearchError> {
    match op {
        Op::Colon | Op::Eq => Ok(cond_one(text_pattern(dialect, col, value)?)),
        _ => Err(unsupported_op(key, op)),
    }
}

/// A text-column predicate: a Scryfall `/regex/` literal compiles to a regex match,
/// otherwise a case-insensitive substring.
pub(super) fn text_pattern(
    dialect: Dialect,
    col: &str,
    value: &str,
) -> Result<SimpleExpr, SearchError> {
    match as_regex(value) {
        Some(pattern) => regex_expr(dialect, col, pattern),
        None => Ok(contains(dialect, col, value)),
    }
}

/// Recognise a `/pattern/` regex literal, returning the inner pattern.
fn as_regex(value: &str) -> Option<&str> {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 && bytes[0] == b'/' && bytes[bytes.len() - 1] == b'/' {
        Some(&value[1..value.len() - 1])
    } else {
        None
    }
}

/// Compile a regex literal to a case-insensitive regex match. On SQLite this is
/// `COALESCE(col, '') REGEXP (?i)pattern` (the registered Rust-`regex` UDF); on
/// Postgres it is `COALESCE(col, '') ~* pattern` (POSIX ARE, already CI). The
/// pattern is validated with the `regex` crate (a bad pattern is a 422) — a
/// best-effort gate under Postgres, whose POSIX ARE syntax differs from Rust-regex
/// for exotic constructs (`\d`, `\b`, lookaround, lazy quantifiers).
fn regex_expr(dialect: Dialect, col: &str, pattern: &str) -> Result<SimpleExpr, SearchError> {
    let ci = format!("(?i){pattern}");
    regex::Regex::new(&ci).map_err(|_| invalid("regex", pattern, "invalid regular expression"))?;
    Ok(cust_vals(
        dialect,
        format!("COALESCE({col}, '') {} ?", dialect.regex_operator()),
        [dialect.regex_pattern(pattern)],
    ))
}

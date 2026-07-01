//! Free-text filters: name / type / oracle substring and exact matches.

use sea_orm::Condition;
use sea_orm::sea_query::{Expr, SimpleExpr};

use super::common::{cond_one, escape_like};
use super::super::error::{SearchError, invalid, unsupported_op};
use super::super::lexer::Op;

/// Case-insensitive substring match; total (NULL → no match).
pub(super) fn contains(col: &str, value: &str) -> SimpleExpr {
    let pattern = format!("%{}%", escape_like(value));
    Expr::cust_with_values(format!("IFNULL({col}, '') LIKE ? ESCAPE '\\'"), [pattern])
}

/// Case-insensitive exact match (wildcard-free LIKE keeps ASCII case folding).
pub(super) fn exact(col: &str, value: &str) -> SimpleExpr {
    let pattern = escape_like(value);
    Expr::cust_with_values(format!("IFNULL({col}, '') LIKE ? ESCAPE '\\'"), [pattern])
}

pub(super) fn text_field(
    col: &str,
    key: &str,
    op: Op,
    value: &str,
) -> Result<Condition, SearchError> {
    match op {
        Op::Colon | Op::Eq => Ok(cond_one(text_pattern(col, value)?)),
        _ => Err(unsupported_op(key, op)),
    }
}

/// A text-column predicate: a Scryfall `/regex/` literal compiles to a `REGEXP`
/// match, otherwise a case-insensitive substring.
pub(super) fn text_pattern(col: &str, value: &str) -> Result<SimpleExpr, SearchError> {
    match as_regex(value) {
        Some(pattern) => regex_expr(col, pattern),
        None => Ok(contains(col, value)),
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

/// Compile a regex literal to `IFNULL(col, '') REGEXP ?`, validated with the same
/// `regex` crate SQLite uses (a bad pattern is a 422) and made case-insensitive.
fn regex_expr(col: &str, pattern: &str) -> Result<SimpleExpr, SearchError> {
    let ci = format!("(?i){pattern}");
    regex::Regex::new(&ci).map_err(|_| invalid("regex", pattern, "invalid regular expression"))?;
    Ok(Expr::cust_with_values(
        format!("IFNULL({col}, '') REGEXP ?"),
        [ci],
    ))
}

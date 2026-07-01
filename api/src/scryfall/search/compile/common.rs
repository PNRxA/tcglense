//! Shared low-level helpers: single-clause condition wrappers, the custom-SQL
//! builders, the comparison-operator SQL, LIKE escaping, the numeric-string
//! guard, and the NULL-safe text equality/inequality builders.

use sea_orm::Condition;
use sea_orm::Value;
use sea_orm::sea_query::{Expr, SimpleExpr};

use super::super::lexer::Op;

pub(super) fn cond_one(expr: SimpleExpr) -> Condition {
    Condition::all().add(expr)
}

/// A single-clause condition from a parameterless custom SQL fragment.
pub(super) fn raw<T>(sql: T) -> Condition
where
    T: Into<String>,
{
    cond_one(Expr::cust(sql))
}

/// A single-clause condition from a custom SQL fragment with bound values.
pub(super) fn raw_vals<T, V, I>(sql: T, values: I) -> Condition
where
    T: Into<String>,
    V: Into<Value>,
    I: IntoIterator<Item = V>,
{
    cond_one(Expr::cust_with_values(sql, values))
}

/// NULL-safe equality on a nullable text column (`IFNULL(col, '') = ?`).
pub(super) fn text_eq(col: &str, value: &str) -> Condition {
    raw_vals(format!("IFNULL({col}, '') = ?"), [value.to_string()])
}

/// NULL-safe inequality on a nullable text column (`col IS NULL OR col <> ?`).
pub(super) fn text_ne(col: &str, value: &str) -> Condition {
    raw_vals(format!("({col} IS NULL OR {col} <> ?)"), [value.to_string()])
}

/// Escape LIKE metacharacters so user input matches literally (with `ESCAPE '\'`).
pub(crate) fn escape_like(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// SQL symbol for a comparison operator (`:` means equality in numeric contexts).
pub(super) fn cmp_sql(op: Op) -> &'static str {
    match op {
        Op::Colon | Op::Eq => "=",
        Op::Ne => "<>",
        Op::Gt => ">",
        Op::Ge => ">=",
        Op::Lt => "<",
        Op::Le => "<=",
    }
}

/// SQL that is true only when `col` holds a plain integer string.
pub(super) fn numeric_guard(col: &str) -> String {
    format!("{col} IS NOT NULL AND {col} GLOB '[0-9]*' AND {col} NOT GLOB '*[^0-9]*'")
}

//! Free-text filters: name / type / oracle substring and exact matches.

use sea_orm::Condition;
use sea_orm::sea_query::{Expr, SimpleExpr};

use super::common::{cond_one, escape_like};
use super::super::error::{SearchError, unsupported_op};
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
        Op::Colon | Op::Eq => Ok(cond_one(contains(col, value))),
        _ => Err(unsupported_op(key, op)),
    }
}

//! Numeric-ish filters: mana value, power / toughness / loyalty, prices, and
//! collector number.

use sea_orm::Condition;

use super::common::{cmp_sql, numeric_guard, raw, raw_vals, text_eq, text_ne};
use super::super::error::{SearchError, invalid, unsupported_op};
use super::super::lexer::Op;

// ----- mana value (cmc) -----

pub(super) fn cmc(op: Op, value: &str) -> Result<Condition, SearchError> {
    let lower = value.to_lowercase();
    if lower == "even" || lower == "odd" {
        // Parity is an equality-only predicate; a relational operator is meaningless.
        if !matches!(op, Op::Colon | Op::Eq) {
            return Err(unsupported_op("cmc", op));
        }
        let parity = if lower == "even" { 0 } else { 1 };
        let sql = format!(
            "(cmc IS NOT NULL AND cmc = CAST(cmc AS INTEGER) AND CAST(cmc AS INTEGER) % 2 = {parity})"
        );
        return Ok(raw(sql));
    }
    let n: f64 = value
        .parse()
        .map_err(|_| invalid("cmc", value, "expected a number"))?;
    let sql = format!("(cmc IS NOT NULL AND cmc {} ?)", cmp_sql(op));
    Ok(raw_vals(sql, [n]))
}

// ----- power / toughness / loyalty (text columns, can be *, 1+*, X) -----

fn stat_column(s: &str) -> Option<&'static str> {
    match s {
        "pow" | "power" => Some("power"),
        "tou" | "toughness" => Some("toughness"),
        "loy" | "loyalty" => Some("loyalty"),
        "def" | "defense" => Some("defense"),
        _ => None,
    }
}

pub(super) fn ptl(col: &str, key: &str, op: Op, value: &str) -> Result<Condition, SearchError> {
    // RHS may name another stat column (pow>tou): numeric compare, both guarded.
    if let Some(other) = stat_column(&value.to_lowercase()) {
        let sql = format!(
            "(({}) AND ({}) AND CAST({col} AS REAL) {} CAST({other} AS REAL))",
            numeric_guard(col),
            numeric_guard(other),
            cmp_sql(op),
        );
        return Ok(raw(sql));
    }
    match op {
        Op::Colon | Op::Eq => Ok(text_eq(col, value)),
        Op::Ne => Ok(text_ne(col, value)),
        Op::Gt | Op::Ge | Op::Lt | Op::Le => {
            let n: f64 = value
                .parse()
                .map_err(|_| invalid(key, value, "expected a number"))?;
            let sql = format!(
                "(({}) AND CAST({col} AS REAL) {} ?)",
                numeric_guard(col),
                cmp_sql(op)
            );
            Ok(raw_vals(sql, [n]))
        }
    }
}

/// `pt:`/`powtou:` — compare power + toughness (both numeric-guarded).
pub(super) fn pt(op: Op, value: &str) -> Result<Condition, SearchError> {
    let n: f64 = value
        .parse()
        .map_err(|_| invalid("pt", value, "expected a number"))?;
    let sql = format!(
        "(({}) AND ({}) AND CAST(power AS REAL) + CAST(toughness AS REAL) {} ?)",
        numeric_guard("power"),
        numeric_guard("toughness"),
        cmp_sql(op)
    );
    Ok(raw_vals(sql, [n]))
}

// ----- prices (text decimal columns) -----

pub(super) fn price(col: &str, key: &str, op: Op, value: &str) -> Result<Condition, SearchError> {
    let n: f64 = value
        .parse()
        .map_err(|_| invalid(key, value, "expected a number"))?;
    let sql = format!(
        "({col} IS NOT NULL AND {col} <> '' AND CAST({col} AS REAL) {} ?)",
        cmp_sql(op)
    );
    Ok(raw_vals(sql, [n]))
}

// ----- collector number -----

pub(super) fn collector_number(op: Op, value: &str) -> Result<Condition, SearchError> {
    match op {
        Op::Colon | Op::Eq => Ok(raw_vals(
            "lower(collector_number) = ?".to_string(),
            [value.to_lowercase()],
        )),
        Op::Ne => Ok(raw_vals(
            "lower(collector_number) <> ?".to_string(),
            [value.to_lowercase()],
        )),
        Op::Gt | Op::Ge | Op::Lt | Op::Le => {
            let n: i32 = value
                .parse()
                .map_err(|_| invalid("cn", value, "range requires a numeric collector number"))?;
            let sql = format!(
                "(collector_number_int IS NOT NULL AND collector_number_int {} ?)",
                cmp_sql(op)
            );
            Ok(raw_vals(sql, [n]))
        }
    }
}

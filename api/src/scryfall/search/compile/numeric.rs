//! Numeric-ish filters: mana value, power / toughness / loyalty, prices, and
//! collector number.

use sea_orm::Condition;

use super::super::error::{SearchError, invalid, unsupported_op};
use super::super::lexer::Op;
use super::common::{cmp_sql, raw, raw_vals, text_eq, text_ne};
use crate::db::Dialect;

// ----- mana value (cmc) -----

pub(super) fn cmc(dialect: Dialect, op: Op, value: &str) -> Result<Condition, SearchError> {
    let lower = value.to_lowercase();
    if lower == "even" || lower == "odd" {
        // Parity is an equality-only predicate; a relational operator is meaningless.
        if !matches!(op, Op::Colon | Op::Eq) {
            return Err(unsupported_op("cmc", op));
        }
        let parity = if lower == "even" { 0 } else { 1 };
        // The `cmc = CAST(cmc AS INTEGER)` guard restricts the parity test to
        // integer-valued cmc, so SQLite's truncating and Postgres's rounding CAST
        // both return that integer — identical on both backends, no FLOOR needed.
        let sql = format!(
            "(cmc IS NOT NULL AND cmc = CAST(cmc AS INTEGER) AND CAST(cmc AS INTEGER) % 2 = {parity})"
        );
        return Ok(raw(sql));
    }
    let n: f64 = value
        .parse()
        .map_err(|_| invalid("cmc", value, "expected a number"))?;
    let sql = format!("(cmc IS NOT NULL AND cmc {} ?)", cmp_sql(op));
    Ok(raw_vals(dialect, sql, [n]))
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

pub(super) fn ptl(
    dialect: Dialect,
    col: &str,
    key: &str,
    op: Op,
    value: &str,
) -> Result<Condition, SearchError> {
    // RHS may name another stat column (pow>tou): numeric compare, both guarded.
    // Each CAST is wrapped in its own integer-string-guarded CASE so Postgres never
    // evaluates `CAST('*' AS REAL)` (which it hard-errors on). The guards are ALSO
    // re-ANDed as total outer guards, so the whole leaf is total (0/1, never NULL) —
    // otherwise a non-numeric value (`*`, `X`, `1+*`) yields NULL and `NOT` (`-pow>tou`)
    // would drop the row instead of matching it (the pre-Postgres SQLite behaviour).
    if let Some(other) = stat_column(&value.to_lowercase()) {
        let g_col = dialect.integer_string_guard(col);
        let g_other = dialect.integer_string_guard(other);
        let lhs = format!("CASE WHEN {g_col} THEN CAST({col} AS REAL) ELSE NULL END");
        let rhs = format!("CASE WHEN {g_other} THEN CAST({other} AS REAL) ELSE NULL END");
        return Ok(raw(format!(
            "(({g_col}) AND ({g_other}) AND ({lhs} {} {rhs}))",
            cmp_sql(op)
        )));
    }
    match op {
        Op::Colon | Op::Eq => Ok(text_eq(dialect, col, value)),
        Op::Ne => Ok(text_ne(dialect, col, value)),
        Op::Gt | Op::Ge | Op::Lt | Op::Le => {
            let n: f64 = value
                .parse()
                .map_err(|_| invalid(key, value, "expected a number"))?;
            // The guard is re-ANDed outside the CASE so the leaf is total (0/1, never
            // NULL): a non-numeric value fails the outer guard, so `NOT` (`-pow>=5`)
            // matches it rather than dropping it (the pre-Postgres SQLite behaviour).
            let guard = dialect.integer_string_guard(col);
            let case = format!("CASE WHEN {guard} THEN CAST({col} AS REAL) ELSE NULL END");
            let sql = format!("(({guard}) AND ({case} {} ?))", cmp_sql(op));
            Ok(raw_vals(dialect, sql, [n]))
        }
    }
}

/// `pt:`/`powtou:` — compare power + toughness (both numeric-guarded). Each cast is
/// wrapped in its guarded CASE and summed; both guards are re-ANDed as total outer
/// guards, so the leaf is total (0/1, never NULL) — a non-numeric side makes the whole
/// leaf false (not NULL), so `NOT` (`-pt>5`) matches it. This restores the old
/// both-guards-must-hold negation behaviour on SQLite (a bare CASE sum went NULL).
pub(super) fn pt(dialect: Dialect, op: Op, value: &str) -> Result<Condition, SearchError> {
    let n: f64 = value
        .parse()
        .map_err(|_| invalid("pt", value, "expected a number"))?;
    let g_power = dialect.integer_string_guard("power");
    let g_toughness = dialect.integer_string_guard("toughness");
    let p = format!("CASE WHEN {g_power} THEN CAST(power AS REAL) ELSE NULL END");
    let t = format!("CASE WHEN {g_toughness} THEN CAST(toughness AS REAL) ELSE NULL END");
    let sql = format!(
        "(({g_power}) AND ({g_toughness}) AND (({p}) + ({t}) {} ?))",
        cmp_sql(op)
    );
    Ok(raw_vals(dialect, sql, [n]))
}

// ----- prices (text decimal columns) -----

pub(super) fn price(
    dialect: Dialect,
    col: &str,
    key: &str,
    op: Op,
    value: &str,
) -> Result<Condition, SearchError> {
    let n: f64 = value
        .parse()
        .map_err(|_| invalid(key, value, "expected a number"))?;
    // The CAST lives inside the CASE so Postgres never evaluates `CAST('' AS REAL)`
    // (it errors); the decimal-shape guard additionally keeps Postgres's strict CAST
    // from erroring on junk. The same guard is re-ANDed outside the CASE so the leaf is
    // total (0/1, never NULL): an unpriced card fails the guard, so `NOT` (`-usd<1`)
    // matches it rather than dropping it (the pre-Postgres SQLite behaviour). On SQLite
    // the guard is the historical non-empty check, so its rows are unchanged.
    let guard = dialect.decimal_string_guard(col);
    let case = format!("CASE WHEN {guard} THEN CAST({col} AS REAL) ELSE NULL END");
    let sql = format!("(({guard}) AND ({case} {} ?))", cmp_sql(op));
    Ok(raw_vals(dialect, sql, [n]))
}

// ----- collector number -----

pub(super) fn collector_number(
    dialect: Dialect,
    op: Op,
    value: &str,
) -> Result<Condition, SearchError> {
    match op {
        Op::Colon | Op::Eq => Ok(raw_vals(
            dialect,
            "lower(collector_number) = ?".to_string(),
            [value.to_lowercase()],
        )),
        Op::Ne => Ok(raw_vals(
            dialect,
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
            Ok(raw_vals(dialect, sql, [n]))
        }
    }
}

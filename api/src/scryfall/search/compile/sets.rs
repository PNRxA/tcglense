//! Set filters: `set:` (set code) and `settype:` (the set's Scryfall set_type).

use sea_orm::Condition;
use sea_orm::Value;

use super::super::error::{SearchError, invalid, unsupported_op};
use super::super::lexer::Op;
use super::common::{cmp_sql, raw_vals};
use crate::db::Dialect;

pub(super) fn set(dialect: Dialect, op: Op, value: &str) -> Result<Condition, SearchError> {
    match op {
        Op::Colon | Op::Eq => Ok(raw_vals(
            dialect,
            "set_code = ?".to_string(),
            [value.to_lowercase()],
        )),
        Op::Ne => Ok(raw_vals(
            dialect,
            "set_code <> ?".to_string(),
            [value.to_lowercase()],
        )),
        _ => Err(unsupported_op("set", op)),
    }
}

/// Map a Scryfall `st:` value to the provider's stored `set_type`. Most pass
/// through unchanged; a couple of Scryfall aliases differ from the stored name.
fn normalize_set_type(v: &str) -> String {
    match v.to_lowercase().as_str() {
        "boxset" => "box",
        "unset" => "funny",
        other => other,
    }
    .to_string()
}

/// `st:` / `settype:` — match a printing whose *set* has the given Scryfall
/// `set_type` (e.g. `expansion`, `commander`, `funny`). `set_type` lives on
/// `card_sets`, not `cards`, so we resolve it with a game-scoped subquery on the
/// set code. `set_code` is non-null, so `IN` / `NOT IN` stay total (0/1) and the
/// leaf negates cleanly. An unrecognised set type simply matches no rows (mirrors
/// Scryfall, and lets new provider set types work without a code change).
pub(super) fn set_type(dialect: Dialect, op: Op, value: &str) -> Result<Condition, SearchError> {
    let st = normalize_set_type(value);
    let select = "SELECT code FROM card_sets WHERE game = ? AND LOWER(COALESCE(set_type, '')) = ?";
    let bind = || {
        [
            Value::from(crate::scryfall::GAME.to_string()),
            Value::from(st.clone()),
        ]
    };
    match op {
        Op::Colon | Op::Eq => Ok(raw_vals(dialect, format!("set_code IN ({select})"), bind())),
        Op::Ne => Ok(raw_vals(
            dialect,
            format!("set_code NOT IN ({select})"),
            bind(),
        )),
        _ => Err(unsupported_op("settype", op)),
    }
}

/// `prints <op> N` — number of printings of this card (its `oracle_id` siblings).
pub(super) fn prints_filter(
    dialect: Dialect,
    op: Op,
    value: &str,
) -> Result<Condition, SearchError> {
    let n: i64 = value
        .parse()
        .map_err(|_| invalid("prints", value, "expected a number"))?;
    Ok(sibling_count(dialect, "COUNT(*)", op, n))
}

/// `sets`/`papersets <op> N` — number of distinct sets this card appears in
/// (equal here since the catalogue is paper-only).
pub(super) fn sets_filter(dialect: Dialect, op: Op, value: &str) -> Result<Condition, SearchError> {
    let n: i64 = value
        .parse()
        .map_err(|_| invalid("sets", value, "expected a number"))?;
    Ok(sibling_count(dialect, "COUNT(DISTINCT c2.set_code)", op, n))
}

/// A `GAME`-scoped correlated subquery over a card's `oracle_id` siblings (a card
/// with no `oracle_id` is its own sole sibling, so the count is always ≥ 1 and the
/// leaf stays total for `-`/`not:`). `agg` is a fixed aggregate; user input binds.
fn sibling_count(dialect: Dialect, agg: &str, op: Op, n: i64) -> Condition {
    let sql = format!(
        "(SELECT {agg} FROM cards c2 WHERE c2.game = ? AND \
         ((cards.oracle_id IS NOT NULL AND c2.oracle_id = cards.oracle_id) \
          OR (cards.oracle_id IS NULL AND c2.id = cards.id))) {} ?",
        cmp_sql(op)
    );
    raw_vals(
        dialect,
        sql,
        [
            Value::from(crate::scryfall::GAME.to_string()),
            Value::from(n),
        ],
    )
}

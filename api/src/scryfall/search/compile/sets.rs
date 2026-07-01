//! Set filters: `set:` (set code) and `settype:` (the set's Scryfall set_type).

use sea_orm::Condition;
use sea_orm::Value;

use super::common::raw_vals;
use super::super::error::{SearchError, unsupported_op};
use super::super::lexer::Op;

pub(super) fn set(op: Op, value: &str) -> Result<Condition, SearchError> {
    match op {
        Op::Colon | Op::Eq => Ok(raw_vals("set_code = ?".to_string(), [value.to_lowercase()])),
        Op::Ne => Ok(raw_vals("set_code <> ?".to_string(), [value.to_lowercase()])),
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
pub(super) fn set_type(op: Op, value: &str) -> Result<Condition, SearchError> {
    let st = normalize_set_type(value);
    let select = "SELECT code FROM card_sets WHERE game = ? AND LOWER(IFNULL(set_type, '')) = ?";
    let bind = || {
        [
            Value::from(crate::scryfall::GAME.to_string()),
            Value::from(st.clone()),
        ]
    };
    match op {
        Op::Colon | Op::Eq => Ok(raw_vals(format!("set_code IN ({select})"), bind())),
        Op::Ne => Ok(raw_vals(format!("set_code NOT IN ({select})"), bind())),
        _ => Err(unsupported_op("settype", op)),
    }
}

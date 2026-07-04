//! Enumerated-value filters: language, layout, game, and oracle id.

use sea_orm::Condition;

use crate::db::Dialect;
use super::common::{raw, raw_vals, text_eq, text_ne};
use super::super::error::{SearchError, unsupported_op};
use super::super::lexer::Op;

fn lang_code(lower: &str) -> String {
    match lower {
        "english" => "en",
        "japanese" => "ja",
        "german" => "de",
        "french" => "fr",
        "italian" => "it",
        "spanish" => "es",
        "portuguese" => "pt",
        "russian" => "ru",
        "korean" => "ko",
        "chinese simplified" => "zhs",
        "chinese traditional" => "zht",
        other => other,
    }
    .to_string()
}

pub(super) fn lang(dialect: Dialect, op: Op, value: &str) -> Result<Condition, SearchError> {
    match op {
        Op::Colon | Op::Eq => {
            let lower = value.to_lowercase();
            if lower == "any" || lower == "*" {
                return Ok(Condition::all());
            }
            Ok(raw_vals(dialect, "lang = ?".to_string(), [lang_code(&lower)]))
        }
        _ => Err(unsupported_op("lang", op)),
    }
}

pub(super) fn layout(dialect: Dialect, op: Op, value: &str) -> Result<Condition, SearchError> {
    match op {
        Op::Colon | Op::Eq => Ok(text_eq(dialect, "layout", &value.to_lowercase())),
        _ => Err(unsupported_op("layout", op)),
    }
}

pub(super) fn game(op: Op, value: &str) -> Result<Condition, SearchError> {
    match op {
        Op::Colon | Op::Eq => match value.to_lowercase().as_str() {
            // Catalogue is paper-only: paper matches all, other engines match none.
            "paper" => Ok(Condition::all()),
            _ => Ok(raw("1 = 0")),
        },
        _ => Err(unsupported_op("game", op)),
    }
}

pub(super) fn oracleid(dialect: Dialect, op: Op, value: &str) -> Result<Condition, SearchError> {
    match op {
        Op::Colon | Op::Eq => Ok(text_eq(dialect, "oracle_id", value)),
        Op::Ne => Ok(text_ne(dialect, "oracle_id", value)),
        _ => Err(unsupported_op("oracleid", op)),
    }
}

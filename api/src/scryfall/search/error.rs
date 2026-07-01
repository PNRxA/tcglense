//! Search error type and its constructors, plus the mapping to the HTTP layer.

use crate::error::AppError;
use super::lexer::Op;

/// A query that could not be parsed or that uses an unsupported filter. The
/// `Display` text is user-facing (surfaced verbatim as the 422 body).
#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    #[error("search query ended unexpectedly")]
    UnexpectedEof,
    #[error("unexpected '{0}' in search query")]
    UnexpectedToken(String),
    #[error("unbalanced parentheses in search query")]
    UnbalancedParen,
    #[error("unterminated quoted text in search query")]
    UnterminatedString,
    #[error("empty parentheses in search query")]
    EmptyGroup,
    #[error("a search operator is missing its field name")]
    MissingKey,
    #[error("filter '{key}' is missing a value after '{op}'")]
    MissingValue { key: String, op: Op },
    #[error("unknown search filter '{0}'")]
    UnknownKey(String),
    #[error("search filter '{0}' is not supported")]
    UnsupportedKey(String),
    #[error("filter '{key}' does not support the '{op}' operator")]
    UnsupportedOperator { key: String, op: Op },
    #[error("invalid value '{value}' for '{key}': {reason}")]
    InvalidValue {
        key: String,
        value: String,
        reason: String,
    },
    #[error("search query is too complex")]
    TooComplex,
}

impl From<SearchError> for AppError {
    fn from(err: SearchError) -> Self {
        AppError::Validation(err.to_string())
    }
}

pub(super) fn invalid(key: &str, value: &str, reason: &str) -> SearchError {
    SearchError::InvalidValue {
        key: key.to_string(),
        value: value.to_string(),
        reason: reason.to_string(),
    }
}

pub(super) fn unsupported_op(key: &str, op: Op) -> SearchError {
    SearchError::UnsupportedOperator {
        key: key.to_string(),
        op,
    }
}

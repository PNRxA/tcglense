//! Import failure type and its mapping onto the app's HTTP error responses.

use crate::error::AppError;

/// A failure while importing a collection. Converts to the right `AppError` (and thus
/// HTTP status + JSON body) via the `From` impl below.
#[derive(Debug)]
pub enum ImportError {
    /// The source string couldn't be parsed into a collection id -> 422.
    InvalidSource(String),
    /// The provider has no public collection at that id -> 404.
    CollectionNotFound(String),
    /// The provider collection has no cards to import -> 422 (guards a `Replace` from
    /// silently wiping the user's collection against an empty/misresolved source).
    EmptyCollection,
    /// A `Replace` matched none of our catalog -> 422. Guards against wiping the whole
    /// collection when the source's cards simply aren't in our catalog (e.g. the
    /// catalog hasn't been synced), rather than deleting everything and importing nothing.
    NoMatchingCards,
    /// The collection is larger than we'll import in one request -> 422.
    TooLarge { count: usize, max: usize },
    /// The provider kept rate-limiting us (`429`) even after backing off -> 503.
    RateLimited,
    /// The provider request or response parse failed -> 502.
    Upstream(String),
    /// A local database error -> 500.
    Db(sea_orm::DbErr),
}

impl From<ImportError> for AppError {
    fn from(err: ImportError) -> Self {
        match err {
            ImportError::InvalidSource(msg) => AppError::Validation(msg),
            ImportError::CollectionNotFound(id) => {
                AppError::NotFound(format!("no public collection found for '{id}'"))
            }
            ImportError::EmptyCollection => {
                AppError::Validation("the collection has no cards to import".to_string())
            }
            ImportError::NoMatchingCards => AppError::Validation(
                "none of the collection's cards are in our catalog, so there was nothing to \
                 import (your collection was left unchanged)"
                    .to_string(),
            ),
            ImportError::TooLarge { count, max } => AppError::Validation(format!(
                "collection is too large to import ({count} cards; the limit is {max})"
            )),
            ImportError::RateLimited => AppError::ServiceUnavailable(
                "the collection provider is rate-limiting us; please try again in a few minutes"
                    .to_string(),
            ),
            ImportError::Upstream(detail) => {
                // Log the upstream detail server-side; return a generic gateway error.
                tracing::warn!(error = %detail, "collection provider request failed");
                AppError::BadGateway("the collection provider could not be reached".to_string())
            }
            ImportError::Db(err) => AppError::from(err),
        }
    }
}

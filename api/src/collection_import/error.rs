//! Import failure type and its mapping onto the app's HTTP error responses.

use crate::error::AppError;

/// A failure while importing a collection or deck. Converts to the right `AppError` (and thus
/// HTTP status + JSON body) via the `From` impl below.
#[derive(Debug)]
pub enum ImportError {
    /// The source string couldn't be parsed into a collection id -> 422.
    InvalidSource(String),
    /// The provider has no public collection at that id -> 404.
    CollectionNotFound(String),
    /// The provider has no public deck at that id -> 404.
    DeckNotFound(String),
    /// The provider collection has no cards to import -> 422 (guards a `Replace` from
    /// silently wiping the user's collection against an empty/misresolved source).
    EmptyCollection,
    /// The fetched/uploaded deck has no usable card rows -> 422.
    EmptyDeck,
    /// A `Replace` matched none of our catalog -> 422. Guards against wiping the whole
    /// collection when the source's cards simply aren't in our catalog (e.g. the
    /// catalog hasn't been synced), rather than deleting everything and importing nothing.
    NoMatchingCards,
    /// No imported deck card matched the local catalog -> 422; do not create an empty deck.
    NoMatchingDeckCards,
    /// The source is larger than we'll import in one request -> 422.
    TooLarge { count: usize, max: usize },
    /// The provider kept rate-limiting us (`429`) even after backing off -> 503.
    RateLimited,
    /// The provider refused to serve us at all (e.g. Moxfield's bot wall rejecting an
    /// unapproved User-Agent) -> 502, with the actionable detail passed through —
    /// unlike [`Upstream`](Self::Upstream), this is a deployment-configuration problem
    /// the message helps fix, not incidental upstream detail to hide.
    ProviderDenied(String),
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
            ImportError::DeckNotFound(id) => {
                AppError::NotFound(format!("no public deck found for '{id}'"))
            }
            ImportError::EmptyCollection => {
                AppError::Validation("the collection has no cards to import".to_string())
            }
            ImportError::EmptyDeck => {
                AppError::Validation("the deck has no cards to import".to_string())
            }
            ImportError::NoMatchingCards => AppError::Validation(
                "none of the collection's cards are in our catalog, so there was nothing to \
                 import (your collection was left unchanged)"
                    .to_string(),
            ),
            ImportError::NoMatchingDeckCards => AppError::Validation(
                "none of the deck's cards are in our catalog, so no deck was created"
                    .to_string(),
            ),
            ImportError::TooLarge { count, max } => AppError::Validation(format!(
                "source is too large to import ({count} cards; the limit is {max})"
            )),
            ImportError::RateLimited => AppError::ServiceUnavailable(
                "the provider is rate-limiting us; please try again in a few minutes"
                    .to_string(),
            ),
            ImportError::ProviderDenied(detail) => {
                tracing::warn!(error = %detail, "import provider denied our request");
                AppError::BadGateway(detail)
            }
            ImportError::Upstream(detail) => {
                // Log the upstream detail server-side; return a generic gateway error.
                tracing::warn!(error = %detail, "import provider request failed");
                AppError::BadGateway("the import provider could not be reached".to_string())
            }
            ImportError::Db(err) => AppError::from(err),
        }
    }
}

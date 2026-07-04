//! Error type for the TCGCSV backfill. It runs on a background task with no
//! request path, so — like `scryfall::ingest::IngestError` — it is logged, never
//! turned into an HTTP response.

/// Failure modes of the historic price backfill.
#[derive(Debug, thiserror::Error)]
pub enum BackfillError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("database error: {0}")]
    Db(#[from] sea_orm::DbErr),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    /// A `7z` archive that couldn't be opened or decoded (stringified because the
    /// `sevenz_rust2::Error` type isn't part of our public surface).
    #[error("7z archive error: {0}")]
    Archive(String),
    /// A background blocking task (7z decode) panicked or was cancelled.
    #[error("worker task error: {0}")]
    Join(#[from] tokio::task::JoinError),
}

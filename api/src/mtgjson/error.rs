//! Error type for the MTGJSON sealed-contents ingest. It's only ever surfaced to
//! `catalog::refresh_all` (which logs it), so it needs `Display`, not an `AppError`
//! conversion.

/// Anything that can go wrong fetching or ingesting MTGJSON sealed contents.
#[derive(Debug, thiserror::Error)]
pub enum MtgjsonError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("database error: {0}")]
    Db(#[from] sea_orm::DbErr),
    #[error("failed to read the AllPrintings stream: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse AllPrintings: {0}")]
    Parse(String),
    #[error("parse task failed: {0}")]
    Join(String),
}

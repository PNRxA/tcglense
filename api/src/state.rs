use std::sync::Arc;

use sea_orm::DatabaseConnection;

use crate::catalog::images::ImageCache;
use crate::config::Config;

/// Shared, cheaply-clonable application state passed to every handler.
#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    pub config: Arc<Config>,
    /// Precomputed Argon2 hash of a fixed dummy password, used by login to
    /// equalize timing on the user-not-found path (mitigating user enumeration).
    /// Computed once at startup so the request path can never degrade to a
    /// fast no-op if hashing were to fail.
    pub dummy_password_hash: Arc<str>,
    /// Lazy on-disk cache + downloader for card images.
    pub images: Arc<ImageCache>,
}

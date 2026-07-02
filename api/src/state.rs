use std::sync::Arc;

use sea_orm::DatabaseConnection;

use crate::auth::password::hash_password;
use crate::catalog::images::ImageCache;
use crate::collection_import::ProviderSettings;
use crate::collection_import::jobs::ImportQueue;
use crate::config::Config;
use crate::error::AppError;

/// The fixed plaintext whose Argon2 hash backs the login timing-equalizer (see
/// `dummy_password_hash`). Its value is irrelevant to security — only the cost of
/// verifying against its hash matters — but it is the one canonical constant so
/// production and the test harness stay identically wired.
const TIMING_EQUALIZER_PLAINTEXT: &str = "tcglense-timing-equalizer";

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
    /// Shared outbound HTTP client for request-path provider calls (e.g. importing a
    /// collection from Archidekt). Follows redirects and carries the app User-Agent;
    /// `reqwest::Client` is internally reference-counted, so cloning it is cheap.
    pub http: reqwest::Client,
    /// Background queue + global rate limiter for collection imports/syncs (they run
    /// asynchronously because the provider rate limit makes them slow).
    pub imports: Arc<ImportQueue>,
}

impl AppState {
    /// Assemble the shared state from its already-built dependencies, owning the
    /// wiring that must stay identical between production (`main.rs`) and the
    /// security-test harness: the precomputed timing-equalizer hash, the on-disk
    /// image cache rooted at `DATA_DIR/images`, and the background import queue.
    ///
    /// `http` follows redirects (provider data/import calls); `image_http` disables
    /// them (the image proxy). Returns `Err` only if hashing the timing-equalizer
    /// constant fails, so callers `.expect` it at startup rather than degrade the
    /// timing defense to a fast no-op.
    pub fn new(
        config: Config,
        db: DatabaseConnection,
        http: reqwest::Client,
        image_http: reqwest::Client,
    ) -> Result<Self, AppError> {
        let dummy_password_hash: Arc<str> = hash_password(TIMING_EQUALIZER_PLAINTEXT)?.into();
        let images = Arc::new(ImageCache::new(config.data_dir.join("images"), image_http));
        // Deployment-level provider settings (e.g. Moxfield's approved User-Agent) are
        // captured into the import queue so background workers don't need the config.
        let imports = Arc::new(ImportQueue::default().with_settings(ProviderSettings {
            moxfield_user_agent: config.moxfield_user_agent.clone(),
        }));
        Ok(Self {
            db,
            config: Arc::new(config),
            dummy_password_hash,
            images,
            http,
            imports,
        })
    }
}

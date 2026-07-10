use std::sync::{Arc, RwLock};

use sea_orm::{ConnectionTrait, DatabaseConnection};

use crate::auth::password::hash_password;
use crate::captcha::Captcha;
use crate::catalog::fingerprints::FingerprintIndex;
use crate::catalog::images::ImageCache;
use crate::collection_import::ProviderSettings;
use crate::collection_import::jobs::ImportQueue;
use crate::config::Config;
use crate::email::Emailer;
use crate::error::AppError;
use crate::ratelimit::{AuthRateLimiter, UserRateLimiter};

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
    /// Background queue + per-provider rate limiters for collection imports/syncs (they run
    /// asynchronously because the provider rate limits make them slow).
    pub imports: Arc<ImportQueue>,
    /// Outbound transactional email (verification / password-reset links); a
    /// logging no-op when no provider key is configured.
    pub email: Arc<Emailer>,
    /// CAPTCHA verifier for the auth endpoints; a pass-through when no key is set.
    pub captcha: Arc<Captcha>,
    /// Per-IP rate limiters for the auth endpoints (in-memory, or Redis-backed when
    /// `REDIS_URL` is configured; see [`crate::ratelimit::AuthRateLimiter`]).
    pub rate_limiters: Arc<AuthRateLimiter>,
    /// Per-user rate limiters for the authenticated API surface (collection + `me`),
    /// keyed by the access-token user id (in-memory, or Redis-backed; see
    /// [`crate::ratelimit::UserRateLimiter`]).
    pub user_rate_limiters: Arc<UserRateLimiter>,
    /// The visual scanner's in-memory perceptual-hash match index. Empty until loaded
    /// from the `card_fingerprint` table at startup (and rebuilt after each build /
    /// sync pass) by [`crate::tasks`]. Read behind the lock — each scan clones the
    /// inner `Arc` and matches against it without holding the lock (see
    /// [`Self::fingerprint_index`]); a rebuild swaps the inner `Arc` wholesale.
    pub fingerprint_index: Arc<RwLock<Arc<FingerprintIndex>>>,
}

impl AppState {
    /// Assemble the shared state from its already-built dependencies, owning the
    /// wiring that must stay identical between production (`main.rs`) and the
    /// security-test harness: the precomputed timing-equalizer hash, the on-disk
    /// image cache rooted at `DATA_DIR/images`, and the background import queue.
    ///
    /// `http` follows redirects (provider data/import calls); `image_http` disables
    /// them (the image proxy). `redis` is the optional shared Redis connection
    /// backing the rate limiters (`Some` only when `REDIS_URL` is set and Redis was
    /// reachable at boot; `None` = in-memory / per-process). Returns `Err` only if
    /// hashing the timing-equalizer constant fails, so callers `.expect` it at
    /// startup rather than degrade the timing defense to a fast no-op.
    pub fn new(
        config: Config,
        db: DatabaseConnection,
        http: reqwest::Client,
        image_http: reqwest::Client,
        redis: Option<redis::aio::ConnectionManager>,
    ) -> Result<Self, AppError> {
        let dummy_password_hash: Arc<str> = hash_password(TIMING_EQUALIZER_PLAINTEXT)?.into();
        let images = Arc::new(ImageCache::new(
            config.data_dir.join("images"),
            image_http,
            config.cdn_mode,
        ));
        // Deployment-level provider settings (e.g. Moxfield's approved User-Agent) are
        // captured into the import queue so background workers don't need the config.
        let imports = Arc::new(ImportQueue::default().with_settings(ProviderSettings {
            moxfield_user_agent: config.moxfield_user_agent.clone(),
        }));
        // Sends ride the shared client (with a per-request timeout); the key and
        // From address are captured here so handlers just call `state.email`.
        let email = Arc::new(Emailer::from_config(&config, http.clone()));
        // CAPTCHA verifier (Turnstile, or a pass-through when unconfigured) rides
        // the same shared client; the per-IP auth limiters are in-memory.
        let captcha = Arc::new(Captcha::from_config(&config, http.clone()));
        // Redis-backed when a connection was established at boot, else in-memory.
        // `ConnectionManager: Clone`, so both limiter sets share the one multiplexed
        // connection.
        let rate_limiters = Arc::new(AuthRateLimiter::new(redis.clone()));
        let user_rate_limiters = Arc::new(UserRateLimiter::new(redis));
        Ok(Self {
            db,
            config: Arc::new(config),
            dummy_password_hash,
            images,
            http,
            imports,
            email,
            captcha,
            rate_limiters,
            user_rate_limiters,
            fingerprint_index: Arc::new(RwLock::new(Arc::new(FingerprintIndex::default()))),
        })
    }

    /// The SQL [`Dialect`] of the live connection, for handlers compiling
    /// backend-specific SQL.
    pub fn dialect(&self) -> crate::db::Dialect {
        crate::db::Dialect::from_backend(self.db.get_database_backend())
    }

    /// A snapshot of the current fingerprint match index. Clones the inner `Arc` under
    /// a brief read lock and returns it, so the caller matches against a stable index
    /// without holding the lock across the (sync, few-ms) Hamming scan — and a
    /// concurrent rebuild that swaps the inner `Arc` never disturbs an in-flight scan.
    pub fn fingerprint_index(&self) -> Arc<FingerprintIndex> {
        self.fingerprint_index
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Swap in a freshly-built fingerprint match index (called after a build / sync
    /// pass loads the current fingerprints from the table).
    pub fn set_fingerprint_index(&self, index: FingerprintIndex) {
        *self
            .fingerprint_index
            .write()
            .unwrap_or_else(|e| e.into_inner()) = Arc::new(index);
    }
}

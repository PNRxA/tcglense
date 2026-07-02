//! Background import jobs + the global import queue.
//!
//! Because the provider rate limit (see [`super::rate_limit`]) makes an import take
//! well over a minute, imports don't run inline in the request. Instead the handler
//! [`spawn_import_job`]s a background task and returns a job id immediately; the client
//! polls the job's status (`queued` → `running` → `complete`/`error`).
//!
//! Concurrency is capped at one import at a time ([`MAX_CONCURRENT_IMPORTS`]) so imports
//! form a simple global queue — a job waiting for the slot reports `queued`. The shared
//! [`RateLimiter`] additionally bounds the request rate across everything.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use chrono::Utc;
use sea_orm::sea_query::Expr;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use tokio::sync::Semaphore;
use tokio::time::Instant;

use super::rate_limit::RateLimiter;
use super::{
    ImportSummary, Provider, ProviderContext, ProviderSettings, ReconcileMode, execute_import,
};
use crate::entities::collection_source;
use crate::error::AppError;

/// How long a finished job's result is retained for the client to poll before pruning.
const JOB_TTL: Duration = Duration::from_secs(15 * 60);
/// Upper bound on tracked jobs, to cap memory and abuse.
const MAX_TRACKED_JOBS: usize = 200;
/// Imports that run at once. One keeps them a simple global queue; with the provider
/// rate limit, more concurrency wouldn't finish faster (they'd share one request budget).
const MAX_CONCURRENT_IMPORTS: usize = 1;
/// Requests/minute we allow to the collection provider, across all imports.
pub const PROVIDER_REQUESTS_PER_MINUTE: u32 = 20;

/// A job's lifecycle status. Cloned out under the lock for the status endpoint.
#[derive(Clone)]
pub enum JobStatus {
    /// Accepted, waiting for the import slot.
    Queued,
    /// Actively fetching + reconciling.
    Running,
    /// Finished successfully, carrying the import summary.
    Complete(ImportSummary),
    /// Failed, carrying a user-facing message.
    Failed(String),
}

struct Job {
    user_id: i32,
    game: String,
    status: JobStatus,
    updated_at: Instant,
}

/// The process-wide import queue: the job registry, the id counter, the shared provider
/// rate limiter, and the concurrency slots. Held in `AppState` behind an `Arc`.
pub struct ImportQueue {
    jobs: Mutex<HashMap<u64, Job>>,
    next_id: AtomicU64,
    limiter: RateLimiter,
    permits: Arc<Semaphore>,
    /// Deployment-level provider settings (e.g. Moxfield's approved User-Agent),
    /// captured at startup so background workers don't need the full app config.
    settings: ProviderSettings,
}

impl Default for ImportQueue {
    fn default() -> Self {
        Self::new(PROVIDER_REQUESTS_PER_MINUTE)
    }
}

impl ImportQueue {
    /// Build a queue whose provider requests are capped at `requests_per_minute`.
    pub fn new(requests_per_minute: u32) -> Self {
        Self {
            jobs: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            limiter: RateLimiter::per_minute(requests_per_minute),
            permits: Arc::new(Semaphore::new(MAX_CONCURRENT_IMPORTS)),
            settings: ProviderSettings::default(),
        }
    }

    /// Attach deployment-level provider settings (builder-style, used at startup).
    pub fn with_settings(mut self, settings: ProviderSettings) -> Self {
        self.settings = settings;
        self
    }

    /// A job's status, scoped to its owner + game — anyone else (or an unknown id) gets
    /// `None`, which the handler renders as a 404 so job ids aren't cross-user probes.
    pub fn status(&self, id: u64, user_id: i32, game: &str) -> Option<JobStatus> {
        let jobs = self.jobs.lock().expect("import jobs mutex poisoned");
        jobs.get(&id)
            .filter(|j| j.user_id == user_id && j.game == game)
            .map(|j| j.status.clone())
    }

    /// Register a new queued job, pruning stale finished ones first. Returns the id, or a
    /// 503 when too many jobs are tracked.
    fn create(&self, user_id: i32, game: &str) -> Result<u64, AppError> {
        let mut jobs = self.jobs.lock().expect("import jobs mutex poisoned");
        let now = Instant::now();
        jobs.retain(|_, j| {
            matches!(j.status, JobStatus::Queued | JobStatus::Running)
                || now.duration_since(j.updated_at) < JOB_TTL
        });
        if jobs.len() >= MAX_TRACKED_JOBS {
            return Err(AppError::ServiceUnavailable(
                "too many collection imports in progress; please try again shortly".to_string(),
            ));
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        jobs.insert(
            id,
            Job {
                user_id,
                game: game.to_string(),
                status: JobStatus::Queued,
                updated_at: now,
            },
        );
        Ok(id)
    }

    fn set(&self, id: u64, status: JobStatus) {
        if let Some(j) = self
            .jobs
            .lock()
            .expect("import jobs mutex poisoned")
            .get_mut(&id)
        {
            j.status = status;
            j.updated_at = Instant::now();
        }
    }
}

/// Everything a background import needs, captured after synchronous validation.
pub struct ImportRequest {
    pub user_id: i32,
    pub game: String,
    pub provider: Provider,
    pub collection_id: String,
    pub mode: ReconcileMode,
    /// Whether to stamp the saved source's `last_synced_at` on success (a re-sync).
    pub stamp_source_synced: bool,
}

/// Queue an import and spawn its background worker; returns the job id immediately. The
/// worker waits for the concurrency slot (reporting `queued`), then fetches + reconciles
/// throttled by the shared provider rate limiter, recording the outcome on the job.
pub fn spawn_import_job(
    db: DatabaseConnection,
    http: reqwest::Client,
    imports: Arc<ImportQueue>,
    request: ImportRequest,
) -> Result<u64, AppError> {
    let id = imports.create(request.user_id, &request.game)?;

    tokio::spawn(async move {
        // Wait for the import slot — this is the queue; status stays `queued` until now.
        // `acquire_owned` only errors if the semaphore is closed, which we never do.
        let Ok(_permit) = imports.permits.clone().acquire_owned().await else {
            imports.set(id, JobStatus::Failed("import queue unavailable".to_string()));
            return;
        };
        imports.set(id, JobStatus::Running);

        let ctx = ProviderContext {
            http: &http,
            limiter: &imports.limiter,
            settings: &imports.settings,
        };
        let result = execute_import(
            &db,
            &ctx,
            request.user_id,
            &request.game,
            request.provider,
            &request.collection_id,
            request.mode,
        )
        .await;

        match result {
            Ok(summary) => {
                if request.stamp_source_synced
                    && let Err(err) = stamp_last_synced(&db, request.user_id, &request.game).await
                {
                    tracing::warn!(error = %err, "failed to stamp collection sync time");
                }
                imports.set(id, JobStatus::Complete(summary));
            }
            Err(err) => {
                // `From<ImportError>` logs upstream detail and yields a safe message.
                let message = AppError::from(err).to_string();
                imports.set(id, JobStatus::Failed(message));
            }
        }
    });

    Ok(id)
}

/// Stamp `last_synced_at` (and `updated_at`) on the user's saved source for a game, on a
/// successful re-sync. Best-effort — a failure here doesn't fail the import.
async fn stamp_last_synced(
    db: &DatabaseConnection,
    user_id: i32,
    game: &str,
) -> Result<(), sea_orm::DbErr> {
    let now = Utc::now();
    collection_source::Entity::update_many()
        .col_expr(collection_source::Column::LastSyncedAt, Expr::value(now))
        .col_expr(collection_source::Column::UpdatedAt, Expr::value(now))
        .filter(collection_source::Column::UserId.eq(user_id))
        .filter(collection_source::Column::Game.eq(game))
        .exec(db)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary() -> ImportSummary {
        ImportSummary {
            provider: "archidekt",
            mode: ReconcileMode::Replace,
            total_rows: 1,
            distinct_cards: 1,
            matched_cards: 1,
            unmatched_cards: 0,
            unmatched_sample: vec![],
            regular_copies: 1,
            foil_copies: 0,
            removed_cards: 0,
            stopped_early: false,
        }
    }

    #[test]
    fn create_then_status_is_queued_and_owner_scoped() {
        let q = ImportQueue::default();
        let id = q.create(1, "mtg").expect("create");
        assert!(matches!(q.status(id, 1, "mtg"), Some(JobStatus::Queued)));
        // Wrong user, wrong game, or unknown id all read as absent (=> 404).
        assert!(q.status(id, 2, "mtg").is_none());
        assert!(q.status(id, 1, "pokemon").is_none());
        assert!(q.status(id + 999, 1, "mtg").is_none());
    }

    #[test]
    fn set_transitions_status() {
        let q = ImportQueue::default();
        let id = q.create(7, "mtg").expect("create");
        q.set(id, JobStatus::Running);
        assert!(matches!(q.status(id, 7, "mtg"), Some(JobStatus::Running)));
        q.set(id, JobStatus::Complete(summary()));
        assert!(matches!(q.status(id, 7, "mtg"), Some(JobStatus::Complete(_))));
    }

    #[test]
    fn ids_are_unique() {
        let q = ImportQueue::default();
        let a = q.create(1, "mtg").unwrap();
        let b = q.create(1, "mtg").unwrap();
        assert_ne!(a, b);
    }
}

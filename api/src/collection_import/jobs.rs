//! Background import jobs + the global import queue.
//!
//! Because the provider rate limit (see [`super::rate_limit`]) makes an import take
//! well over a minute, imports don't run inline in the request. Instead the handler
//! [`spawn_import_job`]s a background task and returns a job id immediately; the client
//! polls the job's status (`queued` → `running` → `complete`/`error`).
//!
//! Concurrency is capped at one import at a time ([`MAX_CONCURRENT_IMPORTS`]) so imports
//! form a simple global queue — a job waiting for the slot reports `queued`. The
//! per-provider [`ProviderLimiters`] additionally bound each provider's request rate
//! across every import that talks to it.

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

use super::rate_limit::ProviderLimiters;
use super::{
    ImportSummary, ProgressReporter, ProgressSnapshot, Provider, ProviderContext, ProviderSettings,
    ReconcileMode, execute_import,
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
    /// Live fetch progress, shared with the running worker (which writes it). Read into a
    /// snapshot by [`ImportQueue::view`] for the status endpoint.
    progress: Arc<ProgressReporter>,
    updated_at: Instant,
}

/// A point-in-time view of a job for the status endpoint: its lifecycle status plus a
/// snapshot of its live fetch progress.
pub struct JobView {
    pub status: JobStatus,
    pub progress: ProgressSnapshot,
}

/// The process-wide import queue: the job registry, the id counter, the per-provider
/// rate limiters, and the concurrency slots. Held in `AppState` behind an `Arc`.
pub struct ImportQueue {
    jobs: Mutex<HashMap<u64, Job>>,
    next_id: AtomicU64,
    limiters: ProviderLimiters,
    permits: Arc<Semaphore>,
    /// Deployment-level provider settings (e.g. Moxfield's approved User-Agent),
    /// captured at startup so background workers don't need the full app config.
    settings: ProviderSettings,
}

impl Default for ImportQueue {
    fn default() -> Self {
        Self::new(ProviderLimiters::default())
    }
}

impl ImportQueue {
    /// Build a queue whose provider requests are governed by the given per-provider limiters.
    pub fn new(limiters: ProviderLimiters) -> Self {
        Self {
            jobs: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            limiters,
            permits: Arc::new(Semaphore::new(MAX_CONCURRENT_IMPORTS)),
            settings: ProviderSettings::default(),
        }
    }

    /// Attach deployment-level provider settings (builder-style, used at startup).
    pub fn with_settings(mut self, settings: ProviderSettings) -> Self {
        self.settings = settings;
        self
    }

    /// Shared outbound provider limiters. Deck imports are single-request, inline
    /// operations, but they still consume the same provider request budget as collection
    /// imports so the two surfaces cannot independently exceed an upstream limit.
    pub(crate) fn limiters(&self) -> &ProviderLimiters {
        &self.limiters
    }

    /// Deployment-level provider settings shared with inline deck imports (currently the
    /// approved Moxfield User-Agent, when configured).
    pub(crate) fn settings(&self) -> &ProviderSettings {
        &self.settings
    }

    /// A job's status + live progress, scoped to its owner + game — anyone else (or an
    /// unknown id) gets `None`, which the handler renders as a 404 so job ids aren't
    /// cross-user probes.
    pub fn view(&self, id: u64, user_id: i32, game: &str) -> Option<JobView> {
        let jobs = self.jobs.lock().expect("import jobs mutex poisoned");
        jobs.get(&id)
            .filter(|j| j.user_id == user_id && j.game == game)
            .map(|j| JobView {
                status: j.status.clone(),
                progress: j.progress.snapshot(),
            })
    }

    /// Register a new queued job, pruning stale finished ones first. Returns the id and the
    /// shared progress reporter the worker publishes into, or a 503 when too many jobs are
    /// tracked.
    fn create(&self, user_id: i32, game: &str) -> Result<(u64, Arc<ProgressReporter>), AppError> {
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
        let progress = Arc::new(ProgressReporter::default());
        jobs.insert(
            id,
            Job {
                user_id,
                game: game.to_string(),
                status: JobStatus::Queued,
                progress: progress.clone(),
                updated_at: now,
            },
        );
        Ok((id, progress))
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
}

/// Queue an import and spawn its background worker; returns the job id immediately. The
/// worker waits for the concurrency slot (reporting `queued`), then fetches + reconciles
/// throttled by the provider's own rate limiter (from `ProviderLimiters`), recording the
/// outcome on the job.
pub fn spawn_import_job(
    db: DatabaseConnection,
    http: reqwest::Client,
    imports: Arc<ImportQueue>,
    analytics: Arc<crate::analytics_cache::AnalyticsCache>,
    request: ImportRequest,
) -> Result<u64, AppError> {
    let (id, progress) = imports.create(request.user_id, &request.game)?;

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
            limiters: &imports.limiters,
            settings: &imports.settings,
            progress: progress.as_ref(),
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

        // Orphan the user's cached analytics bodies (#413) on success AND failure:
        // the import pipeline commits mutations before its outcome is known (the
        // pre-fetch star-holding fold, the reconcile's own transactions), so a
        // failed job may still have changed the holdings. A spurious bump costs
        // one cache miss; a missed one serves stale analytics for the body TTL.
        analytics
            .bump_holdings(request.user_id, &request.game)
            .await;

        match result {
            Ok(summary) => {
                // A successful fetch is a sync of whatever saved link points at this exact
                // collection: stamp its `last_synced_at`. Matching on (provider, external_id)
                // means a one-off import of the saved collection (or a re-sync of it) marks
                // the link synced, while importing an *unrelated* collection leaves it alone.
                // Best-effort — a stamp failure never fails the import.
                if let Err(err) = stamp_last_synced(
                    &db,
                    request.user_id,
                    &request.game,
                    request.provider,
                    &request.collection_id,
                )
                .await
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

/// Stamp `last_synced_at` (and `updated_at`) on the saved source that points at the
/// just-imported collection — matched on `(user, game, provider, external_id)` — so a
/// successful import or re-sync of a saved link marks it synced, while importing some
/// *other* collection leaves the saved link's marker untouched. A no-op (zero rows) when
/// no saved link matches. Best-effort — a failure here doesn't fail the import.
async fn stamp_last_synced(
    db: &DatabaseConnection,
    user_id: i32,
    game: &str,
    provider: Provider,
    external_id: &str,
) -> Result<(), sea_orm::DbErr> {
    let now = Utc::now();
    collection_source::Entity::update_many()
        .col_expr(collection_source::Column::LastSyncedAt, Expr::value(now))
        .col_expr(collection_source::Column::UpdatedAt, Expr::value(now))
        .filter(collection_source::Column::UserId.eq(user_id))
        .filter(collection_source::Column::Game.eq(game))
        .filter(collection_source::Column::Provider.eq(provider.as_str()))
        .filter(collection_source::Column::ExternalId.eq(external_id))
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

    /// A job's lifecycle status, via the owner-scoped view (progress is asserted
    /// separately below).
    fn status_of(q: &ImportQueue, id: u64, user_id: i32, game: &str) -> Option<JobStatus> {
        q.view(id, user_id, game).map(|v| v.status)
    }

    #[test]
    fn create_then_status_is_queued_and_owner_scoped() {
        let q = ImportQueue::default();
        let (id, _progress) = q.create(1, "mtg").expect("create");
        assert!(matches!(status_of(&q, id, 1, "mtg"), Some(JobStatus::Queued)));
        // Wrong user, wrong game, or unknown id all read as absent (=> 404).
        assert!(q.view(id, 2, "mtg").is_none());
        assert!(q.view(id, 1, "pokemon").is_none());
        assert!(q.view(id + 999, 1, "mtg").is_none());
    }

    #[test]
    fn set_transitions_status() {
        let q = ImportQueue::default();
        let (id, _progress) = q.create(7, "mtg").expect("create");
        q.set(id, JobStatus::Running);
        assert!(matches!(status_of(&q, id, 7, "mtg"), Some(JobStatus::Running)));
        q.set(id, JobStatus::Complete(summary()));
        assert!(matches!(status_of(&q, id, 7, "mtg"), Some(JobStatus::Complete(_))));
    }

    #[test]
    fn view_reflects_live_progress() {
        let q = ImportQueue::default();
        let (id, progress) = q.create(1, "mtg").expect("create");
        // Fresh job: nothing fetched, no total known yet.
        let v = q.view(id, 1, "mtg").expect("view");
        assert_eq!(v.progress.fetched_rows, 0);
        assert_eq!(v.progress.total_rows, None);
        // The worker publishes progress into the shared reporter; the view sees it.
        progress.set_total(50);
        progress.add_fetched(25);
        let v = q.view(id, 1, "mtg").expect("view");
        assert_eq!(v.progress.fetched_rows, 25);
        assert_eq!(v.progress.total_rows, Some(50));
    }

    #[test]
    fn ids_are_unique() {
        let q = ImportQueue::default();
        let (a, _) = q.create(1, "mtg").unwrap();
        let (b, _) = q.create(1, "mtg").unwrap();
        assert_ne!(a, b);
    }

    /// Stamping only marks the saved link that points at the just-imported collection:
    /// importing an unrelated collection leaves it untouched, importing the saved one
    /// stamps it. This is what lets a one-off import of the saved link (or a re-sync)
    /// update "Last synced" while an unrelated import doesn't.
    #[tokio::test]
    async fn stamp_last_synced_only_marks_the_matching_saved_link() {
        use crate::test_support::{insert_user, migrated_memory_db};
        use sea_orm::{ActiveModelTrait, Set};

        let db = migrated_memory_db().await;
        let user_id = insert_user(&db, "sync@example.com").await;
        let now = Utc::now();
        collection_source::ActiveModel {
            user_id: Set(user_id),
            game: Set("mtg".to_string()),
            provider: Set(Provider::Archidekt.as_str().to_string()),
            external_id: Set("123".to_string()),
            last_synced_at: Set(None),
            smart: Set(false),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(&db)
        .await
        .expect("insert source");

        let reload = || async {
            collection_source::Entity::find()
                .filter(collection_source::Column::UserId.eq(user_id))
                .one(&db)
                .await
                .expect("query source")
                .expect("source exists")
        };

        // A stamp for a *different* collection id must not touch the saved link.
        stamp_last_synced(&db, user_id, "mtg", Provider::Archidekt, "999")
            .await
            .expect("stamp");
        assert!(
            reload().await.last_synced_at.is_none(),
            "importing an unrelated collection left the saved link unsynced",
        );

        // A stamp for the saved collection id marks it synced.
        stamp_last_synced(&db, user_id, "mtg", Provider::Archidekt, "123")
            .await
            .expect("stamp");
        assert!(
            reload().await.last_synced_at.is_some(),
            "importing the saved collection stamped the link",
        );
    }
}

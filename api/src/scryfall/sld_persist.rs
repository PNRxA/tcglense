//! Persist the Secret Lair drop snapshot in the database, so the drop store can reboot with the
//! last-good scraped/imported snapshot instead of the committed seed.
//!
//! The drop store ([`super::drops`]) is an in-memory overlay reseeded from the committed
//! `sld_drops.json` on every boot. That committed seed can be months stale, so without persistence a
//! restart serves it until the next scrape/import — and the daily refresh is *deferred* when it ran
//! within the interval ([`super::sld_tasks::initial_delay`]), so the stale window can be up to a full
//! interval. Persisting each successful snapshot and **reseeding from it at boot**
//! ([`super::sld_tasks`]) closes that window: the store holds the last-good drops during the
//! deferral, the mirror origin serves the same ETag it served before the restart (so consumers
//! `304`), and a consumer's conditional import `304`s onto the persisted snapshot, not the seed.
//!
//! One row, keyed by a constant [`SNAPSHOT_KEY`] discriminator so the write is a singleton upsert
//! (the same `(game, dataset)`-keyed idiom as [`crate::catalog::ingest_state`], kept as its own table
//! rather than a column on `ingest_state` because the ~58 KB blob would otherwise be dragged into
//! every unfiltered `ingest_state` read — e.g. the sitemap's global last-modified query). The stored
//! `content_version` + `updated_at` also give an operator a queryable "which snapshot is loaded, and
//! when did it last refresh" signal without rebuilding the table.

use chrono::Utc;
use sea_orm::{
    ActiveValue::{NotSet, Set},
    ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter,
    sea_query::OnConflict,
};

use crate::entities::{prelude::SldDropSnapshot, sld_drop_snapshot};

/// The single row's discriminator. `"mtg/sld"` mirrors the `(game, dataset)` key used elsewhere and
/// keeps the door open for a second drop-grouped set/game without a schema change.
const SNAPSHOT_KEY: &str = "mtg/sld";

/// Load the persisted canonical snapshot JSON, or `None` when nothing has been persisted yet (first
/// boot → the caller keeps the committed seed). Best-effort: the caller logs an error and keeps the
/// seed, never treating it as fatal (same posture as the sync bookkeeping).
pub async fn load(db: &DatabaseConnection) -> Result<Option<String>, DbErr> {
    let row = SldDropSnapshot::find()
        .filter(sld_drop_snapshot::Column::SnapshotKey.eq(SNAPSHOT_KEY))
        .one(db)
        .await?;
    Ok(row.map(|r| r.snapshot_json))
}

/// Upsert the canonical snapshot JSON + its content version, stamping `updated_at = now`. Called
/// after a successful install so the persisted row always matches what the store (and the mirror
/// endpoint) now serves. The `snapshot_key` unique index is the `ON CONFLICT` target.
pub async fn save(
    db: &DatabaseConnection,
    snapshot_json: &str,
    content_version: &str,
) -> Result<(), DbErr> {
    let model = sld_drop_snapshot::ActiveModel {
        id: NotSet,
        snapshot_key: Set(SNAPSHOT_KEY.to_string()),
        snapshot_json: Set(snapshot_json.to_string()),
        content_version: Set(content_version.to_string()),
        updated_at: Set(Utc::now()),
    };
    SldDropSnapshot::insert(model)
        .on_conflict(
            OnConflict::column(sld_drop_snapshot::Column::SnapshotKey)
                .update_columns([
                    sld_drop_snapshot::Column::SnapshotJson,
                    sld_drop_snapshot::Column::ContentVersion,
                    sld_drop_snapshot::Column::UpdatedAt,
                ])
                .to_owned(),
        )
        .exec_without_returning(db)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::migrated_memory_db;

    /// A minimal valid snapshot covering `mtg/sld` (what a reseed's `install_snapshot` requires).
    fn snapshot(slug: &str, version: &str) -> (String, String) {
        (
            format!(
                r#"{{"sets":[{{"game":"mtg","set":"sld","drops":[{{"slug":"{slug}","title":"{slug}","collector_numbers":["1","2"]}}]}}]}}"#
            ),
            version.to_string(),
        )
    }

    #[tokio::test]
    async fn load_returns_none_before_anything_is_persisted() {
        let db = migrated_memory_db().await;
        assert_eq!(load(&db).await.expect("query"), None);
    }

    #[tokio::test]
    async fn save_then_load_round_trips_the_exact_json() {
        let db = migrated_memory_db().await;
        let (json, version) = snapshot("a", "deadbeefdeadbeef");
        save(&db, &json, &version).await.expect("save");
        assert_eq!(load(&db).await.expect("query"), Some(json));
    }

    #[tokio::test]
    async fn save_upserts_the_single_row_in_place() {
        let db = migrated_memory_db().await;
        let (json1, v1) = snapshot("a", "v1");
        let (json2, v2) = snapshot("b", "v2");
        save(&db, &json1, &v1).await.expect("save 1");
        save(&db, &json2, &v2).await.expect("save 2");

        // The singleton key means the second save overwrites in place — one row, latest content.
        let rows = SldDropSnapshot::find().all(&db).await.expect("all");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].snapshot_json, json2);
        assert_eq!(rows[0].content_version, "v2");
    }
}

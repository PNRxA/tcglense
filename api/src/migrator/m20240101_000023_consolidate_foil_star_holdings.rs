use sea_orm::{ConnectionTrait, DbErr, TransactionTrait};
use sea_orm_migration::prelude::*;

/// One-time cleanup for issue #209: fold every collection holding on a separately-modelled
/// **foil** printing (`…★`, e.g. `sld` `741★`) onto its nonfoil base card (`741`) as a foil
/// copy, then drop the `…★` holding. Predates the import-path fix, so it repairs the
/// duplicate rows already sitting in existing collections; new imports never create them
/// (see [`crate::collection_import::consolidate`], which shares the matching rule).
///
/// The rule is deliberately conservative — the exact twin of the live one: only a
/// `finishes = "foil"` star with a `finishes = "nonfoil"` sibling (same game, set, oracle
/// id, and collector number sans the star) is folded. An ambiguous star (base itself
/// `nonfoil,foil`), an `etched` star, or a star with no base (a standalone promo) keeps its
/// own holding. `cards` is catalog data, populated by the sync rather than a migration, so
/// on a fresh/unsynced DB the joins match nothing and this is a no-op.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // One transaction so a boot crash mid-fold rolls back entirely — the version row is
        // written only after `up` returns, so the clean re-run stays correct (the fold isn't
        // idempotent until the `…★` holdings have been deleted by the final statement).
        manager
            .get_connection()
            .transaction::<_, (), DbErr>(|txn| {
                Box::pin(async move { consolidate_foil_star_holdings(txn).await })
            })
            .await
            .map_err(flatten_transaction_error)
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // A destructive data fold: the base's post-fold foil count no longer records how
        // many copies came from the star, so the split can't be reconstructed. Nothing to
        // undo (the schema is unchanged).
        Ok(())
    }
}

/// Run the three-statement fold (add-to-existing-base, insert-missing-base, delete-star) in
/// order. Cross-backend plain SQL (correlated subqueries + `REPLACE`/`LIKE`, no
/// `UPDATE…FROM` or `LEAST`), so it runs byte-identically on SQLite and Postgres. Exposed
/// `pub(crate)` so the migration test can drive it against a DB carrying data (the migrator
/// itself runs it once, on an empty collection).
///
/// Each statement's subquery joins a `finishes = 'foil'` star (`sc`) to its
/// `finishes = 'nonfoil'` base (`bc`) — same game, set, oracle id, and collector number once
/// the star is stripped. In statements 1 & 2 the star is reached through its *holding* (`s`);
/// statement 3 needs only the catalog to name the star card ids to delete.
pub(crate) async fn consolidate_foil_star_holdings<C: ConnectionTrait>(
    conn: &C,
) -> Result<(), DbErr> {
    // 1. Existing base holdings: add the star's copies (whichever finish they were stored
    //    under) to the base's foil. The `EXISTS` restricts the update to bases whose owner
    //    actually holds the star, so unrelated holdings aren't touched; `updated_at` isn't
    //    in the SET, so the collection's recency order is preserved.
    conn.execute_unprepared(ADD_STAR_TO_EXISTING_BASE).await?;
    // 2. Bases the owner doesn't yet hold: create the holding with the star's copies as
    //    foil (0 regular). Runs after step 1 so a base updated there is skipped here.
    conn.execute_unprepared(INSERT_MISSING_BASE).await?;
    // 3. Drop the now-folded star holdings (only those with a valid base — orphans stay).
    conn.execute_unprepared(DELETE_FOLDED_STARS).await?;
    Ok(())
}

const ADD_STAR_TO_EXISTING_BASE: &str = r#"
UPDATE collection_items
SET foil_quantity = foil_quantity + COALESCE((
        SELECT SUM(s.quantity + s.foil_quantity)
        FROM collection_items s
        JOIN cards sc ON sc.id = s.card_id
        JOIN cards bc ON bc.game = sc.game
                     AND bc.set_code = sc.set_code
                     AND bc.oracle_id = sc.oracle_id
                     AND bc.collector_number = REPLACE(sc.collector_number, '★', '')
                     AND bc.finishes = 'nonfoil'
        WHERE sc.finishes = 'foil'
          AND sc.collector_number LIKE '%★'
          AND s.user_id = collection_items.user_id
          AND s.game = collection_items.game
          AND bc.id = collection_items.card_id
    ), 0)
WHERE EXISTS (
        SELECT 1
        FROM collection_items s
        JOIN cards sc ON sc.id = s.card_id
        JOIN cards bc ON bc.game = sc.game
                     AND bc.set_code = sc.set_code
                     AND bc.oracle_id = sc.oracle_id
                     AND bc.collector_number = REPLACE(sc.collector_number, '★', '')
                     AND bc.finishes = 'nonfoil'
        WHERE sc.finishes = 'foil'
          AND sc.collector_number LIKE '%★'
          AND s.user_id = collection_items.user_id
          AND s.game = collection_items.game
          AND bc.id = collection_items.card_id
    )"#;

const INSERT_MISSING_BASE: &str = r#"
INSERT INTO collection_items (user_id, game, card_id, quantity, foil_quantity, created_at, updated_at)
SELECT s.user_id, s.game, bc.id, 0, (s.quantity + s.foil_quantity), s.created_at, s.updated_at
FROM collection_items s
JOIN cards sc ON sc.id = s.card_id
JOIN cards bc ON bc.game = sc.game
             AND bc.set_code = sc.set_code
             AND bc.oracle_id = sc.oracle_id
             AND bc.collector_number = REPLACE(sc.collector_number, '★', '')
             AND bc.finishes = 'nonfoil'
WHERE sc.finishes = 'foil'
  AND sc.collector_number LIKE '%★'
  AND NOT EXISTS (
        SELECT 1 FROM collection_items b
        WHERE b.user_id = s.user_id AND b.game = s.game AND b.card_id = bc.id
    )"#;

const DELETE_FOLDED_STARS: &str = r#"
DELETE FROM collection_items
WHERE card_id IN (
        SELECT sc.id
        FROM cards sc
        JOIN cards bc ON bc.game = sc.game
                     AND bc.set_code = sc.set_code
                     AND bc.oracle_id = sc.oracle_id
                     AND bc.collector_number = REPLACE(sc.collector_number, '★', '')
                     AND bc.finishes = 'nonfoil'
        WHERE sc.finishes = 'foil' AND sc.collector_number LIKE '%★'
    )"#;

/// Both arms of a [`sea_orm::TransactionError`] over `DbErr` are `DbErr`s.
fn flatten_transaction_error(err: sea_orm::TransactionError<DbErr>) -> DbErr {
    match err {
        sea_orm::TransactionError::Connection(e) => e,
        sea_orm::TransactionError::Transaction(e) => e,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::card;
    use crate::test_support::{
        card_model, insert_holding, insert_user, migrated_memory_db, owned_counts,
    };
    use sea_orm::{ActiveModelTrait, DatabaseConnection, IntoActiveModel};

    async fn insert_card(
        db: &DatabaseConnection,
        id: i32,
        set_code: &str,
        collector_number: &str,
        finishes: &str,
        oracle_id: &str,
    ) -> i32 {
        card::Model {
            external_id: format!("ext-{id}"),
            set_code: set_code.into(),
            collector_number: collector_number.into(),
            finishes: Some(finishes.into()),
            oracle_id: Some(oracle_id.into()),
            ..card_model(id)
        }
        .into_active_model()
        .insert(db)
        .await
        .expect("insert card")
        .id
    }

    #[tokio::test]
    async fn folds_legacy_star_holdings_and_leaves_ambiguous_or_orphan_ones() {
        let db = migrated_memory_db().await;
        let user = insert_user(&db, "legacy@test.example").await;

        // Pair 1: base not yet held, star held (the plain issue #209 case).
        let base1 = insert_card(&db, 1, "sld", "741", "nonfoil", "ora-1").await;
        let star1 = insert_card(&db, 2, "sld", "741★", "foil", "ora-1").await;
        insert_holding(&db, user, star1, 1, 0).await; // stored as regular, pre-fix

        // Pair 2: base already held (2 regular) AND star held (3 foil) -> merge.
        let base2 = insert_card(&db, 3, "sld", "500", "nonfoil", "ora-2").await;
        let star2 = insert_card(&db, 4, "sld", "500★", "foil", "ora-2").await;
        insert_holding(&db, user, base2, 2, 0).await;
        insert_holding(&db, user, star2, 0, 3).await;

        // Ambiguous: base is itself foilable -> the star is NOT folded.
        insert_card(&db, 5, "stx", "33", "nonfoil,foil", "ora-3").await;
        let ambiguous = insert_card(&db, 6, "stx", "33★", "foil", "ora-3").await;
        insert_holding(&db, user, ambiguous, 0, 1).await;

        // Orphan: a foil star with no base sibling -> left alone.
        let orphan = insert_card(&db, 7, "pxln", "1★", "foil", "ora-4").await;
        insert_holding(&db, user, orphan, 0, 2).await;

        consolidate_foil_star_holdings(&db).await.expect("fold");

        assert_eq!(
            owned_counts(&db, user, base1).await,
            Some((0, 1)),
            "star's copy became base foil"
        );
        assert_eq!(
            owned_counts(&db, user, star1).await,
            None,
            "star holding removed"
        );
        assert_eq!(
            owned_counts(&db, user, base2).await,
            Some((2, 3)),
            "2 regular kept, 3 foil folded in"
        );
        assert_eq!(
            owned_counts(&db, user, star2).await,
            None,
            "star holding removed"
        );
        assert_eq!(
            owned_counts(&db, user, ambiguous).await,
            Some((0, 1)),
            "ambiguous star untouched"
        );
        assert_eq!(
            owned_counts(&db, user, orphan).await,
            Some((0, 2)),
            "orphan star untouched"
        );
    }

    #[tokio::test]
    async fn fold_is_idempotent_once_stars_are_gone() {
        // Re-running after the stars are deleted is a no-op (the join finds no star holdings),
        // so a crash-free second boot / a down+up cycle can't double-count.
        let db = migrated_memory_db().await;
        let user = insert_user(&db, "legacy@test.example").await;
        let base = insert_card(&db, 1, "sld", "741", "nonfoil", "ora-1").await;
        let star = insert_card(&db, 2, "sld", "741★", "foil", "ora-1").await;
        insert_holding(&db, user, star, 2, 0).await;

        consolidate_foil_star_holdings(&db)
            .await
            .expect("fold once");
        consolidate_foil_star_holdings(&db)
            .await
            .expect("fold again");

        assert_eq!(
            owned_counts(&db, user, base).await,
            Some((0, 2)),
            "still just the one fold"
        );
    }

    #[tokio::test]
    async fn fold_scopes_stars_to_their_owner() {
        // Two users each holding the same star: the fold must not cross user boundaries.
        let db = migrated_memory_db().await;
        let u1 = insert_user(&db, "u1@test.example").await;
        let u2 = insert_user(&db, "u2@test.example").await;
        let base = insert_card(&db, 1, "sld", "741", "nonfoil", "ora-1").await;
        let star = insert_card(&db, 2, "sld", "741★", "foil", "ora-1").await;
        insert_holding(&db, u1, star, 1, 0).await;
        insert_holding(&db, u2, star, 4, 0).await;

        consolidate_foil_star_holdings(&db).await.expect("fold");

        assert_eq!(owned_counts(&db, u1, base).await, Some((0, 1)));
        assert_eq!(owned_counts(&db, u2, base).await, Some((0, 4)));
    }
}

use sea_orm::TransactionTrait;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

/// Email-first registration (issue #176) creates the account row when the
/// visitor submits just their address; the password arrives later, via the
/// emailed completion link. A row with `password_hash IS NULL` is that pending
/// state — it cannot sign in until the registration is completed.
///
/// SQLite has no `ALTER COLUMN`, so dropping the NOT NULL constraint is the
/// add-copy-drop-rename dance (the column has no index/uniqueness riding on
/// it, so plain column DDL suffices — no table rebuild needed). The dance runs
/// inside ONE transaction: sea-orm-migration doesn't wrap SQLite migrations
/// itself, and four independently-committed statements would leave a
/// crash-interrupted boot with a half-renamed column that the re-run (the
/// version row is only written after `up` returns) could never repair.
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .transaction::<_, (), DbErr>(|txn| {
                Box::pin(async move {
                    txn.execute_unprepared(
                        "ALTER TABLE users ADD COLUMN password_hash_nullable TEXT",
                    )
                    .await?;
                    txn.execute_unprepared("UPDATE users SET password_hash_nullable = password_hash")
                        .await?;
                    txn.execute_unprepared("ALTER TABLE users DROP COLUMN password_hash")
                        .await?;
                    txn.execute_unprepared(
                        "ALTER TABLE users RENAME COLUMN password_hash_nullable TO password_hash",
                    )
                    .await?;
                    Ok(())
                })
            })
            .await
            .map_err(flatten_transaction_error)
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .transaction::<_, (), DbErr>(|txn| {
                Box::pin(async move {
                    // Reverse dance. Pending (password-less) registrations cannot
                    // be represented under NOT NULL; they are deleted — they hold
                    // no password, no sessions, and re-registering recreates them.
                    txn.execute_unprepared("DELETE FROM users WHERE password_hash IS NULL")
                        .await?;
                    txn.execute_unprepared(
                        "ALTER TABLE users ADD COLUMN password_hash_not_null TEXT NOT NULL DEFAULT ''",
                    )
                    .await?;
                    txn.execute_unprepared("UPDATE users SET password_hash_not_null = password_hash")
                        .await?;
                    txn.execute_unprepared("ALTER TABLE users DROP COLUMN password_hash")
                        .await?;
                    txn.execute_unprepared(
                        "ALTER TABLE users RENAME COLUMN password_hash_not_null TO password_hash",
                    )
                    .await?;
                    Ok(())
                })
            })
            .await
            .map_err(flatten_transaction_error)
    }
}

/// Both arms of a [`sea_orm::TransactionError`] over `DbErr` are `DbErr`s.
fn flatten_transaction_error(err: sea_orm::TransactionError<DbErr>) -> DbErr {
    match err {
        sea_orm::TransactionError::Connection(e) => e,
        sea_orm::TransactionError::Transaction(e) => e,
    }
}

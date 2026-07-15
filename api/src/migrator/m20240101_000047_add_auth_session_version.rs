use sea_orm::TransactionTrait;
use sea_orm_migration::prelude::*;

/// Add the account generation copied into access and refresh tokens. Password
/// reset increments it so every previously issued session is rejected.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // SQLite migrator runs are not automatically transactional. Start a
        // transaction for a bare connection, while reusing SeaORM's outer
        // Postgres transaction when one is already present.
        if !matches!(
            manager.get_connection(),
            SchemaManagerConnection::Transaction(_)
        ) {
            return manager
                .get_connection()
                .transaction::<_, (), DbErr>(|txn| {
                    Box::pin(async move { apply_up(&SchemaManager::new(txn)).await })
                })
                .await
                .map_err(flatten_transaction_error);
        }
        apply_up(manager).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if !matches!(
            manager.get_connection(),
            SchemaManagerConnection::Transaction(_)
        ) {
            return manager
                .get_connection()
                .transaction::<_, (), DbErr>(|txn| {
                    Box::pin(async move { apply_down(&SchemaManager::new(txn)).await })
                })
                .await
                .map_err(flatten_transaction_error);
        }
        apply_down(manager).await
    }
}

async fn apply_up(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .alter_table(
            Table::alter()
                .table(Users::Table)
                .add_column(
                    ColumnDef::new(Users::SessionVersion)
                        .big_integer()
                        .not_null()
                        .default(0i64),
                )
                .to_owned(),
        )
        .await?;
    manager
        .alter_table(
            Table::alter()
                .table(RefreshTokens::Table)
                .add_column(
                    ColumnDef::new(RefreshTokens::SessionVersion)
                        .big_integer()
                        .not_null()
                        .default(0i64),
                )
                .to_owned(),
        )
        .await?;
    Ok(())
}

async fn apply_down(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .alter_table(
            Table::alter()
                .table(RefreshTokens::Table)
                .drop_column(RefreshTokens::SessionVersion)
                .to_owned(),
        )
        .await?;
    manager
        .alter_table(
            Table::alter()
                .table(Users::Table)
                .drop_column(Users::SessionVersion)
                .to_owned(),
        )
        .await?;
    Ok(())
}

fn flatten_transaction_error(err: sea_orm::TransactionError<DbErr>) -> DbErr {
    match err {
        sea_orm::TransactionError::Connection(err) => err,
        sea_orm::TransactionError::Transaction(err) => err,
    }
}

#[derive(DeriveIden)]
enum Users {
    Table,
    SessionVersion,
}

#[derive(DeriveIden)]
enum RefreshTokens {
    Table,
    SessionVersion,
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::Database;

    #[tokio::test]
    async fn sqlite_up_rolls_back_if_the_second_table_change_fails() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        db.execute_unprepared("CREATE TABLE users (id INTEGER PRIMARY KEY)")
            .await
            .unwrap();
        let manager = SchemaManager::new(&db);

        assert!(Migration.up(&manager).await.is_err());
        assert!(
            !manager
                .has_column("users", "session_version")
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn sqlite_down_rolls_back_if_the_second_table_change_fails() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        db.execute_unprepared("CREATE TABLE users (id INTEGER PRIMARY KEY)")
            .await
            .unwrap();
        db.execute_unprepared(
            "CREATE TABLE refresh_tokens (\
                id INTEGER PRIMARY KEY, \
                session_version BIGINT NOT NULL DEFAULT 0\
            )",
        )
        .await
        .unwrap();
        let manager = SchemaManager::new(&db);

        assert!(Migration.down(&manager).await.is_err());
        assert!(
            manager
                .has_column("refresh_tokens", "session_version")
                .await
                .unwrap()
        );
    }
}

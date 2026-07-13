use sea_orm::{ConnectionTrait, DatabaseBackend};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();

        // Opt-in public handle (issue #362). SQLite folds case at the storage layer via
        // COLLATE NOCASE so the (username, discriminator) unique index below is
        // case-insensitive on the name without the writer having to lowercase. Postgres
        // has no NOCASE collation, so the column is plain and the case-insensitive
        // uniqueness is enforced by a lower(username) functional index — mirroring the
        // email precedent in m20240101_000001.
        let mut username = ColumnDef::new(Users::Username);
        username.string().null();
        if backend == DatabaseBackend::Sqlite {
            username.extra("COLLATE NOCASE");
        }

        // SQLite allows only one ADD COLUMN per ALTER, so add the two columns in
        // separate statements.
        manager
            .alter_table(
                Table::alter()
                    .table(Users::Table)
                    .add_column(&mut username)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Users::Table)
                    .add_column(ColumnDef::new(Users::Discriminator).integer().null())
                    .to_owned(),
            )
            .await?;

        // Case-insensitive uniqueness on the (username, discriminator) pair: "alice#0001"
        // and "Alice#0001" collide, "alice#0002" is free. This same index also serves the
        // handle -> user_id lookup and the "which tags are taken for this name" allocation
        // scan (its leading column).
        if backend == DatabaseBackend::Postgres {
            // Functional lower(username) index (sea-query Index::create can't express a
            // functional index). Partial predicate keeps it to opt-in rows only — every
            // account that never set a handle has username IS NULL. Queries MUST filter
            // `lower(username) = lower($1) AND discriminator = $2` to use it.
            manager
                .get_connection()
                .execute_unprepared(
                    r#"CREATE UNIQUE INDEX "idx_users_username_discriminator" ON "users" (lower("username"), "discriminator") WHERE "username" IS NOT NULL"#,
                )
                .await?;
        } else {
            // SQLite: the column's COLLATE NOCASE makes the username leg case-insensitive;
            // NULL usernames are distinct in a UNIQUE index, so unset accounts never
            // collide. `WHERE username = ? AND discriminator = ?` is served case-insensitively
            // by this index.
            manager
                .create_index(
                    Index::create()
                        .name("idx_users_username_discriminator")
                        .table(Users::Table)
                        .col(Users::Username)
                        .col(Users::Discriminator)
                        .unique()
                        .to_owned(),
                )
                .await?;
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();

        // Drop the index before the columns: SQLite refuses to DROP COLUMN while an
        // index references it. (Postgres would cascade, but drop explicitly for symmetry.)
        if backend == DatabaseBackend::Postgres {
            manager
                .get_connection()
                .execute_unprepared(r#"DROP INDEX IF EXISTS "idx_users_username_discriminator""#)
                .await?;
        } else {
            manager
                .drop_index(
                    Index::drop()
                        .name("idx_users_username_discriminator")
                        .table(Users::Table)
                        .to_owned(),
                )
                .await?;
        }

        manager
            .alter_table(
                Table::alter()
                    .table(Users::Table)
                    .drop_column(Users::Discriminator)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Users::Table)
                    .drop_column(Users::Username)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Username,
    Discriminator,
}

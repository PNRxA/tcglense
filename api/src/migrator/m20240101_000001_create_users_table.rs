use sea_orm::{ConnectionTrait, DatabaseBackend};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();

        // Email column. SQLite folds case at the storage layer via COLLATE NOCASE so
        // the unique index below can't be bypassed by a writer that forgets to
        // lowercase. Postgres has no NOCASE collation, so the column is plain and the
        // case-insensitive uniqueness is enforced by a lower(email) functional index.
        let mut email = ColumnDef::new(Users::Email);
        email.string().not_null();
        if backend == DatabaseBackend::Sqlite {
            email.extra("COLLATE NOCASE");
        }

        manager
            .create_table(
                Table::create()
                    .table(Users::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Users::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(&mut email)
                    .col(ColumnDef::new(Users::PasswordHash).string().not_null())
                    .col(ColumnDef::new(Users::DisplayName).string().null())
                    .col(
                        ColumnDef::new(Users::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Users::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // Unique email index. SQLite indexes the column directly (its COLLATE NOCASE
        // makes lookups case-insensitive); Postgres indexes lower(email) since
        // sea-query's Index::create() can't express a functional index. App code
        // lowercases every email on write+read, so this is defense-in-depth. No user
        // upsert uses ON CONFLICT on email (register is filter-then-insert), so a
        // functional (non-arbiter) index is safe.
        if backend == DatabaseBackend::Postgres {
            manager
                .get_connection()
                .execute_unprepared(
                    r#"CREATE UNIQUE INDEX "idx_users_email" ON "users" (lower("email"))"#,
                )
                .await?;
            // The functional lower(email) index above enforces case-insensitive
            // uniqueness but can't serve the plain `WHERE email = $1` auth lookups
            // (login / register), which would otherwise seq-scan. Add a NON-unique
            // plain-column index for them (uniqueness stays on the functional index).
            manager
                .get_connection()
                .execute_unprepared(r#"CREATE INDEX "idx_users_email_lookup" ON "users" ("email")"#)
                .await?;
        } else {
            manager
                .create_index(
                    Index::create()
                        .name("idx_users_email")
                        .table(Users::Table)
                        .col(Users::Email)
                        .unique()
                        .to_owned(),
                )
                .await?;
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Dropping the table drops its index on both backends.
        manager
            .drop_table(Table::drop().table(Users::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
    Email,
    PasswordHash,
    DisplayName,
    CreatedAt,
    UpdatedAt,
}

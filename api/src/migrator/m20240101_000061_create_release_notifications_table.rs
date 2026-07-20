use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Per-user "we already told this user about this release" ledger, so the release-alert
        // evaluator is edge-triggered: it sends the day-before heads-up once per (user, release)
        // and never re-sends on later passes. `kind` distinguishes a Secret Lair drop
        // (`sld_drop`, keyed by drop slug) from a regular set (`set`, keyed by set code); `ref_key`
        // is that stable per-release key. A row is written only after a channel actually accepts
        // the message, so an undeliverable heads-up retries next pass (until the release passes out
        // of the look-ahead window) rather than being silently swallowed.
        manager
            .create_table(
                Table::create()
                    .table(ReleaseNotifications::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ReleaseNotifications::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(ReleaseNotifications::UserId)
                            .integer()
                            .not_null(),
                    )
                    // `sld_drop` | `set` — which kind of release this row records.
                    .col(
                        ColumnDef::new(ReleaseNotifications::Kind)
                            .string()
                            .not_null(),
                    )
                    // The stable per-release key within the kind: a drop slug, or a set code.
                    .col(
                        ColumnDef::new(ReleaseNotifications::RefKey)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ReleaseNotifications::Game)
                            .string()
                            .not_null(),
                    )
                    // The release date we notified about (ISO `YYYY-MM-DD`), for reference.
                    .col(
                        ColumnDef::new(ReleaseNotifications::ReleaseDate)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ReleaseNotifications::SentAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    // Deleting a user removes their notification ledger.
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_release_notifications_user_id")
                            .from(ReleaseNotifications::Table, ReleaseNotifications::UserId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // The dedup key: at most one row per (user, kind, release). Also the lookup the
        // evaluator runs each pass to skip already-notified releases for a page of users.
        manager
            .create_index(
                Index::create()
                    .name("idx_release_notifications_user_kind_ref")
                    .table(ReleaseNotifications::Table)
                    .col(ReleaseNotifications::UserId)
                    .col(ReleaseNotifications::Kind)
                    .col(ReleaseNotifications::RefKey)
                    .unique()
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(ReleaseNotifications::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum ReleaseNotifications {
    Table,
    Id,
    UserId,
    Kind,
    RefKey,
    Game,
    ReleaseDate,
    SentAt,
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
}

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Users::Table)
                    .add_column(
                        ColumnDef::new(Users::EmailVerifiedAt)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .to_owned(),
            )
            .await?;

        // Grandfather every existing account as verified: they predate email
        // verification, and login now refuses unverified users — without this
        // backfill the migration would lock every existing user out. Bind a real
        // chrono value (not CURRENT_TIMESTAMP) so the stored text matches the
        // format SeaORM writes for `DateTimeUtc` everywhere else.
        manager
            .exec_stmt(
                Query::update()
                    .table(Users::Table)
                    .value(Users::EmailVerifiedAt, chrono::Utc::now())
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Users::Table)
                    .drop_column(Users::EmailVerifiedAt)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Users {
    Table,
    EmailVerifiedAt,
}

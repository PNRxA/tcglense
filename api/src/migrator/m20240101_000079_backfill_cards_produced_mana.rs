use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // `cards.produced_mana` used to store "produces nothing" as NULL, because Scryfall omits
        // the field for a non-producer and the ingest joined an absent list to NULL. The deck
        // mana-base read (issue #670) needs to tell a checked non-producer from a row never
        // rewritten since the column arrived, so `scryfall::map` now writes `""` for the former
        // and reads NULL as "not checked yet" — the `token_parts` stance. Every NULL at this
        // point IS a checked non-producer (the column has been rewritten by every bulk import
        // since migration 13), so backfill the convention here rather than reporting the whole
        // catalog "unchecked" until the next import, and rather than letting that import
        // rewrite most of the table in one tick: `updated_at` is deliberately left alone, since
        // the price-alert evaluator narrows by it and nothing about these rows changed.
        manager
            .get_connection()
            .execute_unprepared(
                r#"UPDATE "cards" SET "produced_mana" = '' WHERE "produced_mana" IS NULL"#,
            )
            .await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Both spellings read as "produces nothing" everywhere (`produces:c` reads NULL or
        // `""`), so there is nothing to undo.
        Ok(())
    }
}

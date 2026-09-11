use sea_orm::DatabaseBackend;
use sea_orm_migration::prelude::*;

/// Index `products` on `(game, product_type)` — the pair the sealed-catalog filter
/// dropdown's type facet selects on, and one no index of `m..019` leads with.
///
/// `handlers::catalog::products::product_facets` answers the SPA's type dropdown with
/// `SELECT DISTINCT "product_type" FROM "products" WHERE "game" = $1`, and `m..019` ships
/// only `(game, external_id)` and `(game, set_code)` — a `DISTINCT` on a third column can
/// use neither, so the read falls back to a full heap scan of the game's whole partition
/// to produce ~17 rows. Every one of those rows is then discarded but for its distinct
/// value. Measured in production: ~2.0s for a facets request that returns 17 types.
///
/// With this index Postgres takes an index-only scan over the game range instead of the
/// wide heap (`product_type` is `NOT NULL`, so no `IS NULL` gap makes the index
/// non-covering), and on PG18+ a skip scan can jump between distinct values rather than
/// walking every entry. The same "a facet read must not scan the table it faces" rule the
/// grouped `set_code` half of this handler already gets for free from
/// `idx_products_game_set_code`.
///
/// It also serves the exclusivity derivation's comparison-product lookup
/// (`catalog::sealed_exclusives`), which selects the booster products of a set by
/// `game = ? AND product_type IN (…)`.
///
/// Plain `CREATE INDEX` with `SET LOCAL statement_timeout = 0` on Postgres — the pattern
/// `m..066`/`m..069`/`m..074` set: the pending batch runs in one transaction on boot, so a
/// role-default timeout killing this build would roll back every migration with it.
const INDEX: &str = "idx_products_game_product_type";

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if manager.get_database_backend() == DatabaseBackend::Postgres {
            manager
                .get_connection()
                .execute_unprepared("SET LOCAL statement_timeout = 0")
                .await?;
        }

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name(INDEX)
                    .table(Products::Table)
                    .col(Products::Game)
                    .col(Products::ProductType)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(Index::drop().name(INDEX).table(Products::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Products {
    Table,
    Game,
    ProductType,
}

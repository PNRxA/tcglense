use sea_orm::{ConnectionTrait, DatabaseBackend};
use sea_orm_migration::prelude::*;

/// A **descending-latest** index on each price-history twin for the collection *movers*
/// reference-date lookup (`handlers::collection::price_movements`), surfaced by profiling the
/// four slow `card_price_history` statements the weak prod Postgres logged after a daily
/// snapshot.
///
/// The movers endpoint anchors every window to "the most recent snapshot date across the
/// user's priced holdings", found with `SELECT MAX(as_of_date) WHERE game = ? AND
/// {card,product}_id IN (…owned…)` before the per-item anchor aggregate runs. The existing
/// indexes are both keyed `(game, {card,product}_id, as_of_date)` — great for *per-item*
/// range scans and the covering value-history read (`m..031`), but for this global `MAX` they
/// force the planner to read **every** owned item's whole history just to take one maximum:
/// on a faithful 17.8M-row Postgres 16 repro (20k cards, a 951-card collection) that is
/// ~846k index entries, planned either as a bitmap heap scan (~176k cold heap-page reads —
/// the shape that logged ~5 s on prod) or, once the visibility map warms, a full index-only
/// scan (~950 heap fetches, ~0.5 s warm). Both scale with the collection's total captured
/// days.
///
/// Leading with `as_of_date` instead lets the same `MAX` become a backward index-only scan
/// that stops at the first owned row: `game` seeks, the scan walks `as_of_date` **descending**,
/// and the trailing `{card,product}_id` lets the `IN (…)` membership test be checked
/// index-only — so it reads only the newest handful of entries (measured: 30 entries, 6
/// buffers, ~2 ms) instead of the whole history. Trailing the id also keeps this robust to the
/// churned visibility map the snapshot leaves behind (the m..031 concern): right after a fresh,
/// un-VACUUMed capture the backward scan pays ~30 heap-visibility fetches at the tail, not
/// 176k — the tiny-point-seek shape the covering-index audit endorsed (contrast the rejected
/// full-table index-only scans in `docs/tradeoffs.md` §Price history).
///
/// Trade-off (see `docs/tradeoffs.md`): like `m..031` this is another large, never-pruned
/// index on the unbounded price-history tables — extra disk and a little write amplification on
/// the periodic snapshot batch. It is accepted because (a) the read it fixes is a
/// heap-fetch-heavy scan on a weak, cold instance run on every movers cache-miss, exactly the
/// class `m..031` was created to eliminate, and (b) the write cost is minimal: `as_of_date`
/// increases monotonically, so each day's inserts land at the B-tree's right edge (no random
/// writes, negligible bloat). It does **not** replace the covering index — the per-item
/// value-history scan still wants the `(game, id, as_of_date)` key order — and it is left
/// non-unique so it never becomes the snapshot's `ON CONFLICT` upsert target.
///
/// Notes:
/// - Plain (non-`CONCURRENTLY`) `CREATE INDEX`: `CONCURRENTLY` cannot run inside the
///   migration's transaction (mirrors `m..027`/`m..031`). Price writes happen only during the
///   periodic sync, so the build's `SHARE` lock is usually uncontended.
/// - `up()` issues `SET LOCAL statement_timeout = 0` on Postgres first (mirrors `m..031`): the
///   whole pending batch runs in one transaction, so a server/role-default `statement_timeout`
///   killing a slow build over the large table would roll the entire batch back and fail boot.
const CARD_INDEX: &str = "idx_card_price_history_game_date_card";
const PRODUCT_INDEX: &str = "idx_product_price_history_game_date_product";

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // One transaction for the whole batch, so a slow build must not hit a
        // server/role-default statement_timeout and roll everything back.
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
                    .name(CARD_INDEX)
                    .table(CardPriceHistory::Table)
                    .col(CardPriceHistory::Game)
                    .col(CardPriceHistory::AsOfDate)
                    .col(CardPriceHistory::CardId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name(PRODUCT_INDEX)
                    .table(ProductPriceHistory::Table)
                    .col(ProductPriceHistory::Game)
                    .col(ProductPriceHistory::AsOfDate)
                    .col(ProductPriceHistory::ProductId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name(PRODUCT_INDEX)
                    .table(ProductPriceHistory::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_index(
                Index::drop()
                    .name(CARD_INDEX)
                    .table(CardPriceHistory::Table)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum CardPriceHistory {
    Table,
    Game,
    CardId,
    AsOfDate,
}

#[derive(DeriveIden)]
enum ProductPriceHistory {
    Table,
    Game,
    ProductId,
    AsOfDate,
}

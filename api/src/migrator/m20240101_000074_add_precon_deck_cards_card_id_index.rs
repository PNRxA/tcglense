use sea_orm::DatabaseBackend;
use sea_orm_migration::prelude::*;

/// Index `precon_deck_cards` on `card_id` — the column the card page's "preconstructed
/// decks containing this card" lookup selects on, and one no index of `m..071` leads with.
///
/// `handlers::precons::read::card_precons` filters membership rows by `card_id IN
/// (<printing ids>)` alone (a precon card row carries no game column — the game scoping
/// arrives through the printing ids themselves), and `m..071` ships only
/// `(precon_deck_id, board, card_id, foil)` and `(precon_deck_id)`, neither of which can
/// serve a predicate on `card_id`. Without this, every card-page lookup reads the whole
/// table — the cost scales with the instance's precon catalog (~180k rows for MTG), not
/// with the one card asked about. The same "index both sides of the join" rule the `art:`
/// search leaf learned the hard way (`m..068`).
///
/// One column: the lookup projects deck id, board, quantity and foil, so no reachable
/// index is covering; the win is replacing the full scan with a bitmap lookup, and the
/// table is rewritten wholesale every sealed sync, so a wider index would only slow that.
///
/// Plain `CREATE INDEX` with `SET LOCAL statement_timeout = 0` on Postgres — the pattern
/// `m..066`/`m..069` set: the pending batch runs in one transaction on boot.
const INDEX: &str = "idx_precon_deck_cards_card_id";

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
                    .table(PreconDeckCards::Table)
                    .col(PreconDeckCards::CardId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name(INDEX)
                    .table(PreconDeckCards::Table)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum PreconDeckCards {
    Table,
    CardId,
}

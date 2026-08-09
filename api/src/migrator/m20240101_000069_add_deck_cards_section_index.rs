use sea_orm::DatabaseBackend;
use sea_orm_migration::prelude::*;

/// Index `deck_cards` on `section_id` — the column the deck list's derived facets select
/// on, and the one column of that table no index led with.
///
/// `handlers::decks::facets::deck_facets_by_deck` (#609) scopes both of its card
/// scans by section id alone, deliberately: a section belongs to exactly one deck, so
/// binding the deck ids a second time would be redundant. But `m..043` ships
/// `(deck_id, card_id, section_id)` and `(deck_id)`, and neither can serve a predicate
/// on `section_id` — so each scan read the whole table. That cost scales with **every
/// deck row in the instance**, not with the caller's decks, and it lands on the deck list
/// (authed and public) plus every rename, import and folder move that returns a header.
///
/// Measured on Postgres 16, 4 000 decks / 500k `deck_cards`, one user's 20-deck list:
///
/// | scan                    | before                      | after      |
/// |-------------------------|-----------------------------|------------|
/// | command-zone cards      | 28.3 ms (parallel seq scan) | **4.4 ms** |
/// | colour-identity union   | 29.8 ms (parallel seq scan) | **8.0 ms** |
///
/// ~58 ms → ~12 ms per header build, and the gap widens as the table grows: the "before"
/// side is a full scan, so it tracks total rows while the "after" side tracks the page's
/// own sections.
///
/// Notes:
/// - **One column.** The scans project `deck_id` and `card_id` (they join `cards` for the
///   name/colours), so no reachable index is covering; the win is the bitmap lookup that
///   replaces the scan, and a wider index would only cost write time on a hot table.
/// - `card_counts_by_deck`'s `section_id NOT IN (<maybeboard ids>)` is an *anti*-join
///   against a subquery, already filtered by `deck_id` — it is not what this index is for
///   and its plan is unchanged.
/// - Plain `CREATE INDEX` with `SET LOCAL statement_timeout = 0` on Postgres, the pattern
///   `m..066`/`m..050` set: the pending batch runs in one transaction on boot.
const INDEX: &str = "idx_deck_cards_section_id";

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
                    .table(DeckCards::Table)
                    .col(DeckCards::SectionId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(Index::drop().name(INDEX).table(DeckCards::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum DeckCards {
    Table,
    SectionId,
}

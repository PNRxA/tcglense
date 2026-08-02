use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // A tracked game had exactly one number per seat: `life`. Commander damage is one of the
        // two ways a Commander game actually ends, and poison/energy/experience are ordinary
        // tabletop state, so a pod that died to a commander was being logged as if it died to
        // life loss.
        //
        // The event, not the seat, is what grows: `life_events` already carries a `kind`
        // discriminator and is already re-folded by an undo, so a second axis on the same row
        // gets every counter the existing replay guarantees for free — rather than five more
        // denormalised columns on `life_session_players`, each needing its own writer and its
        // own undo path.
        manager
            .alter_table(
                Table::alter()
                    .table(LifeEvents::Table)
                    .add_column(
                        ColumnDef::new(LifeEvents::Counter)
                            .string()
                            .not_null()
                            // Every row written before this migration was a life change, so the
                            // default backfills the whole existing history correctly and no data
                            // statement is needed.
                            .default("life"),
                    )
                    .to_owned(),
            )
            .await?;

        // Which seat's commander dealt the damage. Set only for `commander_damage`, null for
        // every other counter.
        //
        // FK-less and orphan-tolerant, the call `life_session_players.deck_id` makes: a seat
        // removed mid-game (someone scooped and was taken off the table) cascades away with
        // *its own* events, but must not delete the damage it dealt to the players still
        // sitting there — that history is theirs, and the honest reading of a source that no
        // longer resolves is "an opponent who has left", not a deleted row.
        manager
            .alter_table(
                Table::alter()
                    .table(LifeEvents::Table)
                    .add_column(ColumnDef::new(LifeEvents::SourcePlayerId).integer().null())
                    .to_owned(),
            )
            .await?;

        // Which counters this game tracks, as a CSV of slugs (`life` is implicit and always
        // tracked, so it is never listed). Per-session rather than global because a Standard pod
        // has no business seeing a commander-damage matrix — the same reason `layout` is a
        // session column rather than a user preference.
        manager
            .alter_table(
                Table::alter()
                    .table(LifeSessions::Table)
                    .add_column(
                        ColumnDef::new(LifeSessions::Counters)
                            .string()
                            .not_null()
                            .default(""),
                    )
                    .to_owned(),
            )
            .await?;

        // Backfill the games already played, from the only signal there is: a Commander pod was
        // tracking commander damage in someone's head whether or not we stored it, so its
        // sessions open with the matrix on. Everything else keeps the empty default — a counter
        // nobody used is noise on the mat, and it is one tap to turn on.
        //
        // Written through the query builder (not raw SQL) so it runs on both backends.
        let commander_formats = ["commander", "edh", "brawl", "oathbreaker", "duel commander"];
        manager
            .exec_stmt(
                Query::update()
                    .table(LifeSessions::Table)
                    .value(LifeSessions::Counters, "commander_damage")
                    .and_where(
                        Expr::expr(Func::lower(Expr::col(LifeSessions::Format)))
                            .is_in(commander_formats),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(LifeSessions::Table)
                    .drop_column(LifeSessions::Counters)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(LifeEvents::Table)
                    .drop_column(LifeEvents::SourcePlayerId)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(LifeEvents::Table)
                    .drop_column(LifeEvents::Counter)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum LifeEvents {
    Table,
    Counter,
    SourcePlayerId,
}

#[derive(DeriveIden)]
enum LifeSessions {
    Table,
    Format,
    Counters,
}

use sea_orm_migration::prelude::*;

/// Creates `combos` + `combo_pieces` (issue #683): the Commander Spellbook combo database,
/// keyed by **oracle_id** so every printing of a card participates.
///
/// Both tables are rebuilt wholesale by `spellbook::ingest` on every changed upstream
/// version (the same posture as `card_rulings` / the art tags), so neither carries a
/// foreign key onto `cards` — a piece the catalog doesn't hold still describes the combo
/// — and the `combos.id` is never on the wire (`external_id`, upstream's own variant id,
/// is the identity).
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Combos::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Combos::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Combos::Game).string().not_null())
                    .col(ColumnDef::new(Combos::ExternalId).string().not_null())
                    .col(ColumnDef::new(Combos::ColorIdentity).string().not_null())
                    .col(ColumnDef::new(Combos::ManaNeeded).string().not_null())
                    .col(ColumnDef::new(Combos::ManaValueNeeded).integer().null())
                    .col(ColumnDef::new(Combos::Prerequisites).text().not_null())
                    .col(ColumnDef::new(Combos::Description).text().not_null())
                    .col(ColumnDef::new(Combos::Notes).text().not_null())
                    .col(ColumnDef::new(Combos::Popularity).integer().not_null())
                    .col(ColumnDef::new(Combos::BracketTag).string().not_null())
                    .col(ColumnDef::new(Combos::PieceCount).integer().not_null())
                    .col(ColumnDef::new(Combos::TemplateCount).integer().not_null())
                    .col(ColumnDef::new(Combos::Templates).text().not_null())
                    .col(ColumnDef::new(Combos::Produces).text().not_null())
                    .col(ColumnDef::new(Combos::CommanderLegal).boolean().not_null())
                    .to_owned(),
            )
            .await?;

        // The URL identity: one row per upstream variant per game.
        manager
            .create_index(
                Index::create()
                    .name("idx_combos_game_external_id")
                    .table(Combos::Table)
                    .col(Combos::Game)
                    .col(Combos::ExternalId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(ComboPieces::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ComboPieces::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(ComboPieces::Game).string().not_null())
                    .col(ColumnDef::new(ComboPieces::ComboId).integer().not_null())
                    .col(ColumnDef::new(ComboPieces::OracleId).string().not_null())
                    .col(ColumnDef::new(ComboPieces::Name).string().not_null())
                    .col(ColumnDef::new(ComboPieces::Quantity).integer().not_null())
                    .col(
                        ColumnDef::new(ComboPieces::MustBeCommander)
                            .boolean()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ComboPieces::ZoneLocations)
                            .string()
                            .not_null(),
                    )
                    .col(ColumnDef::new(ComboPieces::Position).integer().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_combo_pieces_combo")
                            .from(ComboPieces::Table, ComboPieces::ComboId)
                            .to(Combos::Table, Combos::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // The deck read and the card page both ask "which combos hold this oracle id":
        // driven from the card side, the combo id rides along for the join to the parent.
        manager
            .create_index(
                Index::create()
                    .name("idx_combo_pieces_game_oracle_id")
                    .table(ComboPieces::Table)
                    .col(ComboPieces::Game)
                    .col(ComboPieces::OracleId)
                    .col(ComboPieces::ComboId)
                    .to_owned(),
            )
            .await?;
        // Hydrating a combo's pieces, and the cascade on rebuild.
        manager
            .create_index(
                Index::create()
                    .name("idx_combo_pieces_combo_id")
                    .table(ComboPieces::Table)
                    .col(ComboPieces::ComboId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(ComboPieces::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Combos::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Combos {
    Table,
    Id,
    Game,
    ExternalId,
    ColorIdentity,
    ManaNeeded,
    ManaValueNeeded,
    Prerequisites,
    Description,
    Notes,
    Popularity,
    BracketTag,
    PieceCount,
    TemplateCount,
    Templates,
    Produces,
    CommanderLegal,
}

#[derive(DeriveIden)]
enum ComboPieces {
    Table,
    Id,
    Game,
    ComboId,
    OracleId,
    Name,
    Quantity,
    MustBeCommander,
    ZoneLocations,
    Position,
}

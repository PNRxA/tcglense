use sea_orm_migration::prelude::*;

/// Booster **print sheets with their weights** and **pack slot configurations** (issue
/// #682): the data a booster's expected value and a simulated "open a pack" need, which
/// the sealed-contents sync used to discard.
///
/// Three tables, all catalog data rebuilt wholesale by the MTGJSON sealed sync
/// (`crate::mtgjson::ingest::boosters`) in the same transaction as `sealed_contents`:
///
/// - `booster_configs` — one row per `(game, set_code, code)`: the booster's name, its pack
///   variants (a JSON list of `{ weight, slots: [[sheet, count]…] }`) and their total weight.
/// - `booster_sheets` — one row per `(config, sheet name)`: the sheet's flags (foil, fixed,
///   colour-balanced, duplicates allowed), its total weight, and its cards as a JSON
///   `[[card_id, weight]…]` list in upstream order. JSON rather than a rows table because the
///   only read is product-keyed and needs whole sheets, and a normalised table would hold on
///   the order of a million rows nothing queries by card.
/// - `sealed_packs` — one row per `(game, product, config)`: how many of that booster one
///   copy of the product opens, with nested box → pack references flattened at ingest.
///
/// Row ids are not stable across rebuilds and never reach the wire. `product_id` and the
/// sheet's card ids are FK-less (orphan-tolerant, like every other catalog link); the
/// config → sheet / pack links cascade, and the ingest deletes children explicitly anyway
/// because SQLite doesn't enforce foreign keys by default.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(BoosterConfigs::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(BoosterConfigs::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(BoosterConfigs::Game).string().not_null())
                    // Lowercased set code, as `cards.set_code` / `products.set_code` store it.
                    .col(ColumnDef::new(BoosterConfigs::SetCode).string().not_null())
                    // MTGJSON's booster key (`play`, `collector`, `draft`, …).
                    .col(ColumnDef::new(BoosterConfigs::Code).string().not_null())
                    .col(ColumnDef::new(BoosterConfigs::Name).string().null())
                    // Σ of the stored variants' weights.
                    .col(
                        ColumnDef::new(BoosterConfigs::TotalWeight)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    // JSON list of pack variants (see `entities::booster_config::Variant`).
                    .col(ColumnDef::new(BoosterConfigs::Variants).text().not_null())
                    .col(
                        ColumnDef::new(BoosterConfigs::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(BoosterConfigs::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_booster_configs_game_set_code")
                    .table(BoosterConfigs::Table)
                    .col(BoosterConfigs::Game)
                    .col(BoosterConfigs::SetCode)
                    .col(BoosterConfigs::Code)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(BoosterSheets::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(BoosterSheets::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(BoosterSheets::ConfigId).integer().not_null())
                    .col(ColumnDef::new(BoosterSheets::Name).string().not_null())
                    .col(
                        ColumnDef::new(BoosterSheets::Foil)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(BoosterSheets::BalanceColors)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(BoosterSheets::AllowDuplicates)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(BoosterSheets::Fixed)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    // Upstream's `totalWeight` (else Σ parsed weights), counted before any
                    // unresolved card is dropped — see the entity.
                    .col(
                        ColumnDef::new(BoosterSheets::TotalWeight)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    // JSON `[[card_id, weight], …]` in upstream order.
                    .col(ColumnDef::new(BoosterSheets::Cards).text().not_null())
                    .col(
                        ColumnDef::new(BoosterSheets::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(BoosterSheets::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    // Rebuilding a configuration removes its sheets.
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_booster_sheets_config_id")
                            .from(BoosterSheets::Table, BoosterSheets::ConfigId)
                            .to(BoosterConfigs::Table, BoosterConfigs::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_booster_sheets_config_name")
                    .table(BoosterSheets::Table)
                    .col(BoosterSheets::ConfigId)
                    .col(BoosterSheets::Name)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(SealedPacks::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(SealedPacks::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(SealedPacks::Game).string().not_null())
                    // `products.id`, deliberately FK-less (orphan-tolerant, like
                    // `sealed_contents.product_id`).
                    .col(ColumnDef::new(SealedPacks::ProductId).integer().not_null())
                    .col(ColumnDef::new(SealedPacks::ConfigId).integer().not_null())
                    .col(
                        ColumnDef::new(SealedPacks::Quantity)
                            .integer()
                            .not_null()
                            .default(1),
                    )
                    .col(
                        ColumnDef::new(SealedPacks::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(SealedPacks::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_sealed_packs_config_id")
                            .from(SealedPacks::Table, SealedPacks::ConfigId)
                            .to(BoosterConfigs::Table, BoosterConfigs::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        // One row per (game, product, configuration): two paths to the same booster sum.
        manager
            .create_index(
                Index::create()
                    .name("idx_sealed_packs_unique")
                    .table(SealedPacks::Table)
                    .col(SealedPacks::Game)
                    .col(SealedPacks::ProductId)
                    .col(SealedPacks::ConfigId)
                    .unique()
                    .to_owned(),
            )
            .await?;
        // The product-keyed read: every `/ev` and `/open` starts here.
        manager
            .create_index(
                Index::create()
                    .name("idx_sealed_packs_game_product")
                    .table(SealedPacks::Table)
                    .col(SealedPacks::Game)
                    .col(SealedPacks::ProductId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Children first: the FK cascades on Postgres, but dropping in dependency order
        // works on both backends without relying on it.
        manager
            .drop_table(Table::drop().table(SealedPacks::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(BoosterSheets::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(BoosterConfigs::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum BoosterConfigs {
    Table,
    Id,
    Game,
    SetCode,
    Code,
    Name,
    TotalWeight,
    Variants,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum BoosterSheets {
    Table,
    Id,
    ConfigId,
    Name,
    Foil,
    BalanceColors,
    AllowDuplicates,
    Fixed,
    TotalWeight,
    Cards,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum SealedPacks {
    Table,
    Id,
    Game,
    ProductId,
    ConfigId,
    Quantity,
    CreatedAt,
    UpdatedAt,
}

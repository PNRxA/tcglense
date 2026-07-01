use sea_orm_migration::prelude::*;

/// Adds the remaining `default_cards` fields the Scryfall search syntax filters on
/// (legalities, keywords, artist, finishes, promo/frame flags, ranks, …). All
/// nullable (default NULL) so the ADD COLUMNs are valid on SQLite and the next card
/// import backfills them.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // SQLite allows only one ALTER option per statement, so add each column
        // in its own `ALTER TABLE`.
        for column in [
            ColumnDef::new(Cards::PriceUsdEtched).string().null().to_owned(),
            ColumnDef::new(Cards::Keywords).text().null().to_owned(),
            ColumnDef::new(Cards::ProducedMana).text().null().to_owned(),
            ColumnDef::new(Cards::ColorIndicator).text().null().to_owned(),
            ColumnDef::new(Cards::Watermark).string().null().to_owned(),
            ColumnDef::new(Cards::FlavorText).text().null().to_owned(),
            ColumnDef::new(Cards::IllustrationId).string().null().to_owned(),
            ColumnDef::new(Cards::Artist).string().null().to_owned(),
            ColumnDef::new(Cards::ArtistIds).text().null().to_owned(),
            ColumnDef::new(Cards::BorderColor).string().null().to_owned(),
            ColumnDef::new(Cards::Frame).string().null().to_owned(),
            ColumnDef::new(Cards::FrameEffects).text().null().to_owned(),
            ColumnDef::new(Cards::SecurityStamp).string().null().to_owned(),
            ColumnDef::new(Cards::PromoTypes).text().null().to_owned(),
            ColumnDef::new(Cards::Finishes).text().null().to_owned(),
            ColumnDef::new(Cards::Defense).string().null().to_owned(),
            ColumnDef::new(Cards::Legalities).text().null().to_owned(),
            ColumnDef::new(Cards::FullArt).boolean().null().to_owned(),
            ColumnDef::new(Cards::Textless).boolean().null().to_owned(),
            ColumnDef::new(Cards::Oversized).boolean().null().to_owned(),
            ColumnDef::new(Cards::Promo).boolean().null().to_owned(),
            ColumnDef::new(Cards::Reprint).boolean().null().to_owned(),
            ColumnDef::new(Cards::Variation).boolean().null().to_owned(),
            ColumnDef::new(Cards::Booster).boolean().null().to_owned(),
            ColumnDef::new(Cards::StorySpotlight).boolean().null().to_owned(),
            ColumnDef::new(Cards::ContentWarning).boolean().null().to_owned(),
            ColumnDef::new(Cards::HighresImage).boolean().null().to_owned(),
            ColumnDef::new(Cards::Reserved).boolean().null().to_owned(),
            ColumnDef::new(Cards::GameChanger).boolean().null().to_owned(),
            ColumnDef::new(Cards::EdhrecRank).integer().null().to_owned(),
            ColumnDef::new(Cards::PennyRank).integer().null().to_owned(),
        ] {
            manager
                .alter_table(
                    Table::alter()
                        .table(Cards::Table)
                        .add_column(column)
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for column in [
            Cards::PriceUsdEtched,
            Cards::Keywords,
            Cards::ProducedMana,
            Cards::ColorIndicator,
            Cards::Watermark,
            Cards::FlavorText,
            Cards::IllustrationId,
            Cards::Artist,
            Cards::ArtistIds,
            Cards::BorderColor,
            Cards::Frame,
            Cards::FrameEffects,
            Cards::SecurityStamp,
            Cards::PromoTypes,
            Cards::Finishes,
            Cards::Defense,
            Cards::Legalities,
            Cards::FullArt,
            Cards::Textless,
            Cards::Oversized,
            Cards::Promo,
            Cards::Reprint,
            Cards::Variation,
            Cards::Booster,
            Cards::StorySpotlight,
            Cards::ContentWarning,
            Cards::HighresImage,
            Cards::Reserved,
            Cards::GameChanger,
            Cards::EdhrecRank,
            Cards::PennyRank,
        ] {
            manager
                .alter_table(
                    Table::alter()
                        .table(Cards::Table)
                        .drop_column(column)
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Cards {
    Table,
    PriceUsdEtched,
    Keywords,
    ProducedMana,
    ColorIndicator,
    Watermark,
    FlavorText,
    IllustrationId,
    Artist,
    ArtistIds,
    BorderColor,
    Frame,
    FrameEffects,
    SecurityStamp,
    PromoTypes,
    Finishes,
    Defense,
    Legalities,
    FullArt,
    Textless,
    Oversized,
    Promo,
    Reprint,
    Variation,
    Booster,
    StorySpotlight,
    ContentWarning,
    HighresImage,
    Reserved,
    GameChanger,
    EdhrecRank,
    PennyRank,
}

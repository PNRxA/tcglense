use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(CardFingerprint::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(CardFingerprint::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(CardFingerprint::Game).string().not_null())
                    // Scryfall card id (a UUID) — the same external key holdings use, so a
                    // match resolves straight to a card without a fragile internal-id path.
                    .col(
                        ColumnDef::new(CardFingerprint::ExternalId)
                            .string()
                            .not_null(),
                    )
                    // 0 for a single-faced card / the front of a double-faced one. Kept so
                    // per-face fingerprints (a flipped MDFC) can be added without a migration.
                    .col(
                        ColumnDef::new(CardFingerprint::FaceIndex)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    // Which fingerprint algorithm+parameters produced these bytes. Bumped to
                    // force a rebuild + a client cache-bust when the algorithm changes; the
                    // in-memory match index only loads rows at the current version.
                    .col(
                        ColumnDef::new(CardFingerprint::AlgoVersion)
                            .integer()
                            .not_null(),
                    )
                    // The perceptual hash itself — an opaque fixed-width BLOB (BYTEA on
                    // Postgres). Never queried by content in SQL: nearest-neighbour search is
                    // a Hamming scan in Rust/WASM, so this stays a plain column on both
                    // backends (no pgvector / sqlite-vec, no dialect-specific popcount).
                    .col(
                        ColumnDef::new(CardFingerprint::Fingerprint)
                            .binary()
                            .not_null(),
                    )
                    // Which image variant was hashed (e.g. `small`) — recorded so a later
                    // switch to a different source size is a detectable, rebuildable change.
                    .col(
                        ColumnDef::new(CardFingerprint::SourceSize)
                            .string()
                            .not_null(),
                    )
                    // SHA-256 (hex) of the fetched source-image bytes: lets an incremental
                    // rebuild skip a card whose art is byte-identical to what was hashed.
                    .col(
                        ColumnDef::new(CardFingerprint::SourceImageHash)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(CardFingerprint::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(CardFingerprint::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // One fingerprint row per (game, card, face): a rebuild upserts on this key rather
        // than inserting duplicates. Also the covering index for the `cards LEFT JOIN
        // card_fingerprint ... IS NULL` scan that enumerates cards still needing a build.
        manager
            .create_index(
                Index::create()
                    .name("idx_card_fingerprint_game_external_face")
                    .table(CardFingerprint::Table)
                    .col(CardFingerprint::Game)
                    .col(CardFingerprint::ExternalId)
                    .col(CardFingerprint::FaceIndex)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // Loading the current in-memory match index is `WHERE game = ? AND algo_version = ?`;
        // this keeps that off a full scan of the (potentially large) fingerprint table.
        manager
            .create_index(
                Index::create()
                    .name("idx_card_fingerprint_game_algo_version")
                    .table(CardFingerprint::Table)
                    .col(CardFingerprint::Game)
                    .col(CardFingerprint::AlgoVersion)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(CardFingerprint::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum CardFingerprint {
    Table,
    Id,
    Game,
    ExternalId,
    FaceIndex,
    AlgoVersion,
    Fingerprint,
    SourceSize,
    SourceImageHash,
    CreatedAt,
    UpdatedAt,
}

use sea_orm_migration::prelude::*;

/// Adds the three indexes a pre-launch audit of every query pattern found missing.
///
/// - `collection_items (user_id, game, updated_at, id)` and its wish-list twin:
///   the default list sort (`CollectionSort::Recent` in
///   `handlers::collection/wishlist::read`) filters by `(user_id, game)` and orders
///   by `updated_at, id`, but the only existing index — the unique
///   `(user_id, game, card_id)` — can't serve that sort, so every page materializes
///   and sorts the user's whole holding set. Equality columns lead, sort keys
///   follow; because the handler applies one shared direction to both sort columns,
///   this single ascending index serves the default `DESC` (backward scan) and the
///   reversed `ASC` (forward scan) alike, and the trailing `id` makes the order
///   total so equal `updated_at` stamps (bulk imports) still paginate
///   deterministically.
/// - `refresh_tokens (expires_at)`: `auth::refresh::prune_expired` (the 6h
///   maintenance loop) deletes by `expires_at <= now` and otherwise full-scans a
///   table that accumulates ~30 days of minted + rotated tokens; on SQLite the
///   scan-DELETE holds the single writer for its duration. `expires_at` is set once
///   at insert and never updated, so the index is insert-only.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_index(
                Index::create()
                    .name("idx_collection_items_user_game_updated_at_id")
                    .table(CollectionItems::Table)
                    .col(CollectionItems::UserId)
                    .col(CollectionItems::Game)
                    .col(CollectionItems::UpdatedAt)
                    .col(CollectionItems::Id)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_wishlist_items_user_game_updated_at_id")
                    .table(WishlistItems::Table)
                    .col(WishlistItems::UserId)
                    .col(WishlistItems::Game)
                    .col(WishlistItems::UpdatedAt)
                    .col(WishlistItems::Id)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_refresh_tokens_expires_at")
                    .table(RefreshTokens::Table)
                    .col(RefreshTokens::ExpiresAt)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_refresh_tokens_expires_at")
                    .table(RefreshTokens::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_index(
                Index::drop()
                    .name("idx_wishlist_items_user_game_updated_at_id")
                    .table(WishlistItems::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_index(
                Index::drop()
                    .name("idx_collection_items_user_game_updated_at_id")
                    .table(CollectionItems::Table)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum CollectionItems {
    Table,
    Id,
    UserId,
    Game,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum WishlistItems {
    Table,
    Id,
    UserId,
    Game,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum RefreshTokens {
    Table,
    ExpiresAt,
}

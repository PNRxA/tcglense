use sea_orm_migration::prelude::*;

mod m20240101_000001_create_users_table;
mod m20240101_000002_create_refresh_tokens_table;
mod m20240101_000003_add_replaced_by_to_refresh_tokens;
mod m20240101_000004_create_card_sets_table;
mod m20240101_000005_create_cards_table;
mod m20240101_000006_create_ingest_state_table;
mod m20240101_000007_add_text_stats_to_cards;
mod m20240101_000008_create_card_price_history_table;
mod m20240101_000009_add_cards_oracle_id_index;
mod m20240101_000010_create_collection_items_table;
mod m20240101_000011_create_collection_sources_table;
mod m20240101_000012_add_collection_source_smart;
mod m20240101_000013_add_scryfall_fields_to_cards;
mod m20240101_000014_add_user_email_verified_at;
mod m20240101_000015_create_email_tokens_table;
mod m20240101_000016_create_wishlist_items_table;
mod m20240101_000017_make_user_password_hash_nullable;
mod m20240101_000018_add_tcgplayer_ids_to_cards;
mod m20240101_000019_create_products_table;
mod m20240101_000020_create_product_price_history_table;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20240101_000001_create_users_table::Migration),
            Box::new(m20240101_000002_create_refresh_tokens_table::Migration),
            Box::new(m20240101_000003_add_replaced_by_to_refresh_tokens::Migration),
            Box::new(m20240101_000004_create_card_sets_table::Migration),
            Box::new(m20240101_000005_create_cards_table::Migration),
            Box::new(m20240101_000006_create_ingest_state_table::Migration),
            Box::new(m20240101_000007_add_text_stats_to_cards::Migration),
            Box::new(m20240101_000008_create_card_price_history_table::Migration),
            Box::new(m20240101_000009_add_cards_oracle_id_index::Migration),
            Box::new(m20240101_000010_create_collection_items_table::Migration),
            Box::new(m20240101_000011_create_collection_sources_table::Migration),
            Box::new(m20240101_000012_add_collection_source_smart::Migration),
            Box::new(m20240101_000013_add_scryfall_fields_to_cards::Migration),
            Box::new(m20240101_000014_add_user_email_verified_at::Migration),
            Box::new(m20240101_000015_create_email_tokens_table::Migration),
            Box::new(m20240101_000016_create_wishlist_items_table::Migration),
            Box::new(m20240101_000017_make_user_password_hash_nullable::Migration),
            Box::new(m20240101_000018_add_tcgplayer_ids_to_cards::Migration),
            Box::new(m20240101_000019_create_products_table::Migration),
            Box::new(m20240101_000020_create_product_price_history_table::Migration),
        ]
    }
}

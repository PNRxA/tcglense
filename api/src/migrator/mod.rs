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
mod m20240101_000021_create_sealed_contents_table;
mod m20240101_000022_add_recency_and_prune_indexes;
mod m20240101_000023_consolidate_foil_star_holdings;
mod m20240101_000024_add_cards_set_code_collector_number_index;
mod m20240101_000025_add_browse_sort_and_email_prune_indexes;
mod m20240101_000026_add_cards_game_finishes_index;
mod m20240101_000027_add_cards_trgm_search_indexes;
mod m20240101_000028_create_sealed_components_table;
mod m20240101_000029_add_msrp_to_products;
mod m20240101_000030_create_api_keys_table;
mod m20240101_000031_add_card_price_history_covering_index;
mod m20240101_000032_create_card_fingerprints_table;
mod m20240101_000033_add_cards_subtype_facet_index;
mod m20240101_000034_add_cards_subtype_partial_index;
mod m20240101_000035_create_wishlist_product_items_table;
mod m20240101_000036_add_username_and_discriminator_to_users;
mod m20240101_000037_create_collection_visibility_table;
mod m20240101_000038_drop_user_display_name;
mod m20240101_000039_add_collection_display_prefs;
mod m20240101_000040_create_deck_folders_table;
mod m20240101_000041_create_decks_table;
mod m20240101_000042_create_deck_sections_table;
mod m20240101_000043_create_deck_cards_table;
mod m20240101_000044_add_cards_foil_variant_star_index;
mod m20240101_000045_add_sealed_components_child_product_index;
mod m20240101_000046_add_refresh_token_family;
mod m20240101_000047_add_user_currency;
mod m20240101_000048_add_auth_session_version;
mod m20240101_000049_create_collection_product_items_table;
mod m20240101_000050_add_price_history_latest_date_indexes;
mod m20240101_000051_add_wishlist_visibility;

#[cfg(test)]
pub(crate) use m20240101_000023_consolidate_foil_star_holdings::consolidate_foil_star_holdings;

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
            Box::new(m20240101_000021_create_sealed_contents_table::Migration),
            Box::new(m20240101_000022_add_recency_and_prune_indexes::Migration),
            Box::new(m20240101_000023_consolidate_foil_star_holdings::Migration),
            Box::new(m20240101_000024_add_cards_set_code_collector_number_index::Migration),
            Box::new(m20240101_000025_add_browse_sort_and_email_prune_indexes::Migration),
            Box::new(m20240101_000026_add_cards_game_finishes_index::Migration),
            Box::new(m20240101_000027_add_cards_trgm_search_indexes::Migration),
            Box::new(m20240101_000028_create_sealed_components_table::Migration),
            Box::new(m20240101_000029_add_msrp_to_products::Migration),
            Box::new(m20240101_000030_create_api_keys_table::Migration),
            Box::new(m20240101_000031_add_card_price_history_covering_index::Migration),
            Box::new(m20240101_000032_create_card_fingerprints_table::Migration),
            Box::new(m20240101_000033_add_cards_subtype_facet_index::Migration),
            Box::new(m20240101_000034_add_cards_subtype_partial_index::Migration),
            Box::new(m20240101_000035_create_wishlist_product_items_table::Migration),
            Box::new(m20240101_000036_add_username_and_discriminator_to_users::Migration),
            Box::new(m20240101_000037_create_collection_visibility_table::Migration),
            Box::new(m20240101_000038_drop_user_display_name::Migration),
            Box::new(m20240101_000039_add_collection_display_prefs::Migration),
            Box::new(m20240101_000040_create_deck_folders_table::Migration),
            Box::new(m20240101_000041_create_decks_table::Migration),
            Box::new(m20240101_000042_create_deck_sections_table::Migration),
            Box::new(m20240101_000043_create_deck_cards_table::Migration),
            Box::new(m20240101_000044_add_cards_foil_variant_star_index::Migration),
            Box::new(m20240101_000045_add_sealed_components_child_product_index::Migration),
            Box::new(m20240101_000046_add_refresh_token_family::Migration),
            Box::new(m20240101_000047_add_user_currency::Migration),
            Box::new(m20240101_000048_add_auth_session_version::Migration),
            Box::new(m20240101_000049_create_collection_product_items_table::Migration),
            Box::new(m20240101_000050_add_price_history_latest_date_indexes::Migration),
            Box::new(m20240101_000051_add_wishlist_visibility::Migration),
        ]
    }
}

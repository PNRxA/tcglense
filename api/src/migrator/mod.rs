use sea_orm_migration::prelude::*;

mod m20240101_000001_create_users_table;
mod m20240101_000002_create_refresh_tokens_table;
mod m20240101_000003_add_replaced_by_to_refresh_tokens;
mod m20240101_000004_create_card_sets_table;
mod m20240101_000005_create_cards_table;
mod m20240101_000006_create_ingest_state_table;

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
        ]
    }
}

use sea_orm::entity::prelude::*;

/// The persisted Secret Lair drop snapshot (issue: restart-refresh). One row, keyed by a constant
/// `snapshot_key` discriminator (see `scryfall::sld_persist`), holding the canonical snapshot JSON so
/// the in-memory drop store can reseed from the last-good scrape/import on boot instead of the
/// committed `sld_drops.json` seed. `snapshot_json` is a `.text()` column; a plain `String` field
/// maps it on both SQLite and Postgres (the codebase never annotates `column_type = "Text"`).
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "sld_drop_snapshot")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub snapshot_key: String,
    pub snapshot_json: String,
    pub content_version: String,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

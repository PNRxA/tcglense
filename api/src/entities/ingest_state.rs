use sea_orm::entity::prelude::*;

/// SeaORM entity for the `ingest_state` table.
///
/// Bookkeeping for the background import of an external dataset (e.g. Scryfall
/// `default_cards`) per game. One row per `(game, dataset)`; the import compares
/// `source_updated_at` against the provider's current value to skip re-importing
/// an unchanged dataset, and surfaces `status` so the UI can show progress.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "ingest_state")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Game discriminator, e.g. `"mtg"`.
    pub game: String,
    /// Dataset key, e.g. `"default_cards"`.
    pub dataset: String,
    /// The provider's `updated_at` for the dataset we last imported.
    pub source_updated_at: Option<String>,
    /// `"idle"` | `"running"` | `"complete"` | `"error"`.
    pub status: String,
    /// Human-readable detail (progress note or error message).
    pub detail: Option<String>,
    pub sets_imported: i32,
    pub cards_imported: i32,
    pub started_at: Option<DateTimeUtc>,
    pub finished_at: Option<DateTimeUtc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

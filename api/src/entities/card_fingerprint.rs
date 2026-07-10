use sea_orm::entity::prelude::*;

/// SeaORM entity for the `card_fingerprint` table.
///
/// A perceptual-hash fingerprint of one card printing's image, used by the visual
/// scanner to identify a photographed card without OCR. Keyed by the same external
/// (Scryfall) id holdings use, so a match resolves straight to a card. The bytes are
/// an opaque fixed-width hash; nearest-neighbour search is a Hamming scan done in
/// Rust (or in the browser), never in SQL — so this is a plain BLOB/BYTEA column that
/// behaves identically on SQLite and Postgres.
///
/// Rows are produced by the opt-in fingerprint build task (see
/// [`crate::catalog::fingerprints`]); a normal self-host imports a prebuilt index and
/// never runs the build itself.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "card_fingerprint")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Game discriminator, e.g. `"mtg"`.
    pub game: String,
    /// Provider id of the printing (Scryfall card id, a UUID), unique within a game.
    pub external_id: String,
    /// `0` for a single-faced card / the front of a double-faced one.
    pub face_index: i32,
    /// Which fingerprint algorithm + parameters produced these bytes (bumped to force
    /// a rebuild and a client cache-bust); the match index loads only the current one.
    pub algo_version: i32,
    /// The perceptual hash — an opaque fixed-width byte string (32 bytes for the
    /// 256-bit pHash). Never compared in SQL.
    pub fingerprint: Vec<u8>,
    /// Which image variant was hashed (e.g. `"small"`).
    pub source_size: String,
    /// SHA-256 (hex) of the fetched source-image bytes, so a rebuild can skip a card
    /// whose art is byte-identical to what was already hashed.
    pub source_image_hash: String,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

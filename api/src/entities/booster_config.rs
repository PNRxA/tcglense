use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// SeaORM entity for the `booster_configs` table: one **booster configuration** — the
/// pack variants a booster rolls and the sheets they draw from — sourced from
/// [MTGJSON](https://mtgjson.com)'s per-set `booster` map (see [`crate::mtgjson::boosters`]).
///
/// A row is keyed by `(game, set_code, code)`: `code` is MTGJSON's booster key (`play`,
/// `collector`, `draft`, `set`, `default`, …) and `set_code` the lowercased set it belongs
/// to. Its sheets live in [`super::booster_sheet`] and the products that open it link
/// through [`super::sealed_pack`]. Only configurations some catalog product actually opens
/// are stored — the read is product-keyed, so a configuration no product references is of
/// no use to it.
///
/// The variants are stored as a JSON column rather than a table: a configuration has a
/// handful of them, each a `weight` and a small `(sheet, count)` list, and every read wants
/// the whole list at once. The whole table (with its sheets and pack links) is rebuilt
/// wholesale on each sync, so a row id is not stable and never reaches the wire.
///
/// `Eq` is derivable — every column is an integer, string, option, or timestamp.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "booster_configs")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Game discriminator, e.g. `"mtg"`.
    pub game: String,
    /// Lowercased set code the configuration belongs to (`blb`).
    pub set_code: String,
    /// MTGJSON's booster key (`play`, `collector`, `draft`, …) — the second half of the
    /// identity, and what a product's `contents.pack` reference names.
    pub code: String,
    /// MTGJSON's display name for the booster (`Play Booster`), when it states one.
    pub name: Option<String>,
    /// Σ of the stored variants' weights — the denominator a variant's `weight` is a share
    /// of. Recomputed from the stored variants (upstream's `boostersTotalWeight` is only a
    /// cross-check), so the shares always sum to one over what's stored.
    pub total_weight: i64,
    /// JSON: the pack variants, in upstream order — see [`Variant`] / [`Model::variants`].
    pub variants: String,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

/// One pack **variant** ("configuration" upstream): with probability
/// `weight / total_weight` a pack is this variant, and then draws `count` cards from each
/// named sheet. The stored shape of the `variants` JSON column.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Variant {
    /// This variant's share of `Model::total_weight`.
    pub weight: u64,
    /// `(sheet name, cards drawn from it)`, sorted by sheet name — upstream states them as a
    /// JSON object, so there is no order to preserve and sorting makes the column
    /// deterministic across rebuilds.
    pub slots: Vec<(String, u32)>,
}

impl Model {
    /// The stored variants. A malformed column reads as **no** variants rather than failing
    /// the request: the column is written only by the ingest from a shape it just encoded,
    /// so a parse failure is a bug worth a warning, not a `500` on a public catalog read.
    pub fn variants(&self) -> Vec<Variant> {
        decode_variants(&self.variants)
    }
}

/// Parse a `variants` column; see [`Model::variants`] for the failure stance.
pub fn decode_variants(raw: &str) -> Vec<Variant> {
    match serde_json::from_str::<Vec<Variant>>(raw) {
        Ok(variants) => variants,
        Err(err) => {
            tracing::warn!(error = %err, "booster_configs.variants is not valid JSON; reading it as empty");
            Vec::new()
        }
    }
}

/// Encode variants for the `variants` column (the ingest's side of [`Model::variants`]).
/// Serialising a `Vec` of plain structs cannot fail; an empty list encodes as `[]`.
pub fn encode_variants(variants: &[Variant]) -> String {
    serde_json::to_string(variants).unwrap_or_else(|_| "[]".to_string())
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants_round_trip_through_the_column() {
        let variants = vec![
            Variant {
                weight: 3,
                slots: vec![("common".to_string(), 7), ("rare".to_string(), 1)],
            },
            Variant {
                weight: 1,
                slots: vec![("common".to_string(), 6), ("list".to_string(), 1)],
            },
        ];
        let encoded = encode_variants(&variants);
        assert_eq!(decode_variants(&encoded), variants);
        assert_eq!(encode_variants(&[]), "[]");
    }

    #[test]
    fn a_malformed_column_reads_as_no_variants() {
        assert!(decode_variants("not json").is_empty());
        assert!(decode_variants("").is_empty());
    }
}

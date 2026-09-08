use sea_orm::entity::prelude::*;

/// SeaORM entity for the `booster_sheets` table: one **print sheet** of a booster
/// configuration ([`super::booster_config`]) — the weighted card pool a pack slot draws
/// from — sourced from MTGJSON's `booster[code].sheets` (see [`crate::mtgjson::boosters`]).
///
/// One row per `(config, sheet name)`. The cards ride a JSON column of
/// `[[card_id, weight], …]` (internal `cards.id`s, upstream order) rather than a rows
/// table: the only read is product-keyed and always needs a whole sheet, and a
/// normalised table would hold on the order of a million rows nothing queries by card
/// (`docs/tradeoffs.md`). `card_id` is not foreign-keyed to `cards` (orphan-tolerant, like
/// every other card link); a card that didn't resolve to our catalog at ingest is dropped
/// from `cards` **but its weight stays in `total_weight`**, so a read can tell how much of
/// the sheet it can't account for rather than silently re-normalising over what it has.
///
/// Rebuilt wholesale with its configuration on every sync; a row id never reaches the wire.
///
/// `Eq` is derivable — every column is an integer, string, bool, or timestamp.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "booster_sheets")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// `booster_configs.id` this sheet belongs to.
    pub config_id: i32,
    /// The sheet's name (`common`, `rareMythic`, `foil`, …) — what a variant's slot names.
    pub name: String,
    /// Whether every card on the sheet is foil (a foil sheet prices at the foil price).
    pub foil: bool,
    /// Upstream balances colours when drawing the sheet. Recorded, not simulated.
    pub balance_colors: bool,
    /// Draws from the sheet may repeat a card (upstream: `allowDuplicates`). Without it,
    /// a pack's picks from one sheet are without replacement.
    pub allow_duplicates: bool,
    /// The sheet is a fixed list — a slot takes its cards **in order** rather than at random
    /// (a bundle's land pack, a precon-like insert). Upstream: `fixed`.
    pub fixed: bool,
    /// The denominator a card's weight is a share of: upstream's `totalWeight` when stated,
    /// else Σ of every parsed weight — computed **before** dropping cards our catalog
    /// doesn't hold, so `total_weight - Σ stored weights` is the unaccounted share.
    pub total_weight: i64,
    /// JSON: `[[card_id, weight], …]` in upstream order — see [`Model::cards`]. On a
    /// **fixed** sheet a card the catalog doesn't hold is kept as [`UNRESOLVED_CARD_ID`]
    /// rather than dropped, because that sheet is read *positionally*; on every other sheet
    /// it is dropped (its weight still stands in `total_weight` either way).
    pub cards: String,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

impl Model {
    /// The stored `(card id, weight)` pairs, in upstream order. A malformed column reads as
    /// **no** cards rather than failing the request (same stance as
    /// [`super::booster_config::Model::variants`]).
    pub fn cards(&self) -> Vec<(i32, u32)> {
        decode_cards(&self.cards)
    }
}

/// The `card_id` standing in for a card our catalog doesn't hold, on a **fixed** sheet
/// only. A fixed sheet's slot takes its first `count` cards *by position*, so compacting an
/// unresolvable card out of the list would shift every card after it — a bundle's land pack
/// would then attribute, and deal, the wrong printings. No real row can collide with it:
/// both backends start `cards.id` at 1.
///
/// A placeholder position is a real card of the pack that we simply can't name: it consumes
/// its pick (so `cards_per_pack` stays right), is priced at nothing, and its weight counts
/// as unaccounted for — exactly like the dropped card it stands in for. Every other sheet
/// is drawn by weight, where position means nothing, so there an unresolvable card is
/// dropped as before.
pub const UNRESOLVED_CARD_ID: i32 = 0;

/// Parse a `cards` column; see [`Model::cards`] for the failure stance.
pub fn decode_cards(raw: &str) -> Vec<(i32, u32)> {
    match serde_json::from_str::<Vec<(i32, u32)>>(raw) {
        Ok(cards) => cards,
        Err(err) => {
            tracing::warn!(error = %err, "booster_sheets.cards is not valid JSON; reading it as empty");
            Vec::new()
        }
    }
}

/// Encode `(card id, weight)` pairs for the `cards` column. Serialising a `Vec` of tuples
/// of integers cannot fail; an empty sheet encodes as `[]`.
pub fn encode_cards(cards: &[(i32, u32)]) -> String {
    serde_json::to_string(cards).unwrap_or_else(|_| "[]".to_string())
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cards_round_trip_through_the_column_in_order() {
        let cards = vec![(42, 1), (7, 12), (42, 3)];
        let encoded = encode_cards(&cards);
        assert_eq!(encoded, "[[42,1],[7,12],[42,3]]");
        assert_eq!(decode_cards(&encoded), cards);
        assert_eq!(encode_cards(&[]), "[]");
    }

    #[test]
    fn a_malformed_column_reads_as_no_cards() {
        assert!(decode_cards("{").is_empty());
        assert!(
            decode_cards("[[1]]").is_empty(),
            "a pair missing its weight is malformed"
        );
    }
}

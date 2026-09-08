//! Shared loading + wire shapes for the combo reads (issue #683): the deck page's
//! "Combos" (`handlers::decks::analysis::combos`) and the card page's "Combos with"
//! (`handlers::catalog::combos`) both hydrate the same rows into the same summary, so a
//! combo reads identically wherever it is shown.

use std::collections::HashMap;

use sea_orm::{
    ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QueryOrder, QuerySelect,
};
use serde::Serialize;

use crate::entities::card;
use crate::entities::prelude::{Card, Combo, ComboPiece};
use crate::entities::{combo, combo_piece};
use crate::spellbook::{self, model::ProducedFeature};

/// Ids per `IN (…)` lookup — the same bound every chunked catalog lookup uses.
pub(crate) const COMBO_CHUNK: usize = 900;

/// One combo as the wire describes it, without any deck-relative claim. Everything here is
/// Commander Spellbook's own datum; `url` is the page it came from — the link the source's
/// terms ask for.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct ComboSummary {
    /// Commander Spellbook's variant id (`"245-2034-6705"`).
    pub id: String,
    /// The combo's page on Commander Spellbook.
    pub url: String,
    /// Colour identity letters in WUBRG order; empty for colourless.
    pub identity: Vec<String>,
    /// Mana to start it, Scryfall-style (`"{6}"`); `null` when none is needed.
    pub mana_needed: Option<String>,
    pub mana_value_needed: Option<i32>,
    /// Prerequisites beyond the pieces themselves; `null` when none.
    pub prerequisites: Option<String>,
    /// The step-by-step description.
    pub description: String,
    /// Upstream's popularity counter — higher is more played.
    pub popularity: i32,
    /// Upstream's bracket tag (`"C"` casual, `"R"` ruthless, …); `null` when unset.
    pub bracket_tag: Option<String>,
    /// Wildcard requirements this app can't evaluate ("A free sacrifice outlet").
    pub templates: Vec<String>,
    /// What it produces — the results a player cares about, by name.
    pub produces: Vec<String>,
}

/// One card of a combo, as the card page shows it.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "ComboPiece"))]
pub struct ComboPieceResponse {
    /// The card's gameplay identity (Scryfall `oracle_id`).
    pub oracle_id: String,
    pub name: String,
    /// Copies the combo needs.
    pub quantity: i32,
    /// Whether it has to be in the command zone.
    pub must_be_commander: bool,
    /// A printing of it to link to — the newest the catalog holds; `null` when the catalog
    /// has none (a spoiler, a card set not imported yet).
    pub card_id: Option<String>,
}

/// A combo containing the viewed card, on the card page.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct CardCombo {
    #[serde(flatten)]
    #[cfg_attr(test, ts(flatten))]
    pub summary: ComboSummary,
    /// Every piece, in the combo's own order — the viewed card included.
    pub pieces: Vec<ComboPieceResponse>,
}

fn blank_to_none(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

impl ComboSummary {
    /// Dress a stored row. `produces` keeps the **standalone and contextual** results
    /// (upstream's `S` / `C`) — the effects a player reads a combo for — and falls back to
    /// every listed feature when a combo names only helpers, so nothing reads as producing
    /// nothing.
    pub(crate) fn from_model(m: &combo::Model) -> Self {
        let features: Vec<ProducedFeature> = serde_json::from_str(&m.produces).unwrap_or_default();
        let mut produces: Vec<String> = features
            .iter()
            .filter(|f| matches!(f.status.as_str(), "S" | "C"))
            .map(|f| f.name.clone())
            .collect();
        if produces.is_empty() {
            produces = features.into_iter().map(|f| f.name).collect();
        }
        Self {
            id: m.external_id.clone(),
            url: spellbook::combo_url(&m.external_id),
            identity: spellbook::model::split_list(&m.color_identity),
            mana_needed: blank_to_none(&m.mana_needed),
            mana_value_needed: m.mana_value_needed,
            prerequisites: blank_to_none(&m.prerequisites),
            description: m.description.clone(),
            popularity: m.popularity,
            bracket_tag: blank_to_none(&m.bracket_tag),
            templates: serde_json::from_str(&m.templates).unwrap_or_default(),
            produces,
        }
    }
}

/// Load combos by row id **in the order given**, each with its pieces in position order.
/// An id that no longer resolves (a rebuild between two reads) is skipped.
pub(crate) async fn load_combos(
    db: &DatabaseConnection,
    game: &str,
    ids: &[i32],
) -> Result<Vec<(combo::Model, Vec<combo_piece::Model>)>, DbErr> {
    let mut combos: HashMap<i32, combo::Model> = HashMap::new();
    let mut pieces: HashMap<i32, Vec<combo_piece::Model>> = HashMap::new();
    for chunk in ids.chunks(COMBO_CHUNK) {
        for row in Combo::find()
            .filter(combo::Column::Game.eq(game))
            .filter(combo::Column::Id.is_in(chunk.iter().copied()))
            .all(db)
            .await?
        {
            combos.insert(row.id, row);
        }
        for row in ComboPiece::find()
            .filter(combo_piece::Column::ComboId.is_in(chunk.iter().copied()))
            .order_by_asc(combo_piece::Column::Position)
            .order_by_asc(combo_piece::Column::Id)
            .all(db)
            .await?
        {
            pieces.entry(row.combo_id).or_default().push(row);
        }
    }
    Ok(ids
        .iter()
        .filter_map(|id| {
            combos
                .remove(id)
                .map(|c| (c, pieces.remove(id).unwrap_or_default()))
        })
        .collect())
}

/// One printing to link each gameplay identity to: the newest released printing the
/// listings would show (a folded foil-★ star never represents its base). Identities the
/// catalog doesn't hold are simply absent from the map.
pub(crate) async fn representative_printings(
    db: &DatabaseConnection,
    game: &str,
    oracle_ids: &[String],
) -> Result<HashMap<String, String>, DbErr> {
    let mut best: HashMap<String, (String, String)> = HashMap::new();
    for chunk in oracle_ids.chunks(COMBO_CHUNK) {
        let rows: Vec<(Option<String>, String, Option<String>)> = Card::find()
            .select_only()
            .column(card::Column::OracleId)
            .column(card::Column::ExternalId)
            .column(card::Column::ReleasedAt)
            .filter(card::Column::Game.eq(game))
            .filter(card::Column::OracleId.is_in(chunk.iter().map(String::as_str)))
            .filter(card::Column::FoldedOntoId.is_null())
            .into_tuple()
            .all(db)
            .await?;
        for (oracle_id, external_id, released_at) in rows {
            let Some(oracle_id) = oracle_id else { continue };
            let released = released_at.unwrap_or_default();
            // Newest first; on a tie the smallest external id, so the answer is stable.
            let better = match best.get(&oracle_id) {
                None => true,
                Some((held_released, held_id)) => {
                    released > *held_released
                        || (released == *held_released && external_id < *held_id)
                }
            };
            if better {
                best.insert(oracle_id, (released, external_id));
            }
        }
    }
    Ok(best
        .into_iter()
        .map(|(oracle, (_, external))| (oracle, external))
        .collect())
}

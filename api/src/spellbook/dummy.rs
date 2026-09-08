//! Offline dummy combos for `SEED_DUMMY_DATA`, over the dummy catalog's cards.
//!
//! The dummy catalog gives a deterministic handful of its base-set cards an `oracle_id`
//! (`scryfall::dummy::catalog`); these combos are built over those, so the seeded e2e
//! account can put a combo's pieces in a deck and see the panel light up, and the
//! security suite can pin the deck read's rules (complete / one short / must be the
//! commander / a template counts as a missing card) against real rows. Written through
//! the same [`replace_combos`] the sync uses, so the rows are shaped identically.

use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, QuerySelect};

use super::model::{ComboRecord, PieceRecord, ProducedFeature};
use super::{GAME, replace_combos};
use crate::entities::card;
use crate::entities::prelude::Card;
use crate::scryfall::ingest::IngestError;

/// The dummy card names the combos are built over, in the order the seed references them:
/// the base set's first cards carry `dummy-oracle-base-{n}` ids (see the catalog seed).
fn dummy_oracle(n: usize) -> String {
    format!("dummy-oracle-base-{n:04}")
}

fn produces(name: &str) -> ProducedFeature {
    ProducedFeature {
        name: name.to_string(),
        status: "S".to_string(),
    }
}

fn piece(oracle_id: &str, name: &str, must_be_commander: bool) -> PieceRecord {
    PieceRecord {
        oracle_id: oracle_id.to_string(),
        name: name.to_string(),
        quantity: 1,
        must_be_commander,
        zones: vec!["B".to_string()],
    }
}

/// Seed the dummy combos. Reads the seeded cards' names by oracle id so the pieces are
/// spelt exactly as the catalog spells them; a catalog without those cards seeds nothing.
pub async fn seed(db: &DatabaseConnection) -> Result<(), IngestError> {
    let wanted: Vec<String> = (1..=6).map(dummy_oracle).collect();
    let names: Vec<(String, String)> = Card::find()
        .select_only()
        .column(card::Column::OracleId)
        .column(card::Column::Name)
        .filter(card::Column::Game.eq(GAME))
        .filter(card::Column::OracleId.is_in(wanted.iter().map(String::as_str)))
        .order_by_asc(card::Column::OracleId)
        .distinct()
        .into_tuple::<(Option<String>, String)>()
        .all(db)
        .await?
        .into_iter()
        .filter_map(|(oracle, name)| oracle.map(|o| (o, name)))
        .collect();
    let name = |n: usize| -> Option<(String, String)> {
        let oracle = dummy_oracle(n);
        names
            .iter()
            .find(|(o, _)| *o == oracle)
            .map(|(_, name)| (oracle.clone(), name.clone()))
    };
    let (Some(c1), Some(c2), Some(c3), Some(c4), Some(c5), Some(c6)) =
        (name(1), name(2), name(3), name(4), name(5), name(6))
    else {
        tracing::warn!("dummy combo seed skipped: the dummy catalog's oracle ids are missing");
        return Ok(());
    };

    let records = vec![
        // A two-card combo: the panel's headline case.
        ComboRecord {
            id: "dummy-1-2".to_string(),
            identity: vec!["W".to_string(), "U".to_string()],
            mana_needed: "{2}".to_string(),
            mana_value_needed: Some(2),
            prerequisites: "Both permanents are on the battlefield.".to_string(),
            description: format!(
                "Tap {} to untap {}.\nTap {} to draw a card.\nRepeat.",
                c1.1, c2.1, c2.1
            ),
            notes: String::new(),
            popularity: 120,
            bracket_tag: "R".to_string(),
            templates: Vec::new(),
            produces: vec![produces("Infinite card draw"), produces("Infinite untap")],
            commander_legal: true,
            pieces: vec![piece(&c1.0, &c1.1, false), piece(&c2.0, &c2.1, false)],
        },
        // A three-card combo sharing two pieces with the first, so a deck holding the
        // first two is "one card short" of this one.
        ComboRecord {
            id: "dummy-1-2-3".to_string(),
            identity: vec!["W".to_string(), "U".to_string(), "B".to_string()],
            mana_needed: "{4}".to_string(),
            mana_value_needed: Some(4),
            prerequisites: String::new(),
            description: format!(
                "With {} and {} looping, {} deals 1 damage to each opponent per iteration.",
                c1.1, c2.1, c3.1
            ),
            notes: String::new(),
            popularity: 40,
            bracket_tag: "R".to_string(),
            templates: Vec::new(),
            produces: vec![produces("Infinite damage")],
            commander_legal: true,
            pieces: vec![
                piece(&c1.0, &c1.1, false),
                piece(&c2.0, &c2.1, false),
                piece(&c3.0, &c3.1, false),
            ],
        },
        // A commander-led combo: the first piece has to be in the command zone.
        ComboRecord {
            id: "dummy-4-5".to_string(),
            identity: vec!["R".to_string()],
            mana_needed: "{3}".to_string(),
            mana_value_needed: Some(3),
            prerequisites: format!("{} is your commander.", c4.1),
            description: format!("Cast {} from the command zone; {} copies it.", c4.1, c5.1),
            notes: String::new(),
            popularity: 15,
            bracket_tag: "C".to_string(),
            templates: Vec::new(),
            produces: vec![produces("Infinite ETB")],
            commander_legal: true,
            pieces: vec![piece(&c4.0, &c4.1, true), piece(&c5.0, &c5.1, false)],
        },
        // A combo needing a template — a wildcard this app can't evaluate, so it is always
        // at least one card short.
        ComboRecord {
            id: "dummy-6-t".to_string(),
            identity: vec!["G".to_string()],
            mana_needed: String::new(),
            mana_value_needed: None,
            prerequisites: String::new(),
            description: format!("Sacrifice {} to any free outlet; it returns.", c6.1),
            notes: String::new(),
            popularity: 3,
            bracket_tag: "C".to_string(),
            templates: vec!["A free sacrifice outlet".to_string()],
            produces: vec![produces("Infinite death triggers")],
            commander_legal: true,
            pieces: vec![piece(&c6.0, &c6.1, false)],
        },
    ];
    let pieces = replace_combos(db, records).await?;
    tracing::info!(combos = 4, pieces, "seeded dummy combos");
    Ok(())
}

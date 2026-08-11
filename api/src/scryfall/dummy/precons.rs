//! Fabricated preconstructed decks for the offline dummy catalog: a Commander deck, a small
//! all-foil deck with no command zone, a starter deck with a sideboard and two Jumpstart
//! themes, over the seeded dummy cards — so
//! the precon browser (list, facets, detail, copy) has data to serve with no network, and
//! the e2e suite has something to click.
//!
//! Pure data plus one write: the decks are declared here by *card external id* and resolved
//! against the just-seeded catalog, exactly as the sealed-contents seed does. The derived
//! columns (`card_count`, `color_identity`, `face_card_id`) are computed the same way the
//! real ingest computes them, so an offline row is shaped like a synced one.
//!
//! The three cover the shapes the UI has to handle: a deck with a command zone (colours read
//! off the commander), one without (colours read off the deck), and one with a sideboard.

use std::collections::HashMap;

use chrono::Utc;
use sea_orm::{
    ActiveValue::{NotSet, Set},
    ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QuerySelect,
};

use super::super::GAME;
use super::super::ingest::IngestError;
use crate::entities::precon_deck_card::PreconBoard;
use crate::entities::prelude::{Card, PreconDeck, PreconDeckCard, Product};
use crate::entities::{card, precon_deck, precon_deck_card, product};
use crate::handlers::decks::COLOUR_ORDER;
use crate::handlers::shared::dto::split_csv;

/// One fabricated precon: its metadata plus `(board, card external id, copies, foil)` rows.
struct SeedPrecon {
    slug: &'static str,
    name: &'static str,
    set_code: &'static str,
    deck_type: &'static str,
    released_at: &'static str,
    /// The dummy sealed product that ships it, when there is one.
    product_external_id: Option<&'static str>,
    cards: Vec<(PreconBoard, String, i32, bool)>,
}

/// The fabricated precons — the single source of truth for the offline browser.
fn dummy_precons() -> Vec<SeedPrecon> {
    let card = |set: &str, n: i32| format!("dummy-{set}-{n:04}");

    // A Commander deck: one commander (the colours the tile shows come from it alone) plus
    // a deck, shipped in the dummy commander-deck product (900004).
    let mut commander_cards = vec![(PreconBoard::Commander, card("dmu", 1), 1, true)];
    for n in 2..=12 {
        commander_cards.push((PreconBoard::Main, card("dmu", n), 1, false));
    }
    // A pile of basics, so a copied deck has a realistic Lands section — plus a foil copy of
    // that same printing. A board listing one printing in both finishes is two rows by design
    // (the ingest keys on `(card, finish)`), and it's the shape every Jumpstart theme and
    // bundle land pack has, so the offline catalog carries it: a copy must fold the pair into
    // one deck card rather than trip `deck_cards`' unique key.
    commander_cards.push((PreconBoard::Main, card("dmb", 1), 20, false));
    commander_cards.push((PreconBoard::Main, card("dmb", 1), 1, true));

    // A small all-foil deck with no command zone, in its own order. It sits in `sld` because
    // that set really does still publish precons (7 Commander decks and a Dandan deck) once the
    // Secret Lair *drops* are excluded — and its type must stay outside `NOT_A_DECK_TYPES`, or
    // the offline catalog would seed rows the real derivation refuses to create.
    let dandan_cards = (1..=4)
        .map(|n| (PreconBoard::Main, card("sld", n), 1, true))
        .collect();

    // A starter deck with a sideboard, so the detail page's sideboard split has data.
    let mut starter_cards: Vec<(PreconBoard, String, i32, bool)> = (2..=9)
        .map(|n| (PreconBoard::Main, card("dmb", n), 2, false))
        .collect();
    starter_cards.push((PreconBoard::Side, card("dmb", 10), 3, false));

    // Two Jumpstart-style themes in the base set, so one set carries several decks across
    // several types — the shape that makes the by-type grouping worth having (Marvel ships 51
    // Jumpstart themes beside 12 Box Sets; 136 of 295 real sets span more than one type). With
    // one deck per set the grouped views would render but demonstrate nothing.
    let theme = |first: i32| -> Vec<(PreconBoard, String, i32, bool)> {
        (first..first + 4)
            .map(|n| (PreconBoard::Main, card("dmb", n), 2, false))
            .collect()
    };

    vec![
        SeedPrecon {
            slug: "dummy-universe-commander-dmu",
            name: "Dummy Universe Commander",
            set_code: "dmu",
            deck_type: "Commander Deck",
            released_at: "2024-06-20",
            product_external_id: Some("900004"),
            cards: commander_cards,
        },
        SeedPrecon {
            slug: "dummy-dandan-deck-sld",
            name: "Dummy Dandan Deck",
            set_code: "sld",
            deck_type: "Dandan Deck",
            released_at: "2019-12-02",
            product_external_id: Some("900006"),
            cards: dandan_cards,
        },
        SeedPrecon {
            slug: "dummy-base-set-starter-dmb",
            name: "Dummy Base Set Starter",
            set_code: "dmb",
            deck_type: "Starter Deck",
            released_at: "2024-01-15",
            product_external_id: None,
            cards: starter_cards,
        },
        SeedPrecon {
            slug: "dummy-base-set-jumpstart-ember-dmb",
            name: "Dummy Base Set Jumpstart: Ember",
            set_code: "dmb",
            deck_type: "Jumpstart",
            released_at: "2024-01-15",
            product_external_id: None,
            cards: theme(2),
        },
        SeedPrecon {
            slug: "dummy-base-set-jumpstart-tide-dmb",
            name: "Dummy Base Set Jumpstart: Tide",
            set_code: "dmb",
            deck_type: "Jumpstart",
            released_at: "2024-01-15",
            product_external_id: None,
            cards: theme(6),
        },
    ]
}

/// Seed the dummy precon decks, returning how many card rows were written. Runs after the
/// cards + products are seeded (it resolves both), and replaces the game's rows wholesale so
/// a reseed is idempotent — matching the real ingest's semantics.
pub(super) async fn seed_precons(db: &DatabaseConnection) -> Result<u64, IngestError> {
    let precons = dummy_precons();

    let card_exts: Vec<String> = precons
        .iter()
        .flat_map(|p| p.cards.iter().map(|(_, ext, ..)| ext.clone()))
        .collect();
    let cards: HashMap<String, (i32, Option<String>)> = Card::find()
        .select_only()
        .column(card::Column::ExternalId)
        .column(card::Column::Id)
        .column(card::Column::ColorIdentity)
        .filter(card::Column::Game.eq(GAME))
        .filter(card::Column::ExternalId.is_in(card_exts))
        .into_tuple::<(String, i32, Option<String>)>()
        .all(db)
        .await?
        .into_iter()
        .map(|(ext, id, identity)| (ext, (id, identity)))
        .collect();
    let product_exts: Vec<String> = precons
        .iter()
        .filter_map(|p| p.product_external_id.map(str::to_string))
        .collect();
    let products: HashMap<String, i32> = Product::find()
        .select_only()
        .column(product::Column::ExternalId)
        .column(product::Column::Id)
        .filter(product::Column::Game.eq(GAME))
        .filter(product::Column::ExternalId.is_in(product_exts))
        .into_tuple::<(String, i32)>()
        .all(db)
        .await?
        .into_iter()
        .collect();

    // Wholesale rebuild, children first (SQLite doesn't enforce the cascade by default —
    // the same reason the real ingest deletes explicitly).
    let existing: Vec<i32> = PreconDeck::find()
        .select_only()
        .column(precon_deck::Column::Id)
        .filter(precon_deck::Column::Game.eq(GAME))
        .into_tuple()
        .all(db)
        .await?;
    if !existing.is_empty() {
        PreconDeckCard::delete_many()
            .filter(precon_deck_card::Column::PreconDeckId.is_in(existing))
            .exec(db)
            .await?;
    }
    PreconDeck::delete_many()
        .filter(precon_deck::Column::Game.eq(GAME))
        .exec(db)
        .await?;

    let now = Utc::now();
    let mut written = 0u64;
    for precon in precons {
        // Resolve the deck's cards, dropping any the catalog doesn't hold.
        let resolved: Vec<(PreconBoard, i32, Option<String>, i32, bool)> = precon
            .cards
            .iter()
            .filter_map(|(board, ext, quantity, foil)| {
                let (id, identity) = cards.get(ext)?;
                Some((*board, *id, identity.clone(), *quantity, *foil))
            })
            .collect();
        if resolved.is_empty() {
            continue;
        }

        // Derived columns, folded exactly as `mtgjson::ingest::precons` folds them: counts
        // over the resolved rows, colours off the command zone when there is one, and the
        // face card is the first commander else the first non-sideboard card.
        let card_count = resolved
            .iter()
            .filter(|(board, ..)| *board != PreconBoard::Side)
            .map(|(_, _, _, quantity, _)| quantity)
            .sum();
        let sideboard_count = resolved
            .iter()
            .filter(|(board, ..)| *board == PreconBoard::Side)
            .map(|(_, _, _, quantity, _)| quantity)
            .sum();
        let has_zone = resolved
            .iter()
            .any(|(board, ..)| *board == PreconBoard::Commander);
        let colour_source = if has_zone {
            PreconBoard::Commander
        } else {
            PreconBoard::Main
        };
        let letters: Vec<String> = resolved
            .iter()
            .filter(|(board, ..)| *board == colour_source)
            .flat_map(|(_, _, identity, _, _)| split_csv(identity.clone()))
            .collect();
        let color_identity = COLOUR_ORDER
            .iter()
            .filter(|colour| letters.iter().any(|held| held == *colour))
            .copied()
            .collect::<Vec<&str>>()
            .join("");
        let face_card_id = resolved
            .iter()
            .find(|(board, ..)| *board == PreconBoard::Commander)
            .or_else(|| {
                resolved
                    .iter()
                    .find(|(board, ..)| *board != PreconBoard::Side)
            })
            .map(|(_, id, ..)| *id);

        let deck = precon_deck::ActiveModel {
            id: NotSet,
            game: Set(GAME.to_string()),
            slug: Set(precon.slug.to_string()),
            name: Set(precon.name.to_string()),
            set_code: Set(precon.set_code.to_string()),
            deck_type: Set(precon.deck_type.to_string()),
            released_at: Set(Some(precon.released_at.to_string())),
            color_identity: Set(Some(color_identity)),
            card_count: Set(card_count),
            sideboard_count: Set(sideboard_count),
            face_card_id: Set(face_card_id),
            product_id: Set(precon
                .product_external_id
                .and_then(|ext| products.get(ext).copied())),
            created_at: Set(now),
            updated_at: Set(now),
        };
        let deck_id = PreconDeck::insert(deck).exec(db).await?.last_insert_id;

        // Positions run per board, as the real builder assigns them.
        let mut position_by_board: HashMap<&'static str, i32> = HashMap::new();
        let rows: Vec<precon_deck_card::ActiveModel> = resolved
            .iter()
            .map(|(board, card_id, _, quantity, foil)| {
                let position = position_by_board.entry(board.as_str()).or_insert(0);
                let model = precon_deck_card::ActiveModel {
                    id: NotSet,
                    precon_deck_id: Set(deck_id),
                    card_id: Set(*card_id),
                    board: Set(board.as_str().to_string()),
                    quantity: Set(*quantity),
                    foil: Set(*foil),
                    position: Set(*position),
                };
                *position += 1;
                model
            })
            .collect();
        written += rows.len() as u64;
        PreconDeckCard::insert_many(rows)
            .exec_without_returning(db)
            .await?;
    }

    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::migrated_memory_db;

    /// The offline catalog must not seed a shape the real derivation refuses to produce.
    ///
    /// `mtgjson::precons` drops `NOT_A_DECK_TYPES` at derivation, so a dummy precon in one of
    /// those categories would put a row in dev and e2e that no synced instance can ever hold —
    /// and the dummy seed is exactly where such a contradiction hides, because it bypasses the
    /// derivation entirely. Reads the real list rather than restating it, so adding a category
    /// there fails here instead of silently diverging.
    #[test]
    fn no_seeded_precon_uses_an_excluded_deck_type() {
        for precon in dummy_precons() {
            assert!(
                !crate::mtgjson::precons::NOT_A_DECK_TYPES.contains(&precon.deck_type),
                "{} seeds `{}`, which the derivation excludes",
                precon.slug,
                precon.deck_type
            );
        }
    }

    /// The declared decks are internally sound: unique slugs, a non-empty card list each,
    /// and the three shapes the browser has to render (command zone / none / sideboard).
    #[test]
    fn declared_precons_are_distinct_and_cover_the_shapes() {
        let precons = dummy_precons();
        let slugs: std::collections::HashSet<&str> = precons.iter().map(|p| p.slug).collect();
        assert_eq!(slugs.len(), precons.len(), "slugs must be unique");
        assert!(precons.iter().all(|p| !p.cards.is_empty()));
        assert!(
            precons
                .iter()
                .any(|p| p.cards.iter().any(|(b, ..)| *b == PreconBoard::Commander))
        );
        assert!(
            precons
                .iter()
                .any(|p| p.cards.iter().all(|(b, ..)| *b != PreconBoard::Commander))
        );
        assert!(
            precons
                .iter()
                .any(|p| p.cards.iter().any(|(b, ..)| *b == PreconBoard::Side))
        );
        // One set with several decks across several types — what the grouped views are for.
        let dmb: Vec<&SeedPrecon> = precons.iter().filter(|p| p.set_code == "dmb").collect();
        assert!(dmb.len() >= 3, "the base set carries several decks");
        assert!(
            dmb.iter()
                .map(|p| p.deck_type)
                .collect::<std::collections::HashSet<_>>()
                .len()
                >= 2,
            "…across more than one deck type"
        );
    }

    /// Seeding twice leaves one copy of each deck — the seed is upsert-safe like the rest of
    /// the dummy catalog, and a dev who reruns it doesn't end up with duplicates.
    #[tokio::test]
    async fn seeding_twice_is_idempotent() {
        let db = migrated_memory_db().await;
        super::super::seed(&db).await.expect("seed dummy catalog");
        let first = PreconDeck::find().all(&db).await.unwrap().len();
        assert!(first > 0, "the dummy catalog seeds precons");
        super::super::seed(&db).await.expect("reseed dummy catalog");
        assert_eq!(PreconDeck::find().all(&db).await.unwrap().len(), first);
    }

    /// The seeded Commander deck reads its colours off its commander and fronts the tile
    /// with it — the two derivations the real ingest performs.
    #[tokio::test]
    async fn seeded_commander_deck_derives_its_facets() {
        let db = migrated_memory_db().await;
        super::super::seed(&db).await.expect("seed dummy catalog");
        let deck = PreconDeck::find()
            .filter(precon_deck::Column::Slug.eq("dummy-universe-commander-dmu"))
            .one(&db)
            .await
            .unwrap()
            .expect("the commander precon is seeded");
        assert!(deck.card_count > 0);
        assert_eq!(deck.sideboard_count, 0);
        assert!(deck.face_card_id.is_some(), "its commander fronts the tile");
        assert!(deck.product_id.is_some(), "it links its sealed product");
        let commander = PreconDeckCard::find()
            .filter(precon_deck_card::Column::PreconDeckId.eq(deck.id))
            .filter(precon_deck_card::Column::Board.eq(PreconBoard::Commander.as_str()))
            .all(&db)
            .await
            .unwrap();
        assert_eq!(commander.len(), 1);
        assert_eq!(deck.face_card_id, Some(commander[0].card_id));
    }
}

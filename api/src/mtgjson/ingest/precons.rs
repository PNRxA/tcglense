//! Rebuild of the `precon_decks` / `precon_deck_cards` tables from the precons the pure
//! pass resolved ([`crate::mtgjson::precons`]).
//!
//! Same shape as the membership + composition rebuilds it runs beside: map external ids
//! (Scryfall id -> `cards.id`, TCGplayer product id -> `products.id`) onto our catalog, then
//! **replace** the game's rows inside the caller's transaction so a reader never sees a
//! half-rebuilt browser and a deck upstream dropped can't linger.
//!
//! Three things are decided here rather than at read time, because a precon is immutable
//! between syncs and the browse list is a public, CDN-cacheable read that must not pay a
//! per-row card scan:
//!
//! * `card_count` / `sideboard_count` — copies, counted over the cards that **resolved**
//!   against our catalog (like every other card count in the app, which inner-joins
//!   `cards`), so the number always matches the list the detail page can actually show.
//! * `color_identity` — the command zone's when the deck has one, the union over its
//!   mainboard otherwise. That is the *same rule* the deck list's derived facets use
//!   ([`crate::handlers::decks::facets`]): a Commander precon is Mardu because its
//!   commander is, and a sideboard never colours a deck.
//! * `face_card_id` — the card that fronts the tile: the first commander, else the first
//!   card upstream lists (which for a Secret Lair drop is the drop's own leading card).
//!
//! Child rows are deleted **explicitly** rather than through the table's `ON DELETE
//! CASCADE`: SQLite (the default backend) doesn't enforce foreign keys unless
//! `PRAGMA foreign_keys` is on, so relying on the cascade would silently orphan every
//! precon card on a self-host while working fine on Postgres.

use std::collections::{HashMap, HashSet};

use sea_orm::{
    ActiveValue::{NotSet, Set},
    ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QuerySelect, QueryTrait,
    prelude::DateTimeUtc,
};

use super::super::precons::RawPrecon;
use super::super::{GAME, MtgjsonError};
use super::{IN_CHUNK, INSERT_BATCH};
use crate::entities::precon_deck_card::PreconBoard;
use crate::entities::prelude::{Card, PreconDeck, PreconDeckCard};
use crate::entities::{card, precon_deck, precon_deck_card};
use crate::handlers::decks::COLOUR_ORDER;
use crate::handlers::shared::dto::split_csv;

/// What a rebuild wrote, for the sync's `ingest_state` detail line.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PreconStats {
    pub decks: usize,
    pub cards: usize,
}

/// Replace the game's precon decks + cards inside `txn`.
///
/// Cards that don't resolve against our catalog are skipped (and their deck's counts reflect
/// that); a deck left with **no** resolved card is not written at all, since a precon with an
/// empty decklist is worse than no entry.
pub(crate) async fn rebuild<C: ConnectionTrait>(
    txn: &C,
    precons: &[RawPrecon],
    card_ids: &HashMap<String, i32>,
    product_ids: &HashMap<String, i32>,
    now: DateTimeUtc,
) -> Result<PreconStats, MtgjsonError> {
    // Resolve each precon's cards onto internal ids first: the colour fold below needs the
    // resolved ids, and the deck row needs the counts derived from them.
    let resolved: Vec<Resolved> = precons
        .iter()
        .filter_map(|precon| Resolved::build(precon, card_ids, product_ids))
        .collect();

    let colours = colour_identities(txn, &resolved).await?;

    // 1. Clear the game's rows, children first (see the module note on cascades).
    PreconDeckCard::delete_many()
        .filter(precon_deck_card::Column::PreconDeckId.in_subquery(game_precon_ids()))
        .exec(txn)
        .await?;
    PreconDeck::delete_many()
        .filter(precon_deck::Column::Game.eq(GAME))
        .exec(txn)
        .await?;

    // 2. Insert the deck rows in batches, then read back the ids their slugs took. Doing it
    // this way (rather than one insert per deck, keeping the returned model) is what keeps
    // ~3 000 decks to a handful of round trips instead of 3 000 — the difference between a
    // fast rebuild and a slow one on a networked Postgres.
    let deck_models: Vec<precon_deck::ActiveModel> = resolved
        .iter()
        .map(|r| r.to_active_model(colours.get(r.precon.slug.as_str()).cloned().flatten(), now))
        .collect();
    for chunk in deck_models.chunks(INSERT_BATCH) {
        PreconDeck::insert_many(chunk.iter().cloned())
            .exec_without_returning(txn)
            .await?;
    }
    let ids_by_slug: HashMap<String, i32> = PreconDeck::find()
        .select_only()
        .column(precon_deck::Column::Slug)
        .column(precon_deck::Column::Id)
        .filter(precon_deck::Column::Game.eq(GAME))
        .into_tuple::<(String, i32)>()
        .all(txn)
        .await?
        .into_iter()
        .collect();

    // 3. Insert the cards against those ids.
    let card_models: Vec<precon_deck_card::ActiveModel> = resolved
        .iter()
        .filter_map(|r| {
            let deck_id = ids_by_slug.get(r.precon.slug.as_str()).copied()?;
            Some(r.cards.iter().map(move |c| precon_deck_card::ActiveModel {
                id: NotSet,
                precon_deck_id: Set(deck_id),
                card_id: Set(c.card_id),
                board: Set(c.board.to_string()),
                quantity: Set(c.quantity),
                foil: Set(c.foil),
                position: Set(c.position),
            }))
        })
        .flatten()
        .collect();
    let cards_written = card_models.len();
    for chunk in card_models.chunks(INSERT_BATCH) {
        PreconDeckCard::insert_many(chunk.iter().cloned())
            .exec_without_returning(txn)
            .await?;
    }

    Ok(PreconStats {
        decks: resolved.len(),
        cards: cards_written,
    })
}

/// Sub-select of this game's `precon_decks.id` — what the child delete filters against, so
/// "the game's precon cards" is expressed once, through the query API (parameterised and
/// dialect-neutral) like every other query here.
fn game_precon_ids() -> sea_orm::sea_query::SelectStatement {
    PreconDeck::find()
        .select_only()
        .column(precon_deck::Column::Id)
        .filter(precon_deck::Column::Game.eq(GAME))
        .into_query()
}

/// One precon whose cards resolved onto internal catalog ids.
struct Resolved<'a> {
    precon: &'a RawPrecon,
    cards: Vec<ResolvedCard>,
    card_count: i32,
    sideboard_count: i32,
    face_card_id: Option<i32>,
    product_id: Option<i32>,
}

struct ResolvedCard {
    card_id: i32,
    board: &'static str,
    quantity: i32,
    foil: bool,
    position: i32,
}

impl<'a> Resolved<'a> {
    /// Resolve one precon, or `None` when none of its cards are in our catalog.
    fn build(
        precon: &'a RawPrecon,
        card_ids: &HashMap<String, i32>,
        product_ids: &HashMap<String, i32>,
    ) -> Option<Self> {
        let cards: Vec<ResolvedCard> = precon
            .cards
            .iter()
            .filter_map(|c| {
                Some(ResolvedCard {
                    card_id: card_ids.get(&c.scryfall_id).copied()?,
                    board: c.board,
                    quantity: c.quantity,
                    foil: c.foil,
                    position: c.position,
                })
            })
            .collect();
        if cards.is_empty() {
            return None;
        }

        let side = PreconBoard::Side.as_str();
        let commander = PreconBoard::Commander.as_str();
        let card_count = cards
            .iter()
            .filter(|c| c.board != side)
            .map(|c| c.quantity)
            .sum();
        let sideboard_count = cards
            .iter()
            .filter(|c| c.board == side)
            .map(|c| c.quantity)
            .sum();
        // The face card: the first commander, else the first card upstream listed. `cards`
        // is already in board order (commander, main, side) with upstream's order inside
        // each board, so this is just "the first one that isn't a sideboard card".
        let face_card_id = cards
            .iter()
            .find(|c| c.board == commander)
            .or_else(|| cards.iter().find(|c| c.board != side))
            .map(|c| c.card_id);
        // The deck's own product link: the first sealed product that resolved.
        let product_id = precon
            .product_ids
            .iter()
            .find_map(|id| product_ids.get(id).copied());

        Some(Resolved {
            precon,
            cards,
            card_count,
            sideboard_count,
            face_card_id,
            product_id,
        })
    }

    /// The cards whose colours describe the deck: its command zone when it has one, its
    /// mainboard otherwise — never its sideboard.
    fn colour_source(&self) -> impl Iterator<Item = i32> + '_ {
        let commander = PreconBoard::Commander.as_str();
        let main = PreconBoard::Main.as_str();
        let has_zone = self.cards.iter().any(|c| c.board == commander);
        let wanted = if has_zone { commander } else { main };
        self.cards
            .iter()
            .filter(move |c| c.board == wanted)
            .map(|c| c.card_id)
    }

    fn to_active_model(
        &self,
        color_identity: Option<String>,
        now: DateTimeUtc,
    ) -> precon_deck::ActiveModel {
        precon_deck::ActiveModel {
            id: NotSet,
            game: Set(GAME.to_string()),
            slug: Set(self.precon.slug.clone()),
            name: Set(self.precon.name.clone()),
            set_code: Set(self.precon.set_code.clone()),
            deck_type: Set(self.precon.deck_type.clone()),
            released_at: Set(self.precon.released_at.clone()),
            color_identity: Set(color_identity),
            card_count: Set(self.card_count),
            sideboard_count: Set(self.sideboard_count),
            face_card_id: Set(self.face_card_id),
            product_id: Set(self.product_id),
            // Derived by `catalog::precon_values::refresh_precon_values` (each sync tick,
            // right after this rebuild), never here: card prices move on ticks this
            // ETag-gated rebuild doesn't run on, so a value folded now would go stale.
            price_cents: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        }
    }
}

/// Fold each precon's colour identity, keyed by slug: one chunked read of every relevant
/// card's `color_identity`, then a per-deck union in memory.
///
/// `None` for a deck whose colour source resolved to no card at all (a sideboard-only
/// precon) — the same "nothing to read a colour off" the deck list's `null` means, as
/// distinct from `Some("")`, a deck that genuinely plays no colour.
async fn colour_identities<C: ConnectionTrait>(
    txn: &C,
    resolved: &[Resolved<'_>],
) -> Result<HashMap<String, Option<String>>, MtgjsonError> {
    let wanted: Vec<i32> = resolved
        .iter()
        .flat_map(|r| r.colour_source())
        .collect::<HashSet<i32>>()
        .into_iter()
        .collect();
    let mut identity_by_card: HashMap<i32, Vec<String>> = HashMap::new();
    for chunk in wanted.chunks(IN_CHUNK) {
        let rows: Vec<(i32, Option<String>)> = Card::find()
            .select_only()
            .column(card::Column::Id)
            .column(card::Column::ColorIdentity)
            .filter(card::Column::Id.is_in(chunk.iter().copied()))
            .into_tuple()
            .all(txn)
            .await?;
        for (id, identity) in rows {
            identity_by_card.insert(id, split_csv(identity));
        }
    }

    Ok(resolved
        .iter()
        .map(|r| {
            let mut found = false;
            let mut letters: HashSet<&str> = HashSet::new();
            for card_id in r.colour_source() {
                let Some(identity) = identity_by_card.get(&card_id) else {
                    continue;
                };
                found = true;
                for letter in identity {
                    if let Some(known) = COLOUR_ORDER.iter().find(|c| **c == letter.as_str()) {
                        letters.insert(known);
                    }
                }
            }
            let value = found.then(|| {
                COLOUR_ORDER
                    .iter()
                    .filter(|colour| letters.contains(*colour))
                    .copied()
                    .collect::<Vec<&str>>()
                    .join("")
            });
            (r.precon.slug.clone(), value)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::prelude::Product;
    use crate::mtgjson::precons::{RawPrecon, RawPreconCard};
    use crate::test_support::migrated_memory_db;
    use chrono::Utc;
    use sea_orm::{ActiveModelTrait, DatabaseConnection, QueryOrder};

    /// Insert a card with a colour identity and return its internal id.
    async fn insert_card(db: &DatabaseConnection, ext: &str, identity: Option<&str>) -> i32 {
        let now = Utc::now();
        crate::entities::card::ActiveModel {
            game: Set(GAME.to_string()),
            external_id: Set(ext.to_string()),
            name: Set(format!("Card {ext}")),
            set_code: Set("tmc".to_string()),
            set_name: Set("TMC".to_string()),
            collector_number: Set("1".to_string()),
            lang: Set("en".to_string()),
            digital: Set(false),
            color_identity: Set(identity.map(str::to_string)),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    async fn insert_product(db: &DatabaseConnection, ext: &str) -> i32 {
        let now = Utc::now();
        crate::entities::product::ActiveModel {
            game: Set(GAME.to_string()),
            external_id: Set(ext.to_string()),
            name: Set(format!("Product {ext}")),
            clean_name: Set(None),
            set_code: Set("tmc".to_string()),
            product_type: Set("commander_deck".to_string()),
            url: Set(None),
            image_url: Set(None),
            price_usd: Set(None),
            price_usd_foil: Set(None),
            released_at: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    fn raw_card(scryfall: &str, board: PreconBoard, quantity: i32, position: i32) -> RawPreconCard {
        RawPreconCard {
            scryfall_id: scryfall.to_string(),
            board: board.as_str(),
            quantity,
            foil: false,
            position,
        }
    }

    fn precon(slug: &str, cards: Vec<RawPreconCard>, product: Option<&str>) -> RawPrecon {
        RawPrecon {
            slug: slug.to_string(),
            name: slug.to_string(),
            set_code: "tmc".to_string(),
            deck_type: "Commander Deck".to_string(),
            released_at: Some("2026-03-06".to_string()),
            product_ids: product.into_iter().map(str::to_string).collect(),
            cards,
        }
    }

    async fn ids(db: &DatabaseConnection) -> (HashMap<String, i32>, HashMap<String, i32>) {
        use crate::entities::prelude::Card as CardEntity;
        let cards = CardEntity::find()
            .all(db)
            .await
            .unwrap()
            .into_iter()
            .map(|c| (c.external_id, c.id))
            .collect();
        let products = Product::find()
            .all(db)
            .await
            .unwrap()
            .into_iter()
            .map(|p| (p.external_id, p.id))
            .collect();
        (cards, products)
    }

    /// The write path end to end: rows land, counts exclude the sideboard, the colours come
    /// off the **command zone** (not the mainboard), the commander fronts the deck, and the
    /// sealed product links.
    #[tokio::test]
    async fn rebuild_writes_rows_and_derives_facets() {
        let db = migrated_memory_db().await;
        let commander = insert_card(&db, "sf-cmd", Some("W,U")).await;
        insert_card(&db, "sf-main", Some("B")).await;
        insert_card(&db, "sf-side", Some("R")).await;
        insert_product(&db, "657865").await;
        let (cards, products) = ids(&db).await;

        let precons = vec![precon(
            "turtle-power-tmc",
            vec![
                raw_card("sf-cmd", PreconBoard::Commander, 1, 0),
                raw_card("sf-main", PreconBoard::Main, 20, 0),
                raw_card("sf-side", PreconBoard::Side, 3, 0),
            ],
            Some("657865"),
        )];
        let stats = rebuild(&db, &precons, &cards, &products, Utc::now())
            .await
            .unwrap();
        assert_eq!((stats.decks, stats.cards), (1, 3));

        let deck = PreconDeck::find().one(&db).await.unwrap().expect("a deck");
        assert_eq!(deck.card_count, 21, "the sideboard is counted apart");
        assert_eq!(deck.sideboard_count, 3);
        assert_eq!(
            deck.color_identity.as_deref(),
            Some("WU"),
            "a deck with a command zone is its commander's colours, not its deck's"
        );
        assert_eq!(deck.face_card_id, Some(commander));
        assert!(deck.product_id.is_some());
    }

    /// With no command zone, the colours are the union over the **mainboard** — and the
    /// sideboard never colours a deck.
    #[tokio::test]
    async fn a_deck_without_a_command_zone_takes_its_mainboards_colours() {
        let db = migrated_memory_db().await;
        insert_card(&db, "sf-a", Some("G")).await;
        insert_card(&db, "sf-b", Some("U")).await;
        insert_card(&db, "sf-side", Some("R")).await;
        let (cards, products) = ids(&db).await;

        let precons = vec![precon(
            "a-drop-sld",
            vec![
                raw_card("sf-a", PreconBoard::Main, 1, 0),
                raw_card("sf-b", PreconBoard::Main, 1, 1),
                raw_card("sf-side", PreconBoard::Side, 1, 0),
            ],
            None,
        )];
        rebuild(&db, &precons, &cards, &products, Utc::now())
            .await
            .unwrap();

        let deck = PreconDeck::find().one(&db).await.unwrap().expect("a deck");
        assert_eq!(deck.color_identity.as_deref(), Some("UG"));
        assert_eq!(deck.product_id, None);
    }

    /// A colourless deck answers `Some("")`; a deck whose colour source resolved to nothing
    /// answers `None`. The two are different claims, and the wire keeps them apart.
    #[tokio::test]
    async fn colourless_and_unknown_are_distinct_answers() {
        let db = migrated_memory_db().await;
        insert_card(&db, "sf-colourless", Some("")).await;
        insert_card(&db, "sf-side", Some("R")).await;
        let (cards, products) = ids(&db).await;

        let precons = vec![
            precon(
                "colourless-tmc",
                vec![raw_card("sf-colourless", PreconBoard::Main, 1, 0)],
                None,
            ),
            // Only a sideboard resolved: there is nothing to read a colour off.
            precon(
                "sideboard-only-tmc",
                vec![raw_card("sf-side", PreconBoard::Side, 1, 0)],
                None,
            ),
        ];
        rebuild(&db, &precons, &cards, &products, Utc::now())
            .await
            .unwrap();

        let decks = PreconDeck::find()
            .order_by_asc(precon_deck::Column::Slug)
            .all(&db)
            .await
            .unwrap();
        assert_eq!(decks[0].color_identity.as_deref(), Some(""));
        assert_eq!(decks[1].color_identity, None);
    }

    /// Cards the catalog doesn't hold are skipped (and don't count); a deck with **no**
    /// resolvable card is not written at all.
    #[tokio::test]
    async fn unresolvable_cards_and_decks_are_skipped() {
        let db = migrated_memory_db().await;
        insert_card(&db, "sf-known", Some("W")).await;
        let (cards, products) = ids(&db).await;

        let precons = vec![
            precon(
                "partial-tmc",
                vec![
                    raw_card("sf-known", PreconBoard::Main, 1, 0),
                    raw_card("sf-unknown", PreconBoard::Main, 4, 1),
                ],
                None,
            ),
            precon(
                "ghost-tmc",
                vec![raw_card("sf-nope", PreconBoard::Main, 1, 0)],
                None,
            ),
        ];
        let stats = rebuild(&db, &precons, &cards, &products, Utc::now())
            .await
            .unwrap();
        assert_eq!((stats.decks, stats.cards), (1, 1));
        let deck = PreconDeck::find().one(&db).await.unwrap().expect("a deck");
        assert_eq!(deck.slug, "partial-tmc");
        assert_eq!(
            deck.card_count, 1,
            "the count reflects the cards the page can actually show"
        );
    }

    /// A rebuild **replaces**: a deck upstream dropped leaves with its cards, rather than
    /// lingering as an orphan (which the table's `ON DELETE CASCADE` would not achieve on
    /// SQLite, where foreign keys aren't enforced by default).
    #[tokio::test]
    async fn rebuild_replaces_the_previous_run() {
        let db = migrated_memory_db().await;
        insert_card(&db, "sf-a", Some("W")).await;
        insert_card(&db, "sf-b", Some("U")).await;
        let (cards, products) = ids(&db).await;

        let first = vec![
            precon(
                "keeper-tmc",
                vec![raw_card("sf-a", PreconBoard::Main, 1, 0)],
                None,
            ),
            precon(
                "goner-tmc",
                vec![raw_card("sf-b", PreconBoard::Main, 1, 0)],
                None,
            ),
        ];
        rebuild(&db, &first, &cards, &products, Utc::now())
            .await
            .unwrap();
        assert_eq!(PreconDeckCard::find().all(&db).await.unwrap().len(), 2);

        let second = vec![precon(
            "keeper-tmc",
            vec![raw_card("sf-a", PreconBoard::Main, 2, 0)],
            None,
        )];
        rebuild(&db, &second, &cards, &products, Utc::now())
            .await
            .unwrap();

        let decks = PreconDeck::find().all(&db).await.unwrap();
        assert_eq!(decks.len(), 1);
        assert_eq!(decks[0].slug, "keeper-tmc");
        let rows = PreconDeckCard::find().all(&db).await.unwrap();
        assert_eq!(rows.len(), 1, "the dropped deck's cards went with it");
        assert_eq!(rows[0].precon_deck_id, decks[0].id);
        assert_eq!(rows[0].quantity, 2);
    }
}

//! Import a whole deck from Archidekt or Moxfield, then create it atomically.
//!
//! This deliberately sits beside `collection_import`: provider parsing/fetch policy and
//! card-id resolution are reused, but the write path is not the collection reconcile
//! engine. A deck is a new container with sections and cards, inserted all-or-nothing.

mod archidekt;
mod categorize;
mod moxfield;
mod parser;
mod types;

use std::collections::{HashMap, HashSet};

use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter,
    QuerySelect, Set, TransactionTrait,
};

use crate::collection_import::csv_import::PrintingRow;
use crate::collection_import::reconcile::{resolve_card_ids, resolve_newest_printing_by_name};
use crate::collection_import::{
    ImportError, Provider, ProviderContext, UNMATCHED_SAMPLE_CAP, printing_rows_to_holdings,
};
use crate::entities::collection_item::MAX_CARD_QUANTITY;
use crate::entities::prelude::{Card, Deck, DeckCard};
use crate::entities::{card, deck, deck_card, deck_section};

pub use parser::render_text_section_header;
pub use types::{CreatedDeckImport, DeckCardRow, DeckImportFileFormat, ParsedDeck};

const MAX_DECKS_PER_GAME: u64 = 1_000;
const MAX_SECTIONS_PER_DECK: usize = 200;
const MAX_DECK_NAME: usize = 200;
const MAX_FORMAT: usize = 50;
const MAX_SECTION_NAME: usize = 100;
const INSERT_CHUNK: usize = 100;

/// Maximum source rows accepted for one deck import. Real constructed decks and cubes
/// are comfortably below this, while the bound prevents a collection-sized upload from
/// turning one synchronous request into tens of thousands of deck rows.
pub const MAX_DECK_IMPORT_ROWS: usize = 2_000;

pub fn parse_source(provider: Provider, input: &str) -> Result<String, ImportError> {
    let id = match provider {
        Provider::Archidekt => archidekt::parse_deck_id(input),
        Provider::Moxfield => moxfield::parse_deck_id(input),
        // No public API to link to; the handler gates on `network_import_enabled` first,
        // so this is a defensive fallthrough (a Mythic Tools deck arrives as a file).
        Provider::MythicTools => None,
    };
    id.ok_or_else(|| {
        ImportError::InvalidSource(format!(
            "couldn't read a {} deck id from '{}'",
            provider.label(),
            input.trim()
        ))
    })
}

pub async fn fetch_deck(
    provider: Provider,
    ctx: &ProviderContext<'_>,
    deck_id: &str,
) -> Result<ParsedDeck, ImportError> {
    match provider {
        Provider::Archidekt => archidekt::fetch_deck(ctx, deck_id).await,
        Provider::Moxfield => moxfield::fetch_deck(ctx, deck_id).await,
        Provider::MythicTools => Err(ImportError::InvalidSource(format!(
            "{} decks can't be fetched — upload the exported deck file instead",
            provider.label()
        ))),
    }
}

pub fn parse_file(
    provider: Provider,
    format: DeckImportFileFormat,
    name: String,
    bytes: &[u8],
) -> Result<ParsedDeck, ImportError> {
    parser::parse_file(provider, format, name, bytes)
}

/// Resolve every row, aggregate by `(section, card)`, and create a new deck + its
/// imported sections/cards in one transaction. An empty or zero-match import creates
/// nothing. Unlike a normal blank deck, imported decks are not seeded with defaults:
/// explicit provider categories stay authoritative, while generic Mainboard rows may be
/// filed into the preset type buckets.
pub async fn create_deck_from_rows(
    db: &DatabaseConnection,
    user_id: i32,
    game: &str,
    mut parsed: ParsedDeck,
    auto_categorize: bool,
) -> Result<CreatedDeckImport, ImportError> {
    if parsed.rows.len() > MAX_DECK_IMPORT_ROWS {
        return Err(ImportError::TooLarge {
            count: parsed.rows.len(),
            max: MAX_DECK_IMPORT_ROWS,
        });
    }
    if parsed.rows.is_empty() {
        return Err(ImportError::EmptyDeck);
    }
    let total_rows = parsed.rows.len();

    // Reuse the collection importer's exact `(set, collector_number)` resolution for
    // Moxfield rows, while preserving each row's section/name alongside the result.
    let mut tuple_indexes = Vec::new();
    let mut tuple_rows = Vec::new();
    for (index, row) in parsed.rows.iter().enumerate() {
        if row.external_card_id.is_some() {
            continue;
        }
        if let (Some(set_code), Some(collector_number)) =
            (row.set_code.clone(), row.collector_number.clone())
        {
            tuple_indexes.push(index);
            tuple_rows.push(PrintingRow {
                set_code: Some(set_code),
                collector_number: Some(collector_number),
                name: row.card_name.clone(),
                foil: row.foil,
                quantity: row.quantity,
            });
        }
    }
    if !tuple_rows.is_empty() {
        let holdings = printing_rows_to_holdings(db, game, tuple_rows).await?;
        for (index, holding) in tuple_indexes.into_iter().zip(holdings) {
            let row = parsed.rows.get_mut(index).ok_or_else(|| {
                ImportError::Db(sea_orm::DbErr::Custom(
                    "deck import row disappeared during card resolution".to_string(),
                ))
            })?;
            row.external_card_id = Some(holding.external_card_id);
        }
    }

    let external_ids: Vec<String> = parsed
        .rows
        .iter()
        .filter_map(|row| row.external_card_id.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let ids_by_external = resolve_card_ids(db, game, &external_ids).await?;
    let ids_by_name = resolve_names(db, game, &parsed.rows).await?;
    let type_lines = if auto_categorize {
        let resolved_ids: Vec<i32> = ids_by_external
            .values()
            .chain(ids_by_name.values())
            .copied()
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        resolve_type_lines(db, &resolved_ids).await?
    } else {
        HashMap::new()
    };

    let mut aggregate: HashMap<(String, i32), (i64, i64)> = HashMap::new();
    let mut section_order = Vec::new();
    // Keep the first spelling of a section name while treating later case variants as
    // the same section. The canonical spelling must also be used as the aggregate key.
    let mut canonical_sections: HashMap<String, String> = HashMap::new();
    let mut matched_ids = HashSet::new();
    let mut unmatched_labels = HashSet::new();
    let mut unmatched_sample = Vec::new();
    for row in &parsed.rows {
        let card_id = row
            .external_card_id
            .as_ref()
            .and_then(|external| ids_by_external.get(external))
            .copied()
            // Name-only plain text has no printing key, so it deliberately falls back
            // to the newest exact-name catalog printing. A supplied but unmatched
            // set/collector tuple must stay unmatched rather than silently changing art.
            .or_else(|| {
                (row.set_code.is_none() || row.collector_number.is_none())
                    .then(|| ids_by_name.get(&row.card_name).copied())
                    .flatten()
            });
        let Some(card_id) = card_id else {
            let label = unmatched_label(row);
            if unmatched_labels.insert(label.clone())
                && unmatched_sample.len() < UNMATCHED_SAMPLE_CAP
            {
                unmatched_sample.push(label);
            }
            continue;
        };
        matched_ids.insert(card_id);
        let mut cleaned_section = clean_section(&row.section);
        if auto_categorize && categorize::is_generic_section(&cleaned_section) {
            let type_line = type_lines.get(&card_id).and_then(|line| line.as_deref());
            if let Some(preset) = categorize::preset_section(type_line) {
                cleaned_section = preset.to_string();
            }
        }
        let section_key = cleaned_section.to_ascii_lowercase();
        let section = if let Some(canonical) = canonical_sections.get(&section_key) {
            canonical.clone()
        } else {
            if section_order.len() >= MAX_SECTIONS_PER_DECK {
                return Err(ImportError::InvalidSource(format!(
                    "the deck has more than {MAX_SECTIONS_PER_DECK} sections"
                )));
            }
            canonical_sections.insert(section_key, cleaned_section.clone());
            section_order.push(cleaned_section.clone());
            cleaned_section
        };
        let counts = aggregate.entry((section, card_id)).or_default();
        let quantity = i64::from(row.quantity.max(0));
        if row.foil {
            counts.1 += quantity;
        } else {
            counts.0 += quantity;
        }
    }
    if aggregate.is_empty() {
        return Err(ImportError::NoMatchingDeckCards);
    }

    let deck_count = Deck::find()
        .filter(deck::Column::UserId.eq(user_id))
        .filter(deck::Column::Game.eq(game))
        .count(db)
        .await
        .map_err(ImportError::Db)?;
    if deck_count >= MAX_DECKS_PER_GAME {
        return Err(ImportError::InvalidSource(format!(
            "you can have at most {MAX_DECKS_PER_GAME} decks per game"
        )));
    }

    let now = Utc::now();
    let txn = db.begin().await.map_err(ImportError::Db)?;
    let deck = deck::ActiveModel {
        user_id: Set(user_id),
        game: Set(game.to_string()),
        folder_id: Set(None),
        name: Set(clean_name(&parsed.name)),
        description: Set(Some(format!("Imported from {}", parsed.provider.label()))),
        format: Set(parsed.format.map(|format| truncate(&format, MAX_FORMAT))),
        is_public: Set(false),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(&txn)
    .await
    .map_err(ImportError::Db)?;

    let mut section_ids = HashMap::new();
    for (position, section_name) in section_order.into_iter().enumerate() {
        let section = deck_section::ActiveModel {
            deck_id: Set(deck.id),
            name: Set(section_name.clone()),
            position: Set(position as i32),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(&txn)
        .await
        .map_err(ImportError::Db)?;
        section_ids.insert(section_name, section.id);
    }

    let card_count = aggregate
        .values()
        .map(|(regular, foil)| i64::from(clamp_count(*regular)) + i64::from(clamp_count(*foil)))
        .sum();
    let mut cards = Vec::with_capacity(aggregate.len());
    for ((section, card_id), (regular, foil)) in aggregate {
        let section_id = section_ids.get(&section).copied().ok_or_else(|| {
            ImportError::Db(sea_orm::DbErr::Custom(
                "deck import section disappeared before card insertion".to_string(),
            ))
        })?;
        cards.push(deck_card::ActiveModel {
            deck_id: Set(deck.id),
            section_id: Set(section_id),
            card_id: Set(card_id),
            quantity: Set(clamp_count(regular)),
            foil_quantity: Set(clamp_count(foil)),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        });
    }
    for chunk in cards.chunks(INSERT_CHUNK) {
        DeckCard::insert_many(chunk.iter().cloned())
            .exec(&txn)
            .await
            .map_err(ImportError::Db)?;
    }
    txn.commit().await.map_err(ImportError::Db)?;

    Ok(CreatedDeckImport {
        deck,
        card_count,
        provider: parsed.provider,
        total_rows,
        matched_cards: matched_ids.len(),
        unmatched_cards: unmatched_labels.len(),
        unmatched_sample,
    })
}

/// The catalog card id for each row that named a card but no printing. When a plain list
/// names no printing we pick the newest catalog printing deterministically — the shared
/// rule in [`resolve_newest_printing_by_name`], so a pasted list means the same printing
/// whether it's imported as a deck or into a collection. Provider CSVs carrying
/// set/number never take this fallback.
async fn resolve_names(
    db: &DatabaseConnection,
    game: &str,
    rows: &[DeckCardRow],
) -> Result<HashMap<String, i32>, ImportError> {
    let names: Vec<String> = rows
        .iter()
        .filter(|row| row.external_card_id.is_none() && !row.card_name.is_empty())
        .map(|row| row.card_name.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    Ok(resolve_newest_printing_by_name(db, game, &names)
        .await?
        .into_iter()
        .map(|(name, (card_id, _external_id))| (name, card_id))
        .collect())
}

async fn resolve_type_lines(
    db: &DatabaseConnection,
    card_ids: &[i32],
) -> Result<HashMap<i32, Option<String>>, ImportError> {
    let mut result = HashMap::new();
    for chunk in card_ids.chunks(crate::collection_import::IN_CHUNK) {
        let rows: Vec<(i32, Option<String>)> = Card::find()
            .select_only()
            .column(card::Column::Id)
            .column(card::Column::TypeLine)
            .filter(card::Column::Id.is_in(chunk.iter().copied()))
            .into_tuple()
            .all(db)
            .await
            .map_err(ImportError::Db)?;
        result.extend(rows);
    }
    Ok(result)
}

fn unmatched_label(row: &DeckCardRow) -> String {
    if !row.card_name.is_empty() {
        if let (Some(set), Some(number)) = (&row.set_code, &row.collector_number) {
            return format!("{} ({} #{})", row.card_name, set, number);
        }
        return row.card_name.clone();
    }
    row.external_card_id
        .clone()
        .unwrap_or_else(|| "Unknown card".to_string())
}

fn clean_name(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        return "Imported deck".to_string();
    }
    truncate(value, MAX_DECK_NAME)
}

fn clean_section(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        return "Mainboard".to_string();
    }
    truncate(value, MAX_SECTION_NAME)
}

fn truncate(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

fn clamp_count(value: i64) -> i32 {
    value.clamp(0, i64::from(MAX_CARD_QUANTITY)) as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::prelude::DeckSection;
    use crate::test_support::{insert_card, insert_user, migrated_memory_db};
    use sea_orm::sea_query::Expr;

    #[tokio::test]
    async fn creates_sections_and_aggregated_cards_atomically() {
        let db = migrated_memory_db().await;
        let user_id = insert_user(&db, "deck-import@test.example").await;
        let card_id = insert_card(&db, "uid-a").await;
        let result = create_deck_from_rows(
            &db,
            user_id,
            crate::scryfall::GAME,
            ParsedDeck {
                provider: Provider::Archidekt,
                name: "Provider deck".into(),
                format: Some("Commander".into()),
                rows: vec![
                    DeckCardRow {
                        section: "Ramp".into(),
                        card_name: "Sol Ring".into(),
                        external_card_id: Some("uid-a".into()),
                        set_code: None,
                        collector_number: None,
                        foil: false,
                        quantity: 2,
                    },
                    DeckCardRow {
                        section: "Ramp".into(),
                        card_name: "Sol Ring".into(),
                        external_card_id: Some("uid-a".into()),
                        set_code: None,
                        collector_number: None,
                        foil: true,
                        quantity: 1,
                    },
                ],
            },
            true,
        )
        .await
        .expect("import");
        let sections = DeckSection::find()
            .filter(deck_section::Column::DeckId.eq(result.deck.id))
            .all(&db)
            .await
            .expect("sections");
        let cards = DeckCard::find()
            .filter(deck_card::Column::DeckId.eq(result.deck.id))
            .all(&db)
            .await
            .expect("cards");
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].name, "Ramp");
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].card_id, card_id);
        assert_eq!((cards[0].quantity, cards[0].foil_quantity), (2, 1));
    }

    #[tokio::test]
    async fn auto_categorizes_only_generic_sections() {
        let db = migrated_memory_db().await;
        let user_id = insert_user(&db, "deck-categorize@test.example").await;
        let card_id = insert_card(&db, "uid-creature").await;
        Card::update_many()
            .col_expr(
                card::Column::TypeLine,
                Expr::value("Artifact Creature — Golem"),
            )
            .filter(card::Column::Id.eq(card_id))
            .exec(&db)
            .await
            .expect("set type line");

        let parsed = |section: &str, name: &str| ParsedDeck {
            provider: Provider::Archidekt,
            name: name.into(),
            format: None,
            rows: vec![DeckCardRow {
                section: section.into(),
                card_name: "Test creature".into(),
                external_card_id: Some("uid-creature".into()),
                set_code: None,
                collector_number: None,
                foil: false,
                quantity: 1,
            }],
        };

        let automatic = create_deck_from_rows(
            &db,
            user_id,
            crate::scryfall::GAME,
            parsed("Mainboard", "Automatic"),
            true,
        )
        .await
        .expect("automatic import");
        let automatic_section = DeckSection::find()
            .filter(deck_section::Column::DeckId.eq(automatic.deck.id))
            .one(&db)
            .await
            .expect("section query")
            .expect("automatic section");
        assert_eq!(automatic_section.name, "Creatures");

        let explicit = create_deck_from_rows(
            &db,
            user_id,
            crate::scryfall::GAME,
            parsed("Ramp", "Explicit"),
            true,
        )
        .await
        .expect("explicit import");
        let explicit_section = DeckSection::find()
            .filter(deck_section::Column::DeckId.eq(explicit.deck.id))
            .one(&db)
            .await
            .expect("section query")
            .expect("explicit section");
        assert_eq!(explicit_section.name, "Ramp");

        let disabled = create_deck_from_rows(
            &db,
            user_id,
            crate::scryfall::GAME,
            parsed("Mainboard", "Disabled"),
            false,
        )
        .await
        .expect("non-automatic import");
        let disabled_section = DeckSection::find()
            .filter(deck_section::Column::DeckId.eq(disabled.deck.id))
            .one(&db)
            .await
            .expect("section query")
            .expect("disabled section");
        assert_eq!(disabled_section.name, "Mainboard");
    }

    #[tokio::test]
    async fn zero_match_creates_nothing() {
        let db = migrated_memory_db().await;
        let user_id = insert_user(&db, "deck-zero@test.example").await;
        let err = create_deck_from_rows(
            &db,
            user_id,
            crate::scryfall::GAME,
            ParsedDeck {
                provider: Provider::Archidekt,
                name: "No matches".into(),
                format: None,
                rows: vec![DeckCardRow {
                    section: "Mainboard".into(),
                    card_name: "Ghost".into(),
                    external_card_id: Some("missing".into()),
                    set_code: None,
                    collector_number: None,
                    foil: false,
                    quantity: 1,
                }],
            },
            true,
        )
        .await
        .expect_err("zero match");
        assert!(matches!(err, ImportError::NoMatchingDeckCards));
        assert_eq!(Deck::find().count(&db).await.expect("count"), 0);
    }

    #[tokio::test]
    async fn database_boundary_rejects_too_many_rows_before_creating_a_deck() {
        let db = migrated_memory_db().await;
        let user_id = insert_user(&db, "deck-cap@test.example").await;
        let rows = vec![
            DeckCardRow {
                section: "Mainboard".into(),
                card_name: "Repeated".into(),
                external_card_id: Some("uid-a".into()),
                set_code: None,
                collector_number: None,
                foil: false,
                quantity: 1,
            };
            MAX_DECK_IMPORT_ROWS + 1
        ];
        let err = create_deck_from_rows(
            &db,
            user_id,
            crate::scryfall::GAME,
            ParsedDeck {
                provider: Provider::Archidekt,
                name: "Too large".into(),
                format: None,
                rows,
            },
            true,
        )
        .await
        .expect_err("oversized deck");
        assert!(matches!(
            err,
            ImportError::TooLarge { count, max }
                if count == MAX_DECK_IMPORT_ROWS + 1 && max == MAX_DECK_IMPORT_ROWS
        ));
        assert_eq!(Deck::find().count(&db).await.expect("count"), 0);
    }
}

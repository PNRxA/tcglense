//! Streaming import of Scryfall's `default_cards` bulk file into the `cards`
//! and `card_sets` tables, plus the `/sets` metadata.
//!
//! The bulk file is a single JSON array with **one object per line**, so we
//! stream it line-by-line (decompressing gzip on the fly) and upsert in batches
//! — memory stays bounded regardless of the ~500 MB download. Only paper cards
//! and non-digital sets are stored. Progress and completion are recorded in
//! `ingest_state` so the API/UI can report status and so an unchanged dataset
//! (same `updated_at`) is skipped on the next boot.

use std::collections::HashSet;

use chrono::Utc;
use reqwest::Client;
use sea_orm::{
    ActiveValue::{NotSet, Set},
    ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, prelude::DateTimeUtc,
    sea_query::OnConflict,
};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_util::io::StreamReader;

use super::client;
use super::model::{CardFace, ScryfallCard, ScryfallSet, StoredFace};
use super::{DATASET, GAME};
use crate::entities::prelude::{Card, CardSet, IngestState};
use crate::entities::{card, card_set, ingest_state};

/// Rows per upsert. ~34 card columns × 400 ≈ 13.6k bound parameters, comfortably
/// under SQLite's default 32 766 parameter limit.
pub(super) const CARD_BATCH: usize = 400;
const SET_BATCH: usize = 300;
/// Emit a progress update to `ingest_state` every this many flushed card batches.
const PROGRESS_EVERY: u32 = 25;

/// Error type for the background import. Logged, never surfaced to a request.
#[derive(Debug, thiserror::Error)]
pub enum IngestError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("database error: {0}")]
    Db(#[from] sea_orm::DbErr),
    #[error("{0}")]
    Other(String),
}

/// Refresh MTG card data from Scryfall, recording status in `ingest_state`.
///
/// On error the state row is best-effort marked `"error"` so the next boot
/// retries, and the error is returned for logging by the caller.
pub async fn refresh(db: &DatabaseConnection, client: &Client) -> Result<(), IngestError> {
    match refresh_inner(db, client).await {
        Ok(()) => Ok(()),
        Err(err) => {
            let _ = put_state(
                db,
                None,
                "error",
                Some(truncate(&err.to_string(), 500)),
                0,
                0,
                None,
                Some(Utc::now()),
            )
            .await;
            Err(err)
        }
    }
}

async fn refresh_inner(db: &DatabaseConnection, client: &Client) -> Result<(), IngestError> {
    let entry = client::bulk_data(client)
        .await?
        .into_iter()
        .find(|b| b.kind == DATASET)
        .ok_or_else(|| IngestError::Other(format!("scryfall bulk dataset '{DATASET}' not found")))?;

    // Skip if we already imported this exact version.
    let existing = IngestState::find()
        .filter(ingest_state::Column::Game.eq(GAME))
        .filter(ingest_state::Column::Dataset.eq(DATASET))
        .one(db)
        .await?;
    if let Some(state) = &existing
        && state.status == "complete"
        && state.source_updated_at.as_deref() == Some(entry.updated_at.as_str())
    {
        tracing::info!(updated_at = %entry.updated_at, "scryfall {DATASET} already up to date");
        return Ok(());
    }

    let started = Utc::now();
    tracing::info!(
        updated_at = %entry.updated_at,
        size_mb = entry.size.unwrap_or(0) / 1_000_000,
        "importing scryfall {DATASET}"
    );
    put_state(db, Some(entry.updated_at.clone()), "running", Some("importing sets".into()), 0, 0, Some(started), None).await?;

    // Sets first, so cards can reference stored sets.
    let sets = client::all_sets(client).await?;
    let paper_codes: HashSet<String> = sets
        .iter()
        .filter(|s| !s.digital.unwrap_or(false))
        .map(|s| s.code.to_lowercase())
        .collect();
    let sets_imported = import_sets(db, &sets).await?;
    put_state(
        db,
        Some(entry.updated_at.clone()),
        "running",
        Some("importing cards".into()),
        sets_imported,
        0,
        Some(started),
        None,
    )
    .await?;

    let cards_imported = import_cards(
        db,
        client,
        &entry.download_uri,
        &paper_codes,
        &entry.updated_at,
        sets_imported,
        started,
    )
    .await?;

    // A run that fetched everything but stored nothing means the data was bad
    // (e.g. the bulk download served a non-card body, or the format drifted). Fail
    // it rather than recording "complete", which would version-lock the empty state
    // and suppress re-import on the next boot.
    if cards_imported == 0 {
        return Err(IngestError::Other(format!(
            "import produced 0 cards from {sets_imported} sets; treating as failure to retry"
        )));
    }

    put_state(
        db,
        Some(entry.updated_at.clone()),
        "complete",
        None,
        sets_imported,
        cards_imported,
        Some(started),
        Some(Utc::now()),
    )
    .await?;
    tracing::info!(sets = sets_imported, cards = cards_imported, "scryfall import complete");
    Ok(())
}

/// Upsert all non-digital (paper) sets. Returns the number stored.
pub(super) async fn import_sets(
    db: &DatabaseConnection,
    sets: &[ScryfallSet],
) -> Result<i32, IngestError> {
    let now = Utc::now();
    let models: Vec<card_set::ActiveModel> = sets
        .iter()
        .filter(|s| !s.digital.unwrap_or(false))
        .map(|s| map_set(s, now))
        .collect();
    let count = models.len() as i32;

    let mut iter = models.into_iter();
    loop {
        let chunk: Vec<card_set::ActiveModel> = iter.by_ref().take(SET_BATCH).collect();
        if chunk.is_empty() {
            break;
        }
        CardSet::insert_many(chunk)
            .on_conflict(
                OnConflict::columns([card_set::Column::Game, card_set::Column::Code])
                    .update_columns([
                        card_set::Column::Name,
                        card_set::Column::SetType,
                        card_set::Column::ReleasedAt,
                        card_set::Column::CardCount,
                        card_set::Column::Digital,
                        card_set::Column::IconSvgUri,
                        card_set::Column::ParentSetCode,
                        card_set::Column::ExternalId,
                        card_set::Column::UpdatedAt,
                    ])
                    .to_owned(),
            )
            .exec_without_returning(db)
            .await?;
    }
    Ok(count)
}

/// Stream the bulk card file and upsert paper cards in batches. Returns the
/// number stored.
#[allow(clippy::too_many_arguments)]
async fn import_cards(
    db: &DatabaseConnection,
    client: &Client,
    url: &str,
    paper_codes: &HashSet<String>,
    updated_at: &str,
    sets_imported: i32,
    started: DateTimeUtc,
) -> Result<i32, IngestError> {
    let stream = client::download_stream(client, url).await?;
    let mut lines = BufReader::with_capacity(64 * 1024, StreamReader::new(stream)).lines();

    let now = Utc::now();
    let mut batch: Vec<card::ActiveModel> = Vec::with_capacity(CARD_BATCH);
    let mut total: i32 = 0;
    let mut batches_since_progress: u32 = 0;
    let mut skipped_parse: u64 = 0;

    while let Some(raw) = lines.next_line().await? {
        // Each element sits on its own line, terminated by a comma except the
        // last; the array brackets get their own lines.
        let line = raw.trim();
        let line = line.strip_suffix(',').unwrap_or(line).trim();
        if line.is_empty() || line == "[" || line == "]" {
            continue;
        }
        let scry: ScryfallCard = match serde_json::from_str(line) {
            Ok(card) => card,
            Err(err) => {
                skipped_parse += 1;
                if skipped_parse <= 5 {
                    tracing::warn!(error = %err, "skipping unparseable card line");
                }
                continue;
            }
        };

        // Paper only, and only cards whose set we actually stored.
        let set_code = scry.set.to_lowercase();
        if !scry.games.iter().any(|g| g == "paper") || !paper_codes.contains(&set_code) {
            continue;
        }
        batch.push(map_card(scry, now));

        if batch.len() >= CARD_BATCH {
            let n = batch.len() as i32;
            flush_cards(db, std::mem::take(&mut batch)).await?;
            total += n;
            batch.reserve(CARD_BATCH);
            batches_since_progress += 1;
            if batches_since_progress >= PROGRESS_EVERY {
                batches_since_progress = 0;
                let _ = put_state(
                    db,
                    Some(updated_at.to_string()),
                    "running",
                    Some(format!("imported {total} cards")),
                    sets_imported,
                    total,
                    Some(started),
                    None,
                )
                .await;
            }
        }
    }

    if !batch.is_empty() {
        let n = batch.len() as i32;
        flush_cards(db, batch).await?;
        total += n;
    }
    if skipped_parse > 0 {
        tracing::warn!(count = skipped_parse, "skipped unparseable card lines");
    }
    Ok(total)
}

pub(super) async fn flush_cards(
    db: &DatabaseConnection,
    batch: Vec<card::ActiveModel>,
) -> Result<(), IngestError> {
    if batch.is_empty() {
        return Ok(());
    }
    Card::insert_many(batch)
        .on_conflict(
            OnConflict::columns([card::Column::Game, card::Column::ExternalId])
                .update_columns([
                    card::Column::OracleId,
                    card::Column::Name,
                    card::Column::SetCode,
                    card::Column::SetName,
                    card::Column::CollectorNumber,
                    card::Column::CollectorNumberInt,
                    card::Column::Rarity,
                    card::Column::Lang,
                    card::Column::ReleasedAt,
                    card::Column::ManaCost,
                    card::Column::Cmc,
                    card::Column::TypeLine,
                    card::Column::ColorIdentity,
                    card::Column::Colors,
                    card::Column::Layout,
                    card::Column::OracleText,
                    card::Column::Power,
                    card::Column::Toughness,
                    card::Column::Loyalty,
                    card::Column::ImageSmall,
                    card::Column::ImageNormal,
                    card::Column::ImageLarge,
                    card::Column::ImageArtCrop,
                    card::Column::ImagePng,
                    card::Column::CardFaces,
                    card::Column::PriceUsd,
                    card::Column::PriceUsdFoil,
                    card::Column::PriceEur,
                    card::Column::PriceTix,
                    card::Column::Digital,
                    card::Column::UpdatedAt,
                ])
                .to_owned(),
        )
        .exec_without_returning(db)
        .await?;
    Ok(())
}

fn map_set(set: &ScryfallSet, now: DateTimeUtc) -> card_set::ActiveModel {
    card_set::ActiveModel {
        id: NotSet,
        game: Set(GAME.to_string()),
        code: Set(set.code.to_lowercase()),
        name: Set(set.name.clone()),
        set_type: Set(set.set_type.clone()),
        released_at: Set(set.released_at.clone()),
        card_count: Set(set.card_count.unwrap_or(0) as i32),
        digital: Set(set.digital.unwrap_or(false)),
        icon_svg_uri: Set(set.icon_svg_uri.clone()),
        parent_set_code: Set(set.parent_set_code.clone()),
        external_id: Set(Some(set.id.clone())),
        created_at: Set(now),
        updated_at: Set(now),
    }
}

pub(super) fn map_card(card: ScryfallCard, now: DateTimeUtc) -> card::ActiveModel {
    // Resolve display images from the top-level `image_uris`, falling back to the
    // first face for multi-faced cards (which have no top-level images).
    let (image_small, image_normal, image_large, image_art_crop, image_png) = {
        let primary = card.image_uris.as_ref().or_else(|| {
            card.card_faces
                .as_ref()
                .and_then(|faces| faces.first())
                .and_then(|face| face.image_uris.as_ref())
        });
        (
            primary.and_then(|u| u.small.clone()),
            primary.and_then(|u| u.normal.clone()),
            primary.and_then(|u| u.large.clone()),
            primary.and_then(|u| u.art_crop.clone()),
            primary.and_then(|u| u.png.clone()),
        )
    };

    let card_faces = match &card.card_faces {
        Some(faces) if !faces.is_empty() => {
            let stored: Vec<StoredFace> = faces.iter().map(StoredFace::from_face).collect();
            serde_json::to_string(&stored).ok()
        }
        _ => None,
    };

    let (price_usd, price_usd_foil, price_eur, price_tix) = match &card.prices {
        Some(p) => (p.usd.clone(), p.usd_foil.clone(), p.eur.clone(), p.tix.clone()),
        None => (None, None, None, None),
    };

    let color_identity = join_colors(&card.color_identity);
    let colors = join_colors(&card.colors);
    let collector_number_int = leading_int(&card.collector_number);

    // Searchable gameplay text and creature stats. Single-faced cards carry these
    // at the top level; multi-faced cards carry them per face. For `oracle_text`
    // we join the faces' text (so an `o:` search matches text on either face);
    // for power/toughness/loyalty we take the first face that has a value.
    let oracle_text = card.oracle_text.clone().or_else(|| {
        card.card_faces.as_ref().and_then(|faces| {
            let joined = faces
                .iter()
                .filter_map(|f| f.oracle_text.as_deref())
                .filter(|t| !t.is_empty())
                .collect::<Vec<_>>()
                .join("\n//\n");
            (!joined.is_empty()).then_some(joined)
        })
    });
    let power = card.power.clone().or_else(|| face_stat(&card.card_faces, |f| &f.power));
    let toughness = card.toughness.clone().or_else(|| face_stat(&card.card_faces, |f| &f.toughness));
    let loyalty = card.loyalty.clone().or_else(|| face_stat(&card.card_faces, |f| &f.loyalty));

    card::ActiveModel {
        id: NotSet,
        game: Set(GAME.to_string()),
        external_id: Set(card.id),
        oracle_id: Set(card.oracle_id),
        name: Set(card.name),
        set_code: Set(card.set.to_lowercase()),
        set_name: Set(card.set_name),
        collector_number: Set(card.collector_number),
        collector_number_int: Set(collector_number_int),
        rarity: Set(card.rarity),
        lang: Set(card.lang),
        released_at: Set(card.released_at),
        mana_cost: Set(card.mana_cost),
        cmc: Set(card.cmc),
        type_line: Set(card.type_line),
        color_identity: Set(color_identity),
        colors: Set(colors),
        layout: Set(card.layout),
        oracle_text: Set(oracle_text),
        power: Set(power),
        toughness: Set(toughness),
        loyalty: Set(loyalty),
        image_small: Set(image_small),
        image_normal: Set(image_normal),
        image_large: Set(image_large),
        image_art_crop: Set(image_art_crop),
        image_png: Set(image_png),
        card_faces: Set(card_faces),
        price_usd: Set(price_usd),
        price_usd_foil: Set(price_usd_foil),
        price_eur: Set(price_eur),
        price_tix: Set(price_tix),
        digital: Set(card.digital.unwrap_or(false)),
        created_at: Set(now),
        updated_at: Set(now),
    }
}

fn join_colors(value: &Option<Vec<String>>) -> Option<String> {
    match value {
        Some(colors) if !colors.is_empty() => Some(colors.join(",")),
        _ => None,
    }
}

/// First face that carries a value for the given stat accessor (power/toughness/
/// loyalty live per-face on multi-faced cards rather than at the top level).
fn face_stat(
    faces: &Option<Vec<CardFace>>,
    get: impl Fn(&CardFace) -> &Option<String>,
) -> Option<String> {
    faces
        .as_ref()
        .and_then(|fs| fs.iter().find_map(|f| get(f).clone()))
}

/// Parse the leading run of ASCII digits of a collector number (`"12a"` -> 12).
fn leading_int(collector_number: &str) -> Option<i32> {
    let digits: String = collector_number
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

pub(super) fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max).collect();
        format!("{head}…")
    }
}

/// Upsert the single `ingest_state` row for `(GAME, DATASET)`.
#[allow(clippy::too_many_arguments)]
pub(super) async fn put_state(
    db: &DatabaseConnection,
    source_updated_at: Option<String>,
    status: &str,
    detail: Option<String>,
    sets_imported: i32,
    cards_imported: i32,
    started_at: Option<DateTimeUtc>,
    finished_at: Option<DateTimeUtc>,
) -> Result<(), IngestError> {
    let model = ingest_state::ActiveModel {
        id: NotSet,
        game: Set(GAME.to_string()),
        dataset: Set(DATASET.to_string()),
        source_updated_at: Set(source_updated_at),
        status: Set(status.to_string()),
        detail: Set(detail),
        sets_imported: Set(sets_imported),
        cards_imported: Set(cards_imported),
        started_at: Set(started_at),
        finished_at: Set(finished_at),
    };
    IngestState::insert(model)
        .on_conflict(
            OnConflict::columns([ingest_state::Column::Game, ingest_state::Column::Dataset])
                .update_columns([
                    ingest_state::Column::SourceUpdatedAt,
                    ingest_state::Column::Status,
                    ingest_state::Column::Detail,
                    ingest_state::Column::SetsImported,
                    ingest_state::Column::CardsImported,
                    ingest_state::Column::StartedAt,
                    ingest_state::Column::FinishedAt,
                ])
                .to_owned(),
        )
        .exec_without_returning(db)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_CARD: &str = r#"{"object":"card","id":"abc-123","oracle_id":"ora-1","name":"Llanowar Elves","lang":"en","released_at":"2018-07-13","set":"M19","set_name":"Core Set 2019","collector_number":"314","rarity":"common","layout":"normal","mana_cost":"{G}","cmc":1.0,"type_line":"Creature — Elf Druid","oracle_text":"{T}: Add {G}.","power":"1","toughness":"1","color_identity":["G"],"colors":["G"],"digital":false,"games":["paper","mtgo"],"image_uris":{"small":"https://img/small.jpg","normal":"https://img/normal.jpg","large":"https://img/large.jpg","png":"https://img/card.png","art_crop":"https://img/art.jpg"},"prices":{"usd":"0.25","usd_foil":"1.50","eur":"0.10","tix":"0.03"}}"#;

    #[test]
    fn maps_a_simple_card() {
        let scry: ScryfallCard = serde_json::from_str(SAMPLE_CARD).unwrap();
        assert!(scry.games.iter().any(|g| g == "paper"));
        let now = Utc::now();
        let model = map_card(scry, now);
        assert_eq!(model.external_id.as_ref(), "abc-123");
        // Set code is lowercased so it matches stored sets.
        assert_eq!(model.set_code.as_ref(), "m19");
        assert_eq!(model.color_identity.as_ref().as_deref(), Some("G"));
        assert_eq!(model.image_normal.as_ref().as_deref(), Some("https://img/normal.jpg"));
        assert_eq!(model.price_usd.as_ref().as_deref(), Some("0.25"));
        assert_eq!(model.oracle_text.as_ref().as_deref(), Some("{T}: Add {G}."));
        assert_eq!(model.power.as_ref().as_deref(), Some("1"));
        assert_eq!(model.toughness.as_ref().as_deref(), Some("1"));
        assert!(model.loyalty.as_ref().is_none());
        assert!(model.card_faces.as_ref().is_none());
    }

    #[test]
    fn double_faced_card_uses_front_face_images_and_stores_faces() {
        let dfc = r#"{"object":"card","id":"dfc-1","name":"Delver of Secrets // Insectile Aberration","lang":"en","set":"isd","set_name":"Innistrad","collector_number":"51","games":["paper"],"layout":"transform","card_faces":[{"name":"Delver of Secrets","mana_cost":"{U}","type_line":"Creature — Human Wizard","oracle_text":"At the beginning of your upkeep, look at the top card.","power":"1","toughness":"1","image_uris":{"small":"https://img/front-small.jpg","normal":"https://img/front.jpg"}},{"name":"Insectile Aberration","mana_cost":"","type_line":"Creature — Human Insect","oracle_text":"Flying","power":"3","toughness":"2","image_uris":{"small":"https://img/back-small.jpg","normal":"https://img/back.jpg"}}]}"#;
        let scry: ScryfallCard = serde_json::from_str(dfc).unwrap();
        let model = map_card(scry, Utc::now());
        // Falls back to the front face for the listing thumbnail.
        assert_eq!(model.image_normal.as_ref().as_deref(), Some("https://img/front.jpg"));
        // Both faces are persisted as JSON.
        let faces = model.card_faces.as_ref().clone().unwrap();
        assert!(faces.contains("Insectile Aberration"));
        assert!(faces.contains("https://img/back.jpg"));
        // Oracle text joins both faces; P/T come from the first face that has them.
        let oracle = model.oracle_text.as_ref().clone().unwrap();
        assert!(oracle.contains("top card"));
        assert!(oracle.contains("Flying"));
        assert_eq!(model.power.as_ref().as_deref(), Some("1"));
        assert_eq!(model.toughness.as_ref().as_deref(), Some("1"));
    }

    #[test]
    fn join_colors_handles_empty_and_present() {
        assert_eq!(join_colors(&None), None);
        assert_eq!(join_colors(&Some(vec![])), None);
        assert_eq!(join_colors(&Some(vec!["W".into(), "U".into()])), Some("W,U".to_string()));
    }

    #[test]
    fn leading_int_parses_digit_prefix() {
        assert_eq!(leading_int("314"), Some(314));
        assert_eq!(leading_int("12a"), Some(12));
        assert_eq!(leading_int("★"), None);
        assert_eq!(leading_int("GR-1"), None);
    }

    #[test]
    fn truncate_respects_char_boundaries() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 5).chars().count(), 6); // 5 chars + ellipsis
    }
}

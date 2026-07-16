//! Export a deck as an Archidekt CSV, Moxfield CSV, or Moxfield plain-text list.

use std::collections::HashMap;

use axum::extract::State;
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use csv::{QuoteStyle, Terminator, WriterBuilder};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
use serde::Deserialize;

use crate::auth::extractor::AuthUser;
use crate::deck_import::render_text_section_header;
use crate::entities::prelude::{Card, DeckCard, DeckSection};
use crate::entities::{card, deck_card, deck_section};
use crate::error::AppError;
use crate::extract::{Path, Query};
use crate::handlers::collection::export::csv_download;
use crate::handlers::shared::require_game;
use crate::state::AppState;

use super::load_deck;

#[derive(Debug, Deserialize)]
pub struct ExportParams {
    pub format: Option<String>,
}

#[derive(Clone, Copy)]
enum ExportFormat {
    Archidekt,
    Moxfield,
    MoxfieldText,
}

impl ExportFormat {
    fn parse(value: Option<&str>) -> Result<Self, AppError> {
        match value
            .map(|value| value.trim().to_ascii_lowercase())
            .as_deref()
        {
            None | Some("") | Some("archidekt") => Ok(Self::Archidekt),
            Some("moxfield") => Ok(Self::Moxfield),
            Some("moxfield-text") | Some("text") => Ok(Self::MoxfieldText),
            Some(other) => Err(AppError::Validation(format!(
                "unknown deck export format '{other}' (expected 'archidekt', 'moxfield', or 'moxfield-text')"
            ))),
        }
    }
}

/// Export deck
///
/// `GET /api/decks/{game}/{deck_id}/export?format=...` downloads the caller's whole deck
/// as an Archidekt CSV, Moxfield CSV, or Moxfield plain-text list. Every format preserves
/// sections/boards, exact printings, finishes, and quantities and round-trips through the
/// deck importer.
#[utoipa::path(
    get,
    path = "/api/decks/{game}/{deck_id}/export",
    tag = "Decks",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("deck_id" = i32, Path, description = "Deck id"),
        ("format" = Option<String>, Query, description = "`archidekt`, `moxfield`, or `moxfield-text`"),
    ),
    responses(
        (status = 200, description = "A downloadable provider-shaped deck list."),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game or the deck is not the caller's."),
        (status = 422, description = "Unknown export format."),
    ),
)]
pub async fn export_deck(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((game, deck_id)): Path<(String, i32)>,
    Query(params): Query<ExportParams>,
) -> Result<Response, AppError> {
    require_game(&game)?;
    let deck = load_deck(&state, user.id, &game, deck_id).await?;
    let format = ExportFormat::parse(params.format.as_deref())?;
    let sections = DeckSection::find()
        .filter(deck_section::Column::DeckId.eq(deck.id))
        .order_by_asc(deck_section::Column::Position)
        .order_by_asc(deck_section::Column::Id)
        .all(&state.db)
        .await?;
    let section_names: HashMap<i32, (i32, String)> = sections
        .into_iter()
        .map(|section| (section.id, (section.position, section.name)))
        .collect();
    let mut rows: Vec<(deck_card::Model, Option<card::Model>)> = DeckCard::find()
        .find_also_related(Card)
        .filter(deck_card::Column::DeckId.eq(deck.id))
        .all(&state.db)
        .await?;
    rows.sort_by(|(left, left_card), (right, right_card)| {
        let left_section = section_names
            .get(&left.section_id)
            .map(|value| value.0)
            .unwrap_or(i32::MAX);
        let right_section = section_names
            .get(&right.section_id)
            .map(|value| value.0)
            .unwrap_or(i32::MAX);
        left_section
            .cmp(&right_section)
            .then_with(|| card_sort(left_card.as_ref()).cmp(&card_sort(right_card.as_ref())))
    });

    match format {
        ExportFormat::Archidekt => csv_download(
            build_csv(ExportFormat::Archidekt, &rows, &section_names)?,
            &format!("tcglense-{game}-deck-{}-archidekt.csv", deck.id),
        ),
        ExportFormat::Moxfield => csv_download(
            build_csv(ExportFormat::Moxfield, &rows, &section_names)?,
            &format!("tcglense-{game}-deck-{}-moxfield.csv", deck.id),
        ),
        ExportFormat::MoxfieldText => text_download(
            build_text(&rows, &section_names),
            &format!("tcglense-{game}-deck-{}-moxfield.txt", deck.id),
        ),
    }
}

fn card_sort(card: Option<&card::Model>) -> (String, String, String) {
    card.map(|card| {
        (
            card.name.to_ascii_lowercase(),
            card.set_code.clone(),
            card.collector_number.clone(),
        )
    })
    .unwrap_or_default()
}

fn build_csv(
    format: ExportFormat,
    rows: &[(deck_card::Model, Option<card::Model>)],
    sections: &HashMap<i32, (i32, String)>,
) -> Result<String, AppError> {
    let mut writer = WriterBuilder::new()
        .quote_style(match format {
            ExportFormat::Archidekt => QuoteStyle::Necessary,
            ExportFormat::Moxfield | ExportFormat::MoxfieldText => QuoteStyle::Always,
        })
        .terminator(Terminator::CRLF)
        .from_writer(Vec::new());
    let header = match format {
        ExportFormat::Archidekt => [
            "Quantity",
            "Name",
            "Finish",
            "Scryfall ID",
            "Categories",
            "Edition Code",
            "Collector Number",
        ]
        .as_slice(),
        ExportFormat::Moxfield => [
            "Count",
            "Name",
            "Edition",
            "Foil",
            "Collector Number",
            "Board",
        ]
        .as_slice(),
        ExportFormat::MoxfieldText => {
            return Err(AppError::Internal(
                "plain-text deck export was sent to the CSV builder".to_string(),
            ));
        }
    };
    writer
        .write_record(header)
        .map_err(|error| AppError::Internal(format!("failed to build deck CSV: {error}")))?;
    for (item, card) in rows {
        let Some(card) = card else { continue };
        let section = sections
            .get(&item.section_id)
            .map(|value| value.1.as_str())
            .unwrap_or("Mainboard");
        if item.quantity > 0 {
            write_csv_row(&mut writer, format, card, section, item.quantity, false)?;
        }
        if item.foil_quantity > 0 {
            write_csv_row(&mut writer, format, card, section, item.foil_quantity, true)?;
        }
    }
    let bytes = writer.into_inner().map_err(|error| {
        AppError::Internal(format!(
            "failed to finalize deck CSV: {}",
            error.into_error()
        ))
    })?;
    String::from_utf8(bytes)
        .map_err(|_| AppError::Internal("deck CSV was not valid UTF-8".to_string()))
}

fn write_csv_row(
    writer: &mut csv::Writer<Vec<u8>>,
    format: ExportFormat,
    card: &card::Model,
    section: &str,
    quantity: i32,
    foil: bool,
) -> Result<(), AppError> {
    let quantity = quantity.to_string();
    let row = match format {
        ExportFormat::Archidekt => vec![
            quantity,
            card.name.clone(),
            if foil { "Foil" } else { "Normal" }.to_string(),
            card.external_id.clone(),
            section.to_string(),
            card.set_code.clone(),
            card.collector_number.clone(),
        ],
        ExportFormat::Moxfield => vec![
            quantity,
            card.name.clone(),
            card.set_code.clone(),
            if foil { "foil" } else { "" }.to_string(),
            card.collector_number.clone(),
            section.to_string(),
        ],
        ExportFormat::MoxfieldText => {
            return Err(AppError::Internal(
                "plain-text deck export was sent to the CSV row builder".to_string(),
            ));
        }
    };
    writer
        .write_record(row)
        .map_err(|error| AppError::Internal(format!("failed to build deck CSV: {error}")))
}

fn build_text(
    rows: &[(deck_card::Model, Option<card::Model>)],
    sections: &HashMap<i32, (i32, String)>,
) -> String {
    let mut output = String::new();
    let mut current_section: Option<i32> = None;
    for (item, card) in rows {
        let Some(card) = card else { continue };
        if current_section != Some(item.section_id) {
            if !output.is_empty() {
                output.push('\n');
            }
            output.push_str(&render_text_section_header(
                sections
                    .get(&item.section_id)
                    .map(|value| value.1.as_str())
                    .unwrap_or("Mainboard"),
            ));
            output.push('\n');
            current_section = Some(item.section_id);
        }
        if item.quantity > 0 {
            output.push_str(&text_row(card, item.quantity, false));
        }
        if item.foil_quantity > 0 {
            output.push_str(&text_row(card, item.foil_quantity, true));
        }
    }
    output
}

fn text_row(card: &card::Model, quantity: i32, foil: bool) -> String {
    format!(
        "{quantity} {} ({}) {}{}\n",
        card.name,
        card.set_code.to_ascii_uppercase(),
        card.collector_number,
        if foil { " *F*" } else { "" }
    )
}

fn text_download(body: String, filename: &str) -> Result<Response, AppError> {
    let disposition = HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
        .map_err(|_| AppError::Internal("invalid export filename".into()))?;
    Ok((
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/plain; charset=utf-8"),
            ),
            (header::CONTENT_DISPOSITION, disposition),
        ],
        body,
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_export_formats() {
        assert!(matches!(
            ExportFormat::parse(None),
            Ok(ExportFormat::Archidekt)
        ));
        assert!(matches!(
            ExportFormat::parse(Some("moxfield")),
            Ok(ExportFormat::Moxfield)
        ));
        assert!(matches!(
            ExportFormat::parse(Some("moxfield-text")),
            Ok(ExportFormat::MoxfieldText)
        ));
        assert!(ExportFormat::parse(Some("arena")).is_err());
    }
}

//! Create a deck from an Archidekt/Moxfield link or uploaded deck export.

use axum::{Json, extract::State};

use crate::auth::extractor::WritableUser;
use crate::collection_import::{ProgressReporter, Provider, ProviderContext};
use crate::deck_import;
use crate::error::AppError;
use crate::extract::{JsonBody, Path};
use crate::handlers::shared::require_game;
use crate::state::AppState;

use super::{DeckImportRequest, DeckImportResponse, DeckResponse, validate_name};

/// Maximum decoded file contents accepted in the JSON import body. The router applies
/// the same 16 MiB cap to the encoded request; this second check keeps the parser bound
/// explicit and produces a useful validation error.
pub const MAX_DECK_UPLOAD_BYTES: usize = 16 * 1024 * 1024;

/// Import deck
///
/// `POST /api/decks/{game}/import` creates a new deck from either a public provider URL
/// (`source`) or uploaded CSV/plain-text contents (`contents` + `format`). Provider
/// boards/categories become the deck's exact sections. The fetch/parse, card resolution,
/// and straight insert run inline; no collection reconcile/job queue is involved.
#[utoipa::path(
    post,
    path = "/api/decks/{game}/import",
    tag = "Decks",
    security(("api_key" = [])),
    params(("game" = String, Path, description = "Game id slug, e.g. `mtg`")),
    request_body = DeckImportRequest,
    responses(
        (status = 200, description = "The created deck header and card-match summary.", body = DeckImportResponse),
        (status = 401, description = "Missing or invalid API key."),
        (status = 403, description = "API key is read-only."),
        (status = 404, description = "Unknown game or no public deck at the provider id."),
        (status = 422, description = "Invalid source/file/provider, empty deck, or no catalog matches."),
        (status = 502, description = "The provider denied or failed the request."),
        (status = 503, description = "The provider repeatedly rate-limited the request."),
    ),
)]
pub async fn import_deck(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
    Path(game): Path<String>,
    JsonBody(payload): JsonBody<DeckImportRequest>,
) -> Result<Json<DeckImportResponse>, AppError> {
    require_game(&game)?;
    let provider = Provider::from_id(&payload.provider).ok_or_else(|| {
        AppError::Validation(format!(
            "unknown deck provider '{}'",
            payload.provider.trim()
        ))
    })?;
    if !provider.supports_game(&game) {
        return Err(AppError::Validation(format!(
            "{} deck import is not available for '{game}'",
            provider.label()
        )));
    }

    let parsed = match (payload.source, payload.contents) {
        (Some(source), None) => {
            // Match the collection link-import gate exactly: Moxfield remains upload-only
            // until its explicitly approved User-Agent path is enabled.
            if !provider.network_import_enabled() {
                return Err(AppError::Validation(format!(
                    "{} live import is temporarily unavailable; upload a deck CSV or text export instead",
                    provider.label()
                )));
            }
            let deck_id = deck_import::parse_source(provider, &source)?;
            let progress = ProgressReporter::default();
            let context = ProviderContext {
                http: &state.http,
                limiters: state.imports.limiters(),
                settings: state.imports.settings(),
                progress: &progress,
            };
            deck_import::fetch_deck(provider, &context, &deck_id).await?
        }
        (None, Some(contents)) => {
            if contents.is_empty() {
                return Err(AppError::Validation(
                    "no deck file was uploaded".to_string(),
                ));
            }
            if contents.len() > MAX_DECK_UPLOAD_BYTES {
                return Err(AppError::Validation(format!(
                    "deck file is too large (limit is {} MiB)",
                    MAX_DECK_UPLOAD_BYTES / 1024 / 1024
                )));
            }
            let format = payload.format.ok_or_else(|| {
                AppError::Validation("an uploaded deck needs a file format".to_string())
            })?;
            if provider == Provider::Archidekt && format == deck_import::DeckImportFileFormat::Text
            {
                return Err(AppError::Validation(
                    "Archidekt deck uploads must use its CSV export".to_string(),
                ));
            }
            let name = match payload.name {
                Some(name) => validate_name(&name, "name", 200)?,
                None => format!("Imported {} deck", provider.label()),
            };
            deck_import::parse_file(provider, format, name, contents.as_bytes())?
        }
        (Some(_), Some(_)) => {
            return Err(AppError::Validation(
                "provide either a deck source link or uploaded contents, not both".to_string(),
            ));
        }
        (None, None) => {
            return Err(AppError::Validation(
                "provide a deck source link or uploaded contents".to_string(),
            ));
        }
    };

    let created = deck_import::create_deck_from_rows(&state.db, user.id, &game, parsed).await?;
    Ok(Json(DeckImportResponse {
        deck: DeckResponse::from_model(&created.deck, created.card_count),
        provider: created.provider.as_str().to_string(),
        total_rows: created.total_rows,
        matched_cards: created.matched_cards,
        unmatched_cards: created.unmatched_cards,
        unmatched_sample: created.unmatched_sample,
    }))
}

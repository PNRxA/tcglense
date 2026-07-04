//! Catalog image proxy: streams a card image (per face/size), downloading + caching it
//! on disk on first request. The URL allow-list is also reused by the set-icon proxy.

use axum::{
    extract::{Path, Query, State},
    http::header,
    response::{IntoResponse, Response},
};

use crate::entities::card;
use crate::error::AppError;
use crate::handlers::shared::{load_card, require_game, stored_faces};
use crate::scryfall::model::StoredFace;
use crate::state::AppState;

use super::{IMAGE_CACHE_CONTROL, ImageParams};

/// `GET /api/games/{game}/cards/{id}/image?size=normal&face=0`
///
/// Streams the cached image, downloading + persisting it on first request.
pub async fn card_image(
    State(state): State<AppState>,
    Path((game, id)): Path<(String, String)>,
    Query(params): Query<ImageParams>,
) -> Result<Response, AppError> {
    require_game(&game)?;
    let card = load_card(&state, &game, &id).await?;
    let size = normalize_size(params.size.as_deref());

    // Resolve the upstream URL (and a stable cache key) for the requested face.
    let (source_url, cache_key) = match params.face {
        Some(idx) => {
            let face = stored_faces(&card)
                .into_iter()
                .nth(idx)
                .ok_or_else(|| AppError::NotFound(format!("card '{id}' has no face {idx}")))?;
            let url = face_image_url(&face, size)
                .ok_or_else(|| AppError::NotFound("no image for that face/size".to_string()))?;
            (url, format!("{id}-f{idx}"))
        }
        None => {
            let url = card_image_url(&card, size)
                .or_else(|| {
                    // Multi-faced cards have no top-level image; use the front face.
                    stored_faces(&card)
                        .into_iter()
                        .next()
                        .and_then(|f| face_image_url(&f, size))
                })
                .ok_or_else(|| AppError::NotFound("no image for that card/size".to_string()))?;
            (url, id.clone())
        }
    };

    // Defense-in-depth: only ever fetch from the provider CDN, so a bad stored URL
    // can't turn this public proxy into an SSRF. All images are Scryfall today.
    if !is_allowed_image_url(&source_url) {
        tracing::warn!(card = %id, url = %source_url, "refusing to proxy non-allowlisted image URL");
        return Err(AppError::NotFound("no image available".to_string()));
    }

    let image = state
        .images
        .get(&game, size, &cache_key, &source_url)
        .await
        .map_err(|err| {
            tracing::error!(error = %err, card = %id, "failed to cache card image");
            AppError::Internal(format!("image cache error: {err}"))
        })?;

    Ok((
        [
            (header::CONTENT_TYPE, image.content_type),
            (header::CACHE_CONTROL, IMAGE_CACHE_CONTROL),
        ],
        image.bytes,
    )
        .into_response())
}

/// Whether the image proxy is allowed to fetch a URL: HTTPS on a known provider CDN
/// (Scryfall for card art / set icons, the TCGplayer CDN for sealed-product images).
/// Stored/derived image URLs all come from those providers; this guards against a bad
/// value ever turning the proxy into an SSRF.
pub(super) fn is_allowed_image_url(url: &str) -> bool {
    match reqwest::Url::parse(url) {
        Ok(parsed) => {
            parsed.scheme() == "https"
                && parsed.host_str().is_some_and(|host| {
                    host == "scryfall.io"
                        || host.ends_with(".scryfall.io")
                        || host == "tcgplayer-cdn.tcgplayer.com"
                })
        }
        Err(_) => false,
    }
}

/// Map a requested image size to a stored, allow-listed size (default `normal`).
pub(super) fn normalize_size(requested: Option<&str>) -> &'static str {
    match requested {
        Some("small") => "small",
        Some("large") => "large",
        Some("png") => "png",
        Some("art_crop") => "art_crop",
        _ => "normal",
    }
}

fn card_image_url(card: &card::Model, size: &str) -> Option<String> {
    match size {
        "small" => card.image_small.clone(),
        "large" => card.image_large.clone(),
        "png" => card.image_png.clone(),
        "art_crop" => card.image_art_crop.clone(),
        _ => card.image_normal.clone(),
    }
}

fn face_image_url(face: &StoredFace, size: &str) -> Option<String> {
    match size {
        "small" => face.image_small.clone(),
        "large" => face.image_large.clone(),
        "png" => face.image_png.clone(),
        "art_crop" => face.image_art_crop.clone(),
        _ => face.image_normal.clone(),
    }
}

//! Visual card scanner: identify a photographed card from a perceptual hash.
//!
//! `POST /api/games/{game}/scan` — the browser detects + crops a card, computes its
//! 256-bit pHash on-device (`web/src/lib/scan/phash.ts`, the byte-for-byte twin of the
//! reference hasher) and sends **only** that 32-byte fingerprint — never the image. The
//! server runs a Hamming scan against the in-memory fingerprint index
//! ([`crate::catalog::fingerprints`]) and returns the nearest catalog printings, ranked.
//! A fingerprint is a small, non-reversible vector, so the photo never leaves the device.
//!
//! Auth-gated (scanning builds a signed-in user's collection, and it keeps the per-user
//! rate limiter able to cover it) and `no-store`.

use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};

use crate::auth::extractor::AuthUser;
use crate::catalog::fingerprints;
use crate::error::AppError;
use crate::extract::{JsonBody, Path};
use crate::handlers::shared::{CardResponse, require_game};
use crate::phash::PHASH_BYTES;
use crate::state::AppState;

/// Hard cap on the requested match count, independent of the server's default.
const MAX_SCAN_TOP_K: u32 = 25;

/// Hard cap on how many fingerprints one scan may carry — the client pools a short burst
/// of frames, each with a few geometric variants (crop quality varies frame-to-frame, so
/// pooling catches a good moment). Bounds the per-request match work (index × N, still
/// a few ms).
const MAX_SCAN_FINGERPRINTS: usize = 64;

/// A scan request: the client-computed fingerprint(s) and how many matches to return.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct ScanRequest {
    /// One or more 256-bit perceptual hashes (32 bytes each): the cropped card plus a
    /// few small geometric variants (rotations / inset corrections) the client tries, so
    /// a residually-rotated or loosely-cropped scan still matches the tight, upright
    /// reference. The server keeps each card's **best** (minimum) distance across them.
    pub fingerprints: Vec<Vec<u8>>,
    /// How many ranked matches to return (clamped to `[1, 25]`); absent = the server
    /// default (`FINGERPRINT_TOP_K`).
    #[serde(default)]
    pub top_k: Option<u32>,
}

/// One ranked scan match.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct ScanMatch {
    /// The matched printing.
    pub card: CardResponse,
    /// Hamming distance (0..=256) between the query and this card's fingerprint —
    /// smaller is a closer visual match. Surfaced so the client can flag a low-confidence
    /// result or present a chooser among near-ties.
    pub distance: u32,
}

/// The scan response: ranked matches, nearest first — empty when nothing is within the
/// confidence radius (a scan of something not in the catalog).
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct ScanResponse {
    pub data: Vec<ScanMatch>,
}

/// Scan a card
///
/// `POST /api/games/{game}/scan` — identify a card from its perceptual hash.
#[utoipa::path(
    post,
    path = "/api/games/{game}/scan",
    tag = "Cards",
    security(("api_key" = [])),
    params(("game" = String, Path, description = "Game id slug, e.g. `mtg`")),
    request_body = ScanRequest,
    responses(
        (status = 200, description = "Ranked matches nearest first (empty when nothing is within the confidence radius).", body = ScanResponse),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game, or the visual scanner is not available on this instance."),
        (status = 422, description = "No fingerprints, too many, or a fingerprint of the wrong byte length."),
    ),
)]
pub async fn scan_cards(
    State(state): State<AppState>,
    AuthUser(_user): AuthUser,
    Path(game): Path<String>,
    JsonBody(payload): JsonBody<ScanRequest>,
) -> Result<Json<ScanResponse>, AppError> {
    require_game(&game)?;

    if payload.fingerprints.is_empty() || payload.fingerprints.len() > MAX_SCAN_FINGERPRINTS {
        return Err(AppError::Validation(format!(
            "send between 1 and {MAX_SCAN_FINGERPRINTS} fingerprints"
        )));
    }
    let mut queries: Vec<[u8; PHASH_BYTES]> = Vec::with_capacity(payload.fingerprints.len());
    for fp in &payload.fingerprints {
        let arr: [u8; PHASH_BYTES] = fp.as_slice().try_into().map_err(|_| {
            AppError::Validation(format!(
                "each fingerprint must be exactly {PHASH_BYTES} bytes"
            ))
        })?;
        queries.push(arr);
    }

    let index = state.fingerprint_index();
    if index.is_empty() {
        // No index built / imported yet — distinct from "matched nothing" so the client
        // can tell "scanner unavailable here" apart from "this card isn't recognised".
        return Err(AppError::NotFound(
            "the visual scanner is not available on this instance yet".to_string(),
        ));
    }

    let top_k = payload
        .top_k
        .map(|k| k.clamp(1, MAX_SCAN_TOP_K))
        .unwrap_or(state.config.fingerprint_top_k) as usize;
    let max_distance = state.config.fingerprint_max_distance;

    // The ranked nearest neighbours within the candidate radius (`FINGERPRINT_MAX_DISTANCE`);
    // beyond it a hit is more likely garbage than the card. The client shows the whole
    // ranked list as pickable candidates after a manual capture, so even a weak-but-
    // plausible match is offered (the user picks the right one) rather than dropped.
    let hits: Vec<fingerprints::ScanHit> = index
        .nearest(&game, &queries, top_k)
        .into_iter()
        .filter(|hit| hit.distance <= max_distance)
        .collect();
    if hits.is_empty() {
        return Ok(Json(ScanResponse { data: Vec::new() }));
    }

    // Dress each hit with its full card detail (one query), preserving the ranked order.
    let external_ids: Vec<String> = hits.iter().map(|hit| hit.external_id.clone()).collect();
    let cards = fingerprints::cards_by_external_id(&state.db, &game, external_ids).await?;
    let data: Vec<ScanMatch> = hits
        .into_iter()
        .filter_map(|hit| {
            cards.get(&hit.external_id).cloned().map(|card| ScanMatch {
                card: CardResponse::from(card),
                distance: hit.distance,
            })
        })
        .collect();

    Ok(Json(ScanResponse { data }))
}

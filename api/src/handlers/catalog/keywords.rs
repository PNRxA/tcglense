//! Rules-keyword glossary endpoint: what each keyword on a card actually does.
//!
//! Backs two SPA surfaces off one payload — the inline tooltips on a card's rules text
//! and the browsable `/keywords` glossary pages. The data is a curated static table
//! ([`crate::catalog::keywords`]), not a query, so this handler is a serialisation of
//! already-built rows; it sits in the public catalog route group all the same, which
//! gets it the shared `ETag` + CDN caching every other `/api/games/...` read has.

use axum::Json;

use crate::catalog::keywords::{self, KeywordEntry};
use crate::error::AppError;
use crate::extract::Path;
use crate::handlers::shared::{DataBody, require_game};

/// List rules keywords
///
/// `GET /api/games/{game}/keywords` -> every keyword ability, keyword action and
/// ability word the game defines, name-ordered, each with a plain-English explanation
/// (the official reminder text where one exists) and the `slug` its glossary page lives
/// at. A small, fully static payload that changes only with a release, so it is
/// aggressively cacheable and the SPA fetches it once per session.
///
/// A supported game with no curated glossary yet answers `200` with an empty list, not
/// an error — only an *unknown* game is a `404`.
#[utoipa::path(
    get,
    path = "/api/games/{game}/keywords",
    tag = "Cards",
    params(("game" = String, Path, description = "Game id slug, e.g. `mtg`")),
    responses(
        (status = 200, description = "The game's keyword glossary, ordered by name.", body = DataBody<Vec<KeywordEntry>>),
        (status = 404, description = "Unknown game."),
    ),
)]
pub async fn list_keywords(
    Path(game): Path<String>,
) -> Result<Json<DataBody<Vec<KeywordEntry>>>, AppError> {
    require_game(&game)?;
    Ok(Json(DataBody {
        data: keywords::glossary(&game).to_vec(),
    }))
}

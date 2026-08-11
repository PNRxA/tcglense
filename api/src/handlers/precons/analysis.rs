//! The precon browser's analysis reads: composition + draw odds, the legality verdict, the
//! estimated Commander bracket, and a seeded goldfish hand — the same four a deck page shows.
//!
//! This is the **third** mirror of `decks::analysis` (after the owner's deck and the public
//! `/u/{handle}/decks/{id}` share), and like the second it computes nothing of its own: it
//! builds a [`DeckAnalysisInput`] and hands it to the very same `analyse_*` functions, so a
//! precon page and the deck you get from "Copy to my decks" can never report different
//! verdicts for the same list.
//!
//! Three things make a precon different from a deck, and each is load-bearing:
//!
//! * **There are no `deck_sections` rows to key on.** The section ids and names are
//!   synthesised per *board*, and they must be **exactly** the ones the SPA already assigns in
//!   `web/src/lib/precons.ts` (`commander`=0 "Command zone", `main`=1 "Deck", `side`=2
//!   "Sideboard"): the stats panel builds its library checkboxes from the page's own sections
//!   but ticks them against this response's `default_library_section_ids` and echoes them back
//!   as `?sections=`. A different vocabulary here is not a type error — it is a panel that
//!   renders every box unchecked and filters on ids the server matches nothing against.
//!   [`SECTIONS`] is that mirror; a test pins it against the same table the SPA's test pins.
//!   The names are separately load-bearing: `analysis::rules` finds the command zone and the
//!   sideboard by **name**, and all three of these land in the zone their label claims.
//! * **A precon row is a single finish**, so the two rows a printing in both finishes produces
//!   are folded into one entry (the copy's `push_folded` does the same for the same reason).
//!   Unfolded they would double-count `DeckLegality.unknown_count` and give the goldfish a
//!   different slot sequence than the deck a copy produces.
//! * **Row ids are re-minted on every sync** (the tables are rebuilt wholesale), so entries are
//!   ordered by card *name* with a tie-break on the catalog's own stable `cards.id` — never on
//!   `precon_deck_cards.id` or `position`. The goldfish is a pure function of its query string;
//!   ordering on a churning id would silently re-deal every bookmarked hand each night.

use axum::{
    Json,
    extract::State,
    http::header,
    response::{IntoResponse, Response},
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};

use crate::entities::precon_deck_card::PreconBoard;
use crate::entities::prelude::{Card, PreconDeckCard};
use crate::entities::{card, precon_deck, precon_deck_card};
use crate::error::AppError;
use crate::extract::{Path, Query};
use crate::handlers::decks::DeckSectionResponse;
use crate::handlers::decks::{
    AnalysisEntry, CardFacts, DeckAnalysisInput, DeckAnalytics, DeckBracketEstimate, DeckLegality,
    GoldfishHand, GoldfishParams, StatsParams, analyse_bracket, analyse_goldfish, analyse_legality,
    analyse_stats,
};
use crate::handlers::shared::{DataBody, require_game};
use crate::state::AppState;

use super::{copy::precon_format, load_precon};

/// The synthetic section per board: `(board, id, name)`, in reading order.
///
/// **Mirrored** in `web/src/lib/precons.ts` (`PRECON_BOARDS` + `BOARD_LABEL` +
/// `boardSectionId`), with tests pinning both sides — see this module's header for why a
/// divergence is a silently broken panel rather than an error. The ids are the board's index,
/// assigned by board and not by presence, so a precon with no sideboard still gives its
/// mainboard id 1.
const SECTIONS: [(PreconBoard, i32, &str); 3] = [
    (PreconBoard::Commander, 0, "Command zone"),
    (PreconBoard::Main, 1, "Deck"),
    (PreconBoard::Side, 2, "Sideboard"),
];

fn section_for(board: &str) -> Option<(i32, &'static str)> {
    SECTIONS
        .iter()
        .find(|(b, _, _)| b.as_str() == board)
        .map(|(_, id, name)| (*id, *name))
}

/// Build a precon's analysis input, plus the catalog rows the goldfish hands back.
///
/// `models` is **positionally aligned** with `input.entries` — `analyse_goldfish` indexes one
/// by a slot it derived from the other, so a row dropped from one must be dropped from both.
async fn load_precon_analysis(
    state: &AppState,
    precon: &precon_deck::Model,
) -> Result<(DeckAnalysisInput, Vec<card::Model>), AppError> {
    // Ordered by the catalog's own stable columns, never the precon row's id (see header).
    let rows: Vec<(precon_deck_card::Model, Option<card::Model>)> = PreconDeckCard::find()
        .find_also_related(Card)
        .filter(precon_deck_card::Column::PreconDeckId.eq(precon.id))
        .order_by_asc(card::Column::Name)
        .order_by_asc(card::Column::Id)
        .all(&state.db)
        .await?;

    let mut entries: Vec<AnalysisEntry> = Vec::with_capacity(rows.len());
    let mut models: Vec<card::Model> = Vec::with_capacity(rows.len());
    let mut present: Vec<i32> = Vec::new();

    for (row, card) in rows {
        // A card whose catalog row is gone is skipped from BOTH vectors, exactly as the deck
        // loader does — the alignment above is the contract.
        let Some(model) = card else { continue };
        // A board this build doesn't know contributes nothing: an entry with no matching
        // section would be silently treated as mainboard by the rules engine.
        let Some((section_id, _)) = section_for(&row.board) else {
            continue;
        };
        let (quantity, foil_quantity) = if row.foil {
            (0, row.quantity)
        } else {
            (row.quantity, 0)
        };

        // Fold the finish pair: one printing on one board is one entry carrying both counts.
        if let Some(at) = entries
            .iter()
            .position(|e| e.section_id == section_id && e.facts.id == model.external_id)
        {
            entries[at].quantity += quantity;
            entries[at].foil_quantity += foil_quantity;
            continue;
        }
        if !present.contains(&section_id) {
            present.push(section_id);
        }
        entries.push(AnalysisEntry {
            facts: CardFacts::from(&model),
            section_id,
            quantity,
            foil_quantity,
        });
        models.push(model);
    }

    // Exactly one section per board that actually has cards, in reading order.
    let sections = SECTIONS
        .iter()
        .filter(|(_, id, _)| present.contains(id))
        .map(|(_, id, name)| DeckSectionResponse {
            id: *id,
            name: (*name).to_string(),
            position: *id,
            is_maybeboard: false,
        })
        .collect();

    Ok((DeckAnalysisInput { sections, entries }, models))
}

/// Resolve the precon and its analysis input in one step — every read here starts this way.
async fn load(
    state: &AppState,
    game: &str,
    slug: &str,
) -> Result<(precon_deck::Model, DeckAnalysisInput, Vec<card::Model>), AppError> {
    require_game(game)?;
    let precon = load_precon(state, game, slug).await?;
    let (input, models) = load_precon_analysis(state, &precon).await?;
    Ok((precon, input, models))
}

/// Preconstructed deck analytics
///
/// `GET /api/games/{game}/precons/{slug}/stats` -> the same composition + draw-odds payload a
/// deck page reads, computed by the same core over the published decklist.
#[utoipa::path(
    get,
    path = "/api/games/{game}/precons/{slug}/stats",
    tag = "Preconstructed decks",
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("slug" = String, Path, description = "Precon slug, e.g. `turtle-power-tmc`"),
        StatsParams,
    ),
    responses(
        (status = 200, description = "Composition and draw odds.", body = DeckAnalytics),
        (status = 404, description = "Unknown game or precon."),
        (status = 422, description = "Malformed section list."),
    ),
)]
pub async fn precon_stats(
    State(state): State<AppState>,
    Path((game, slug)): Path<(String, String)>,
    Query(params): Query<StatsParams>,
) -> Result<Json<DeckAnalytics>, AppError> {
    let (_, input, _) = load(&state, &game, &slug).await?;
    Ok(Json(analyse_stats(&input, &params)?))
}

/// Preconstructed deck legality
///
/// `GET /api/games/{game}/precons/{slug}/legality` -> the format verdict, or `null` for a
/// precon whose deck *type* states no format (a Jumpstart theme, an intro pack, …) — the same
/// answer the deck you copy from it would give, from the same [`precon_format`] mapping.
#[utoipa::path(
    get,
    path = "/api/games/{game}/precons/{slug}/legality",
    tag = "Preconstructed decks",
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("slug" = String, Path, description = "Precon slug, e.g. `turtle-power-tmc`"),
    ),
    responses(
        (status = 200, description = "The legality verdict, or null when the type states no format.", body = DataBody<Option<DeckLegality>>),
        (status = 404, description = "Unknown game or precon."),
    ),
)]
pub async fn precon_legality(
    State(state): State<AppState>,
    Path((game, slug)): Path<(String, String)>,
) -> Result<Json<DataBody<Option<DeckLegality>>>, AppError> {
    let (precon, input, _) = load(&state, &game, &slug).await?;
    let format = precon_format(&precon.deck_type);
    // `DataBody` because the answer is nullable, exactly as the deck and public mirrors wrap
    // theirs — the SPA reads all three through the same hooks.
    Ok(Json(DataBody {
        data: analyse_legality(format.as_deref(), &input),
    }))
}

/// Preconstructed deck bracket estimate
///
/// `GET /api/games/{game}/precons/{slug}/bracket` -> the estimated Commander bracket, or
/// `null` for every precon that isn't a Commander deck (the ladder is only defined there).
#[utoipa::path(
    get,
    path = "/api/games/{game}/precons/{slug}/bracket",
    tag = "Preconstructed decks",
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("slug" = String, Path, description = "Precon slug, e.g. `turtle-power-tmc`"),
    ),
    responses(
        (status = 200, description = "The bracket estimate, or null outside Commander.", body = DataBody<Option<DeckBracketEstimate>>),
        (status = 404, description = "Unknown game or precon."),
    ),
)]
pub async fn precon_bracket(
    State(state): State<AppState>,
    Path((game, slug)): Path<(String, String)>,
) -> Result<Json<DataBody<Option<DeckBracketEstimate>>>, AppError> {
    let (precon, input, _) = load(&state, &game, &slug).await?;
    let format = precon_format(&precon.deck_type);
    Ok(Json(DataBody {
        data: analyse_bracket(format.as_deref(), &input),
    }))
}

/// Preconstructed deck sample hand
///
/// `GET /api/games/{game}/precons/{slug}/goldfish` -> a seeded opening hand, a pure function of
/// its query string exactly as the deck goldfish is.
///
/// **A seedless request answers `no-store`.** Without a `seed` the roll is random, so the
/// response is not a function of its URL — and these routes sit in the public *catalog* cache
/// group (`s-maxage=3600` + a day of `stale-while-revalidate`), so a shared cache would
/// otherwise pin one anonymous visitor's hand as *the* hand for the best part of a day. Same
/// rule, and the same reason, as the public deck mirror's.
#[utoipa::path(
    get,
    path = "/api/games/{game}/precons/{slug}/goldfish",
    tag = "Preconstructed decks",
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("slug" = String, Path, description = "Precon slug, e.g. `turtle-power-tmc`"),
        GoldfishParams,
    ),
    responses(
        (status = 200, description = "A seeded sample hand.", body = GoldfishHand),
        (status = 404, description = "Unknown game or precon."),
        (status = 422, description = "Malformed parameters or an oversized library."),
    ),
)]
pub async fn precon_goldfish(
    State(state): State<AppState>,
    Path((game, slug)): Path<(String, String)>,
    Query(params): Query<GoldfishParams>,
) -> Result<Response, AppError> {
    let seedless = params.seed.is_none();
    let (_, input, models) = load(&state, &game, &slug).await?;
    let hand = analyse_goldfish(&input, &models, &params)?;

    let mut response = Json(hand).into_response();
    if seedless {
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            header::HeaderValue::from_static("no-store"),
        );
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The SPA mints these ids and names itself (`web/src/lib/precons.ts`) and the two must
    /// agree exactly, or the stats panel's library checkboxes address sections the server never
    /// returns. Pinned here and there, like `lifeLayout.ts`'s vocabulary.
    #[test]
    fn section_vocabulary_matches_the_spa() {
        assert_eq!(
            SECTIONS
                .iter()
                .map(|(b, id, name)| (b.as_str(), *id, *name))
                .collect::<Vec<_>>(),
            vec![
                ("commander", 0, "Command zone"),
                ("main", 1, "Deck"),
                ("side", 2, "Sideboard"),
            ]
        );
    }

    /// The names are what `analysis::rules` reads a deck's zones off — not the ids, and not the
    /// board strings. A rename that still compiles would make every Commander precon report
    /// "no commander" and drag its sideboard into the deck.
    #[test]
    fn section_names_land_in_the_zones_they_claim() {
        use crate::handlers::decks::{DeckZone, deck_zone};
        let zones: Vec<DeckZone> = SECTIONS
            .iter()
            .map(|(_, _, name)| deck_zone(name))
            .collect();
        assert_eq!(
            zones,
            vec![DeckZone::Command, DeckZone::Main, DeckZone::Sideboard],
            "the synthesised names must land in the zones their labels claim"
        );
    }
}

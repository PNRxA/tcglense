//! The format vocabulary the legality verdict is keyed by, and the free-text → format-key
//! normaliser that bridges a deck's user-typed `format` label to it.
//!
//! `deck.format` is free text (the user picked from a select or typed their own), while
//! `cards.legalities` is Scryfall's per-format object keyed by slug. Everything that judges
//! a deck has to cross that gap first, so the table and the normaliser live here, below the
//! rules and the per-card check that both consume them.
//!
//! **Mirrored vocabulary.** `web/src/lib/legality.ts` holds the same table, because the
//! SPA's format select and the card page's legality panel are pure rendering and must not
//! wait on a request to draw a dropdown. Both sides carry a test pinning the list, so a
//! key added to one alone fails that side's test rather than silently disagreeing — the
//! arrangement `web/src/lib/lifeLayout.ts` already uses for the life counter's layouts.
//! `GET /api/games/{game}/formats` publishes the server's copy so a CLI can complete and
//! validate `--format` without hard-coding it.

use axum::{Json, extract::State};
use serde::Serialize;

use crate::error::AppError;
use crate::extract::Path;
use crate::handlers::shared::{DataBody, require_game};
use crate::state::AppState;

/// Select-menu grouping for a format. Ordering within the table is the display order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum DeckFormatGroup {
    Constructed,
    Commander,
    Arena,
    Other,
}

/// One legality-tracked format: its Scryfall key, how it's spelled to a human, the
/// grouping a select renders it under, the extra spellings [`normalize_format_key`]
/// accepts, and whether it is one of the six most-played (what the card page's legality
/// panel shows before its "show all" expansion).
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckFormat {
    /// The key used in a card's `legalities` object (Scryfall's format slug).
    pub key: String,
    /// Display label; also the string stored in `deck.format` when picked from the select.
    pub label: String,
    pub group: DeckFormatGroup,
    /// Extra spellings accepted when normalising a free-text format label.
    pub aliases: Vec<String>,
    pub popular: bool,
}

/// The static table behind [`DeckFormat`], without the per-request allocation.
struct Format {
    key: &'static str,
    label: &'static str,
    group: DeckFormatGroup,
    aliases: &'static [&'static str],
    popular: bool,
}

const fn f(
    key: &'static str,
    label: &'static str,
    group: DeckFormatGroup,
    aliases: &'static [&'static str],
    popular: bool,
) -> Format {
    Format {
        key,
        label,
        group,
        aliases,
        popular,
    }
}

use DeckFormatGroup::{Arena, Commander, Constructed, Other};

/// Every format legality is tracked for, in display order. `future` and `tlr` exist in the
/// provider data and are deliberately absent (meaningless to a deck builder).
const MTG_FORMATS: &[Format] = &[
    f("standard", "Standard", Constructed, &[], true),
    f("pioneer", "Pioneer", Constructed, &[], true),
    f("modern", "Modern", Constructed, &[], true),
    f("legacy", "Legacy", Constructed, &[], true),
    f("vintage", "Vintage", Constructed, &[], false),
    f("pauper", "Pauper", Constructed, &[], true),
    f("commander", "Commander", Commander, &["edh", "cedh"], true),
    f("oathbreaker", "Oathbreaker", Commander, &[], false),
    f(
        "paupercommander",
        "Pauper Commander",
        Commander,
        &["pdh", "pauperedh"],
        false,
    ),
    f(
        "duel",
        "Duel Commander",
        Commander,
        &["duelcommander", "frenchcommander"],
        false,
    ),
    f("predh", "PreDH", Commander, &["preedh"], false),
    f("alchemy", "Alchemy", Arena, &[], false),
    f("historic", "Historic", Arena, &[], false),
    f("timeless", "Timeless", Arena, &[], false),
    f("gladiator", "Gladiator", Arena, &[], false),
    f("brawl", "Brawl", Arena, &["historicbrawl"], false),
    f("standardbrawl", "Standard Brawl", Arena, &[], false),
    f(
        "competitivebrawl",
        "Competitive Brawl",
        Arena,
        &["compbrawl"],
        false,
    ),
    f("penny", "Penny Dreadful", Other, &[], false),
    f(
        "oldschool",
        "Old School",
        Other,
        &["oldschool9394", "9394"],
        false,
    ),
    f("premodern", "Premodern", Other, &[], false),
];

/// Lowercase and strip everything but letters and digits, so `"Comp. Brawl"` and
/// `"compbrawl"` are the same spelling and `"  E.D.H. "` reaches `commander`.
fn canon(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Map a free-form deck format label to a legality key, or `None` when it isn't a
/// legality-tracked format. `None` means "don't evaluate legality" — never "illegal".
pub(crate) fn normalize_format_key(text: Option<&str>) -> Option<&'static str> {
    let wanted = canon(text?);
    if wanted.is_empty() {
        return None;
    }
    MTG_FORMATS
        .iter()
        .find(|format| {
            canon(format.key) == wanted
                || canon(format.label) == wanted
                || format.aliases.iter().any(|alias| canon(alias) == wanted)
        })
        .map(|format| format.key)
}

/// Display label for a legality key, falling back to the key itself.
pub(crate) fn format_label(key: &str) -> String {
    MTG_FORMATS
        .iter()
        .find(|format| format.key == key)
        .map(|format| format.label.to_string())
        .unwrap_or_else(|| key.to_string())
}

fn format_table() -> Vec<DeckFormat> {
    MTG_FORMATS
        .iter()
        .map(|format| DeckFormat {
            key: format.key.to_string(),
            label: format.label.to_string(),
            group: format.group,
            aliases: format.aliases.iter().map(|a| (*a).to_string()).collect(),
            popular: format.popular,
        })
        .collect()
}

/// List deck formats
///
/// `GET /api/games/{game}/formats` -> every format the game tracks deck legality for, in
/// display order, each with the spellings `deck.format` accepts. Lets a client validate or
/// complete a format without hard-coding the list. Public and cacheable; a game with no
/// legality data returns an empty list. Returns `{ data: DeckFormat[] }`.
#[utoipa::path(
    get,
    path = "/api/games/{game}/formats",
    tag = "Decks",
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
    ),
    responses(
        (status = 200, description = "The game's legality-tracked deck formats, in display order.", body = DataBody<Vec<DeckFormat>>),
        (status = 404, description = "Unknown game."),
    ),
)]
pub async fn list_deck_formats(
    State(_state): State<AppState>,
    Path(game): Path<String>,
) -> Result<Json<DataBody<Vec<DeckFormat>>>, AppError> {
    require_game(&game)?;
    let data = if game == crate::scryfall::GAME {
        format_table()
    } else {
        Vec::new()
    };
    Ok(Json(DataBody { data }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The vocabulary, pinned. `web/src/lib/__tests__/legality.spec.ts` pins the same list
    /// on the SPA side; a key added to one alone fails there, which is the whole point of
    /// having both lists written down.
    #[test]
    fn format_vocabulary_is_pinned() {
        let keys: Vec<&str> = MTG_FORMATS.iter().map(|f| f.key).collect();
        assert_eq!(
            keys,
            vec![
                "standard",
                "pioneer",
                "modern",
                "legacy",
                "vintage",
                "pauper",
                "commander",
                "oathbreaker",
                "paupercommander",
                "duel",
                "predh",
                "alchemy",
                "historic",
                "timeless",
                "gladiator",
                "brawl",
                "standardbrawl",
                "competitivebrawl",
                "penny",
                "oldschool",
                "premodern",
            ]
        );
        let popular: Vec<&str> = MTG_FORMATS
            .iter()
            .filter(|f| f.popular)
            .map(|f| f.key)
            .collect();
        assert_eq!(
            popular,
            vec![
                "standard",
                "pioneer",
                "modern",
                "legacy",
                "pauper",
                "commander"
            ],
            "the card page's legality panel shows exactly six before expanding"
        );
    }

    #[test]
    fn every_key_and_label_normalises_to_itself() {
        for format in MTG_FORMATS {
            assert_eq!(normalize_format_key(Some(format.key)), Some(format.key));
            assert_eq!(normalize_format_key(Some(format.label)), Some(format.key));
            for alias in format.aliases {
                assert_eq!(normalize_format_key(Some(alias)), Some(format.key));
            }
        }
    }

    #[test]
    fn accepts_the_spellings_users_type() {
        assert_eq!(normalize_format_key(Some("EDH")), Some("commander"));
        assert_eq!(normalize_format_key(Some("cEDH")), Some("commander"));
        assert_eq!(normalize_format_key(Some("  E.D.H. ")), Some("commander"));
        assert_eq!(
            normalize_format_key(Some("Pauper EDH")),
            Some("paupercommander")
        );
        assert_eq!(normalize_format_key(Some("PDH")), Some("paupercommander"));
        assert_eq!(
            normalize_format_key(Some("Comp. Brawl")),
            Some("competitivebrawl")
        );
        assert_eq!(normalize_format_key(Some("Historic Brawl")), Some("brawl"));
        assert_eq!(normalize_format_key(Some("Duel Commander")), Some("duel"));
        assert_eq!(normalize_format_key(Some("PreDH")), Some("predh"));
    }

    #[test]
    fn untracked_formats_are_none_not_illegal() {
        for text in ["Cube", "Limited", "Casual", "kitchen table", "", "   "] {
            assert_eq!(normalize_format_key(Some(text)), None, "{text}");
        }
        assert_eq!(normalize_format_key(None), None);
    }

    #[test]
    fn labels_fall_back_to_the_key() {
        assert_eq!(format_label("commander"), "Commander");
        assert_eq!(format_label("standardbrawl"), "Standard Brawl");
        assert_eq!(format_label("something-else"), "something-else");
    }
}

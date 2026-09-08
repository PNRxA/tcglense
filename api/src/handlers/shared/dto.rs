//! Shared card response DTOs: the public card payload (`CardResponse` + its faces
//! and prices) reused by both the catalog and collection endpoints, the detail-only
//! `CardDetailResponse` the single-card route wraps it in (issue #673), plus the two
//! small `card::Model` accessors they're built from.
//!
//! The wire DTOs here (and in the other handler modules) carry a test-only
//! `ts_rs::TS` derive: `cargo test` exports each one as a TypeScript type into
//! `web/src/lib/api/generated/` (committed; CI checks for drift), so the SPA's
//! API types are generated from these structs rather than hand-mirrored. The
//! `ts(rename)`s pin the names the web code already uses.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::entities::card;
use crate::scryfall::model::StoredFace;

#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "CardPrices"))]
pub(crate) struct PricesResponse {
    pub usd: Option<String>,
    pub usd_foil: Option<String>,
    pub eur: Option<String>,
    pub tix: Option<String>,
}

#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "CardFace"))]
pub(crate) struct CardFaceResponse {
    pub name: Option<String>,
    pub mana_cost: Option<String>,
    pub type_line: Option<String>,
    pub oracle_text: Option<String>,
    pub power: Option<String>,
    pub toughness: Option<String>,
    pub loyalty: Option<String>,
}

/// A single printing of a card, as the SPA sees it.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "Card"))]
pub(crate) struct CardResponse {
    pub id: String,
    pub name: String,
    pub set_code: String,
    pub set_name: String,
    pub collector_number: String,
    pub rarity: Option<String>,
    pub lang: String,
    pub released_at: Option<String>,
    pub mana_cost: Option<String>,
    pub cmc: Option<f64>,
    pub type_line: Option<String>,
    pub oracle_text: Option<String>,
    pub power: Option<String>,
    pub toughness: Option<String>,
    pub loyalty: Option<String>,
    pub color_identity: Vec<String>,
    pub colors: Vec<String>,
    pub layout: Option<String>,
    pub prices: PricesResponse,
    /// Whether an image is available through the image proxy for this card.
    pub has_image: bool,
    /// The Secret Lair drop this card belongs to (its curated title), for sets
    /// broken into drops; `None` for everything else.
    pub drop_name: Option<String>,
    /// Stable slug of the drop above (anchors/links), paired with `drop_name`.
    pub drop_slug: Option<String>,
    /// What the group named above *is* — `"drop"` for a Secret Lair drop, `"treatment"` for
    /// one of The Zeta Set's print-treatment sections — paired with `drop_name`, so the card
    /// page heads the row truthfully (the set's `drop_noun`).
    pub drop_noun: Option<String>,
    /// Whether this printing is a Secret Lair **chase / bonus** card — the optional
    /// card handed out with a qualifying drop purchase (Scryfall's `sldbonus` promo
    /// type). These have no sealed product of their own, so the card page has nothing
    /// in its "found in" section; the flag lets the SPA mark the card as a chase card
    /// and link it to its drop instead (issue #295).
    pub secret_lair_bonus: bool,
    /// Whether this printing is a Secret Lair **spend-incentive** promo — the card handed
    /// out for reaching a cart spend threshold during a superdrop (e.g. the Avatar foil
    /// Path of Ancestry, one per $199 spent), rather than included with a specific drop.
    /// Scryfall tags these `sldbonus` like the per-drop bonus cards above, so the flag comes
    /// from a curated list (see [`crate::scryfall::drops::is_spend_incentive`]); it lets the
    /// SPA mark them as spend rewards instead of ordinary chase cards (issue #331).
    pub secret_lair_spend_incentive: bool,
    /// Present for multi-faced cards; request face images via `?face=N`.
    pub faces: Vec<CardFaceResponse>,
    /// Per-format legality, parsed from the stored Scryfall object: format key
    /// (`"modern"`, `"commander"`, …) -> `"legal" | "not_legal" | "banned" |
    /// "restricted"`. `None` when the catalog row has no legality data (issue #557).
    pub legalities: Option<BTreeMap<String, String>>,
}

/// The single-card route's payload (`GET /api/games/{game}/cards/{id}`, issue #673): the
/// shared [`CardResponse`] every listing carries, **flattened**, plus the print + collector
/// details only the card page reads — who painted it, its flavour text, the finishes it
/// comes in, frame/border/stamp/promo facts, the Reserved List flag, its EDHREC / Penny
/// rank, the mana it produces, and a Battle's printed defense.
///
/// Detail-only on purpose: `CardResponse` rides every list payload (a grid page is up to 200
/// of them, CDN/ETag-cached), so these ~20 columns stay off it and live here, on the one
/// route that answers for a single card. Nothing here is derived — every field is the
/// catalog column as stored (`defense` is the printed box, rendered like P/T), so the
/// response stays a pure function of its URL and the public cache rules are unchanged.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "CardDetail"))]
pub(crate) struct CardDetailResponse {
    #[serde(flatten)]
    #[schema(inline)]
    pub card: CardResponse,
    /// The illustrator credited on the printing (`a:` searches it); a multi-artist card
    /// carries every name in one string, as printed.
    pub artist: Option<String>,
    /// Scryfall's stable ids for the artist(s) above, one per artist.
    pub artist_ids: Vec<String>,
    /// The artwork's id — every printing of the same painting shares it (the key the art
    /// tags and a future "other printings of this artwork" read join on).
    pub illustration_id: Option<String>,
    /// The printed flavour text; a multi-faced card's faces are joined by `\n//\n`.
    pub flavor_text: Option<String>,
    /// The printed watermark (a guild or faction mark, a Universes Beyond logo), if any.
    pub watermark: Option<String>,
    /// The finishes this printing exists in — `nonfoil` / `foil` / `etched`.
    pub finishes: Vec<String>,
    /// The frame layout (`1993`, `2003`, `2015`, `future`, …).
    pub frame: Option<String>,
    /// Frame effects on the printing (`showcase`, `extendedart`, `legendary`, …).
    pub frame_effects: Vec<String>,
    /// `black` / `white` / `silver` / `gold` / `borderless`.
    pub border_color: Option<String>,
    /// The holofoil security stamp (`oval`, `triangle`, `acorn`, `arena`, …), if any.
    pub security_stamp: Option<String>,
    /// Scryfall's promo-type tags (`prerelease`, `buyabox`, `sldbonus`, …).
    pub promo_types: Vec<String>,
    /// The colours of mana this card can produce.
    pub produced_mana: Vec<String>,
    /// A Battle's printed defense box, exactly as stored (the `def:` filter's column).
    pub defense: Option<String>,
    /// On the Reserved List — never to be reprinted (`is:reserved`).
    pub reserved: bool,
    pub full_art: bool,
    pub textless: bool,
    pub promo: bool,
    /// A variation of another printing in the same set (alternate art, a different frame).
    pub variation: bool,
    /// A Story Spotlight card (the planeswalker-symbol frame stamp).
    pub story_spotlight: bool,
    /// Carries Wizards' content warning (a printing the publisher has disavowed).
    pub content_warning: bool,
    /// Popularity rank on EDHREC (lower is more played); `null` when unranked.
    pub edhrec_rank: Option<i32>,
    /// Popularity rank in Penny Dreadful; `null` when unranked.
    pub penny_rank: Option<i32>,
}

impl From<card::Model> for CardDetailResponse {
    fn from(m: card::Model) -> Self {
        // Move the detail columns out first — `CardResponse::from` consumes the row, and
        // none of these are read by it.
        let artist = m.artist.clone();
        let artist_ids = split_csv(m.artist_ids.clone());
        let illustration_id = m.illustration_id.clone();
        let flavor_text = m.flavor_text.clone();
        let watermark = m.watermark.clone();
        let finishes = split_csv(m.finishes.clone());
        let frame = m.frame.clone();
        let frame_effects = split_csv(m.frame_effects.clone());
        let border_color = m.border_color.clone();
        let security_stamp = m.security_stamp.clone();
        let promo_types = split_csv(m.promo_types.clone());
        let produced_mana = split_csv(m.produced_mana.clone());
        let defense = m.defense.clone();
        // The flags are provider booleans (Scryfall sends every one on every card), so a
        // NULL only ever means a row the sync hasn't rewritten yet — read as `false`.
        let reserved = m.reserved.unwrap_or(false);
        let full_art = m.full_art.unwrap_or(false);
        let textless = m.textless.unwrap_or(false);
        let promo = m.promo.unwrap_or(false);
        let variation = m.variation.unwrap_or(false);
        let story_spotlight = m.story_spotlight.unwrap_or(false);
        let content_warning = m.content_warning.unwrap_or(false);
        let edhrec_rank = m.edhrec_rank;
        let penny_rank = m.penny_rank;

        CardDetailResponse {
            card: CardResponse::from(m),
            artist,
            artist_ids,
            illustration_id,
            flavor_text,
            watermark,
            finishes,
            frame,
            frame_effects,
            border_color,
            security_stamp,
            promo_types,
            produced_mana,
            defense,
            reserved,
            full_art,
            textless,
            promo,
            variation,
            story_spotlight,
            content_warning,
            edhrec_rank,
            penny_rank,
        }
    }
}

impl From<card::Model> for CardResponse {
    fn from(m: card::Model) -> Self {
        // `drop_for` returns an owned `Drop` (the store is swapped by the daily refresh, so
        // there's no `'static` table to borrow from); move its title/slug out — no clone.
        let (drop_name, drop_slug) =
            crate::scryfall::drops::drop_for(&m.game, &m.set_code, &m.collector_number)
                .map(|d| (d.title, d.slug))
                .unzip();
        // The noun rides only beside a drop: a set that is drop-grouped but doesn't list this
        // number (a newer-than-snapshot printing, "Other" in the grouped view) names no group.
        let drop_noun = drop_name
            .as_ref()
            .and_then(|_| crate::scryfall::drops::section_noun(&m.game, &m.set_code))
            .map(str::to_string);
        let secret_lair_bonus = is_secret_lair_bonus(m.promo_types.as_deref());
        let secret_lair_spend_incentive =
            crate::scryfall::drops::is_spend_incentive(&m.game, &m.set_code, &m.collector_number);

        let legalities = parse_legalities(m.legalities.as_deref());

        let stored_faces = stored_faces(&m);

        let has_image = m.image_normal.is_some()
            || m.image_small.is_some()
            || m.image_large.is_some()
            || stored_faces
                .iter()
                .any(|f| f.image_normal.is_some() || f.image_small.is_some());

        let faces = stored_faces
            .into_iter()
            .map(|f| CardFaceResponse {
                name: f.name,
                mana_cost: f.mana_cost,
                type_line: f.type_line,
                oracle_text: f.oracle_text,
                power: f.power,
                toughness: f.toughness,
                loyalty: f.loyalty,
            })
            .collect();

        CardResponse {
            id: m.external_id,
            name: m.name,
            set_code: m.set_code,
            set_name: m.set_name,
            collector_number: m.collector_number,
            rarity: m.rarity,
            lang: m.lang,
            released_at: m.released_at,
            mana_cost: m.mana_cost,
            cmc: m.cmc,
            type_line: m.type_line,
            oracle_text: m.oracle_text,
            power: m.power,
            toughness: m.toughness,
            loyalty: m.loyalty,
            color_identity: split_csv(m.color_identity),
            colors: split_csv(m.colors),
            layout: m.layout,
            prices: PricesResponse {
                usd: m.price_usd,
                usd_foil: m.price_usd_foil,
                eur: m.price_eur,
                tix: m.price_tix,
            },
            has_image,
            drop_name,
            drop_slug,
            drop_noun,
            secret_lair_bonus,
            secret_lair_spend_incentive,
            faces,
            legalities,
        }
    }
}

/// Whether a card's comma-joined `promo_types` marks it as a Secret Lair chase/bonus
/// card. `sldbonus` is Scryfall's explicit tag for the optional card given with a
/// qualifying Secret Lair purchase, so this is an exact-token match, not a substring
/// one (`"sldbonus"` must be a whole entry, never part of another tag).
pub(crate) fn is_secret_lair_bonus(promo_types: Option<&str>) -> bool {
    promo_types.is_some_and(|types| types.split(',').any(|t| t == "sldbonus"))
}

pub(crate) fn stored_faces(card: &card::Model) -> Vec<StoredFace> {
    card.card_faces
        .as_deref()
        .and_then(|json| serde_json::from_str(json).ok())
        .unwrap_or_default()
}

/// Parse the stored per-format legality object (`cards.legalities`, provider-written
/// JSON) into the wire map. Tolerant by design: a missing column, non-JSON text, a
/// non-object, or non-string values all yield `None` rather than failing the request.
pub(crate) fn parse_legalities(raw: Option<&str>) -> Option<BTreeMap<String, String>> {
    raw.and_then(|json| serde_json::from_str(json).ok())
}

pub(crate) fn split_csv(value: Option<String>) -> Vec<String> {
    value
        .map(|v| {
            v.split(',')
                .filter(|part| !part.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_csv_handles_empty() {
        assert_eq!(split_csv(None), Vec::<String>::new());
        assert_eq!(split_csv(Some(String::new())), Vec::<String>::new());
        assert_eq!(split_csv(Some("W,U".to_string())), vec!["W", "U"]);
    }

    #[test]
    fn detects_secret_lair_bonus() {
        // Absent / empty / unrelated promo types are not chase cards.
        assert!(!is_secret_lair_bonus(None));
        assert!(!is_secret_lair_bonus(Some("")));
        assert!(!is_secret_lair_bonus(Some("buyabox,prerelease")));
        // The tag anywhere in the comma-joined list marks a chase card.
        assert!(is_secret_lair_bonus(Some("sldbonus")));
        assert!(is_secret_lair_bonus(Some("sldbonus,universesbeyond")));
        assert!(is_secret_lair_bonus(Some("ffx,sldbonus,universesbeyond")));
        // Exact-token match: a tag that merely contains the substring must not match.
        assert!(!is_secret_lair_bonus(Some("notsldbonus")));
    }

    /// The detail wrapper is a pure projection of the row: the CSV columns split into
    /// arrays, a NULL provider boolean reads as `false`, and the shared `CardResponse`
    /// it wraps is unchanged (it is what every listing still answers).
    #[test]
    fn card_detail_splits_csv_columns_and_reads_null_flags_as_false() {
        let row = card::Model {
            artist: Some("Rebecca Guay".into()),
            artist_ids: Some("artist-a,artist-b".into()),
            flavor_text: Some("The siege breaks at dawn.".into()),
            finishes: Some("nonfoil,foil".into()),
            frame_effects: Some("showcase,legendary".into()),
            defense: Some("5".into()),
            reserved: Some(true),
            edhrec_rank: Some(1234),
            // `full_art` and the rest stay NULL, the shape of a row the sync predates.
            ..crate::test_support::card_model(1)
        };
        let detail = CardDetailResponse::from(row.clone());

        assert_eq!(detail.artist.as_deref(), Some("Rebecca Guay"));
        assert_eq!(detail.artist_ids, ["artist-a", "artist-b"]);
        assert_eq!(detail.finishes, ["nonfoil", "foil"]);
        assert_eq!(detail.frame_effects, ["showcase", "legendary"]);
        assert_eq!(detail.defense.as_deref(), Some("5"));
        assert!(detail.reserved);
        assert_eq!(detail.edhrec_rank, Some(1234));
        // NULL columns: empty arrays (never `None`-ish) and `false` flags, no ranks.
        assert!(detail.promo_types.is_empty());
        assert!(detail.produced_mana.is_empty());
        assert!(!detail.full_art);
        assert!(!detail.content_warning);
        assert_eq!(detail.penny_rank, None);

        // The wrapped payload is exactly the shared `Card` the listings answer.
        let plain = CardResponse::from(row);
        assert_eq!(detail.card.id, plain.id);
        assert_eq!(detail.card.name, plain.name);
        assert_eq!(detail.card.set_code, plain.set_code);
    }

    #[test]
    fn parse_legalities_is_tolerant() {
        // The happy path: the stored Scryfall object becomes the wire map.
        let parsed = parse_legalities(Some(r#"{"modern":"banned","vintage":"restricted"}"#))
            .expect("valid object parses");
        assert_eq!(parsed["modern"], "banned");
        assert_eq!(parsed["vintage"], "restricted");
        // Anything malformed degrades to None instead of failing the request.
        assert_eq!(parse_legalities(None), None);
        assert_eq!(parse_legalities(Some("")), None);
        assert_eq!(parse_legalities(Some("not json")), None);
        assert_eq!(parse_legalities(Some(r#"["legal"]"#)), None);
        assert_eq!(parse_legalities(Some(r#"{"modern":1}"#)), None);
    }
}

//! Mapping from Scryfall's card/set JSON shapes into our SeaORM `ActiveModel`s.
//! Kept separate from the streaming import (`ingest`) so the pure, side-effect-free
//! shaping — and its unit tests — stand on their own.

use sea_orm::{
    ActiveValue::{NotSet, Set},
    prelude::DateTimeUtc,
};

use super::GAME;
use super::model::{CardFace, RelatedCard, ScryfallCard, ScryfallSet, StoredFace, StoredPart};
use crate::entities::{card, card_set};

pub(super) fn map_set(set: &ScryfallSet, now: DateTimeUtc) -> card_set::ActiveModel {
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

    let (price_usd, price_usd_foil, price_usd_etched, price_eur, price_tix) = match &card.prices {
        Some(p) => (
            p.usd.clone(),
            p.usd_foil.clone(),
            p.usd_etched.clone(),
            p.eur.clone(),
            p.tix.clone(),
        ),
        None => (None, None, None, None, None),
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
    let power = card
        .power
        .clone()
        .or_else(|| face_stat(&card.card_faces, |f| &f.power));
    let toughness = card
        .toughness
        .clone()
        .or_else(|| face_stat(&card.card_faces, |f| &f.toughness));
    let loyalty = card
        .loyalty
        .clone()
        .or_else(|| face_stat(&card.card_faces, |f| &f.loyalty));

    // Comma-joined array columns (same shape as colours).
    let keywords = join_colors(&card.keywords);
    let produced_mana = join_colors(&card.produced_mana);
    let artist_ids = join_colors(&card.artist_ids);
    let frame_effects = join_colors(&card.frame_effects);
    let promo_types = join_colors(&card.promo_types);
    let finishes = join_colors(&card.finishes);

    // Per-face fallbacks: use the top-level value, else the first face that has one
    // (mirrors the power/toughness/loyalty handling above).
    let watermark = card
        .watermark
        .clone()
        .or_else(|| face_stat(&card.card_faces, |f| &f.watermark));
    let illustration_id = card
        .illustration_id
        .clone()
        .or_else(|| face_stat(&card.card_faces, |f| &f.illustration_id));
    let defense = card
        .defense
        .clone()
        .or_else(|| face_stat(&card.card_faces, |f| &f.defense));
    // Flavour text joins the faces like oracle_text, so `ft:` matches either face.
    let flavor_text = card.flavor_text.clone().or_else(|| {
        card.card_faces.as_ref().and_then(|faces| {
            let joined = faces
                .iter()
                .filter_map(|f| f.flavor_text.as_deref())
                .filter(|t| !t.is_empty())
                .collect::<Vec<_>>()
                .join("\n//\n");
            (!joined.is_empty()).then_some(joined)
        })
    });
    // Colour indicator: top-level, else the first face that carries one.
    let color_indicator = join_colors(&card.color_indicator).or_else(|| {
        card.card_faces.as_ref().and_then(|faces| {
            faces.iter().find_map(|f| {
                f.color_indicator
                    .as_ref()
                    .filter(|v| !v.is_empty())
                    .map(|v| v.join(","))
            })
        })
    });
    // Legalities object stored verbatim as JSON (queried via json_extract).
    let legalities = card
        .legalities
        .as_ref()
        .and_then(|v| serde_json::to_string(v).ok());
    // The tokens and emblems this printing makes — an empty array when it makes none, and
    // NULL only for a row written before this column existed (see `token_parts`).
    let token_parts = token_parts(&card.id, card.all_parts.as_deref());

    card::ActiveModel {
        // Derived by `super::foil_variants::refresh_foil_variant_folds`, never by the provider:
        // left untouched so the upsert (which also excludes it) can't clobber a fold.
        folded_onto_id: NotSet,
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
        token_parts: Set(token_parts),
        price_usd: Set(price_usd),
        price_usd_foil: Set(price_usd_foil),
        price_usd_etched: Set(price_usd_etched),
        price_eur: Set(price_eur),
        price_tix: Set(price_tix),
        tcgplayer_id: Set(card.tcgplayer_id),
        tcgplayer_etched_id: Set(card.tcgplayer_etched_id),
        keywords: Set(keywords),
        produced_mana: Set(produced_mana),
        color_indicator: Set(color_indicator),
        watermark: Set(watermark),
        flavor_text: Set(flavor_text),
        illustration_id: Set(illustration_id),
        artist: Set(card.artist),
        artist_ids: Set(artist_ids),
        border_color: Set(card.border_color),
        frame: Set(card.frame),
        frame_effects: Set(frame_effects),
        security_stamp: Set(card.security_stamp),
        promo_types: Set(promo_types),
        finishes: Set(finishes),
        defense: Set(defense),
        legalities: Set(legalities),
        full_art: Set(card.full_art),
        textless: Set(card.textless),
        oversized: Set(card.oversized),
        promo: Set(card.promo),
        reprint: Set(card.reprint),
        variation: Set(card.variation),
        booster: Set(card.booster),
        story_spotlight: Set(card.story_spotlight),
        content_warning: Set(card.content_warning),
        highres_image: Set(card.highres_image),
        reserved: Set(card.reserved),
        game_changer: Set(card.game_changer),
        edhrec_rank: Set(card.edhrec_rank),
        penny_rank: Set(card.penny_rank),
        digital: Set(card.digital.unwrap_or(false)),
        created_at: Set(now),
        updated_at: Set(now),
    }
}

/// The tokens and emblems a printing makes, as the JSON stored in `cards.token_parts`.
///
/// Filtered out of Scryfall's `all_parts` (see [`RelatedCard`]) rather than stored whole,
/// because most of that list is not a token: it always carries the card **itself**, and it
/// carries meld halves and combo pieces, all of which are real cards a deck would hold as
/// cards. Two rules keep exactly the pieces a player has to bring *besides* their deck:
///
/// * `component == "token"` — the provider's own answer, and the only one for tokens.
/// * an **emblem**, which Scryfall files as a `combo_piece` and distinguishes only by its
///   printed type line (`"Emblem — Elspeth"`). Reading the type line here is the same move
///   `CardFacts` makes for the card's own types: it's the provider's printed datum, not a
///   guess about rules text.
///
/// **A card that makes nothing stores `[]`, never NULL.** The two have to be different: NULL
/// is a row written before this column existed, which is every row until the next bulk
/// import lands, and answering "this deck makes no tokens" because the catalog hasn't been
/// re-imported yet would be a wrong answer rather than a missing one. `analyse_tokens`
/// reports the NULL rows as unchecked and the empty ones as settled.
pub(super) fn token_parts(card_id: &str, all_parts: Option<&[RelatedCard]>) -> Option<String> {
    let mut parts: Vec<StoredPart> = Vec::new();
    for part in all_parts.unwrap_or_default() {
        // The self entry (`combo_piece`, or `meld_part` on a meld card) is not a token this
        // card makes, and a token card's own entry would otherwise recommend itself.
        if part.id == card_id || parts.iter().any(|kept| kept.id == part.id) {
            continue;
        }
        if !is_token_component(part) {
            continue;
        }
        // A nameless entry has nothing to render and nothing to group by; the id alone
        // resolves only if that printing happens to be in the catalog.
        let Some(name) = part.name.clone().filter(|n| !n.is_empty()) else {
            continue;
        };
        parts.push(StoredPart {
            id: part.id.clone(),
            name,
            type_line: part.type_line.clone(),
        });
    }
    // Infallible for this shape (a list of string fields), and a hypothetical failure must
    // not read as "makes none" — so it degrades to the same empty array, never to NULL.
    Some(serde_json::to_string(&parts).unwrap_or_else(|_| "[]".to_string()))
}

/// Whether a related card is a token or an emblem — see [`token_parts`].
fn is_token_component(part: &RelatedCard) -> bool {
    part.component.as_deref() == Some("token")
        || part.type_line.as_deref().is_some_and(|line| {
            line.trim_start()
                .get(..6)
                .is_some_and(|head| head.eq_ignore_ascii_case("emblem"))
        })
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    const SAMPLE_CARD: &str = r#"{"object":"card","id":"abc-123","oracle_id":"ora-1","name":"Llanowar Elves","lang":"en","released_at":"2018-07-13","set":"M19","set_name":"Core Set 2019","collector_number":"314","rarity":"common","layout":"normal","mana_cost":"{G}","cmc":1.0,"type_line":"Creature — Elf Druid","oracle_text":"{T}: Add {G}.","power":"1","toughness":"1","color_identity":["G"],"colors":["G"],"digital":false,"games":["paper","mtgo"],"tcgplayer_id":179421,"tcgplayer_etched_id":250123,"image_uris":{"small":"https://img/small.jpg","normal":"https://img/normal.jpg","large":"https://img/large.jpg","png":"https://img/card.png","art_crop":"https://img/art.jpg"},"prices":{"usd":"0.25","usd_foil":"1.50","eur":"0.10","tix":"0.03"}}"#;

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
        assert_eq!(
            model.image_normal.as_ref().as_deref(),
            Some("https://img/normal.jpg")
        );
        assert_eq!(model.price_usd.as_ref().as_deref(), Some("0.25"));
        // TCGplayer product ids are picked up for the historic price backfill join.
        assert_eq!(model.tcgplayer_id.as_ref(), &Some(179421));
        assert_eq!(model.tcgplayer_etched_id.as_ref(), &Some(250123));
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
        assert_eq!(
            model.image_normal.as_ref().as_deref(),
            Some("https://img/front.jpg")
        );
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
        assert_eq!(
            join_colors(&Some(vec!["W".into(), "U".into()])),
            Some("W,U".to_string())
        );
    }

    /// The four shapes `all_parts` actually ships, in one card: a token, the card itself
    /// (which Scryfall files as a `combo_piece`), an emblem (also a `combo_piece`, told
    /// apart only by its type line), and a meld result (a real card, not something a player
    /// brings alongside the deck).
    #[test]
    fn token_parts_keeps_tokens_and_emblems_only() {
        let card = r#"{"object":"card","id":"self-1","name":"Elspeth, Sun's Champion","lang":"en","set":"thb","set_name":"Theros","collector_number":"9","games":["paper"],"all_parts":[
            {"object":"related_card","id":"self-1","component":"combo_piece","name":"Elspeth, Sun's Champion","type_line":"Legendary Planeswalker — Elspeth"},
            {"object":"related_card","id":"tok-soldier","component":"token","name":"Soldier","type_line":"Token Creature — Soldier"},
            {"object":"related_card","id":"emb-elspeth","component":"combo_piece","name":"Elspeth, Sun's Champion Emblem","type_line":"Emblem — Elspeth"},
            {"object":"related_card","id":"meld-1","component":"meld_result","name":"Brisela","type_line":"Legendary Creature — Eldrazi Angel"}
        ]}"#;
        let scry: ScryfallCard = serde_json::from_str(card).unwrap();
        let model = map_card(scry, Utc::now());
        let stored: Vec<StoredPart> =
            serde_json::from_str(model.token_parts.as_ref().as_deref().unwrap()).unwrap();
        assert_eq!(
            stored.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            vec!["tok-soldier", "emb-elspeth"],
            "the card itself and the meld result are not things a player brings alongside"
        );
        assert_eq!(stored[0].name, "Soldier");
    }

    /// Wurmcoil Engine makes two *different* Wurm tokens that share a name and a type line,
    /// so both ids have to survive here — the read groups them by the token printing's own
    /// oracle id, which it can only do if this doesn't collapse them first.
    #[test]
    fn token_parts_keeps_same_named_siblings_apart() {
        let card = r#"{"object":"card","id":"self-2","name":"Wurmcoil Engine","lang":"en","set":"som","set_name":"Scars","collector_number":"1","games":["paper"],"all_parts":[
            {"object":"related_card","id":"tok-deathtouch","component":"token","name":"Wurm","type_line":"Token Artifact Creature — Wurm"},
            {"object":"related_card","id":"tok-lifelink","component":"token","name":"Wurm","type_line":"Token Artifact Creature — Wurm"},
            {"object":"related_card","id":"tok-lifelink","component":"token","name":"Wurm","type_line":"Token Artifact Creature — Wurm"}
        ]}"#;
        let scry: ScryfallCard = serde_json::from_str(card).unwrap();
        let model = map_card(scry, Utc::now());
        let stored: Vec<StoredPart> =
            serde_json::from_str(model.token_parts.as_ref().as_deref().unwrap()).unwrap();
        assert_eq!(
            stored.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            vec!["tok-deathtouch", "tok-lifelink"],
            "distinct ids stay distinct; a repeated id is stored once"
        );
    }

    /// A card with no relations at all stores an empty array, **not** NULL: NULL is reserved
    /// for a row written before the column existed, and the deck read words those two
    /// differently ("makes none" vs "not checked yet").
    #[test]
    fn token_parts_are_empty_never_null_for_a_card_that_makes_none() {
        let scry: ScryfallCard = serde_json::from_str(SAMPLE_CARD).unwrap();
        assert!(scry.all_parts.is_none());
        let model = map_card(scry, Utc::now());
        assert_eq!(model.token_parts.as_ref().as_deref(), Some("[]"));
    }

    #[test]
    fn leading_int_parses_digit_prefix() {
        assert_eq!(leading_int("314"), Some(314));
        assert_eq!(leading_int("12a"), Some(12));
        assert_eq!(leading_int("★"), None);
        assert_eq!(leading_int("GR-1"), None);
    }
}

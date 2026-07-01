//! Mapping from Scryfall's card/set JSON shapes into our SeaORM `ActiveModel`s.
//! Kept separate from the streaming import (`ingest`) so the pure, side-effect-free
//! shaping — and its unit tests — stand on their own.

use sea_orm::{
    ActiveValue::{NotSet, Set},
    prelude::DateTimeUtc,
};

use super::GAME;
use super::model::{CardFace, ScryfallCard, ScryfallSet, StoredFace};
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

    let (price_usd, price_usd_foil, price_eur, price_tix) = match &card.prices {
        Some(p) => (
            p.usd.clone(),
            p.usd_foil.clone(),
            p.eur.clone(),
            p.tix.clone(),
        ),
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

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
        assert_eq!(
            model.image_normal.as_ref().as_deref(),
            Some("https://img/normal.jpg")
        );
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

    #[test]
    fn leading_int_parses_digit_prefix() {
        assert_eq!(leading_int("314"), Some(314));
        assert_eq!(leading_int("12a"), Some(12));
        assert_eq!(leading_int("★"), None);
        assert_eq!(leading_int("GR-1"), None);
    }
}

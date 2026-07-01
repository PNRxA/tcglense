use sea_orm::entity::prelude::*;

/// SeaORM entity for the `cards` table.
///
/// Generic across games via the `game` discriminator. For MTG there is one row
/// per Scryfall printing (paper only), sourced from the `default_cards` bulk
/// file. Image URLs are stored as the upstream Scryfall URIs; the image proxy
/// lazily downloads and caches the bytes to disk on first view, so no image is
/// fetched until something actually displays it.
///
/// `Eq` is intentionally not derived — `cmc` is an `f64`.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "cards")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Game discriminator, e.g. `"mtg"`.
    pub game: String,
    /// Provider id, unique within a game (Scryfall card id, a UUID).
    pub external_id: String,
    /// Gameplay identity shared across printings (Scryfall `oracle_id`).
    pub oracle_id: Option<String>,
    pub name: String,
    /// Set code this printing belongs to (matches `card_sets.code`).
    pub set_code: String,
    pub set_name: String,
    pub collector_number: String,
    /// Leading-digit run of `collector_number` (e.g. `"12a"` -> `12`), used to
    /// sort a set's cards numerically. `None` when the number has no digits.
    pub collector_number_int: Option<i32>,
    pub rarity: Option<String>,
    pub lang: String,
    pub released_at: Option<String>,
    pub mana_cost: Option<String>,
    pub cmc: Option<f64>,
    pub type_line: Option<String>,
    /// Comma-joined colour-identity letters, e.g. `"W,U"`.
    pub color_identity: Option<String>,
    /// Comma-joined colour letters of the card itself.
    pub colors: Option<String>,
    pub layout: Option<String>,
    /// Oracle rules text. For multi-faced cards this is the faces' text joined
    /// with `\n//\n`, so the `o:` filter still matches text on either face.
    pub oracle_text: Option<String>,
    /// Power / toughness / loyalty kept as strings because they can be non-numeric
    /// (`"*"`, `"1+*"`, `"X"`); numeric filters CAST them. For multi-faced cards
    /// these come from the first face that has them.
    pub power: Option<String>,
    pub toughness: Option<String>,
    pub loyalty: Option<String>,
    pub image_small: Option<String>,
    pub image_normal: Option<String>,
    pub image_large: Option<String>,
    pub image_art_crop: Option<String>,
    pub image_png: Option<String>,
    /// JSON array of per-face data (name + image URIs) for multi-faced cards
    /// (transform / modal DFCs) where the top-level `image_uris` is absent.
    pub card_faces: Option<String>,
    pub price_usd: Option<String>,
    pub price_usd_foil: Option<String>,
    pub price_usd_etched: Option<String>,
    pub price_eur: Option<String>,
    pub price_tix: Option<String>,
    // --- Fields ingested for Scryfall search parity (see scryfall::search). ---
    /// Comma-joined keyword abilities, e.g. `"Flying,Trample"`.
    pub keywords: Option<String>,
    /// Comma-joined colours of mana this card can produce.
    pub produced_mana: Option<String>,
    /// Comma-joined colour-indicator pips.
    pub color_indicator: Option<String>,
    pub watermark: Option<String>,
    pub flavor_text: Option<String>,
    pub illustration_id: Option<String>,
    pub artist: Option<String>,
    /// Comma-joined artist ids (for `artists>N` counts).
    pub artist_ids: Option<String>,
    pub border_color: Option<String>,
    pub frame: Option<String>,
    /// Comma-joined frame effects (showcase, extendedart, …).
    pub frame_effects: Option<String>,
    pub security_stamp: Option<String>,
    /// Comma-joined promo categories (buyabox, prerelease, …).
    pub promo_types: Option<String>,
    /// Comma-joined available finishes (nonfoil/foil/etched).
    pub finishes: Option<String>,
    /// Battle starting defence, kept as a string like power/toughness.
    pub defense: Option<String>,
    /// Per-format legality object as a JSON string, queried via `json_extract`.
    pub legalities: Option<String>,
    pub full_art: Option<bool>,
    pub textless: Option<bool>,
    pub oversized: Option<bool>,
    pub promo: Option<bool>,
    pub reprint: Option<bool>,
    pub variation: Option<bool>,
    pub booster: Option<bool>,
    pub story_spotlight: Option<bool>,
    pub content_warning: Option<bool>,
    pub highres_image: Option<bool>,
    pub reserved: Option<bool>,
    pub game_changer: Option<bool>,
    pub edhrec_rank: Option<i32>,
    pub penny_rank: Option<i32>,
    /// Whether this printing is digital-only. Paper ingestion stores `false`.
    pub digital: bool,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

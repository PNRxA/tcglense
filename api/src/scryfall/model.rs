//! Deserialization structs for the subset of the Scryfall API we consume.
//!
//! These mirror only the fields used by ingestion; unknown fields are ignored.

use serde::{Deserialize, Serialize};

/// Envelope of `GET /bulk-data`.
#[derive(Debug, Deserialize)]
pub struct BulkDataList {
    pub data: Vec<BulkData>,
}

/// One entry in the bulk-data catalog (e.g. the `default_cards` file).
///
/// Scryfall moved bulk data to **gzipped JSONL** in 2026-07: an entry now carries
/// `jsonl_download_uri` + `compressed_size` where it used to carry `download_uri` +
/// `size` (a plain JSON array). Both pairs are optional here so either shape
/// deserializes — a required `download_uri` is what silently took every Scryfall import
/// down when the field disappeared, since one missing field fails the whole list and
/// cards, rulings and art tags all start from this catalog. Read the file location
/// through [`Self::file_url`] and [`Self::transfer_size`], never the fields directly.
#[derive(Debug, Deserialize)]
pub struct BulkData {
    #[serde(rename = "type")]
    pub kind: String,
    pub updated_at: String,
    /// Legacy plain-JSON-array file. Absent since the JSONL migration.
    #[serde(default)]
    pub download_uri: Option<String>,
    /// Gzipped JSONL file — one dataset object per line, no array brackets.
    #[serde(default)]
    pub jsonl_download_uri: Option<String>,
    /// Uncompressed size of [`Self::download_uri`], when that's what's on offer.
    #[serde(default)]
    pub size: Option<u64>,
    /// Wire size of [`Self::jsonl_download_uri`] (i.e. gzipped, not the inflated size).
    #[serde(default)]
    pub compressed_size: Option<u64>,
}

impl BulkData {
    /// The file to stream, preferring the JSONL variant. `None` if the entry offers
    /// neither — a contract drift the caller turns into a failed (retried) import
    /// rather than an empty dataset.
    pub fn file_url(&self) -> Option<&str> {
        self.jsonl_download_uri
            .as_deref()
            .or(self.download_uri.as_deref())
    }

    /// Bytes [`Self::file_url`]'s transfer carries, for the import progress bar — the
    /// *compressed* size for a gzipped JSONL file, since that's what comes off the wire.
    pub fn transfer_size(&self) -> Option<u64> {
        if self.jsonl_download_uri.is_some() {
            self.compressed_size
        } else {
            self.size
        }
    }
}

/// Envelope of `GET /sets` (and any paginated Scryfall list).
#[derive(Debug, Deserialize)]
pub struct SetList {
    pub data: Vec<ScryfallSet>,
    #[serde(default)]
    pub has_more: bool,
    #[serde(default)]
    pub next_page: Option<String>,
}

/// A Scryfall set object (only the fields we store).
#[derive(Debug, Deserialize)]
pub struct ScryfallSet {
    pub id: String,
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub set_type: Option<String>,
    #[serde(default)]
    pub released_at: Option<String>,
    #[serde(default)]
    pub card_count: Option<i64>,
    #[serde(default)]
    pub digital: Option<bool>,
    #[serde(default)]
    pub icon_svg_uri: Option<String>,
    #[serde(default)]
    pub parent_set_code: Option<String>,
}

/// One entry in Scryfall's `art_tags` bulk file: a community Tagger tag describing what
/// a card's artwork depicts (issue #140), with its position in the tag hierarchy and the
/// artworks it's directly applied to. Unknown fields (`object`, `uri`, `aliases`, …) are
/// ignored. Every field except the stable `id` is optional defensively — the ingest
/// keeps only tags carrying a `slug` and drops anything else per line, like rulings.
#[derive(Debug, Deserialize)]
pub struct ScryfallArtTag {
    pub id: String,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    /// Tag type: `illustration` for art tags (`oracle` tags ship in a separate file).
    #[serde(rename = "type", default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// Child tags in the hierarchy (UUIDs within the same bulk file). The bulk data holds
    /// only *direct* taggings, so a parent tag's artworks are collected by traversing
    /// these at ingest.
    #[serde(default)]
    pub child_ids: Option<Vec<String>>,
    #[serde(default)]
    pub taggings: Option<Vec<ScryfallArtTagging>>,
}

/// One direct tagging inside a [`ScryfallArtTag`]: the artwork the tag is applied to.
/// `weight` / `annotation` exist on the wire but aren't stored.
#[derive(Debug, Deserialize)]
pub struct ScryfallArtTagging {
    #[serde(default)]
    pub illustration_id: Option<String>,
}

/// One entry in Scryfall's `rulings` bulk file: an official clarification ("Notes and
/// Rules Information") keyed by the card's gameplay identity (`oracle_id`), shared across
/// all its printings. Unknown fields (e.g. `object`) are ignored. Every field is optional
/// defensively — the ingest keeps only rulings carrying an `oracle_id` and a `comment`.
#[derive(Debug, Deserialize)]
pub struct ScryfallRuling {
    #[serde(default)]
    pub oracle_id: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub published_at: Option<String>,
    #[serde(default)]
    pub comment: Option<String>,
}

/// A Scryfall card object (only the fields we store).
///
/// `Default` is derived so the dummy seeder can fill new fields via
/// `..Default::default()` without listing every one.
#[derive(Debug, Default, Deserialize)]
pub struct ScryfallCard {
    pub id: String,
    #[serde(default)]
    pub oracle_id: Option<String>,
    pub name: String,
    pub lang: String,
    #[serde(default)]
    pub released_at: Option<String>,
    pub set: String,
    pub set_name: String,
    pub collector_number: String,
    #[serde(default)]
    pub rarity: Option<String>,
    #[serde(default)]
    pub layout: Option<String>,
    #[serde(default)]
    pub mana_cost: Option<String>,
    #[serde(default)]
    pub cmc: Option<f64>,
    #[serde(default)]
    pub type_line: Option<String>,
    #[serde(default)]
    pub oracle_text: Option<String>,
    #[serde(default)]
    pub power: Option<String>,
    #[serde(default)]
    pub toughness: Option<String>,
    #[serde(default)]
    pub loyalty: Option<String>,
    #[serde(default)]
    pub color_identity: Option<Vec<String>>,
    #[serde(default)]
    pub colors: Option<Vec<String>>,
    #[serde(default)]
    pub digital: Option<bool>,
    /// Where the card exists: e.g. `["paper", "mtgo"]`. Used for the paper filter.
    #[serde(default)]
    pub games: Vec<String>,
    #[serde(default)]
    pub image_uris: Option<ImageUris>,
    #[serde(default)]
    pub card_faces: Option<Vec<CardFace>>,
    #[serde(default)]
    pub prices: Option<Prices>,
    /// TCGplayer product id for the regular/foil printing — the join key onto
    /// TCGCSV's `productId` for the historic price backfill (see `crate::tcgcsv`).
    #[serde(default)]
    pub tcgplayer_id: Option<i32>,
    /// TCGplayer product id for the etched printing, when Scryfall distinguishes one.
    #[serde(default)]
    pub tcgplayer_etched_id: Option<i32>,
    // --- Additional fields ingested for Scryfall search parity. ---
    /// Keyword abilities (Flying, Trample, …). Comma-joined on storage.
    #[serde(default)]
    pub keywords: Option<Vec<String>>,
    /// Colours of mana this card can produce.
    #[serde(default)]
    pub produced_mana: Option<Vec<String>>,
    /// Colour-indicator pips; also present per-face on some DFCs.
    #[serde(default)]
    pub color_indicator: Option<Vec<String>>,
    /// Printed watermark/affiliation; per-face on some cards.
    #[serde(default)]
    pub watermark: Option<String>,
    /// Printed flavour text; per-face on multi-faced cards.
    #[serde(default)]
    pub flavor_text: Option<String>,
    /// Illustration id (distinct artwork); per-face on some cards.
    #[serde(default)]
    pub illustration_id: Option<String>,
    #[serde(default)]
    pub artist: Option<String>,
    #[serde(default)]
    pub artist_ids: Option<Vec<String>>,
    #[serde(default)]
    pub border_color: Option<String>,
    /// Frame edition ("1993"/"2015"/"future"/…).
    #[serde(default)]
    pub frame: Option<String>,
    /// Frame effects (showcase, extendedart, …). Comma-joined on storage.
    #[serde(default)]
    pub frame_effects: Option<Vec<String>>,
    #[serde(default)]
    pub security_stamp: Option<String>,
    /// Promo categories (buyabox, prerelease, …). Comma-joined on storage.
    #[serde(default)]
    pub promo_types: Option<Vec<String>>,
    /// Available finishes (nonfoil/foil/etched). Comma-joined on storage.
    #[serde(default)]
    pub finishes: Option<Vec<String>>,
    /// Battle starting defence (string like power/toughness); per-face on some cards.
    #[serde(default)]
    pub defense: Option<String>,
    /// Per-format legality object, stored verbatim as a JSON string.
    #[serde(default)]
    pub legalities: Option<serde_json::Value>,
    #[serde(default)]
    pub full_art: Option<bool>,
    #[serde(default)]
    pub textless: Option<bool>,
    #[serde(default)]
    pub oversized: Option<bool>,
    #[serde(default)]
    pub promo: Option<bool>,
    #[serde(default)]
    pub reprint: Option<bool>,
    #[serde(default)]
    pub variation: Option<bool>,
    #[serde(default)]
    pub booster: Option<bool>,
    #[serde(default)]
    pub story_spotlight: Option<bool>,
    #[serde(default)]
    pub content_warning: Option<bool>,
    #[serde(default)]
    pub highres_image: Option<bool>,
    #[serde(default)]
    pub reserved: Option<bool>,
    #[serde(default)]
    pub game_changer: Option<bool>,
    #[serde(default)]
    pub edhrec_rank: Option<i32>,
    #[serde(default)]
    pub penny_rank: Option<i32>,
}

/// The image URLs Scryfall offers for a card (or a single face).
#[derive(Debug, Deserialize)]
pub struct ImageUris {
    #[serde(default)]
    pub small: Option<String>,
    #[serde(default)]
    pub normal: Option<String>,
    #[serde(default)]
    pub large: Option<String>,
    #[serde(default)]
    pub png: Option<String>,
    #[serde(default)]
    pub art_crop: Option<String>,
}

/// One face of a multi-faced card (transform / modal DFC). Such cards usually
/// have no top-level `image_uris`; the per-face images live here.
#[derive(Debug, Default, Deserialize)]
pub struct CardFace {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub mana_cost: Option<String>,
    #[serde(default)]
    pub type_line: Option<String>,
    #[serde(default)]
    pub oracle_text: Option<String>,
    #[serde(default)]
    pub power: Option<String>,
    #[serde(default)]
    pub toughness: Option<String>,
    #[serde(default)]
    pub loyalty: Option<String>,
    #[serde(default)]
    pub image_uris: Option<ImageUris>,
    /// Per-face fields folded into the top-level card columns when the top level
    /// lacks them (see `map::map_card`).
    #[serde(default)]
    pub watermark: Option<String>,
    #[serde(default)]
    pub flavor_text: Option<String>,
    #[serde(default)]
    pub illustration_id: Option<String>,
    #[serde(default)]
    pub defense: Option<String>,
    #[serde(default)]
    pub color_indicator: Option<Vec<String>>,
}

/// Current price snapshot for a card.
#[derive(Debug, Deserialize)]
pub struct Prices {
    #[serde(default)]
    pub usd: Option<String>,
    #[serde(default)]
    pub usd_foil: Option<String>,
    #[serde(default)]
    pub usd_etched: Option<String>,
    #[serde(default)]
    pub eur: Option<String>,
    #[serde(default)]
    pub tix: Option<String>,
}

/// Slimmed per-face record persisted as JSON in `cards.card_faces`, so the API
/// and UI can render both faces (and the image proxy can resolve `?face=N`)
/// without re-fetching from Scryfall.
#[derive(Debug, Serialize, Deserialize)]
pub struct StoredFace {
    pub name: Option<String>,
    pub mana_cost: Option<String>,
    pub type_line: Option<String>,
    #[serde(default)]
    pub oracle_text: Option<String>,
    #[serde(default)]
    pub power: Option<String>,
    #[serde(default)]
    pub toughness: Option<String>,
    #[serde(default)]
    pub loyalty: Option<String>,
    pub image_small: Option<String>,
    pub image_normal: Option<String>,
    pub image_large: Option<String>,
    pub image_png: Option<String>,
    pub image_art_crop: Option<String>,
}

impl StoredFace {
    pub fn from_face(face: &CardFace) -> Self {
        let img = face.image_uris.as_ref();
        StoredFace {
            name: face.name.clone(),
            mana_cost: face.mana_cost.clone(),
            type_line: face.type_line.clone(),
            oracle_text: face.oracle_text.clone(),
            power: face.power.clone(),
            toughness: face.toughness.clone(),
            loyalty: face.loyalty.clone(),
            image_small: img.and_then(|u| u.small.clone()),
            image_normal: img.and_then(|u| u.normal.clone()),
            image_large: img.and_then(|u| u.large.clone()),
            image_png: img.and_then(|u| u.png.clone()),
            image_art_crop: img.and_then(|u| u.art_crop.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A verbatim entry from the live bulk-data catalog (2026-07-30), pinning the shape
    /// that replaced `download_uri`/`size`: a gzipped JSONL file and its compressed size.
    const JSONL_ENTRY: &str = r#"{"object":"bulk_data","id":"48da5752-eeb6-4126-bf97-8829e20ad14f","type":"art_tags","updated_at":"2026-07-30T09:01:23.670+00:00","uri":"https://api.scryfall.com/bulk-data/48da5752-eeb6-4126-bf97-8829e20ad14f","name":"Art Tags","description":"A JSON file containing all art (illustration) tags sourced from Tagger, the Scryfall community tagging project.","jsonl_download_uri":"https://data.scryfall.io/art-tags/art-tags-20260730090123.jsonl.gz","compressed_size":12359045}"#;

    /// The pre-migration entry shape, kept working so a mirror still serving the old
    /// catalog (or a rollback upstream) imports rather than failing the whole list.
    const LEGACY_ENTRY: &str = r#"{"object":"bulk_data","id":"48da5752","type":"art_tags","updated_at":"2026-07-23T09:01:23.670+00:00","download_uri":"https://data.scryfall.io/art-tags/art-tags-20260723090123.json","size":48000000}"#;

    #[test]
    fn reads_the_jsonl_catalog_entry() {
        let entry: BulkData = serde_json::from_str(JSONL_ENTRY).expect("parses");
        assert_eq!(entry.kind, "art_tags");
        assert_eq!(entry.updated_at, "2026-07-30T09:01:23.670+00:00");
        assert_eq!(
            entry.file_url(),
            Some("https://data.scryfall.io/art-tags/art-tags-20260730090123.jsonl.gz")
        );
        // The bar counts wire bytes, so the compressed size is the one that matches.
        assert_eq!(entry.transfer_size(), Some(12_359_045));
    }

    #[test]
    fn reads_the_legacy_catalog_entry() {
        let entry: BulkData = serde_json::from_str(LEGACY_ENTRY).expect("parses");
        assert_eq!(
            entry.file_url(),
            Some("https://data.scryfall.io/art-tags/art-tags-20260723090123.json")
        );
        assert_eq!(entry.transfer_size(), Some(48_000_000));
    }

    #[test]
    fn prefers_jsonl_when_an_entry_offers_both() {
        let both = r#"{"type":"rulings","updated_at":"x","download_uri":"https://old/rulings.json","size":10,"jsonl_download_uri":"https://new/rulings.jsonl.gz","compressed_size":3}"#;
        let entry: BulkData = serde_json::from_str(both).expect("parses");
        assert_eq!(entry.file_url(), Some("https://new/rulings.jsonl.gz"));
        assert_eq!(entry.transfer_size(), Some(3));
    }

    /// An entry with no file at all still deserializes — one dataset missing its URL must
    /// not fail the whole catalog for the others; the caller errors on use instead.
    #[test]
    fn an_entry_without_any_file_url_still_parses() {
        let entry: BulkData =
            serde_json::from_str(r#"{"type":"oracle_tags","updated_at":"x"}"#).expect("parses");
        assert_eq!(entry.file_url(), None);
        assert_eq!(entry.transfer_size(), None);
    }
}

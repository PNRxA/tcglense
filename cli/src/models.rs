//! Wire types mirroring the TCGLense API's JSON DTOs.
//!
//! These are hand-maintained duplicates of the Rust DTOs the API serialises (the
//! same shapes the SPA consumes via `web/src/lib/api/generated/`). Only the fields
//! the CLI reads/renders are modelled; unknown fields are ignored on deserialize.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A page of results plus the cursor metadata to paginate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page<T> {
    pub data: Vec<T>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
    pub has_more: bool,
}

/// The `{ "data": T }` envelope used by non-paginated list endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataBody<T> {
    pub data: T,
}

// ---------------------------------------------------------------------------
// Auth
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: i64,
    pub email: String,
    pub created_at: String,
    pub username: Option<String>,
    pub discriminator: Option<i64>,
    pub handle: Option<String>,
    pub currency: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthResponse {
    pub access_token: String,
    pub user: User,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterResponse {
    pub completion_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicConfig {
    pub maintenance_mode: bool,
    pub turnstile_site_key: Option<String>,
    pub signups_enabled: bool,
    pub signups_disabled_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrencyRatesResponse {
    pub base: String,
    pub as_of: String,
    pub rates: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyInfo {
    pub id: i64,
    pub name: String,
    pub scope: String,
    pub key_prefix: String,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyList {
    pub data: Vec<ApiKeyInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatedApiKey {
    pub id: i64,
    pub name: String,
    pub scope: String,
    pub key: String,
    pub key_prefix: String,
    pub created_at: String,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsernameAvailability {
    pub valid: bool,
    pub reason: Option<String>,
}

// ---------------------------------------------------------------------------
// Catalog
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Game {
    pub id: String,
    pub name: String,
    pub publisher: String,
    pub data_source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardSet {
    pub code: String,
    pub name: String,
    pub set_type: Option<String>,
    pub released_at: Option<String>,
    pub card_count: i64,
    pub icon_svg_uri: Option<String>,
    pub parent_set_code: Option<String>,
    pub has_drops: bool,
    pub has_subtypes: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardPrices {
    pub usd: Option<String>,
    pub usd_foil: Option<String>,
    pub eur: Option<String>,
    pub tix: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardFace {
    pub name: Option<String>,
    pub mana_cost: Option<String>,
    pub type_line: Option<String>,
    pub oracle_text: Option<String>,
    pub power: Option<String>,
    pub toughness: Option<String>,
    pub loyalty: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Card {
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
    #[serde(default)]
    pub color_identity: Vec<String>,
    #[serde(default)]
    pub colors: Vec<String>,
    pub layout: Option<String>,
    pub prices: CardPrices,
    pub has_image: bool,
    pub drop_name: Option<String>,
    pub drop_slug: Option<String>,
    #[serde(default)]
    pub secret_lair_bonus: bool,
    #[serde(default)]
    pub secret_lair_spend_incentive: bool,
    #[serde(default)]
    pub faces: Vec<CardFace>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricePoint {
    pub date: String,
    pub usd: Option<String>,
    pub usd_foil: Option<String>,
    pub eur: Option<String>,
    pub tix: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestStatus {
    pub status: String,
    pub detail: Option<String>,
    pub sets_imported: i64,
    pub cards_imported: i64,
    pub source_updated_at: Option<String>,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropGroup {
    pub slug: Option<String>,
    pub title: String,
    pub card_count: i64,
    pub cheapest_prints_usd: Option<String>,
    pub cards: Vec<Card>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtypeGroup {
    pub slug: Option<String>,
    pub title: String,
    pub card_count: i64,
    pub cards: Vec<Card>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanMatch {
    pub card: Card,
    pub distance: i64,
}

// ---------------------------------------------------------------------------
// Sealed products
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductPrices {
    pub usd: Option<String>,
    pub usd_foil: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Product {
    pub id: String,
    pub name: String,
    pub set_code: String,
    pub set_name: Option<String>,
    pub product_type: String,
    pub url: Option<String>,
    pub has_image: bool,
    pub prices: ProductPrices,
    pub msrp: Option<String>,
    pub released_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductPricePoint {
    pub date: String,
    pub usd: Option<String>,
    pub usd_foil: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductSetRef {
    pub code: String,
    pub name: Option<String>,
    pub product_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductFacets {
    pub types: Vec<String>,
    pub sets: Vec<ProductSetRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SealedProductRef {
    pub product: Product,
    pub membership: String,
    pub foil: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductCardEntry {
    pub card: Card,
    pub membership: String,
    pub foil: bool,
    pub exclusive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductCardSection {
    pub key: String,
    pub total: i64,
    pub booster_family: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductComponent {
    pub kind: String,
    pub name: String,
    pub quantity: i64,
    pub product: Option<Product>,
    pub card: Option<Card>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductContainer {
    pub product: Product,
    pub quantity: i64,
}

// ---------------------------------------------------------------------------
// Collection / wish list
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CollectionQuantities {
    pub quantity: i64,
    pub foil_quantity: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionEntry {
    pub card: Card,
    pub quantity: i64,
    pub foil_quantity: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionSummary {
    pub unique_cards: i64,
    pub total_cards: i64,
    pub total_value_usd: Option<String>,
    pub bulk_value_usd: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionSet {
    pub code: String,
    pub name: String,
    pub set_type: Option<String>,
    pub released_at: Option<String>,
    pub card_count: i64,
    pub icon_svg_uri: Option<String>,
    pub parent_set_code: Option<String>,
    pub has_drops: bool,
    pub has_subtypes: bool,
    pub owned_cards: i64,
    pub owned_copies: i64,
    pub owned_value_usd: Option<String>,
    pub owned_bulk_value_usd: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionSource {
    pub provider: String,
    pub external_id: String,
    pub url: String,
    pub last_synced_at: Option<String>,
    pub smart: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionValuePoint {
    pub date: String,
    pub value_usd: Option<String>,
    pub sealed_value_usd: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionVisibility {
    pub public: bool,
    pub show_value_chart: bool,
    pub show_movers: bool,
    pub handle: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionDropGroup {
    pub slug: Option<String>,
    pub title: String,
    pub card_count: i64,
    pub cards: Vec<CollectionEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionSubtypeGroup {
    pub slug: Option<String>,
    pub title: String,
    pub card_count: i64,
    pub cards: Vec<CollectionEntry>,
}

// Movers ---------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionMover {
    pub card: Card,
    pub quantity: i64,
    pub foil_quantity: i64,
    pub value_now: String,
    pub value_prev: String,
    pub change_usd: String,
    pub change_pct: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionMoverList {
    pub gainers: Vec<CollectionMover>,
    pub losers: Vec<CollectionMover>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionSealedMover {
    pub product: Product,
    pub quantity: i64,
    pub foil_quantity: i64,
    pub value_now: String,
    pub value_prev: String,
    pub change_usd: String,
    pub change_pct: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionSealedMoverList {
    pub gainers: Vec<CollectionSealedMover>,
    pub losers: Vec<CollectionSealedMover>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionSealedMovers {
    pub as_of: Option<String>,
    pub day_as_of: Option<String>,
    pub day: CollectionSealedMoverList,
    pub week: CollectionSealedMoverList,
    pub month: CollectionSealedMoverList,
    pub year: CollectionSealedMoverList,
    pub two_year: CollectionSealedMoverList,
    pub three_year: CollectionSealedMoverList,
    pub all_time: CollectionSealedMoverList,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionMovers {
    pub as_of: Option<String>,
    pub day_as_of: Option<String>,
    pub day: CollectionMoverList,
    pub week: CollectionMoverList,
    pub month: CollectionMoverList,
    pub year: CollectionMoverList,
    pub two_year: CollectionMoverList,
    pub three_year: CollectionMoverList,
    pub all_time: CollectionMoverList,
    pub sealed: CollectionSealedMovers,
}

// Sealed-product holdings ----------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductHoldingEntry {
    pub product: Product,
    pub quantity: i64,
    pub foil_quantity: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductHoldingSummary {
    pub unique_products: i64,
    pub total_products: i64,
    pub total_value_usd: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductHoldingSet {
    pub code: String,
    pub name: Option<String>,
    pub unique_products: i64,
    pub total_products: i64,
    pub total_value_usd: Option<String>,
}

// Import / sync --------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportProgress {
    pub fetched: i64,
    pub total: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportSummary {
    pub provider: String,
    pub mode: String,
    pub total_rows: i64,
    pub distinct_cards: i64,
    pub matched_cards: i64,
    pub unmatched_cards: i64,
    pub unmatched_sample: Vec<String>,
    pub regular_copies: i64,
    pub foil_copies: i64,
    pub removed_cards: i64,
    pub stopped_early: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportJob {
    pub job_id: i64,
    pub status: String,
    #[serde(default)]
    pub progress: Option<ImportProgress>,
    #[serde(default)]
    pub summary: Option<ImportSummary>,
    #[serde(default)]
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// Decks
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Deck {
    pub id: i64,
    pub game: String,
    pub name: String,
    pub description: Option<String>,
    pub format: Option<String>,
    pub folder_id: Option<i64>,
    pub is_public: bool,
    pub card_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeckSection {
    pub id: i64,
    pub name: String,
    pub position: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeckCardEntry {
    pub card: Card,
    pub section_id: i64,
    pub quantity: i64,
    pub foil_quantity: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeckDetail {
    pub id: i64,
    pub game: String,
    pub name: String,
    pub description: Option<String>,
    pub format: Option<String>,
    pub folder_id: Option<i64>,
    pub is_public: bool,
    pub handle: Option<String>,
    pub summary: CollectionSummary,
    pub sections: Vec<DeckSection>,
    pub cards: Vec<DeckCardEntry>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeckFolder {
    pub id: i64,
    pub name: String,
    pub deck_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeckVisibility {
    pub public: bool,
    pub handle: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeckImportResponse {
    pub deck: Deck,
    pub provider: String,
    pub total_rows: i64,
    pub matched_cards: i64,
    pub unmatched_cards: i64,
    pub unmatched_sample: Vec<String>,
}

// ---------------------------------------------------------------------------
// Public sharing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicGameSummary {
    pub game: String,
    pub summary: CollectionSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicProfile {
    pub username: String,
    pub discriminator: i64,
    pub handle: String,
    pub member_since: String,
    pub games: Vec<PublicGameSummary>,
}

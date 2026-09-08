//! Booster expected value + the pack opener (issue #682) over the real router.
//!
//! What these pin, over and above the pure-function unit tests beside each engine:
//!
//! * `/ev` is ordinary **public catalog**: anonymous, shared-cacheable, ETagged — an
//!   expected value is the same number for every visitor and moves only when prices do.
//! * A product with no booster data answers `{ "data": null }`, **not** an error. A client
//!   hides the panel from that one response; treating "nothing to say" as a failure would
//!   put an error banner on every commander deck in the catalog.
//! * The opener is **reproducible over HTTP**: the same URL deals the same cards, and a
//!   seedless request echoes back a seed that replays it.
//! * A **seedless** opening is `no-store`. It sits in the same CDN-cached group as the rest
//!   of the catalog, and a random roll is not a function of its URL — without this one
//!   visitor's box would be served as everyone's for the best part of a day.
//! * The bounds are refused as `422`s **before** anything is drawn, so no request can turn a
//!   `quantity` from ingested data into an unbounded response.
//!
//! Rows are seeded straight through the entities: the tables are rebuilt wholesale by the
//! MTGJSON sealed sync, which these in-process tests never run.

use super::harness::*;
use crate::entities::booster_config::{Variant, encode_variants};
use crate::entities::booster_sheet::encode_cards;
use crate::entities::{booster_config, booster_sheet, card, sealed_pack};
use crate::test_support::{insert_card, insert_product};
use axum::http::header::ETAG;
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, DatabaseConnection};

/// A booster pack product (one pack per copy).
const PACK: &str = "910001";
/// A bundle: six of the same pack per copy.
const BUNDLE: &str = "910002";
/// A commander deck: catalogued, but nothing in it opens.
const DECK: &str = "910003";

/// A catalog card at a known price (`insert_card` seeds an unpriced row).
async fn seed_card(db: &DatabaseConnection, external_id: &str, usd: Option<&str>) -> i32 {
    let id = insert_card(db, external_id).await;
    card::ActiveModel {
        id: Set(id),
        price_usd: Set(usd.map(str::to_string)),
        ..Default::default()
    }
    .update(db)
    .await
    .expect("price the card");
    id
}

async fn insert_config(db: &DatabaseConnection, variants: &[Variant]) -> i32 {
    let now = Utc::now();
    booster_config::ActiveModel {
        game: Set("mtg".to_string()),
        set_code: Set("tst".to_string()),
        code: Set("play".to_string()),
        name: Set(Some("Play Booster".to_string())),
        total_weight: Set(variants.iter().map(|v| v.weight as i64).sum()),
        variants: Set(encode_variants(variants)),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await
    .expect("insert booster config")
    .id
}

async fn insert_sheet(
    db: &DatabaseConnection,
    config_id: i32,
    name: &str,
    fixed: bool,
    total_weight: i64,
    cards: &[(i32, u32)],
) {
    let now = Utc::now();
    booster_sheet::ActiveModel {
        config_id: Set(config_id),
        name: Set(name.to_string()),
        foil: Set(false),
        balance_colors: Set(false),
        allow_duplicates: Set(false),
        fixed: Set(fixed),
        total_weight: Set(total_weight),
        cards: Set(encode_cards(cards)),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await
    .expect("insert booster sheet");
}

async fn insert_pack(db: &DatabaseConnection, product_id: i32, config_id: i32, quantity: i32) {
    let now = Utc::now();
    sealed_pack::ActiveModel {
        game: Set("mtg".to_string()),
        product_id: Set(product_id),
        config_id: Set(config_id),
        quantity: Set(quantity),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await
    .expect("insert sealed pack");
}

/// A hand-computable booster, and three products: a pack that opens one of it, a bundle that
/// opens six, and a deck that opens nothing.
///
/// ```text
/// W = 4:  v0 (weight 3) = common x3, rare x1
///         v1 (weight 1) = common x3, rare x1, land x1
/// common  T=6  six cards w1: five @ $0.30, one unpriced   -> 3 picks x 25c = 75c
/// rare    T=4  [w3 @ $1.00, w1 @ $20.00]                  -> 1 pick x 575c = 575c
/// land    fixed [one card @ $0.20], only v1               -> 0.25 x 20c = 5c
/// ```
///
/// So one pack is worth `$6.55`, and the bundle's six `$39.30`.
async fn seed_boosters(app: &TestApp) {
    let db = &app.state.db;
    insert_product(
        db,
        PACK,
        "Test Play Booster",
        "tst",
        "play_pack",
        Some("4.99"),
    )
    .await;
    insert_product(db, BUNDLE, "Test Bundle", "tst", "bundle", Some("39.99")).await;
    insert_product(
        db,
        DECK,
        "Test Commander Deck",
        "tst",
        "commander_deck",
        Some("44.99"),
    )
    .await;

    let mut commons = Vec::new();
    for n in 1..=5 {
        commons.push((seed_card(db, &format!("bst-c{n}"), Some("0.30")).await, 1));
    }
    // A card with no market price: it counts as $0 and shows up in `priced_share`.
    commons.push((seed_card(db, "bst-c6", None).await, 1));
    let rare_common = seed_card(db, "bst-r1", Some("1.00")).await;
    let rare_chase = seed_card(db, "bst-r2", Some("20.00")).await;
    let land = seed_card(db, "bst-l1", Some("0.20")).await;

    let config = insert_config(
        db,
        &[
            Variant {
                weight: 3,
                slots: vec![("common".to_string(), 3), ("rare".to_string(), 1)],
            },
            Variant {
                weight: 1,
                slots: vec![
                    ("common".to_string(), 3),
                    ("rare".to_string(), 1),
                    ("land".to_string(), 1),
                ],
            },
        ],
    )
    .await;
    insert_sheet(db, config, "common", false, 6, &commons).await;
    insert_sheet(
        db,
        config,
        "rare",
        false,
        4,
        &[(rare_common, 3), (rare_chase, 1)],
    )
    .await;
    insert_sheet(db, config, "land", true, 1, &[(land, 1)]).await;

    let pack_id = product_id(db, PACK).await;
    let bundle_id = product_id(db, BUNDLE).await;
    insert_pack(db, pack_id, config, 1).await;
    insert_pack(db, bundle_id, config, 6).await;
}

async fn product_id(db: &DatabaseConnection, external_id: &str) -> i32 {
    use crate::entities::{prelude::Product, product};
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    Product::find()
        .filter(product::Column::ExternalId.eq(external_id))
        .one(db)
        .await
        .expect("query product")
        .expect("product exists")
        .id
}

#[tokio::test]
async fn expected_value_is_publicly_readable_shared_cacheable_and_etagged() {
    let app = test_app().await;
    seed_boosters(&app).await;

    let (status, headers, body) =
        send(&app, get(&format!("/api/games/mtg/products/{PACK}/ev"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        cache_control(&headers),
        Some(crate::handlers::cache::PUBLIC_CATALOG_CACHE),
        "an expected value is the same number for every visitor"
    );
    assert!(
        headers.get(ETAG).is_some(),
        "a cacheable catalog success carries an ETag so a revalidation is cheap"
    );

    let ev = &body["data"];
    assert_eq!(ev["ev_usd"], "6.55", "{ev:?}");
    assert_eq!(ev["packs"].as_array().expect("packs").len(), 1);
    let pack = &ev["packs"][0];
    assert_eq!(pack["booster_code"], "play");
    assert_eq!(pack["name"], "Play Booster");
    assert_eq!(pack["set_code"], "tst");
    assert_eq!(pack["quantity"], 1);
    assert_eq!(pack["ev_usd"], "6.55");
    // 3/4 x 4 cards + 1/4 x 5 cards.
    assert!((pack["cards_per_pack"].as_f64().unwrap_or_default() - 4.25).abs() < 1e-9);
    assert_eq!(
        pack["slots"]
            .as_array()
            .expect("slots")
            .iter()
            .map(|s| (
                s["sheet"].as_str().unwrap_or_default(),
                s["ev_usd"].as_str().unwrap_or_default()
            ))
            .collect::<Vec<_>>(),
        vec![("common", "0.75"), ("rare", "5.75"), ("land", "0.05")],
    );

    // The chase rare leads the list, quoted as odds per pack and never as an infinity.
    let best = &ev["top"][0];
    assert_eq!(best["card"]["id"], "bst-r2");
    assert_eq!(best["contribution_usd"], "5.00");
    assert!((best["one_in"].as_f64().unwrap_or_default() - 4.0).abs() < 1e-9);
    for entry in ev["top"].as_array().expect("top") {
        assert!(
            entry["one_in"].as_f64().is_some_and(f64::is_finite),
            "odds must be a finite number: {entry:?}"
        );
    }

    // The honest half of the response rides with it.
    let caveats = ev["caveats"].as_array().expect("caveats");
    assert!(!caveats.is_empty());
    assert!(
        caveats[0]
            .as_str()
            .unwrap_or_default()
            .contains("no single pack is worth this"),
        "{caveats:?}"
    );
    assert!(
        caveats
            .iter()
            .any(|c| c.as_str().unwrap_or_default().contains("count as $0")),
        "one common has no market price, and the response says so: {caveats:?}"
    );
}

#[tokio::test]
async fn a_multi_pack_product_is_worth_its_packs() {
    let app = test_app().await;
    seed_boosters(&app).await;

    let (status, _, body) = send(&app, get(&format!("/api/games/mtg/products/{BUNDLE}/ev"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["ev_usd"], "39.30", "six packs at $6.55");
    assert_eq!(body["data"]["packs"][0]["quantity"], 6);
    // The copy's top scales the money by the quantity while the odds stay per pack.
    assert_eq!(body["data"]["top"][0]["contribution_usd"], "30.00");
    assert!(
        (body["data"]["top"][0]["one_in"]
            .as_f64()
            .unwrap_or_default()
            - 4.0)
            .abs()
            < 1e-9,
        "one in four *packs*, however many packs the product holds"
    );
}

#[tokio::test]
async fn a_product_with_no_booster_data_answers_null_rather_than_an_error() {
    let app = test_app().await;
    seed_boosters(&app).await;

    let (status, headers, body) =
        send(&app, get(&format!("/api/games/mtg/products/{DECK}/ev"))).await;
    assert_eq!(status, StatusCode::OK, "nothing to say is not a failure");
    assert!(
        body["data"].is_null(),
        "a deck opens nothing, so it has no expected value: {body:?}"
    );
    assert_eq!(
        cache_control(&headers),
        Some(crate::handlers::cache::PUBLIC_CATALOG_CACHE)
    );
}

#[tokio::test]
async fn an_unknown_product_is_a_no_store_404_on_both_reads() {
    let app = test_app().await;
    seed_boosters(&app).await;

    for path in [
        "/api/games/mtg/products/nope/ev",
        "/api/games/mtg/products/nope/open?seed=1",
        "/api/games/nope/products/910001/ev",
        "/api/games/nope/products/910001/open?seed=1",
    ] {
        let (status, headers, _) = send(&app, get(path)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
        assert_eq!(
            cache_control(&headers),
            Some("no-store"),
            "a 404 must never be CDN-pinned: {path}"
        );
        assert!(headers.get(ETAG).is_none(), "{path}");
    }
}

#[tokio::test]
async fn the_same_seed_opens_the_same_packs_and_is_cacheable() {
    let app = test_app().await;
    seed_boosters(&app).await;

    let path = format!("/api/games/mtg/products/{BUNDLE}/open?seed=1");
    let (status, headers, first) = send(&app, get(&path)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        cache_control(&headers),
        Some(crate::handlers::cache::PUBLIC_CATALOG_CACHE),
        "a seeded opening is a pure function of its URL, so caching it caches the answer"
    );
    assert!(headers.get(ETAG).is_some());

    let (_, _, second) = send(&app, get(&path)).await;
    assert_eq!(first, second, "the same URL deals the same cards");

    assert_eq!(first["seed"], 1);
    assert_eq!(first["copies"], 1);
    let packs = first["packs"].as_array().expect("packs");
    assert_eq!(packs.len(), 6, "one copy of the bundle opens six packs");
    for pack in packs {
        let cards = pack["cards"].as_array().expect("cards");
        // Every variant is three commons + a rare, and the rarer one adds a land.
        assert!(
            cards.len() == 4 || cards.len() == 5,
            "a pack deals its rolled variant's slots: {pack:?}"
        );
        let commons: Vec<&str> = cards
            .iter()
            .filter(|c| c["sheet"] == "common")
            .map(|c| c["card"]["id"].as_str().unwrap_or_default())
            .collect();
        let mut unique = commons.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(commons.len(), 3);
        assert_eq!(unique.len(), 3, "a slot is drawn without replacement");
    }

    let dealt = packs
        .iter()
        .map(|p| p["cards"].as_array().map_or(0, Vec::len))
        .sum::<usize>();
    assert_eq!(
        first["priced_count"].as_u64().unwrap_or_default()
            + first["unpriced_count"].as_u64().unwrap_or_default(),
        dealt as u64,
        "every pulled card is counted exactly once"
    );
    assert!(
        first["caveats"]
            .as_array()
            .and_then(|c| c.first())
            .and_then(|c| c.as_str())
            .unwrap_or_default()
            .contains("roll of the dice"),
        "an opening says what it is: {:?}",
        first["caveats"]
    );
}

#[tokio::test]
async fn opening_more_copies_extends_the_same_run() {
    let app = test_app().await;
    seed_boosters(&app).await;

    let (_, _, one) = send(
        &app,
        get(&format!("/api/games/mtg/products/{PACK}/open?seed=9")),
    )
    .await;
    let (_, _, five) = send(
        &app,
        get(&format!(
            "/api/games/mtg/products/{PACK}/open?seed=9&copies=5"
        )),
    )
    .await;
    assert_eq!(five["copies"], 5);
    assert_eq!(five["packs"].as_array().expect("packs").len(), 5);
    assert_eq!(
        one["packs"][0], five["packs"][0],
        "pack n is the same pack whatever `copies` was, so a shared URL means one thing"
    );
}

#[tokio::test]
async fn a_seedless_opening_is_never_shared_cached() {
    let app = test_app().await;
    seed_boosters(&app).await;

    let (status, headers, body) =
        send(&app, get(&format!("/api/games/mtg/products/{PACK}/open"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        cache_control(&headers),
        Some("no-store"),
        "a random roll is not a function of its URL; a CDN must not serve one visitor's box \
         as everyone's"
    );
    assert!(
        headers.get(ETAG).is_none(),
        "a no-store response carries no ETag"
    );
    // The minted seed rides back so the roll can be replayed or shared.
    let seed = body["seed"].as_u64().expect("a seed is echoed");
    let (_, headers, replay) = send(
        &app,
        get(&format!("/api/games/mtg/products/{PACK}/open?seed={seed}")),
    )
    .await;
    assert_eq!(replay["packs"], body["packs"], "the echoed seed replays it");
    assert_eq!(
        cache_control(&headers),
        Some(crate::handlers::cache::PUBLIC_CATALOG_CACHE),
        "and once it names its seed it is ordinary cacheable catalog"
    );
}

#[tokio::test]
async fn the_opening_bounds_are_422s() {
    let app = test_app().await;
    seed_boosters(&app).await;

    // Nothing to open.
    let (status, headers, body) =
        send(&app, get(&format!("/api/games/mtg/products/{DECK}/open"))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(cache_control(&headers), Some("no-store"));
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("no booster data"),
        "{body:?}"
    );

    // Zero copies.
    let (status, _, body) = send(
        &app,
        get(&format!("/api/games/mtg/products/{PACK}/open?copies=0")),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body:?}");

    // Six packs a copy: six copies is exactly the limit, seven is past it.
    let (status, _, body) = send(
        &app,
        get(&format!(
            "/api/games/mtg/products/{BUNDLE}/open?seed=1&copies=6"
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["packs"].as_array().expect("packs").len(), 36);

    let (status, _, body) = send(
        &app,
        get(&format!(
            "/api/games/mtg/products/{BUNDLE}/open?seed=1&copies=7"
        )),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("too many"),
        "{body:?}"
    );

    // An absurd copy count is refused the same way rather than overflowing anything.
    let (status, _, _) = send(
        &app,
        get(&format!(
            "/api/games/mtg/products/{BUNDLE}/open?seed=1&copies=4000000000"
        )),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

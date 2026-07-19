//! DB-backed sitemaps (issues #75 and #294).
//!
//! The sitemap index + child sitemaps advertise the public catalog to crawlers.
//! These drive the real router so the route wiring, the XML shape, the configured
//! public-site-URL `<loc>`s, and the cache policy are all covered end to end. The
//! configured origin under test is `test_state`'s `public_site_url`
//! (`https://sitemap.test`).

use chrono::Utc;
use sea_orm::{DatabaseConnection, EntityTrait, Set};

use super::harness::*;
use crate::entities::{card, product};
use crate::handlers::sitemap::{MAX_URLS_PER_SITEMAP, SITEMAP_CACHE_CONTROL};

/// Count the `<loc>` entries in a child sitemap body — i.e. how many URLs the chunk
/// advertised.
fn loc_count(body: &str) -> usize {
    body.matches("<loc>").count()
}

/// Bulk-insert `n` minimal `mtg` cards with zero-padded external ids `c00001…`, so
/// their auto-increment ids follow insertion order (chunk `k` then holds
/// `c{(k-1)*MAX+1}…`). Batched to stay under SQLite's per-statement parameter bound.
async fn seed_sequential_cards(db: &DatabaseConnection, n: usize) {
    let now = Utc::now();
    let models: Vec<card::ActiveModel> = (1..=n)
        .map(|i| card::ActiveModel {
            game: Set("mtg".to_string()),
            external_id: Set(format!("c{i:05}")),
            name: Set(format!("Card {i}")),
            set_code: Set("tst".to_string()),
            set_name: Set("Test Set".to_string()),
            collector_number: Set(i.to_string()),
            lang: Set("en".to_string()),
            digital: Set(false),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        })
        .collect();
    for batch in models.chunks(1_000) {
        card::Entity::insert_many(batch.to_vec())
            .exec(db)
            .await
            .expect("bulk insert cards");
    }
}

/// [`seed_sequential_cards`]' twin for the `products` table (external ids `p00001…`).
async fn seed_sequential_products(db: &DatabaseConnection, n: usize) {
    let now = Utc::now();
    let models: Vec<product::ActiveModel> = (1..=n)
        .map(|i| product::ActiveModel {
            game: Set("mtg".to_string()),
            external_id: Set(format!("p{i:05}")),
            name: Set(format!("Product {i}")),
            set_code: Set("tst".to_string()),
            product_type: Set("bundle".to_string()),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        })
        .collect();
    for batch in models.chunks(1_000) {
        product::Entity::insert_many(batch.to_vec())
            .exec(db)
            .await
            .expect("bulk insert products");
    }
}

#[tokio::test]
async fn sitemap_index_lists_child_sitemaps() {
    let app = test_app_with_catalog().await;
    let (status, headers, body) = send_text(&app, get("/sitemap.xml")).await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        content_type(&headers)
            .unwrap()
            .starts_with("application/xml")
    );
    // A sitemap is expensive to build and changes at most daily, so it gets the
    // longer, shared-cacheable sitemap policy (not the catalog default).
    assert_eq!(cache_control(&headers), Some(SITEMAP_CACHE_CONTROL));

    assert!(body.contains("<sitemapindex"), "not an index doc: {body}");
    // Children are referenced against the configured public site origin, at the
    // root /sitemaps/ path (not /api/).
    assert!(body.contains("<loc>https://sitemap.test/sitemaps/pages.xml</loc>"));
    assert!(body.contains("<loc>https://sitemap.test/sitemaps/sets.xml</loc>"));
    // The seeded catalog has cards and sealed products, so there is at least one
    // chunk of each.
    assert!(body.contains("<loc>https://sitemap.test/sitemaps/cards-1.xml</loc>"));
    assert!(body.contains("<loc>https://sitemap.test/sitemaps/products-1.xml</loc>"));
    assert!(
        !body.contains("/api/sitemaps/"),
        "children must live at the root: {body}"
    );
}

#[tokio::test]
async fn sitemap_api_aliases_still_answer() {
    // The pre-#294 URLs are already submitted to search consoles and pointed at by
    // deployed robots.txt files, so the /api/ paths keep serving the same documents.
    let app = test_app_with_catalog().await;

    let (status, _h, body) = send_text(&app, get("/api/sitemap.xml")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("<sitemapindex"), "not an index doc: {body}");
    // Even via the alias, children are advertised at the root path.
    assert!(body.contains("<loc>https://sitemap.test/sitemaps/pages.xml</loc>"));

    let (status, _h, body) = send_text(&app, get("/api/sitemaps/pages.xml")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("<urlset"), "not a urlset doc: {body}");
}

#[tokio::test]
async fn sitemap_pages_covers_static_and_game_routes() {
    let app = test_app_with_catalog().await;
    let (status, headers, body) = send_text(&app, get("/sitemaps/pages.xml")).await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        content_type(&headers)
            .unwrap()
            .starts_with("application/xml")
    );
    assert!(body.contains("<urlset"));
    assert!(body.contains("<loc>https://sitemap.test/</loc>"));
    assert!(body.contains("<loc>https://sitemap.test/cards</loc>"));
    assert!(body.contains("<loc>https://sitemap.test/cards/mtg</loc>"));
    assert!(body.contains("<loc>https://sitemap.test/cards/mtg/cards</loc>"));
    // The sealed hub + per-game set-tile landing, the flat sealed-product browse
    // (its new home), and the legal pages (issue #294).
    assert!(body.contains("<loc>https://sitemap.test/sealed</loc>"));
    assert!(body.contains("<loc>https://sitemap.test/sealed/mtg</loc>"));
    assert!(body.contains("<loc>https://sitemap.test/sealed/mtg/products</loc>"));
    assert!(body.contains("<loc>https://sitemap.test/docs</loc>"));
    assert!(body.contains("<loc>https://sitemap.test/terms</loc>"));
    assert!(body.contains("<loc>https://sitemap.test/privacy</loc>"));
}

#[tokio::test]
async fn sitemap_sets_lists_seeded_sets() {
    let app = test_app_with_catalog().await;

    // Discover a real seeded set code from the catalog, then assert the sitemap
    // advertises its SPA detail page.
    let (_s, _h, sets) = send(&app, get("/api/games/mtg/sets")).await;
    let code = sets["data"][0]["code"].as_str().expect("a seeded set code");

    let (status, _h, body) = send_text(&app, get("/sitemaps/sets.xml")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains(&format!(
            "<loc>https://sitemap.test/cards/mtg/sets/{code}</loc>"
        )),
        "set {code} missing from sitemap: {body}"
    );

    // A seeded sealed product's set also gets its own sealed-catalog set page (prep
    // for the `/sealed/{game}/sets/{code}` landing), carrying the matching card-set's
    // release date as its <lastmod> since the dummy catalog's product sets ("dmb" /
    // "dmu") both resolve against seeded `card_sets` rows.
    let (_s, _h, products) = send(&app, get("/api/games/mtg/products")).await;
    let product_set_code = products["data"][0]["set_code"]
        .as_str()
        .expect("a seeded product set code");
    let matching_set = sets["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["code"] == product_set_code)
        .expect("the product's set is a seeded card set");
    let released_at = matching_set["released_at"]
        .as_str()
        .expect("the seeded set has a release date");

    let expected_loc =
        format!("<loc>https://sitemap.test/sealed/mtg/sets/{product_set_code}</loc>");
    assert!(
        body.contains(&format!("{expected_loc}<lastmod>{released_at}</lastmod>")),
        "sealed set {product_set_code} missing (with lastmod {released_at}) from sitemap: {body}"
    );
}

#[tokio::test]
async fn sitemap_cards_chunk_lists_cards_and_out_of_range_is_404() {
    let app = test_app_with_catalog().await;

    let (status, headers, body) = send_text(&app, get("/sitemaps/cards-1.xml")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cache_control(&headers), Some(SITEMAP_CACHE_CONTROL));
    assert!(
        body.contains("<loc>https://sitemap.test/cards/mtg/cards/"),
        "no card URLs in chunk: {body}"
    );

    // A chunk past the end is a 404, and — like every error — is never shared-cached.
    let (status, headers, _b) = send_text(&app, get("/sitemaps/cards-9999.xml")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(cache_control(&headers), Some("no-store"));

    // An unknown child name is likewise a 404.
    let (status, _h, _b) = send_text(&app, get("/sitemaps/bogus.xml")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn sitemap_products_chunk_lists_sealed_products_and_out_of_range_is_404() {
    let app = test_app_with_catalog().await;

    let (status, headers, body) = send_text(&app, get("/sitemaps/products-1.xml")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cache_control(&headers), Some(SITEMAP_CACHE_CONTROL));
    assert!(
        body.contains("<loc>https://sitemap.test/sealed/mtg/"),
        "no sealed-product URLs in chunk: {body}"
    );

    let (status, headers, _b) = send_text(&app, get("/sitemaps/products-9999.xml")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(cache_control(&headers), Some("no-store"));
}

// The chunk tests above only reach chunk 1 (the dummy seed is far smaller than
// MAX_URLS_PER_SITEMAP), where the keyset window starts at offset 0 and matches every
// row — a degenerate case indistinguishable from the old OFFSET query. The two tests
// below seed one row past a full chunk so the *second* chunk's window starts at a
// mid-catalog primary key, exercising the keyset seek (issue #334) at the real chunk
// boundary: chunk 1 must fill exactly and not spill, chunk 2 must hold precisely the
// remainder (a `>=` seek, so its own first card is included and not dropped), and the
// two must partition the catalog with no gap or overlap.

#[tokio::test]
async fn sitemap_card_chunks_partition_the_catalog_at_the_boundary() {
    let app = test_app().await;
    let max = MAX_URLS_PER_SITEMAP as usize;
    seed_sequential_cards(&app.state.db, max + 1).await;

    // The index advertises exactly two card chunks for `max + 1` cards — no phantom
    // third chunk.
    let (_s, _h, index) = send_text(&app, get("/sitemap.xml")).await;
    assert!(index.contains("<loc>https://sitemap.test/sitemaps/cards-2.xml</loc>"));
    assert!(
        !index.contains("cards-3.xml"),
        "phantom third chunk: {index}"
    );

    // Chunk 1: exactly the first `max` cards (c00001…c{max}); it must not reach into
    // the overflow card that belongs to chunk 2.
    let boundary = format!("/cards/mtg/cards/c{max:05}</loc>");
    let overflow = format!("/cards/mtg/cards/c{:05}</loc>", max + 1);
    let (status, _h, chunk1) = send_text(&app, get("/sitemaps/cards-1.xml")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(loc_count(&chunk1), max, "chunk 1 must be a full window");
    assert!(
        chunk1.contains("/cards/mtg/cards/c00001</loc>"),
        "missing first card"
    );
    assert!(chunk1.contains(&boundary), "missing boundary card");
    assert!(!chunk1.contains(&overflow), "chunk 1 spilled into chunk 2");

    // Chunk 2: precisely the one remaining card, seeked to by its own id — the `>=`
    // window includes it (a `>` regression would drop it and 404 the empty chunk).
    let (status, _h, chunk2) = send_text(&app, get("/sitemaps/cards-2.xml")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(loc_count(&chunk2), 1, "chunk 2 holds only the remainder");
    assert!(chunk2.contains(&overflow), "chunk 2 missing its card");
    assert!(!chunk2.contains(&boundary), "chunk 2 overlaps chunk 1");

    // One past the last real chunk is still a 404.
    let (status, _h, _b) = send_text(&app, get("/sitemaps/cards-3.xml")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn sitemap_product_chunks_partition_the_catalog_at_the_boundary() {
    let app = test_app().await;
    let max = MAX_URLS_PER_SITEMAP as usize;
    seed_sequential_products(&app.state.db, max + 1).await;

    let (_s, _h, index) = send_text(&app, get("/sitemap.xml")).await;
    assert!(index.contains("<loc>https://sitemap.test/sitemaps/products-2.xml</loc>"));
    assert!(
        !index.contains("products-3.xml"),
        "phantom third chunk: {index}"
    );

    let boundary = format!("/sealed/mtg/p{max:05}</loc>");
    let overflow = format!("/sealed/mtg/p{:05}</loc>", max + 1);
    let (status, _h, chunk1) = send_text(&app, get("/sitemaps/products-1.xml")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(loc_count(&chunk1), max, "chunk 1 must be a full window");
    assert!(
        chunk1.contains("/sealed/mtg/p00001</loc>"),
        "missing first product"
    );
    assert!(chunk1.contains(&boundary), "missing boundary product");
    assert!(!chunk1.contains(&overflow), "chunk 1 spilled into chunk 2");

    let (status, _h, chunk2) = send_text(&app, get("/sitemaps/products-2.xml")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(loc_count(&chunk2), 1, "chunk 2 holds only the remainder");
    assert!(chunk2.contains(&overflow), "chunk 2 missing its product");
    assert!(!chunk2.contains(&boundary), "chunk 2 overlaps chunk 1");

    let (status, _h, _b) = send_text(&app, get("/sitemaps/products-3.xml")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
